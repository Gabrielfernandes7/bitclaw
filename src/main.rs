//! Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação,
//! interface) e abre o terminal. A lógica do agente fica na biblioteca.

mod app;

use bitclaw::agent::ChatModel;
use bitclaw::config::AgentConfig;
use bitclaw::models::{Effort, ModelCatalog, ModelInfo, SharedEffort};
use bitclaw::ollama::{self, OllamaCatalog, OllamaSettings, SharedOptions};
use bitclaw::session::Session;
use bitclaw::skills;
use bitclaw::toolkit;
use bitclaw::tools::Approver;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// Agente padrão, embutido no binário como dado. `--config` usa outro.
const DEFAULT_CONFIG: &str = include_str!("../agents/bitclaw.toml");

struct Args {
    config: Option<PathBuf>,
    model: Option<String>,
    url: Option<String>,
    dir: PathBuf,
    auto_yes: bool,
    no_color: bool,
    mouse: bool,
    think: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: None,
        model: None,
        url: None,
        dir: std::env::current_dir().map_err(|e| e.to_string())?,
        auto_yes: false,
        no_color: false,
        mouse: false,
        think: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => args.config = Some(PathBuf::from(it.next().ok_or("--config precisa de valor")?)),
            "--model" => args.model = Some(it.next().ok_or("--model precisa de valor")?),
            "--url" => args.url = Some(it.next().ok_or("--url precisa de valor")?),
            "--dir" => args.dir = PathBuf::from(it.next().ok_or("--dir precisa de valor")?),
            "--yes" => args.auto_yes = true,
            "--no-color" => args.no_color = true,
            "--mouse" => args.mouse = true,
            "--think" => args.think = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--config A.toml] [--model M] [--url U] [--dir D] [--yes] [--no-color] [--mouse] [--think]");
                std::process::exit(0);
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(args)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("erro: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let color = !args.no_color && std::env::var_os("NO_COLOR").is_none();

    let mut cfg = match &args.config {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("não consegui ler {}: {e}", path.display()))?;
            AgentConfig::from_toml(&text)?
        }
        None => AgentConfig::from_toml(DEFAULT_CONFIG)?,
    };
    if let Some(m) = args.model {
        cfg.model.name = m;
    }
    // Ferramenta desconhecida no TOML é erro já na partida, antes de falar com o Ollama.
    toolkit::registry_from_names(&cfg.tools).map_err(|e| format!("configuração: {e}"))?;
    for warning in cfg.warnings() {
        eprintln!("aviso: {warning}");
    }
    // Configuração do provedor. `--url` vale mais que o arquivo.
    let mut provider = OllamaSettings::from_config(&cfg)?;
    if let Some(u) = args.url {
        provider.url = u;
    }

    // Catálogo do provedor. Sem o Ollama o agente não faz nada útil: falha já na inicialização.
    let catalog: Arc<dyn ModelCatalog + Send + Sync> = Arc::new(OllamaCatalog::new(provider.url.clone()));
    let probe = build_client(&provider, &ModelInfo::unknown(&cfg.model.name), &Arc::new(Mutex::new(Effort::Off)), &shared_options(&cfg, &provider))?;
    match probe.has_model() {
        Ok(true) => {}
        Ok(false) => eprintln!("aviso: o modelo '{0}' não aparece no Ollama. Instale com: ollama pull {0}", cfg.model.name),
        Err(e) => return Err(format!("{e}. Inicie o Ollama com: ollama serve")),
    }

    // Modelo padrão = o da configuração. O ativo começa nele e pode ser trocado com `/model`.
    let default_info = catalog
        .find(&cfg.model.name)
        .ok()
        .flatten()
        .unwrap_or_else(|| ModelInfo::unknown(&cfg.model.name));
    // Raciocínio começa desligado, salvo `think = true` na configuração ou `--think`.
    let start_effort = if cfg.model.think || args.think { default_info.capabilities.default_on() } else { Effort::Off };
    let effort: SharedEffort = Arc::new(Mutex::new(start_effort));
    let active = Arc::new(Mutex::new(default_info.clone()));
    // Contexto e temperatura: começam na configuração e mudam com `/context` e `/temp`.
    let options = shared_options(&cfg, &provider);

    let root = args.dir.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
    // As skills são procuradas em relação ao diretório de execução, não ao `--dir`.
    let skills_path = match cfg.skills_dir.as_deref() {
        Some(d) => Some(std::env::current_dir().map_err(|e| e.to_string())?.join(d)),
        None => None,
    };
    let (project_skills, ignored) = match &skills_path {
        Some(p) => skills::load_with_ignored(p)?,
        None => (Vec::new(), Vec::new()),
    };
    let skills = skills::with_builtins(project_skills);
    let mut info = vec![format!("projeto {}", root.display()), model_line(&cfg, &provider, &default_info)];
    // Os nomes das skills não vão para a tela da TUI (o painel `/skills` os mostra). O modo simples os lista.
    let mut skill_info = Vec::new();
    match &skills_path {
        Some(p) if skills.is_empty() && !p.is_dir() => {
            info.push(format!("skills: pasta {} não encontrada", p.display()));
        }
        _ if !skills.is_empty() => {
            let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
            skill_info.push(format!("skills: {}", names.join(", ")));
        }
        _ => {}
    }
    if !ignored.is_empty() {
        info.push(format!("ignoradas: {}", ignored.join(", ")));
    }
    let skill_lines = skills::summary(&skills);
    // Skills desligadas no painel `/skills`: a interface e a sessão do agente compartilham este conjunto.
    let skills_off = skills::new_disabled();
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();

    // Fábrica de modelos para a troca em tempo de execução: cada modelo é um cliente novo, com o mesmo
    // esforço compartilhado e o histórico da sessão preservado.
    let switch_provider = provider.clone();
    let switch_effort = Arc::clone(&effort);
    let switch_options = Arc::clone(&options);
    let make_model = move |model: &ModelInfo| -> Result<Box<dyn ChatModel>, String> {
        Ok(Box::new(build_client(&switch_provider, model, &switch_effort, &switch_options)?))
    };

    if interactive {
        let settings = app::tui::Settings {
            title: cfg.name.clone(),
            info,
            skills: skills.clone(),
            skills_off: Arc::clone(&skills_off),
            model: Arc::clone(&active),
            default_model: cfg.model.name.clone(),
            effort: Arc::clone(&effort),
            options: Arc::clone(&options),
            default_options: provider.options(cfg.model.context_window),
            catalog: Arc::clone(&catalog),
            auto_yes: args.auto_yes,
            color,
            mouse: args.mouse,
            root: root.clone(),
        };
        // A sessão é montada na thread do agente, que recebe o aprovador pronto.
        let session_effort = Arc::clone(&effort);
        let session_options = Arc::clone(&options);
        app::tui::run(
            settings,
            move |approver| {
                let client = build_client(&provider, &default_info, &session_effort, &session_options)?;
                Ok(build_session(&cfg, client, approver, &root)?.with_skills(skills, skills_off))
            },
            Box::new(make_model),
        )
    } else {
        let approver: Box<dyn Approver> = Box::new(app::plain::StdinApprover { auto_edits: args.auto_yes });
        let client = build_client(&provider, &default_info, &effort, &options)?;
        let mut session = build_session(&cfg, client, approver, &root)?.with_skills(skills, skills::new_disabled());
        let mut header = vec![cfg.name.clone()];
        header.extend(info);
        header.extend(skill_info);
        app::plain::run(&mut session, &header, args.auto_yes, &active, &effort, &skill_lines)
    }
}

// Cliente para um modelo. A janela e a temperatura vêm da configuração; o modelo e suas capacidades, do catálogo.
// Monta a sessão: as ferramentas vêm do toolkit, pelos nomes do TOML.
fn build_session(cfg: &AgentConfig, client: ollama::Client, approver: Box<dyn Approver>, root: &std::path::Path) -> Result<Session, String> {
    let tools = toolkit::registry_from_names(&cfg.tools).map_err(|e| format!("configuração: {e}"))?;
    Session::builder(cfg).model(Box::new(client)).approver(approver).tools(tools).hook(Box::new(toolkit::claims::FileClaims)).root(root).build()
}

fn build_client(
    provider: &OllamaSettings,
    info: &ModelInfo,
    effort: &SharedEffort,
    options: &SharedOptions,
) -> Result<ollama::Client, String> {
    ollama::Client::new(
        provider.url.clone(),
        info.id.clone(),
        Arc::clone(options),
        info.capabilities.clone(),
        Arc::clone(effort),
    )
}

// Opções iniciais, as da configuração, num estado que a interface e o cliente compartilham.
fn shared_options(cfg: &AgentConfig, provider: &OllamaSettings) -> SharedOptions {
    Arc::new(Mutex::new(provider.options(cfg.model.context_window)))
}

// Linha com modelo, tamanho e janela, quando o Ollama responde.
fn model_line(cfg: &AgentConfig, provider: &OllamaSettings, info: &ModelInfo) -> String {
    let effort = Arc::new(Mutex::new(Effort::Off));
    match build_client(provider, info, &effort, &shared_options(cfg, provider)).and_then(|c| c.model_details()) {
        Ok(d) => format!(
            "modelo {} · {} · {} · janela {} tok",
            cfg.model.name,
            d.parameter_size.unwrap_or_else(|| "?".into()),
            d.quantization.unwrap_or_else(|| "?".into()),
            cfg.model.context_window,
        ),
        Err(_) => format!("modelo {} · janela {} tok", cfg.model.name, cfg.model.context_window),
    }
}

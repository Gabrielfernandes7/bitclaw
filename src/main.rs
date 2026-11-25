//! Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação,
//! interface) e abre o terminal. A lógica do agente fica na biblioteca.

mod tui;

use bitclaw::agent::ChatModel;
use bitclaw::config::AgentConfig;
use bitclaw::models::{Effort, ModelCatalog, ModelInfo, SharedEffort};
use bitclaw::ollama::{self, Options, OllamaCatalog};
use bitclaw::session::Session;
use bitclaw::skills;
use bitclaw::tools::{Approver, AutoApprover};
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
    no_mouse: bool,
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
        no_mouse: false,
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
            "--no-mouse" => args.no_mouse = true,
            "--think" => args.think = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--config A.toml] [--model M] [--url U] [--dir D] [--yes] [--no-color] [--no-mouse] [--think]");
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
    if let Some(u) = args.url {
        cfg.model.url = u;
    }

    // Catálogo do provedor. Sem o Ollama o agente não faz nada útil: falha já na inicialização.
    let catalog: Arc<dyn ModelCatalog + Send + Sync> = Arc::new(OllamaCatalog::new(cfg.model.url.clone()));
    let probe = build_client(&cfg, &ModelInfo::unknown(&cfg.model.name), &Arc::new(Mutex::new(Effort::Off)))?;
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
    let mut info = vec![format!("projeto {}", root.display()), model_line(&cfg, &default_info)];
    match &skills_path {
        Some(p) if skills.is_empty() && !p.is_dir() => {
            info.push(format!("skills: pasta {} não encontrada", p.display()));
        }
        _ if !skills.is_empty() => {
            let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
            info.push(format!("skills: {}", names.join(", ")));
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
    let switch_cfg = cfg.clone();
    let switch_effort = Arc::clone(&effort);
    let make_model = move |model: &ModelInfo| -> Result<Box<dyn ChatModel>, String> {
        Ok(Box::new(build_client(&switch_cfg, model, &switch_effort)?))
    };

    if interactive {
        let settings = tui::Settings {
            title: cfg.name.clone(),
            info,
            skills: skills.clone(),
            skills_off: Arc::clone(&skills_off),
            model: Arc::clone(&active),
            default_model: cfg.model.name.clone(),
            effort: Arc::clone(&effort),
            catalog: Arc::clone(&catalog),
            auto_yes: args.auto_yes,
            color,
            mouse: !args.no_mouse,
        };
        // A sessão é montada na thread do agente, que recebe o aprovador pronto.
        let session_effort = Arc::clone(&effort);
        tui::run(
            settings,
            move |approver| {
                let client = build_client(&cfg, &default_info, &session_effort)?;
                Ok(Session::from_config(&cfg, Box::new(client), approver, &root)?.with_skills(skills, skills_off))
            },
            Box::new(make_model),
        )
    } else {
        let approver: Box<dyn Approver> =
            if args.auto_yes { Box::new(AutoApprover) } else { Box::new(tui::plain::StdinApprover) };
        let client = build_client(&cfg, &default_info, &effort)?;
        let mut session = Session::from_config(&cfg, Box::new(client), approver, &root)?.with_skills(skills, skills::new_disabled());
        let mut header = vec![cfg.name.clone()];
        header.extend(info);
        tui::plain::run(&mut session, &header, args.auto_yes, &active, &effort, &skill_lines)
    }
}

// Cliente para um modelo. A janela e a temperatura vêm da configuração; o modelo e suas capacidades, do catálogo.
fn build_client(cfg: &AgentConfig, info: &ModelInfo, effort: &SharedEffort) -> Result<ollama::Client, String> {
    let options = Options { num_ctx: cfg.model.num_ctx, temperature: cfg.model.temperature };
    ollama::Client::new(
        cfg.model.url.clone(),
        info.id.clone(),
        options,
        info.capabilities.clone(),
        Arc::clone(effort),
    )
}

// Linha com modelo, tamanho e janela, quando o Ollama responde.
fn model_line(cfg: &AgentConfig, info: &ModelInfo) -> String {
    let effort = Arc::new(Mutex::new(Effort::Off));
    match build_client(cfg, info, &effort).and_then(|c| c.model_details()) {
        Ok(d) => format!(
            "modelo {} · {} · {} · janela {} tok",
            cfg.model.name,
            d.parameter_size.unwrap_or_else(|| "?".into()),
            d.quantization.unwrap_or_else(|| "?".into()),
            cfg.model.num_ctx,
        ),
        Err(_) => format!("modelo {} · janela {} tok", cfg.model.name, cfg.model.num_ctx),
    }
}

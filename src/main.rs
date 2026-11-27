//! Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação,
//! interface) e abre o terminal. A lógica do agente fica na biblioteca.

mod app;

use bitclaw::agent::ChatModel;
use bitclaw::config::AgentConfig;
use app::provider::Provider;
use bitclaw::models::{Effort, ModelInfo, SharedEffort};
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
    think: bool,
    no_mouse: bool,
    plan: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: None,
        model: None,
        url: None,
        dir: std::env::current_dir().map_err(|e| e.to_string())?,
        auto_yes: false,
        no_color: false,
        think: false,
        no_mouse: false,
        plan: false,
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
            // O mouse é capturado por padrão: a roda rola a conversa e arrastar seleciona e copia.
            // `--no-mouse` devolve o mouse ao terminal (seleção nativa; a conversa rola com PageUp/PageDown).
            "--no-mouse" => args.no_mouse = true,
            "--mouse" => args.no_mouse = false,
            "--think" => args.think = true,
            // Modo Plan: só ferramentas que declaram somente leitura. Nada é alterado nem executado.
            "--plan" => args.plan = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--config A.toml] [--model M] [--url U] [--dir D] [--yes] [--no-color] [--no-mouse] [--think] [--plan]");
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
    // O provedor (Ollama, OpenRouter...) vem de `[model] provider`; `--url` vale mais que o arquivo.
    let provider = Provider::connect(&cfg, args.url)?;
    let catalog = Arc::clone(&provider.catalog);

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
    let options = Arc::clone(&provider.options);

    let root = args.dir.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
    // As skills são procuradas em relação ao diretório de execução, não ao `--dir`.
    let skills_path = match cfg.skills_dir.as_deref() {
        Some(d) => Some(std::env::current_dir().map_err(|e| e.to_string())?.join(d)),
        None => None,
    };
    let (mut project_skills, mut ignored) = match &skills_path {
        Some(p) => skills::load_with_ignored(p)?,
        None => (Vec::new(), Vec::new()),
    };
    // Skills baixadas com `/skill install` ficam em `.bitclaw/skills`, dentro do projeto. A do projeto
    // prevalece se o nome se repete.
    let (installed, installed_ignored) = app::skill_install::load_installed(&root)?;
    ignored.extend(installed_ignored);
    for skill in installed {
        if project_skills.iter().any(|s| s.name == skill.name) {
            ignored.push(format!("{} (baixada, mas o projeto já tem uma skill com esse nome)", skill.name));
        } else {
            project_skills.push(skill);
        }
    }
    let skills = skills::with_builtins(project_skills);
    let provider_label = provider.label().to_string();
    let model_details = provider.model_details(&default_info);
    let mut info = vec![
        app::header::project_line(&root),
        app::header::model_line(&cfg.model.name, start_effort, &provider_label, &model_details, cfg.model.context_window),
    ];
    info.extend(provider.notices.iter().cloned());
    if args.plan {
        info.push("modo plano: só leitura, sem edição, criação, remoção nem scripts".into());
    }
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
    let make_model = move |model: &ModelInfo| -> Result<Box<dyn ChatModel>, String> {
        switch_provider.client(model, &switch_effort)
    };

    if interactive {
        let settings = app::tui::Settings {
            title: cfg.name.clone(),
            info,
            provider: provider_label,
            details: model_details,
            skills: skills.clone(),
            skills_off: Arc::clone(&skills_off),
            model: Arc::clone(&active),
            default_model: cfg.model.name.clone(),
            effort: Arc::clone(&effort),
            options: Arc::clone(&options),
            default_options: provider.default_options,
            catalog: Arc::clone(&catalog),
            auto_yes: args.auto_yes,
            color,
            mouse: !args.no_mouse,
            root: root.clone(),
        };
        // A sessão é montada na thread do agente, que recebe o aprovador pronto.
        let session_effort = Arc::clone(&effort);
        app::tui::run(
            settings,
            move |approver| {
                let client = provider.client(&default_info, &session_effort)?;
                Ok(build_session(&cfg, client, approver, &root, args.plan, args.auto_yes)?.with_skills(skills, skills_off))
            },
            Box::new(make_model),
        )
    } else {
        let approver: Box<dyn Approver> = Box::new(app::plain::StdinApprover { auto_edits: args.auto_yes });
        let client = provider.client(&default_info, &effort)?;
        let mut session = build_session(&cfg, client, approver, &root, args.plan, args.auto_yes)?.with_skills(skills, skills::new_disabled());
        let mut header = vec![cfg.name.clone()];
        header.extend(info);
        header.extend(skill_info);
        app::plain::run(&mut session, &header, args.auto_yes, &active, &effort, &skill_lines)
    }
}

// Monta a sessão: as ferramentas vêm do toolkit, pelos nomes do TOML.
fn build_session(cfg: &AgentConfig, client: Box<dyn ChatModel>, approver: Box<dyn Approver>, root: &std::path::Path, plan: bool, auto_yes: bool) -> Result<Session, String> {
    let tools = toolkit::registry_from_names(&cfg.tools).map_err(|e| format!("configuração: {e}"))?;
    let tools = if plan { tools.read_only() } else { tools };
    // Cada decisão sobre alteração ou script fica registrada em `.bitclaw/approvals.jsonl`.
    let approver: Box<dyn Approver> = Box::new(app::receipts::Recorded::new(approver, root, auto_yes));
    let builder = Session::builder(cfg).model(client).approver(approver).tools(tools).hook(Box::new(toolkit::claims::FileClaims)).retriever(Box::new(toolkit::fs::SymbolIndex::default())).root(root);
    let builder = if cfg.limits.verify { builder.verifier(Box::new(toolkit::verify::ProjectCheck)) } else { builder };
    builder.build()
}

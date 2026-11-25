//! Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação,
//! interface) e abre o terminal. A lógica do agente fica na biblioteca.

mod brand;
mod markdown;
mod plain;
mod tui;

use bitclaw::config::AgentConfig;
use bitclaw::skills;
use bitclaw::ollama::{self, Options};
use bitclaw::session::Session;
use bitclaw::tools::{Approver, AutoApprover};
use std::io::{self, IsTerminal};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::path::PathBuf;

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
            "--think" => args.think = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--config A.toml] [--model M] [--url U] [--dir D] [--yes] [--no-color] [--think]");
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
    // O raciocínio pode ser ligado e desligado durante a sessão (/think); este indicador é compartilhado.
    let think = Arc::new(AtomicBool::new(cfg.model.think || args.think));

    // Sem o Ollama o agente não faz nada útil: falha já na inicialização, com a instrução de correção.
    match build_client(&cfg, &think)?.has_model() {
        Ok(true) => {}
        Ok(false) => eprintln!("aviso: o modelo '{0}' não aparece no Ollama. Instale com: ollama pull {0}", cfg.model.name),
        Err(e) => return Err(format!("{e}. Inicie o Ollama com: ollama serve")),
    }

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
    let mut info = vec![format!("projeto {}", root.display()), model_line(&cfg, &think)];
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

    if interactive {
        let settings = tui::Settings {
            title: cfg.name.clone(),
            info,
            skills: skills.clone(),
            skills_off: Arc::clone(&skills_off),
            model: cfg.model.name.clone(),
            auto_yes: args.auto_yes,
            color,
            think: Arc::clone(&think),
        };
        // A sessão é montada na thread do agente, que recebe o aprovador pronto.
        tui::run(settings, move |approver| {
            let client = build_client(&cfg, &think)?;
            Ok(Session::from_config(&cfg, Box::new(client), approver, &root)?.with_skills(skills, skills_off))
        })
    } else {
        let approver: Box<dyn Approver> =
            if args.auto_yes { Box::new(AutoApprover) } else { Box::new(plain::StdinApprover) };
        let mut session = Session::from_config(&cfg, Box::new(build_client(&cfg, &think)?), approver, &root)?.with_skills(skills, skills::new_disabled());
        let mut header = vec![cfg.name.clone()];
        header.extend(info);
        plain::run(&mut session, &header, args.auto_yes, &think, &skill_lines)
    }
}

fn build_client(cfg: &AgentConfig, think: &Arc<AtomicBool>) -> Result<ollama::Client, String> {
    let options = Options { num_ctx: cfg.model.num_ctx, temperature: cfg.model.temperature, think: cfg.model.think };
    Ok(ollama::Client::new(cfg.model.url.clone(), cfg.model.name.clone(), options)?.with_think(Arc::clone(think)))
}

// Linha com modelo, tamanho e janela, quando o Ollama responde.
fn model_line(cfg: &AgentConfig, think: &Arc<AtomicBool>) -> String {
    match build_client(cfg, think).and_then(|c| c.model_info()) {
        Ok(info) => format!(
            "modelo {} · {} · {} · janela {} tok",
            cfg.model.name,
            info.parameter_size.unwrap_or_else(|| "?".into()),
            info.quantization.unwrap_or_else(|| "?".into()),
            cfg.model.num_ctx,
        ),
        Err(_) => format!("modelo {} · janela {} tok", cfg.model.name, cfg.model.num_ctx),
    }
}

//! Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação,
//! interface) e abre o terminal. A lógica do agente fica na biblioteca.

mod brand;
mod markdown;
mod plain;
mod tui;

use bitclaw::config::AgentConfig;
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
        Ok(false) => eprintln!("⚠ o modelo '{0}' não aparece no Ollama. Instale com: ollama pull {0}", cfg.model.name),
        Err(e) => return Err(format!("{e}. Inicie o Ollama com: ollama serve")),
    }

    let root = args.dir.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
    let info = vec![format!("projeto {}", root.display()), model_line(&cfg, &think)];
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();

    if interactive {
        let settings = tui::Settings {
            title: cfg.name.clone(),
            info,
            model: cfg.model.name.clone(),
            auto_yes: args.auto_yes,
            color,
            think: Arc::clone(&think),
        };
        // A sessão é montada na thread do agente, que recebe o aprovador pronto.
        tui::run(settings, move |approver| {
            let client = build_client(&cfg, &think)?;
            Session::from_config(&cfg, Box::new(client), approver, &root)
        })
    } else {
        let approver: Box<dyn Approver> =
            if args.auto_yes { Box::new(AutoApprover) } else { Box::new(plain::StdinApprover) };
        let mut session = Session::from_config(&cfg, Box::new(build_client(&cfg, &think)?), approver, &root)?;
        let mut header = vec![cfg.name.clone()];
        header.extend(info);
        plain::run(&mut session, &header, args.auto_yes, &think)
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

//! Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação,
//! interface) e abre o terminal. A lógica do agente fica na biblioteca.

mod ui;

use bitclaw::config::AgentConfig;
use bitclaw::ollama::{self, Options};
use bitclaw::session::Session;
use bitclaw::tools::{Approver, AutoApprover};
use std::io::{self, BufRead, Write};
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
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: None,
        model: None,
        url: None,
        dir: std::env::current_dir().map_err(|e| e.to_string())?,
        auto_yes: false,
        no_color: false,
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
            "-h" | "--help" => {
                println!("uso: bitclaw [--config A.toml] [--model M] [--url U] [--dir D] [--yes] [--no-color]");
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
    ui::init(args.no_color);

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

    // Sem o Ollama o agente não faz nada útil: falha já na inicialização, com a instrução de correção.
    match build_client(&cfg)?.has_model() {
        Ok(true) => {}
        Ok(false) => eprintln!(
            "{}",
            ui::yellow(&format!("⚠ o modelo '{0}' não aparece no Ollama. Instale com: ollama pull {0}", cfg.model.name))
        ),
        Err(e) => return Err(format!("{e}. Inicie o Ollama com: ollama serve")),
    }

    let client = build_client(&cfg)?;
    let approver: Box<dyn Approver> = if args.auto_yes { Box::new(AutoApprover) } else { Box::new(ui::StdinApprover) };
    let mut session = Session::from_config(&cfg, Box::new(client), approver, &args.dir)?;
    let feedback = ui::Feedback::new();

    let info = model_lines(&cfg);
    println!("{}", ui::header(&cfg.name, &[format!("projeto {}", session.root().display()), info]));
    if args.auto_yes {
        println!("{}", ui::yellow("⚠ --yes ativo: edições serão aplicadas sem confirmação"));
    }
    println!("{}\n", ui::gray("Digite uma tarefa. /limpar-contexto apaga o contexto. /sair encerra."));

    let stdin = io::stdin();
    loop {
        print!("{}", ui::prompt(&cfg.name, &session.usage()));
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break; // EOF
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        match input {
            "/sair" => break,
            "/limpar-contexto" => {
                session.clear();
                println!("{}\n", ui::gray("contexto limpo; o prompt do sistema foi mantido."));
                continue;
            }
            _ => {}
        }
        match session.submit(input, &feedback) {
            Ok(report) => ui::report(&report),
            Err(e) => eprintln!("{}", ui::red(&format!("erro: {e}"))),
        }
    }
    Ok(())
}

fn build_client(cfg: &AgentConfig) -> Result<ollama::Client, String> {
    let options = Options { num_ctx: cfg.model.num_ctx, temperature: cfg.model.temperature };
    ollama::Client::new(cfg.model.url.clone(), cfg.model.name.clone(), options)
}

// Linha com modelo, tamanho e janela, quando o Ollama responde.
fn model_lines(cfg: &AgentConfig) -> String {
    match build_client(cfg).and_then(|c| c.model_info()) {
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

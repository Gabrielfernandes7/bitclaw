//! Binário do BitClaw: monta as peças (configuração, adaptador do Ollama, aprovação) e abre o terminal.
//! A lógica do agente fica na biblioteca.

use bitclaw::agent::{Progress, TurnReport};
use bitclaw::config::AgentConfig;
use bitclaw::ollama::{self, Options};
use bitclaw::session::Session;
use bitclaw::tools::{self, Approver, AutoApprover, StdinApprover};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// Agente padrão, embutido no binário como dado. `--config` usa outro arquivo.
const DEFAULT_CONFIG: &str = include_str!("../agents/bitclaw.toml");

struct Args {
    config: Option<PathBuf>,
    model: Option<String>,
    url: Option<String>,
    dir: PathBuf,
    auto_yes: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        config: None,
        model: None,
        url: None,
        dir: std::env::current_dir().map_err(|e| e.to_string())?,
        auto_yes: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => args.config = Some(PathBuf::from(it.next().ok_or("--config precisa de valor")?)),
            "--model" => args.model = Some(it.next().ok_or("--model precisa de valor")?),
            "--url" => args.url = Some(it.next().ok_or("--url precisa de valor")?),
            "--dir" => args.dir = PathBuf::from(it.next().ok_or("--dir precisa de valor")?),
            "--yes" => args.auto_yes = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--config A.toml] [--model M] [--url U] [--dir D] [--yes]");
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
    let mut cfg = match &args.config {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("não consegui ler {}: {e}", path.display()))?;
            AgentConfig::from_toml(&text)?
        }
        None => AgentConfig::from_toml(DEFAULT_CONFIG)?,
    };
    // Linha de comando sobrescreve a configuração.
    if let Some(m) = args.model {
        cfg.model.name = m;
    }
    if let Some(u) = args.url {
        cfg.model.url = u;
    }

    let options = Options { num_ctx: cfg.model.num_ctx, temperature: cfg.model.temperature };
    let client = ollama::Client::new(cfg.model.url.clone(), cfg.model.name.clone(), options)?;
    let approver: Box<dyn Approver> = if args.auto_yes { Box::new(AutoApprover) } else { Box::new(StdinApprover) };
    let mut session = Session::from_config(&cfg, Box::new(client), approver, &args.dir)?;
    let spinner = Spinner::new();

    println!("{} · projeto {}", cfg.name, session.root().display());
    print_model_info(&cfg);
    match ollama_has_model(&cfg) {
        Ok(true) => {}
        Ok(false) => eprintln!(
            "aviso: o modelo '{0}' não aparece no Ollama. Instale com: ollama pull {0}",
            cfg.model.name
        ),
        Err(e) => eprintln!("aviso: não consegui listar os modelos do Ollama: {e}"),
    }
    if args.auto_yes {
        println!("aviso: --yes ativo, edições serão aplicadas sem confirmação");
    }
    println!("Digite uma tarefa. /limpar-contexto apaga o contexto. /sair encerra.\n");

    let stdin = io::stdin();
    loop {
        let usage = session.usage();
        print!(
            "{} [contexto {:.0}% · ~{}/{} tok]> ",
            cfg.name, usage.percent, usage.tokens, usage.window
        );
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break; // EOF
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/sair" {
            break;
        }
        if input == "/limpar-contexto" {
            session.clear();
            println!("contexto limpo; o prompt do sistema foi mantido.\n");
            continue;
        }
        match session.submit(input, &spinner) {
            Ok(report) => print_report(&report),
            Err(e) => eprintln!("erro: {e}"),
        }
    }
    Ok(())
}

// Verifica se o modelo configurado está instalado no Ollama.
fn ollama_has_model(cfg: &AgentConfig) -> Result<bool, String> {
    let options = Options { num_ctx: cfg.model.num_ctx, temperature: cfg.model.temperature };
    ollama::Client::new(cfg.model.url.clone(), cfg.model.name.clone(), options)?.has_model()
}

fn print_model_info(cfg: &AgentConfig) {
    let options = Options { num_ctx: cfg.model.num_ctx, temperature: cfg.model.temperature };
    match ollama::Client::new(cfg.model.url.clone(), cfg.model.name.clone(), options).and_then(|c| c.model_info()) {
        Ok(info) => println!(
            "modelo {} · {} · quantização {} · janela máxima {} tokens",
            cfg.model.name,
            info.parameter_size.unwrap_or_else(|| "?".into()),
            info.quantization.unwrap_or_else(|| "?".into()),
            info.context_length.map_or("?".into(), |c| c.to_string()),
        ),
        Err(_) => println!("modelo {}", cfg.model.name),
    }
    println!("janela em uso: {} tokens", cfg.model.num_ctx);
}

// Mostra "modelo pensando" no terminal enquanto o modelo responde.
struct Spinner {
    slot: Mutex<Option<(Arc<AtomicBool>, JoinHandle<()>)>>,
}

impl Spinner {
    fn new() -> Self {
        Self { slot: Mutex::new(None) }
    }
}

impl Progress for Spinner {
    fn model_waiting(&self, waiting: bool) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if waiting {
            if slot.is_some() {
                return;
            }
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);
            let started = Instant::now();
            let handle = thread::spawn(move || {
                const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                let mut i = 0;
                while !flag.load(Ordering::Relaxed) {
                    eprint!(
                        "\r\x1b[2m{} modelo pensando… {}s\x1b[0m",
                        FRAMES[i % FRAMES.len()],
                        started.elapsed().as_secs()
                    );
                    io::stderr().flush().ok();
                    i += 1;
                    thread::sleep(Duration::from_millis(100));
                }
                eprint!("\r\x1b[K");
                io::stderr().flush().ok();
            });
            *slot = Some((stop, handle));
        } else if let Some((stop, handle)) = slot.take() {
            stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
        }
    }
}

// Mostra o texto do modelo e, em seguida, o que o programa de fato executou.
// As linhas de aviso vêm do programa, não do modelo.
fn print_report(report: &TurnReport) {
    for t in &report.trims {
        println!(
            "[contexto] descartados {} turno(s) antigo(s); caracteres {} → {} (orçamento {})",
            t.turns, t.chars_before, t.chars_after, t.budget
        );
    }
    if report.text.is_empty() {
        println!("\n(sem resposta)\n");
    } else {
        println!("\n{}\n", report.text);
    }

    if !report.actions.is_empty() {
        println!("ações:");
        for a in &report.actions {
            let target = a.args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            println!("  [{}] {} {}", a.outcome.label(), a.name, target);
            if !matches!(a.outcome, tools::Outcome::Done(_)) {
                println!("      {}", a.outcome.text());
            }
        }
    }

    if let Some(e) = &report.error {
        println!("interrompido: {e}");
    }
    for w in report.warnings() {
        println!("⚠ {w}");
    }
    println!();
}

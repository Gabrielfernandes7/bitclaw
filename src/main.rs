mod agent;
mod context;
mod ollama;
mod sandbox;
mod tools;

use agent::TurnReport;
use context::ContextManager;
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const DEFAULT_MODEL: &str = "nemotron-3-nano:4b-mlx";
const DEFAULT_URL: &str = "http://localhost:11434";
// Tokens reservados para a resposta do modelo, fora do orçamento do histórico.
const RESERVE_OUTPUT_TOKENS: usize = 1024;

const SYSTEM_PROMPT: &str = "Você é o BitClaw, um assistente que lê e altera arquivos de um projeto local. \
Use as ferramentas para inspecionar o projeto antes de responder. \
Para criar um arquivo novo, use create_file. Para alterar um existente, leia-o com read_file e use edit_file com o trecho exato. \
Nunca diga que criou, alterou ou executou algo sem que a ferramenta tenha retornado sucesso. \
Se uma ferramenta retornar ERRO, corrija a chamada ou explique a falha. \
Faça uma alteração por vez. Responda em português, de forma curta.";

struct Args {
    model: String,
    url: String,
    dir: PathBuf,
    auto_yes: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        model: DEFAULT_MODEL.into(),
        url: DEFAULT_URL.into(),
        dir: std::env::current_dir().map_err(|e| e.to_string())?,
        auto_yes: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--model" => args.model = it.next().ok_or("--model precisa de valor")?,
            "--url" => args.url = it.next().ok_or("--url precisa de valor")?,
            "--dir" => args.dir = PathBuf::from(it.next().ok_or("--dir precisa de valor")?),
            "--yes" => args.auto_yes = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--model M] [--url U] [--dir D] [--yes]");
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
    let root = args.dir.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
    let client = ollama::Client::new(args.url.clone(), args.model.clone())?;
    let registry = tools::Registry::default_set();
    let approver: Box<dyn tools::Approver> = if args.auto_yes {
        Box::new(tools::AutoApprover)
    } else {
        Box::new(tools::StdinApprover)
    };

    println!("BitClaw · modelo {} · projeto {}", args.model, root.display());
    match client.has_model() {
        Ok(true) => {}
        Ok(false) => eprintln!(
            "aviso: o modelo '{}' não aparece no Ollama. Instale com: ollama pull {}",
            args.model, args.model
        ),
        Err(e) => eprintln!("aviso: não consegui listar os modelos do Ollama: {e}"),
    }
    if args.auto_yes {
        println!("aviso: --yes ativo, edições serão aplicadas sem confirmação");
    }
    println!("Digite uma tarefa. /sair encerra.\n");

    let mut messages = vec![json!({"role": "system", "content": SYSTEM_PROMPT})];
    let mut ctx = ContextManager::new(ollama::NUM_CTX as usize, RESERVE_OUTPUT_TOKENS);
    let stdin = io::stdin();
    loop {
        print!("bitclaw> ");
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
        messages.push(json!({"role": "user", "content": input}));

        match agent::run_turn(&client, &registry, &root, &mut messages, approver.as_ref(), &mut ctx) {
            Ok(report) => print_report(&report),
            Err(e) => eprintln!("erro: {e}"),
        }
    }
    Ok(())
}

/// Mostra o texto do modelo e, em seguida, o que o programa de fato executou.
/// A linha final vem do programa, não do modelo.
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
    if report.attempted_changes() && !report.changed() {
        println!("nenhuma alteração foi aplicada neste turno.");
    }
    println!();
}

//! Modo simples: quando a entrada ou a saída não é um terminal (pipes, scripts), o BitClaw lê
//! uma tarefa por linha e escreve texto puro, sem cores nem interface interativa.

use crate::brand;
use bitclaw::agent::{Progress, TurnReport};
use bitclaw::session::Session;
use bitclaw::tools::{Approver, Outcome, Preview, PreviewLine};
use serde_json::Value;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// Pergunta no terminal e lê a resposta da entrada padrão.
pub struct StdinApprover;

impl Approver for StdinApprover {
    fn approve(&self, preview: &Preview) -> bool {
        println!("\n── {} ──", preview.title);
        for line in &preview.lines {
            match line {
                PreviewLine::Context(t) => println!("  {t}"),
                PreviewLine::Removed(t) => println!("- {t}"),
                PreviewLine::Added(t) => println!("+ {t}"),
                PreviewLine::Note(t) => println!("{t}"),
            }
        }
        print!("Aplicar? [s/N] ");
        io::stdout().flush().ok();
        let mut answer = String::new();
        if io::stdin().lock().read_line(&mut answer).is_err() {
            return false;
        }
        matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
    }
}

// Mostra cada ferramenta chamada, em texto puro.
struct Lines;

impl Progress for Lines {
    fn model_waiting(&self, _waiting: bool) {}

    fn tool_call(&self, name: &str, args: &Value) {
        let target = args.get("path").and_then(Value::as_str).unwrap_or("");
        println!("  · {name} {target}");
    }
}

pub fn run(session: &mut Session, header: &[String], auto_yes: bool, think: &Arc<AtomicBool>) -> Result<(), String> {
    println!("{}", brand::plain_header(header));
    if auto_yes {
        println!("aviso: --yes ativo: edições serão aplicadas sem confirmação");
    }
    if think.load(Ordering::Relaxed) {
        println!("aviso: {}", brand::THINK_WARNING);
    }
    let interactive = io::stdin().is_terminal();
    let stdin = io::stdin();
    loop {
        if interactive {
            print!("› ");
            io::stdout().flush().ok();
        }
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let input = line.trim();
        match input {
            "" => continue,
            "/exit" | "/quit" | "/sair" => break,
            "/think" => {
                let on = !think.fetch_xor(true, Ordering::Relaxed);
                if on {
                    println!("aviso: {}\n", brand::THINK_WARNING);
                } else {
                    println!("thinking desligado.\n");
                }
                continue;
            }
            "/clear" | "/limpar-contexto" => {
                session.clear();
                println!("contexto limpo; o prompt do sistema foi mantido.\n");
                continue;
            }
            _ => {}
        }
        match session.submit(input, &Lines) {
            Ok(report) => print_report(&report),
            Err(e) => eprintln!("erro: {e}"),
        }
    }
    Ok(())
}

fn print_report(report: &TurnReport) {
    for t in &report.trims {
        println!(
            "[contexto] descartados {} turno(s) antigo(s); caracteres {} → {} (orçamento {})",
            t.turns, t.chars_before, t.chars_after, t.budget
        );
    }
    for a in &report.actions {
        let target = a.args.get("path").and_then(Value::as_str).unwrap_or("");
        let icon = match a.outcome {
            Outcome::Done(_) => "✓",
            Outcome::Refused(_) => "⊘",
            Outcome::Error(_) => "✗",
        };
        println!("  {icon} {} {target}", a.name);
        if !matches!(a.outcome, Outcome::Done(_)) {
            println!("      {}", a.outcome.text());
        }
    }
    if report.text.is_empty() {
        println!("\n(sem resposta)");
    } else {
        println!("\n{}", report.text);
    }
    if let Some(e) = &report.error {
        println!("interrompido: {e}");
    }
    for w in report.warnings() {
        println!("aviso: {w}");
    }
    println!();
}

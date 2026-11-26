//! Modo simples: quando a entrada ou a saída não é um terminal (pipes, scripts), o BitClaw lê
//! uma tarefa por linha e escreve texto puro, sem cores nem interface interativa.

use super::init;
use super::tui::brand;
use bitclaw::agent::{Progress, TurnReport};
use bitclaw::models::{Effort, ModelInfo, SharedEffort};
use bitclaw::session::Session;
use bitclaw::tools::{Approver, Outcome, Preview, PreviewLine};
use serde_json::Value;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::{Arc, Mutex};

// Pergunta no terminal e lê a resposta da entrada padrão. Com `auto_edits` (`--yes`), as edições
// passam sem perguntar; a execução de scripts sempre pergunta.
pub struct StdinApprover {
    pub auto_edits: bool,
}

impl Approver for StdinApprover {
    fn approve(&self, preview: &Preview) -> bool {
        self.auto_edits || ask(preview, "Aplicar? [s/N] ")
    }

    fn approve_run(&self, preview: &Preview) -> bool {
        ask(preview, "Executar? [s/N] ")
    }
}

// Mostra a prévia e lê "s" ou "sim" (ou "y"/"yes") como aprovação.
fn ask(preview: &Preview, question: &str) -> bool {
    println!("\n── {} ──", preview.title);
    for line in &preview.lines {
        match line {
            PreviewLine::Context(t) => println!("  {t}"),
            PreviewLine::Removed(t) => println!("- {t}"),
            PreviewLine::Added(t) => println!("+ {t}"),
            PreviewLine::Note(t) => println!("{t}"),
        }
    }
    print!("{question}");
    io::stdout().flush().ok();
    let mut answer = String::new();
    if io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
}

// Mostra cada ferramenta chamada, em texto puro.
struct Lines;

impl Progress for Lines {
    fn model_waiting(&self, _waiting: bool) {}

    fn tool_call(&self, name: &str, args: &Value) {
        let target = args.get("path").and_then(Value::as_str).unwrap_or("");
        println!("  · {name} {target}");
    }

    // A saída de um script aparece como o programa a escreveu, sem depender do resumo do modelo.
    fn tool_done(&self, name: &str, _args: &Value, outcome: &Outcome) {
        const SHOWN: usize = 20;
        let (Outcome::Done(text) | Outcome::Error(text)) = outcome else { return };
        if name != "run_script" {
            return;
        }
        let Some((summary, output)) = text.split_once('\n') else { return };
        println!("  {summary}");
        let all: Vec<&str> = output.lines().collect();
        for line in all.iter().take(SHOWN) {
            println!("    │ {line}");
        }
        if all.len() > SHOWN {
            println!("    … mais {} linhas", all.len() - SHOWN);
        }
    }
}

pub fn run(
    session: &mut Session,
    header: &[String],
    auto_yes: bool,
    active: &Arc<Mutex<ModelInfo>>,
    effort: &SharedEffort,
    skills: &[String],
) -> Result<(), String> {
    println!("{}", brand::plain_header(header));
    if auto_yes {
        println!("aviso: --yes ativo: edições serão aplicadas sem confirmação; executar scripts continua pedindo");
    }
    if !effort.lock().unwrap_or_else(|e| e.into_inner()).is_off() {
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
            "/exit" | "/quit" => break,
            "/skills" => {
                if skills.is_empty() {
                    println!("nenhuma skill carregada.\n");
                } else {
                    for line in skills {
                        println!("{line}");
                    }
                    println!();
                }
                continue;
            }
            "/think" => {
                let caps = active.lock().unwrap_or_else(|e| e.into_inner()).capabilities.clone();
                if matches!(caps.reasoning, bitclaw::models::Reasoning::Unsupported) {
                    println!("este modelo não tem raciocínio configurável.\n");
                    continue;
                }
                let mut current = effort.lock().unwrap_or_else(|e| e.into_inner());
                *current = caps.toggle_think(*current);
                if current.is_off() {
                    println!("thinking desligado.\n");
                } else {
                    println!("aviso: {}\n", brand::THINK_WARNING);
                }
                continue;
            }
            "/init" => {
                match init::write(session.root(), session.approver()) {
                    Ok(message) => println!("{message}\n"),
                    Err(e) => eprintln!("erro: {e}"),
                }
                continue;
            }
            _ if input == "/effort" || input.starts_with("/effort ") => {
                let arg = input["/effort".len()..].trim();
                let caps = active.lock().unwrap_or_else(|e| e.into_inner()).capabilities.clone();
                if arg.is_empty() {
                    let current = *effort.lock().unwrap_or_else(|e| e.into_inner());
                    let levels: Vec<&str> = caps.supported_efforts().into_iter().map(Effort::as_str).collect();
                    println!("esforço {} · níveis deste modelo: {}\n", current.as_str(), levels.join(", "));
                } else {
                    match caps.parse_for(arg) {
                        Ok(chosen) => {
                            *effort.lock().unwrap_or_else(|e| e.into_inner()) = chosen;
                            println!("esforço: {}\n", chosen.as_str());
                        }
                        Err(msg) => println!("{msg}\n"),
                    }
                }
                continue;
            }
            "/clear" => {
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
        println!("{}", t.summary());
    }
    if std::env::var_os("BITCLAW_STATS").is_some() && !report.actions.is_empty() {
        println!("{}", report.tool_usage_line());
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
    for w in &report.warnings {
        println!("aviso: {w}");
    }
    println!();
}

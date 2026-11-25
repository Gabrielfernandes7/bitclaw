//! Interface de terminal do BitClaw. Só o binário usa este módulo: a biblioteca não imprime nada.
//! Paleta inspirada no crabe (amarelo #FFCC00 como cor principal). Sem cor se a saída não for
//! um terminal, se `NO_COLOR` estiver definido ou se `--no-color` for usado.

use bitclaw::agent::{Progress, TurnReport};
use bitclaw::context::Usage;
use bitclaw::tools::{Approver, Outcome, Preview, PreviewLine};
use serde_json::Value;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static COLOR: AtomicBool = AtomicBool::new(false);

// Decide, uma vez, se cores são usadas.
pub fn init(no_color_flag: bool) {
    let enabled = !no_color_flag && std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
    COLOR.store(enabled, Ordering::Relaxed);
}

fn color_on() -> bool {
    COLOR.load(Ordering::Relaxed)
}

const YELLOW: &str = "38;2;255;204;0";
const GREEN: &str = "38;2;0;200;120";
const RED: &str = "38;2;255;77;77";
const GRAY: &str = "38;2;170;170;170";

// Aplica um código SGR ao texto, se cores estiverem ativas.
fn paint(code: &str, text: &str) -> String {
    if color_on() { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() }
}

pub fn yellow(t: &str) -> String {
    paint(YELLOW, t)
}

pub fn bold_yellow(t: &str) -> String {
    if color_on() { format!("\x1b[1;{YELLOW}m{t}\x1b[0m") } else { t.to_string() }
}

pub fn gray(t: &str) -> String {
    paint(GRAY, t)
}

fn green(t: &str) -> String {
    paint(GREEN, t)
}

pub fn red(t: &str) -> String {
    paint(RED, t)
}

// Cabeçalho em caixa arredondada, como o título do crabe.
pub fn header(title: &str, lines: &[String]) -> String {
    let mut body = vec![format!("🦀  {}", bold_yellow(title))];
    body.extend(lines.iter().cloned());
    let width = body.iter().map(|l| visible_len(l)).max().unwrap_or(0) + 4;
    let border = |s: &str| yellow(s);
    let mut out = String::new();
    out.push_str(&border(&format!("╭{}╮", "─".repeat(width))));
    out.push('\n');
    for line in &body {
        let pad = width - 2 - visible_len(line);
        out.push_str(&format!("{} {}{} {}\n", border("│"), line, " ".repeat(pad), border("│")));
    }
    out.push_str(&border(&format!("╰{}╯", "─".repeat(width))));
    out
}

// Tamanho visível de uma string, sem contar sequências ANSI. Conta caracteres, não bytes.
fn visible_len(s: &str) -> usize {
    let mut n = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\x1b' {
            in_esc = true;
        } else if (c as u32) >= 0x1F000 {
            // Emoji ocupam duas colunas no terminal.
            n += 2;
        } else {
            n += 1;
        }
    }
    n
}

// Prompt com barra de uso da janela. A cor muda conforme o nível.
pub fn prompt(name: &str, usage: &Usage) -> String {
    let filled = ((usage.percent / 100.0) * 10.0).round().clamp(0.0, 10.0) as usize;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
    let colored = if usage.percent >= 90.0 {
        red(&bar)
    } else if usage.percent >= 70.0 {
        yellow(&bar)
    } else {
        green(&bar)
    };
    format!(
        "{} {} {} ",
        bold_yellow(name),
        gray(&format!("[{colored} {:.0}% · {}/{}]", usage.percent, usage.tokens, usage.window)),
        yellow("›"),
    )
}

// Mostra a prévia de uma alteração e pede confirmação.
pub struct StdinApprover;

impl Approver for StdinApprover {
    fn approve(&self, preview: &Preview) -> bool {
        println!("\n{}", bold_yellow(&format!("── {} ──", preview.title)));
        for line in &preview.lines {
            match line {
                PreviewLine::Context(t) => println!("  {}", gray(t)),
                PreviewLine::Removed(t) => println!("{}", red(&format!("- {t}"))),
                PreviewLine::Added(t) => println!("{}", green(&format!("+ {t}"))),
                PreviewLine::Note(t) => println!("{}", red(t)),
            }
        }
        print!("{} ", yellow("Aplicar? [s/N]"));
        io::stdout().flush().ok();
        let mut answer = String::new();
        if io::stdin().lock().read_line(&mut answer).is_err() {
            return false;
        }
        matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
    }
}

// Sinais do agente para o terminal: espera do modelo (spinner) e chamadas de ferramenta.
pub struct Feedback {
    slot: Mutex<Option<(Arc<AtomicBool>, JoinHandle<()>)>>,
}

impl Feedback {
    pub fn new() -> Self {
        Self { slot: Mutex::new(None) }
    }
}

impl Progress for Feedback {
    fn model_waiting(&self, waiting: bool) {
        let mut slot = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if waiting {
            if slot.is_some() || !io::stderr().is_terminal() {
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
                        "\r{}",
                        gray(&format!("{} pensando… {}s", FRAMES[i % FRAMES.len()], started.elapsed().as_secs()))
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

    fn tool_call(&self, name: &str, args: &Value) {
        let target = args.get("path").and_then(Value::as_str).unwrap_or("");
        println!("{}", gray(&format!("  · {name} {target}")));
    }
}

// Relatório do turno: ações primeiro (o que foi executado de fato), depois o texto do modelo.
pub fn report(report: &TurnReport) {
    for t in &report.trims {
        println!(
            "{}",
            gray(&format!(
                "[contexto] descartados {} turno(s) antigo(s); caracteres {} → {} (orçamento {})",
                t.turns, t.chars_before, t.chars_after, t.budget
            ))
        );
    }
    if !report.actions.is_empty() {
        for a in &report.actions {
            let target = a.args.get("path").and_then(Value::as_str).unwrap_or("");
            let (icon, paint_fn): (&str, fn(&str) -> String) = match a.outcome {
                Outcome::Done(_) => ("✓", green),
                Outcome::Refused(_) => ("⊘", yellow),
                Outcome::Error(_) => ("✗", red),
            };
            println!("  {} {} {}", paint_fn(icon), a.name, gray(target));
            if !matches!(a.outcome, Outcome::Done(_)) {
                println!("      {}", gray(&a.outcome.text()));
            }
        }
    }
    if report.text.is_empty() {
        println!("\n{}", gray("(sem resposta)"));
    } else {
        println!("\n{}", report.text);
    }
    if let Some(e) = &report.error {
        println!("{}", red(&format!("interrompido: {e}")));
    }
    for w in report.warnings() {
        println!("{}", yellow(&format!("⚠ {w}")));
    }
    println!();
}

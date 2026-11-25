//! Interface de terminal do BitClaw. Só o binário usa este módulo: a biblioteca não imprime nada.
//! Paleta inspirada no crabe (amarelo #FFCC00 como cor principal). Sem cor se a saída não for
//! um terminal, se `NO_COLOR` estiver definido ou se `--no-color` for usado.

use bitclaw::agent::{Progress, TurnReport};
use bitclaw::context::Usage;
use bitclaw::tools::{Approver, Outcome, Preview, PreviewLine};
use serde_json::Value;
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

static COLOR: AtomicBool = AtomicBool::new(false);

// Decide, uma vez, se cores são usadas.
pub fn init(no_color_flag: bool) {
    let enabled = !no_color_flag && std::env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal();
    COLOR.store(enabled, Ordering::Relaxed);
}

fn color_on() -> bool {
    COLOR.load(Ordering::Relaxed)
}

// Cores via owo-colors, só quando ativas.
pub fn yellow(t: &str) -> String {
    if color_on() { t.truecolor(255, 204, 0).to_string() } else { t.to_string() }
}

pub fn bold_yellow(t: &str) -> String {
    if color_on() { t.truecolor(255, 204, 0).bold().to_string() } else { t.to_string() }
}

pub fn gray(t: &str) -> String {
    if color_on() { t.truecolor(170, 170, 170).to_string() } else { t.to_string() }
}

fn green(t: &str) -> String {
    if color_on() { t.truecolor(0, 200, 120).to_string() } else { t.to_string() }
}

pub fn red(t: &str) -> String {
    if color_on() { t.truecolor(255, 77, 77).to_string() } else { t.to_string() }
}

// Mascote monocromático: mapa de pixels 10×6, renderizado em meio-blocos (3 linhas de terminal)
// na cor da marca. Sinais de tijolo: dois pinos no topo e uma faixa de emenda (D) abaixo dos olhos.
// Legenda: Y corpo, T pino, D emenda, S pé (preenchidos); E olho (buraco); '.' vazio.
const CRAB: [&str; 6] = [
    "YY.T..T.YY",
    "YYYYYYYYYY",
    "YYEYYYYEYY",
    "YDDDDDDDDY",
    "..YYYYYY..",
    ".S.S..S.S.",
];

// Uma célula do terminal mostra dois pixels: o de cima e o de baixo (`▀`, `▄`, `█`).
fn crab_cell(top: char, bottom: char) -> &'static str {
    let on = |c: char| matches!(c, 'Y' | 'T' | 'D' | 'S');
    match (on(top), on(bottom)) {
        (true, true) => "█",
        (true, false) => "▀",
        (false, true) => "▄",
        (false, false) => " ",
    }
}

// Linhas de terminal do mascote: cada uma junta duas linhas do mapa.
fn crab_rows() -> Vec<String> {
    CRAB.chunks(2)
        .map(|pair| {
            let row: String = pair[0].chars().zip(pair[1].chars()).map(|(t, b)| crab_cell(t, b)).collect();
            yellow(&row)
        })
        .collect()
}

// Cabeçalho sem caixa, no estilo da tela inicial do Claude Code: mascote à esquerda,
// nome em destaque e informações em cinza à direita.
pub fn header(title: &str, lines: &[String]) -> String {
    let mut right = vec![bold_yellow(title)];
    right.extend(lines.iter().map(|l| gray(l)));
    let glyph = crab_rows();
    let offset = glyph.len().saturating_sub(right.len()) / 2;
    glyph
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let text = i.checked_sub(offset).and_then(|j| right.get(j)).cloned().unwrap_or_default();
            format!("{g}  {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Tamanho visível de uma string, sem contar sequências ANSI. Conta caracteres, não bytes.
#[cfg(test)]
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

// Entrada em duas linhas, como nos TUIs de agente: linha de status (uso da janela) e o `›`.
// A cor da barra muda conforme o nível.
pub fn prompt(_name: &str, usage: &Usage) -> String {
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
        "{}\n{} ",
        gray(&format!("contexto {colored} {:.0}% · {}/{} tok", usage.percent, usage.tokens, usage.window)),
        bold_yellow("›"),
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
        ask_yes_no("Aplicar?")
    }
}

// Confirmação: seleção interativa num terminal; leitura de linha quando a entrada é um pipe.
fn ask_yes_no(question: &str) -> bool {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return inquire::Confirm::new(question)
            .with_default(false)
            .with_parser(&|a| match a.trim().to_lowercase().as_str() {
                "s" | "sim" | "y" | "yes" => Ok(true),
                "n" | "nao" | "não" | "no" => Ok(false),
                _ => Err(()),
            })
            .with_formatter(&|v| if v { "sim".into() } else { "não".into() })
            .with_error_message("Responda s ou n.")
            .prompt()
            .unwrap_or(false);
    }
    print!("{} ", yellow(&format!("{question} [s/N]")));
    io::stdout().flush().ok();
    let mut answer = String::new();
    if io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes")
}

// Sinais do agente para o terminal: espera do modelo (spinner) e chamadas de ferramenta.
pub struct Feedback {
    spinner: Mutex<Option<ProgressBar>>,
}

impl Feedback {
    pub fn new() -> Self {
        Self { spinner: Mutex::new(None) }
    }
}

impl Progress for Feedback {
    fn model_waiting(&self, waiting: bool) {
        let mut slot = self.spinner.lock().unwrap_or_else(|e| e.into_inner());
        if waiting {
            if slot.is_some() || !io::stderr().is_terminal() {
                return;
            }
            let bar = ProgressBar::new_spinner();
            let template = if color_on() {
                "{spinner:.yellow} {msg:.dim} {elapsed}"
            } else {
                "{spinner} {msg} {elapsed}"
            };
            bar.set_style(
                ProgressStyle::with_template(template)
                    .unwrap_or_else(|_| ProgressStyle::default_spinner())
                    .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠋"]),
            );
            bar.set_message("pensando…");
            bar.enable_steady_tick(Duration::from_millis(100));
            *slot = Some(bar);
        } else if let Some(bar) = slot.take() {
            bar.finish_and_clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crab_map_is_a_rectangle_and_rows_have_the_same_visible_width() {
        assert_eq!(CRAB.len() % 2, 0);
        assert!(CRAB.iter().all(|r| r.chars().count() == CRAB[0].chars().count()));
        assert!(CRAB.iter().all(|r| r.chars().all(|c| ".YTDSE".contains(c))));
        let rows = crab_rows();
        assert_eq!(rows.len(), CRAB.len() / 2);
        assert!(rows.iter().all(|r| visible_len(r) == CRAB[0].chars().count()));
    }

    #[test]
    fn header_has_no_border_and_keeps_all_lines() {
        COLOR.store(false, Ordering::Relaxed);
        let text = header("BitClaw", &["projeto /tmp/x".into(), "modelo nemotron · janela 16384 tok".into()]);
        assert_eq!(text.lines().count(), CRAB.len() / 2);
        assert!(!text.contains('╭') && !text.contains('│'));
        assert!(text.contains("BitClaw") && text.contains("projeto /tmp/x"));
    }
}

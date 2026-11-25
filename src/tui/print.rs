//! Saída para a conversa: cabeçalho, avisos, prévias e relatório do turno.

use super::{App, brand, markdown, theme::*, view::{self, aviso_line, note}};
use super::locked;
use bitclaw::agent::{INTERRUPTED, TurnReport};
use bitclaw::tools::{Outcome, Preview, PreviewLine};
use serde_json::Value;
use ratatui::text::{Line, Span};
use std::io;

impl App {
    pub(super) fn width(&mut self) -> io::Result<usize> {
        Ok(self.terminal.size()?.width as usize)
    }

    // Acrescenta linhas à conversa. Se o usuário rolou para cima, a tela fica onde está.
    pub(super) fn print(&mut self, lines: Vec<Line<'static>>) -> io::Result<()> {
        if self.scroll_back > 0 {
            self.scroll_back += lines.len();
        }
        self.transcript.extend(lines);
        let extra = self.transcript.len().saturating_sub(super::MAX_TRANSCRIPT);
        if extra > 0 {
            self.transcript.drain(..extra);
        }
        Ok(())
    }

    // O nome, o projeto e o modelo ficam ao lado do mascote, na área fixa. Aqui vai só o que não cabe
    // lá: as demais linhas de informação (skills, pastas ignoradas) e os avisos de partida.
    pub(super) fn print_header(&mut self) -> io::Result<()> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        for extra in self.settings.info.iter().skip(view::INFO_ROWS - 1) {
            lines.push(note(extra));
        }
        if self.settings.auto_yes {
            lines.push(aviso_line("--yes ativo: edições serão aplicadas sem confirmação"));
        }
        if !locked(&self.settings.effort).is_off() {
            lines.extend(self.warning(brand::THINK_WARNING)?);
        }
        if lines.is_empty() {
            return Ok(());
        }
        lines.push(Line::default());
        self.print(lines)
    }

    // Aviso em amarelo, quebrado na largura da janela.
    pub(super) fn warning(&mut self, text: &str) -> io::Result<Vec<Line<'static>>> {
        let width = self.width()?;
        Ok(markdown::plain(text, width, Span::styled("  aviso: ", bold(ORANGE)), fg(ORANGE), &palette()))
    }

    // Prévia de uma alteração, com diff colorido, antes de pedir a decisão.
    pub(super) fn print_preview(&mut self, preview: &Preview) -> io::Result<()> {
        let width = self.width()?.saturating_sub(4).max(10);
        let cut = |t: &str| -> String { t.chars().take(width).collect() };
        let mut lines = vec![Line::from(Span::styled(format!("── {} ──", preview.title), bold(YELLOW)))];
        for line in &preview.lines {
            lines.push(match line {
                PreviewLine::Context(t) => Line::from(Span::styled(format!("  {}", cut(t)), fg(GRAY))),
                PreviewLine::Removed(t) => Line::from(Span::styled(format!("- {}", cut(t)), fg(RED))),
                PreviewLine::Added(t) => Line::from(Span::styled(format!("+ {}", cut(t)), fg(GREEN))),
                PreviewLine::Note(t) => Line::from(Span::styled(cut(t), fg(RED))),
            });
        }
        self.print(lines)
    }

    // Relatório do turno: ações primeiro (o que foi executado de fato), depois o texto do modelo.
    pub(super) fn print_report(&mut self, report: &TurnReport) -> io::Result<()> {
        let width = self.width()?;
        let mut lines: Vec<Line<'static>> = Vec::new();
        for t in &report.trims {
            lines.push(note(&format!(
                "[contexto] descartados {} turno(s) antigo(s); caracteres {} → {} (orçamento {})",
                t.turns, t.chars_before, t.chars_after, t.budget
            )));
        }
        for a in &report.actions {
            let target = a.args.get("path").and_then(Value::as_str).unwrap_or("");
            let (icon, color) = match a.outcome {
                Outcome::Done(_) => ("✓", GREEN),
                Outcome::Refused(_) => ("⊘", YELLOW),
                Outcome::Error(_) => ("✗", RED),
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(icon, fg(color)),
                Span::raw(format!(" {} ", a.name)),
                Span::styled(target.to_string(), fg(GRAY)),
            ]));
            if !matches!(a.outcome, Outcome::Done(_)) {
                let detail: String = a.outcome.text().lines().next().unwrap_or("").chars().take(width.saturating_sub(6)).collect();
                lines.push(Line::from(Span::styled(format!("      {detail}"), fg(GRAY))));
            }
        }
        let interrupted = report.error.as_deref() == Some(INTERRUPTED);
        if report.text.is_empty() {
            if !interrupted {
                lines.push(Line::default());
                lines.push(note("(sem resposta)"));
            }
        } else {
            lines.push(Line::default());
            lines.extend(markdown::render(&report.text, width, &palette()));
        }
        if let Some(e) = &report.error {
            lines.push(if interrupted {
                note("interrompido")
            } else {
                Line::from(Span::styled(format!("  interrompido: {e}"), fg(RED)))
            });
        }
        for w in report.warnings() {
            lines.push(aviso_line(&w));
        }
        lines.push(Line::default());
        self.print(lines)
    }
}

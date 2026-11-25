//! Saída para a conversa: cabeçalho, avisos, prévias e relatório do turno.

use super::{App, brand, markdown, theme::*, view::{self, aviso_line, note}};
use super::locked;
use bitclaw::agent::{INTERRUPTED, TurnReport};
use bitclaw::tools::{Preview, PreviewLine};
use ratatui::text::{Line, Span};
use std::io;

// Marca a primeira linha da resposta com `●`, para separar os turnos. Troca o recuo de dois espaços,
// que tem a mesma largura, sem deslocar o texto.
fn mark_reply(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if let Some(first) = lines.first_mut() {
        let mark = Span::styled("● ", bold(YELLOW));
        let indented = first.spans.first().is_some_and(|s| s.content.starts_with("  "));
        if indented {
            let head = first.spans[0].clone();
            first.spans[0] = mark;
            let rest = head.content[2..].to_string();
            if !rest.is_empty() {
                first.spans.insert(1, Span::styled(rest, head.style));
            }
        } else {
            first.spans.insert(0, mark);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_of_a_reply_gets_the_mark_without_shifting_the_text() {
        let lines = mark_reply(vec![Line::from(Span::raw("  primeira")), Line::from(Span::raw("  segunda"))]);
        let text = |l: &Line| -> String { l.spans.iter().map(|s| s.content.as_ref()).collect() };
        assert_eq!(text(&lines[0]), "● primeira");
        assert_eq!(text(&lines[1]), "  segunda", "as demais linhas ficam como estavam");
        let bare = mark_reply(vec![Line::from(Span::raw("sem recuo"))]);
        assert_eq!(text(&bare[0]), "● sem recuo");
    }
}

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
            lines.push(aviso_line("--yes ativo: edições serão aplicadas sem confirmação; executar scripts continua pedindo"));
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

    // Raciocínio do modelo: texto cinza e recuado, limitado a algumas linhas para não tomar a conversa.
    pub(super) fn print_reasoning(&mut self, text: &str) -> io::Result<()> {
        const MAX_LINES: usize = 12;
        let width = self.width()?.saturating_sub(4).max(10);
        let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let mut lines = vec![Line::from(Span::styled("  raciocínio do modelo", bold(GRAY)))];
        for line in all.iter().take(MAX_LINES) {
            let cut: String = line.chars().take(width).collect();
            lines.push(Line::from(Span::styled(format!("  {cut}"), fg(GRAY))));
        }
        if all.len() > MAX_LINES {
            lines.push(note(&format!("… mais {} linhas de raciocínio", all.len() - MAX_LINES)));
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
    // Com `runs` (executar um script), nenhuma linha é cortada: as longas são quebradas, para que nada
    // do que será executado fique escondido além da borda do terminal.
    pub(super) fn print_preview(&mut self, preview: &Preview, runs: bool) -> io::Result<()> {
        let width = self.width()?.saturating_sub(4).max(10);
        let rows = |t: &str| -> Vec<String> {
            if !runs {
                return vec![t.chars().take(width).collect()];
            }
            let chars: Vec<char> = t.chars().collect();
            if chars.is_empty() { vec![String::new()] } else { chars.chunks(width).map(|c| c.iter().collect()).collect() }
        };
        let mut lines = vec![Line::from(Span::styled(format!("── {} ──", preview.title), bold(YELLOW)))];
        for line in &preview.lines {
            let (text, prefix, style) = match line {
                PreviewLine::Context(t) => (t, "  ", fg(GRAY)),
                PreviewLine::Removed(t) => (t, "- ", fg(RED)),
                PreviewLine::Added(t) => (t, "+ ", fg(GREEN)),
                PreviewLine::Note(t) => (t, "", fg(RED)),
            };
            for row in rows(text) {
                lines.push(Line::from(Span::styled(format!("{prefix}{row}"), style)));
            }
        }
        self.print(lines)
    }

    // Relatório do turno: o que sobrou depois dos passos (que já apareceram ao vivo), o texto do modelo e avisos.
    pub(super) fn print_report(&mut self, report: &TurnReport) -> io::Result<()> {
        let width = self.width()?;
        let mut lines: Vec<Line<'static>> = Vec::new();
        for t in &report.trims {
            lines.push(note(&format!(
                "[contexto] descartados {} turno(s) antigo(s); caracteres {} → {} (orçamento {})",
                t.turns, t.chars_before, t.chars_after, t.budget
            )));
        }
        let interrupted = report.error.as_deref() == Some(INTERRUPTED);
        if report.text.is_empty() {
            if !interrupted {
                lines.push(Line::default());
                lines.push(note("(sem resposta)"));
            }
        } else {
            lines.push(Line::default());
            lines.extend(mark_reply(markdown::render(&report.text, width, &palette())));
        }
        if let Some(e) = &report.error {
            lines.push(if interrupted {
                note("interrompido")
            } else {
                Line::from(Span::styled(format!("  interrompido: {e}"), fg(RED)))
            });
        }
        for w in &report.warnings {
            lines.push(aviso_line(&w));
        }
        lines.push(Line::default());
        self.print(lines)
    }
}

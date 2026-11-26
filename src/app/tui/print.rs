//! Saída para a conversa: cabeçalho, avisos, prévias e relatório do turno.

use super::{App, brand, markdown, theme::*, view::{self, aviso_line, note}};
use super::locked;
use bitclaw::agent::{INTERRUPTED, TurnReport};
use bitclaw::tools::{Preview, PreviewLine};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::io;

// Colunas que um caractere ocupa na tela (os de largura dupla, como emojis, valem 2).
fn cell_width(c: char) -> usize {
    Span::raw(c.to_string()).width()
}

// Quebra uma linha que passa de `width` colunas, sem perder as cores. Prefere quebrar nos espaços; as
// continuações recuam como o começo da linha (e, num item com `•`, alinham com o texto depois dele).
// Linhas que já cabem passam sem mudança.
pub(super) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width < 8 || line.width() <= width {
        return vec![line];
    }
    let cells: Vec<(char, Style)> = line.spans.iter().flat_map(|s| s.content.chars().map(move |c| (c, s.style))).collect();
    // A marca `● ` do começo da resposta ocupa o lugar do recuo de dois espaços: as continuações alinham com o texto.
    let indent = match cells.as_slice() {
        [('●', _), (' ', _), ..] => 2,
        _ => cells.iter().take_while(|(c, _)| *c == ' ').count(),
    };
    let hang = (indent + if cells.get(indent).is_some_and(|(c, _)| *c == '•') { 2 } else { 0 }).min(width / 2);
    let mut rows: Vec<Vec<(char, Style)>> = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let budget = if rows.is_empty() { width } else { width - hang };
        let mut used = 0;
        let mut end = start;
        while end < cells.len() && used + cell_width(cells[end].0) <= budget {
            used += cell_width(cells[end].0);
            end += 1;
        }
        let mut next = end;
        if end < cells.len() {
            // Quebra no último espaço que cabe; sem espaço (uma palavra enorme), corta na borda.
            if let Some(space) = cells[start..end].iter().rposition(|(c, _)| *c == ' ').filter(|&i| i > 0) {
                end = start + space;
                next = end + 1;
            }
        }
        rows.push(cells[start..end].to_vec());
        start = next;
    }
    rows.into_iter()
        .enumerate()
        .map(|(i, row)| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (c, style) in row {
                match spans.last_mut() {
                    Some(last) if last.style == style => last.content.to_mut().push(c),
                    _ => spans.push(Span::styled(c.to_string(), style)),
                }
            }
            if i > 0 {
                spans.insert(0, Span::raw(" ".repeat(hang)));
            }
            Line::from(spans)
        })
        .collect()
}

// Corta uma linha na borda da janela, sem quebrá-la. É para linhas que não podem continuar embaixo,
// como as do cabeçalho, que ficam ao lado do mascote.
pub(super) fn clip_line(line: Line<'static>, width: usize) -> Line<'static> {
    let mut used = 0;
    let mut spans: Vec<Span<'static>> = Vec::new();
    for span in line.spans {
        let mut kept = String::new();
        for c in span.content.chars() {
            if used + cell_width(c) > width {
                break;
            }
            used += cell_width(c);
            kept.push(c);
        }
        let done = kept.chars().count() < span.content.chars().count();
        if !kept.is_empty() {
            spans.push(Span::styled(kept, span.style));
        }
        if done {
            break;
        }
    }
    Line::from(spans)
}

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
    fn a_marked_reply_wraps_aligned_with_its_text() {
        let line = mark_reply(vec![Line::from(Span::raw("  uma resposta que precisa quebrar"))]).remove(0);
        let rows: Vec<String> = wrap_line(line, 20).iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert_eq!(rows, ["● uma resposta que", "  precisa quebrar"]);
    }

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

    // Acrescenta linhas à conversa. Elas são quebradas na largura da janela na hora de desenhar, então
    // redimensionar reflui o texto em vez de cortá-lo.
    pub(super) fn print(&mut self, lines: Vec<Line<'static>>) -> io::Result<()> {
        self.transcript.push(lines);
        Ok(())
    }

    // A abertura, no topo da conversa: o mascote com o nome, o projeto e o modelo ao lado, e depois o
    // que não cabe ali (skills, pastas ignoradas) e os avisos de partida.
    pub(super) fn print_header(&mut self) -> io::Result<()> {
        let width = self.terminal.size()?.width;
        let idle = view::Mascot { mood: brand::Mood::Idle, ms: 0 };
        let (banner, _) = view::banner_lines(&self.settings, &self.stats, idle, width);
        let mut lines: Vec<Line<'static>> = banner.into_iter().map(|l| clip_line(l, usize::from(width))).collect();
        for extra in self.settings.info.iter().skip(view::INFO_ROWS - 1) {
            lines.push(note(extra));
        }
        if self.settings.auto_yes {
            lines.push(aviso_line("--yes ativo: edições serão aplicadas sem confirmação; executar scripts continua pedindo"));
        }
        if !locked(&self.settings.effort).is_off() {
            lines.extend(self.warning(brand::THINK_WARNING)?);
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
            lines.push(note(&t.summary()));
        }
        if std::env::var_os("BITCLAW_STATS").is_some() {
            let usage = report.tool_usage_line();
            if !usage.is_empty() {
                lines.push(note(&usage));
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

#[cfg(test)]
mod wrap_tests {
    use super::*;
    use ratatui::style::Color;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_line_that_fits_is_left_alone() {
        let line = Line::from(Span::raw("  curta"));
        assert_eq!(wrap_line(line.clone(), 40), vec![line]);
    }

    #[test]
    fn a_long_line_wraps_at_spaces_keeping_the_indent_and_the_color() {
        let gray = Style::default().fg(Color::Gray);
        let line = Line::from(Span::styled("  um texto bem comprido que não cabe numa janela estreita", gray));
        let rows = wrap_line(line, 24);
        assert!(rows.len() >= 3, "{rows:?}");
        for r in &rows {
            assert!(r.width() <= 24, "{:?}", text(r));
            assert!(text(r).starts_with("  "), "recuo mantido: {:?}", text(r));
        }
        let joined: Vec<String> = rows.iter().map(|r| text(r).trim().to_string()).collect();
        assert_eq!(joined.join(" "), "um texto bem comprido que não cabe numa janela estreita", "nenhuma palavra se perde");
        assert!(rows.iter().flat_map(|r| &r.spans).filter(|s| !s.content.trim().is_empty()).all(|s| s.style == gray), "a cor segue");
    }

    #[test]
    fn a_bullet_hangs_its_continuation_under_the_text() {
        let line = Line::from(Span::raw("  • perto de zero o modelo escolhe quase sempre o token mais provável"));
        let rows = wrap_line(line, 30);
        assert!(text(&rows[0]).starts_with("  • perto"), "{rows:?}");
        for r in &rows[1..] {
            assert!(text(r).starts_with("    ") && !text(r).starts_with("     "), "alinha com o texto depois do •: {:?}", text(r));
        }
    }

    #[test]
    fn clipping_cuts_at_the_edge_and_keeps_the_colors() {
        let red = Style::default().fg(Color::Red);
        let line = Line::from(vec![Span::raw("ab"), Span::styled("cdefgh", red)]);
        let clipped = clip_line(line, 5);
        assert_eq!(text(&clipped), "abcde");
        assert_eq!(clipped.spans[1].style, red);
        assert_eq!(text(&clip_line(Line::from("curta"), 20)), "curta");
    }

    #[test]
    fn a_word_longer_than_the_window_is_cut_at_the_edge() {
        let rows = wrap_line(Line::from("x".repeat(50)), 20);
        assert!(rows.iter().all(|r| r.width() <= 20), "{rows:?}");
        assert_eq!(rows.iter().map(text).collect::<String>().len(), 50);
    }
}

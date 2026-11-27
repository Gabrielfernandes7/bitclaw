//! Painel de esforço (`/effort` sem argumento): um controle deslizante com os níveis do modelo ativo.

use super::keys::key_hints;
use super::super::theme::*;
use bitclaw::models::Effort;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

// Níveis do modelo (com `off` na frente), o nível em foco no cursor e o que já está em uso.
pub(in crate::app::tui) struct EffortPanel {
    pub(in crate::app::tui) levels: Vec<Effort>,
    pub(in crate::app::tui) index: usize,
    pub(in crate::app::tui) current: Effort,
}

impl EffortPanel {
    // O cursor começa no nível que já está em uso.
    pub(in crate::app::tui) fn new(levels: Vec<Effort>, current: Effort) -> Self {
        let index = levels.iter().position(|&e| e == current).unwrap_or(0);
        Self { levels, index, current }
    }

    // Nível em foco, se houver algum.
    pub(in crate::app::tui) fn chosen(&self) -> Option<Effort> {
        self.levels.get(self.index).copied()
    }

    // Anda um nível para a direita (`forward`) ou para a esquerda, parando nas pontas.
    pub(in crate::app::tui) fn step(&mut self, forward: bool) {
        let last = self.levels.len().saturating_sub(1);
        self.index = if forward { (self.index + 1).min(last) } else { self.index.saturating_sub(1) };
    }
}

// Cor de cada nível: quanto mais raciocínio, mais quente. `off` fica cinza; `on` (modelos sem níveis)
// fica verde, para o controle de dois pontos também mostrar que acendeu.
pub(in crate::app::tui) fn effort_color(effort: Effort) -> Color {
    match effort {
        Effort::Low | Effort::On => GREEN,
        Effort::Medium => YELLOW,
        Effort::High => RED,
        Effort::Off => GRAY,
    }
}

// Uma linha explicando o que cada nível faz.
pub(in crate::app::tui) fn effort_help(effort: Effort) -> &'static str {
    match effort {
        Effort::Off => "sem raciocínio: resposta direta",
        Effort::On => "raciocínio ligado no padrão do modelo",
        Effort::Low => "raciocínio curto, respostas mais rápidas",
        Effort::Medium => "equilíbrio entre velocidade e raciocínio",
        Effort::High => "raciocínio mais profundo, porém mais lento",
    }
}

// Título, folga, controle, nomes, nível, ajuda e dica: o painel não pede mais que isso.
pub(in crate::app::tui) const EFFORT_PANEL_ROWS: u16 = 7;

// Traços entre dois pontos do controle; os nomes dos níveis cabem nesse espaço.
const SEGMENT: usize = 6;

// ◂ ●━━━━━━●━━━━━━◉──────○ ▸ : até o cursor, a reta é cheia e cada trecho tem a cor do nível a que chega,
// então ela esquenta conforme o esforço sobe (cinza, verde, amarelo, vermelho). Depois do cursor, fica fina
// e cinza. As setas apagam na ponta em que não dá mais para andar. Sem cores, cheia e fina ainda se distinguem.
fn slider_line(panel: &EffortPanel) -> Line<'static> {
    let here = panel.chosen().map_or(GRAY, effort_color);
    let arrow = |can: bool| if can { fg(here) } else { fg(DIM) };
    let mut spans = vec![Span::styled("◂ ", arrow(panel.index > 0))];
    for (i, &level) in panel.levels.iter().enumerate() {
        let color = effort_color(level);
        if i > 0 {
            let span = if i <= panel.index {
                Span::styled("━".repeat(SEGMENT), fg(color))
            } else {
                Span::styled("─".repeat(SEGMENT), fg(DIM))
            };
            spans.push(span);
        }
        spans.push(match i.cmp(&panel.index) {
            std::cmp::Ordering::Less => Span::styled("●", fg(color)),
            std::cmp::Ordering::Equal => Span::styled("◉", bold(color)),
            std::cmp::Ordering::Greater => Span::styled("○", fg(GRAY)),
        });
    }
    spans.push(Span::styled(" ▸", arrow(panel.index + 1 < panel.levels.len())));
    Line::from(spans)
}

// Nome de cada nível centrado sob o seu ponto, na cor do nível; o do cursor em negrito.
fn names_line(panel: &EffortPanel) -> Line<'static> {
    let mut spans = Vec::new();
    let mut col = 0;
    for (i, &level) in panel.levels.iter().enumerate() {
        let name = level.as_str();
        let dot = 2 + i * (SEGMENT + 1);
        let start = dot.saturating_sub(name.len() / 2).max(col);
        spans.push(Span::raw(" ".repeat(start - col)));
        let style = if i == panel.index { bold(effort_color(level)) } else { fg(effort_color(level)) };
        spans.push(Span::styled(name, style));
        col = start + name.len();
    }
    Line::from(spans)
}

pub(in crate::app::tui) fn draw_effort_panel(f: &mut Frame, area: Rect, panel: &EffortPanel) {
    let [title, _gap, slider, names, label, help, _rest, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    f.render_widget(Paragraph::new(Line::from(Span::styled("Esforço de raciocínio", bold(YELLOW)))), title);
    f.render_widget(Paragraph::new(slider_line(panel)), slider);
    f.render_widget(Paragraph::new(names_line(panel)), names);

    if let Some(level) = panel.chosen() {
        let mut label_spans = vec![Span::styled(level.as_str(), bold(effort_color(level)))];
        if level == panel.current {
            label_spans.push(Span::styled("  (atual)", fg(GREEN)));
        }
        f.render_widget(Paragraph::new(Line::from(label_spans)), label);
        f.render_widget(Paragraph::new(Line::from(Span::styled(effort_help(level), fg(GRAY)))), help);
    }

    let keys = key_hints(&[("←/→", "muda o nível"), ("enter", "aplica"), ("esc", "fecha")], hint.width as usize);
    f.render_widget(Paragraph::new(keys), hint);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_starts_on_the_current_level_and_stops_at_the_ends() {
        let levels = vec![Effort::Off, Effort::Low, Effort::Medium, Effort::High];
        let mut panel = EffortPanel::new(levels, Effort::Medium);
        assert_eq!(panel.chosen(), Some(Effort::Medium));
        panel.step(true);
        assert_eq!(panel.chosen(), Some(Effort::High));
        panel.step(true);
        assert_eq!(panel.chosen(), Some(Effort::High), "para na ponta");
        for _ in 0..5 {
            panel.step(false);
        }
        assert_eq!(panel.chosen(), Some(Effort::Off), "para na outra ponta");
    }

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_track_is_filled_and_colored_up_to_the_cursor() {
        let levels = vec![Effort::Off, Effort::Low, Effort::Medium, Effort::High];
        let line = slider_line(&EffortPanel::new(levels, Effort::Medium));
        assert_eq!(text(&line), "◂ ●━━━━━━●━━━━━━◉──────○ ▸");
        let color = |s: &str| line.spans.iter().find(|x| x.content == s).and_then(|x| x.style.fg);
        assert_eq!(color("◉"), Some(YELLOW), "cursor na cor do nível");
        let filled: Vec<_> = line.spans.iter().filter(|s| s.content.starts_with('━')).map(|s| s.style.fg).collect();
        assert_eq!(filled, vec![Some(GREEN), Some(YELLOW)], "cada trecho na cor do nível a que chega");
        assert_eq!(color("──────"), Some(DIM));
        assert_eq!(color(" ▸"), Some(YELLOW), "ainda dá para subir");
    }

    #[test]
    fn arrows_dim_at_the_ends_and_names_sit_under_their_dots() {
        let levels = vec![Effort::Off, Effort::Low, Effort::Medium, Effort::High];
        let panel = EffortPanel::new(levels, Effort::Off);
        let line = slider_line(&panel);
        assert_eq!(line.spans[0].style.fg, Some(DIM), "na ponta esquerda não dá para descer");
        let track = text(&line);
        let names = text(&names_line(&panel));
        for name in ["off", "low", "medium", "high"] {
            let at = names.find(name).unwrap();
            let center = names[..at].chars().count() + name.len() / 2;
            let under: char = track.chars().nth(center).unwrap();
            assert!("●◉○".contains(under), "{name} sob um ponto: {track:?} / {names:?}");
        }
    }

    #[test]
    fn unknown_current_level_starts_at_off() {
        let panel = EffortPanel::new(vec![Effort::Off, Effort::On], Effort::High);
        assert_eq!(panel.chosen(), Some(Effort::Off));
    }
}

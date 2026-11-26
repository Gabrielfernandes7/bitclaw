//! Painel de esforço (`/effort` sem argumento): um controle deslizante com os níveis do modelo ativo.

use super::super::theme::*;
use bitclaw::models::Effort;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
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

// Cor de cada nível: quanto mais raciocínio, mais quente. `off` e `on` ficam cinza.
pub(in crate::app::tui) fn effort_color(effort: Effort) -> Color {
    match effort {
        Effort::Low => GREEN,
        Effort::Medium => YELLOW,
        Effort::High => RED,
        Effort::Off | Effort::On => GRAY,
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

pub(in crate::app::tui) fn draw_effort_panel(f: &mut Frame, panel: &EffortPanel) {
    let [title, _gap, slider, label, help, _rest, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(Paragraph::new(Line::from(Span::styled("Esforço de raciocínio", bold(YELLOW)))), title);

    // ◂ ◉──────○──────○ ▸ : um ponto por nível, ligados por traços. O ponto cheio é o cursor.
    let mut spans = vec![Span::styled("◂ ", fg(GRAY))];
    for (i, &level) in panel.levels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("──────", fg(GRAY)));
        }
        let span = if i == panel.index {
            Span::styled("◉", bold(effort_color(level)))
        } else {
            Span::styled("○", fg(GRAY))
        };
        spans.push(span);
    }
    spans.push(Span::styled(" ▸", fg(GRAY)));
    f.render_widget(Paragraph::new(Line::from(spans)), slider);

    if let Some(level) = panel.chosen() {
        let mut label_spans = vec![Span::styled(level.as_str(), bold(effort_color(level)))];
        if level == panel.current {
            label_spans.push(Span::styled("  (current)", fg(GREEN)));
        }
        f.render_widget(Paragraph::new(Line::from(label_spans)), label);
        f.render_widget(Paragraph::new(Line::from(Span::styled(effort_help(level), fg(GRAY)))), help);
    }

    let hint_text = "←/→ muda o nível · enter escolhe · esc fecha";
    f.render_widget(Paragraph::new(Line::from(Span::styled(hint_text, fg(GRAY)))), hint);
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

    #[test]
    fn unknown_current_level_starts_at_off() {
        let panel = EffortPanel::new(vec![Effort::Off, Effort::On], Effort::High);
        assert_eq!(panel.chosen(), Some(Effort::Off));
    }
}

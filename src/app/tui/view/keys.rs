//! Rodapé de teclas dos painéis: a tecla em destaque e a ação em cinza, no mesmo formato em todos.

use super::super::theme::*;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const SEP: &str = " · ";

// Largura em colunas do rodapé com todos os pares.
fn width(pairs: &[(&str, &str)]) -> usize {
    let text: usize = pairs.iter().map(|(k, a)| k.chars().count() + 1 + a.chars().count()).sum();
    text + SEP.chars().count() * pairs.len().saturating_sub(1)
}

// Monta `tecla ação · tecla ação`. Se não cabe em `room` colunas, tira os pares do meio para
// o fim, mas mantém o último (por convenção, o `esc`), que é a saída do painel.
pub(in crate::app::tui) fn key_hints(pairs: &[(&str, &str)], room: usize) -> Line<'static> {
    let mut pairs = pairs.to_vec();
    while pairs.len() > 1 && width(&pairs) > room {
        pairs.remove(pairs.len() - 2);
    }
    let mut spans = Vec::new();
    for (i, (key, action)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(SEP, fg(GRAY)));
        }
        spans.push(Span::styled(key.to_string(), Style::default().add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {action}"), fg(GRAY)));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn fits_everything_when_there_is_room() {
        let line = key_hints(&[("↑/↓", "navega"), ("enter", "escolhe"), ("esc", "fecha")], 80);
        assert_eq!(text(&line), "↑/↓ navega · enter escolhe · esc fecha");
    }

    #[test]
    fn drops_the_middle_first_and_keeps_esc() {
        let line = key_hints(&[("↑/↓", "navega"), ("enter", "escolhe"), ("esc", "fecha")], 22);
        assert_eq!(text(&line), "↑/↓ navega · esc fecha");
        let tiny = key_hints(&[("↑/↓", "navega"), ("enter", "escolhe"), ("esc", "fecha")], 5);
        assert_eq!(text(&tiny), "esc fecha");
    }
}

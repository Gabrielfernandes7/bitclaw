//! Nome, projeto e modelo, ao lado do mascote enquanto o agente está ocioso.

use super::super::theme::*;
use ratatui::text::{Line, Span};

// Linhas de texto que cabem ao lado do mascote: o nome e mais duas.
pub(in crate::tui) const INFO_ROWS: usize = 3;

// O nome do agente e as duas primeiras linhas de informação (projeto e modelo). A linha do modelo
// descreve o modelo do início; se ele foi trocado com `/model`, mostra o ativo no lugar.
pub(in crate::tui) fn info_lines(title: &str, info: &[String], active: &str, default: &str) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(title.to_string(), bold(YELLOW)))];
    for (i, text) in info.iter().take(INFO_ROWS - 1).enumerate() {
        let text = if i == 1 && active != default { format!("modelo {active} · trocado com /model") } else { text.clone() };
        lines.push(Line::from(Span::styled(text, fg(GRAY))));
    }
    lines
}

// As linhas de informação com a janela trocada: a do modelo foi escrita com a janela da configuração
// (`janela 16384 tok`); se `/context` a mudou, mostra a atual.
pub(in crate::tui) fn with_window(info: &[String], default: u32, current: u32) -> Vec<String> {
    if default == current {
        return info.to_vec();
    }
    let (old, new) = (format!("janela {default} tok"), format!("janela {current} tok"));
    info.iter().map(|l| l.replace(&old, &new)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_shows_the_window_in_use() {
        let info = vec!["modelo m · janela 16384 tok".to_string()];
        assert_eq!(with_window(&info, 16384, 16384), info, "sem mudança, nada muda");
        assert_eq!(with_window(&info, 16384, 32768), vec!["modelo m · janela 32768 tok".to_string()]);
    }

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn shows_the_name_the_project_and_the_model() {
        let info = vec!["projeto /p".to_string(), "modelo m:1b · 1B".to_string(), "skills: a".to_string()];
        let lines: Vec<String> = info_lines("BitClaw", &info, "m:1b", "m:1b").iter().map(text).collect();
        assert_eq!(lines, vec!["BitClaw", "projeto /p", "modelo m:1b · 1B"], "o resto vai para o histórico");
    }

    #[test]
    fn a_switched_model_replaces_the_stale_model_line() {
        let info = vec!["projeto /p".to_string(), "modelo m:1b · 1B".to_string()];
        let lines: Vec<String> = info_lines("BitClaw", &info, "outro:3b", "m:1b").iter().map(text).collect();
        assert_eq!(lines[2], "modelo outro:3b · trocado com /model");
        assert_eq!(lines[1], "projeto /p");
    }

    #[test]
    fn works_with_less_information_than_rows() {
        assert_eq!(info_lines("BitClaw", &[], "m", "m").len(), 1);
    }
}

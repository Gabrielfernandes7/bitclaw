//! Nome, projeto e modelo, ao lado do mascote enquanto o agente está ocioso.

use super::super::theme::*;
use ratatui::text::{Line, Span};

// Linhas de texto que cabem ao lado do mascote: o nome e mais duas.
pub(in crate::app::tui) const INFO_ROWS: usize = 3;

// O nome do agente, a linha do projeto (a primeira de `info`) e a linha do modelo, já montada com o
// modelo, o esforço e a janela de agora.
pub(in crate::app::tui) fn info_lines(title: &str, info: &[String], model_line: String) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(title.to_string(), bold(YELLOW)))];
    if let Some(project) = info.first() {
        lines.push(Line::from(Span::styled(project.clone(), fg(GRAY))));
    }
    lines.push(Line::from(Span::styled(model_line, fg(GRAY))));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn shows_the_name_the_project_and_the_model() {
        let info = vec!["Projeto (p) /p".to_string(), "linha velha".to_string(), "skills: a".to_string()];
        let lines: Vec<String> = info_lines("BitClaw", &info, "m:1b thinking off".into()).iter().map(text).collect();
        assert_eq!(lines, vec!["BitClaw", "Projeto (p) /p", "m:1b thinking off"], "o resto vai para o histórico");
        assert_eq!(lines.len(), INFO_ROWS);
    }

    #[test]
    fn without_a_project_line_only_the_model_follows_the_name() {
        assert_eq!(info_lines("BitClaw", &[], "m".into()).len(), 2);
    }
}

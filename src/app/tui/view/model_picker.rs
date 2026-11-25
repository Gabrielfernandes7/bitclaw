//! Painel de modelos (`/model`).

use super::super::{Settings, locked, theme::*};
use super::PICKER_VISIBLE;
use bitclaw::models::{Effort, ModelCapabilities, ModelInfo, Reasoning};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

// Lista de modelos do catálogo, lida quando o painel abre. A busca olha o id.
pub(in crate::app::tui) struct ModelPicker {
    pub(in crate::app::tui) query: String,
    pub(in crate::app::tui) index: usize,
    pub(in crate::app::tui) models: Vec<ModelInfo>,
}

// Linhas visíveis do painel de modelos, índices em `models`.
pub(in crate::app::tui) fn model_rows(p: &ModelPicker) -> Vec<usize> {
    let q = p.query.to_lowercase();
    (0..p.models.len()).filter(|&i| q.is_empty() || p.models[i].id.to_lowercase().contains(&q)).collect()
}

// Capacidades numa linha só: é o que pesa na escolha.
pub(in crate::app::tui) fn capabilities_text(caps: &ModelCapabilities) -> String {
    let tools = if caps.tool_calling { "ferramentas" } else { "sem ferramentas" };
    let reasoning = match &caps.reasoning {
        Reasoning::Unsupported => "sem raciocínio".to_string(),
        Reasoning::Toggle => "raciocínio liga/desliga".to_string(),
        Reasoning::Levels(_) => {
            let levels: Vec<&str> = caps.supported_efforts().into_iter().filter(|e| !e.is_off()).map(Effort::as_str).collect();
            format!("raciocínio: {}", levels.join(", "))
        }
    };
    let mut parts = vec![tools.to_string(), reasoning];
    if let Some(window) = caps.context_window {
        parts.push(format!("janela {window} tok"));
    }
    parts.join(" · ")
}

pub(in crate::app::tui) fn draw_model_picker(f: &mut Frame, settings: &Settings, p: &ModelPicker) {
    let rows = model_rows(p);
    let active = locked(&settings.model).id.clone();
    let [title, search, list, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let head = vec![
        Span::styled(format!("{} modelos", p.models.len()), bold(YELLOW)),
        Span::styled(format!(" · ativo {active} · padrão {}", settings.default_model), fg(GRAY)),
    ];
    f.render_widget(Paragraph::new(Line::from(head)), title);

    let box_style = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(fg(GRAY));
    let search_text = if p.query.is_empty() {
        Line::from(Span::styled("⌕ Buscar modelos…", fg(GRAY)))
    } else {
        Line::from(vec![Span::styled("⌕ ", fg(GRAY)), Span::raw(p.query.clone())])
    };
    f.render_widget(Paragraph::new(search_text).block(box_style), search);

    let name_w = p.models.iter().map(|m| m.id.chars().count()).max().unwrap_or(0).clamp(12, 36);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  nenhum modelo encontrado", fg(GRAY))));
    } else {
        let start = if p.index >= PICKER_VISIBLE { p.index + 1 - PICKER_VISIBLE } else { 0 };
        for (pos, &i) in rows.iter().enumerate().skip(start).take(PICKER_VISIBLE) {
            let model = &p.models[i];
            let selected = pos == p.index;
            let is_active = model.id == active;
            let (cursor, name_style) = if selected { ("❯ ", bold(YELLOW)) } else { ("  ", Style::default()) };
            let (mark, mark_style) = if is_active { ("● ", fg(YELLOW)) } else { ("○ ", fg(GRAY)) };
            let default = if model.id == settings.default_model { " · padrão" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(cursor, fg(YELLOW)),
                Span::styled(mark, mark_style),
                Span::styled(format!("{:<name_w$}", model.id), name_style),
                Span::styled(format!("  {}{default}", capabilities_text(&model.capabilities)), fg(GRAY)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines), list);

    let help = "↑/↓ navega · enter escolhe · digite para buscar · esc fecha";
    f.render_widget(Paragraph::new(Line::from(Span::styled(help, fg(GRAY)))), hint);
}

// Ficha de um modelo para `/model show`.
pub(in crate::app::tui) fn model_card(model: &ModelInfo, active: bool) -> Vec<Line<'static>> {
    let label = |k: &str, v: String| {
        Line::from(vec![Span::styled(format!("  {k:<11}"), fg(GRAY)), Span::styled(v, Style::default())])
    };
    let provider = if model.provider.is_empty() { "desconhecido".to_string() } else { model.provider.clone() };
    let mut lines = vec![Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(model.id.clone(), bold(YELLOW)),
        Span::styled(if active { "  (ativo)" } else { "" }, fg(GREEN)),
    ])];
    lines.push(label("provedor", provider));
    lines.push(label("capacidades", capabilities_text(&model.capabilities)));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_card_lists_id_provider_and_capabilities() {
        let text = |l: &Line| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        let model = ModelInfo::unknown("m:1b");
        let card: Vec<String> = model_card(&model, true).iter().map(text).collect();
        assert_eq!(card[0], "  m:1b  (ativo)");
        assert!(card[1].contains("provedor") && card[1].contains("desconhecido"), "{card:?}");
        assert!(card[2].contains("capacidades") && card[2].contains("ferramentas"), "{card:?}");
        assert_eq!(model_card(&model, false)[0].spans.iter().map(|s| s.content.as_ref()).collect::<String>(), "  m:1b");
    }
}

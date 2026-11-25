//! Painel de skills (`/skills`).

use super::super::{Settings, theme::*};
use bitclaw::skills::{self, Skill};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

// Estado do painel: o que foi digitado na busca, a linha escolhida e a ordem.
pub(in crate::tui) struct Picker {
    pub(in crate::tui) query: String,
    pub(in crate::tui) index: usize,
    pub(in crate::tui) by_name: bool,
}

// Linhas visíveis do painel, índices em `skills`. A busca olha nome e descrição.
pub(in crate::tui) fn picker_rows(skills: &[Skill], p: &Picker) -> Vec<usize> {
    let q = p.query.to_lowercase();
    let mut rows: Vec<usize> = (0..skills.len())
        .filter(|&i| {
            q.is_empty()
                || skills[i].name.to_lowercase().contains(&q)
                || skills[i].description.to_lowercase().contains(&q)
        })
        .collect();
    if p.by_name {
        rows.sort_by(|&a, &b| skills[a].name.cmp(&skills[b].name));
    }
    rows
}

pub(in crate::tui) const PICKER_VISIBLE: usize = 8;

// Título (1), caixa de busca (3), lista (até PICKER_VISIBLE linhas) e dica (1).
pub(in crate::tui) fn picker_height(rows: usize) -> u16 {
    (1 + 3 + rows.clamp(1, PICKER_VISIBLE) + 1) as u16
}

pub(in crate::tui) fn draw_picker(f: &mut Frame, settings: &Settings, p: &Picker) {
    let rows = picker_rows(&settings.skills, p);
    let off = skills::disabled_now(&settings.skills_off);
    let enabled: Vec<&Skill> = settings.skills.iter().filter(|s| !off.contains(&s.name)).collect();
    let tokens: usize = enabled.iter().map(|s| skills::catalog_tokens(s)).sum();
    let [title, search, list, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let head = vec![
        Span::styled(format!("{} skills", settings.skills.len()), bold(YELLOW)),
        Span::styled(format!(" · {} ativas · ~{tokens} tokens no prompt", enabled.len()), fg(GRAY)),
    ];
    f.render_widget(Paragraph::new(Line::from(head)), title);

    // Caixa arredondada com a busca, como no Claude Code.
    let box_style = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(fg(GRAY));
    let search_text = if p.query.is_empty() {
        Line::from(Span::styled("⌕ Buscar skills…", fg(GRAY)))
    } else {
        Line::from(vec![Span::styled("⌕ ", fg(GRAY)), Span::raw(p.query.clone())])
    };
    f.render_widget(Paragraph::new(search_text).block(box_style), search);

    // Largura da coluna de nomes: a maior entre todas as skills, para alinhar os metadados.
    let name_w = settings.skills.iter().map(|s| s.name.chars().count()).max().unwrap_or(0).clamp(12, 36);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if rows.is_empty() {
        lines.push(Line::from(Span::styled("  nenhuma skill encontrada", fg(GRAY))));
    } else {
        let start = if p.index >= PICKER_VISIBLE { p.index + 1 - PICKER_VISIBLE } else { 0 };
        for (pos, &i) in rows.iter().enumerate().skip(start).take(PICKER_VISIBLE) {
            let skill = &settings.skills[i];
            let selected = pos == p.index;
            let on = !off.contains(&skill.name);
            let origin = if skill.builtin { "embutida" } else { "projeto" };
            let (cursor, name_style) = if selected {
                ("❯ ", bold(YELLOW))
            } else if on {
                ("  ", Style::default())
            } else {
                ("  ", fg(GRAY))
            };
            let mark = if on { "✔ " } else { "○ " };
            let mark_style = if on { fg(YELLOW) } else { fg(GRAY) };
            lines.push(Line::from(vec![
                Span::styled(cursor, fg(YELLOW)),
                Span::styled(mark, mark_style),
                Span::styled(format!("{:<name_w$}", skill.name), name_style),
                Span::styled(
                    format!("  {origin} · ~{} tokens", skills::catalog_tokens(skill)),
                    fg(GRAY),
                ),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines), list);

    let help = "↑/↓ navega · espaço liga/desliga · digite para buscar · tab ordena · esc fecha";
    f.render_widget(Paragraph::new(Line::from(Span::styled(help, fg(GRAY)))), hint);
}

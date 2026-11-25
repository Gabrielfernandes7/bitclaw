//! O que a interface desenha: a área fixa embaixo e os painéis.

mod info;
mod model_picker;
mod picker;
mod status;

pub(in crate::tui) use self::info::*;
pub(in crate::tui) use self::model_picker::*;
pub(in crate::tui) use self::picker::*;
pub(in crate::tui) use self::status::*;

use super::brand::{self, Mood, TRICK_MS};
use super::commands::keyword_ranges;
use super::{Cmd, MAX_EDITOR_ROWS, Settings, Work, locked, theme::*};
use bitclaw::context::Usage;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui_textarea::TextArea;
use std::time::Instant;

// Aviso: rótulo "aviso:" em laranja, sem glifo.
pub(in crate::tui) fn aviso_line(text: &str) -> Line<'static> {
    Line::from(vec![Span::styled("  aviso: ", bold(ORANGE)), Span::styled(text.to_string(), fg(ORANGE))])
}

// Linha cinza com o recuo padrão.
pub(in crate::tui) fn note(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {text}"), fg(GRAY)))
}

// área fixa embaixo

// Altura da faixa do mascote.
pub(in crate::tui) const MASCOT_ROWS: u16 = 3;

// Tudo o que `draw` precisa, emprestado do estado da interface.
pub(in crate::tui) struct View<'a> {
    pub(in crate::tui) editor: &'a TextArea<'static>,
    pub(in crate::tui) work: Option<&'a Work>,
    pub(in crate::tui) usage: Option<Usage>,
    pub(in crate::tui) settings: &'a Settings,
    pub(in crate::tui) menu: Vec<&'static Cmd>,
    pub(in crate::tui) menu_index: usize,
    pub(in crate::tui) picker: Option<&'a Picker>,
    pub(in crate::tui) model_picker: Option<&'a ModelPicker>,
    pub(in crate::tui) mascot: Mascot,
}

// O estado do mascote e há quanto tempo ele dura, em ms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::tui) struct Mascot {
    pub(in crate::tui) mood: Mood,
    pub(in crate::tui) ms: u128,
}

// O número que o mascote faz agora: o que foi disparado, enquanto durar. Fora disso ele fica parado.
pub(in crate::tui) fn mascot_for(play: Option<(Instant, Mood)>) -> Mascot {
    match play {
        Some((at, mood)) if at.elapsed().as_millis() < TRICK_MS => Mascot { mood, ms: at.elapsed().as_millis() },
        _ => Mascot { mood: Mood::Idle, ms: 0 },
    }
}

// Onde o mascote fica dentro da área fixa. Em janelas estreitas ele some e só sobra o status.
pub(in crate::tui) fn mascot_rect(area: Rect) -> Option<Rect> {
    (area.width >= brand::WIDTH + 2 + 12 && area.height >= MASCOT_ROWS)
        .then(|| Rect::new(area.x, area.y, brand::WIDTH, MASCOT_ROWS))
}

pub(in crate::tui) fn draw(f: &mut Frame, v: &View) {
    if let Some(models) = v.model_picker {
        draw_model_picker(f, v.settings, models);
        return;
    }
    if let Some(picker) = v.picker {
        draw_picker(f, v.settings, picker);
        return;
    }
    let editor_rows = v.editor.lines().len().clamp(1, MAX_EDITOR_ROWS) as u16;
    let [status, composer, menu, footer] = Layout::vertical([
        Constraint::Length(MASCOT_ROWS),
        Constraint::Length(editor_rows + 2),
        Constraint::Length(v.menu.len() as u16),
        Constraint::Length(1),
    ])
    .areas(f.area());

    // Mascote à esquerda; o texto do turno fica ao lado, na linha do meio.
    let text_area = match mascot_rect(status) {
        Some(rect) => {
            let lines: Vec<Line> = brand::frame(v.mascot.mood, v.mascot.ms)
                .into_iter()
                .map(|l| Line::from(Span::styled(l, fg(YELLOW))))
                .collect();
            f.render_widget(Paragraph::new(lines), rect);
            Rect::new(status.x + brand::WIDTH + 2, status.y, status.width - brand::WIDTH - 2, status.height)
        }
        None => status,
    };
    if v.work.is_none() {
        // Ocioso: o nome, o projeto e o modelo ao lado do mascote.
        let active = locked(&v.settings.model).id.clone();
        let lines = info_lines(&v.settings.title, &v.settings.info, &active, &v.settings.default_model);
        f.render_widget(Paragraph::new(lines), text_area);
    } else {
        let [_, middle, _] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)]).areas(text_area);
        f.render_widget(Paragraph::new(status_line(v.work)), middle);
    }

    // Editor delimitado por réguas, como no Claude Code.
    let block = Block::new().borders(Borders::TOP | Borders::BOTTOM).border_style(fg(GRAY));
    let inner = block.inner(composer);
    f.render_widget(block, composer);
    let [mark, text] = Layout::horizontal([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new("›").style(bold(YELLOW)), mark);
    f.render_widget(v.editor, text);
    highlight_keywords(f, v.editor, text);

    // Menu de comandos, abaixo do editor.
    let chosen = v.menu_index.min(v.menu.len().saturating_sub(1));
    let rows: Vec<Line> = v
        .menu
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (mark, name) = if i == chosen { ("› ", bold(YELLOW)) } else { ("  ", fg(GRAY)) };
            Line::from(vec![
                Span::styled(mark, name),
                Span::styled(format!("{:<8} ", c.name), name),
                Span::styled(c.desc, fg(GRAY)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(rows), menu);

    // Rodapé: modelo, modo e contexto à esquerda; atalhos à direita, na versão que couber.
    let model_id = locked(&v.settings.model).id.clone();
    let mut info = vec![Span::styled(
        format!("{} · {} · ", model_id, if v.settings.auto_yes { "Automático" } else { "Manual" }),
        fg(GRAY),
    )];
    let effort = *locked(&v.settings.effort);
    if !effort.is_off() {
        info.push(Span::styled(format!("esforço {}", effort.as_str()), fg(YELLOW)));
        info.push(Span::styled(" · ", fg(GRAY)));
    }
    info.push(Span::styled("contexto ", fg(GRAY)));
    match v.usage {
        Some(u) => {
            let color = if u.percent >= 90.0 { RED } else if u.percent >= 70.0 { YELLOW } else { GREEN };
            info.push(Span::styled(format!("{:.0}%", u.percent), fg(color)));
            info.push(Span::styled(format!(" ({}/{})", u.tokens, u.window), fg(GRAY)));
        }
        None => info.push(Span::styled("—", fg(GRAY))),
    }
    let info_width: usize = info.iter().map(Span::width).sum();
    let room = (footer.width as usize).saturating_sub(info_width + 2);
    let hints: &[&str] = match v.work {
        _ if !v.menu.is_empty() => &["↑↓ escolhe · tab completa · enter executa", "tab completa · enter executa", ""],
        None => &["enter envia · shift+enter nova linha · / comandos", "enter envia · / comandos", ""],
        Some(w) if w.approval.is_some() => &["s aplica · n recusa", ""],
        Some(_) => &["esc interrompe", ""],
    };
    let hint = hints.iter().find(|h| h.chars().count() <= room).copied().unwrap_or("");
    let [left, right] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(hint.chars().count() as u16)]).areas(footer);
    f.render_widget(Paragraph::new(Line::from(info)), left);
    f.render_widget(Paragraph::new(hint).style(fg(GRAY)), right);
}

// Destaca no editor as palavras reservadas: o comando e o subcomando de `/model`. Só vale quando a
// primeira linha está visível, isto é, o editor ainda não rolou.
fn highlight_keywords(f: &mut Frame, editor: &TextArea<'static>, area: Rect) {
    let lines = editor.lines();
    if lines.len() > MAX_EDITOR_ROWS || area.height == 0 {
        return;
    }
    for (start, end) in keyword_ranges(&lines[0]) {
        let (start, end) = (start as u16, (end as u16).min(area.width));
        if start >= end {
            continue;
        }
        f.buffer_mut().set_style(Rect::new(area.x + start, area.y, end - start, 1), bold(KEYWORD));
    }
}

//! O que a interface desenha: a área fixa embaixo e os painéis.

mod model_picker;
mod picker;

pub(in crate::tui) use self::model_picker::*;
pub(in crate::tui) use self::picker::*;

use super::{Cmd, MAX_EDITOR_ROWS, Settings, Work, locked, theme::*};
use bitclaw::context::Usage;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui_textarea::TextArea;

// Aviso: rótulo "aviso:" em laranja, sem glifo.
pub(in crate::tui) fn aviso_line(text: &str) -> Line<'static> {
    Line::from(vec![Span::styled("  aviso: ", bold(ORANGE)), Span::styled(text.to_string(), fg(ORANGE))])
}

// Linha cinza com o recuo padrão.
pub(in crate::tui) fn note(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {text}"), fg(GRAY)))
}

// ---------- área fixa embaixo ----------

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
        Constraint::Length(1),
        Constraint::Length(editor_rows + 2),
        Constraint::Length(v.menu.len() as u16),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(Paragraph::new(status_line(v.work)), status);

    // Editor delimitado por réguas, como no Claude Code.
    let block = Block::new().borders(Borders::TOP | Borders::BOTTOM).border_style(fg(GRAY));
    let inner = block.inner(composer);
    f.render_widget(block, composer);
    let [mark, text] = Layout::horizontal([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new("›").style(bold(YELLOW)), mark);
    f.render_widget(v.editor, text);

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

// Spinner e atividade do turno, ou a pergunta de aprovação.
pub(in crate::tui) fn status_line(work: Option<&Work>) -> Line<'static> {
    let Some(work) = work else { return Line::default() };
    if work.approval.is_some() {
        return Line::from(Span::styled("▸ Aplicar esta alteração? [s] sim · [n] não", bold(YELLOW)));
    }
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let elapsed = work.started.elapsed();
    let frame = FRAMES[(elapsed.as_millis() / 80) as usize % FRAMES.len()];
    let label = if work.cancelling {
        "interrompendo, aguardando o modelo".to_string()
    } else {
        work.activity.clone()
    };
    Line::from(vec![
        Span::styled(format!("{frame} "), fg(YELLOW)),
        Span::styled(format!("{label}… {}s", elapsed.as_secs()), fg(GRAY)),
    ])
}

//! O que a interface desenha: a conversa rolável em cima e, fixos embaixo, o editor ou um painel.

mod effort_panel;
mod info;
mod keys;
mod model_picker;
mod picker;
mod status;
mod status_panel;

pub(in crate::app::tui) use self::effort_panel::*;
pub(in crate::app::tui) use self::info::*;
pub(in crate::app::tui) use self::model_picker::*;
pub(in crate::app::tui) use self::picker::*;
pub(in crate::app::tui) use self::status::*;
pub(in crate::app::tui) use self::status_panel::*;

use super::brand::{self, Mood, TRICK_MS};
use super::commands::keyword_ranges;
use super::mention::Entry;
use super::transcript::Transcript;
use super::{Cmd, MAX_EDITOR_ROWS, Settings, Work, locked, theme::*};
use bitclaw::context::Usage;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui_textarea::TextArea;
use std::time::Instant;

// Aviso: rótulo "aviso:" em laranja, sem glifo.
pub(in crate::app::tui) fn aviso_line(text: &str) -> Line<'static> {
    Line::from(vec![Span::styled("  aviso: ", bold(ORANGE)), Span::styled(text.to_string(), fg(ORANGE))])
}

// Linha cinza com o recuo padrão.
pub(in crate::app::tui) fn note(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {text}"), fg(GRAY)))
}

// área fixa embaixo

// Altura da faixa do mascote.
pub(in crate::app::tui) const MASCOT_ROWS: u16 = 3;

// Linhas em branco acima do mascote, para o cabeçalho não encostar no topo do terminal.
pub(in crate::app::tui) const TOP_GAP: u16 = 1;

// Tudo o que `draw` precisa, emprestado do estado da interface.
pub(in crate::app::tui) struct View<'a> {
    pub(in crate::app::tui) editor: &'a TextArea<'static>,
    pub(in crate::app::tui) work: Option<&'a Work>,
    pub(in crate::app::tui) usage: Option<Usage>,
    pub(in crate::app::tui) settings: &'a Settings,
    pub(in crate::app::tui) menu: Vec<&'static Cmd>,
    pub(in crate::app::tui) menu_index: usize,
    pub(in crate::app::tui) picker: Option<&'a Picker>,
    pub(in crate::app::tui) model_picker: Option<&'a ModelPicker>,
    // Painel `/effort`, se estiver aberto.
    pub(in crate::app::tui) effort_panel: Option<&'a EffortPanel>,
    // Painel `/status`, se estiver aberto, e as estatísticas que ele mostra.
    pub(in crate::app::tui) status_panel: Option<&'a StatusPanel>,
    pub(in crate::app::tui) stats: &'a SessionStats,
    // A conversa, já quebrada na largura da janela.
    pub(in crate::app::tui) transcript: &'a Transcript,
    // Aviso curto no rodapé (ex.: "copiado"), no lugar dos atalhos, por alguns instantes.
    pub(in crate::app::tui) flash: Option<&'a str>,
    // Linhas reservadas ao que o modelo está gerando agora.
    pub(in crate::app::tui) live_rows: usize,
    // Sugestões de arquivo para o `@` sob o cursor. Vazio quando não há menção.
    pub(in crate::app::tui) mentions: Vec<Entry>,
}

// O estado do mascote e há quanto tempo ele dura, em ms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app::tui) struct Mascot {
    pub(in crate::app::tui) mood: Mood,
    pub(in crate::app::tui) ms: u128,
}

// O número que o mascote faz agora: o que foi disparado, enquanto durar. Fora disso ele fica parado.
pub(in crate::app::tui) fn mascot_for(play: Option<(Instant, Mood)>) -> Mascot {
    match play {
        Some((at, mood)) if at.elapsed().as_millis() < TRICK_MS => Mascot { mood, ms: at.elapsed().as_millis() },
        _ => Mascot { mood: Mood::Idle, ms: 0 },
    }
}

// Onde o mascote fica dentro da área fixa. Em janelas estreitas ele some e só sobra o status.
pub(in crate::app::tui) fn mascot_rect(area: Rect) -> Option<Rect> {
    (area.width >= brand::WIDTH + 2 + 12 && area.height >= MASCOT_ROWS)
        .then(|| Rect::new(area.x, area.y, brand::WIDTH, MASCOT_ROWS))
}

// Linhas da abertura (a folga e o mascote com o nome, o projeto e o modelo ao lado). São as primeiras
// linhas da conversa: durante a saudação o mascote anima ali mesmo, e depois elas rolam como o resto.
pub(in crate::app::tui) const BANNER_ROWS: usize = (TOP_GAP + MASCOT_ROWS) as usize;

// Altura da parte fixa embaixo: réguas e editor, menu, rodapé, a linha do passo e o que o modelo gera.
pub(in crate::app::tui) fn viewport_height(editor_rows: usize, menu_rows: usize, summary_rows: usize, extra_rows: usize) -> u16 {
    (editor_rows.clamp(1, MAX_EDITOR_ROWS) + 2 + menu_rows + 1 + summary_rows + extra_rows) as u16
}

// Nome, projeto e modelo, com os tokens gastos na sessão na linha do modelo. A linha do modelo é
// montada a cada quadro: o modelo, o esforço e a janela de agora, não os do início.
fn header_lines(settings: &Settings, stats: &SessionStats) -> Vec<Line<'static>> {
    let active = locked(&settings.model).id.clone();
    let effort = *locked(&settings.effort);
    let window = locked(&settings.options).num_ctx;
    // Os dados do provedor (tamanho, quantização) são do modelo do início; trocado o modelo, não valem.
    let details = if active == settings.default_model { settings.details.as_str() } else { "" };
    let model = crate::app::header::model_line(&active, effort, &settings.provider, details, window);
    let mut lines = info_lines(&settings.title, &settings.info, model);
    let full = lines.len() == INFO_ROWS;
    if stats.tokens > 0
        && full
        && let Some(model) = lines.last_mut()
    {
        model.spans.push(Span::styled(format!(" · {} tokens gastos", stats.tokens), fg(GRAY)));
    }
    lines
}

// A abertura: `BANNER_ROWS` linhas, com o mascote à esquerda do texto. Em janela estreita o mascote some.
// O segundo valor diz se o mascote foi desenhado.
pub(in crate::app::tui) fn banner_lines(settings: &Settings, stats: &SessionStats, mascot: Mascot, width: u16) -> (Vec<Line<'static>>, bool) {
    let mut lines = vec![Line::default(); TOP_GAP as usize];
    let head = header_lines(settings, stats);
    let with_mascot = mascot_rect(Rect::new(0, 0, width, MASCOT_ROWS)).is_some();
    if with_mascot {
        for (i, art) in brand::frame(mascot.mood, mascot.ms).into_iter().enumerate().take(MASCOT_ROWS as usize) {
            let mut spans = vec![Span::styled(art, fg(YELLOW)), Span::raw("  ")];
            if let Some(h) = head.get(i) {
                spans.extend(h.spans.iter().cloned());
            }
            lines.push(Line::from(spans));
        }
    } else {
        lines.extend(head.into_iter().take(MASCOT_ROWS as usize));
    }
    lines.resize(BANNER_ROWS, Line::default());
    (lines, with_mascot)
}

// Altura do painel aberto, se houver, com a régua que o separa da conversa.
fn panel_height(v: &View) -> Option<u16> {
    let rows = if v.effort_panel.is_some() {
        EFFORT_PANEL_ROWS
    } else if let Some(mp) = v.model_picker {
        picker_height(model_rows(mp).len())
    } else if let Some(p) = v.picker {
        picker_height(picker_rows(&v.settings.skills, p).len())
    } else if v.status_panel.is_some() {
        STATUS_PANEL_ROWS
    } else {
        return None;
    };
    Some(rows + 1)
}

// Altura da parte fixa embaixo: o painel aberto, ou o editor com o menu, o passo e o que o modelo gera.
pub(in crate::app::tui) fn bottom_height(v: &View) -> u16 {
    panel_height(v).unwrap_or_else(|| {
        let menu = v.menu.len().max(v.mentions.len());
        viewport_height(v.editor.lines().len(), menu, usize::from(v.work.is_some()), v.live_rows)
    })
}

// Desenha a tela: a conversa em cima e, fixos embaixo, o painel aberto ou o editor. Devolve a área da
// conversa, para o mouse saber o que foi clicado.
pub(in crate::app::tui) fn draw(f: &mut Frame, v: &View) -> Rect {
    let area = f.area();
    let bottom = bottom_height(v).min(area.height);
    let [chat, fixed] = Layout::vertical([Constraint::Min(0), Constraint::Length(bottom)]).areas(area);
    draw_chat(f, v, chat);
    if panel_height(v).is_some() {
        let [rule, panel] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(fixed);
        f.render_widget(Paragraph::new("─".repeat(rule.width as usize)).style(fg(GRAY)), rule);
        draw_panel(f, v, panel);
    } else {
        draw_composer(f, v, fixed);
    }
    chat
}

fn draw_panel(f: &mut Frame, v: &View, area: Rect) {
    if let Some(panel) = v.effort_panel {
        draw_effort_panel(f, area, panel);
    } else if let Some(models) = v.model_picker {
        draw_model_picker(f, area, v.settings, models);
    } else if let Some(picker) = v.picker {
        draw_picker(f, area, v.settings, picker);
    } else if let Some(panel) = v.status_panel {
        let active = locked(&v.settings.model).id.clone();
        let effort = locked(&v.settings.effort).as_str().to_string();
        let snapshot = Snapshot { settings: v.settings, model: active, effort, usage: v.usage, stats: v.stats };
        draw_status_panel(f, area, &snapshot, panel);
    }
}

// A conversa. Rolada para cima, a última linha vira um aviso de como voltar ao fim.
fn draw_chat(f: &mut Frame, v: &View, area: Rect) {
    if area.height == 0 {
        return;
    }
    f.render_widget(Paragraph::new(v.transcript.visible(area.height as usize)), area);
    let back = v.transcript.scroll_back.min(v.transcript.total_rows().saturating_sub(area.height as usize));
    if back > 0 {
        let text = format!(" ↓ {back} linhas abaixo · PageDown ou a roda do mouse voltam ao fim ");
        let width = (text.chars().count() as u16).min(area.width);
        let at = Rect::new(area.x + area.width - width, area.y + area.height - 1, width, 1);
        f.render_widget(Paragraph::new(text).style(fg(YELLOW).add_modifier(ratatui::style::Modifier::REVERSED)), at);
    }
}

// O editor e o que fica junto dele: o que o modelo gera, o passo, o menu e o rodapé.
fn draw_composer(f: &mut Frame, v: &View, area: Rect) {
    let editor_rows = v.editor.lines().len().clamp(1, MAX_EDITOR_ROWS) as u16;
    // Enquanto o agente trabalha, uma linha com o passo atual fica logo acima do editor.
    let summary_rows = if v.work.is_some() { 1 } else { 0 };
    let [live_area, summary, composer, menu, footer] = Layout::vertical([
        Constraint::Length(v.live_rows as u16),
        Constraint::Length(summary_rows),
        Constraint::Length(editor_rows + 2),
        Constraint::Length(v.menu.len().max(v.mentions.len()) as u16),
        Constraint::Length(1),
    ])
    .areas(area);

    // O que o modelo está gerando agora, encostado embaixo da sua área (logo acima do passo).
    let live = live_lines(v.work, live_area.width);
    let shown = live.len().min(live_area.height as usize);
    if shown > 0 {
        let area = Rect::new(live_area.x, live_area.y + live_area.height - shown as u16, live_area.width, shown as u16);
        f.render_widget(Paragraph::new(live[live.len() - shown..].to_vec()), area);
    }
    f.render_widget(Paragraph::new(status_line(v.work)), Rect::new(summary.x + 2, summary.y, summary.width.saturating_sub(2), summary.height));

    // Editor delimitado por réguas, como no Claude Code.
    // Esperando a decisão do usuário: o compositor vira a pergunta, com as réguas em amarelo.
    let deciding = v.work.filter(|w| w.approval.is_some());
    let block = Block::new().borders(Borders::TOP | Borders::BOTTOM).border_style(fg(if deciding.is_some() { YELLOW } else { GRAY }));
    let inner = block.inner(composer);
    f.render_widget(block, composer);
    let [mark, text] = Layout::horizontal([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new("❯").style(bold(YELLOW)), mark);
    if let Some(work) = deciding {
        f.render_widget(Paragraph::new(approval_prompt(work, text.width as usize)), text);
    } else {
        f.render_widget(v.editor, text);
        highlight_keywords(f, v.editor, text);
    }

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

    // Sugestões de arquivo, no mesmo lugar do menu de comandos.
    if !v.mentions.is_empty() {
        let chosen = v.menu_index.min(v.mentions.len() - 1);
        let rows: Vec<Line> = v
            .mentions
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (mark, style) = if i == chosen { ("› ", bold(YELLOW)) } else { ("  ", fg(if e.is_dir { KEYWORD } else { GRAY })) };
                Line::from(vec![Span::styled(mark, style), Span::styled(format!("@{}", e.path), style)])
            })
            .collect();
        f.render_widget(Paragraph::new(rows), menu);
    }

    // Rodapé: modelo, modo e contexto à esquerda; atalhos à direita, na versão que couber.
    let model_id = locked(&v.settings.model).id.clone();
    // O modo só aparece quando é o diferente do normal (`--yes`, "Automático"); o padrão fica implícito.
    let mode = if v.settings.auto_yes { "Automático · " } else { "" };
    let mut info = vec![Span::styled(format!("{model_id} · {mode}"), fg(GRAY))];
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
        _ if !v.mentions.is_empty() => &["↑↓ escolhe · tab ou enter completa · esc fecha", "tab completa · esc fecha", ""],
        _ if !v.menu.is_empty() => &["↑↓ escolhe · tab completa · enter executa", "tab completa · enter executa", ""],
        None => &["enter envia · shift+enter nova linha · / comandos", "enter envia · / comandos", ""],
        // A pergunta e as teclas já estão no compositor; o rodapé não repete.
        Some(w) if w.approval.is_some() => &[""],
        Some(_) => &["esc interrompe", ""],
    };
    let hint = hints.iter().find(|h| h.chars().count() <= room).copied().unwrap_or("");
    // Um aviso curto (como "copiado") toma o lugar dos atalhos por alguns instantes.
    let (hint, hint_style) = match v.flash {
        Some(text) if text.chars().count() <= room => (text, bold(GREEN)),
        _ => (hint, fg(GRAY)),
    };
    let [left, right] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(hint.chars().count() as u16)]).areas(footer);
    f.render_widget(Paragraph::new(Line::from(info)), left);
    f.render_widget(Paragraph::new(hint).style(hint_style), right);
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

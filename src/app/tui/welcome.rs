//! Tela de boas-vindas: aparece na primeira abertura, antes da conversa. Mostra o mascote numa cena
//! noturna, o que o BitClaw conferiu (provedor, modelo, projeto) e as dicas de uso. Enter segue para a
//! conversa; Esc sai. A arte segue `docs/identidade/ARTE-TERMINAL.md`.
//!
//! A marca de "já vista" fica em `~/.config/bitclaw/welcomed`. `BITCLAW_WELCOME=1` mostra de novo.

use super::brand;
use super::theme::{DIM, GRAY, GREEN, RED, YELLOW, bold, fg};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use std::time::{Duration, Instant};

const MARKER: &str = "welcomed";
const MOON: Color = Color::Rgb(230, 226, 211);
const SEA: Color = Color::Rgb(47, 79, 120);
// Largura máxima da cena: acima disso ela fica centralizada, sem esticar.
const SCENE_MAX: usize = 72;
// Cada uma cabe em 40 colunas, com a margem.
const TIPS: [&str; 4] = ["@ cita arquivos · / abre os comandos", "/undo desfaz o último turno", "bitclaw --plan só lê, sem alterar nada", "bitclaw update busca a versão nova"];

// Uma linha da lista do que foi conferido na abertura.
pub struct Check {
    pub ok: bool,
    pub text: String,
}

// Se a tela deve aparecer: na primeira abertura, ou quando pedida com `BITCLAW_WELCOME=1`.
pub fn should_show() -> bool {
    if std::env::var_os("BITCLAW_WELCOME").is_some_and(|v| v == "1") {
        return true;
    }
    crate::app::update::config_dir().is_some_and(|d| !d.join(MARKER).exists())
}

fn mark_shown() {
    if let Some(dir) = crate::app::update::config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(MARKER), "");
    }
}

// Mostra a tela até o usuário escolher. `Ok(true)` = seguir para a conversa, `Ok(false)` = sair.
pub fn show(checks: &[Check], color: bool) -> Result<bool, String> {
    super::theme::set_color(color);
    let mut terminal = ratatui::init();
    let start = Instant::now();
    let result = (|| -> std::io::Result<bool> {
        loop {
            let ms = start.elapsed().as_millis();
            terminal.draw(|f| {
                let area = f.area();
                let lines = screen(area.width as usize, area.height as usize, checks, ms);
                let top = (area.height as usize).saturating_sub(lines.len()) / 2;
                let body = Rect::new(area.x, area.y + top as u16, area.width, area.height.saturating_sub(top as u16));
                // Linha mais longa que um terminal estreito quebra, em vez de sumir pela direita.
                f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
            })?;
            if event::poll(Duration::from_millis(120))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                match k.code {
                    KeyCode::Enter | KeyCode::Char(' ') => return Ok(true),
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(false),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(false),
                    _ => {}
                }
            }
        }
    })();
    ratatui::restore();
    let go_on = result.map_err(|e| e.to_string())?;
    if go_on {
        mark_shown();
    }
    Ok(go_on)
}

// Escala do mascote para o tamanho do terminal: 2 com espaço de sobra, 1 no comum, 0 = sem cena.
fn scale_for(width: usize, height: usize) -> usize {
    if width >= 64 && height >= 36 {
        2
    } else if width >= 40 && height >= 24 {
        1
    } else {
        0
    }
}

// Uma célula da cena: caractere e cor. Espaço é transparente.
type Canvas = Vec<Vec<(char, Color)>>;

fn put(cv: &mut Canvas, x: usize, y: usize, art: &[String], color: Color) {
    for (dy, line) in art.iter().enumerate() {
        for (dx, ch) in line.chars().enumerate() {
            if ch != ' '
                && let Some(cell) = cv.get_mut(y + dy).and_then(|r| r.get_mut(x + dx))
            {
                *cell = (ch, color);
            }
        }
    }
}

// Céu com estrelas, lua crescente, mar ao fundo, o mascote e o chão pontilhado. O mar é desenhado antes
// do mascote: passa por trás dele e aparece entre as patas.
fn scene(width: usize, scale: usize, ms: u128) -> Vec<Line<'static>> {
    let w = width.min(SCENE_MAX);
    let crab = brand::big_crab(scale, !blinking(ms));
    let sky = if scale == 2 { 6 } else { 4 };
    let h = sky + crab.len() + 1;
    let mut cv: Canvas = vec![vec![(' ', GRAY); w]; h];
    // Estrelas em posições fixas (fração da largura), algumas piscando.
    const STARS: [(usize, usize); 11] = [(4, 1), (17, 3), (30, 0), (42, 2), (56, 1), (69, 4), (94, 2), (11, 5), (52, 4), (89, 6), (37, 6)];
    for (i, (fx, y)) in STARS.iter().enumerate() {
        // No mesmo ciclo de 4 s da piscada: a cena inteira se repete.
        let twinkle = i % 4 == 0 && ((ms % 4000) / 1000 + i as u128) % 3 == 0;
        let star = if twinkle || (fx + y) % 3 == 0 { '.' } else { '*' };
        if let Some(cell) = cv.get_mut(*y).and_then(|r| r.get_mut(fx * w / 100)) {
            *cell = (star, GRAY);
        }
    }
    put(&mut cv, w.saturating_sub(14), 1, &brand::moon(), MOON);
    let sea_row = sky + crab.len() / 2;
    for x in (0..w).step_by(2) {
        cv[sea_row][x] = ('~', SEA);
    }
    put(&mut cv, w / 8, sky, &crab, YELLOW);
    for cell in cv[h - 1].iter_mut() {
        *cell = ('.', DIM);
    }
    let pad = " ".repeat(width.saturating_sub(w) / 2);
    cv.into_iter()
        .map(|row| {
            let mut spans = vec![Span::raw(pad.clone())];
            spans.extend(row.into_iter().map(|(ch, color)| Span::styled(ch.to_string(), fg(color))));
            Line::from(spans)
        })
        .collect()
}

// Pisca a cada 4 segundos, por 150 ms.
fn blinking(ms: u128) -> bool {
    (2600..2750).contains(&(ms % 4000))
}

// A tela inteira: cena (se couber), título, o que foi conferido, dicas e as teclas. Num terminal baixo
// demais, as dicas saem primeiro: o que foi conferido e as teclas ficam sempre.
fn screen(width: usize, height: usize, checks: &[Check], ms: u128) -> Vec<Line<'static>> {
    let full = compose(width, height, checks, ms, true);
    if full.len() <= height { full } else { compose(width, height, checks, ms, false) }
}

fn compose(width: usize, height: usize, checks: &[Check], ms: u128, tips: bool) -> Vec<Line<'static>> {
    let scale = scale_for(width, height);
    let mut lines = if scale > 0 { scene(width, scale, ms) } else { Vec::new() };
    let pad = " ".repeat(width.saturating_sub(SCENE_MAX.min(width)) / 2 + 2);
    let text = |spans: Vec<Span<'static>>| {
        let mut all = vec![Span::raw(pad.clone())];
        all.extend(spans);
        Line::from(all)
    };
    lines.push(Line::default());
    lines.push(text(vec![Span::styled("Boas-vindas ao BitClaw", bold(YELLOW))]));
    lines.push(text(vec![Span::styled("agente de código com modelos locais", fg(GRAY))]));
    lines.push(Line::default());
    for c in checks {
        let (mark, color) = if c.ok { ("✓ ", GREEN) } else { ("✗ ", RED) };
        lines.push(text(vec![Span::styled(mark, fg(color)), Span::raw(c.text.clone())]));
    }
    if tips {
        lines.push(Line::default());
        for tip in TIPS {
            lines.push(text(vec![Span::styled(tip, fg(GRAY))]));
        }
    }
    lines.push(Line::default());
    lines.push(text(vec![Span::styled("enter", bold(YELLOW)), Span::styled(" começa · ", fg(GRAY)), Span::styled("esc", bold(YELLOW)), Span::styled(" sai", fg(GRAY))]));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    fn checks() -> Vec<Check> {
        vec![Check { ok: true, text: "Ollama respondendo".into() }, Check { ok: false, text: "modelo x não está".into() }]
    }

    #[test]
    fn the_scene_grows_with_the_terminal_and_disappears_when_it_does_not_fit() {
        assert_eq!(scale_for(120, 40), 2);
        assert_eq!(scale_for(80, 24), 1);
        assert_eq!(scale_for(39, 30), 0, "estreito demais");
        assert_eq!(scale_for(80, 20), 0, "baixo demais");
    }

    #[test]
    fn every_size_fits_the_terminal_and_keeps_the_text() {
        for (w, h) in [(120, 40), (80, 24), (64, 36), (40, 24), (30, 12)] {
            let lines = screen(w, h, &checks(), 0);
            assert!(lines.len() <= h, "{w}x{h}: {} linhas", lines.len());
            // Os textos fixos cabem em 40 colunas; abaixo disso (e para checagens longas) a tela quebra a linha.
            if w >= 40 {
                assert!(lines.iter().all(|l| l.width() <= w), "{w}x{h}: linha mais larga que a tela");
            }
            let text = plain(&lines).join("\n");
            assert!(text.contains("Boas-vindas ao BitClaw") && text.contains("enter"), "{w}x{h}");
            assert!(text.contains("✓ Ollama respondendo") && text.contains("✗ modelo x"), "{w}x{h}");
        }
    }

    #[test]
    fn the_scene_has_the_mascot_the_moon_the_sea_and_the_ground() {
        let rows = plain(&scene(80, 1, 0));
        assert_eq!(rows.len(), 4 + 6 + 1);
        let all = rows.join("\n");
        assert!(all.contains('~') && all.contains('*'));
        assert!(rows.last().unwrap().trim_start().chars().all(|c| c == '.'), "chão pontilhado");
        let crab = brand::big_crab(1, true);
        assert!(rows[4].contains(crab[0].trim()), "o mascote começa logo abaixo do céu");
    }

    #[test]
    fn the_mascot_blinks() {
        assert_ne!(plain(&scene(80, 1, 0)), plain(&scene(80, 1, 2650)));
        assert_eq!(plain(&scene(80, 1, 0)), plain(&scene(80, 1, 4000)), "o ciclo se repete");
    }
}

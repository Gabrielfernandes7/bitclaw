//! Cores e estilos da interface. `--no-color` e `NO_COLOR` desligam todas.

use super::markdown::Palette;
use ratatui::style::{Color, Modifier, Style};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const YELLOW: Color = Color::Rgb(255, 204, 0);
pub(super) const GRAY: Color = Color::Rgb(150, 150, 150);
pub(super) const GREEN: Color = Color::Rgb(0, 200, 120);
pub(super) const RED: Color = Color::Rgb(255, 77, 77);
pub(super) const ORANGE: Color = Color::Rgb(255, 150, 60);
pub(super) const BLUE: Color = Color::Rgb(70, 150, 255);
pub(super) const KEYWORD: Color = Color::Rgb(110, 200, 255);
pub(super) const CODE: Color = Color::Rgb(200, 200, 200);

static COLOR: AtomicBool = AtomicBool::new(true);

// Estilo com a cor pedida, ou sem cor se `--no-color` ou `NO_COLOR` estiverem ativos.
pub(super) fn fg(color: Color) -> Style {
    if COLOR.load(Ordering::Relaxed) { Style::default().fg(color) } else { Style::default() }
}

pub(super) fn bold(color: Color) -> Style {
    fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn palette() -> Palette {
    Palette {
        text: Style::default(),
        code: fg(CODE),
        heading: bold(YELLOW),
        bullet: fg(YELLOW),
    }
}

// Liga ou desliga as cores para o resto do programa.
pub(super) fn set_color(on: bool) {
    COLOR.store(on, Ordering::Relaxed);
}

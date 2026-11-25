//! Status do turno: o texto com pulso e a torre de blocos de montar que sobe enquanto o agente trabalha.

use super::super::Work;
use super::super::theme::*;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

// Cores dos blocos, na ordem em que aparecem: amarelo, azul, verde e vermelho.
const PALETTE: [Color; 4] = [YELLOW, BLUE, GREEN, RED];

// Largura da torre em colunas.
pub(in crate::tui) const TOWER_WIDTH: u16 = 6;

const STEP_MS: u128 = 140;
const CYCLE_FRAMES: u128 = 12;

// A torre num instante: a cor de cada linha (de cima para baixo) e se ela está piscando.
// Em cada ciclo caem três blocos, um após o outro; a torre completa pisca e recomeça.
// Quadros do ciclo: 0-2 o 1º bloco cai; 3-4 o 2º cai; 5 o 3º encaixa; 6-10 torre completa
// (6 e 7 piscam); 11 vazio.
fn tower(ms: u128) -> ([Option<Color>; 3], bool) {
    let frame = ms / STEP_MS;
    let (cycle, i) = ((frame / CYCLE_FRAMES) as usize, (frame % CYCLE_FRAMES) as usize);
    let color = |k: usize| Some(PALETTE[(cycle * 3 + k) % PALETTE.len()]);
    let mut rows = [None; 3];
    match i {
        0..=2 => rows[i] = color(0),
        3..=4 => {
            rows[2] = color(0);
            rows[i - 3] = color(1);
        }
        5..=10 => {
            rows = [color(2), color(1), color(0)];
        }
        _ => {}
    }
    (rows, (6..=7).contains(&i))
}

// Três linhas da torre, prontas para desenhar.
pub(in crate::tui) fn tower_lines(ms: u128) -> Vec<Line<'static>> {
    let (rows, flash) = tower(ms);
    rows.iter()
        .map(|row| match row {
            Some(color) => {
                let style = if flash { bold(WHITE) } else { fg(*color) };
                Line::from(Span::styled("▐████▌", style))
            }
            None => Line::from(" ".repeat(TOWER_WIDTH as usize)),
        })
        .collect()
}

// Amarelo que respira: vai do escuro ao claro e volta, num ciclo de 1,4 s.
fn pulse(ms: u128) -> Color {
    let phase = (ms % 1400) as f32 / 1400.0;
    let wave = if phase < 0.5 { phase * 2.0 } else { (1.0 - phase) * 2.0 };
    let lerp = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * wave) as u8;
    Color::Rgb(lerp(150, 255), lerp(115, 204), lerp(0, 0))
}

// Glifo que gira e muda de cor a cada passada pela paleta.
fn glyph(ms: u128) -> (&'static str, Color) {
    const GLYPHS: [&str; 8] = ["✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳"];
    let step = (ms / 120) as usize;
    (GLYPHS[step % GLYPHS.len()], PALETTE[(step / GLYPHS.len()) % PALETTE.len()])
}

fn seconds(ms: u128) -> String {
    format!("{}s", ms / 1000)
}

// Atividade do turno com pulso, ou a pergunta de aprovação.
pub(in crate::tui) fn status_line(work: Option<&Work>) -> Line<'static> {
    let Some(work) = work else { return Line::default() };
    if work.approval.is_some() {
        return Line::from(Span::styled("▸ Aplicar esta alteração? [s] sim · [n] não", bold(YELLOW)));
    }
    let ms = work.started.elapsed().as_millis();
    let label = if work.cancelling {
        "interrompendo, aguardando o modelo".to_string()
    } else {
        work.activity.clone()
    };
    let (mark, mark_color) = glyph(ms);
    let model_ms = work.model_time().as_millis();
    let mut detail = seconds(ms);
    if model_ms >= 1000 {
        detail.push_str(&format!(" · modelo {}", seconds(model_ms)));
    }
    Line::from(vec![
        Span::styled(format!("{mark} "), bold(mark_color)),
        Span::styled(format!("{label}…"), Style::default().patch(bold(pulse(ms)))),
        Span::styled(format!(" ({detail})"), fg(GRAY)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(ms: u128) -> usize {
        tower(ms).0.iter().flatten().count()
    }

    #[test]
    fn the_tower_grows_one_block_at_a_time_then_resets() {
        let at = |frame: u128| filled(frame * STEP_MS);
        assert_eq!((at(0), at(2)), (1, 1), "o primeiro bloco cai");
        assert_eq!((at(3), at(4)), (2, 2), "o segundo cai");
        assert_eq!((at(5), at(10)), (3, 3), "o terceiro encaixa e a torre fica");
        assert_eq!(at(11), 0, "recomeça do vazio");
        assert_eq!(at(12), 1, "e o ciclo se repete");
    }

    #[test]
    fn a_block_falls_from_the_top_to_the_bottom() {
        let row = |frame: u128| tower(frame * STEP_MS).0.iter().position(|c| c.is_some());
        assert_eq!((row(0), row(1), row(2)), (Some(0), Some(1), Some(2)));
    }

    #[test]
    fn colors_run_through_yellow_blue_green_red_across_cycles() {
        let bottom = |cycle: u128| tower(cycle * CYCLE_FRAMES * STEP_MS + 10 * STEP_MS).0[2];
        let got: Vec<_> = (0..4).map(bottom).collect();
        assert_eq!(got, vec![Some(YELLOW), Some(RED), Some(GREEN), Some(BLUE)]);
        let all: Vec<_> = tower(6 * STEP_MS).0.to_vec();
        assert_eq!(all, vec![Some(GREEN), Some(BLUE), Some(YELLOW)], "1º ciclo: amarelo, azul, verde de baixo para cima");
    }

    #[test]
    fn the_finished_tower_flashes_briefly() {
        assert!(tower(6 * STEP_MS).1 && tower(7 * STEP_MS).1);
        assert!(!tower(5 * STEP_MS).1 && !tower(8 * STEP_MS).1);
    }

    #[test]
    fn tower_lines_are_always_three_rows_of_the_same_width() {
        for ms in (0..6000).step_by(70) {
            let lines = tower_lines(ms);
            assert_eq!(lines.len(), 3);
            assert!(lines.iter().all(|l| l.width() == TOWER_WIDTH as usize), "em {ms} ms");
        }
    }

    #[test]
    fn the_pulse_stays_between_dark_and_bright_yellow() {
        for ms in (0..3000).step_by(50) {
            let Color::Rgb(r, g, b) = pulse(ms) else { panic!("cor inesperada") };
            assert!((150..=255).contains(&r) && (115..=204).contains(&g) && b == 0, "{ms} ms: {r},{g},{b}");
        }
        assert_ne!(pulse(0), pulse(700), "respira");
    }

    #[test]
    fn the_glyph_spins_and_changes_color_each_lap() {
        assert_ne!(glyph(0).0, glyph(120).0);
        assert_eq!(glyph(0).1, PALETTE[0]);
        assert_eq!(glyph(8 * 120).1, PALETTE[1]);
    }
}

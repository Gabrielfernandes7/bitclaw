//! Status do turno: o glifo que gira e o texto com pulso amarelo enquanto o agente trabalha.

use super::super::Work;
use super::super::theme::*;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

// Cores dos blocos, na ordem em que aparecem: amarelo, azul, verde e vermelho.
const PALETTE: [Color; 4] = [YELLOW, BLUE, GREEN, RED];

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

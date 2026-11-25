//! Status do turno: uma lâmpada que pulsa e a fase atual em amarelo, enquanto o agente trabalha.

use super::super::Work;
use super::super::theme::*;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

// Amarelo que respira: vai do escuro ao claro e volta, num ciclo de 1,4 s.
fn pulse(ms: u128) -> Color {
    let phase = (ms % 1400) as f32 / 1400.0;
    let wave = if phase < 0.5 { phase * 2.0 } else { (1.0 - phase) * 2.0 };
    let lerp = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * wave) as u8;
    Color::Rgb(lerp(150, 255), lerp(115, 204), lerp(0, 0))
}

// A lâmpada: um ponto que acende e apaga no mesmo ritmo do pulso. Dá para trocar o glifo aqui.
fn lamp(ms: u128) -> (&'static str, Color) {
    ("●", pulse(ms))
}

fn seconds(ms: u128) -> String {
    format!("{}s", ms / 1000)
}

// Quantas linhas do fim de cada texto a área ao vivo mostra.
const LIVE_TAIL: usize = 4;

// O que o modelo está gerando agora, as últimas linhas do raciocínio e da resposta, antes de
// elas entrarem na conversa. Vazio quando nada está sendo gerado.
pub(in crate::tui) fn live_lines(work: Option<&Work>, width: u16) -> Vec<Line<'static>> {
    let Some(work) = work else { return Vec::new() };
    let room = (width as usize).saturating_sub(6).max(10);
    let mut lines = Vec::new();
    for (title, text) in [("raciocinando", &work.thinking), ("respondendo", &work.answer)] {
        let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if tail.is_empty() {
            continue;
        }
        lines.push(Line::from(Span::styled(format!("  {title}"), bold(GRAY))));
        let from = tail.len().saturating_sub(LIVE_TAIL);
        for line in &tail[from..] {
            // Só o fim da linha, que é o que acabou de chegar.
            let chars: Vec<char> = line.chars().collect();
            let shown: String = chars[chars.len().saturating_sub(room)..].iter().collect();
            lines.push(Line::from(vec![Span::styled("  │ ", fg(GRAY)), Span::styled(shown, fg(GRAY))]));
        }
    }
    lines
}

// Atividade do turno com pulso, ou a pergunta de aprovação.
pub(in crate::tui) fn status_line(work: Option<&Work>) -> Line<'static> {
    let Some(work) = work else { return Line::default() };
    if work.approval.is_some() {
        let question = if work.approval_runs { "Executar este script?" } else { "Aplicar esta alteração?" };
        return Line::from(Span::styled(format!("▸ {question} [s] sim · [n] não"), bold(YELLOW)));
    }
    let ms = work.started.elapsed().as_millis();
    let label = if work.cancelling {
        "interrompendo, aguardando o modelo".to_string()
    } else {
        work.activity.clone()
    };
    let (mark, mark_color) = lamp(ms);
    let model_ms = work.model_time().as_millis();
    let mut detail = seconds(ms);
    if model_ms >= 1000 {
        detail.push_str(&format!(" · modelo {}", seconds(model_ms)));
    }
    if work.tokens > 0 {
        detail.push_str(&format!(" · {} tok", work.tokens));
    }
    if work.steps > 0 {
        let plural = if work.steps == 1 { "passo" } else { "passos" };
        detail.push_str(&format!(" · {} {plural}", work.steps));
    }
    Line::from(vec![
        Span::styled(format!("{mark} "), Style::default().patch(bold(mark_color))),
        Span::styled(format!("{label}…"), bold(YELLOW)),
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
    fn live_lines_show_only_the_tail_of_what_the_model_is_generating() {
        let text = |l: &Line| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        assert!(live_lines(None, 80).is_empty());
        let mut w = Work::new("pensando");
        assert!(live_lines(Some(&w), 80).is_empty(), "nada gerado, nada a mostrar");
        w.thinking = (1..=10).map(|n| format!("passo {n}\n")).collect();
        let lines: Vec<String> = live_lines(Some(&w), 80).iter().map(text).collect();
        assert_eq!(lines[0], "  raciocinando");
        assert_eq!(lines.len(), 1 + LIVE_TAIL, "{lines:?}");
        assert_eq!(lines.last().unwrap(), "  │ passo 10");
        w.answer = "18 dias".into();
        let lines: Vec<String> = live_lines(Some(&w), 80).iter().map(text).collect();
        assert!(lines.iter().any(|l| l == "  respondendo") && lines.last().unwrap() == "  │ 18 dias", "{lines:?}");
    }

    #[test]
    fn the_status_line_counts_the_steps_done_in_this_turn() {
        let text = |w: &Work| status_line(Some(w)).spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        let mut w = Work::new("lendo AGENTS.md");
        assert!(!text(&w).contains("passo"), "sem passos, a linha não fala deles");
        w.steps = 1;
        assert!(text(&w).ends_with("passo)"), "{}", text(&w));
        w.steps = 3;
        assert!(text(&w).ends_with("3 passos)"), "{}", text(&w));
    }

    #[test]
    fn the_lamp_pulses_with_the_same_rhythm_as_the_text() {
        assert_eq!(lamp(0).0, "●", "um só glifo: só a cor pulsa");
        assert_eq!(lamp(700).1, pulse(700));
        assert_ne!(lamp(0).1, lamp(700).1);
    }
}

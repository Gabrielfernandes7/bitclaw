//! A tela do SOUL em tela cheia: o BitClaw feliz com o coração que pulsa, centralizado, e as cinco
//! perguntas uma de cada vez embaixo dele, no mesmo estilo da tela de boas-vindas. Só aparece num
//! terminal de verdade e com a janela de tamanho razoável; sem isso, `soul.rs` pergunta por linhas.
//!
//! As regras das respostas ficam em `Flow`, sem terminal, para os testes. Enter confirma a resposta (vazio
//! fica com o padrão); `pular` dispensa as perguntas; Esc também dispensa e Ctrl+C sai.

use super::theme::{GRAY, RED, YELLOW, bold, fg, set_color};
use crate::app::soul::{Answers, PROMPTS, SKIP_WORD};
use crate::app::soul_art;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

// Abaixo disso a tela não cabe: a versão por linhas toma conta.
const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 16;

// O estado das perguntas: qual é a atual, as respostas dadas e o que está sendo digitado.
#[derive(Debug, Default)]
pub struct Flow {
    step: usize,
    raw: Vec<String>,
    buf: String,
}

// O que uma tecla de Enter decide: seguir para a próxima pergunta, ou terminar (com as respostas, ou dispensadas).
#[derive(Debug, PartialEq)]
pub enum Step {
    Next,
    Done(Option<Answers>),
}

impl Flow {
    pub fn type_char(&mut self, c: char) {
        self.buf.push(c);
    }

    pub fn backspace(&mut self) {
        self.buf.pop();
    }

    // Confirma a resposta da pergunta atual. `pular` em qualquer uma dispensa tudo, como no fluxo por linhas.
    pub fn enter(&mut self) -> Step {
        let text = self.buf.trim().to_string();
        if text.eq_ignore_ascii_case(SKIP_WORD) {
            return Step::Done(None);
        }
        self.raw.push(text);
        self.buf.clear();
        self.step += 1;
        if self.step == PROMPTS.len() { Step::Done(Some(Answers::from_raw(&self.raw))) } else { Step::Next }
    }
}

// Mostra a tela até as perguntas acabarem. `None`: não há terminal ou a janela é pequena, e quem pergunta
// deve usar o fluxo por linhas. `Some(Ok(None))`: a pessoa dispensou. `Some(Ok(Some(..)))`: respondeu tudo.
pub fn run(color: bool) -> Option<io::Result<Option<Answers>>> {
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return None;
    }
    let (width, height) = terminal::size().ok()?;
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return None;
    }
    set_color(color);
    let mut terminal = ratatui::init();
    let start = Instant::now();
    let result = (|| -> io::Result<Option<Answers>> {
        let mut flow = Flow::default();
        loop {
            let ms = start.elapsed().as_millis();
            terminal.draw(|f| draw(f, &flow, ms))?;
            if event::poll(Duration::from_millis(60))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Char('c') if ctrl => return Ok(None),
                    KeyCode::Enter => {
                        if let Step::Done(answers) = flow.enter() {
                            return Ok(answers);
                        }
                    }
                    KeyCode::Backspace => flow.backspace(),
                    KeyCode::Char(c) if !ctrl => flow.type_char(c),
                    _ => {}
                }
            }
        }
    })();
    ratatui::restore();
    Some(result)
}

fn draw(f: &mut Frame, flow: &Flow, ms: u128) {
    let area = f.area();
    let lines = screen(flow, ms);
    let top = (area.height as usize).saturating_sub(lines.len()) / 2;
    let body = Rect::new(area.x, area.y + top as u16, area.width, area.height.saturating_sub(top as u16));
    // Cada linha é centralizada pela própria largura; a arte tem sempre a mesma largura, então não se desmonta.
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: false }), body);
}

// Tudo o que a tela mostra num instante: a arte, o progresso, a pergunta atual e o que está sendo digitado.
fn screen(flow: &Flow, ms: u128) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = soul_art::cells(ms)
        .into_iter()
        .map(|row| Line::from(row.into_iter().map(|(ch, tone)| Span::styled(ch.to_string(), tone_style(tone))).collect::<Vec<_>>()))
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("Cinco perguntas para eu falar do seu jeito", fg(GRAY))));
    lines.push(Line::default());
    let progress: Vec<Span<'static>> = (0..PROMPTS.len())
        .flat_map(|i| {
            let mark = if i < flow.step { "● " } else { "○ " };
            [Span::styled(mark, fg(if i < flow.step { YELLOW } else { GRAY }))]
        })
        .collect();
    lines.push(Line::from(progress));
    match PROMPTS.get(flow.step) {
        Some(prompt) => {
            lines.push(Line::from(Span::styled(*prompt, bold(YELLOW))));
            lines.push(Line::from(vec![Span::styled("› ", fg(YELLOW)), Span::raw(flow.buf.clone()), Span::styled("█", fg(GRAY))]));
        }
        None => lines.push(Line::default()),
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("enter continua · pular dispensa · esc sai", fg(GRAY))));
    lines
}

// Tom de uma célula da arte: amarelo do caranguejo, vermelho do coração, sem cor no resto.
fn tone_style(tone: char) -> Style {
    match tone {
        'Y' => fg(YELLOW),
        'R' => fg(RED),
        _ => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_all(flow: &mut Flow, text: &str) {
        for c in text.chars() {
            flow.type_char(c);
        }
    }

    #[test]
    fn five_enters_answer_every_question_in_order() {
        let mut flow = Flow::default();
        type_all(&mut flow, "Ba");
        assert_eq!(flow.enter(), Step::Next);
        type_all(&mut flow, "inglês");
        assert_eq!(flow.enter(), Step::Next);
        type_all(&mut flow, "2");
        assert_eq!(flow.enter(), Step::Next);
        assert_eq!(flow.enter(), Step::Next, "vazio fica com o padrão da pergunta");
        type_all(&mut flow, "programo");
        let Step::Done(Some(answers)) = flow.enter() else { panic!("deveria terminar") };
        assert_eq!(answers.name, "Ba");
        assert_eq!(answers.language, "inglês");
        assert!(answers.detailed);
        assert!(answers.plan_first, "vazio na pergunta 4 mantém 'explico antes'");
        assert_eq!(answers.about, "programo");
    }

    #[test]
    fn pular_in_any_question_dismisses_everything() {
        let mut flow = Flow::default();
        type_all(&mut flow, "Ba");
        flow.enter();
        type_all(&mut flow, "PULAR");
        assert_eq!(flow.enter(), Step::Done(None));
    }

    #[test]
    fn backspace_edits_the_current_answer_only() {
        let mut flow = Flow::default();
        type_all(&mut flow, "Bax");
        flow.backspace();
        flow.enter();
        assert_eq!(flow.raw, vec!["Ba".to_string()]);
    }

    #[test]
    fn the_screen_shows_the_current_question_and_progress() {
        let mut flow = Flow::default();
        let first = screen(&flow, 0);
        let text: String = first[first.len() - 4].spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, PROMPTS[0]);
        flow.enter();
        let second = screen(&flow, 0);
        let progress: String = second[second.len() - 5].spans.iter().map(|s| s.content.to_string()).collect();
        assert!(progress.starts_with("● ○ "), "{progress:?}");
    }

    #[test]
    fn the_whole_screen_is_centered_and_shows_the_first_question() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| draw(f, &Flow::default(), 0)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..20).map(|y| (0..60).map(|x| buffer[(x, y)].symbol().to_string()).collect()).collect();
        for row in &rows {
            println!("{}", row.trim_end());
        }
        let question = rows.iter().find(|r| r.contains("Como devo te chamar")).expect("a primeira pergunta aparece");
        let left = question.len() - question.trim_start().len();
        let right = question.len() - question.trim_end().len();
        assert!(left.abs_diff(right) <= 2, "centralizada: {left} à esquerda, {right} à direita");
    }

    #[test]
    fn the_art_rows_all_have_the_same_width() {
        let lines = screen(&Flow::default(), 0);
        let widths: Vec<usize> = lines[..6].iter().map(|l| l.spans.iter().map(|s| s.content.chars().count()).sum()).collect();
        assert!(widths.iter().all(|w| *w == widths[0]), "{widths:?}");
    }
}

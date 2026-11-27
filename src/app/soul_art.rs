//! A tela do SOUL: o BitClaw feliz segurando um coração que pulsa, em meios-blocos (`█ ▀ ▄`).
//!
//! O desenho fica no alto da tela e continua batendo enquanto a pessoa responde às perguntas: uma
//! thread redesenha só as linhas da arte (salva o cursor, sobe até elas, pinta, volta). Cada quadro
//! vem só do tempo (`frame`), sem estado escondido, como o mascote do cabeçalho.
//!
//! Regras de `docs/identidade/ARTE-TERMINAL.md`: uma cor por desenho (caranguejo amarelo, coração
//! vermelho), olho como buraco (aqui, de olhos fechados: feliz) e só caracteres de largura 1. O corpo
//! vem de `assets/bitclaw-crab.svg`, a fonte única. Sem terminal, sem cor ou com a janela pequena, a
//! arte não aparece e as perguntas seguem como sempre.

use super::tui::brand::{CRAB_SVG, svg_grid};
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// Tela de pixels: 14 colunas × 12 linhas (6 linhas de terminal). O corpo do mascote (10×6) fica embaixo,
// os braços sobem pelos lados e o coração fica entre eles, com a ponta entre os pinos do topo.
const COLS: usize = 14;
const ROWS: usize = 12;
const TERM_ROWS: usize = ROWS / 2;
const INDENT: &str = "  ";
const CRAB_AT: (usize, usize) = (2, 6);

// Uma batida (lub-dub) a cada `CYCLE_MS`: o coração incha, murcha, incha de novo e descansa.
pub const CYCLE_MS: u128 = 1200;
// Quanto espera entre conferências do relógio.
const TICK: Duration = Duration::from_millis(40);

// Coração cheio (batida): 8 de largura, encosta nas mãos. Coluna inicial 3.
const HEART_BIG: [&str; 6] = [".##..##.", "########", "########", ".######.", "..####..", "...##..."];
// Coração em repouso: 6 de largura, ponta no mesmo lugar, e as mãos soltam um pixel. Coluna inicial 4, linha 1.
const HEART_SMALL: [&str; 5] = [".#..#.", "######", "######", ".####.", "..##.."];

// O coração está inchado neste instante?
pub fn beating(ms: u128) -> bool {
    let t = ms % CYCLE_MS;
    t < 180 || (270..450).contains(&t)
}

// O mapa de pixels de um quadro: 'Y' mascote, 'R' coração, '.' vazio.
pub fn map(ms: u128) -> Vec<Vec<char>> {
    let mut m = vec![vec!['.'; COLS]; ROWS];
    // Corpo: o do SVG, de olhos fechados (os buracos se enchem).
    let mut crab = svg_grid(CRAB_SVG);
    for (x, y) in [(2, 2), (7, 2)] {
        crab[y][x] = true;
    }
    for (r, row) in crab.iter().enumerate() {
        for (c, &on) in row.iter().enumerate() {
            if on {
                m[CRAB_AT.1 + r][CRAB_AT.0 + c] = 'Y';
            }
        }
    }
    // Braços erguidos, das pinças até as mãos que seguram o coração.
    for r in 1..=5 {
        // A mão (duas linhas, dois pixels de largura) fecha em volta do coração; o braço é fino.
        let cols: &[usize] = if r <= 2 { &[1, 2, 11, 12] } else { &[2, 11] };
        for &c in cols {
            m[r][c] = 'Y';
        }
    }
    let (heart, x0, y0): (&[&str], usize, usize) =
        if beating(ms) { (&HEART_BIG, 3, 0) } else { (&HEART_SMALL, 4, 1) };
    for (r, line) in heart.iter().enumerate() {
        for (c, ch) in line.chars().enumerate() {
            if ch == '#' {
                m[y0 + r][x0 + c] = 'R';
            }
        }
    }
    m
}

fn cell(top: char, bottom: char) -> (char, char) {
    match (top != '.', bottom != '.') {
        (true, true) => ('█', top),
        (true, false) => ('▀', top),
        (false, true) => ('▄', bottom),
        (false, false) => (' ', '.'),
    }
}

// As células de um quadro, linha a linha: cada uma é (caractere, tom), com tom 'Y' (amarelo), 'R' (vermelho)
// ou '.' (sem cor). A tela cheia do SOUL pinta com elas; o fluxo por linhas usa `frame`.
pub fn cells(ms: u128) -> Vec<Vec<(char, char)>> {
    let m = map(ms);
    (0..TERM_ROWS).map(|r| (0..COLS).map(|c| cell(m[2 * r][c], m[2 * r + 1][c])).collect()).collect()
}

// As linhas de terminal de um quadro, já com o recuo. Com `color`, amarelo e vermelho em ANSI.
pub fn frame(ms: u128, color: bool) -> Vec<String> {
    let m = map(ms);
    (0..TERM_ROWS)
        .map(|r| {
            let mut line = String::from(INDENT);
            let mut current = '.';
            for c in 0..COLS {
                let (ch, tone) = cell(m[2 * r][c], m[2 * r + 1][c]);
                if color && tone != current {
                    line.push_str(match tone {
                        'Y' => "\x1b[38;2;255;204;0m",
                        'R' => "\x1b[38;2;255;77;77m",
                        _ => "\x1b[0m",
                    });
                    current = tone;
                }
                line.push(ch);
            }
            if color {
                line.push_str("\x1b[0m");
            }
            line
        })
        .collect()
}

// Linhas que a arte ocupa na tela, com a linha em branco que a separa das perguntas.
pub const HEIGHT: usize = TERM_ROWS + 1;

// A arte na tela, batendo numa thread até ser largada.
pub struct Art {
    stop: Arc<AtomicBool>,
    below: Arc<AtomicUsize>,
    width: usize,
    handle: Option<JoinHandle<()>>,
}

impl Art {
    // Desenha a arte e começa a animar. `None` (sem desenhar nada) se a entrada ou a saída não é um
    // terminal ou se a janela é pequena demais para a arte e as perguntas caberem juntas.
    pub fn start(color: bool) -> Option<Art> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return None;
        }
        let (cols, rows) = crossterm::terminal::size().ok()?;
        if (cols as usize) < INDENT.len() + COLS + 4 || (rows as usize) < HEIGHT + 10 {
            return None;
        }
        let mut out = io::stdout().lock();
        let mut text = String::from("\n");
        for line in frame(0, color) {
            text.push_str(&line);
            text.push('\n');
        }
        text.push('\n');
        out.write_all(text.as_bytes()).ok()?;
        out.flush().ok()?;
        drop(out);

        let stop = Arc::new(AtomicBool::new(false));
        let below = Arc::new(AtomicUsize::new(0));
        let (s, b) = (stop.clone(), below.clone());
        let handle = thread::spawn(move || {
            let start = Instant::now();
            let mut shown = beating(0);
            while !s.load(Ordering::Relaxed) {
                thread::sleep(TICK);
                let ms = start.elapsed().as_millis();
                if beating(ms) != shown {
                    shown = beating(ms);
                    draw(ms, color, b.load(Ordering::Relaxed));
                }
            }
        });
        Some(Art { stop, below, width: cols as usize, handle: Some(handle) })
    }

    // Avisa que uma linha de `chars` caracteres foi escrita abaixo da arte. Linha mais larga que a
    // janela quebra, e cada pedaço conta: o cursor tem de subir por todos eles até o alto da arte.
    pub fn line(&self, chars: usize) {
        self.below.fetch_add(screen_lines(chars, self.width), Ordering::Relaxed);
    }
}

impl Drop for Art {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// Quantas linhas da tela uma linha de `chars` caracteres ocupa numa janela de `width` colunas.
pub fn screen_lines(chars: usize, width: usize) -> usize {
    chars.div_ceil(width.max(1)).max(1)
}

// Repinta as linhas da arte sem mexer no resto da tela: esconde o cursor, salva a posição, sobe até o
// alto da arte, pinta cada linha e volta. Tudo numa escrita só, para não se misturar com as perguntas.
fn draw(ms: u128, color: bool, below: usize) {
    let up = HEIGHT + below;
    let mut s = format!("\x1b[?25l\x1b7\x1b[{up}A\r");
    for line in frame(ms, color) {
        s.push_str(&line);
        s.push_str("\x1b[B\r");
    }
    s.push_str("\x1b8\x1b[?25h");
    let mut out = io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(ms: u128) -> String {
        frame(ms, false).join("\n")
    }

    #[test]
    fn every_frame_has_the_same_size() {
        for ms in (0..CYCLE_MS * 2).step_by(30) {
            let rows = frame(ms, false);
            assert_eq!(rows.len(), TERM_ROWS);
            for r in &rows {
                assert_eq!(r.chars().count(), INDENT.len() + COLS, "{r:?}");
            }
        }
    }

    #[test]
    fn the_heart_beats_twice_then_rests() {
        assert!(beating(0));
        assert!(!beating(220));
        assert!(beating(300));
        assert!(!beating(600));
        assert!(!beating(CYCLE_MS - 1));
        assert!(beating(CYCLE_MS), "o ciclo recomeça");
        assert_ne!(plain(0), plain(600), "inchado e em repouso são desenhos diferentes");
        assert_eq!(plain(0), plain(CYCLE_MS), "o quadro só depende do instante dentro do ciclo");
    }

    #[test]
    fn the_crab_has_closed_eyes_and_the_two_studs() {
        let m = map(600);
        // Olhos (colunas 2 e 7 do mascote, linha 2) cheios: de olhos fechados, sem buraco.
        for x in [2, 7] {
            assert_eq!(m[CRAB_AT.1 + 2][CRAB_AT.0 + x], 'Y');
        }
        // Os dois pinos do topo do corpo, assinatura do mascote.
        for x in [3, 6] {
            assert_eq!(m[CRAB_AT.1][CRAB_AT.0 + x], 'Y');
        }
    }

    #[test]
    fn the_heart_sits_between_the_hands_and_the_tip_between_the_studs() {
        let big = map(0);
        let small = map(600);
        let hearts = |m: &Vec<Vec<char>>| m.iter().flatten().filter(|&&c| c == 'R').count();
        assert!(hearts(&big) > hearts(&small));
        // A ponta (colunas 6 e 7) fica entre os pinos (5 e 8) em repouso e inchado.
        for m in [&big, &small] {
            assert_eq!(m[5][6], 'R');
            assert_eq!(m[5][7], 'R');
            assert_eq!(m[6][5], 'Y');
            assert_eq!(m[6][8], 'Y');
        }
        // Inchado, o coração encosta nas mãos; em repouso, as mãos soltam um pixel.
        assert_eq!((big[1][2], big[1][3]), ('Y', 'R'));
        assert_eq!((small[1][2], small[1][3]), ('Y', '.'));
    }

    #[test]
    fn the_heart_and_the_crab_never_share_a_terminal_cell() {
        for ms in [0, 600] {
            let m = map(ms);
            for r in 0..TERM_ROWS {
                for c in 0..COLS {
                    let tones = [m[2 * r][c], m[2 * r + 1][c]];
                    assert!(!(tones.contains(&'Y') && tones.contains(&'R')), "{ms}ms r{r} c{c}");
                }
            }
        }
    }

    #[test]
    fn a_long_line_counts_every_screen_line_it_wraps_into() {
        assert_eq!(screen_lines(0, 80), 1, "linha vazia ainda ocupa uma linha");
        assert_eq!(screen_lines(80, 80), 1, "a quebra é preguiçosa: 80 colunas cabem em 80");
        assert_eq!(screen_lines(81, 80), 2);
        assert_eq!(screen_lines(161, 80), 3);
    }

    #[test]
    fn only_width_one_characters_and_ansi_only_with_color() {
        for ch in plain(0).chars() {
            assert!(matches!(ch, ' ' | '█' | '▀' | '▄' | '\n'), "{ch:?}");
        }
        assert!(!plain(0).contains('\x1b'));
        let colored = frame(0, true).join("");
        assert!(colored.contains("38;2;255;204;0") && colored.contains("38;2;255;77;77"));
    }
}

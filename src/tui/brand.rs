//! Identidade visual do BitClaw: o mascote e o cabeçalho de texto. Só o binário usa este módulo.

// Mascote monocromático: mapa de pixels 10×6, renderizado em meio-blocos (3 linhas de terminal).
// Sinais de tijolo: dois pinos no topo e uma faixa de emenda (D) abaixo dos olhos.
// Legenda: Y corpo, T pino, D emenda, S pé (preenchidos); E olho (buraco); '.' vazio.
const CRAB_MAP: [&str; 6] = [
    "YY.T..T.YY",
    "YYYYYYYYYY",
    "YYEYYYYEYY",
    "YDDDDDDDDY",
    "..YYYYYY..",
    ".S.S..S.S.",
];

// Uma célula do terminal mostra dois pixels: o de cima e o de baixo (`▀`, `▄`, `█`).
fn cell(top: char, bottom: char) -> char {
    let on = |c: char| matches!(c, 'Y' | 'T' | 'D' | 'S');
    match (on(top), on(bottom)) {
        (true, true) => '█',
        (true, false) => '▀',
        (false, true) => '▄',
        (false, false) => ' ',
    }
}

// Linhas de terminal de um mapa de pixels: cada uma junta duas linhas do mapa.
fn render(map: &[Vec<char>]) -> Vec<String> {
    map.chunks(2).map(|pair| pair[0].iter().zip(pair[1].iter()).map(|(t, b)| cell(*t, *b)).collect()).collect()
}

fn base_map() -> Vec<Vec<char>> {
    CRAB_MAP.iter().map(|r| r.chars().collect()).collect()
}

// Linhas de terminal do mascote parado, como aparece no cabeçalho.
pub fn crab() -> Vec<String> {
    render(&base_map())
}

// Estados do mascote. Cada um tem a sua animação, calculada só a partir do tempo: sem estado
// escondido, o que torna cada quadro testável.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    // Esperando uma tarefa: pisca e olha em volta.
    Idle,
    // O modelo raciocina: os pinos do topo contam em binário, como bits virando.
    Thinking,
    // Uma ferramenta roda: as pinças batucam.
    Working,
    // Espera a decisão do usuário: pinças em pé e pinos piscando devagar.
    Waiting,
    // Turno terminou bem: estala as pinças.
    Happy,
    // Turno terminou com erro: pinças caídas, olhos fechados.
    Sad,
}

// Quanto tempo as reações de fim de turno duram.
pub const HAPPY_MS: u128 = 1600;
pub const SAD_MS: u128 = 2400;

// Olhos: buracos no corpo (linha 2). Fechados, o buraco some; olhando de lado, ele anda um pixel.
#[derive(Clone, Copy, PartialEq)]
enum Eyes {
    Open,
    Closed,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq)]
enum Claws {
    Up,
    Down,
    // Só a esquerda ou só a direita caída: batucar.
    LeftDown,
    RightDown,
    // Fechadas uma contra a outra: o estalo.
    Snap,
}

struct Pose {
    eyes: Eyes,
    claws: Claws,
    // Pinos do topo acesos: (esquerdo, direito).
    studs: (bool, bool),
    // Segundo quadro dos pés.
    step: bool,
}

fn pose(mood: Mood, ms: u128) -> Pose {
    let tick = |period: u128| (ms / period) as usize;
    match mood {
        Mood::Idle => {
            let t = ms % 4400;
            let eyes = match t {
                3000..3140 => Eyes::Closed,
                800..1400 => Eyes::Left,
                1800..2400 => Eyes::Right,
                _ => Eyes::Open,
            };
            Pose { eyes, claws: Claws::Up, studs: (true, true), step: false }
        }
        Mood::Thinking => {
            let n = tick(240) % 4;
            let eyes = if tick(720) % 2 == 0 { Eyes::Left } else { Eyes::Right };
            Pose { eyes, claws: Claws::Up, studs: (n & 2 != 0, n & 1 != 0), step: tick(240) % 2 == 1 }
        }
        Mood::Working => {
            let claws = if tick(200) % 2 == 0 { Claws::LeftDown } else { Claws::RightDown };
            Pose { eyes: Eyes::Open, claws, studs: (true, true), step: tick(200) % 2 == 1 }
        }
        Mood::Waiting => {
            let on = tick(600) % 2 == 0;
            Pose { eyes: Eyes::Open, claws: Claws::Up, studs: (on, on), step: false }
        }
        Mood::Happy => {
            let claws = if tick(200) % 2 == 0 { Claws::Snap } else { Claws::Up };
            Pose { eyes: Eyes::Closed, claws, studs: (true, true), step: tick(200) % 2 == 1 }
        }
        Mood::Sad => Pose { eyes: Eyes::Closed, claws: Claws::Down, studs: (false, false), step: false },
    }
}

// Quadro do mascote para o estado e o tempo (em ms) desde que o estado começou.
pub fn frame(mood: Mood, ms: u128) -> Vec<String> {
    let p = pose(mood, ms);
    let mut m = base_map();
    // Pinos (linha 0, colunas 3 e 6).
    m[0][3] = if p.studs.0 { 'T' } else { '.' };
    m[0][6] = if p.studs.1 { 'T' } else { '.' };
    // Pinças (linha 0, colunas 0-1 e 8-9).
    let mut set = |cols: &[usize], v: char| cols.iter().for_each(|&c| m[0][c] = v);
    match p.claws {
        Claws::Up => {}
        Claws::Down => {
            set(&[0, 1, 8, 9], '.');
        }
        Claws::LeftDown => set(&[0, 1], '.'),
        Claws::RightDown => set(&[8, 9], '.'),
        Claws::Snap => {
            set(&[0, 9], '.');
            set(&[1, 2, 7, 8], 'Y');
        }
    }
    // Olhos (linha 2, buracos nas colunas 2 e 7).
    let holes: &[usize] = match p.eyes {
        Eyes::Open => &[2, 7],
        Eyes::Closed => &[],
        Eyes::Left => &[1, 6],
        Eyes::Right => &[3, 8],
    };
    m[2] = "YYYYYYYYYY".chars().collect();
    holes.iter().for_each(|&c| m[2][c] = 'E');
    // Pés (linha 5).
    m[5] = if p.step { "S.S....S.S".chars().collect() } else { ".S.S..S.S.".chars().collect() };
    render(&m)
}

// Aviso mostrado sempre que o raciocínio do modelo é ligado.
pub const THINK_WARNING: &str = "thinking ligado: o modelo raciocina antes de responder, o que gasta mais tokens e enche a janela de contexto mais rápido. Nem todo modelo suporta.";

// Largura do mascote em colunas.
#[cfg(test)]
fn crab_width() -> usize {
    CRAB_MAP[0].chars().count()
}

// Cabeçalho sem cor: mascote à esquerda e o texto à direita, centralizado na vertical.
pub fn plain_header(lines: &[String]) -> String {
    let glyph = crab();
    let offset = glyph.len().saturating_sub(lines.len()) / 2;
    let mut out: Vec<String> = glyph
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let text = i.checked_sub(offset).and_then(|j| lines.get(j)).cloned().unwrap_or_default();
            format!("{g}  {text}")
        })
        .collect();
    // Linhas que não cabem ao lado do mascote descem abaixo dele, alinhadas com o texto.
    let pad = " ".repeat(glyph.first().map_or(0, |g| g.chars().count()) + 2);
    for j in (glyph.len() - offset.min(glyph.len()))..lines.len() {
        out.push(format!("{pad}{}", lines[j]));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crab_map_is_a_rectangle_with_an_even_number_of_rows() {
        assert_eq!(CRAB_MAP.len() % 2, 0);
        assert!(CRAB_MAP.iter().all(|r| r.chars().count() == crab_width()));
        assert!(CRAB_MAP.iter().all(|r| r.chars().all(|c| ".YTDSE".contains(c))));
    }

    #[test]
    fn every_mood_keeps_the_mascot_size_over_time() {
        let moods = [Mood::Idle, Mood::Thinking, Mood::Working, Mood::Waiting, Mood::Happy, Mood::Sad];
        for mood in moods {
            for ms in (0..9000).step_by(40) {
                let f = frame(mood, ms);
                assert_eq!(f.len(), CRAB_MAP.len() / 2, "{mood:?} em {ms} ms");
                assert!(f.iter().all(|r| r.chars().count() == crab_width()), "{mood:?} em {ms} ms");
            }
        }
    }

    #[test]
    fn the_resting_frame_is_the_header_mascot() {
        assert_eq!(frame(Mood::Idle, 0), crab());
    }

    #[test]
    fn idle_blinks_and_looks_around() {
        let open = frame(Mood::Idle, 0);
        assert_ne!(frame(Mood::Idle, 3050), open, "pisca");
        assert_ne!(frame(Mood::Idle, 1000), open, "olha para um lado");
        assert_ne!(frame(Mood::Idle, 1000), frame(Mood::Idle, 2000), "e para o outro");
        assert_eq!(frame(Mood::Idle, 4400), open, "o ciclo se repete");
    }

    #[test]
    fn thinking_studs_count_in_binary() {
        let studs = |ms| {
            let f = frame(Mood::Thinking, ms);
            let row: Vec<char> = f[0].chars().collect();
            (row[3] != ' ' && row[3] != '▄', row[6] != ' ' && row[6] != '▄')
        };
        let seen: Vec<_> = (0..4).map(|i| studs(i * 240)).collect();
        assert_eq!(seen, vec![(false, false), (false, true), (true, false), (true, true)]);
    }

    #[test]
    fn each_mood_looks_different() {
        let at = |m| frame(m, 100);
        let frames = [at(Mood::Thinking), at(Mood::Working), at(Mood::Happy), at(Mood::Sad)];
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "{i} e {j} iguais");
            }
        }
    }

    #[test]
    fn crab_rows_all_have_the_same_width() {
        let rows = crab();
        assert_eq!(rows.len(), CRAB_MAP.len() / 2);
        assert!(rows.iter().all(|r| r.chars().count() == crab_width()));
    }

    #[test]
    fn header_pairs_the_crab_with_the_text() {
        let text = plain_header(&["BitClaw".into(), "projeto /tmp/x".into()]);
        assert_eq!(text.lines().count(), 3);
        assert!(text.contains("BitClaw") && text.contains("projeto /tmp/x"));
    }
}

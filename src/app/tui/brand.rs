//! Identidade visual do BitClaw: o mascote e o cabeçalho de texto. Só o binário usa este módulo.

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;
use resvg::{tiny_skia, usvg};
use std::sync::OnceLock;

// Mascote monocromático. A arte vem de `assets/bitclaw-crab.svg`, a fonte única: cada retângulo
// preenchido é um pixel numa grade de 10×6 (SVG de 240×144, pixels de 24). Um buraco no SVG é um
// olho ou um vão. Cada duas linhas da grade viram uma linha do terminal, com meios-blocos (`█ ▀ ▄`).
pub const CRAB_SVG: &str = include_str!("../../../assets/bitclaw-crab.svg");
pub const PIXEL: u32 = 24;
pub const GRID: (usize, usize) = (10, 6);
pub const COLOR: &str = "#FFCC00";
pub const SVG_WIDTH: u32 = 240;
pub const SVG_HEIGHT: u32 = 144;

// Mapa de pixels lido do SVG: 'Y' aceso, '.' apagado. Os pinos (T) e os pés (S) viram 'T' e 'S'
// aqui, para os quadros os moverem por posição. O mapa é lido uma vez e reaproveitado.
fn base_map() -> &'static Vec<Vec<char>> {
    static MAP: std::sync::OnceLock<Vec<Vec<char>>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let (cols, rows) = GRID;
        let mut map = vec![vec!['.'; cols]; rows];
        for tag in CRAB_SVG.split("<rect").skip(1) {
            let (Some(x), Some(y)) = (svg_attr(tag, "x"), svg_attr(tag, "y")) else { continue };
            let (c, r) = ((x / PIXEL) as usize, (y / PIXEL) as usize);
            if r < rows && c < cols {
                map[r][c] = 'Y';
            }
        }
        // Pinos do topo (colunas 3 e 6) e pés (a última linha): só nomes, o desenho não muda.
        for c in [3, 6] {
            if map[0][c] == 'Y' {
                map[0][c] = 'T';
            }
        }
        for cell in map[rows - 1].iter_mut().filter(|c| **c == 'Y') {
            *cell = 'S';
        }
        map
    })
}

// Valor de um atributo numérico de um `<rect …>`. Procura ` nome="` para não casar com `width`.
fn svg_attr(tag: &str, name: &str) -> Option<u32> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let rest = &tag[start..];
    rest[..rest.find('"')?].parse().ok()
}

// Largura do mascote em colunas do terminal.
pub const WIDTH: u16 = GRID.0 as u16;

// Duas linhas do mapa por linha do terminal: `▀`, `▄`, `█` ou espaço, conforme os dois pixels.
fn cell(top: char, bottom: char) -> char {
    let on = |c: char| matches!(c, 'Y' | 'T' | 'S');
    match (on(top), on(bottom)) {
        (true, true) => '█',
        (true, false) => '▀',
        (false, true) => '▄',
        (false, false) => ' ',
    }
}

fn render(map: &[Vec<char>]) -> Vec<String> {
    map.chunks(2).map(|pair| pair[0].iter().zip(pair[1].iter()).map(|(t, b)| cell(*t, *b)).collect()).collect()
}

// Linhas de terminal do mascote parado, como aparece no cabeçalho.
pub fn crab() -> Vec<String> {
    render(&base_map())
}

// Números de animação do mascote. Ele fica parado (`Idle` em 0 ms) e só se mexe ao abrir o
// terminal e a cada clique. Cada quadro é calculado só a partir do tempo: sem estado escondido,
// o que torna cada um testável.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    // Parado, de olhos abertos. Com tempo correndo, pisca e olha em volta.
    Idle,
    // Cumprimento ao abrir: acorda com uma piscada, olha para os lados e estala as pinças.
    Greet,
    // Os pinos do topo contam em binário, como bits virando.
    Thinking,
    // As pinças batucam, uma de cada vez.
    Working,
    // Estala as pinças, de olhos fechados.
    Happy,
}

// Quanto dura cada número, e a ordem em que os cliques os mostram.
pub const TRICK_MS: u128 = 2200;
pub const TRICKS: [Mood; 4] = [Mood::Happy, Mood::Thinking, Mood::Working, Mood::Greet];

// Número de abertura: sorteado a cada vez que o terminal abre, entre os mesmos de `TRICKS`.
// Sem crate de aleatoriedade: o `RandomState` da std recebe uma semente nova a cada uso. (Os
// nanossegundos do relógio não servem: no macOS vêm sempre em múltiplos de 1000, e o resto por 4 fica 0.)
pub fn opening_mood() -> Mood {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let pick = RandomState::new().build_hasher().finish() as usize;
    TRICKS[pick % TRICKS.len()]
}

#[cfg(test)]
mod opening_tests {
    use super::*;

    #[test]
    fn the_opening_number_changes_between_openings() {
        let seen: std::collections::HashSet<_> = (0..40).map(|_| opening_mood() as u8).collect();
        assert!(seen.len() > 1, "a abertura não pode repetir sempre o mesmo número");
    }
}

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
        Mood::Greet => {
            let eyes = match ms {
                0..160 => Eyes::Closed,
                400..800 => Eyes::Left,
                800..1200 => Eyes::Right,
                _ => Eyes::Open,
            };
            let claws = if (1300..2100).contains(&ms) && tick(200) % 2 == 1 { Claws::Snap } else { Claws::Up };
            Pose { eyes, claws, studs: (true, true), step: false }
        }
        Mood::Happy => {
            let claws = if tick(200) % 2 == 0 { Claws::Snap } else { Claws::Up };
            Pose { eyes: Eyes::Closed, claws, studs: (true, true), step: tick(200) % 2 == 1 }
        }
    }
}

// Matriz de pixels do quadro para o estado e o tempo (em ms).
fn frame_map(mood: Mood, ms: u128) -> Vec<Vec<char>> {
    let p = pose(mood, ms);
    let mut m = base_map().clone();
    // Pinos (linha 0, colunas 3 e 6).
    m[0][3] = if p.studs.0 { 'T' } else { '.' };
    m[0][6] = if p.studs.1 { 'T' } else { '.' };
    // Pinças (linha 0, colunas 0-1 e 8-9).
    let mut set = |cols: &[usize], v: char| cols.iter().for_each(|&c| m[0][c] = v);
    match p.claws {
        Claws::Up => {}
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
    m
}

// Converte uma matriz 10×6 em SVG com retângulos de cor `#FFCC00` na mesma geometria do SVG original.
pub fn map_to_svg(map: &[Vec<char>]) -> String {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{SVG_WIDTH}\" height=\"{SVG_HEIGHT}\" viewBox=\"0 0 {SVG_WIDTH} {SVG_HEIGHT}\" shape-rendering=\"crispEdges\">\n  <title>BitClaw</title>\n"
    );
    for (r, row) in map.iter().enumerate() {
        for (c, &cell) in row.iter().enumerate() {
            if matches!(cell, 'Y' | 'T' | 'S') {
                let x = c as u32 * PIXEL;
                let y = r as u32 * PIXEL;
                svg.push_str(&format!(
                    "  <rect x=\"{x}\" y=\"{y}\" width=\"{PIXEL}\" height=\"{PIXEL}\" fill=\"{COLOR}\"/>\n"
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

// SVG correspondente ao estado e tempo (em ms). No repouso (Idle em 0ms), preserva `assets/bitclaw-crab.svg`.
pub fn frame_svg(mood: Mood, ms: u128) -> String {
    if mood == Mood::Idle && ms == 0 {
        return CRAB_SVG.to_string();
    }
    map_to_svg(&frame_map(mood, ms))
}

// Rasteriza qualquer SVG para DynamicImage RGBA em memória via resvg e tiny-skia.
pub fn rasterize_svg(svg: &str) -> Result<DynamicImage, String> {
    let opt = usvg::Options::default();
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "falha ao alocar pixmap para o mascote".to_string())?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let img = image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.take_demultiplied())
        .ok_or_else(|| "falha ao converter pixmap para imagem RGBA".to_string())?;
    Ok(DynamicImage::ImageRgba8(img))
}

// Imagem do mascote parado, rasterizada uma única vez e reaproveitada em repouso.
pub fn resting_image() -> &'static DynamicImage {
    static RESTING: OnceLock<DynamicImage> = OnceLock::new();
    RESTING.get_or_init(|| {
        rasterize_svg(CRAB_SVG).expect("o SVG do mascote precisa rasterizar com sucesso")
    })
}

// Imagem rasterizada do mascote para o estado e tempo dados.
pub fn frame_image(mood: Mood, ms: u128) -> DynamicImage {
    if mood == Mood::Idle && ms == 0 {
        return resting_image().clone();
    }
    let svg = frame_svg(mood, ms);
    rasterize_svg(&svg).unwrap_or_else(|_| resting_image().clone())
}

// Quadro do mascote para o estado e o tempo (em ms) desde que o estado começou: representação textual de fallback.
pub fn frame(mood: Mood, ms: u128) -> Vec<String> {
    render(&frame_map(mood, ms))
}

/// Suporte e detecção de protocolos gráficos no terminal (iTerm2, Kitty, Sixel).
#[derive(Clone, Debug)]
pub struct Graphics {
    picker: Picker,
}

impl Graphics {
    /// Detecta o protocolo gráfico suportado pelo terminal.
    ///
    /// Ordem de prioridade e compatibilidade:
    /// 1. iTerm2 (alvo principal no macOS): usa o iTerm2 Inline Images Protocol (`ProtocolType::Iterm2`).
    /// 2. Terminal.app: NÃO suporta imagens. Retorna `None` imediatamente (fallback textual gracioso).
    /// 3. Outros terminais com suporte comprovado (WezTerm, Ghostty, Kitty): seleciona o protocolo adequado.
    /// 4. Qualquer outro terminal (VS Code inclusive): `None`, e o mascote sai em meios-blocos.
    pub fn detect() -> Option<Self> {
        if let Ok(prog) = std::env::var("TERM_PROGRAM") {
            // Terminal.app do macOS: nunca suporta imagens de terminal.
            // Retorna None imediatamente, sem sequências de escape nem delay.
            if prog == "Apple_Terminal" {
                return None;
            }
            // VS Code (xterm.js) não desenha estes protocolos por padrão, e herda variáveis do
            // terminal que o abriu (ITERM_SESSION_ID, KITTY_WINDOW_ID). Fica nos meios-blocos.
            if prog == "vscode" {
                return None;
            }
            if prog.contains("iTerm") {
                let mut opts = ratatui_image::picker::cap_parser::QueryStdioOptions::default();
                opts.timeout_ms = 40;
                if let Ok(picker) = Picker::from_query_stdio_with_options(opts) {
                    if picker.protocol_type() == ProtocolType::Iterm2 {
                        return Some(Self { picker });
                    }
                }
                let mut picker = Picker::halfblocks();
                picker.set_protocol_type(ProtocolType::Iterm2);
                return Some(Self { picker });
            }
            if prog.contains("WezTerm") {
                let mut picker = Picker::halfblocks();
                picker.set_protocol_type(ProtocolType::Iterm2);
                return Some(Self { picker });
            }
            if prog == "ghostty" {
                let mut picker = Picker::halfblocks();
                picker.set_protocol_type(ProtocolType::Kitty);
                return Some(Self { picker });
            }
        }

        if std::env::var("ITERM_SESSION_ID").is_ok() {
            let mut picker = Picker::halfblocks();
            picker.set_protocol_type(ProtocolType::Iterm2);
            return Some(Self { picker });
        }

        if std::env::var("KITTY_WINDOW_ID").is_ok() || std::env::var("TERM").is_ok_and(|t| t == "xterm-kitty") {
            let mut picker = Picker::halfblocks();
            picker.set_protocol_type(ProtocolType::Kitty);
            return Some(Self { picker });
        }

        None
    }

    pub fn is_supported() -> bool {
        Self::detect().is_some()
    }

    #[cfg(test)]
    pub fn with_protocol(protocol_type: ProtocolType) -> Self {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(protocol_type);
        Self { picker }
    }

    #[cfg(test)]
    pub fn protocol_type(&self) -> ProtocolType {
        self.picker.protocol_type()
    }

    pub fn new_protocol(&self, image: DynamicImage, size: Size) -> Result<Protocol, String> {
        self.picker.new_protocol(image, size, Resize::Fit(None)).map_err(|e| e.to_string())
    }
}

// O que muda ao aumentar a janela de contexto. Mostrado em `/context` e ao aumentá-la.
pub const CONTEXT_RAISE_HEAD: &str = "contexto maior: o modelo lembra mais da conversa antes de o BitClaw cortar o histórico, mas isso tem custo.";
pub const CONTEXT_COSTS: [&str; 5] = [
    "memória: o cache de atenção cresce junto com a janela e o Ollama o reserva ao carregar o modelo. Em Mac com memória unificada, passar do que cabe causa swap e lentidão.",
    "recarga: mudar a janela costuma fazer o Ollama recarregar o modelo (alguns segundos) e o próximo prompt perde o cache.",
    "velocidade: processar o prompt e gerar ficam mais lentos conforme o histórico enche.",
    "tokens: cada chamada reenvia todo o histórico, então o total gasto cresce mais rápido.",
    "qualidade: modelos pequenos perdem o foco em contextos longos; acima do máximo do modelo o Ollama trunca.",
];
// Ao diminuir a janela.
pub const CONTEXT_LOWER: &str = "contexto menor: mais leve e rápido, mas o histórico é cortado mais cedo e o agente esquece o que veio antes. O prompt do sistema e as ferramentas ocupam uma parte fixa da janela.";

// O que a temperatura faz. Mostrado em `/temp`.
pub const TEMPERATURE_NOTES: [&str; 4] = [
    "perto de 0: o modelo escolhe quase sempre o token mais provável. Respostas estáveis e repetíveis, boas para ferramentas e código; em troca, variam menos.",
    "de 0.6 a 0.9: equilíbrio. O Ollama usa 0.8 quando nada é pedido.",
    "1 ou mais: respostas mais variadas e criativas, porém com mais erro em argumentos de ferramentas e menos fidelidade ao pedido.",
    "a temperatura não corrige laços de raciocínio: nos testes com o nemotron-3-nano:4b-mlx eles apareceram em 0, 0.1 e 0.6.",
];

// Aviso mostrado sempre que o raciocínio do modelo é ligado.
pub const THINK_WARNING: &str = "thinking ligado: o modelo raciocina antes de responder, o que gasta mais tokens e enche a janela de contexto mais rápido. Nem todo modelo suporta.";

// Largura do mascote em colunas.
#[cfg(test)]
fn crab_width() -> usize {
    base_map()[0].len()
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
    fn crab_map_is_a_rectangle() {
        assert!(base_map().iter().all(|r| r.len() == base_map()[0].len()));
        assert!(base_map().iter().all(|r| r.iter().all(|c| ".YTSE".contains(*c))));
    }

    #[test]
    fn every_mood_keeps_the_mascot_size_over_time() {
        let moods = [Mood::Idle, Mood::Greet, Mood::Thinking, Mood::Working, Mood::Happy];
        for mood in moods {
            for ms in (0..9000).step_by(40) {
                let f = frame(mood, ms);
                assert_eq!(f.len(), base_map().len() / 2, "{mood:?} em {ms} ms");
                assert!(f.iter().all(|r| r.chars().count() == crab_width()), "{mood:?} em {ms} ms");
            }
        }
    }

    #[test]
    fn the_map_is_read_from_the_svg() {
        let rows: Vec<String> = base_map().iter().map(|r| r.iter().collect()).collect();
        assert_eq!(rows, ["YY.T..T.YY", "YYYYYYYYYY", "YY.YYYY.YY", "YYYYYYYYYY", "..YYYYYY..", ".S.S..S.S."]);
    }

    #[test]
    fn width_constant_matches_the_map() {
        assert_eq!(WIDTH as usize, crab_width());
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
    fn greeting_wakes_up_looks_around_and_snaps_then_rests() {
        assert_ne!(frame(Mood::Greet, 0), crab(), "acorda de olhos fechados");
        assert_ne!(frame(Mood::Greet, 500), frame(Mood::Greet, 1000), "olha para um lado e para o outro");
        assert_ne!(frame(Mood::Greet, 1500), crab(), "estala as pinças");
        assert_eq!(frame(Mood::Greet, TRICK_MS - 1), crab(), "termina parado");
    }

    #[test]
    fn clicks_cycle_through_every_trick() {
        assert_eq!(TRICKS.len(), 4);
        assert!(TRICKS.iter().all(|m| frame(*m, 300) != crab() || *m == Mood::Greet), "cada número mexe no mascote");
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
        let frames = [at(Mood::Thinking), at(Mood::Working), at(Mood::Happy), frame(Mood::Greet, 500)];
        for i in 0..frames.len() {
            for j in i + 1..frames.len() {
                assert_ne!(frames[i], frames[j], "{i} e {j} iguais");
            }
        }
    }

    #[test]
    fn crab_rows_all_have_the_same_width() {
        let rows = crab();
        assert_eq!(rows.len(), base_map().len() / 2);
        assert!(rows.iter().all(|r| r.chars().count() == crab_width()));
    }

    #[test]
    fn header_pairs_the_crab_with_the_text() {
        let text = plain_header(&["BitClaw".into(), "projeto /tmp/x".into()]);
        assert_eq!(text.lines().count(), base_map().len() / 2);
        assert!(text.contains("BitClaw") && text.contains("projeto /tmp/x"));
    }

    #[test]
    fn rasterize_svg_loads_exact_dimensions_and_colors() {
        let img = resting_image();
        assert_eq!(img.width(), SVG_WIDTH);
        assert_eq!(img.height(), SVG_HEIGHT);
        // Pixel em (12, 12) fica dentro do primeiro retângulo da pinça -> #FFCC00 (255, 204, 0, 255)
        let rgba = img.to_rgba8();
        let pixel = rgba.get_pixel(12, 12);
        assert_eq!(pixel.0, [255, 204, 0, 255]);
        // Pixel em (60, 12) fica entre a pinça e o pino -> transparente (0, 0, 0, 0)
        let empty = rgba.get_pixel(60, 12);
        assert_eq!(empty.0, [0, 0, 0, 0]);
    }

    #[test]
    fn frame_svg_and_frame_image_work_for_all_moods() {
        let moods = [Mood::Idle, Mood::Greet, Mood::Thinking, Mood::Working, Mood::Happy];
        for mood in moods {
            let svg = frame_svg(mood, 100);
            assert!(svg.contains("<svg"));
            assert!(svg.contains(COLOR));
            let img = frame_image(mood, 100);
            assert_eq!(img.width(), SVG_WIDTH);
            assert_eq!(img.height(), SVG_HEIGHT);
        }
    }

    #[test]
    fn graphics_protocol_creates_iterm2_protocol() {
        let g = Graphics::with_protocol(ProtocolType::Iterm2);
        assert_eq!(g.protocol_type(), ProtocolType::Iterm2);
        let proto = g.new_protocol(resting_image().clone(), Size::new(WIDTH, 3));
        assert!(proto.is_ok());
    }

    #[test]
    fn graphics_detection_respects_apple_terminal() {
        // Se TERM_PROGRAM for Apple_Terminal, detect() precisa retornar None (fallback textual).
        // Testamos isso verificando se uma variável de ambiente Apple_Terminal é tratada.
        unsafe {
            std::env::set_var("TERM_PROGRAM", "Apple_Terminal");
        }
        assert!(Graphics::detect().is_none(), "Apple_Terminal não deve ativar protocolo gráfico");
        // VS Code herda ITERM_SESSION_ID de quem o abriu, mas não desenha imagens: meios-blocos.
        unsafe {
            std::env::set_var("TERM_PROGRAM", "vscode");
            std::env::set_var("ITERM_SESSION_ID", "w0t0p0");
        }
        assert!(Graphics::detect().is_none(), "vscode deve usar o fallback em meios-blocos");
        unsafe {
            std::env::remove_var("ITERM_SESSION_ID");
            std::env::remove_var("TERM_PROGRAM");
        }
    }
}

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

// Linhas de terminal do mascote: cada uma junta duas linhas do mapa.
pub fn crab() -> Vec<String> {
    CRAB_MAP
        .chunks(2)
        .map(|pair| pair[0].chars().zip(pair[1].chars()).map(|(t, b)| cell(t, b)).collect())
        .collect()
}

// Largura do mascote em colunas.
#[cfg(test)]
fn crab_width() -> usize {
    CRAB_MAP[0].chars().count()
}

// Cabeçalho sem cor: mascote à esquerda e o texto à direita, centralizado na vertical.
pub fn plain_header(lines: &[String]) -> String {
    let glyph = crab();
    let offset = glyph.len().saturating_sub(lines.len()) / 2;
    glyph
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let text = i.checked_sub(offset).and_then(|j| lines.get(j)).cloned().unwrap_or_default();
            format!("{g}  {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
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

//! Renderização leve de Markdown para o terminal: títulos, listas, `código`, **negrito** e blocos
//! de código. Não pretende cobrir o Markdown inteiro; modelos pequenos usam só isso.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

// Estilos usados na renderização. Vêm da interface, que decide se há cores.
#[derive(Clone, Copy)]
pub struct Palette {
    pub text: Style,
    pub code: Style,
    pub heading: Style,
    pub bullet: Style,
}

// Recuo de toda a resposta, para alinhar com o texto do mascote e das ferramentas.
const INDENT: usize = 2;

// Uma palavra com o estilo que ela tem e se havia espaço antes dela.
struct Token {
    text: String,
    style: Style,
    space_before: bool,
}

fn flush(out: &mut Vec<Token>, cur: &mut String, style: Style, space_before: bool) {
    if !cur.is_empty() {
        out.push(Token { text: std::mem::take(cur), style, space_before });
    }
}

// Divide uma linha em palavras. Com `markdown`, crases e `**` alternam o estilo e somem do texto.
fn tokens(text: &str, base: Style, p: &Palette, markdown: bool) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut code, mut bold) = (false, false);
    let (mut pending_space, mut cur_space) = (false, false);
    let mut cur_style = base;
    let style_now = |code: bool, bold: bool| {
        let s = if code { p.code } else { base };
        if bold { s.add_modifier(Modifier::BOLD) } else { s }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if markdown && c == '`' {
            flush(&mut out, &mut cur, cur_style, cur_space);
            code = !code;
            i += 1;
        } else if markdown && c == '*' && chars.get(i + 1) == Some(&'*') {
            flush(&mut out, &mut cur, cur_style, cur_space);
            bold = !bold;
            i += 2;
        } else if c.is_whitespace() {
            flush(&mut out, &mut cur, cur_style, cur_space);
            pending_space = true;
            i += 1;
        } else {
            if cur.is_empty() {
                cur_space = pending_space;
                pending_space = false;
                cur_style = style_now(code, bold);
            }
            cur.push(c);
            i += 1;
        }
    }
    flush(&mut out, &mut cur, cur_style, cur_space);
    out
}

// Parte uma palavra maior que a linha em pedaços que caibam.
fn chunks(word: &str, max: usize) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    chars.chunks(max.max(1)).map(|c| c.iter().collect()).collect()
}

// Quebra as palavras em linhas de no máximo `width` colunas. A primeira começa com `first`;
// as seguintes, com `hang` espaços.
fn wrap(words: Vec<Token>, width: usize, first: Vec<Span<'static>>, hang: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut cur = first;
    let mut cur_w: usize = cur.iter().map(Span::width).sum();
    let mut empty = true;
    for tok in words {
        for (k, piece) in chunks(&tok.text, width.saturating_sub(hang)).into_iter().enumerate() {
            let w = Span::raw(piece.as_str()).width();
            let mut space = !empty && k == 0 && tok.space_before;
            if !empty && cur_w + usize::from(space) + w > width {
                lines.push(Line::from(std::mem::replace(&mut cur, vec![Span::raw(" ".repeat(hang))])));
                cur_w = hang;
                space = false;
            }
            if space {
                cur.push(Span::raw(" "));
                cur_w += 1;
            }
            cur.push(Span::styled(piece, tok.style));
            cur_w += w;
            empty = false;
        }
    }
    lines.push(Line::from(cur));
    lines
}

// Texto simples (mensagem do usuário): preserva as quebras de linha e não interpreta Markdown.
pub fn plain(text: &str, width: usize, first: Span<'static>, style: Style, p: &Palette) -> Vec<Line<'static>> {
    let width = width.max(20);
    let hang = first.width();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let lead = if i == 0 { first.clone() } else { Span::raw(" ".repeat(hang)) };
        out.extend(wrap(tokens(line, style, p, false), width, vec![lead], hang));
    }
    out
}

// Resposta do modelo, com Markdown.
pub fn render(text: &str, width: usize, p: &Palette) -> Vec<Line<'static>> {
    let width = width.max(20);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut in_code = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let extra = raw.chars().count() - trimmed.chars().count();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            let line: String = raw.replace('\t', "    ").chars().take(width.saturating_sub(INDENT)).collect();
            out.push(Line::from(vec![Span::raw(" ".repeat(INDENT)), Span::styled(line, p.code)]));
            continue;
        }
        if trimmed.is_empty() {
            out.push(Line::default());
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        let lead = " ".repeat(INDENT + extra);
        let lines = if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let body = tokens(trimmed[hashes..].trim(), p.heading, p, true);
            wrap(body, width, vec![Span::raw(lead)], INDENT + extra)
        } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            let hang = INDENT + extra + 2;
            wrap(tokens(rest, p.text, p, true), width, vec![Span::raw(lead), Span::styled("• ", p.bullet)], hang)
        } else if let Some((num, rest)) = numbered(trimmed) {
            let prefix = format!("{num} ");
            let hang = INDENT + extra + prefix.chars().count();
            wrap(tokens(rest, p.text, p, true), width, vec![Span::raw(lead), Span::styled(prefix, p.bullet)], hang)
        } else {
            wrap(tokens(trimmed, p.text, p, true), width, vec![Span::raw(lead)], INDENT + extra)
        };
        out.extend(lines);
    }
    // Várias linhas em branco seguidas viram uma só.
    out.dedup_by(|b, a| a.spans.is_empty() && b.spans.is_empty());
    out
}

// "12. texto" → ("12.", "texto").
fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &line[digits..];
    let rest = rest.strip_prefix(". ")?;
    Some((&line[..digits + 1], rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn palette() -> Palette {
        Palette {
            text: Style::default(),
            code: Style::default().fg(Color::Gray),
            heading: Style::default().add_modifier(Modifier::BOLD),
            bullet: Style::default(),
        }
    }

    fn text_of(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn lines_never_exceed_the_width() {
        let long = "uma frase razoavelmente longa para forçar várias quebras de linha dentro do limite de largura";
        for width in [20, 30, 41] {
            for line in render(long, width, &palette()) {
                assert!(line.width() <= width, "largura {} > {width}: {line:?}", line.width());
            }
        }
    }

    #[test]
    fn inline_markers_are_removed_and_styled() {
        let lines = render("use `cargo test` e **sempre** rode", 80, &palette());
        assert_eq!(text_of(&lines), vec!["  use cargo test e sempre rode"]);
        let code = lines[0].spans.iter().find(|s| s.content == "cargo").unwrap();
        assert_eq!(code.style, palette().code);
        let bold = lines[0].spans.iter().find(|s| s.content == "sempre").unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn bullets_and_numbers_get_a_hanging_indent() {
        let lines = render("- primeiro item que é bem comprido e precisa quebrar a linha em duas", 30, &palette());
        let text = text_of(&lines);
        assert!(text[0].starts_with("  • primeiro"));
        assert!(text[1].starts_with("    "), "{text:?}");
        let numbered = text_of(&render("12. item", 30, &palette()));
        assert_eq!(numbered, vec!["  12. item"]);
    }

    #[test]
    fn code_blocks_keep_their_lines_and_hide_the_fences() {
        let lines = render("antes\n```rust\nfn main() {}\n    let x = 1;\n```\ndepois", 40, &palette());
        assert_eq!(text_of(&lines), vec!["  antes", "  fn main() {}", "      let x = 1;", "  depois"]);
    }

    #[test]
    fn headings_are_bold_and_blank_runs_collapse() {
        let lines = render("## Título\n\n\n\ntexto", 40, &palette());
        assert_eq!(text_of(&lines), vec!["  Título", "", "  texto"]);
        assert!(lines[0].spans.iter().any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn plain_text_keeps_markers_and_newlines() {
        let lines = plain("veja `a` e\nsegunda linha", 40, Span::raw("› "), Style::default(), &palette());
        assert_eq!(text_of(&lines), vec!["› veja `a` e", "  segunda linha"]);
    }

    #[test]
    fn very_long_words_are_split() {
        let word = "x".repeat(70);
        for line in render(&word, 30, &palette()) {
            assert!(line.width() <= 30);
        }
    }
}

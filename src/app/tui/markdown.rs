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
    // Classes de token dos blocos de código com linguagem conhecida (bash, python).
    pub keyword: Style,
    pub string: Style,
    pub comment: Style,
    pub number: Style,
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
    let mut code_lang: Option<Lang> = None;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        let extra = raw.chars().count() - trimmed.chars().count();
        if let Some(info) = trimmed.strip_prefix("```") {
            in_code = !in_code;
            code_lang = if in_code { Lang::of(info) } else { None };
            continue;
        }
        if in_code {
            let line: String = raw.replace('\t', "    ").chars().take(width.saturating_sub(INDENT)).collect();
            let mut spans = vec![Span::raw(" ".repeat(INDENT))];
            spans.extend(highlight(&line, code_lang, p));
            out.push(Line::from(spans));
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

// Linguagens com realce: a que vem depois das crases de abertura (```bash). As outras ficam no estilo de código.
#[derive(Clone, Copy, PartialEq)]
enum Lang {
    Bash,
    Python,
}

impl Lang {
    fn of(info: &str) -> Option<Lang> {
        match info.split_whitespace().next()?.to_lowercase().as_str() {
            "bash" | "sh" | "zsh" | "shell" => Some(Lang::Bash),
            "python" | "python3" | "py" => Some(Lang::Python),
            _ => None,
        }
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Lang::Bash => &[
                "if", "then", "else", "elif", "fi", "for", "in", "while", "until", "do", "done", "case", "esac", "function", "select",
                "return", "exit", "local", "export", "readonly", "set", "unset", "shift", "break", "continue",
            ],
            Lang::Python => &[
                "def", "class", "return", "import", "from", "as", "if", "elif", "else", "for", "while", "in", "not", "and", "or", "is",
                "None", "True", "False", "try", "except", "finally", "with", "lambda", "pass", "break", "continue", "raise", "yield",
                "async", "await", "global", "nonlocal", "del", "assert",
            ],
        }
    }
}

// Quebra uma linha de código em trechos com estilo. Sem linguagem conhecida, a linha inteira fica no estilo de código.
fn highlight(line: &str, lang: Option<Lang>, p: &Palette) -> Vec<Span<'static>> {
    let Some(lang) = lang else {
        return vec![Span::styled(line.to_string(), p.code)];
    };
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // `#` começa comentário, exceto `$#` (número de argumentos do bash).
        if c == '#' && !(lang == Lang::Bash && i > 0 && chars[i - 1] == '$') {
            flush_plain(&mut out, &mut plain, p);
            out.push(Span::styled(chars[i..].iter().collect::<String>(), p.comment));
            break;
        }
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                if c == '"' && chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            flush_plain(&mut out, &mut plain, p);
            out.push(Span::styled(chars[i..end].iter().collect::<String>(), p.string));
            i = end;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            if lang.keywords().contains(&word.as_str()) {
                flush_plain(&mut out, &mut plain, p);
                out.push(Span::styled(word, p.keyword));
            } else {
                plain.push_str(&word);
            }
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            flush_plain(&mut out, &mut plain, p);
            out.push(Span::styled(chars[i..j].iter().collect::<String>(), p.number));
            i = j;
            continue;
        }
        plain.push(c);
        i += 1;
    }
    flush_plain(&mut out, &mut plain, p);
    out
}

// Põe o texto simples acumulado como um trecho no estilo de código.
fn flush_plain(out: &mut Vec<Span<'static>>, plain: &mut String, p: &Palette) {
    if !plain.is_empty() {
        out.push(Span::styled(std::mem::take(plain), p.code));
    }
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
            keyword: Style::default().add_modifier(Modifier::BOLD),
            string: Style::default().fg(Color::Green),
            comment: Style::default().fg(Color::DarkGray),
            number: Style::default().fg(Color::Yellow),
        }
    }

    #[test]
    fn bash_and_python_blocks_get_colored_tokens() {
        let p = palette();
        let bash = render("```bash\nif [ -d \"$1\" ]; then echo ok # teste\nfi\n```", 60, &p);
        let spans: Vec<(String, Style)> = bash[0].spans.iter().map(|s| (s.content.to_string(), s.style)).collect();
        assert!(spans.iter().any(|(t, s)| t == "if" && *s == p.keyword), "{spans:?}");
        assert!(spans.iter().any(|(t, s)| t == "\"$1\"" && *s == p.string), "{spans:?}");
        assert!(spans.iter().any(|(t, s)| t.starts_with('#') && *s == p.comment), "{spans:?}");
        let py = render("```python\ndef f(x):\n    return 2 * x  # dobro\n```", 60, &p);
        let spans: Vec<(String, Style)> = py[1].spans.iter().map(|s| (s.content.to_string(), s.style)).collect();
        assert!(spans.iter().any(|(t, s)| t == "return" && *s == p.keyword), "{spans:?}");
        assert!(spans.iter().any(|(t, s)| t == "2" && *s == p.number), "{spans:?}");
        let text: String = py[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "      return 2 * x  # dobro", "o realce não muda o texto");
    }

    #[test]
    fn other_languages_keep_the_plain_code_style() {
        let p = palette();
        let lines = render("```rust\nlet x = 1;\n```", 40, &p);
        assert_eq!(lines[0].spans[1].style, p.code);
        assert_eq!(lines[0].spans[1].content, "let x = 1;");
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

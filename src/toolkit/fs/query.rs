//! O texto procurado pelas buscas: alternativas com `|`, comparação sem maiúsculas nem acentos e a
//! busca ampliada quando nada é encontrado.
//!
//! O pedido chega em português ("a cor amarela"), mas o código costuma estar em inglês (`Yellow`).
//! Modelos pequenos procuram as palavras do pedido ao pé da letra e, sem resultado, passam a adivinhar
//! caminhos. A busca ampliada tenta as palavras soltas e a tradução das mais comuns em código.

// Termos que o modelo pediu: `"yellow|amarelo"` vira dois. Vazios ficam de fora.
pub(super) fn terms(pattern: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in pattern.split('|').map(normalize) {
        let t = t.trim().to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

// Minúsculas e sem acentos, caractere por caractere: o resultado tem o mesmo número de caracteres
// que a entrada, então uma posição achada nele vale no texto original.
pub(super) fn normalize(text: &str) -> String {
    text.chars().map(fold).collect()
}

fn fold(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        _ => c,
    }
}

// Posição (em caracteres) do primeiro termo que aparece na linha já normalizada. Termos de até 3
// caracteres só valem como palavra inteira: `red` não acha `required`, `ui` não acha `build`.
pub(super) fn find_any(line: &str, terms: &[String]) -> Option<usize> {
    terms.iter().filter_map(|t| find_term(line, t)).map(|b| line[..b].chars().count()).min()
}

fn find_term(line: &str, term: &str) -> Option<usize> {
    if term.chars().count() > 3 {
        return line.find(term);
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
    line.match_indices(term)
        .map(|(b, _)| b)
        .find(|&b| !word(line[..b].chars().next_back()) && !word(line[b + term.len()..].chars().next()))
}

// Palavras de português comuns em pedidos e seus nomes usuais em código. A chave é um começo de palavra,
// para cobrir gênero e número (amarelo, amarela, amarelos).
const TRANSLATIONS: &[(&str, &[&str])] = &[
    ("amarel", &["yellow"]),
    ("vermelh", &["red"]),
    ("verde", &["green"]),
    ("azul", &["blue"]),
    ("azuis", &["blue"]),
    ("cinza", &["gray", "grey"]),
    ("branc", &["white"]),
    ("pret", &["black"]),
    ("laranja", &["orange"]),
    ("rox", &["purple", "magenta"]),
    ("rosa", &["pink"]),
    ("cor", &["color", "colour"]),
    ("tema", &["theme", "palette"]),
    ("estilo", &["style"]),
    ("negrito", &["bold"]),
    ("botao", &["button"]),
    ("botoes", &["button"]),
    ("aviso", &["warn"]),
    ("erro", &["error"]),
    ("titulo", &["title"]),
    ("cabecalho", &["header"]),
    ("rodape", &["footer"]),
    ("tecla", &["key"]),
    ("atalho", &["shortcut", "key"]),
    ("janela", &["window"]),
    ("painel", &["panel"]),
    ("barra", &["bar"]),
    ("caixa", &["box"]),
    ("borda", &["border"]),
    ("fundo", &["background"]),
    ("lista", &["list"]),
    ("tabela", &["table"]),
    ("entrada", &["input"]),
    ("saida", &["output"]),
    ("arquivo", &["file"]),
    ("pasta", &["dir", "folder"]),
    ("configurac", &["config"]),
    ("ferramenta", &["tool"]),
    ("modelo", &["model"]),
    ("esforco", &["effort"]),
    ("historico", &["history"]),
    ("comando", &["command"]),
    ("mensagem", &["message"]),
    ("usuario", &["user"]),
    ("senha", &["password"]),
    ("autentica", &["auth"]),
    ("teste", &["test"]),
    ("interface", &["ui", "tui", "view"]),
    ("tela", &["screen", "view"]),
];

// Palavras que não ajudam a achar nada sozinhas.
const STOPWORDS: &[&str] = &[
    "para", "como", "onde", "qual", "quais", "esta", "isso", "aqui", "fica", "ficam", "muda", "mude", "troque",
    "trocar", "altere", "alterar", "deixe", "deixar", "com", "sem", "que", "uma", "das", "dos", "pela", "pelo",
];

// Termos da busca ampliada: cada palavra útil do pedido sozinha e a tradução das conhecidas, sem repetir
// os termos originais. Vazio quando não há o que tentar.
pub(super) fn broaden(original: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: &str| {
        let t = t.to_string();
        if !original.contains(&t) && !out.contains(&t) {
            out.push(t);
        }
    };
    for term in original {
        let words: Vec<&str> = term.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| !w.is_empty()).collect();
        for word in &words {
            for (stem, english) in TRANSLATIONS {
                // `cor` só como palavra inteira (ou plural): como começo, pegaria "corpo", "correto".
                let hit = if stem.len() <= 3 { *word == *stem || *word == format!("{stem}es") } else { word.starts_with(stem) };
                if hit {
                    english.iter().for_each(|e| push(e));
                }
            }
        }
        if words.len() > 1 {
            words.iter().filter(|w| w.chars().count() >= 4 && !STOPWORDS.contains(w)).for_each(|w| push(w));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_split_alternatives_and_fold_accents() {
        assert_eq!(terms("Yellow| Amarelo |"), vec!["yellow", "amarelo"]);
        assert_eq!(normalize("Configuração"), "configuracao");
        assert_eq!(normalize("Ação").chars().count(), "Ação".chars().count());
    }

    #[test]
    fn find_any_returns_the_char_position() {
        let line = normalize("// cor: Color::Yellow");
        assert_eq!(find_any(&line, &terms("yellow|cor")), Some(3));
        assert_eq!(find_any(&line, &terms("azul")), None);
        assert_eq!(find_any("fields: required", &terms("red")), None);
        assert_eq!(find_any("color::red;", &terms("red")), Some(7));
        assert_eq!(find_any("mod ui;", &terms("ui")), Some(4));
    }

    #[test]
    fn broaden_translates_and_splits_phrases() {
        assert_eq!(broaden(&terms("amarela")), vec!["yellow"]);
        assert_eq!(broaden(&terms("interface do terminal")), vec!["ui", "tui", "view", "interface", "terminal"]);
        assert_eq!(broaden(&terms("cores")), vec!["color", "colour"]);
        assert!(broaden(&terms("corpo")).is_empty(), "cor só como palavra inteira");
        assert!(broaden(&terms("yellow")).is_empty());
    }
}

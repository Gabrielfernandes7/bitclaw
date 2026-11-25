//! Menção de arquivos com `@`: digitar `@docs/` lista o que há na pasta, e Tab ou Enter completa.

use std::path::{Component, Path};

// Quantas sugestões aparecem de uma vez.
pub(super) const MENTION_ROWS: usize = 8;

// Pastas que quase nunca interessam numa menção.
const SKIP: &[&str] = &["target", "node_modules"];

// A menção aberta: onde começa na linha e as sugestões.
pub(super) struct Mention {
    pub(super) start: usize,
    pub(super) entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    // Caminho relativo ao projeto, com `/` no fim se for pasta.
    pub(super) path: String,
    pub(super) is_dir: bool,
}

// A menção sob o cursor: onde ela começa na linha (em caracteres, no `@`) e o texto depois do `@`
// até o cursor. `None` se o cursor não está numa palavra que começa com `@`.
pub(super) fn token_at(line: &str, cursor: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut start = cursor;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    (chars.get(start) == Some(&'@') && start < cursor).then(|| (start, chars[start + 1..cursor].iter().collect()))
}

// Sugestões para o texto depois do `@`: o que está na pasta citada e começa com o resto. Pastas
// vêm primeiro, depois por nome. Nunca sai da pasta do projeto.
pub(super) fn suggest(root: &Path, token: &str) -> Vec<Entry> {
    let (dir_part, prefix) = token.rsplit_once('/').map_or(("", token), |(d, p)| (&token[..d.len() + 1], p));
    let outside = Path::new(dir_part).components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if outside {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(root.join(dir_part)) else { return Vec::new() };
    let prefix_lower = prefix.to_lowercase();
    let mut found: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let hidden = name.starts_with('.') && !prefix.starts_with('.');
            if hidden || SKIP.contains(&name.as_str()) || !name.to_lowercase().starts_with(&prefix_lower) {
                return None;
            }
            let is_dir = e.file_type().ok()?.is_dir();
            let path = format!("{dir_part}{name}{}", if is_dir { "/" } else { "" });
            // O que já está digitado por inteiro não precisa ser sugerido de novo.
            (path != token).then_some(Entry { path, is_dir })
        })
        .collect();
    found.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase())));
    found.truncate(MENTION_ROWS);
    found
}

// A linha com a menção trocada pela sugestão. Arquivo ganha um espaço depois, para seguir
// escrevendo; pasta não, para continuar a descer. Devolve a linha nova e a coluna do cursor.
pub(super) fn apply(line: &str, start: usize, cursor: usize, entry: &Entry) -> (String, usize) {
    let chars: Vec<char> = line.chars().collect();
    let cursor = cursor.min(chars.len());
    let before: String = chars[..start].iter().collect();
    let after: String = chars[cursor..].iter().collect();
    let inserted = format!("@{}{}", entry.path, if entry.is_dir { "" } else { " " });
    let col = start + inserted.chars().count();
    (format!("{before}{inserted}{after}"), col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn project() -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("bitclaw-mention-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join("README.md"), "").unwrap();
        fs::write(dir.join("docs/PROJETO.md"), "").unwrap();
        fs::write(dir.join("docs/CHOICE.md"), "").unwrap();
        dir
    }

    fn paths(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn finds_the_mention_under_the_cursor() {
        assert_eq!(token_at("veja @docs/", 11), Some((5, "docs/".into())));
        assert_eq!(token_at("veja @docs/ e mais", 11), Some((5, "docs/".into())), "só até o cursor");
        assert_eq!(token_at("@", 1), Some((0, String::new())), "só o @ já lista a pasta do projeto");
        assert_eq!(token_at("veja docs/", 10), None, "sem @");
        assert_eq!(token_at("a@b", 3), None, "@ no meio de uma palavra não conta");
        assert_eq!(token_at("veja @docs/ ", 12), None, "o espaço encerra a menção");
    }

    #[test]
    fn lists_the_project_root_with_folders_first_and_skips_noise() {
        let root = project();
        let got = suggest(&root, "");
        assert_eq!(paths(&got), vec!["docs/", "src/", "README.md"], "sem .git nem target");
        assert!(got[0].is_dir && !got[2].is_dir);
    }

    #[test]
    fn lists_a_folder_and_filters_by_prefix_ignoring_case() {
        let root = project();
        assert_eq!(paths(&suggest(&root, "docs/")), vec!["docs/CHOICE.md", "docs/PROJETO.md"]);
        assert_eq!(paths(&suggest(&root, "docs/pro")), vec!["docs/PROJETO.md"]);
        assert_eq!(paths(&suggest(&root, "do")), vec!["docs/"]);
        assert_eq!(paths(&suggest(&root, "zzz")), Vec::<&str>::new());
    }

    #[test]
    fn a_complete_path_is_not_suggested_again_but_a_folder_name_gets_its_slash() {
        let root = project();
        assert!(suggest(&root, "docs/PROJETO.md").is_empty());
        assert_eq!(paths(&suggest(&root, "docs")), vec!["docs/"]);
    }

    #[test]
    fn hidden_entries_show_only_when_asked_for() {
        let root = project();
        assert_eq!(paths(&suggest(&root, ".g")), vec![".git/"]);
    }

    #[test]
    fn never_leaves_the_project() {
        let root = project();
        assert!(suggest(&root, "../").is_empty());
        assert!(suggest(&root, "docs/../../").is_empty());
        assert!(suggest(&root, "/etc/").is_empty());
    }

    #[test]
    fn applying_replaces_only_the_mention() {
        let file = Entry { path: "docs/PROJETO.md".into(), is_dir: false };
        let dir = Entry { path: "docs/".into(), is_dir: true };
        assert_eq!(apply("leia @do e resuma", 5, 8, &dir), ("leia @docs/ e resuma".to_string(), 11));
        assert_eq!(apply("leia @docs/P", 5, 12, &file), ("leia @docs/PROJETO.md ".to_string(), 22));
        assert_eq!(apply("@", 0, 1, &dir), ("@docs/".to_string(), 6));
    }
}

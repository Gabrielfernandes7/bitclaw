//! Índice de símbolos do projeto e recuperação para a tarefa. Um modelo pequeno explora mal
//! (lista, lê, deduz); aqui o harness faz a busca e entrega "arquivo:linha: declaração" já pronta.
//!
//! Sem dependências: as declarações vêm do mesmo reconhecedor do `read_file` (`mode="outline"`) e
//! a ordenação é um TF-IDF simples sobre os nomes. O resultado é uma dica; o modelo confere com
//! `read_file` antes de afirmar.

use super::read_file::is_outline_line;
use super::walk::{relative, walk};
use crate::core::retrieve::Retriever;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const EXTENSIONS: &[&str] = &["rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "c", "h", "cpp", "md", "toml"];
const MAX_FILE_BYTES: u64 = 200_000;
const MAX_HITS: usize = 5;
const PER_FILE: usize = 2;
const MAX_LINE: usize = 110;
// Pontuação mínima para sugerir algo: abaixo disso é ruído e não vale gastar contexto.
const MIN_SCORE: f32 = 1.5;
const TTL: Duration = Duration::from_secs(30);

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "where", "what", "how", "file", "code", "uma", "para", "com", "que", "dos",
    "das", "nos", "nas", "por", "como", "onde", "qual", "quais", "esse", "essa", "isso", "arquivo", "projeto", "faça", "fazer", "função",
];

struct Symbol {
    path: String,
    line: usize,
    text: String,
    // Termos do nome (caminho + declaração), já em minúsculas.
    terms: Vec<String>,
}

#[derive(Default)]
pub struct SymbolIndex {
    cache: Mutex<Option<(Instant, Vec<Symbol>)>>,
}

// Quebra em termos: separa por não alfanuméricos, `_` e fronteiras camelCase.
fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let mut current = String::new();
        let mut prev_lower = false;
        for c in word.chars() {
            if c.is_uppercase() && prev_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            prev_lower = c.is_lowercase();
            current.extend(c.to_lowercase());
        }
        out.push(current);
    }
    out.retain(|t| t.chars().count() >= 3 && !STOPWORDS.contains(&t.as_str()));
    out
}

fn matches(query: &str, term: &str) -> bool {
    term == query
        || (query.len() >= 4 && term.starts_with(query))
        || (term.len() >= 4 && query.starts_with(term))
}

fn build(root: &Path) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for file in walk(root, root, false).entries {
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !EXTENSIONS.contains(&ext) || fs::metadata(&file).map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(true) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&file) else { continue };
        let path = relative(root, &file);
        let path_terms = terms(&path);
        for (i, line) in content.lines().enumerate() {
            // Os testes não são a API do arquivo.
            if line.starts_with("#[cfg(test)]") {
                break;
            }
            if !is_outline_line(line) {
                continue;
            }
            let text: String = line.trim().chars().take(MAX_LINE).collect();
            let mut t = terms(&text);
            t.extend(path_terms.iter().cloned());
            symbols.push(Symbol { path: path.clone(), line: i + 1, text, terms: t });
        }
    }
    symbols
}

fn rank<'a>(symbols: &'a [Symbol], task: &str) -> Vec<&'a Symbol> {
    let mut query = terms(task);
    query.sort();
    query.dedup();
    if query.is_empty() || symbols.is_empty() {
        return Vec::new();
    }
    // Quantos símbolos têm cada termo da pergunta: termo raro pesa mais.
    let n = symbols.len() as f32;
    let weight: HashMap<&str, f32> = query
        .iter()
        .map(|q| {
            let df = symbols.iter().filter(|s| s.terms.iter().any(|t| matches(q, t))).count() as f32;
            (q.as_str(), (1.0 + n / (1.0 + df)).ln())
        })
        .collect();
    let mut scored: Vec<(f32, &Symbol)> = symbols
        .iter()
        .map(|s| {
            let score: f32 = query.iter().filter(|q| s.terms.iter().any(|t| matches(q, t))).map(|q| weight[q.as_str()]).sum();
            (score, s)
        })
        .filter(|(score, _)| *score >= MIN_SCORE)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)).then(a.1.line.cmp(&b.1.line)));
    let mut per_file: HashMap<&str, usize> = HashMap::new();
    let mut seen = HashSet::new();
    scored
        .into_iter()
        .map(|(_, s)| s)
        .filter(|s| {
            let n = per_file.entry(s.path.as_str()).or_insert(0);
            *n += 1;
            *n <= PER_FILE && seen.insert((s.path.as_str(), s.line))
        })
        .take(MAX_HITS)
        .collect()
}

fn render(hits: &[&Symbol]) -> Option<String> {
    if hits.is_empty() {
        return None;
    }
    let lines: Vec<String> = hits.iter().map(|s| format!("{}:{}: {}", s.path, s.line, s.text)).collect();
    Some(format!("[Pistas do índice do projeto: confira com read_file (start_line/end_line) antes de afirmar]\n{}", lines.join("\n")))
}

impl Retriever for SymbolIndex {
    fn retrieve(&self, root: &Path, task: &str) -> Option<String> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.as_ref().is_none_or(|(at, _)| at.elapsed() > TTL) {
            *cache = Some((Instant::now(), build(root)));
        }
        let (_, symbols) = cache.as_ref()?;
        render(&rank(symbols, task))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-index-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/sandbox.rs"), "//! caminhos\npub fn resolve_path(p: &str) {}\n").unwrap();
        fs::write(dir.join("src/theme.rs"), "pub const fn accent_color() {}\npub fn yellow_badge() {}\n").unwrap();
        fs::write(dir.join("src/other.rs"), "pub fn unrelated() {}\n#[cfg(test)]\nmod t { pub fn resolve_path_test() {} }\n").unwrap();
        dir
    }

    #[test]
    fn terms_split_snake_and_camel_case_and_drop_stopwords() {
        assert_eq!(terms("resolvePath_now the"), vec!["resolve", "path", "now"]);
    }

    #[test]
    fn the_task_finds_the_declaration_that_matches_its_words() {
        let dir = project("find");
        let out = SymbolIndex::default().retrieve(&dir, "onde o sandbox resolve o caminho path?").unwrap();
        assert!(out.contains("src/sandbox.rs:2: pub fn resolve_path"), "{out}");
        assert!(!out.contains("unrelated"), "{out}");
    }

    #[test]
    fn a_stem_matches_longer_names() {
        let dir = project("stem");
        let out = SymbolIndex::default().retrieve(&dir, "mude o badge yellow para outra cor").unwrap();
        assert!(out.contains("yellow_badge"), "{out}");
    }

    #[test]
    fn a_task_with_no_match_adds_nothing() {
        let dir = project("none");
        assert!(SymbolIndex::default().retrieve(&dir, "olá, tudo bem?").is_none());
    }

    #[test]
    fn test_modules_are_not_indexed() {
        let dir = project("tests");
        let out = SymbolIndex::default().retrieve(&dir, "resolve_path_test").unwrap_or_default();
        assert!(!out.contains("resolve_path_test"), "{out}");
    }
}

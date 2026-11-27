//! Percurso dos arquivos do projeto para as buscas (`grep_files`, `find_files`).
//!
//! Fica de fora, como no ripgrep: o que o `.gitignore` da raiz ignora, arquivos e pastas ocultos
//! (`.env`, `.git`) e pastas geradas. Uma pasta dessas passada explicitamente em `path` é percorrida:
//! a regra vale para o que se encontra pelo caminho, não para o ponto de partida.

use std::fs;
use std::path::{Path, PathBuf};

// Pastas geradas ou de dependências, puladas mesmo sem `.gitignore`.
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "build", "__pycache__"];
// Teto de entradas visitadas: um projeto enorme não trava o turno.
const MAX_ENTRIES: usize = 20_000;

// Regras do `.gitignore` da raiz que valem sem um motor de glob completo: nomes (`target/`),
// caminhos (`docs/gerado`) e sufixos (`*.log`). Linhas com outros curingas ou `!` são ignoradas.
#[derive(Default)]
struct Ignore {
    names: Vec<String>,
    paths: Vec<String>,
    suffixes: Vec<String>,
}

impl Ignore {
    fn load(root: &Path) -> Self {
        let mut rules = Self::default();
        let Ok(text) = fs::read_to_string(root.join(".gitignore")) else { return rules };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let anchored = line.starts_with('/');
            let rule = line.trim_matches('/');
            if let Some(suffix) = rule.strip_prefix('*').filter(|s| !s.contains(['*', '?', '[', '/'])) {
                rules.suffixes.push(suffix.to_string());
            } else if rule.contains(['*', '?', '[']) || rule.is_empty() {
                continue;
            } else if anchored || rule.contains('/') {
                rules.paths.push(rule.to_string());
            } else {
                rules.names.push(rule.to_string());
            }
        }
        rules
    }

    // `rel` é o caminho relativo à raiz, com `/`.
    fn skips(&self, rel: &str, name: &str) -> bool {
        self.names.iter().any(|n| n == name)
            || self.suffixes.iter().any(|s| name.ends_with(s.as_str()))
            || self.paths.iter().any(|p| rel == p || rel.strip_prefix(p.as_str()).is_some_and(|r| r.starts_with('/')))
    }
}

pub(super) struct Walk {
    // Arquivos (e pastas, se pedidas) em ordem estável.
    pub(super) entries: Vec<PathBuf>,
    // Entradas ocultas ou ignoradas deixadas de fora, para o aviso de busca vazia.
    pub(super) skipped: usize,
    // O teto de entradas foi atingido.
    pub(super) capped: bool,
}

// Percorre `start` (já resolvido dentro de `root`). Não segue symlinks.
pub(super) fn walk(root: &Path, start: &Path, with_dirs: bool) -> Walk {
    let ignore = Ignore::load(root);
    let mut walk = Walk { entries: Vec::new(), skipped: 0, capped: false };
    let mut stack = vec![start.to_path_buf()];
    let mut visited = 0;
    while let Some(path) = stack.pop() {
        visited += 1;
        if visited > MAX_ENTRIES {
            walk.capped = true;
            break;
        }
        let Ok(meta) = path.symlink_metadata() else { continue };
        if meta.is_file() {
            walk.entries.push(path);
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        if with_dirs && path != start {
            walk.entries.push(path.clone());
        }
        let Ok(read) = fs::read_dir(&path) else { continue };
        let mut children = Vec::new();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = entry.path();
            let rel = relative(root, &child);
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if name.starts_with('.') || (is_dir && SKIP_DIRS.contains(&name.as_str())) || ignore.skips(&rel, &name) {
                walk.skipped += 1;
                continue;
            }
            children.push(child);
        }
        children.sort();
        stack.extend(children.into_iter().rev());
    }
    walk
}

// Caminho relativo à raiz, com `/` em qualquer sistema.
pub(super) fn relative(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

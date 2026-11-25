//! Teste de fronteira: a regra de dependência de `docs/ARCHITECTURE.md`, verificada no código-fonte.
//!
//! O core não conhece ninguém; adaptadores e toolkit só conhecem o core. Os testes de cada módulo
//! (`tests.rs` e blocos `#[cfg(test)]`) ficam de fora: eles podem montar um agente completo.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "tests.rs") {
            out.push(path);
        }
    }
}

// Linhas de código de produção que citam um caminho proibido. Comentários não contam.
fn violations(text: &str, forbidden: &[&str]) -> Vec<String> {
    let production = text.split("#[cfg(test)]").next().unwrap_or(text);
    production
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .filter(|line| forbidden.iter().any(|f| line.contains(f)))
        .map(|line| line.trim().to_string())
        .collect()
}

fn check(layer: &str, forbidden: &[&str]) {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(layer), &mut files);
    assert!(!files.is_empty(), "nenhum arquivo em src/{layer}");
    let mut found = Vec::new();
    for file in files {
        for line in violations(&fs::read_to_string(&file).unwrap(), forbidden) {
            found.push(format!("{}: {line}", file.display()));
        }
    }
    assert!(found.is_empty(), "src/{layer} quebra a regra de dependência:\n{}", found.join("\n"));
}

#[test]
fn core_knows_nobody() {
    check("core", &["crate::adapters", "crate::toolkit", "crate::app", "bitclaw::"]);
}

#[test]
fn adapters_only_know_the_core() {
    check("adapters", &["crate::toolkit", "crate::app"]);
}

#[test]
fn toolkit_only_knows_the_core() {
    check("toolkit", &["crate::adapters", "crate::app"]);
}

#[test]
fn the_checker_catches_a_violation_and_ignores_comments_and_tests() {
    let bad = "use crate::toolkit::fs;\nfn x() {}\n";
    assert_eq!(violations(bad, &["crate::toolkit"]).len(), 1);
    let fine = "// veja crate::toolkit\nfn x() {}\n#[cfg(test)]\nmod tests { use crate::toolkit::fs; }\n";
    assert!(violations(fine, &["crate::toolkit"]).is_empty());
}

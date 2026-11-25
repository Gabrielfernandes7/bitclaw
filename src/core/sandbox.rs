//! Resolve caminhos do agente dentro da pasta do projeto: recusa caminhos absolutos, `..` e
//! symlinks que saem dela.

use std::path::{Component, Path, PathBuf};

// Resolve um caminho relativo dentro de `root` (já canonicalizado).
// Recusa caminhos absolutos, `..` e qualquer caminho que, após resolver
// symlinks, saia de `root`.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("caminho vazio".into());
    }
    // ESC, CR, quebra de linha e afins nunca fazem parte de um nome legítimo. Aparecem em prévias,
    // passos e erros, onde poderiam esconder texto ou injetar comandos de terminal.
    if rel.chars().any(char::is_control) {
        return Err("caminho com caracteres de controle não é permitido".into());
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err("caminho absoluto não é permitido".into());
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("'..' não é permitido".into());
    }

    let joined = root.join(p);

    // Sobe até o ancestral mais próximo que existe. `symlink_metadata` enxerga também
    // links quebrados: `exists()` os trata como ausentes e deixaria a escrita seguir o link.
    let mut existing = joined.clone();
    while existing.symlink_metadata().is_err() {
        if !existing.pop() {
            return Err("caminho inválido".into());
        }
    }
    // Para um link quebrado, canonicalize falha: recusa em vez de confiar no destino.
    let real = existing
        .canonicalize()
        .map_err(|_| "link simbólico quebrado ou inacessível no caminho".to_string())?;
    if !real.starts_with(root) {
        return Err("caminho fora do diretório permitido (symlink?)".into());
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn rejects_control_characters_in_paths() {
        let root = setup("ctrl");
        for bad in ["a\x1b[2Jb.sh", "a\rb", "a\nb", "dir/\x07.txt"] {
            assert!(resolve(&root, bad).is_err(), "{bad:?}");
        }
        assert!(resolve(&root, "com espaço/é-ã.txt").is_ok(), "acentos e espaços continuam valendo");
    }

    #[test]
    fn allows_inside_paths() {
        let root = setup("ok");
        assert!(resolve(&root, "sub/novo.txt").is_ok());
        assert!(resolve(&root, ".").is_ok());
    }

    #[test]
    fn blocks_escape_attempts() {
        let root = setup("esc");
        assert!(resolve(&root, "../x").is_err());
        assert!(resolve(&root, "/etc/passwd").is_err());
        assert!(resolve(&root, "").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn blocks_symlink_escape() {
        let root = setup("link");
        let outside = std::env::temp_dir().join(format!("bitclaw-outside-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("sub/link")).unwrap();
        assert!(resolve(&root, "sub/link/arquivo.txt").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn blocks_dangling_symlink_escape() {
        // Link quebrado apontando para fora: a escrita seguiria o link para fora do projeto.
        let root = setup("dangling");
        let outside = std::env::temp_dir().join(format!("bitclaw-dangling-target-{}", std::process::id()));
        let _ = fs::remove_file(&outside);
        std::os::unix::fs::symlink(&outside, root.join("sub/quebrado.txt")).unwrap();
        assert!(resolve(&root, "sub/quebrado.txt").is_err());
        assert!(!outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn blocks_dangling_symlink_inside_root_when_target_is_outside() {
        // Link quebrado dentro do projeto, mas com um diretório pai apontando para fora.
        let root = setup("dangling-dir");
        let outside = std::env::temp_dir().join(format!("bitclaw-dangling-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        std::os::unix::fs::symlink(&outside, root.join("sub/fora")).unwrap();
        assert!(resolve(&root, "sub/fora/novo.txt").is_err());
        assert!(!outside.exists());
    }

    #[test]
    fn still_allows_new_file_in_existing_dir() {
        let root = setup("novo");
        assert!(resolve(&root, "sub/novo.txt").is_ok());
    }
}

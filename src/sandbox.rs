use std::path::{Component, Path, PathBuf};

/// Resolve um caminho relativo dentro de `root` (já canonicalizado).
/// Recusa caminhos absolutos, `..` e qualquer caminho que, após resolver
/// symlinks, saia de `root`.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("caminho vazio".into());
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err("caminho absoluto não é permitido".into());
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("'..' não é permitido".into());
    }

    let joined = root.join(p);

    // Sobe até o ancestral mais próximo que existe e verifica onde ele realmente está.
    let mut existing = joined.clone();
    while !existing.exists() {
        if !existing.pop() {
            return Err("caminho inválido".into());
        }
    }
    let real = existing.canonicalize().map_err(|e| e.to_string())?;
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
}

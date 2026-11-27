//! Registro dos projetos onde o BitClaw rodou: uma pasta por linha em `~/.config/bitclaw/projects`.
//!
//! Serve a um só fim: o `bitclaw uninstall --purge` precisa saber onde estão as pastas `.bitclaw/`
//! (backup, recibos, `/undo`, skills instaladas) para não deixar resíduo. O arquivo guarda só o
//! caminho do projeto, nunca o conteúdo, e some junto com o estado global.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "projects";

fn file() -> Option<PathBuf> {
    super::update::config_dir().map(|d| d.join(FILE))
}

// Anota `root` (se ainda não está). Falha em silêncio: o registro nunca atrapalha o uso.
pub fn record(root: &Path) {
    if let Some(file) = file() {
        let _ = record_in(&file, root);
    }
}

fn record_in(file: &Path, root: &Path) -> std::io::Result<()> {
    if read_from(file).iter().any(|p| p == root) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(file)?;
    writeln!(f, "{}", root.display())
}

// Os projetos anotados, sem repetição e só caminhos absolutos.
pub fn list() -> Vec<PathBuf> {
    file().map(|f| read_from(&f)).unwrap_or_default()
}

fn read_from(file: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for line in fs::read_to_string(file).unwrap_or_default().lines() {
        let p = PathBuf::from(line.trim());
        if p.is_absolute() && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-projects-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_project_is_recorded_once() {
        let dir = tempdir("once");
        let file = dir.join("cfg/projects");
        record_in(&file, Path::new("/a/proj")).unwrap();
        record_in(&file, Path::new("/a/proj")).unwrap();
        record_in(&file, Path::new("/b/outro")).unwrap();
        assert_eq!(read_from(&file), vec![PathBuf::from("/a/proj"), PathBuf::from("/b/outro")]);
    }

    #[test]
    fn relative_lines_and_blanks_are_ignored() {
        let dir = tempdir("rel");
        let file = dir.join("projects");
        fs::write(&file, "/ok\n\n../fora\nrelativo\n  /ok  \n").unwrap();
        assert_eq!(read_from(&file), vec![PathBuf::from("/ok")]);
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        assert!(read_from(&std::env::temp_dir().join("bitclaw-nao-existe/projects")).is_empty());
    }
}

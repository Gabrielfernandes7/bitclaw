//! `/restart-dev`: recompila o BitClaw e o reabre no lugar, para ver alterações do código sem sair
//! do terminal. Só faz sentido em desenvolvimento, com o código-fonte ainda no lugar onde foi compilado.

use std::path::{Path, PathBuf};
use std::process::Command;

// Quantas linhas do fim da saída do compilador aparecem quando a compilação falha.
const ERROR_TAIL: usize = 15;

// Pasta do projeto onde este binário foi compilado, se ela ainda existe.
fn source_dir() -> Result<PathBuf, String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    if dir.join("Cargo.toml").is_file() {
        Ok(dir.to_path_buf())
    } else {
        Err("o código-fonte não está mais em ".to_string() + &dir.display().to_string() + "; /restart-dev só funciona em desenvolvimento.")
    }
}

// Recompila com o mesmo perfil do binário em execução. A saída do cargo é capturada para não
// bagunçar o terminal; em caso de erro, devolve o fim dela.
pub(super) fn build() -> Result<(), String> {
    let dir = source_dir()?;
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--color", "never", "--manifest-path"]).arg(dir.join("Cargo.toml"));
    if !cfg!(debug_assertions) {
        cmd.arg("--release");
    }
    let out = cmd.output().map_err(|e| format!("não consegui rodar o cargo: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(tail(&String::from_utf8_lossy(&out.stderr), ERROR_TAIL))
}

// Últimas `n` linhas não vazias de um texto.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

// Troca este processo pelo binário recém-compilado, com os mesmos argumentos. Só volta se falhar.
#[cfg(unix)]
pub(super) fn exec_self() -> String {
    use std::os::unix::process::CommandExt;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return format!("não achei o próprio binário: {e}"),
    };
    let err = Command::new(exe).args(std::env::args_os().skip(1)).exec();
    format!("não consegui reabrir o BitClaw: {err}")
}

#[cfg(not(unix))]
pub(super) fn exec_self() -> String {
    "/restart-dev só funciona em sistemas Unix; rode o cargo run de novo.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_tail_keeps_only_the_last_non_empty_lines() {
        assert_eq!(tail("a\n\nb\nc\n\nd\n", 2), "c\nd");
        assert_eq!(tail("só uma", 5), "só uma");
        assert_eq!(tail("", 3), "");
    }

    #[test]
    fn the_source_dir_exists_when_running_from_the_repo() {
        assert!(source_dir().unwrap().join("Cargo.toml").is_file());
    }
}

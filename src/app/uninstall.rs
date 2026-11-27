//! `bitclaw uninstall`: apaga o binário instalado e o estado global (`~/.config/bitclaw`).
//! As pastas `.bitclaw/` dentro dos projetos (backup, recibos, `/undo`) ficam: são do projeto.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

// O que será apagado. Recusa binário de desenvolvimento e o do `cargo install`, que têm o próprio jeito.
fn plan(exe: &Path, config: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    let shown = exe.display().to_string();
    if shown.contains("/target/debug/") || shown.contains("/target/release/") {
        return Err(format!("{shown} é um binário de desenvolvimento (cargo build). Não há o que desinstalar; apague a pasta target se quiser."));
    }
    if shown.contains("/.cargo/bin/") {
        return Err("este bitclaw veio do cargo install. Desinstale com: cargo uninstall bitclaw".into());
    }
    let mut paths = vec![exe.to_path_buf()];
    if let Some(dir) = config.filter(|d| d.exists()) {
        paths.push(dir.to_path_buf());
    }
    Ok(paths)
}

fn remove(paths: &[PathBuf]) -> Result<(), String> {
    for p in paths {
        let result = if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
        result.map_err(|e| format!("não consegui apagar {}: {e}", p.display()))?;
    }
    Ok(())
}

// O comando inteiro. Sem `--yes`, mostra o que vai apagar e pergunta.
pub fn run(yes: bool) -> Result<String, String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| format!("não achei o próprio binário: {e}"))?;
    let config = super::update::config_dir();
    let paths = plan(&exe, config.as_deref())?;
    println!("vou apagar:");
    for p in &paths {
        println!("  {}", p.display());
    }
    println!("as pastas .bitclaw/ dentro dos seus projetos ficam (backup, recibos e /undo).");
    if !yes {
        print!("confirma? [s/N] ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer).map_err(|e| e.to_string())?;
        if !matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes") {
            return Ok("nada foi apagado.".into());
        }
    }
    remove(&paths)?;
    Ok("bitclaw desinstalado. Até a próxima!".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-uninstall-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn removes_the_binary_and_the_global_state_only() {
        let dir = tempdir("ok");
        let exe = dir.join("bin/bitclaw");
        let config = dir.join("config/bitclaw");
        let project = dir.join("projeto/.bitclaw");
        for d in [exe.parent().unwrap(), &config, &project] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(&exe, "bin").unwrap();
        fs::write(config.join("welcomed"), "").unwrap();
        let paths = plan(&exe, Some(&config)).unwrap();
        assert_eq!(paths, vec![exe.clone(), config.clone()]);
        remove(&paths).unwrap();
        assert!(!exe.exists() && !config.exists());
        assert!(project.exists(), "a pasta do projeto fica");
    }

    #[test]
    fn missing_config_is_skipped_and_dev_or_cargo_binaries_are_refused() {
        let dir = tempdir("skip");
        let exe = dir.join("bitclaw");
        assert_eq!(plan(&exe, Some(&dir.join("nada"))).unwrap(), vec![exe]);
        assert!(plan(Path::new("/x/ClawBit/target/release/bitclaw"), None).unwrap_err().contains("desenvolvimento"));
        assert!(plan(Path::new("/Users/a/.cargo/bin/bitclaw"), None).unwrap_err().contains("cargo uninstall"));
    }
}

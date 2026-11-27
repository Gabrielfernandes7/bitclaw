//! `bitclaw uninstall`: apaga o binário instalado e todo o resíduo do BitClaw: o estado global
//! (`~/.config/bitclaw`), restos de instalação e atualização e, com `--purge`, as pastas `.bitclaw/`
//! dos projetos (backup, recibos, `/undo`, skills instaladas). O `AGENTS.md` e o código ficam: são seus.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

// O que será apagado, em grupos, para mostrar e confirmar cada um.
#[derive(Debug, PartialEq)]
struct Plan {
    // O binário. `None` quando é de desenvolvimento ou do `cargo install`, que têm o próprio jeito.
    binary: Option<PathBuf>,
    // Por que o binário fica (a dica de como removê-lo).
    binary_note: Option<String>,
    config: Option<PathBuf>,
    // Restos de instalação e atualização: `.bitclaw.install`, `.bitclaw.update` e `bitclaw-update-*` no temp.
    leftovers: Vec<PathBuf>,
    // Pastas `.bitclaw/` de projetos (as anotadas e a da pasta atual).
    projects: Vec<PathBuf>,
}

fn binary_verdict(exe: &Path) -> Result<(), String> {
    let shown = exe.display().to_string();
    if shown.contains("/target/debug/") || shown.contains("/target/release/") {
        return Err(format!("{shown} é um binário de desenvolvimento (cargo build): fica onde está; apague a pasta target se quiser."));
    }
    if shown.contains("/.cargo/bin/") {
        return Err("o binário veio do cargo install: desinstale com `cargo uninstall bitclaw`.".into());
    }
    Ok(())
}

// Restos que instalar e atualizar podem deixar para trás se forem interrompidos.
fn leftovers(exe: &Path, tmp: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = exe.parent() {
        for name in [".bitclaw.install", ".bitclaw.update"] {
            let p = dir.join(name);
            if p.symlink_metadata().is_ok() {
                out.push(p);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(tmp) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with("bitclaw-update-") || n.starts_with("bitclaw-install.") || n.starts_with("bitclaw-fresh-")
            })
            .map(|e| e.path())
            .collect();
        found.sort();
        out.extend(found);
    }
    out
}

// A pasta `.bitclaw/` de cada projeto, se existe de verdade: um diretório com esse nome, nunca um link.
// O nome fixo é a trava: o registro é um arquivo de texto e não pode apontar para outra coisa.
fn project_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        let dir = root.join(".bitclaw");
        let real = dir.symlink_metadata().is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
        if real && dir.file_name().is_some_and(|n| n == ".bitclaw") && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

fn plan(exe: &Path, config: Option<&Path>, tmp: &Path, roots: &[PathBuf]) -> Plan {
    let verdict = binary_verdict(exe);
    Plan {
        binary: verdict.is_ok().then(|| exe.to_path_buf()),
        binary_note: verdict.err(),
        config: config.filter(|d| d.exists()).map(Path::to_path_buf),
        leftovers: leftovers(exe, tmp),
        projects: project_dirs(roots),
    }
}

fn size(p: &Path) -> u64 {
    match p.symlink_metadata() {
        Ok(m) if m.is_dir() => fs::read_dir(p).map(|rd| rd.flatten().map(|e| size(&e.path())).sum()).unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} B"),
    }
}

fn remove(paths: &[PathBuf]) -> Result<(), String> {
    for p in paths {
        let is_dir = p.symlink_metadata().is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
        let result = if is_dir { fs::remove_dir_all(p) } else { fs::remove_file(p) };
        result.map_err(|e| format!("não consegui apagar {}: {e}", p.display()))?;
    }
    Ok(())
}

fn confirmed(question: &str) -> Result<bool, String> {
    print!("{question} [s/N] ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer).map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes"))
}

// O comando inteiro. Sem `--yes`, mostra o que vai apagar e pergunta. `purge` inclui as pastas `.bitclaw/`
// dos projetos; sem ele, elas são listadas e (num terminal) perguntadas à parte.
pub fn run(yes: bool, purge: bool) -> Result<String, String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| format!("não achei o próprio binário: {e}"))?;
    let mut roots = super::projects::list();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.canonicalize().unwrap_or(cwd));
    }
    let plan = plan(&exe, super::update::config_dir().as_deref(), &std::env::temp_dir(), &roots);

    let mut now: Vec<PathBuf> = Vec::new();
    now.extend(plan.binary.clone());
    now.extend(plan.config.clone());
    now.extend(plan.leftovers.clone());
    if purge {
        now.extend(plan.projects.clone());
    }
    if now.is_empty() && plan.projects.is_empty() {
        return Ok(match plan.binary_note {
            Some(note) => format!("não há resíduo do BitClaw para apagar. {note}"),
            None => "não há nada para apagar.".into(),
        });
    }

    println!("vou apagar:");
    for p in &now {
        println!("  {} ({})", p.display(), human(size(p)));
    }
    if let Some(note) = &plan.binary_note {
        println!("fica: {note}");
    }
    if !plan.projects.is_empty() && !purge {
        println!("pastas .bitclaw/ de projetos (backup, recibos, /undo e skills instaladas):");
        for p in &plan.projects {
            println!("  {} ({})", p.display(), human(size(p)));
        }
    }
    println!("o AGENTS.md e o código dos seus projetos não são tocados.");

    if !yes && !now.is_empty() && !confirmed("confirma?")? {
        return Ok("nada foi apagado.".into());
    }
    // Sem `--purge`, num terminal e sem `--yes`: pergunta à parte pelas pastas dos projetos.
    if !purge && !yes && !plan.projects.is_empty() && confirmed("apagar também as pastas .bitclaw/ desses projetos?")? {
        now.extend(plan.projects.clone());
    }
    remove(&now)?;
    // O registro de projetos some junto com o estado global: o que ficou precisa estar na mensagem.
    let kept_dirs: Vec<String> = plan.projects.iter().filter(|p| !now.contains(p)).map(|p| p.display().to_string()).collect();
    let kept = if kept_dirs.is_empty() {
        String::new()
    } else {
        format!(" Ficaram as pastas .bitclaw/ de: {}. Apague à mão se quiser (rm -rf), ou rode de novo com --purge antes de desinstalar.", kept_dirs.join(", "))
    };
    Ok(format!("bitclaw desinstalado. Até a próxima!{kept}"))
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
    fn removes_the_binary_the_global_state_and_the_leftovers() {
        let dir = tempdir("ok");
        let exe = dir.join("bin/bitclaw");
        let config = dir.join("config/bitclaw");
        let tmp = dir.join("tmp");
        for d in [exe.parent().unwrap(), &config, &tmp.join("bitclaw-update-123")] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(&exe, "bin").unwrap();
        fs::write(dir.join("bin/.bitclaw.install"), "x").unwrap();
        fs::write(config.join("welcomed"), "").unwrap();
        fs::write(tmp.join("outra-coisa"), "não é do bitclaw").unwrap();
        let p = plan(&exe, Some(&config), &tmp, &[]);
        assert_eq!(p.binary, Some(exe.clone()));
        assert_eq!(p.config, Some(config.clone()));
        assert_eq!(p.leftovers, vec![dir.join("bin/.bitclaw.install"), tmp.join("bitclaw-update-123")]);
        let all: Vec<PathBuf> = [p.binary.clone().into_iter().collect::<Vec<_>>(), p.config.into_iter().collect(), p.leftovers].concat();
        remove(&all).unwrap();
        assert!(!exe.exists() && !config.exists() && !tmp.join("bitclaw-update-123").exists());
        assert!(tmp.join("outra-coisa").exists(), "só apaga o que é do bitclaw");
    }

    #[test]
    fn project_folders_are_found_only_when_they_are_real_bitclaw_directories() {
        let dir = tempdir("proj");
        let (a, b, c, d) = (dir.join("a"), dir.join("b"), dir.join("c"), dir.join("d"));
        for p in [&a, &b, &c, &d] {
            fs::create_dir_all(p).unwrap();
        }
        fs::create_dir_all(a.join(".bitclaw/backup")).unwrap();
        fs::write(a.join(".bitclaw/backup/x"), "conteúdo").unwrap();
        // `.bitclaw` que é arquivo, e `.bitclaw` que é link para outra pasta (que não pode ser apagada).
        fs::write(b.join(".bitclaw"), "arquivo").unwrap();
        let alvo = dir.join("alvo-importante");
        fs::create_dir_all(&alvo).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&alvo, c.join(".bitclaw")).unwrap();
        let found = project_dirs(&[a.clone(), a.clone(), b, c.clone(), d, dir.join("nao-existe")]);
        assert_eq!(found, vec![a.join(".bitclaw")]);
        remove(&found).unwrap();
        assert!(!a.join(".bitclaw").exists() && a.exists());
        assert!(alvo.exists(), "o link nunca é seguido");
    }

    #[test]
    fn dev_and_cargo_binaries_stay_but_the_rest_is_still_cleaned() {
        let dir = tempdir("skip");
        let config = dir.join("cfg");
        fs::create_dir_all(&config).unwrap();
        let tmp = dir.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let dev = plan(Path::new("/x/ClawBit/target/release/bitclaw"), Some(&config), &tmp, &[]);
        assert_eq!(dev.binary, None);
        assert!(dev.binary_note.unwrap().contains("desenvolvimento"));
        assert_eq!(dev.config, Some(config.clone()));
        let cargo = plan(Path::new("/Users/a/.cargo/bin/bitclaw"), None, &tmp, &[]);
        assert_eq!(cargo.binary, None);
        assert!(cargo.binary_note.unwrap().contains("cargo uninstall"));
        assert_eq!(cargo.config, None);
    }

    #[test]
    fn sizes_read_in_human_units() {
        assert_eq!(human(10), "10 B");
        assert_eq!(human(2048), "2 KB");
        assert_eq!(human(3 << 20), "3.0 MB");
    }
}

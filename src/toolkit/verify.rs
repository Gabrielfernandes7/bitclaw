//! Verificação depois de alterar: `cargo check` para Rust e `py_compile` para Python. O erro volta
//! ao modelo curto e cortado: um erro do compilador é o sinal mais barato e confiável que um modelo
//! pequeno pode receber para se corrigir.

use crate::core::agent::{Action, Verifier};
use crate::core::tool::Outcome;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(90);
const MAX_LINES: usize = 12;
const MAX_CHARS: usize = 1500;

#[derive(Default)]
pub struct ProjectCheck;

// Caminhos alterados com sucesso neste turno, pela convenção `path` dos argumentos.
fn changed(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .filter(|a| a.mutates && matches!(a.outcome, Outcome::Done(_)))
        .filter_map(|a| a.args.get("path").and_then(|p| p.as_str()).map(String::from))
        .collect()
}

// Roda o comando com prazo. `Err` = não rodou ou estourou; `Ok((sucesso, saída))` caso contrário.
fn run(mut cmd: Command, dir: &Path) -> Result<(bool, String), String> {
    let mut child = cmd
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
                return Ok((status.success(), text));
            }
            None if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err("tempo esgotado".into());
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

// Só o que importa do erro: linhas de erro e a localização, no máximo `MAX_LINES`.
fn compact(output: &str) -> String {
    let mut lines: Vec<&str> = output
        .lines()
        .filter(|l| l.contains("error") || l.trim_start().starts_with("-->") || l.contains("Error") || l.contains("File \""))
        .collect();
    if lines.is_empty() {
        lines = output.lines().collect();
    }
    lines.truncate(MAX_LINES);
    let text = lines.join("\n");
    text.chars().take(MAX_CHARS).collect()
}

impl Verifier for ProjectCheck {
    fn verify(&self, root: &Path, actions: &[Action]) -> Option<String> {
        let paths = changed(actions);
        if paths.iter().any(|p| p.ends_with(".rs")) && root.join("Cargo.toml").exists() {
            let mut cmd = Command::new("cargo");
            cmd.args(["check", "--quiet", "--message-format=short"]);
            return match run(cmd, root) {
                Ok((false, out)) => Some(format!("cargo check falhou:\n{}", compact(&out))),
                _ => None,
            };
        }
        let py: Vec<&String> = paths.iter().filter(|p| p.ends_with(".py")).collect();
        if !py.is_empty() {
            let mut cmd = Command::new("python3");
            cmd.args(["-m", "py_compile"]).args(py);
            return match run(cmd, root) {
                Ok((false, out)) => Some(format!("python não compila:\n{}", compact(&out))),
                _ => None,
            };
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn done(path: &str) -> Action {
        Action { name: "create_file".into(), args: json!({"path": path}), outcome: Outcome::Done("ok".into()), mutates: true }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bitclaw-verify-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_python_syntax_error_comes_back_short() {
        let d = dir("py-bad");
        std::fs::write(d.join("a.py"), "def f(:\n  pass\n").unwrap();
        let msg = ProjectCheck.verify(&d, &[done("a.py")]).expect("deveria falhar");
        assert!(msg.contains("não compila") && msg.contains("a.py"), "{msg}");
    }

    #[test]
    fn valid_python_and_unrelated_files_pass_silently() {
        let d = dir("py-ok");
        std::fs::write(d.join("a.py"), "x = 1\n").unwrap();
        assert!(ProjectCheck.verify(&d, &[done("a.py")]).is_none());
        assert!(ProjectCheck.verify(&d, &[done("notas.txt")]).is_none());
    }

    #[test]
    fn failed_or_read_only_actions_are_not_checked() {
        let d = dir("py-skip");
        std::fs::write(d.join("a.py"), "def f(:\n").unwrap();
        let mut a = done("a.py");
        a.outcome = Outcome::Error("x".into());
        assert!(ProjectCheck.verify(&d, &[a]).is_none());
    }

    #[test]
    fn compact_keeps_error_lines_and_caps_them() {
        let noisy = (0..40).map(|i| format!("error: problema {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(compact(&noisy).lines().count(), MAX_LINES);
    }
}

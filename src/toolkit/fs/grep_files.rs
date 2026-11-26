//! `grep_files`: procura um texto nos arquivos do projeto e devolve só `arquivo:linha: trecho`.
//! Achar primeiro e ler depois o trecho certo custa bem menos que ler arquivos inteiros.

use crate::core::sandbox::resolve;
use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

// Pastas que não são do projeto, geradas ou internas do BitClaw.
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "build", ".git", ".bitclaw"];
// Resultados no máximo; passando disso, o modelo recebe o aviso para refinar a busca.
const MAX_MATCHES: usize = 40;
// Arquivos maiores que isso (provavelmente gerados ou binários) são ignorados.
const MAX_FILE_BYTES: u64 = 1_000_000;
// Tamanho máximo de cada linha devolvida, em caracteres.
const MAX_LINE_CHARS: usize = 160;

pub struct GrepFiles;

impl Tool for GrepFiles {
    fn name(&self) -> &'static str {
        "grep_files"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "grep_files",
            "Procura um texto (sem regex, ignora maiúsculas) nos arquivos de uma pasta. Devolve arquivo:linha: trecho. \
             Use antes de read_file para achar o trecho certo.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Texto a procurar"},
                    "path": {"type": "string", "description": "Pasta ou arquivo; padrão \".\""}
                },
                "required": ["pattern"]
            }),
        )
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(grep(ctx.root, args).map(Outcome::Done))
    }
}

fn grep(root: &Path, args: &Value) -> Result<String, String> {
    let pattern = str_arg(args, "pattern")?;
    if pattern.trim().is_empty() {
        return Err("'pattern' vazio".into());
    }
    let needle = pattern.to_lowercase();
    let start = resolve(root, args.get("path").and_then(Value::as_str).unwrap_or("."))?;

    let mut found: Vec<String> = Vec::new();
    let mut total = 0;
    for file in files_under(&start) {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        let shown = file.strip_prefix(root).unwrap_or(&file).display().to_string();
        for (i, line) in text.lines().enumerate() {
            if !line.to_lowercase().contains(&needle) {
                continue;
            }
            total += 1;
            if found.len() < MAX_MATCHES {
                let line = line.trim();
                let cut: String = line.chars().take(MAX_LINE_CHARS).collect();
                let cut = if cut.len() < line.len() { format!("{}…", cut.trim_end()) } else { cut };
                found.push(format!("{shown}:{}: {cut}", i + 1));
            }
        }
    }
    if found.is_empty() {
        return Ok(format!("nada encontrado para \"{pattern}\""));
    }
    if total > found.len() {
        found.push(format!("[+{} resultados omitidos; refine o texto ou a pasta]", total - found.len()));
    }
    Ok(found.join("\n"))
}

// Arquivos sob `start`, em ordem estável. Não segue symlinks e pula as pastas de `SKIP_DIRS`.
fn files_under(start: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![start.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(meta) = path.symlink_metadata() else { continue };
        if meta.is_file() {
            if meta.len() <= MAX_FILE_BYTES {
                files.push(path);
            }
        } else if meta.is_dir() {
            let Ok(read) = fs::read_dir(&path) else { continue };
            let mut children: Vec<PathBuf> = read
                .flatten()
                .filter(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
                .map(|e| e.path())
                .collect();
            children.sort();
            stack.extend(children.into_iter().rev());
        }
    }
    files
}

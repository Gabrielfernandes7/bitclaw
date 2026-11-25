//! `create_file`: cria um arquivo novo, com prévia e aprovação.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Tool, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use crate::core::tool::{Preview, PreviewLine};
use std::fs;

pub struct CreateFile;

impl Tool for CreateFile {
    fn name(&self) -> &'static str {
        "create_file"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "create_file",
                "description": "Cria um arquivo NOVO no projeto com o conteúdo dado. Cria também as pastas que faltam no caminho (ex.: skills/guia.md cria a pasta skills). Falha se o arquivo já existir (use edit_file nesse caso). O usuário confirma antes da criação.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto, ex.: fib.py"},
                        "content": {"type": "string", "description": "Conteúdo completo do arquivo"}
                    },
                    "required": ["path", "content"]
                }
            }
        })
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome {
        wrap(create_file(root, args, approver))
    }
}

fn create_file(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let content = str_arg(args, "content")?;
    let path = resolve(root, rel)?;
    // symlink_metadata também pega links quebrados, que `exists()` não enxerga.
    if path.symlink_metadata().is_ok() {
        return Err(format!("{rel} já existe; use edit_file para alterar um arquivo existente"));
    }

    let preview = Preview {
        title: format!("novo arquivo {rel}"),
        lines: content.lines().map(|l| PreviewLine::Added(l.to_string())).collect(),
    };
    if !approver.approve(&preview) {
        return Ok(Outcome::Refused("usuário recusou a criação; nenhum arquivo foi criado".into()));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!("arquivo criado: {rel}")))
}

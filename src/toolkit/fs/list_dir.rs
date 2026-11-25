//! `list_dir`: lista o conteúdo de uma pasta do projeto.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Tool, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use std::fs;

pub struct ListDir;

impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "list_dir",
                "description": "Lista arquivos e pastas de um diretório do projeto. Use \".\" para a raiz.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"}
                    },
                    "required": ["path"]
                }
            }
        })
    }

    fn run(&self, root: &Path, args: &Value, _approver: &dyn Approver) -> Outcome {
        wrap(list_dir(root, args).map(Outcome::Done))
    }
}

fn list_dir(root: &Path, args: &Value) -> Result<String, String> {
    let path = resolve(root, str_arg(args, "path")?)?;
    let mut entries: Vec<String> = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() { format!("{name}/") } else { name }
        })
        .filter(|n| !n.starts_with(".bitclaw"))
        .collect();
    entries.sort();
    Ok(entries.join("\n"))
}

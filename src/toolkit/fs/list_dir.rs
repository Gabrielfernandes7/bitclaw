//! `list_dir`: lista o conteúdo de uma pasta do projeto.

use crate::core::sandbox::resolve;
use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use std::fs;

pub struct ListDir;

impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "list_dir",
            "Lista arquivos e pastas de um diretório do projeto. Use \".\" para a raiz.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Caminho relativo ao projeto"}
                },
                "required": ["path"]
            }),
        )
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: true }
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(list_dir(ctx.root, args).map(Outcome::Done))
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

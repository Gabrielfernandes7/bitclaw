//! `read_file`: lê um arquivo de texto do projeto, truncando o que passa do limite.

use crate::core::sandbox::resolve;
use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use crate::core::tool::MAX_READ_BYTES;
use std::fs;

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "read_file",
            "Lê o conteúdo de um arquivo de texto do projeto.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Caminho relativo ao projeto"}
                },
                "required": ["path"]
            }),
        )
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(read_file(ctx.root, args).map(Outcome::Done))
    }
}

fn read_file(root: &Path, args: &Value) -> Result<String, String> {
    let path = resolve(root, str_arg(args, "path")?)?;
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if content.len() > MAX_READ_BYTES {
        let mut cut = MAX_READ_BYTES;
        while !content.is_char_boundary(cut) {
            cut -= 1;
        }
        return Ok(format!("{}\n[... truncado em {MAX_READ_BYTES} bytes]", &content[..cut]));
    }
    Ok(content)
}

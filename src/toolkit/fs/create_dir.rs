//! `create_dir`: cria uma pasta nova e vazia, com prévia e aprovação.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use crate::core::tool::{Preview, PreviewLine};
use std::fs;

pub struct CreateDir;

impl Tool for CreateDir {
    fn name(&self) -> &'static str {
        "create_dir"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "create_dir",
            "Cria uma pasta NOVA e vazia no projeto, com as pastas pais que faltarem. Use quando o usuário pedir só a pasta, sem arquivo. Falha se já existir. O usuário confirma antes da criação.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Caminho relativo ao projeto da pasta, ex.: skills"}
                },
                "required": ["path"]
            }),
        )
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: false, destructive: false, idempotent: false }
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(create_dir(ctx.root, args, ctx.approver))
    }
}

fn create_dir(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let path = resolve(root, rel)?;
    if path.symlink_metadata().is_ok() {
        return Err(format!("{rel} já existe"));
    }
    let preview = Preview::new(format!("nova pasta {rel}"), vec![PreviewLine::Added(format!("{rel}/"))]);
    if !approver.approve(&preview) {
        return Ok(Outcome::Refused("usuário recusou a criação; nenhuma pasta foi criada".into()));
    }
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!("pasta criada: {rel}")))
}

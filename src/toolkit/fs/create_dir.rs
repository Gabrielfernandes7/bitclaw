//! `create_dir`: cria uma pasta nova e vazia, com prévia e aprovação.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Tool, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use crate::core::tool::{Preview, PreviewLine};
use std::fs;

pub struct CreateDir;

impl Tool for CreateDir {
    fn name(&self) -> &'static str {
        "create_dir"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "create_dir",
                "description": "Cria uma pasta NOVA e vazia no projeto, com as pastas pais que faltarem. Use quando o usuário pedir só a pasta, sem arquivo. Falha se já existir. O usuário confirma antes da criação.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto da pasta, ex.: skills"}
                    },
                    "required": ["path"]
                }
            }
        })
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome {
        wrap(create_dir(root, args, approver))
    }
}

fn create_dir(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let path = resolve(root, rel)?;
    if path.symlink_metadata().is_ok() {
        return Err(format!("{rel} já existe"));
    }
    let preview = Preview { title: format!("nova pasta {rel}"), lines: vec![PreviewLine::Added(format!("{rel}/"))] };
    if !approver.approve(&preview) {
        return Ok(Outcome::Refused("usuário recusou a criação; nenhuma pasta foi criada".into()));
    }
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!("pasta criada: {rel}")))
}

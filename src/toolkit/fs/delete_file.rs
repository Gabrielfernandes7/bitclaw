//! `delete_file`: remove um arquivo, com prévia, aprovação e backup.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use super::backup::backup;
use super::BACKUP_DIR;
use crate::core::tool::{Preview, PreviewLine};
use std::fs;

pub struct DeleteFile;

impl Tool for DeleteFile {
    fn name(&self) -> &'static str {
        "delete_file"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "delete_file",
            "Remove um arquivo existente do projeto. Guarda cópia em .bitclaw/backup/. Não remove pastas. O usuário confirma antes da remoção.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Caminho relativo ao projeto do arquivo a remover"}
                },
                "required": ["path"]
            }),
        )
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(delete_file(ctx.root, args, ctx.approver))
    }
}

fn delete_file(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let path = resolve(root, rel)?;
    if path.is_dir() {
        return Err(format!("{rel} é uma pasta; delete_file remove só arquivos"));
    }
    if !path.is_file() {
        return Err(format!("{rel} não existe"));
    }

    let content = fs::read_to_string(&path).unwrap_or_default();
    let preview = Preview {
        title: format!("remover arquivo {rel}"),
        lines: vec![PreviewLine::Note(format!("{rel} será removido ({} linhas)", content.lines().count()))],
    };
    if !approver.approve(&preview) {
        return Ok(Outcome::Refused(format!("usuário recusou a remoção; {rel} foi mantido")));
    }

    // Backup antes de remover, no mesmo formato da edição.
    backup(root, rel, &content)?;

    fs::remove_file(&path).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!("arquivo removido: {rel} (cópia em {BACKUP_DIR}/)")))
}

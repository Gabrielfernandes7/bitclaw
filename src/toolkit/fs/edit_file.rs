//! `edit_file`: substitui um trecho exato de um arquivo existente, com prévia, aprovação e backup.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Tool, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use super::backup::backup;
use super::preview::edit_preview;
use super::BACKUP_DIR;
use std::fs;

pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Substitui um trecho EXATO (`old`) por `new` em um arquivo existente. `old` deve aparecer exatamente uma vez no arquivo. Use o menor trecho que identifica a mudança (poucas linhas); nunca copie o arquivo inteiro em `old`. O usuário confirma antes da alteração. Leia o arquivo antes de editar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"},
                        "old": {"type": "string", "description": "Trecho exato e curto a ser substituído (poucas linhas, não o arquivo inteiro)"},
                        "new": {"type": "string", "description": "Novo texto"}
                    },
                    "required": ["path", "old", "new"]
                }
            }
        })
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome {
        wrap(edit_file(root, args, approver))
    }
}

fn edit_file(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let old = str_arg(args, "old")?;
    let new = str_arg(args, "new")?;
    if old.is_empty() {
        return Err("'old' não pode ser vazio".into());
    }
    let path = resolve(root, rel)?;
    if !path.is_file() {
        return Err("arquivo não existe (edit_file só altera arquivos existentes; use create_file para criar)".into());
    }

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let at = match content.matches(old).count() {
        0 => return Err("trecho 'old' não encontrado no arquivo. Use read_file e copie o trecho exato, caractere por caractere".into()),
        1 => content.find(old).unwrap_or(0),
        n => return Err(format!("trecho 'old' aparece {n} vezes; torne-o mais específico")),
    };
    let updated = content.replacen(old, new, 1);

    let preview = edit_preview(rel, &updated, content[..at].matches('\n').count(), old, new);
    if !approver.approve(&preview) {
        return Ok(Outcome::Refused("usuário recusou a edição; nada foi alterado".into()));
    }

    // Backup antes de escrever.
    backup(root, rel, &content)?;

    fs::write(&path, &updated).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!(
        "edição aplicada em {rel} (backup: {BACKUP_DIR}/). Não repita esta edição; se a tarefa estiver completa, responda ao usuário sem chamar mais ferramentas."
    )))
}

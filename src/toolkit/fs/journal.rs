//! Diário de alterações para `/undo`, em `.bitclaw/undo.jsonl`: uma linha por arquivo criado, editado ou
//! removido, com o caminho da cópia anterior (em `.bitclaw/backup`) e o digest do que ficou. Cada turno
//! começa com uma marca. `/undo` desfaz o último turno que alterou algo, mas só mexe em arquivo que ainda
//! está como o turno deixou: se o usuário editou depois, o arquivo fica como está e a resposta diz isso.

use crate::core::sandbox::resolve;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const JOURNAL_FILE: &str = ".bitclaw/undo.jsonl";

fn journal_path(root: &Path) -> PathBuf {
    root.join(JOURNAL_FILE)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

// Marca o começo de um turno. Chamado antes de cada tarefa enviada ao agente.
pub fn begin_turn(root: &Path) {
    append(root, &json!({"marca": "turno"}));
}

// Registra uma alteração que já foi feita. `copia` é o caminho (relativo ao projeto) da cópia anterior,
// se havia conteúdo; `depois` é o conteúdo que ficou, ou None se o arquivo ficou removido.
pub(super) fn record(root: &Path, op: &str, rel: &str, copia: Option<&str>, depois: Option<&[u8]>) {
    append(root, &json!({"op": op, "caminho": rel, "copia": copia, "depois": depois.map(digest)}));
}

fn append(root: &Path, entry: &Value) {
    let path = journal_path(root);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{entry}");
    }
}

// Desfaz o último turno com alterações e devolve o que aconteceu, linha por linha, para mostrar ao usuário.
// Turnos sem alteração são pulados e apagados do diário. Cada `/undo` recua um turno.
pub fn undo_last_turn(root: &Path) -> String {
    let path = journal_path(root);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();

    // Anda para trás até a marca do turno. `group` fica na ordem inversa das alterações.
    let mut group: Vec<Value> = Vec::new();
    while let Some(entry) = lines.pop() {
        if entry.get("marca").is_some() {
            if group.is_empty() {
                continue;
            }
            break;
        }
        group.push(entry);
    }
    if group.is_empty() {
        write_journal(&path, &lines);
        return "nada para desfazer: nenhum turno alterou arquivos".into();
    }

    // Primeiro o diário, depois os arquivos: o turno não fica registrado como desfeito se o diário não for gravado.
    if let Err(e) = write_journal_checked(&path, &lines) {
        return format!("não consegui atualizar {JOURNAL_FILE}: {e}; nada foi desfeito");
    }
    let report: Vec<String> = group.iter().map(|entry| revert(root, entry)).collect();
    format!("desfeito o último turno:\n{}", report.join("\n"))
}

fn revert(root: &Path, entry: &Value) -> String {
    let rel = entry["caminho"].as_str().unwrap_or("?");
    let Ok(path) = resolve(root, rel) else {
        return format!("{rel}: caminho fora do projeto; mantive");
    };
    let now = fs::read(&path).ok();
    let now_digest = now.as_deref().map(digest);
    let expected = entry["depois"].as_str();
    match entry["op"].as_str() {
        Some("criado") => {
            if now.is_none() {
                return format!("{rel}: já não existe");
            }
            if now_digest.as_deref() != expected {
                return format!("{rel}: mudou depois do turno; mantive");
            }
            match fs::remove_file(&path) {
                Ok(()) => format!("{rel}: removido (o turno o tinha criado)"),
                Err(e) => format!("{rel}: não consegui remover: {e}"),
            }
        }
        Some("escrito") => {
            if now.is_none() || now_digest.as_deref() != expected {
                return format!("{rel}: mudou depois do turno; mantive");
            }
            restore(root, entry, &path, rel, "voltou ao conteúdo anterior")
        }
        Some("removido") => {
            if now.is_some() {
                return format!("{rel}: já existe de novo; mantive");
            }
            restore(root, entry, &path, rel, "recriado a partir da cópia")
        }
        _ => format!("{rel}: entrada desconhecida no diário; mantive"),
    }
}

// Escreve de volta a cópia anterior guardada no backup.
fn restore(root: &Path, entry: &Value, path: &Path, rel: &str, done: &str) -> String {
    let Some(copia) = entry["copia"].as_str() else {
        return format!("{rel}: sem cópia anterior; mantive");
    };
    let content = match fs::read(root.join(copia)) {
        Ok(c) => c,
        Err(e) => return format!("{rel}: a cópia {copia} não pôde ser lida ({e}); mantive"),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::write(path, content) {
        Ok(()) => format!("{rel}: {done}"),
        Err(e) => format!("{rel}: não consegui escrever: {e}"),
    }
}

fn write_journal(path: &Path, lines: &[Value]) {
    let _ = write_journal_checked(path, lines);
}

fn write_journal_checked(path: &Path, lines: &[Value]) -> Result<(), String> {
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolkit::fs::{CreateFile, EditFile, DeleteFile};
    use crate::core::tool::{Approver, AutoApprover, Tool, ToolContext};
    use serde_json::json;

    fn root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bitclaw-undo-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn run(tool: &dyn Tool, root: &Path, args: Value, approver: &dyn Approver) {
        let ctx = ToolContext::new(root, approver);
        tool.run(&ctx, &args);
    }

    #[test]
    fn undo_reverts_edit_create_and_delete_of_the_last_turn() {
        let r = root("turn");
        fs::write(r.join("a.md"), "antes\n").unwrap();
        fs::write(r.join("b.md"), "para remover\n").unwrap();

        // Turno anterior: cria c.md (fica para o próximo undo).
        begin_turn(&r);
        run(&CreateFile, &r, json!({"path": "c.md", "content": "novo\n"}), &AutoApprover);

        // Último turno: edita a, remove b.
        begin_turn(&r);
        run(&EditFile, &r, json!({"path": "a.md", "old": "antes", "new": "depois"}), &AutoApprover);
        run(&DeleteFile, &r, json!({"path": "b.md"}), &AutoApprover);

        let first = undo_last_turn(&r);
        assert!(first.contains("a.md: voltou"), "{first}");
        assert!(first.contains("b.md: recriado"), "{first}");
        assert_eq!(fs::read_to_string(r.join("a.md")).unwrap(), "antes\n");
        assert_eq!(fs::read_to_string(r.join("b.md")).unwrap(), "para remover\n");
        assert!(r.join("c.md").exists(), "o turno anterior não foi desfeito");

        // Segundo /undo recua o turno anterior: c.md some.
        let second = undo_last_turn(&r);
        assert!(second.contains("c.md: removido"), "{second}");
        assert!(!r.join("c.md").exists());
        assert_eq!(undo_last_turn(&r), "nada para desfazer: nenhum turno alterou arquivos");
    }

    #[test]
    fn undo_leaves_files_the_user_changed_after_the_turn() {
        let r = root("edited-later");
        fs::write(r.join("a.md"), "x\n").unwrap();
        begin_turn(&r);
        run(&EditFile, &r, json!({"path": "a.md", "old": "x", "new": "y"}), &AutoApprover);
        fs::write(r.join("a.md"), "meu ajuste\n").unwrap();

        let report = undo_last_turn(&r);
        assert!(report.contains("a.md: mudou depois do turno; mantive"), "{report}");
        assert_eq!(fs::read_to_string(r.join("a.md")).unwrap(), "meu ajuste\n");
    }

    #[test]
    fn turns_without_changes_are_skipped() {
        let r = root("reads");
        fs::write(r.join("a.md"), "x\n").unwrap();
        begin_turn(&r);
        run(&EditFile, &r, json!({"path": "a.md", "old": "x", "new": "y"}), &AutoApprover);
        begin_turn(&r);
        begin_turn(&r);
        let report = undo_last_turn(&r);
        assert!(report.contains("a.md: voltou"), "{report}");
        assert_eq!(fs::read_to_string(r.join("a.md")).unwrap(), "x\n");
    }
}

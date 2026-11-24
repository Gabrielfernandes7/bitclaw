use crate::sandbox::resolve;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_READ_BYTES: usize = 100_000;
const BACKUP_DIR: &str = ".bitclaw/backup";

/// Esquemas das ferramentas no formato que o Ollama espera.
pub fn schemas() -> Value {
    json!([
        {
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
        },
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Lê o conteúdo de um arquivo de texto do projeto.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"}
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Substitui um trecho EXATO (`old`) por `new` em um arquivo existente. `old` deve aparecer exatamente uma vez no arquivo. O usuário confirma antes da alteração. Leia o arquivo antes de editar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"},
                        "old": {"type": "string", "description": "Trecho exato a ser substituído"},
                        "new": {"type": "string", "description": "Novo texto"}
                    },
                    "required": ["path", "old", "new"]
                }
            }
        }
    ])
}

/// Executa uma ferramenta. Erros voltam como texto para o modelo poder se corrigir.
pub fn execute(root: &Path, name: &str, args: &Value, auto_yes: bool) -> String {
    let result = match name {
        "list_dir" => list_dir(root, args),
        "read_file" => read_file(root, args),
        "edit_file" => edit_file(root, args, auto_yes),
        other => Err(format!("ferramenta desconhecida: {other}")),
    };
    result.unwrap_or_else(|e| format!("ERRO: {e}"))
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("argumento '{key}' ausente ou inválido"))
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

fn edit_file(root: &Path, args: &Value, auto_yes: bool) -> Result<String, String> {
    let rel = str_arg(args, "path")?;
    let old = str_arg(args, "old")?;
    let new = str_arg(args, "new")?;
    if old.is_empty() {
        return Err("'old' não pode ser vazio".into());
    }
    let path = resolve(root, rel)?;
    if !path.is_file() {
        return Err("arquivo não existe (edit_file só altera arquivos existentes)".into());
    }

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    match content.matches(old).count() {
        0 => return Err("trecho 'old' não encontrado no arquivo. Use read_file e copie o trecho exato, caractere por caractere".into()),
        1 => {}
        n => return Err(format!("trecho 'old' aparece {n} vezes; torne-o mais específico")),
    }

    // Mostra a mudança e pede confirmação.
    eprintln!("\n\x1b[1m── edição proposta em {rel} ──\x1b[0m");
    for line in old.lines() {
        eprintln!("\x1b[31m- {line}\x1b[0m");
    }
    for line in new.lines() {
        eprintln!("\x1b[32m+ {line}\x1b[0m");
    }
    if !auto_yes && !confirm()? {
        return Ok("usuário recusou a edição".into());
    }

    // Backup antes de escrever.
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let backup_name = rel.replace(['/', '\\'], "_");
    let backup_dir = root.join(BACKUP_DIR);
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let backup_path = backup_dir.join(format!("{backup_name}.{ts}.bak"));
    fs::write(&backup_path, &content).map_err(|e| e.to_string())?;

    fs::write(&path, content.replacen(old, new, 1)).map_err(|e| e.to_string())?;
    Ok(format!(
        "edição aplicada em {rel} (backup: {BACKUP_DIR}/). Não repita esta edição; se a tarefa estiver completa, responda ao usuário sem chamar mais ferramentas."
    ))
}

fn confirm() -> Result<bool, String> {
    eprint!("Aplicar? [s/N] ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes"))
}

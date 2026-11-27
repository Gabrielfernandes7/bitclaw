//! `read_file`: lê um arquivo de texto do projeto. Sem opções, devolve o arquivo (truncado no
//! limite); com `start_line`/`end_line`, só o trecho; com `mode: "outline"`, só a estrutura.

use crate::core::sandbox::resolve;
use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use crate::core::tool::MAX_READ_BYTES;
use std::fs;

// Limite de linhas do outline, para um arquivo enorme não virar o problema que ele resolve.
const MAX_OUTLINE: usize = 120;
// Tamanho máximo de cada linha do outline, em caracteres.
const MAX_OUTLINE_LINE: usize = 120;

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "read_file",
            "Lê um arquivo de texto do projeto. Para economizar contexto: mode=\"outline\" lista só a estrutura \
             (linha: declaração); start_line/end_line leem só um trecho.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Caminho relativo ao projeto"},
                    "mode": {"type": "string", "enum": ["full", "outline"], "description": "outline = só a estrutura"},
                    "start_line": {"type": "integer", "description": "Primeira linha (a partir de 1)"},
                    "end_line": {"type": "integer", "description": "Última linha, inclusive"}
                },
                "required": ["path"]
            }),
        )
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: true }
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(read_file(ctx.root, args).map(Outcome::Done))
    }
}

// Nomes da mesma pasta que o modelo provavelmente quis dizer: o mesmo nome sem extensão (README → README.md)
// ou com outra caixa (readme.md → README.md). Devolve caminhos relativos ao projeto, no máximo três.
fn similar_names(path: &Path, rel: &str) -> Vec<String> {
    let (Some(dir), Some(wanted)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { return Vec::new() };
    let wanted = wanted.to_lowercase();
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let prefix = Path::new(rel).parent().filter(|p| !p.as_os_str().is_empty());
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| {
            let lower = name.to_lowercase();
            lower == wanted || lower.strip_prefix(&wanted).is_some_and(|rest| rest.starts_with('.'))
        })
        .map(|name| match prefix {
            Some(p) => p.join(&name).to_string_lossy().into_owned(),
            None => name,
        })
        .collect();
    found.sort();
    found.truncate(3);
    found
}

// Número de linha vindo do modelo: aceita inteiro ou texto numérico ("10").
fn line_arg(args: &Value, key: &str) -> Result<Option<usize>, String> {
    let Some(v) = args.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
    match n {
        Some(n) if n >= 1 => Ok(Some(n as usize)),
        _ => Err(format!("'{key}' deve ser um inteiro a partir de 1")),
    }
}

fn read_file(root: &Path, args: &Value) -> Result<String, String> {
    let rel = str_arg(args, "path")?;
    let path = resolve(root, rel)?;
    // Erros que dizem o que fazer: modelos pequenos seguem a mensagem de erro melhor do que o prompt.
    if path.is_dir() {
        return Err(format!("'{rel}' é uma pasta, não um arquivo. Use list_dir para ver o que tem dentro dela"));
    }
    if !path.exists() {
        let similar = similar_names(&path, rel);
        let didyoumean = if similar.is_empty() { String::new() } else { format!(" Você quis dizer: {}?", similar.join(", ")) };
        return Err(format!(
            "'{rel}' não existe.{didyoumean} Use list_dir para ver o que há na pasta ou grep_files para achar o arquivo"
        ));
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let start = line_arg(args, "start_line")?;
    let end = line_arg(args, "end_line")?;
    let outline = args.get("mode").and_then(Value::as_str) == Some("outline");

    if outline {
        return Ok(outline_of(&content));
    }
    if start.is_some() || end.is_some() {
        return Ok(range_of(&content, start.unwrap_or(1), end));
    }
    if content.len() > MAX_READ_BYTES {
        let mut cut = MAX_READ_BYTES;
        while !content.is_char_boundary(cut) {
            cut -= 1;
        }
        let total = content.lines().count();
        return Ok(format!(
            "{}\n[... truncado em {MAX_READ_BYTES} bytes; o arquivo tem {total} linhas. \
             Use start_line/end_line para ler o resto ou mode=\"outline\" para ver a estrutura]",
            &content[..cut]
        ));
    }
    Ok(content)
}

// Trecho [start, end] (1-based, inclusive), com cabeçalho dizendo o que veio e o total.
fn range_of(content: &str, start: usize, end: Option<usize>) -> String {
    let total = content.lines().count();
    if start > total {
        return format!("[o arquivo tem só {total} linhas]");
    }
    let end = end.unwrap_or(total).clamp(start, total);
    let mut body: String = content.lines().skip(start - 1).take(end - start + 1).collect::<Vec<_>>().join("\n");
    let mut note = String::new();
    if body.len() > MAX_READ_BYTES {
        let mut cut = MAX_READ_BYTES;
        while !body.is_char_boundary(cut) {
            cut -= 1;
        }
        body.truncate(cut);
        note = format!("\n[... trecho truncado em {MAX_READ_BYTES} bytes; peça um intervalo menor]");
    }
    format!("[linhas {start}-{end} de {total}]\n{body}{note}")
}

// Estrutura do arquivo: comentários de módulo (`//!`), títulos Markdown e declarações, cada uma com
// o número da linha para depois pedir só o trecho com `start_line`.
fn outline_of(content: &str) -> String {
    let total = content.lines().count();
    let mut out = Vec::new();
    let mut more = 0;
    let mut tests_from = None;
    for (i, line) in content.lines().enumerate() {
        // Testes não são a API do arquivo: param aqui, senão os nomes de teste inundam a estrutura.
        if line.starts_with("#[cfg(test)]") {
            tests_from = Some(i + 1);
            break;
        }
        if !is_outline_line(line) {
            continue;
        }
        if out.len() == MAX_OUTLINE {
            more += 1;
            continue;
        }
        let text = line.trim_end();
        let shown: String = text.chars().take(MAX_OUTLINE_LINE).collect();
        let shown = if shown.len() < text.len() { format!("{}…", shown.trim_end()) } else { shown };
        out.push(format!("{}: {shown}", i + 1));
    }
    if out.is_empty() {
        return format!("[sem estrutura reconhecida em {total} linhas; use start_line/end_line]");
    }
    if more > 0 {
        out.push(format!("[+{more} linhas de estrutura omitidas]"));
    }
    if let Some(n) = tests_from {
        out.push(format!("[testes a partir da linha {n} omitidos]"));
    }
    format!("[outline de {total} linhas]\n{}", out.join("\n"))
}

// Declarações de nível de arquivo ou de impl (até um nível de indentação), nas linguagens mais comuns.
pub(super) fn is_outline_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.starts_with("//!") || trimmed.starts_with("#!") {
        return indent == 0;
    }
    if trimmed.starts_with('#') && trimmed.chars().find(|c| *c != '#') == Some(' ') {
        return indent == 0; // título Markdown
    }
    if indent > 4 {
        return false;
    }
    let mut rest = trimmed;
    for prefix in ["pub(crate) ", "pub(super) ", "pub ", "export default ", "export ", "async ", "unsafe ", "const "] {
        if let Some(r) = rest.strip_prefix(prefix) {
            rest = r;
        }
    }
    ["fn ", "struct ", "enum ", "trait ", "impl", "mod ", "type ", "def ", "class ", "function ", "interface ", "func ", "static "]
        .iter()
        .any(|k| rest.starts_with(k))
}

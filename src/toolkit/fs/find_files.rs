//! `find_files`: acha arquivos e pastas pelo nome ou pelo caminho, em todas as subpastas.
//! É outra operação que `grep_files`: aqui não se lê o conteúdo. Serve para "onde fica o widget de
//! status?" sem percorrer pasta por pasta com `list_dir`.

use super::query::{self, broaden, find_any, normalize};
use super::walk::{relative, walk};
use crate::core::sandbox::resolve;
use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// Caminhos devolvidos no máximo.
const MAX_RESULTS: usize = 40;

pub struct FindFiles;

impl Tool for FindFiles {
    fn name(&self) -> &'static str {
        "find_files"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "find_files",
            "Acha arquivos e pastas pelo NOME ou caminho (não lê o conteúdo), em todas as subpastas. \
             Ignora maiúsculas e acentos; separe alternativas com | (ex.: \"theme|color\", \".rs\", \"widgets/status\"). \
             Use quando souber parte do nome; para texto dentro dos arquivos use grep_files.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Parte do nome ou do caminho; alternativas separadas por |"},
                    "path": {"type": "string", "description": "Pasta onde procurar; padrão \".\" (o projeto todo)"}
                },
                "required": ["pattern"]
            }),
        )
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: true }
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(find(ctx.root, args).map(Outcome::Done))
    }
}

fn find(root: &Path, args: &Value) -> Result<String, String> {
    let pattern = str_arg(args, "pattern")?;
    let terms = query::terms(pattern);
    if terms.is_empty() {
        return Err("'pattern' vazio".into());
    }
    let start = resolve(root, args.get("path").and_then(Value::as_str).unwrap_or("."))?;
    let walked = walk(root, &start, true);

    let found = matching(root, &walked.entries, &terms);
    if !found.is_empty() {
        return Ok(render(found));
    }
    let wider = broaden(&terms);
    let found = matching(root, &walked.entries, &wider);
    if !wider.is_empty() && !found.is_empty() {
        return Ok(format!("nada com \"{pattern}\" no nome; busca ampliada com \"{}\":\n{}", wider.join("|"), render(found)));
    }
    let mut msg = format!(
        "nenhum arquivo ou pasta com \"{pattern}\" no nome ({} caminhos vistos). Tente parte do nome em inglês, \
         ou grep_files para procurar no conteúdo.",
        walked.entries.len()
    );
    if walked.skipped > 0 {
        msg.push_str(&format!(" {} itens ocultos ou ignorados (.gitignore, target) ficaram de fora.", walked.skipped));
    }
    Ok(msg)
}

// Caminhos relativos (pastas com `/` no fim) cujo caminho contém um dos termos.
fn matching(root: &Path, entries: &[PathBuf], terms: &[String]) -> Vec<String> {
    if terms.is_empty() {
        return Vec::new();
    }
    entries
        .iter()
        .filter_map(|p| {
            let rel = relative(root, p);
            find_any(&normalize(&rel), terms)?;
            Some(if p.is_dir() { format!("{rel}/") } else { rel })
        })
        .collect()
}

fn render(mut found: Vec<String>) -> String {
    let total = found.len();
    found.truncate(MAX_RESULTS);
    if total > MAX_RESULTS {
        found.push(format!("[+{} caminhos omitidos; refine o nome ou a pasta]", total - MAX_RESULTS));
    }
    found.join("\n")
}

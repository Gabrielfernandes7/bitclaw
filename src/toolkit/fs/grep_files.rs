//! `grep_files`: procura texto no conteúdo dos arquivos do projeto e devolve os trechos agrupados por
//! arquivo, com número de linha. Achar primeiro e ler depois o trecho certo custa bem menos que ler
//! arquivos inteiros.
//!
//! Feito para modelos pequenos: arquivos com uma definição (`const`, `fn`, `struct`...) vêm primeiro,
//! a saída tem teto e diz o que ficou de fora, e uma busca sem resultado é ampliada (palavras soltas e
//! tradução de termos comuns, ver `query`) antes de responder "nada encontrado".

use super::query::{self, broaden, find_any, normalize};
use super::walk::{relative, walk};
use crate::core::sandbox::resolve;
use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

// Linhas de resultado no máximo; passando disso, o modelo recebe a lista dos arquivos que sobraram.
const MAX_MATCHES: usize = 40;
// Linhas mostradas de um mesmo arquivo: um arquivo cheio de usos não esconde os outros.
const MAX_PER_FILE: usize = 6;
// Arquivos que sobraram, listados só pelo nome.
const MAX_OTHER_FILES: usize = 12;
// Arquivos maiores que isso (provavelmente gerados) são ignorados.
const MAX_FILE_BYTES: u64 = 1_000_000;
// Tamanho máximo de cada linha devolvida, em caracteres.
const MAX_LINE_CHARS: usize = 160;

pub struct GrepFiles;

impl Tool for GrepFiles {
    fn name(&self) -> &'static str {
        "grep_files"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "grep_files",
            "Procura texto DENTRO dos arquivos, em todas as subpastas. Sem regex; ignora maiúsculas e acentos. \
             Separe alternativas com | (ex.: \"yellow|amarelo|color\"): o código costuma usar nomes em inglês. \
             Devolve arquivo e linha: trecho, com definições primeiro. Use antes de read_file para achar o trecho certo.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Texto a procurar; alternativas separadas por |"},
                    "path": {"type": "string", "description": "Pasta ou arquivo; padrão \".\" (o projeto todo)"},
                    "ext": {"type": "string", "description": "Só arquivos com esta extensão, ex.: \"rs\""}
                },
                "required": ["pattern"]
            }),
        )
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: true }
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(grep(ctx.root, args).map(Outcome::Done))
    }
}

// As ocorrências de um arquivo.
struct FileHits {
    path: String,
    // (linha, trecho, é definição)
    lines: Vec<(usize, String, bool)>,
}

impl FileHits {
    fn defines(&self) -> bool {
        self.lines.iter().any(|l| l.2)
    }
}

fn grep(root: &Path, args: &Value) -> Result<String, String> {
    let pattern = str_arg(args, "pattern")?;
    let terms = query::terms(pattern);
    if terms.is_empty() {
        return Err("'pattern' vazio".into());
    }
    let start = resolve(root, args.get("path").and_then(Value::as_str).unwrap_or("."))?;
    let ext = args.get("ext").and_then(Value::as_str).map(|e| e.trim().trim_start_matches('*').trim_start_matches('.').to_lowercase());
    let ext = ext.filter(|e| !e.is_empty());

    let walked = walk(root, &start, false);
    let files: Vec<PathBuf> = walked
        .entries
        .into_iter()
        .filter(|f| ext.as_ref().is_none_or(|e| f.extension().is_some_and(|x| x.to_string_lossy().to_lowercase() == *e)))
        .collect();
    let capped = if walked.capped { "\n[o projeto é grande: parei de procurar no meio; restrinja com path]" } else { "" };

    let hits = search(root, &files, &terms);
    if !hits.is_empty() {
        return Ok(format!("{}{capped}", render(&hits)));
    }
    let wider = broaden(&terms);
    if !wider.is_empty() {
        let hits = search(root, &files, &wider);
        if !hits.is_empty() {
            return Ok(format!(
                "nada encontrado para \"{pattern}\"; busca ampliada com \"{}\":\n{}{capped}",
                wider.join("|"),
                render(&hits)
            ));
        }
    }

    let place = relative(root, &start);
    let place = if place.is_empty() { "no projeto".to_string() } else { format!("em {place}") };
    let only = ext.map(|e| format!(" .{e}")).unwrap_or_default();
    let mut msg = format!("nada encontrado para \"{pattern}\" {place} ({} arquivos{only} lidos).", files.len());
    if !wider.is_empty() {
        msg.push_str(&format!(" Também tentei \"{}\".", wider.join("|")));
    }
    msg.push_str(" Não conclua que não existe: tente sinônimos em inglês como aparecem no código, parte do nome, \
                  ou find_files para procurar pelo nome do arquivo.");
    if walked.skipped > 0 {
        msg.push_str(&format!(
            " {} itens ocultos ou ignorados (.gitignore, target) ficaram de fora; para procurar num deles, passe-o em path.",
            walked.skipped
        ));
    }
    Ok(msg)
}

fn search(root: &Path, files: &[PathBuf], terms: &[String]) -> Vec<FileHits> {
    let mut hits = Vec::new();
    for file in files {
        let Some(text) = read_text(file) else { continue };
        let mut lines = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            let normal = normalize(line);
            let Some(at) = find_any(&normal, terms) else { continue };
            lines.push((i + 1, snippet(line, at), is_definition(line)));
        }
        if !lines.is_empty() {
            hits.push(FileHits { path: relative(root, file), lines });
        }
    }
    // Ordem estável: quem define vem antes de quem só usa.
    hits.sort_by_key(|h| !h.defines());
    hits
}

// Texto do arquivo, ou nada se for grande demais, binário ou não for UTF-8.
fn read_text(file: &Path) -> Option<String> {
    if file.metadata().ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(file).ok()?;
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn render(hits: &[FileHits]) -> String {
    let total: usize = hits.iter().map(|h| h.lines.len()).sum();
    let mut out = vec![format!("{total} ocorrências em {} arquivos:", hits.len())];
    let mut shown = 0;
    let mut others: Vec<String> = Vec::new();
    for file in hits {
        if shown >= MAX_MATCHES {
            others.push(format!("{} ({})", file.path, file.lines.len()));
            continue;
        }
        out.push(file.path.clone());
        let room = MAX_PER_FILE.min(MAX_MATCHES - shown);
        for (n, text, def) in pick(&file.lines, room) {
            let mark = if def { "  ← definição" } else { "" };
            out.push(format!("  {n}: {text}{mark}"));
            shown += 1;
        }
        if file.lines.len() > room {
            out.push(format!("  [+{} neste arquivo]", file.lines.len() - room));
        }
    }
    if !others.is_empty() {
        let extra = others.len().saturating_sub(MAX_OTHER_FILES);
        others.truncate(MAX_OTHER_FILES);
        let more = if extra > 0 { format!(" e mais {extra}") } else { String::new() };
        out.push(format!("[também em: {}{more}; refine com mais termos, ext ou path]", others.join(", ")));
    }
    out.join("\n")
}

// Até `room` linhas do arquivo, definições primeiro, devolvidas na ordem do arquivo.
fn pick(lines: &[(usize, String, bool)], room: usize) -> Vec<(usize, String, bool)> {
    let mut chosen: Vec<&(usize, String, bool)> = lines.iter().filter(|l| l.2).take(room).collect();
    chosen.extend(lines.iter().filter(|l| !l.2).take(room - chosen.len()));
    chosen.sort_by_key(|l| l.0);
    chosen.into_iter().cloned().collect()
}

// A linha (já sem espaços nas pontas) cortada em volta da ocorrência, na posição `at` em caracteres.
fn snippet(line: &str, at: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let start = at.saturating_sub(40).min(chars.len() - MAX_LINE_CHARS);
    let end = start + MAX_LINE_CHARS;
    let body: String = chars[start..end].iter().collect();
    let head = if start > 0 { "…" } else { "" };
    let tail = if end < chars.len() { "…" } else { "" };
    format!("{head}{}{tail}", body.trim())
}

// Linha que declara algo (Rust, Python, JS/TS): é onde uma mudança costuma ser feita.
fn is_definition(line: &str) -> bool {
    let mut rest = line;
    for prefix in ["pub(crate) ", "pub(super) ", "pub(self) ", "pub ", "export default ", "export ", "async ", "unsafe "] {
        if let Some(r) = rest.strip_prefix(prefix) {
            rest = r;
        }
    }
    if let Some(r) = rest.strip_prefix("pub(in ") {
        rest = r.split_once(") ").map_or(r, |(_, r)| r);
    }
    const KEYWORDS: &[&str] = &[
        "fn ", "struct ", "enum ", "const ", "static ", "type ", "trait ", "impl ", "impl<", "mod ", "macro_rules!",
        "def ", "class ", "function ", "interface ",
    ];
    KEYWORDS.iter().any(|k| rest.starts_with(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_recognized_with_visibility() {
        assert!(is_definition("pub(super) const YELLOW: Color = Color::Rgb(255, 204, 0);"));
        assert!(is_definition("pub(in crate::app::tui) fn effort_color(e: Effort) -> Color {"));
        assert!(is_definition("export function render() {"));
        assert!(!is_definition("let c = YELLOW;"));
        assert!(!is_definition("Span::styled(\"x\", fg(YELLOW)),"));
    }

    #[test]
    fn long_lines_are_cut_around_the_match() {
        let line = format!("{}alvo{}", "x".repeat(300), "y".repeat(300));
        let cut = snippet(&line, 300);
        assert!(cut.contains("alvo") && cut.starts_with('…') && cut.ends_with('…'), "{cut}");
        assert!(cut.chars().count() <= MAX_LINE_CHARS + 2);
    }
}

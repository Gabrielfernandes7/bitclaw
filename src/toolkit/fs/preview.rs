//! Prévia de uma edição: o trecho trocado com algumas linhas de contexto.

use crate::core::tool::{Preview, PreviewLine};

const CONTEXT_LINES: usize = 2;

// Monta a prévia de uma edição com algumas linhas de contexto do arquivo resultante,
// para o usuário ver onde o trecho cai, não só o que foi removido e adicionado.
pub(super) fn edit_preview(rel: &str, updated: &str, start_line: usize, old: &str, new: &str) -> Preview {
    let lines: Vec<&str> = updated.lines().collect();
    let len = lines.len();
    let before_end = start_line.min(len);
    let before_begin = before_end.saturating_sub(CONTEXT_LINES);
    let after_begin = (start_line + new.lines().count()).min(len);
    let after_end = (after_begin + CONTEXT_LINES).min(len);

    let mut out = Vec::new();
    out.extend(lines[before_begin..before_end].iter().map(|l| PreviewLine::Context(l.to_string())));
    out.extend(old.lines().map(|l| PreviewLine::Removed(l.to_string())));
    out.extend(new.lines().map(|l| PreviewLine::Added(l.to_string())));
    out.extend(lines[after_begin..after_end].iter().map(|l| PreviewLine::Context(l.to_string())));
    Preview::new(format!("edição proposta em {rel}"), out)
}

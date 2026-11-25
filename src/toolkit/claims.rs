//! Verificação de alegações: o texto final diz que criou, alterou ou removeu algo que as ações não confirmam.

use crate::core::agent::{TurnHook, TurnReport};
use crate::core::tool::Outcome;

// Palavras que indicam que o texto final afirma uma alteração. Heurística: gera aviso, não bloqueia.
// Só formas que afirmam algo já feito. Infinitivos ("posso criar", "quer remover") são ofertas.
const CHANGE_WORDS: [&str; 10] = ["criei", "criou", "criado", "criada", "alterei", "alterou", "editei", "editou", "alterado", "editado"];
const DELETE_WORDS: [&str; 8] = ["removi", "removeu", "removido", "removida", "apaguei", "apagou", "excluí", "excluiu"];

// Algum arquivo foi removido de fato neste turno.
pub fn deleted(report: &TurnReport) -> bool {
    report.actions.iter().any(|a| a.name == "delete_file" && matches!(a.outcome, Outcome::Done(_)))
}

// Avisos sobre o texto final: ele afirma algo que as ações não confirmam.
pub fn warnings(report: &TurnReport) -> Vec<String> {
    let text = report.text.to_lowercase();
    let claims_delete = DELETE_WORDS.iter().any(|w| text.contains(w));
    let claims_change = claims_delete || CHANGE_WORDS.iter().any(|w| text.contains(w));
    let mut out = Vec::new();
    if !report.changed() && (report.attempted_changes() || claims_change) {
        out.push("nenhuma alteração foi aplicada neste turno.".to_string());
    }
    if claims_delete && !deleted(report) {
        out.push("o texto diz que algo foi removido, mas nenhum arquivo foi removido.".to_string());
    }
    out
}

// O gancho que liga `warnings` ao fim de cada turno da sessão.
pub struct FileClaims;

impl TurnHook for FileClaims {
    fn after_turn(&self, report: &TurnReport) -> Vec<String> {
        warnings(report)
    }
}

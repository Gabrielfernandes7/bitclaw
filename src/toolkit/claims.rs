//! Verificação de alegações: o texto final diz que criou, alterou ou removeu algo que as ações não confirmam.

use crate::core::agent::{TurnHook, TurnReport};
use crate::core::tool::Outcome;

// Palavras que indicam que o texto final afirma uma alteração. Heurística: gera aviso, não bloqueia.
// Só formas que afirmam algo já feito. Infinitivos ("posso criar", "quer remover") são ofertas.
const CHANGE_WORDS: [&str; 10] = ["criei", "criou", "criado", "criada", "alterei", "alterou", "editei", "editou", "alterado", "editado"];
const DELETE_WORDS: [&str; 8] = ["removi", "removeu", "removido", "removida", "apaguei", "apagou", "excluí", "excluiu"];

// Palavras que afirmam que um script rodou. Heurística, como as de cima.
const RUN_WORDS: [&str; 6] = ["executado", "executei", "executou", "rodei", "rodou", "a saída é"];

// Marcadores de "saída de ferramenta" que alguns modelos escrevem no próprio texto em vez de chamar a ferramenta.
const FAKE_OUTPUT: [&str; 4] = ["<｜tool", "<|tool", "<tool_response", "<tool_output"];

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
    // O texto diz que executou algo, mas nenhuma execução deu certo: a saída citada não é real.
    let ran = report.actions.iter().any(|a| a.name == "run_script" && matches!(a.outcome, Outcome::Done(_)));
    if !ran && RUN_WORDS.iter().any(|w| text.contains(w)) {
        out.push("o texto diz que um script foi executado, mas nenhuma execução deu certo: a saída citada não é real.".to_string());
    }
    // O modelo escreveu uma saída de ferramenta no texto, mas nenhuma ferramenta rodou: a saída é inventada.
    if report.actions.is_empty() && FAKE_OUTPUT.iter().any(|m| text.contains(m)) {
        out.push("o texto traz uma saída de ferramenta escrita pelo modelo, mas nenhuma ferramenta rodou: ela não é real.".to_string());
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

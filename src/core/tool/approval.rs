//! Aprovação de alterações: a prévia mostrada ao usuário e o `Approver` que decide.

// Uma linha de prévia de uma alteração proposta.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewLine {
    Context(String),
    Removed(String),
    Added(String),
    Note(String),
}

// Prévia de uma alteração: título e linhas. O core não a imprime; quem mostra é o `Approver`.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub title: String,
    pub lines: Vec<PreviewLine>,
}

// Decide se uma alteração proposta pode ser aplicada. Recebe a prévia para mostrar ao usuário.
// Injetado para que o core não leia o stdin nem imprima nada.
pub trait Approver {
    fn approve(&self, preview: &Preview) -> bool;

    // Decide se um script pode ser executado. Por padrão pergunta como qualquer alteração; quem
    // aprova tudo sem olhar (`--yes`) deve sobrescrever isto: código nunca roda sem alguém ver.
    fn approve_run(&self, preview: &Preview) -> bool {
        self.approve(preview)
    }
}

// Aprova todas as alterações (uso com testes). Não aprova execuções de script.
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn approve(&self, _preview: &Preview) -> bool {
        true
    }

    fn approve_run(&self, _preview: &Preview) -> bool {
        false
    }
}

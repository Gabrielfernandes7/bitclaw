//! Aprovação de alterações: a prévia mostrada ao usuário e o `Approver` que decide.

// Uma linha de prévia de uma alteração proposta.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewLine {
    Context(String),
    Removed(String),
    Added(String),
    Note(String),
}

// O tipo de alteração que a prévia mostra. Uma remoção nunca entra em aprovação em lote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Change,
    Removal,
}

// Prévia de uma alteração: título, linhas e tipo. O core não a imprime; quem mostra é o `Approver`.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub title: String,
    pub lines: Vec<PreviewLine>,
    pub kind: PreviewKind,
}

impl Preview {
    pub fn new(title: impl Into<String>, lines: Vec<PreviewLine>) -> Self {
        Self { title: title.into(), lines, kind: PreviewKind::Change }
    }

    pub fn removal(title: impl Into<String>, lines: Vec<PreviewLine>) -> Self {
        Self { title: title.into(), lines, kind: PreviewKind::Removal }
    }

    pub fn is_removal(&self) -> bool {
        self.kind == PreviewKind::Removal
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_removals_are_removals_by_kind_not_by_title() {
        assert!(Preview::removal("apagar a.txt", Vec::new()).is_removal());
        assert!(!Preview::new("remover arquivo a.txt", Vec::new()).is_removal(), "o título não decide");
        assert!(!Preview::new("edição proposta em a.txt", Vec::new()).is_removal());
    }
}

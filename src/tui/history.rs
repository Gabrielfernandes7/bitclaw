// Histórico das tarefas enviadas, navegado com ↑ e ↓ como no shell. Sem terminal: só texto.
#[derive(Default)]
pub(super) struct History {
    pub(super) items: Vec<String>,
    // Posição atual ao navegar; `None` quando o editor mostra o próprio texto.
    pos: Option<usize>,
    // O que estava digitado antes de começar a navegar, para voltar a ele ao descer até o fim.
    draft: String,
}

impl History {
    pub(super) fn push(&mut self, text: &str) {
        if !text.is_empty() && self.items.last().map(String::as_str) != Some(text) {
            self.items.push(text.to_string());
        }
        self.pos = None;
    }

    // ↑: volta uma entrada. `current` é o texto do editor, guardado se for a primeira volta.
    pub(super) fn up(&mut self, current: &str) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let next = match self.pos {
            None => {
                self.draft = current.to_string();
                self.items.len() - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.pos = Some(next);
        Some(self.items[next].clone())
    }

    // ↓: avança uma entrada; depois da última, volta ao texto que estava sendo digitado.
    pub(super) fn down(&mut self) -> Option<String> {
        let p = self.pos?;
        if p + 1 < self.items.len() {
            self.pos = Some(p + 1);
            Some(self.items[p + 1].clone())
        } else {
            self.pos = None;
            Some(std::mem::take(&mut self.draft))
        }
    }

    // Editar um item recuperado o torna o texto atual, sem voltar a navegar a partir dele.
    pub(super) fn edited(&mut self) {
        self.pos = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_walks_back_and_forward_and_restores_the_draft() {
        let mut h = History::default();
        assert_eq!(h.up("rascunho"), None, "sem histórico, nada muda");
        h.push("primeira");
        h.push("segunda");
        h.push("segunda");
        assert_eq!(h.items.len(), 2, "repetida em seguida não entra de novo");
        assert_eq!(h.up("digitando"), Some("segunda".into()));
        assert_eq!(h.up("segunda"), Some("primeira".into()));
        assert_eq!(h.up("primeira"), Some("primeira".into()), "para no começo");
        assert_eq!(h.down(), Some("segunda".into()));
        assert_eq!(h.down(), Some("digitando".into()), "volta ao rascunho");
        assert_eq!(h.down(), None, "fora do histórico, nada muda");
    }

    #[test]
    fn editing_a_recalled_item_starts_a_new_walk_from_the_end() {
        let mut h = History::default();
        h.push("a");
        h.push("b");
        assert_eq!(h.up(""), Some("b".into()));
        h.edited();
        assert_eq!(h.up("b mudado"), Some("b".into()));
        assert_eq!(h.down(), Some("b mudado".into()), "o texto editado vira o rascunho");
    }
}

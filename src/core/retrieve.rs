//! Recuperação antes do turno: quem sabe o que é "código" (ex.: `toolkit::fs::SymbolIndex`) sugere
//! trechos prováveis para a tarefa, e a sessão os anexa à mensagem. O core não sabe de arquivos.

use std::path::Path;

pub trait Retriever {
    // Texto curto com o que provavelmente importa para `task`, ou `None` se nada se destaca.
    fn retrieve(&self, root: &Path, task: &str) -> Option<String>;
}

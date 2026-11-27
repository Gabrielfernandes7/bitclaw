//! Verificação automática depois de uma alteração: quem sabe o que é "compilar" ou "validar"
//! (ex.: `toolkit::verify`) devolve o problema, e o laço o entrega ao modelo para corrigir.

use super::report::Action;
use std::path::Path;

pub trait Verifier {
    // Roda a checagem sobre as ações do turno. `Some(texto)` é um problema curto e acionável;
    // `None` quer dizer que está tudo certo ou que não há o que checar.
    fn verify(&self, root: &Path, actions: &[Action]) -> Option<String>;
}

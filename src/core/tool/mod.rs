//! Contrato das ferramentas do agente: `Tool`, `Outcome`, o `Registry` e a aprovação das alterações.

mod approval;
mod registry;

pub use approval::{Approver, AutoApprover, Preview, PreviewLine};
pub use registry::Registry;

use serde_json::Value;
use std::path::Path;

// Limite do resultado de `read_file`. Com num_ctx de 8192 tokens, 100 KB estouraria o contexto de um modelo de 4B.
pub const MAX_READ_BYTES: usize = 8_000;

// O que realmente aconteceu com uma chamada de ferramenta.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    // A ferramenta executou e o resultado foi aplicado (ou lido).
    Done(String),
    // O usuário recusou a alteração na confirmação.
    Refused(String),
    // A ferramenta falhou; nada foi alterado.
    Error(String),
}

impl Outcome {
    // Texto devolvido ao modelo.
    pub fn text(&self) -> String {
        match self {
            Outcome::Done(s) | Outcome::Refused(s) => s.clone(),
            Outcome::Error(e) => format!("ERRO: {e}"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Done(_) => "ok",
            Outcome::Refused(_) => "recusada",
            Outcome::Error(_) => "erro",
        }
    }
}


// Contrato de uma ferramenta: nome, esquema e execução juntos.
// Adicionar uma ferramenta é implementar esta trait e registrá-la; o loop do agente não muda.
pub trait Tool {
    fn name(&self) -> &'static str;

    // Entrada completa no formato `{"type": "function", "function": {...}}` que o Ollama espera.
    fn schema(&self) -> Value;

    // Se a ferramenta altera arquivos. Usado pelo relatório do turno e pela proteção contra repetição.
    fn mutates(&self) -> bool {
        false
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome;
}


pub(crate) fn wrap(result: Result<Outcome, String>) -> Outcome {
    result.unwrap_or_else(Outcome::Error)
}

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("argumento '{key}' ausente ou inválido"))
}

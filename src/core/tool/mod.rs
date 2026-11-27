//! Contrato das ferramentas do agente: `Tool`, `Outcome`, o `Registry` e a aprovação das alterações.

mod approval;
mod args;
mod registry;

pub use approval::{Approver, AutoApprover, Preview, PreviewKind, PreviewLine};
pub use registry::Registry;

use serde_json::{json, Value};
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


// Descrição de uma ferramenta, neutra de provedor: nome, texto para o modelo e parâmetros em JSON Schema.
// Cada adaptador a embrulha no formato da sua API.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    pub fn new(name: &str, description: &str, parameters: Value) -> Self {
        Self { name: name.into(), description: description.into(), parameters }
    }
}

// Tamanho aproximado das descrições, em JSON, para o orçamento de contexto.
pub fn specs_value(specs: &[ToolSpec]) -> Value {
    Value::Array(
        specs
            .iter()
            .map(|s| json!({"name": s.name, "description": s.description, "parameters": s.parameters}))
            .collect(),
    )
}

// O que uma ferramenta recebe ao rodar: a pasta do projeto e quem aprova alterações.
// Uma ferramenta que não usa arquivos (ex.: consulta a um banco) simplesmente ignora `root`.
pub struct ToolContext<'a> {
    pub root: &'a Path,
    pub approver: &'a dyn Approver,
}

impl<'a> ToolContext<'a> {
    pub fn new(root: &'a Path, approver: &'a dyn Approver) -> Self {
        Self { root, approver }
    }
}

// O que a ferramenta declara sobre si. O agente, o modo Plan e a aprovação decidem por isto, e não
// pelo nome da ferramenta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Annotations {
    // Não altera nada, nem no projeto nem fora dele: pode rodar no modo Plan, sem aprovação.
    pub read_only: bool,
    // Remove ou sobrescreve conteúdo que já existia (com cópia em `.bitclaw/backup`, quando há).
    pub destructive: bool,
    // Chamar de novo com os mesmos argumentos, sem alteração no meio, devolve o mesmo resultado e não
    // tem efeito colateral (ex.: ler e listar). O agente não repete essas chamadas no turno.
    pub idempotent: bool,
}

impl Default for Annotations {
    // Sem declaração, a ferramenta é tratada como alteradora e destrutiva: o lado seguro.
    fn default() -> Self {
        Self { read_only: false, destructive: true, idempotent: false }
    }
}

// Contrato de uma ferramenta: nome, descrição e execução juntos.
// Adicionar uma ferramenta é implementar esta trait e registrá-la; o loop do agente não muda.
pub trait Tool {
    fn name(&self) -> &'static str;

    fn spec(&self) -> ToolSpec;

    fn annotations(&self) -> Annotations {
        Annotations::default()
    }

    // As anotações desta chamada. Quase toda ferramenta tem uma só, igual para todas as chamadas.
    // Uma ferramenta que decide pelos argumentos (um comando de leitura ou de alteração) sobrescreve
    // isto e `per_call_annotations`; sem declarar, vale `annotations()`.
    fn call_annotations(&self, _args: &Value) -> Annotations {
        self.annotations()
    }

    // Verdadeiro se `call_annotations` muda de chamada para chamada. No modo Plan, a ferramenta
    // continua visível e cada chamada é julgada pelos seus argumentos.
    fn per_call_annotations(&self) -> bool {
        false
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome;
}

pub(crate) fn wrap(result: Result<Outcome, String>) -> Outcome {
    result.unwrap_or_else(Outcome::Error)
}

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("argumento '{key}' ausente ou inválido"))
}

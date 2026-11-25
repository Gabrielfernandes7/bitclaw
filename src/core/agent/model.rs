//! Contrato com o provedor de modelo: `ChatModel`, `Reply` e `Chunk`.

use serde_json::Value;

// Resposta de um provedor de modelo.
pub struct Reply {
    pub message: Value,
    // Tokens de entrada avaliados pelo provedor, quando informados. Usado para calibrar o contexto.
    pub prompt_tokens: Option<u64>,
    // Tokens gerados pelo modelo, quando informados.
    pub output_tokens: Option<u64>,
}

// Um pedaço de texto que o modelo acabou de gerar: raciocínio ou resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    Thinking(&'a str),
    Content(&'a str),
    // Nada chegou ainda: o provedor só acorda para o sink conferir se o turno foi cancelado. É o que
    // deixa o Esc valer enquanto o modelo carrega ou processa o prompt, antes do primeiro texto.
    Idle,
}

// Qualquer provedor capaz de completar uma conversa com ferramentas.
pub trait ChatModel {
    fn complete(&self, messages: &[Value], tools: &Value) -> Result<Reply, String>;

    // Igual a `complete`, mas entrega o texto à medida que o modelo o gera. Se `on_chunk` devolve
    // `false`, o provedor para de gerar e devolve o que já tem. Quem não faz streaming usa este padrão.
    fn complete_streaming(
        &self,
        messages: &[Value],
        tools: &Value,
        _on_chunk: &mut dyn FnMut(Chunk) -> bool,
    ) -> Result<Reply, String> {
        self.complete(messages, tools)
    }
}

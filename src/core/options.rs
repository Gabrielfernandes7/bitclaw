//! Opções de geração que a interface pode mudar com a sessão aberta: janela e temperatura.
//! Neutras de provedor: cada adaptador usa o que a sua API aceita.

use std::sync::{Arc, Mutex};

// Janela e temperatura. Vêm da configuração do agente e mudam com `/context` e `/temp`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub num_ctx: u32,
    pub temperature: f32,
}

// Compartilhadas entre a interface e o cliente do modelo: o cliente as lê a cada chamada.
pub type SharedOptions = Arc<Mutex<Options>>;

impl Default for Options {
    fn default() -> Self {
        Self { num_ctx: 16384, temperature: 0.6 }
    }
}

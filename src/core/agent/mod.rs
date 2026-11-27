//! Ciclo modelo → ferramentas → modelo. Não conhece o Ollama nem nomes de ferramentas:
//! recebe um `ChatModel`, um `Registry`, um `ContextManager` e um `Progress`.

mod guard;
mod hook;
mod model;
mod report;
mod text_calls;
mod turn;
mod verify;

pub use hook::TurnHook;
pub use verify::Verifier;
pub use guard::{LOOPING, is_looping};
pub use model::{ChatModel, Chunk, Reply};
pub use report::{Action, Progress, Silent, TurnReport};
pub use turn::{INTERRUPTED, Limits, MAX_CONSECUTIVE_FAILURES, MAX_STEPS, run_turn, run_turn_with};

#[cfg(test)]
mod tests;

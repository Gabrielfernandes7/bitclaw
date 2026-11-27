//! Adaptador do OpenRouter: traduz o core para a API `/chat/completions` (formato OpenAI).
//! Tudo que é específico do OpenRouter fica aqui; o core não sabe que ele existe.
//!
//! A chave de API vem de uma variável de ambiente, nunca do TOML nem de uma mensagem de erro.

mod account;
mod auth;
mod catalog;
mod client;
mod messages;
mod settings;

pub use account::{Account, check_key, parse_account};
pub use auth::ApiKey;
pub use catalog::{OpenRouterCatalog, model_from_entry};
pub use client::{Client, build_body, fold_sse};
pub use settings::{DEFAULT_BASE_URL, DEFAULT_KEY_ENV, OpenRouterSettings};

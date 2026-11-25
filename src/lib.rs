//! BitClaw como biblioteca: o core de agente.
//!
//! Para construir um agente sobre o core, escreva uma configuração (ver `agents/bitclaw.toml`),
//! escolha um `ChatModel` e um `Approver`, e use `Session`:
//!
//! ```no_run
//! use bitclaw::{config::AgentConfig, session::Session, tools::AutoApprover, agent::Silent};
//! # fn exemplo(model: Box<dyn bitclaw::agent::ChatModel>) -> Result<(), String> {
//! let cfg = AgentConfig::from_toml(include_str!("../agents/bitclaw.toml"))?;
//! let mut sessao = Session::from_config(&cfg, model, Box::new(AutoApprover), std::path::Path::new("."))?;
//! let relatorio = sessao.submit("liste os arquivos", &Silent)?;
//! println!("{}", relatorio.text);
//! # Ok(()) }
//! ```

pub mod agent;
pub mod config;
pub mod context;
pub mod init;
pub mod models;
pub mod ollama;
pub mod sandbox;
pub mod session;
pub mod skills;
pub mod tools;

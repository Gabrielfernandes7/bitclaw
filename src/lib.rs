//! BitClaw como biblioteca: o core de agente.
//!
//! Para construir um agente sobre o core, escreva uma configuração (ver `agents/bitclaw.toml`),
//! escolha um `ChatModel` e um `Approver`, e use `Session`:
//!
//! ```no_run
//! use bitclaw::{agent::Silent, config::AgentConfig, session::Session, toolkit, tools::AutoApprover};
//! # fn exemplo(model: Box<dyn bitclaw::agent::ChatModel>) -> Result<(), String> {
//! let cfg = AgentConfig::from_toml(include_str!("../agents/bitclaw.toml"))?;
//! let tools = toolkit::registry_from_names(&cfg.tools)?; // some aqui as ferramentas do seu agente
//! let mut sessao = Session::builder(&cfg)
//!     .model(model)
//!     .approver(Box::new(AutoApprover))
//!     .tools(tools)
//!     .root(".")
//!     .build()?;
//! let relatorio = sessao.submit("liste os arquivos", &Silent)?;
//! println!("{}", relatorio.text);
//! # Ok(()) }
//! ```

pub mod adapters;
pub mod core;
pub mod init;
pub mod toolkit;

// Caminhos antigos, mantidos durante a refatoração.
pub use adapters::ollama;
pub use core::{agent, config, context, models, sandbox, session};
pub use core::skill as skills;
pub use core::tool as tools;
pub use toolkit::script;

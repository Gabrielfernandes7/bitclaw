//! Core do agente: o ciclo modelo → ferramentas → modelo, sem rede, sem terminal e sem provedor.
//!
//! O core não conhece ninguém: quem o conhece são os adaptadores, o toolkit e o app.

pub mod agent;
pub mod config;
pub mod context;
pub mod models;
pub mod options;
pub mod retrieve;
pub mod sandbox;
pub mod session;
pub mod skill;
pub mod tool;

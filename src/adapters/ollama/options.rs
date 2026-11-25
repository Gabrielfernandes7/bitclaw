//! Opções do Ollama: janela, temperatura e a seção `[ollama]` da configuração.

use crate::core::config::AgentConfig;
use serde::Deserialize;
pub use crate::core::options::{Options, SharedOptions};

pub const DEFAULT_URL: &str = "http://localhost:11434";

// Janela de contexto e temperatura padrão. Sem `num_ctx` explícito, o Ollama usa uma janela pequena.
pub const NUM_CTX: u32 = 16384; // 16384 ou 8192
pub const TEMPERATURE: f32 = 0.6;

// A seção `[ollama]` do TOML do agente.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct OllamaSettings {
    pub url: String,
    pub temperature: f32,
}

impl Default for OllamaSettings {
    fn default() -> Self {
        Self { url: DEFAULT_URL.into(), temperature: TEMPERATURE }
    }
}

impl OllamaSettings {
    // Lê `[ollama]`. Sem a seção, usa o formato antigo (`model.url` e `model.temperature`) e depois os padrões.
    pub fn from_config(cfg: &AgentConfig) -> Result<Self, String> {
        let mut settings = Self::default();
        if let Some(url) = &cfg.model.url {
            settings.url = url.clone();
        }
        if let Some(temperature) = cfg.model.temperature {
            settings.temperature = temperature;
        }
        if let Some(section) = cfg.provider_section("ollama") {
            let table = toml::Value::Table(section.clone());
            let own: PartialOllama = table.try_into().map_err(|e| format!("configuração: [ollama]: {e}"))?;
            if let Some(url) = own.url {
                settings.url = url;
            }
            if let Some(temperature) = own.temperature {
                settings.temperature = temperature;
            }
        }
        Ok(settings)
    }

    // As opções de geração iniciais: a janela vem do modelo; a temperatura, daqui.
    pub fn options(&self, context_window: u32) -> Options {
        Options { num_ctx: context_window, temperature: self.temperature }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialOllama {
    url: Option<String>,
    temperature: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(extra: &str) -> AgentConfig {
        AgentConfig::from_toml(&format!(
            "name = \"t\"\nsystem_prompt = \"oi\"\ntools = [\"read_file\"]\n[model]\nname = \"m\"\n{extra}"
        ))
        .unwrap()
    }

    #[test]
    fn defaults_apply_without_a_section() {
        assert_eq!(OllamaSettings::from_config(&cfg("")).unwrap(), OllamaSettings::default());
    }

    #[test]
    fn section_overrides_defaults() {
        let c = cfg("[ollama]\nurl = \"http://x:1\"\ntemperature = 0.3\n");
        let s = OllamaSettings::from_config(&c).unwrap();
        assert_eq!((s.url.as_str(), s.temperature), ("http://x:1", 0.3));
    }

    #[test]
    fn old_model_fields_still_work_and_the_section_wins() {
        let old = cfg("url = \"http://old:1\"\ntemperature = 0.9\n");
        let s = OllamaSettings::from_config(&old).unwrap();
        assert_eq!((s.url.as_str(), s.temperature), ("http://old:1", 0.9));
        let both = cfg("url = \"http://old:1\"\n[ollama]\nurl = \"http://new:1\"\n");
        assert_eq!(OllamaSettings::from_config(&both).unwrap().url, "http://new:1");
    }

    #[test]
    fn unknown_field_in_the_section_is_an_error() {
        let err = OllamaSettings::from_config(&cfg("[ollama]\nurll = \"x\"\n")).unwrap_err();
        assert!(err.contains("urll"), "{err}");
    }
}

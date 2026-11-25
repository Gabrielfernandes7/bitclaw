//! A seção `[openrouter]` da configuração.

use crate::core::config::AgentConfig;
use crate::core::options::Options;
use serde::Deserialize;

pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_KEY_ENV: &str = "OPENROUTER_API_KEY";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct OpenRouterSettings {
    pub base_url: String,
    // O NOME da variável de ambiente com a chave. O TOML é versionado: a chave não entra nele.
    pub api_key_env: String,
    pub temperature: f32,
}

impl Default for OpenRouterSettings {
    fn default() -> Self {
        Self { base_url: DEFAULT_BASE_URL.into(), api_key_env: DEFAULT_KEY_ENV.into(), temperature: Options::default().temperature }
    }
}

impl OpenRouterSettings {
    pub fn from_config(cfg: &AgentConfig) -> Result<Self, String> {
        let Some(section) = cfg.provider_section("openrouter") else {
            return Ok(Self::default());
        };
        let settings: Self = toml::Value::Table(section.clone())
            .try_into()
            .map_err(|e| format!("configuração: [openrouter]: {e}"))?;
        settings.validate()?;
        Ok(settings)
    }

    // Quem cola a chave em `api_key_env` por engano não a vê repetida no erro.
    fn validate(&self) -> Result<(), String> {
        let is_name = !self.api_key_env.is_empty()
            && self.api_key_env.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !self.api_key_env.starts_with("sk-");
        if !is_name {
            return Err("configuração: [openrouter] api_key_env deve ser o NOME de uma variável de ambiente (ex.: OPENROUTER_API_KEY), nunca a chave".into());
        }
        Ok(())
    }

    pub fn options(&self, context_window: u32) -> Options {
        Options { num_ctx: context_window, temperature: self.temperature }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(extra: &str) -> AgentConfig {
        AgentConfig::from_toml(&format!(
            "name = \"t\"\nsystem_prompt = \"oi\"\ntools = []\n[model]\nname = \"m\"\nprovider = \"openrouter\"\n{extra}"
        ))
        .unwrap()
    }

    #[test]
    fn defaults_point_at_the_public_api_and_the_standard_variable() {
        let s = OpenRouterSettings::from_config(&cfg("")).unwrap();
        assert_eq!((s.base_url.as_str(), s.api_key_env.as_str()), (DEFAULT_BASE_URL, "OPENROUTER_API_KEY"));
    }

    #[test]
    fn the_section_overrides_defaults() {
        let s = OpenRouterSettings::from_config(&cfg("[openrouter]\nbase_url = \"http://x/v1\"\napi_key_env = \"MINHA_CHAVE\"\ntemperature = 0.1\n")).unwrap();
        assert_eq!((s.base_url.as_str(), s.api_key_env.as_str(), s.temperature), ("http://x/v1", "MINHA_CHAVE", 0.1));
    }

    #[test]
    fn a_pasted_key_is_rejected_without_being_echoed() {
        let err = OpenRouterSettings::from_config(&cfg("[openrouter]\napi_key_env = \"sk-or-v1-abc123\"\n")).unwrap_err();
        assert!(err.contains("NOME"), "{err}");
        assert!(!err.contains("abc123"), "o erro não pode repetir a chave: {err}");
    }

    #[test]
    fn unknown_field_is_an_error() {
        assert!(OpenRouterSettings::from_config(&cfg("[openrouter]\napi_key = \"x\"\n")).is_err());
    }
}

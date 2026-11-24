//! Configuração de um agente, lida de TOML. Prompt, ferramentas, modelo e limites são dados,
//! não código: trocar um deles não exige recompilar o core.

use crate::agent::{MAX_CONSECUTIVE_FAILURES, MAX_STEPS};
use crate::ollama::{NUM_CTX, TEMPERATURE};
use crate::tools::Registry;
use serde::Deserialize;

// `deny_unknown_fields`: um campo escrito errado é erro, não um padrão silencioso.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub name: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub model: ModelConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub name: String,
    pub url: String,
    #[serde(default = "default_num_ctx")]
    pub num_ctx: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsConfig {
    pub max_steps: usize,
    pub max_consecutive_failures: usize,
    // Tokens reservados para a resposta do modelo, fora do orçamento do histórico.
    pub reserve_output_tokens: usize,
}

fn default_num_ctx() -> u32 {
    NUM_CTX
}

fn default_temperature() -> f32 {
    TEMPERATURE
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_steps: MAX_STEPS,
            max_consecutive_failures: MAX_CONSECUTIVE_FAILURES,
            reserve_output_tokens: 1024,
        }
    }
}

impl AgentConfig {
    // Lê e valida uma configuração em TOML.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let cfg: Self = toml::from_str(text).map_err(|e| format!("configuração inválida: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    // Verifica o que o TOML não consegue expressar: valores coerentes entre si e ferramentas conhecidas.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("configuração: 'name' não pode ser vazio".into());
        }
        if self.system_prompt.trim().is_empty() {
            return Err("configuração: 'system_prompt' não pode ser vazio".into());
        }
        if self.model.name.trim().is_empty() {
            return Err("configuração: 'model.name' não pode ser vazio".into());
        }
        if self.limits.max_steps == 0 {
            return Err("configuração: 'limits.max_steps' deve ser maior que zero".into());
        }
        if self.limits.max_consecutive_failures == 0 {
            return Err("configuração: 'limits.max_consecutive_failures' deve ser maior que zero".into());
        }
        if self.model.num_ctx as usize <= self.limits.reserve_output_tokens {
            return Err("configuração: 'model.num_ctx' deve ser maior que 'limits.reserve_output_tokens'".into());
        }
        Registry::from_names(&self.tools).map(|_| ()).map_err(|e| format!("configuração: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // O arquivo que o binário usa por padrão precisa ser válido.
    const BUNDLED: &str = include_str!("../agents/bitclaw.toml");

    fn minimal(extra: &str) -> String {
        format!(
            r#"
name = "teste"
system_prompt = "oi"
tools = ["read_file"]

[model]
name = "m"
url = "http://localhost:11434"
{extra}
"#
        )
    }

    #[test]
    fn bundled_config_is_valid() {
        let cfg = AgentConfig::from_toml(BUNDLED).expect("agents/bitclaw.toml deve ser válido");
        assert_eq!(cfg.name, "bitclaw");
        assert_eq!(cfg.model.name, "nemotron-3-nano:4b-mlx");
        assert_eq!(cfg.tools.len(), 5);
    }

    #[test]
    fn missing_sections_use_defaults() {
        let cfg = AgentConfig::from_toml(&minimal("")).unwrap();
        assert_eq!(cfg.model.num_ctx, NUM_CTX);
        assert_eq!(cfg.model.temperature, TEMPERATURE);
        assert_eq!(cfg.limits, LimitsConfig::default());
    }

    #[test]
    fn unknown_field_is_an_error() {
        let err = AgentConfig::from_toml(&minimal("temperatura = 0.2")).unwrap_err();
        assert!(err.contains("temperatura"), "{err}");
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let text = minimal("").replace(r#"["read_file"]"#, r#"["read_fil"]"#);
        let err = AgentConfig::from_toml(&text).unwrap_err();
        assert!(err.contains("read_fil"), "{err}");
    }

    #[test]
    fn empty_tool_list_is_an_error() {
        let text = minimal("").replace(r#"["read_file"]"#, "[]");
        assert!(AgentConfig::from_toml(&text).is_err());
    }

    #[test]
    fn context_must_exceed_reserved_output() {
        let text = minimal("num_ctx = 1024");
        let err = AgentConfig::from_toml(&text).unwrap_err();
        assert!(err.contains("num_ctx"), "{err}");
    }

    #[test]
    fn zero_limits_are_errors() {
        let text = minimal("") + "\n[limits]\nmax_steps = 0\n";
        assert!(AgentConfig::from_toml(&text).is_err());
    }
}

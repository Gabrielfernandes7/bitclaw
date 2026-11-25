//! Configuração de um agente, lida de TOML. Prompt, ferramentas, modelo e limites são dados,
//! não código: trocar um deles não exige recompilar o core.

use crate::core::agent::{MAX_CONSECUTIVE_FAILURES, MAX_STEPS};
use serde::Deserialize;

// Janela de contexto padrão, em tokens, quando a configuração não diz.
pub const DEFAULT_CONTEXT_WINDOW: u32 = 16384;

// Campo escrito errado é erro, não um padrão silencioso (`validate` confere o que o serde deixa passar).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AgentConfig {
    pub name: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    // Pasta com skills (subpastas com SKILL.md), relativa ao diretório de execução. Sem valor, não há skills.
    #[serde(default)]
    pub skills_dir: Option<String>,
    pub model: ModelConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    // Seções de provedor (`[ollama]`, `[openrouter]`...). O core não as interpreta: cada adaptador
    // lê a sua com `provider_section`. Qualquer chave que não seja tabela é campo desconhecido.
    #[serde(flatten)]
    pub providers: toml::Table,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub name: String,
    // Janela de contexto do modelo, em tokens. `num_ctx` é o nome antigo.
    #[serde(default = "default_context_window", alias = "num_ctx")]
    pub context_window: u32,
    // Formato antigo: `url` e `temperature` ficavam aqui. Agora são do provedor (`[ollama]`).
    // Ainda são lidos, com aviso (`AgentConfig::warnings`).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    // Raciocínio do modelo ligado desde o início. Gasta mais tokens; o padrão é desligado.
    #[serde(default)]
    pub think: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsConfig {
    pub max_steps: usize,
    pub max_consecutive_failures: usize,
    // Tokens reservados para a resposta do modelo, fora do orçamento do histórico.
    pub reserve_output_tokens: usize,
}

fn default_context_window() -> u32 {
    DEFAULT_CONTEXT_WINDOW
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

    // A seção `[nome]` de um provedor, para o adaptador interpretar. `None` se a configuração não a tem.
    pub fn provider_section(&self, name: &str) -> Option<&toml::Table> {
        self.providers.get(name).and_then(toml::Value::as_table)
    }

    // Avisos sobre formatos antigos ainda aceitos.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for key in ["url", "temperature"] {
            let used = match key {
                "url" => self.model.url.is_some(),
                _ => self.model.temperature.is_some(),
            };
            if used {
                out.push(format!("'model.{key}' está obsoleto: mova para a seção [ollama]."));
            }
        }
        out
    }

    // Verifica o que o TOML não consegue expressar: valores coerentes entre si. Os nomes de `tools` são
    // resolvidos por quem monta o registro (ver `toolkit::registry_from_names`), não pelo core.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("configuração: 'name' não pode ser vazio".into());
        }
        if self.system_prompt.trim().is_empty() {
            return Err("configuração: 'system_prompt' não pode ser vazio".into());
        }
        if self.skills_dir.as_deref().is_some_and(|d| d.trim().is_empty()) {
            return Err("configuração: 'skills_dir' não pode ser vazio".into());
        }
        if let Some(key) = self.providers.iter().find(|(_, v)| !v.is_table()).map(|(k, _)| k) {
            return Err(format!("configuração: campo desconhecido '{key}'"));
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
        if self.model.context_window as usize <= self.limits.reserve_output_tokens {
            return Err("configuração: 'model.context_window' deve ser maior que 'limits.reserve_output_tokens'".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // O arquivo que o binário usa por padrão precisa ser válido.
    const BUNDLED: &str = include_str!("../../agents/bitclaw.toml");

    fn minimal(extra: &str) -> String {
        format!(
            r#"
name = "teste"
system_prompt = "oi"
tools = ["read_file"]

[model]
name = "m"
{extra}
"#
        )
    }

    #[test]
    fn bundled_config_is_valid() {
        let cfg = AgentConfig::from_toml(BUNDLED).expect("agents/bitclaw.toml deve ser válido");
        assert_eq!(cfg.name, "BitClaw");
        assert_eq!(cfg.model.name, "nemotron-3-nano:4b-mlx");
        assert_eq!(cfg.tools.len(), 7);
        assert!(cfg.tools.iter().any(|t| t == "run_script"), "o agente padrão executa scripts, com confirmação");
    }

    #[test]
    fn missing_sections_use_defaults() {
        let cfg = AgentConfig::from_toml(&minimal("")).unwrap();
        assert_eq!(cfg.model.context_window, DEFAULT_CONTEXT_WINDOW);
        assert!(cfg.providers.is_empty());
        assert!(!cfg.model.think, "o raciocínio começa desligado");
        assert_eq!(cfg.limits, LimitsConfig::default());
    }

    #[test]
    fn thinking_can_be_enabled_in_the_config() {
        let cfg = AgentConfig::from_toml(&minimal("think = true")).unwrap();
        assert!(cfg.model.think);
    }

    #[test]
    fn unknown_field_is_an_error() {
        let err = AgentConfig::from_toml(&minimal("temperatura = 0.2")).unwrap_err();
        assert!(err.contains("temperatura"), "{err}");
    }

    #[test]
    fn context_must_exceed_reserved_output() {
        let text = minimal("context_window = 1024");
        let err = AgentConfig::from_toml(&text).unwrap_err();
        assert!(err.contains("context_window"), "{err}");
    }

    #[test]
    fn zero_limits_are_errors() {
        let text = minimal("") + "\n[limits]\nmax_steps = 0\n";
        assert!(AgentConfig::from_toml(&text).is_err());
    }

    #[test]
    fn old_num_ctx_name_still_works() {
        let cfg = AgentConfig::from_toml(&minimal("num_ctx = 8192")).unwrap();
        assert_eq!(cfg.model.context_window, 8192);
    }

    #[test]
    fn provider_sections_are_kept_for_the_adapter() {
        let text = minimal("") + "\n[ollama]\nurl = \"http://x:1\"\n";
        let cfg = AgentConfig::from_toml(&text).unwrap();
        assert_eq!(cfg.provider_section("ollama").unwrap()["url"].as_str(), Some("http://x:1"));
        assert!(cfg.provider_section("openrouter").is_none());
        assert!(cfg.warnings().is_empty());
    }

    #[test]
    fn stray_top_level_value_is_an_error() {
        let text = "skils_dir = \"x\"\n".to_string() + &minimal("");
        let err = AgentConfig::from_toml(&text).unwrap_err();
        assert!(err.contains("skils_dir"), "{err}");
    }

    #[test]
    fn old_url_and_temperature_in_model_warn() {
        let cfg = AgentConfig::from_toml(&minimal("url = \"http://x:1\"\ntemperature = 0.2")).unwrap();
        assert_eq!(cfg.warnings().len(), 2);
    }
}

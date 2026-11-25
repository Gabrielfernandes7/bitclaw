//! Catálogo do OpenRouter, lido de `GET /models`.

use super::auth::ApiKey;
use crate::core::models::{Effort, ModelCapabilities, ModelCatalog, ModelInfo, Reasoning};
use serde_json::Value;
use std::time::Duration;

pub struct OpenRouterCatalog {
    base_url: String,
    key: Option<ApiKey>,
}

impl OpenRouterCatalog {
    pub fn new(base_url: String, key: Option<ApiKey>) -> Self {
        Self { base_url, key }
    }
}

impl ModelCatalog for OpenRouterCatalog {
    // Só modelos que chamam ferramentas: sem isso o agente não faz nada útil.
    fn list(&self) -> Result<Vec<ModelInfo>, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        let mut request = http.get(format!("{}/models", self.base_url));
        if let Some(key) = &self.key {
            request = request.header("Authorization", key.bearer());
        }
        let redact = |text: String| self.key.as_ref().map_or(text.clone(), |k| k.redact(&text));
        let resp = request.send().map_err(|e| redact(format!("falha ao falar com o OpenRouter ({}): {e}", self.base_url)))?;
        if !resp.status().is_success() {
            return Err(format!("OpenRouter respondeu HTTP {} ao listar modelos", resp.status()));
        }
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        let mut models: Vec<ModelInfo> = v
            .get("data")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .filter_map(model_from_entry)
            .filter(|m| m.capabilities.tool_calling)
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }
}

// Converte uma entrada de `/models`. Sem `supported_parameters`, ferramentas são assumidas e raciocínio não.
pub fn model_from_entry(entry: &Value) -> Option<ModelInfo> {
    let id = entry.get("id")?.as_str()?.to_string();
    let params: Option<Vec<&str>> =
        entry.get("supported_parameters").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_str).collect());
    let has = |p: &str| params.as_ref().is_some_and(|list| list.contains(&p));
    let reasoning = if has("reasoning") {
        Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High])
    } else {
        Reasoning::Unsupported
    };
    Some(ModelInfo {
        id,
        provider: "openrouter".into(),
        capabilities: ModelCapabilities {
            tool_calling: params.is_none() || has("tools"),
            reasoning,
            context_window: entry.get("context_length").and_then(Value::as_u64).map(|n| n as usize),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capabilities_come_from_the_supported_parameters() {
        let m = model_from_entry(&json!({
            "id": "vendor/thinker", "context_length": 200000,
            "supported_parameters": ["tools", "reasoning", "temperature"]
        }))
        .unwrap();
        assert_eq!(m.provider, "openrouter");
        assert!(m.capabilities.tool_calling);
        assert_eq!(m.capabilities.context_window, Some(200000));
        assert!(m.capabilities.supports(Effort::High));
    }

    #[test]
    fn a_model_without_tools_is_marked_and_a_bare_entry_is_assumed_to_have_them() {
        let no_tools = model_from_entry(&json!({"id": "a/b", "supported_parameters": ["temperature"]})).unwrap();
        assert!(!no_tools.capabilities.tool_calling);
        assert_eq!(no_tools.capabilities.reasoning, Reasoning::Unsupported);
        let bare = model_from_entry(&json!({"id": "a/c"})).unwrap();
        assert!(bare.capabilities.tool_calling);
        assert_eq!(bare.capabilities.context_window, None);
    }
}

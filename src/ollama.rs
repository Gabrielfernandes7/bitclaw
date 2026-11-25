//! Adaptador do Ollama: traduz o núcleo (modelos, esforço, ferramentas) para a API `/api/chat`.
//! Tudo que é específico do Ollama fica aqui; o resto do BitClaw não sabe que ele existe.

use crate::agent::{ChatModel, Reply};
use crate::models::{Effort, ModelCapabilities, ModelCatalog, ModelInfo, Reasoning, SharedEffort};
use serde_json::{json, Value};
use std::time::Duration;

// Janela de contexto e temperatura padrão. Sem `num_ctx` explícito, o Ollama usa uma janela pequena.
pub const NUM_CTX: u32 = 16384; // 16384 ou 8192
pub const TEMPERATURE: f32 = 0.6;

// Opções de geração enviadas ao Ollama. Vêm da configuração do agente.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub num_ctx: u32,
    pub temperature: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { num_ctx: NUM_CTX, temperature: TEMPERATURE }
    }
}

// Valor do campo `think` para este modelo e este esforço. `None` quando o modelo não raciocina:
// o campo é omitido em vez de enviado com um valor que o modelo não entende.
pub fn think_value(caps: &ModelCapabilities, effort: Effort) -> Option<Value> {
    match caps.reasoning {
        Reasoning::Unsupported => None,
        Reasoning::Toggle => Some(json!(!effort.is_off())),
        Reasoning::Levels(_) => Some(match effort {
            Effort::Low | Effort::Medium | Effort::High => json!(effort.as_str()),
            Effort::On => json!(true),
            Effort::Off => json!(false),
        }),
    }
}

// Corpo da requisição `/api/chat`. Separado do envio para ser testável sem rede.
// Sem ferramentas (lista vazia) o campo `tools` não é enviado.
pub fn build_body(model: &str, messages: &[Value], tools: &Value, options: &Options, think: Option<Value>) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "options": { "temperature": options.temperature, "num_ctx": options.num_ctx }
    });
    if tools.as_array().is_some_and(|t| !t.is_empty()) {
        body["tools"] = tools.clone();
    }
    if let Some(think) = think {
        body["think"] = think;
    }
    body
}

pub struct Client {
    http: reqwest::blocking::Client,
    base_url: String,
    pub model: String,
    pub options: Options,
    caps: ModelCapabilities,
    effort: SharedEffort,
}

// Dados de exibição do modelo, pedidos ao `/api/show`. Campos ausentes ficam como `None`.
pub struct ModelDetails {
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    // Janela máxima que o modelo suporta (não a que está em uso, que é `NUM_CTX`).
    pub context_length: Option<u64>,
}

// O núcleo depende da trait `ChatModel`; este adaptador é a implementação para o Ollama.
impl ChatModel for Client {
    fn complete(&self, messages: &[Value], tools: &Value) -> Result<Reply, String> {
        self.chat(messages, tools)
    }
}

impl Client {
    pub fn new(
        base_url: String,
        model: String,
        options: Options,
        caps: ModelCapabilities,
        effort: SharedEffort,
    ) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, base_url, model, options, caps, effort })
    }

    /// Envia o histórico e as ferramentas; devolve a mensagem do assistente.
    pub fn chat(&self, messages: &[Value], tools: &Value) -> Result<Reply, String> {
        let effort = *self.effort.lock().unwrap_or_else(|e| e.into_inner());
        let think = think_value(&self.caps, effort);
        // Modelo sem ferramentas recebe a conversa sem o catálogo: não tenta chamar nada.
        let no_tools = json!([]);
        let tools = if self.caps.tool_calling { tools } else { &no_tools };
        let body = build_body(&self.model, messages, tools, &self.options, think);
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| format!("falha ao conectar no Ollama ({}): {e}", self.base_url))?;
        let status = resp.status();
        if !status.is_success() {
            // O corpo traz a causa real (ex.: "model not found"), então não é descartado.
            let detail = resp.text().unwrap_or_default();
            return Err(format!("Ollama respondeu HTTP {status}: {}", detail.trim()));
        }
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        Ok(Reply {
            message: v.get("message").cloned().unwrap_or(Value::Null),
            prompt_tokens: v.get("prompt_eval_count").and_then(Value::as_u64),
        })
    }

    /// Detalhes do modelo para mostrar ao usuário. Campos ausentes ficam como `None`.
    pub fn model_details(&self) -> Result<ModelDetails, String> {
        let resp = self
            .http
            .post(format!("{}/api/show", self.base_url))
            .json(&json!({ "model": self.model }))
            .send()
            .map_err(|e| format!("falha ao conectar no Ollama ({}): {e}", self.base_url))?;
        if !resp.status().is_success() {
            return Err(format!("Ollama respondeu HTTP {}", resp.status()));
        }
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        let details = v.get("details");
        let context_length = v
            .get("model_info")
            .and_then(Value::as_object)
            .and_then(|m| m.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, v)| v.as_u64()));
        Ok(ModelDetails {
            parameter_size: details.and_then(|d| d.get("parameter_size")).and_then(Value::as_str).map(String::from),
            quantization: details.and_then(|d| d.get("quantization_level")).and_then(Value::as_str).map(String::from),
            context_length,
        })
    }

    /// Verifica se o modelo configurado está instalado no Ollama.
    pub fn has_model(&self) -> Result<bool, String> {
        let resp = self
            .http
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .map_err(|e| format!("falha ao conectar no Ollama ({}): {e}", self.base_url))?;
        if !resp.status().is_success() {
            return Err(format!("Ollama respondeu HTTP {}", resp.status()));
        }
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        // Sem tag, o Ollama usa `:latest`.
        let wanted = [self.model.clone(), format!("{}:latest", self.model)];
        Ok(v.get("models")
            .and_then(Value::as_array)
            .is_some_and(|ms| {
                ms.iter()
                    .filter_map(|m| m.get("name").and_then(Value::as_str))
                    .any(|n| wanted.iter().any(|w| w == n))
            }))
    }
}

// Catálogo dos modelos instalados no Ollama, lido de `/api/tags`.
pub struct OllamaCatalog {
    base_url: String,
}

impl OllamaCatalog {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

impl ModelCatalog for OllamaCatalog {
    fn list(&self) -> Result<Vec<ModelInfo>, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = http
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .map_err(|e| format!("falha ao conectar no Ollama ({}): {e}", self.base_url))?;
        if !resp.status().is_success() {
            return Err(format!("Ollama respondeu HTTP {}", resp.status()));
        }
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        let models = v.get("models").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        Ok(models.iter().filter_map(model_from_tags).collect())
    }
}

// Converte uma entrada de `/api/tags`. Sem o campo `capabilities` (Ollama antigo), nada é presumido
// sobre raciocínio, e ferramentas são assumidas: é o que o agente precisa.
pub fn model_from_tags(entry: &Value) -> Option<ModelInfo> {
    let id = entry.get("name")?.as_str()?.to_string();
    let reported: Option<Vec<&str>> =
        entry.get("capabilities").and_then(Value::as_array).map(|c| c.iter().filter_map(Value::as_str).collect());
    let has = |cap: &str| reported.as_ref().is_some_and(|c| c.contains(&cap));
    let reasoning = if !has("thinking") {
        Reasoning::Unsupported
    } else if id.contains("gpt-oss") {
        // Esta família aceita níveis de esforço em vez de só liga/desliga.
        Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High])
    } else {
        Reasoning::Toggle
    };
    let context_window = entry
        .get("details")
        .and_then(|d| d.get("context_length"))
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    Some(ModelInfo {
        id,
        provider: "ollama".into(),
        capabilities: ModelCapabilities {
            tool_calling: reported.is_none() || has("tools"),
            reasoning,
            context_window,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels() -> ModelCapabilities {
        ModelCapabilities {
            tool_calling: true,
            reasoning: Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High]),
            context_window: None,
        }
    }

    fn toggle() -> ModelCapabilities {
        ModelCapabilities { tool_calling: true, reasoning: Reasoning::Toggle, context_window: None }
    }

    fn plain() -> ModelCapabilities {
        ModelCapabilities::unknown()
    }

    #[test]
    fn request_sets_explicit_context_window() {
        let body = build_body("m", &[], &json!([]), &Options::default(), None);
        assert_eq!(body["options"]["num_ctx"], NUM_CTX);
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "m");
    }

    #[test]
    fn think_field_follows_the_model_and_the_effort() {
        assert_eq!(think_value(&plain(), Effort::Off), None, "sem raciocínio, o campo não existe");
        assert_eq!(think_value(&toggle(), Effort::Off), Some(json!(false)));
        assert_eq!(think_value(&toggle(), Effort::On), Some(json!(true)));
        assert_eq!(think_value(&levels(), Effort::High), Some(json!("high")));
        assert_eq!(think_value(&levels(), Effort::Off), Some(json!(false)));
    }

    #[test]
    fn tools_are_sent_only_when_there_are_some() {
        let body = build_body("m", &[], &json!([]), &Options::default(), None);
        assert!(body.get("tools").is_none());
        let with = build_body("m", &[], &json!([{"type": "function"}]), &Options::default(), None);
        assert!(with.get("tools").is_some());
    }

    #[test]
    fn think_is_omitted_when_the_model_cannot_reason() {
        let body = build_body("m", &[], &json!([]), &Options::default(), think_value(&plain(), Effort::Off));
        assert!(body.get("think").is_none());
    }

    #[test]
    fn catalog_reads_capabilities_reported_by_ollama() {
        let thinker = json!({"name": "nemotron:4b", "capabilities": ["completion", "tools", "thinking"]});
        let info = model_from_tags(&thinker).unwrap();
        assert_eq!(info.capabilities.reasoning, Reasoning::Toggle);
        assert!(info.capabilities.tool_calling);

        let coder = json!({
            "name": "qwen2.5-coder:3b",
            "capabilities": ["completion", "tools", "insert"],
            "details": {"context_length": 32768}
        });
        let info = model_from_tags(&coder).unwrap();
        assert_eq!(info.capabilities.reasoning, Reasoning::Unsupported);
        assert_eq!(info.capabilities.context_window, Some(32768));

        let gpt = json!({"name": "gpt-oss:20b", "capabilities": ["completion", "tools", "thinking"]});
        assert_eq!(model_from_tags(&gpt).unwrap().capabilities.reasoning, Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High]));
    }

    #[test]
    fn model_without_tool_support_is_marked_as_such() {
        let chat_only = json!({"name": "tiny", "capabilities": ["completion"]});
        assert!(!model_from_tags(&chat_only).unwrap().capabilities.tool_calling);
    }

    #[test]
    fn old_ollama_without_capabilities_is_treated_conservatively() {
        let old = json!({"name": "antigo:1b"});
        let caps = model_from_tags(&old).unwrap().capabilities;
        assert_eq!(caps, ModelCapabilities::unknown());
    }
}

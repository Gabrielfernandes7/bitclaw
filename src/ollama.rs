use crate::agent::{ChatModel, Reply};
use serde_json::{json, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// Janela de contexto e temperatura padrão. Sem `num_ctx` explícito, o Ollama usa uma janela pequena.
pub const NUM_CTX: u32 = 16384; // 16384 ou 8192
pub const TEMPERATURE: f32 = 0.6;

// Opções de geração enviadas ao Ollama. Vêm da configuração do agente.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub num_ctx: u32,
    pub temperature: f32,
    // O modelo raciocina antes de responder. Gasta mais tokens e enche a janela mais rápido.
    pub think: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { num_ctx: NUM_CTX, temperature: TEMPERATURE, think: false }
    }
}

// Corpo da requisição `/api/chat`. Separado do envio para ser testável sem rede.
pub fn build_body(model: &str, messages: &[Value], tools: &Value, options: &Options) -> Value {
    json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "stream": false,
        "think": options.think,
        "options": { "temperature": options.temperature, "num_ctx": options.num_ctx }
    })
}

pub struct Client {
    http: reqwest::blocking::Client,
    base_url: String,
    pub model: String,
    pub options: Options,
    // Liga e desliga o raciocínio em tempo de execução, sem recriar o cliente.
    think: Arc<AtomicBool>,
}

// Dados do modelo informados pelo Ollama.
pub struct ModelInfo {
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
    pub fn new(base_url: String, model: String, options: Options) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?;
        let think = Arc::new(AtomicBool::new(options.think));
        Ok(Self { http, base_url, model, options, think })
    }

    // Usa um indicador compartilhado para o raciocínio: quem o tem pode ligá-lo e desligá-lo.
    pub fn with_think(mut self, think: Arc<AtomicBool>) -> Self {
        self.think = think;
        self
    }

    /// Envia o histórico e as ferramentas; devolve a mensagem do assistente.
    pub fn chat(&self, messages: &[Value], tools: &Value) -> Result<Reply, String> {
        let options = Options { think: self.think.load(Ordering::Relaxed), ..self.options };
        let body = build_body(&self.model, messages, tools, &options);
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

    /// Informações do modelo para mostrar ao usuário. Campos ausentes ficam como `None`.
    pub fn model_info(&self) -> Result<ModelInfo, String> {
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
        Ok(ModelInfo {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_sets_explicit_context_window() {
        let body = build_body("m", &[], &json!([]), &Options::default());
        assert_eq!(body["options"]["num_ctx"], NUM_CTX);
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "m");
    }

    #[test]
    fn thinking_is_off_by_default_and_follows_the_option() {
        let off = build_body("m", &[], &json!([]), &Options::default());
        assert_eq!(off["think"], false);
        let on = build_body("m", &[], &json!([]), &Options { think: true, ..Options::default() });
        assert_eq!(on["think"], true);
    }

    #[test]
    fn the_shared_flag_decides_each_request() {
        let flag = Arc::new(AtomicBool::new(false));
        let client = Client::new("http://x".into(), "m".into(), Options::default()).unwrap().with_think(Arc::clone(&flag));
        assert!(!client.think.load(Ordering::Relaxed));
        flag.store(true, Ordering::Relaxed);
        assert!(client.think.load(Ordering::Relaxed));
    }
}

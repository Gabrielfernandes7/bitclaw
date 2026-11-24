use serde_json::{json, Value};
use std::time::Duration;

pub struct Client {
    http: reqwest::blocking::Client,
    base_url: String,
    pub model: String,
}

impl Client {
    pub fn new(base_url: String, model: String) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, base_url, model })
    }

    /// Envia o histórico e as ferramentas; devolve a mensagem do assistente.
    pub fn chat(&self, messages: &[Value], tools: &Value) -> Result<Value, String> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "stream": false,
            "think": false,
            "options": { "temperature": 0.1 }
        });
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| format!("falha ao conectar no Ollama ({}): {e}", self.base_url))?;
        if !resp.status().is_success() {
            return Err(format!("Ollama respondeu HTTP {}", resp.status()));
        }
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        Ok(v.get("message").cloned().unwrap_or(Value::Null))
    }
}

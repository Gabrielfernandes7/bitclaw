//! Cliente do OpenRouter: `ChatModel` sobre `POST /chat/completions`, com streaming SSE.

use super::auth::ApiKey;
use super::messages::to_openai;
use crate::adapters::lines::{self, MAX_CALL};
use crate::core::agent::{ChatModel, Chunk, Reply};
use crate::core::models::{Effort, ModelCapabilities, Reasoning, SharedEffort};
use crate::core::options::{Options, SharedOptions};
use crate::core::tool::ToolSpec;
use serde_json::{Value, json};
use std::time::Duration;

pub struct Client {
    http: reqwest::blocking::Client,
    base_url: String,
    key: ApiKey,
    key_env: String,
    pub model: String,
    options: SharedOptions,
    caps: ModelCapabilities,
    effort: SharedEffort,
}

impl Client {
    pub fn new(
        base_url: String,
        key: ApiKey,
        key_env: String,
        model: String,
        options: SharedOptions,
        caps: ModelCapabilities,
        effort: SharedEffort,
    ) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(MAX_CALL)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { http, base_url, key, key_env, model, options, caps, effort })
    }

    fn body(&self, messages: &[Value], tools: &[ToolSpec]) -> Value {
        let effort = *self.effort.lock().unwrap_or_else(|e| e.into_inner());
        let options = *self.options.lock().unwrap_or_else(|e| e.into_inner());
        let tools = if self.caps.tool_calling { tools } else { &[] };
        build_body(&self.model, messages, tools, &options, reasoning_value(&self.caps, effort))
    }

    // Erro legível para um status HTTP. Não ecoa cabeçalhos, e o corpo passa pela remoção da chave.
    fn http_error(&self, status: reqwest::StatusCode, detail: &str) -> String {
        match status.as_u16() {
            401 => format!("o OpenRouter recusou a chave (HTTP 401): confira a variável {}", self.key_env),
            402 => "o OpenRouter diz que faltam créditos (HTTP 402): adicione saldo à conta".into(),
            429 => "o OpenRouter limitou as requisições (HTTP 429): espere um instante e tente de novo".into(),
            _ => {
                let message = serde_json::from_str::<Value>(detail)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(String::from))
                    .unwrap_or_else(|| detail.trim().to_string());
                self.key.redact(&format!("o OpenRouter respondeu HTTP {status}: {message}"))
            }
        }
    }
}

// Pedido de raciocínio: só modelos com níveis. Desligado, pede `effort: "none"`: sem isso o modelo raciocina
// por padrão e o texto chega mesmo com o rótulo "thinking off". Modelos de raciocínio obrigatório rejeitam
// `none`; o catálogo ainda não lê essa marca, então o erro do provedor aparece como está.
fn reasoning_value(caps: &ModelCapabilities, effort: Effort) -> Option<Value> {
    match (&caps.reasoning, effort) {
        (Reasoning::Levels(_), Effort::Low | Effort::Medium | Effort::High) => Some(json!({"effort": effort.as_str()})),
        (Reasoning::Levels(_), Effort::On) => Some(json!({"effort": "medium"})),
        (Reasoning::Levels(_), Effort::Off) => Some(json!({"effort": "none"})),
        _ => None,
    }
}

// Corpo da requisição. Separado do envio para ser testável sem rede. A janela (`num_ctx`) não vai: no
// OpenRouter ela é do modelo, e aqui só limita o orçamento do histórico.
pub fn build_body(model: &str, messages: &[Value], tools: &[ToolSpec], options: &Options, reasoning: Option<Value>) -> Value {
    let mut body = json!({
        "model": model,
        "messages": to_openai(messages),
        "stream": true,
        "temperature": options.temperature,
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
                .collect(),
        );
    }
    if let Some(reasoning) = reasoning {
        body["reasoning"] = reasoning;
    }
    body
}

// Junta as linhas SSE numa resposta só, entregando cada pedaço de texto a `on_chunk`. Se ele devolve
// `false`, para e fica com o que juntou. Um item `None` vira `Chunk::Idle`, para o sink poder cancelar.
// As chamadas de ferramenta chegam em pedaços, por `index`; os argumentos são juntados e lidos no fim.
pub fn fold_sse(
    lines: impl Iterator<Item = Result<Option<String>, String>>,
    on_chunk: &mut dyn FnMut(Chunk) -> bool,
) -> Result<Reply, String> {
    let mut content = String::new();
    let mut thinking = String::new();
    // (id, nome, argumentos em texto), na ordem do `index`.
    let mut calls: Vec<(String, String, String)> = Vec::new();
    let (mut prompt_tokens, mut output_tokens) = (None, None);
    for line in lines {
        let Some(line) = line? else {
            if !on_chunk(Chunk::Idle) {
                break;
            }
            continue;
        };
        let line = line.trim();
        // Linhas vazias e comentários (`: OPENROUTER PROCESSING`) são só para manter a conexão viva.
        let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
        if data == "[DONE]" {
            break;
        }
        let v: Value = serde_json::from_str(data).map_err(|e| format!("resposta inválida do OpenRouter: {e}"))?;
        if let Some(err) = v.get("error") {
            let message = err.get("message").and_then(Value::as_str).unwrap_or("erro sem mensagem");
            return Err(format!("OpenRouter: {message}"));
        }
        if let Some(usage) = v.get("usage").filter(|u| u.is_object()) {
            prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64).or(prompt_tokens);
            output_tokens = usage.get("completion_tokens").and_then(Value::as_u64).or(output_tokens);
        }
        let Some(delta) = v.pointer("/choices/0/delta") else { continue };
        if let Some(t) = delta.get("reasoning").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            thinking.push_str(t);
            if !on_chunk(Chunk::Thinking(t)) {
                break;
            }
        }
        if let Some(c) = delta.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
            content.push_str(c);
            if !on_chunk(Chunk::Content(c)) {
                break;
            }
        }
        for piece in delta.get("tool_calls").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let index = piece.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            while calls.len() <= index {
                calls.push(Default::default());
            }
            let call = &mut calls[index];
            if let Some(id) = piece.get("id").and_then(Value::as_str) {
                call.0 = id.to_string();
            }
            if let Some(name) = piece.pointer("/function/name").and_then(Value::as_str) {
                call.1.push_str(name);
            }
            if let Some(args) = piece.pointer("/function/arguments").and_then(Value::as_str) {
                call.2.push_str(args);
            }
        }
    }
    let mut message = json!({"role": "assistant", "content": content});
    if !thinking.is_empty() {
        message["thinking"] = json!(thinking);
    }
    let calls: Vec<Value> = calls
        .into_iter()
        .filter(|(_, name, _)| !name.is_empty())
        .map(|(id, name, args)| {
            let arguments = if args.trim().is_empty() { json!({}) } else { serde_json::from_str(&args).unwrap_or(Value::String(args)) };
            let mut call = json!({"function": {"name": name, "arguments": arguments}});
            if !id.is_empty() {
                call["id"] = json!(id);
            }
            call
        })
        .collect();
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    Ok(Reply { message, prompt_tokens, output_tokens })
}

impl ChatModel for Client {
    fn complete(&self, messages: &[Value], tools: &[ToolSpec]) -> Result<Reply, String> {
        self.complete_streaming(messages, tools, &mut |_| true)
    }

    fn complete_streaming(
        &self,
        messages: &[Value],
        tools: &[ToolSpec],
        on_chunk: &mut dyn FnMut(Chunk) -> bool,
    ) -> Result<Reply, String> {
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", self.key.bearer())
            .json(&self.body(messages, tools))
            .send()
            .map_err(|e| self.key.redact(&format!("falha ao falar com o OpenRouter ({}): {e}", self.base_url)))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(self.http_error(status, &resp.text().unwrap_or_default()));
        }
        fold_sse(lines::stream(resp, "OpenRouter"), on_chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sse(lines: &[&str]) -> impl Iterator<Item = Result<Option<String>, String>> {
        lines.iter().map(|l| Ok(Some(l.to_string()))).collect::<Vec<_>>().into_iter()
    }

    fn levels() -> ModelCapabilities {
        ModelCapabilities { tool_calling: true, reasoning: Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High]), context_window: None }
    }

    #[test]
    fn text_reasoning_and_usage_are_folded() {
        let mut seen = Vec::new();
        let reply = fold_sse(
            sse(&[
                ": OPENROUTER PROCESSING",
                "",
                r#"data: {"choices":[{"delta":{"reasoning":"hmm"}}]}"#,
                r#"data: {"choices":[{"delta":{"content":"Olá"}}]}"#,
                r#"data: {"choices":[{"delta":{"content":", mundo"}}]}"#,
                r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#,
                "data: [DONE]",
            ]),
            &mut |c| {
                seen.push(format!("{c:?}"));
                true
            },
        )
        .unwrap();
        assert_eq!(reply.message["content"], "Olá, mundo");
        assert_eq!(reply.message["thinking"], "hmm");
        assert_eq!((reply.prompt_tokens, reply.output_tokens), (Some(12), Some(3)));
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn tool_call_arguments_arrive_in_pieces_and_are_joined() {
        let reply = fold_sse(
            sse(&[
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":""}}]}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\":"}}]}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.txt\"}"}}]}}]}"#,
                r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_2","function":{"name":"list_dir"}}]}}]}"#,
                "data: [DONE]",
            ]),
            &mut |_| true,
        )
        .unwrap();
        let calls = reply.message["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["id"], "call_1");
        assert_eq!(calls[0]["function"]["arguments"], json!({"path": "a.txt"}));
        assert_eq!(calls[1]["function"]["arguments"], json!({}), "sem argumentos vira objeto vazio");
    }

    #[test]
    fn the_sink_can_stop_the_stream_and_idle_is_forwarded() {
        let mut idles = 0;
        let lines = vec![Ok(None), Ok(Some(r#"data: {"choices":[{"delta":{"content":"a"}}]}"#.to_string())), Ok(Some(r#"data: {"choices":[{"delta":{"content":"b"}}]}"#.to_string()))];
        let reply = fold_sse(lines.into_iter(), &mut |c| {
            if c == Chunk::Idle {
                idles += 1;
                return true;
            }
            false
        })
        .unwrap();
        assert_eq!(idles, 1);
        assert_eq!(reply.message["content"], "a", "parou no primeiro texto");
    }

    #[test]
    fn an_error_in_the_stream_is_reported_without_the_raw_payload() {
        let err = fold_sse(sse(&[r#"data: {"error":{"message":"modelo indisponível","code":502}}"#]), &mut |_| true).err().unwrap();
        assert_eq!(err, "OpenRouter: modelo indisponível");
    }

    #[test]
    fn the_body_is_in_openai_format_and_leaves_the_window_out() {
        let tools = [ToolSpec::new("x", "y", json!({"type": "object"}))];
        let body = build_body("vendor/m", &[json!({"role": "user", "content": "oi"})], &tools, &Options::default(), None);
        assert_eq!(body["model"], "vendor/m");
        assert_eq!(body["stream"], true);
        assert_eq!(body["tools"][0]["function"]["name"], "x");
        assert!(body.get("num_ctx").is_none() && body.get("options").is_none());
        assert!(body.get("reasoning").is_none());
        let empty = build_body("m", &[], &[], &Options::default(), None);
        assert!(empty.get("tools").is_none());
    }

    fn client(key: &str) -> Client {
        Client::new(
            "http://x/v1".into(),
            ApiKey::new(key.into()),
            "OPENROUTER_API_KEY".into(),
            "m".into(),
            Default::default(),
            ModelCapabilities::unknown(),
            std::sync::Arc::new(std::sync::Mutex::new(Effort::Off)),
        )
        .unwrap()
    }

    #[test]
    fn http_errors_are_readable_and_never_carry_the_key() {
        let c = client("sk-or-segredo");
        let status = |n| reqwest::StatusCode::from_u16(n).unwrap();
        assert!(c.http_error(status(401), "").contains("OPENROUTER_API_KEY"));
        assert!(c.http_error(status(402), "").contains("créditos"));
        assert!(c.http_error(status(429), "").contains("limitou"));
        let other = c.http_error(status(500), r#"{"error":{"message":"falhou com sk-or-segredo"}}"#);
        assert!(other.contains("falhou") && !other.contains("sk-or-segredo"), "{other}");
    }

    // Servidor de uma conexão só: devolve `response` e entrega o pedido que recebeu.
    fn mock_server(response: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            // Lê até o fim do corpo (Content-Length) para o cliente não ver a conexão fechar no meio.
            loop {
                let n = socket.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(at) = text.find("\r\n\r\n") {
                    let len = text.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if request.len() >= at + 4 + len {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&request).to_string());
            socket.write_all(response.as_bytes()).unwrap();
        });
        (base, rx)
    }

    #[test]
    fn a_full_call_sends_the_bearer_header_and_reads_the_stream() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
        let response: &'static str = Box::leak(
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_boxed_str(),
        );
        let (base, request) = mock_server(response);
        let mut c = client("sk-or-segredo");
        c.base_url = base;
        let reply = c.complete(&[json!({"role": "user", "content": "oi"})], &[]).unwrap();
        assert_eq!(reply.message["content"], "ok");
        let request = request.recv().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions"), "{request}");
        assert!(request.to_lowercase().contains("authorization: bearer sk-or-segredo"), "{request}");
    }

    #[test]
    fn an_unauthorized_call_gives_a_readable_error_without_the_key() {
        let (base, _) = mock_server("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let mut c = client("sk-or-segredo");
        c.base_url = base;
        let err = c.complete(&[], &[]).err().unwrap();
        assert!(err.contains("401") && err.contains("OPENROUTER_API_KEY") && !err.contains("sk-or-segredo"), "{err}");
    }

    #[test]
    fn reasoning_is_requested_only_for_models_with_levels_and_only_when_on() {
        assert_eq!(reasoning_value(&levels(), Effort::High), Some(json!({"effort": "high"})));
        assert_eq!(reasoning_value(&levels(), Effort::Off), Some(json!({"effort": "none"})));
        assert_eq!(reasoning_value(&ModelCapabilities::unknown(), Effort::High), None);
    }
}

//! Do histórico neutro do core para as mensagens do formato OpenAI.
//!
//! O core guarda `tool_calls` com `arguments` como objeto e resultados como `{"role": "tool", "tool_name"}`.
//! A API do OpenAI quer `arguments` em texto e um `tool_call_id` ligando cada resultado à sua chamada.

use serde_json::{Value, json};

pub(super) fn to_openai(messages: &[Value]) -> Vec<Value> {
    // Chamadas do último turno do assistente que ainda não receberam resultado: (id, nome).
    let mut pending: Vec<(String, String)> = Vec::new();
    let mut out = Vec::with_capacity(messages.len());
    for (i, message) in messages.iter().enumerate() {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
        match role {
            "assistant" => {
                pending.clear();
                let mut m = json!({"role": "assistant", "content": content});
                let calls = message.get("tool_calls").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
                let mut wire = Vec::new();
                for (k, call) in calls.iter().enumerate() {
                    let func = call.get("function").cloned().unwrap_or(Value::Null);
                    let name = func.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let id = call.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("call_{i}_{k}"));
                    let arguments = match func.get("arguments") {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => v.to_string(),
                        None => "{}".into(),
                    };
                    wire.push(json!({"id": id, "type": "function", "function": {"name": name, "arguments": arguments}}));
                    pending.push((id, name));
                }
                if !wire.is_empty() {
                    m["tool_calls"] = Value::Array(wire);
                }
                out.push(m);
            }
            "tool" => {
                let name = message.get("tool_name").and_then(Value::as_str).unwrap_or("");
                let id = match pending.iter().position(|(_, n)| n == name) {
                    Some(at) => pending.remove(at).0,
                    None => format!("call_{i}_orphan"),
                };
                out.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
            }
            other => out.push(json!({"role": other, "content": content})),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_get_ids_string_arguments_and_results_point_back_to_them() {
        let history = vec![
            json!({"role": "system", "content": "s"}),
            json!({"role": "user", "content": "u"}),
            json!({"role": "assistant", "content": "", "thinking": "hmm", "tool_calls": [
                {"function": {"name": "read_file", "arguments": {"path": "a"}}},
                {"function": {"name": "list_dir", "arguments": {"path": "."}}}
            ]}),
            json!({"role": "tool", "tool_name": "list_dir", "content": "x"}),
            json!({"role": "tool", "tool_name": "read_file", "content": "y"}),
        ];
        let out = to_openai(&history);
        assert!(out[2].get("thinking").is_none(), "o raciocínio não volta ao modelo");
        let calls = out[2]["tool_calls"].as_array().unwrap();
        assert_eq!(calls[0]["function"]["arguments"], "{\"path\":\"a\"}");
        assert_eq!(out[3]["tool_call_id"], calls[1]["id"], "o resultado de list_dir aponta para a chamada de list_dir");
        assert_eq!(out[4]["tool_call_id"], calls[0]["id"]);
    }

    #[test]
    fn ids_and_string_arguments_from_the_provider_are_kept() {
        let history = vec![json!({"role": "assistant", "content": "", "tool_calls": [
            {"id": "abc", "function": {"name": "f", "arguments": "{\"x\":1}"}}
        ]})];
        let out = to_openai(&history);
        assert_eq!(out[0]["tool_calls"][0]["id"], "abc");
        assert_eq!(out[0]["tool_calls"][0]["function"]["arguments"], "{\"x\":1}");
    }
}

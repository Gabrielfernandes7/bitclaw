//! Modo restrito: em vez do `tools` nativo, o Ollama recebe um JSON Schema em `format` e o decoder
//! só consegue emitir uma chamada válida (ferramenta existente, argumentos no formato certo) ou uma
//! resposta final. Para um modelo de ~1B, que erra o JSON e o nome da ferramenta, isso troca
//! "torcer para sair certo" por "não tem como sair errado".
//!
//! O resto do BitClaw não percebe: a resposta volta como mensagem com `tool_calls`, igual à nativa.

use crate::core::tool::ToolSpec;
use serde_json::{json, Value};

// Schema de uma saída: ou `{"tool": <nome>, "args": {...}}` ou `{"answer": "<texto>"}`.
pub fn schema(tools: &[ToolSpec]) -> Value {
    let mut options: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "type": "object",
                "properties": {"tool": {"const": t.name}, "args": t.parameters},
                "required": ["tool", "args"],
                "additionalProperties": false
            })
        })
        .collect();
    options.push(json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    }));
    json!({"anyOf": options})
}

// O catálogo em texto curto. O schema do `format` só restringe a geração; o modelo não o enxerga,
// então nomes, descrições e parâmetros vão no prompt.
fn catalog(tools: &[ToolSpec]) -> String {
    let mut out = String::from(
        "\n\nResponda SEMPRE com um objeto JSON. Para usar uma ferramenta: \
         {\"tool\": \"<nome>\", \"args\": {...}}. Para responder ao usuário: {\"answer\": \"<texto>\"}.\nFerramentas:\n",
    );
    for t in tools {
        let params = t.parameters.get("properties").and_then(Value::as_object);
        let names: Vec<&str> = params.map(|p| p.keys().map(String::as_str).collect()).unwrap_or_default();
        out.push_str(&format!("- {}({}): {}\n", t.name, names.join(", "), t.description));
    }
    out
}

// Reescreve o histórico para quem não recebe `tools`: o catálogo entra no prompt de sistema, cada
// chamada do assistente vira o JSON que ele emitiria e cada resultado vira uma mensagem do usuário.
pub fn plain_messages(messages: &[Value], tools: &[ToolSpec]) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len());
    let mut has_system = false;
    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("");
        let content = m.get("content").and_then(Value::as_str).unwrap_or("");
        match role {
            "system" if !has_system => {
                has_system = true;
                out.push(json!({"role": "system", "content": format!("{content}{}", catalog(tools))}));
            }
            "assistant" => {
                let call = m.get("tool_calls").and_then(Value::as_array).and_then(|c| c.first());
                let text = match call {
                    Some(c) => {
                        let f = &c["function"];
                        json!({"tool": f["name"], "args": f["arguments"]}).to_string()
                    }
                    None => json!({"answer": content}).to_string(),
                };
                out.push(json!({"role": "assistant", "content": text}));
            }
            "tool" => {
                let name = m.get("tool_name").and_then(Value::as_str).unwrap_or("ferramenta");
                out.push(json!({"role": "user", "content": format!("Resultado de {name}:\n{content}")}));
            }
            _ => out.push(m.clone()),
        }
    }
    if !has_system {
        out.insert(0, json!({"role": "system", "content": catalog(tools).trim().to_string()}));
    }
    out
}

// Converte o JSON gerado de volta na mensagem que o núcleo entende. Texto que não é JSON (modelo que
// ignorou o `format`) vira resposta comum, sem perder nada.
pub fn into_message(content: &str, known: &[ToolSpec]) -> Value {
    let parsed: Option<Value> = serde_json::from_str(content.trim()).ok();
    if let Some(v) = &parsed {
        if let Some(name) = v.get("tool").and_then(Value::as_str).filter(|n| known.iter().any(|t| t.name == *n)) {
            let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
            return json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{"function": {"name": name, "arguments": args}}]
            });
        }
        if let Some(answer) = v.get("answer").and_then(Value::as_str) {
            return json!({"role": "assistant", "content": answer});
        }
    }
    json!({"role": "assistant", "content": content})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<ToolSpec> {
        vec![ToolSpec::new(
            "read_file",
            "Lê um arquivo.",
            json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
        )]
    }

    #[test]
    fn schema_has_one_branch_per_tool_plus_the_answer() {
        let s = schema(&tools());
        let branches = s["anyOf"].as_array().unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0]["properties"]["tool"]["const"], "read_file");
        assert_eq!(branches[1]["required"], json!(["answer"]));
    }

    #[test]
    fn a_generated_call_becomes_a_native_tool_call() {
        let m = into_message(r#"{"tool":"read_file","args":{"path":"a.rs"}}"#, &tools());
        assert_eq!(m["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(m["tool_calls"][0]["function"]["arguments"], json!({"path": "a.rs"}));
    }

    #[test]
    fn an_answer_and_loose_text_become_plain_content() {
        assert_eq!(into_message(r#"{"answer":"oi"}"#, &tools())["content"], "oi");
        assert_eq!(into_message("texto solto", &tools())["content"], "texto solto");
    }

    #[test]
    fn an_unknown_tool_is_not_turned_into_a_call() {
        let m = into_message(r#"{"tool":"rm_rf","args":{}}"#, &tools());
        assert!(m.get("tool_calls").is_none());
    }

    #[test]
    fn history_is_rewritten_without_native_tool_roles() {
        let history = vec![
            json!({"role": "system", "content": "S"}),
            json!({"role": "user", "content": "leia a.rs"}),
            json!({"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "read_file", "arguments": {"path": "a.rs"}}}]}),
            json!({"role": "tool", "tool_name": "read_file", "content": "fn main(){}"}),
        ];
        let out = plain_messages(&history, &tools());
        assert!(out[0]["content"].as_str().unwrap().contains("read_file(path)"));
        let said: Value = serde_json::from_str(out[2]["content"].as_str().unwrap()).unwrap();
        assert_eq!(said, json!({"tool": "read_file", "args": {"path": "a.rs"}}));
        assert_eq!(out[3]["role"], "user");
        assert!(out.iter().all(|m| m["role"] != "tool"));
    }
}

//! Chamadas de ferramenta que o modelo escreve como texto, e a normalização dos argumentos.

use serde_json::{json, Value};

// Fallback para modelos pequenos que escrevem a chamada como texto JSON,
// ex.: {"name": "read_file", "arguments": {"path": "a.txt"}}.
// Aceita qualquer nome: se a ferramenta não existir, o erro é devolvido ao modelo e ao usuário.
pub(super) fn calls_from_text(content: &str) -> Vec<Value> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with('{') {
                return None;
            }
            let v: Value = serde_json::from_str(line).ok()?;
            let name = v.get("name")?.as_str()?;
            let args = v.get("arguments")?.clone();
            Some(json!({"function": {"name": name, "arguments": args}}))
        })
        .collect()
}

// Alguns modelos devolvem `arguments` como objeto, outros como string JSON.
pub(super) fn normalize_args(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
        Some(v) => v.clone(),
        None => json!({}),
    }
}

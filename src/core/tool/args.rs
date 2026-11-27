//! Confere os argumentos de uma chamada contra o schema que a ferramenta declara, antes de rodá-la.
//!
//! Modelos pequenos inventam nomes de parâmetro (`arquivo` no lugar de `path`) e tipos errados. Sem essa
//! conferência, a ferramenta falha de um jeito que o modelo não entende. Com ela, a resposta diz exatamente
//! o que faltou, o que sobrou e como a chamada deveria ser, e o modelo corrige na próxima tentativa.
//! Não toca em modelo nem em rede: é uma verificação de estrutura, barata em tokens e em tempo.

use serde_json::Value;

// Confere `args` contra o schema (`{"properties": {...}, "required": [...]}`). `Err` traz a lista de problemas
// e o formato esperado, numa linha só.
pub fn check(schema: &Value, args: &Value) -> Result<(), String> {
    let props = schema.get("properties").and_then(Value::as_object);
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    // Sem parâmetros declarados não há contrato a conferir: a ferramenta recebe o que vier.
    let Some(props) = props.filter(|p| !p.is_empty()) else { return Ok(()) };
    let Some(given) = args.as_object() else {
        return Err(format!("os argumentos precisam ser um objeto JSON com: {}", expected(props, &required)));
    };

    let mut problems: Vec<String> = Vec::new();
    let missing: Vec<&str> = required.iter().copied().filter(|r| !given.contains_key(*r)).collect();
    if !missing.is_empty() {
        problems.push(format!("faltam {}", missing.join(", ")));
    }
    let unknown: Vec<&str> = given.keys().map(String::as_str).filter(|k| !props.contains_key(*k)).collect();
    if !unknown.is_empty() {
        problems.push(format!("parâmetros que não existem: {}", unknown.join(", ")));
    }
    for (name, value) in given {
        let Some(spec) = props.get(name) else { continue };
        if let Some(kind) = spec.get("type").and_then(Value::as_str)
            && !matches_type(kind, value)
        {
            problems.push(format!("{name} deveria ser {}", type_name(kind)));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("argumentos inválidos: {}. Use exatamente: {}", problems.join("; "), expected(props, &required)))
    }
}

// Os parâmetros aceitos, com o tipo e marcando os obrigatórios: `path (texto, obrigatório), args (lista)`.
fn expected(props: &serde_json::Map<String, Value>, required: &[&str]) -> String {
    props
        .iter()
        .map(|(name, spec)| {
            let kind = spec.get("type").and_then(Value::as_str).map(type_name).unwrap_or("qualquer valor");
            if required.contains(&name.as_str()) { format!("{name} ({kind}, obrigatório)") } else { format!("{name} ({kind})") }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn matches_type(kind: &str, value: &Value) -> bool {
    match kind {
        "string" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "boolean" => value.is_boolean(),
        // Modelos pequenos mandam número como texto ("4"). A ferramenta converte; aqui isso não é erro.
        "integer" => value.is_i64() || value.is_u64() || value.as_str().is_some_and(|s| s.trim().parse::<i64>().is_ok()),
        "number" => value.is_number() || value.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok()),
        _ => true,
    }
}

fn type_name(kind: &str) -> &'static str {
    match kind {
        "string" => "texto",
        "array" => "lista",
        "object" => "objeto",
        "boolean" => "verdadeiro ou falso",
        "integer" => "número inteiro",
        "number" => "número",
        _ => "um valor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "caminho"},
                "args": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["path"]
        })
    }

    #[test]
    fn a_correct_call_passes() {
        assert!(check(&schema(), &json!({"path": "a.sh", "args": ["."]})).is_ok());
        assert!(check(&schema(), &json!({"path": "a.sh"})).is_ok(), "args é opcional");
    }

    #[test]
    fn a_wrong_name_says_what_is_missing_and_what_exists() {
        let err = check(&schema(), &json!({"arquivo": "a.sh"})).unwrap_err();
        assert!(err.contains("faltam path"), "{err}");
        assert!(err.contains("não existem: arquivo"), "{err}");
        assert!(err.contains("path (texto, obrigatório)") && err.contains("args (lista)"), "{err}");
    }

    #[test]
    fn a_wrong_type_is_named() {
        let err = check(&schema(), &json!({"path": "a.sh", "args": "."})).unwrap_err();
        assert!(err.contains("args deveria ser lista"), "{err}");
    }

    #[test]
    fn a_number_written_as_text_is_accepted_for_integers() {
        let s = json!({"type": "object", "properties": {"start_line": {"type": "integer"}}});
        assert!(check(&s, &json!({"start_line": "4"})).is_ok());
        assert!(check(&s, &json!({"start_line": 4})).is_ok());
        assert!(check(&s, &json!({"start_line": "quatro"})).is_err());
    }

    #[test]
    fn arguments_that_are_not_an_object_are_refused() {
        let err = check(&schema(), &json!("a.sh")).unwrap_err();
        assert!(err.contains("objeto JSON"), "{err}");
    }

    #[test]
    fn a_tool_without_declared_properties_accepts_anything() {
        assert!(check(&json!({"type": "object"}), &json!({"qualquer": 1})).is_ok());
    }
}

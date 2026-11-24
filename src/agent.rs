//! Ciclo modelo → ferramentas → modelo. Não conhece o Ollama: recebe qualquer `ChatModel`.

use crate::tools::{self, Outcome, NAMES};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

pub const MAX_STEPS: usize = 8;

// Qualquer provedor capaz de completar uma conversa com ferramentas.
pub trait ChatModel {
    fn complete(&self, messages: &[Value], tools: &Value) -> Result<Value, String>;
}

impl ChatModel for crate::ollama::Client {
    fn complete(&self, messages: &[Value], tools: &Value) -> Result<Value, String> {
        self.chat(messages, tools)
    }
}

/// Uma chamada de ferramenta feita pelo modelo e o que aconteceu com ela.
pub struct Action {
    pub name: String,
    pub args: Value,
    pub outcome: Outcome,
}

/// Registro do turno. O texto do modelo é só uma parte; as ações são o que o programa executou.
#[derive(Default)]
pub struct TurnReport {
    pub text: String,
    pub actions: Vec<Action>,
    pub error: Option<String>,
}

impl TurnReport {
    /// Houve alteração de fato no disco neste turno.
    pub fn changed(&self) -> bool {
        self.actions.iter().any(|a| is_mutating(&a.name) && matches!(a.outcome, Outcome::Done(_)))
    }

    /// O modelo tentou alterar algo, com sucesso ou não.
    pub fn attempted_changes(&self) -> bool {
        self.actions.iter().any(|a| is_mutating(&a.name))
    }
}

fn is_mutating(name: &str) -> bool {
    matches!(name, "create_file" | "edit_file")
}

/// Roda o ciclo até o modelo responder sem chamar ferramentas.
/// Erros de ferramenta e de nome desconhecido voltam ao modelo como texto e ficam no relatório.
pub fn run_turn(
    model: &dyn ChatModel,
    tools: &Value,
    root: &Path,
    messages: &mut Vec<Value>,
    auto_yes: bool,
) -> Result<TurnReport, String> {
    let mut report = TurnReport::default();
    let mut last_call: Option<String> = None;
    let mut repeats = 0;
    let mut applied_edits = HashSet::new();

    for _ in 0..MAX_STEPS {
        let msg = model.complete(messages, tools)?;
        messages.push(msg.clone());

        let mut calls = msg.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        if calls.is_empty() {
            calls = calls_from_text(msg.get("content").and_then(Value::as_str).unwrap_or(""));
        }
        if calls.is_empty() {
            report.text = msg.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string();
            return Ok(report);
        }

        for call in calls {
            let func = call.get("function").cloned().unwrap_or(Value::Null);
            let name = func.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let args = normalize_args(func.get("arguments"));
            eprintln!("· {name} {args}");

            // Mesma chamada repetida: o modelo está preso, interrompe.
            let key = format!("{name}{args}");
            if last_call.as_deref() == Some(key.as_str()) {
                repeats += 1;
                if repeats >= 2 {
                    report.error = Some("o modelo repetiu a mesma chamada; interrompendo".into());
                    return Ok(report);
                }
            } else {
                repeats = 0;
            }
            last_call = Some(key.clone());

            let outcome = if !NAMES.contains(&name.as_str()) {
                Outcome::Error(format!("ferramenta desconhecida: {name}. Use uma destas: {}", NAMES.join(", ")))
            } else if name == "edit_file" && !applied_edits.insert(key.clone()) {
                // Edição idêntica já feita neste turno: não reaplica nem conta como ação nova.
                messages.push(json!({
                    "role": "tool",
                    "tool_name": name.clone(),
                    "content": "esta edição já foi aplicada neste turno. Responda ao usuário sem chamar mais ferramentas."
                }));
                continue;
            } else {
                tools::execute(root, &name, &args, auto_yes)
            };

            messages.push(json!({"role": "tool", "tool_name": name.clone(), "content": outcome.text()}));
            report.actions.push(Action { name, args, outcome });
        }
    }

    report.error = Some(format!("limite de {MAX_STEPS} passos atingido sem resposta final"));
    Ok(report)
}

/// Fallback para modelos pequenos que escrevem a chamada como texto JSON,
/// ex.: {"name": "read_file", "arguments": {"path": "a.txt"}}.
/// Aceita qualquer nome: se a ferramenta não existir, o erro é devolvido ao modelo e ao usuário.
fn calls_from_text(content: &str) -> Vec<Value> {
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

/// Alguns modelos devolvem `arguments` como objeto, outros como string JSON.
fn normalize_args(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
        Some(v) => v.clone(),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;

    /// Modelo falso com respostas roteirizadas. Não usa rede.
    struct FakeModel {
        replies: RefCell<VecDeque<Value>>,
    }

    impl ChatModel for FakeModel {
        fn complete(&self, _messages: &[Value], _tools: &Value) -> Result<Value, String> {
            self.replies.borrow_mut().pop_front().ok_or_else(|| "roteiro esgotado".to_string())
        }
    }

    fn fake(replies: Vec<Value>) -> FakeModel {
        FakeModel { replies: RefCell::new(replies.into()) }
    }

    fn text(s: &str) -> Value {
        json!({"role": "assistant", "content": s})
    }

    fn call(name: &str, args: Value) -> Value {
        json!({"role": "assistant", "content": "", "tool_calls": [{"function": {"name": name, "arguments": args}}]})
    }

    fn start() -> Vec<Value> {
        vec![json!({"role": "user", "content": "tarefa"})]
    }

    fn root(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-agent-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn create_then_answer_reports_change() {
        let r = root("create");
        let model = fake(vec![
            call("create_file", json!({"path": "fib.py", "content": "print(1)\n"})),
            text("Pronto."),
        ]);
        let report = run_turn(&model, &tools::schemas(), &r, &mut start(), true).unwrap();
        assert!(report.changed());
        assert_eq!(report.text, "Pronto.");
        assert_eq!(fs::read_to_string(r.join("fib.py")).unwrap(), "print(1)\n");
    }

    #[test]
    fn false_success_claim_is_visible_in_report() {
        let r = root("false-claim");
        let model = fake(vec![
            call("edit_file", json!({"path": "fib.py", "old": "a", "new": "b"})),
            text("Arquivo criado com sucesso."),
        ]);
        let mut msgs = start();
        let report = run_turn(&model, &tools::schemas(), &r, &mut msgs, true).unwrap();
        // O modelo afirmou sucesso, mas o relatório mostra que nada foi alterado.
        assert!(report.attempted_changes());
        assert!(!report.changed());
        assert!(matches!(report.actions[0].outcome, Outcome::Error(_)));
        assert!(!r.join("fib.py").exists());
        // E o erro foi devolvido ao modelo para ele poder se corrigir.
        assert!(msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.starts_with("ERRO:"))));
    }

    #[test]
    fn unknown_tool_in_text_is_reported_not_dropped() {
        let r = root("unknown");
        let model = fake(vec![
            text(r#"{"name": "make_file", "arguments": {"path": "a.py"}}"#),
            text("ok"),
        ]);
        let report = run_turn(&model, &tools::schemas(), &r, &mut start(), true).unwrap();
        assert_eq!(report.actions.len(), 1);
        match &report.actions[0].outcome {
            Outcome::Error(e) => assert!(e.contains("ferramenta desconhecida: make_file")),
            other => panic!("esperava erro, veio {other:?}"),
        }
        assert!(!report.changed());
    }

    #[test]
    fn step_limit_stops_the_turn() {
        // Cada resposta chama uma ferramenta diferente, então a repetição não dispara; só o limite de passos.
        let r = root("steps");
        let replies: Vec<Value> = (0..MAX_STEPS + 2)
            .map(|i| call("list_dir", json!({"path": format!("d{i}")})))
            .collect();
        let model = fake(replies);
        let report = run_turn(&model, &tools::schemas(), &r, &mut start(), true).unwrap();
        assert_eq!(report.actions.len(), MAX_STEPS);
        assert!(report.error.as_deref().is_some_and(|e| e.contains("limite")));
    }

    #[test]
    fn model_error_propagates() {
        let r = root("model-error");
        let model = fake(vec![]); // roteiro vazio: o modelo "falha" na primeira chamada
        let result = run_turn(&model, &tools::schemas(), &r, &mut start(), true);
        assert!(result.is_err());
    }

    #[test]
    fn repeated_call_stops_the_turn() {
        let r = root("repeat");
        let same = || call("list_dir", json!({"path": "."}));
        let model = fake(vec![same(), same(), same()]);
        let report = run_turn(&model, &tools::schemas(), &r, &mut start(), true).unwrap();
        assert!(report.error.is_some());
    }
}

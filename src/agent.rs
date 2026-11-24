//! Ciclo modelo → ferramentas → modelo. Não conhece o Ollama nem nomes de ferramentas:
//! recebe um `ChatModel`, um `Registry` e um `ContextManager`.

use crate::context::{self, ContextManager, Trim};
use crate::tools::{Approver, Outcome, Registry};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

pub const MAX_STEPS: usize = 8;

// Resposta de um provedor de modelo.
pub struct Reply {
    pub message: Value,
    // Tokens de entrada avaliados pelo provedor, quando informados. Usado para calibrar o contexto.
    pub prompt_tokens: Option<u64>,
}

// Qualquer provedor capaz de completar uma conversa com ferramentas.
pub trait ChatModel {
    fn complete(&self, messages: &[Value], tools: &Value) -> Result<Reply, String>;
}

// Uma chamada de ferramenta feita pelo modelo e o que aconteceu com ela.
pub struct Action {
    pub name: String,
    pub args: Value,
    pub outcome: Outcome,
    // Veio do registro: a ferramenta altera arquivos.
    pub mutates: bool,
}

// Registro do turno. O texto do modelo é só uma parte; as ações são o que o programa executou.
#[derive(Default)]
pub struct TurnReport {
    pub text: String,
    pub actions: Vec<Action>,
    pub error: Option<String>,
    // Descartes de histórico feitos durante o turno, para o terminal mostrar.
    pub trims: Vec<Trim>,
}

impl TurnReport {
    // Houve alteração de fato no disco neste turno.
    pub fn changed(&self) -> bool {
        self.actions.iter().any(|a| a.mutates && matches!(a.outcome, Outcome::Done(_)))
    }

    // O modelo tentou alterar algo, com sucesso ou não.
    pub fn attempted_changes(&self) -> bool {
        self.actions.iter().any(|a| a.mutates)
    }
}

// Roda o ciclo até o modelo responder sem chamar ferramentas.
// Erros de ferramenta e de nome desconhecido voltam ao modelo como texto e ficam no relatório.
pub fn run_turn(
    model: &dyn ChatModel,
    registry: &Registry,
    root: &Path,
    messages: &mut Vec<Value>,
    approver: &dyn Approver,
    ctx: &mut ContextManager,
) -> Result<TurnReport, String> {
    let tools = registry.schemas();
    let mut report = TurnReport::default();
    let mut last_call: Option<String> = None;
    let mut repeats = 0;
    // Alterações já executadas neste turno. Repetir a mesma alteração não a reaplica.
    let mut applied: HashSet<String> = HashSet::new();

    for _ in 0..MAX_STEPS {
        if let Some(trim) = ctx.fit(messages, &tools)? {
            report.trims.push(trim);
        }
        let sent_chars = context::size(messages) + context::value_size(&tools);
        let reply = model.complete(messages, &tools)?;
        if let Some(tokens) = reply.prompt_tokens {
            ctx.calibrate(sent_chars, tokens);
        }
        let msg = reply.message;
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

            let mutates = registry.mutates(&name);
            if mutates && !applied.insert(key.clone()) {
                // Alteração idêntica já feita neste turno: não reaplica nem conta como ação nova.
                messages.push(json!({
                    "role": "tool",
                    "tool_name": name.clone(),
                    "content": "esta alteração já foi feita neste turno. Responda ao usuário sem chamar mais ferramentas."
                }));
                continue;
            }

            let outcome = registry.execute(root, &name, &args, approver);
            messages.push(json!({"role": "tool", "tool_name": name.clone(), "content": outcome.text()}));
            report.actions.push(Action { name, args, outcome, mutates });
        }
    }

    report.error = Some(format!("limite de {MAX_STEPS} passos atingido sem resposta final"));
    Ok(report)
}

// Fallback para modelos pequenos que escrevem a chamada como texto JSON,
// ex.: {"name": "read_file", "arguments": {"path": "a.txt"}}.
// Aceita qualquer nome: se a ferramenta não existir, o erro é devolvido ao modelo e ao usuário.
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

// Alguns modelos devolvem `arguments` como objeto, outros como string JSON.
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
    use crate::tools::{AutoApprover, Tool};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;

    // Modelo falso com respostas roteirizadas. Não usa rede.
    struct FakeModel {
        replies: RefCell<VecDeque<Value>>,
    }

    impl ChatModel for FakeModel {
        fn complete(&self, _messages: &[Value], _tools: &Value) -> Result<Reply, String> {
            let message = self.replies.borrow_mut().pop_front().ok_or_else(|| "roteiro esgotado".to_string())?;
            Ok(Reply { message, prompt_tokens: None })
        }
    }

    fn fake(replies: Vec<Value>) -> FakeModel {
        FakeModel { replies: RefCell::new(replies.into()) }
    }

    fn ctx() -> ContextManager {
        ContextManager::new(8192, 1024)
    }

    struct Refuse;

    impl Approver for Refuse {
        fn approve(&self) -> bool {
            false
        }
    }

    // Ferramenta de teste: prova que uma ferramenta nova funciona sem editar o loop.
    struct Echo;

    impl Tool for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn schema(&self) -> Value {
            json!({"type": "function", "function": {"name": "echo", "description": "ecoa", "parameters": {"type": "object", "properties": {}}}})
        }

        fn run(&self, _root: &Path, args: &Value, _approver: &dyn Approver) -> Outcome {
            Outcome::Done(args.get("texto").and_then(Value::as_str).unwrap_or("").to_string())
        }
    }

    fn text(s: &str) -> Value {
        json!({"role": "assistant", "content": s})
    }

    fn call(name: &str, args: Value) -> Value {
        json!({"role": "assistant", "content": "", "tool_calls": [{"function": {"name": name, "arguments": args}}]})
    }

    fn start() -> Vec<Value> {
        vec![json!({"role": "system", "content": "sistema"}), json!({"role": "user", "content": "tarefa"})]
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
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx()).unwrap();
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
        let report = run_turn(&model, &Registry::default_set(), &r, &mut msgs, &AutoApprover, &mut ctx()).unwrap();
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
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx()).unwrap();
        // Ferramenta desconhecida vira ação com erro, visível no relatório e devolvida ao modelo.
        assert_eq!(report.actions.len(), 1);
        assert!(matches!(&report.actions[0].outcome, Outcome::Error(e) if e.contains("ferramenta desconhecida: make_file")));
        assert!(!report.attempted_changes());
        assert!(!report.changed());
    }

    #[test]
    fn refused_change_is_not_reported_as_done() {
        let r = root("refuse");
        fs::write(r.join("a.py"), "x = 1\n").unwrap();
        let model = fake(vec![
            call("edit_file", json!({"path": "a.py", "old": "x = 1", "new": "x = 2"})),
            text("Pronto."),
        ]);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &Refuse, &mut ctx()).unwrap();
        assert!(matches!(report.actions[0].outcome, Outcome::Refused(_)));
        assert!(!report.changed());
        assert!(report.attempted_changes());
        assert_eq!(fs::read_to_string(r.join("a.py")).unwrap(), "x = 1\n");
    }

    #[test]
    fn new_tool_works_without_editing_the_loop() {
        let r = root("echo");
        let registry = Registry::new(vec![Box::new(Echo)]);
        let model = fake(vec![call("echo", json!({"texto": "oi"})), text("ok")]);
        let report = run_turn(&model, &registry, &r, &mut start(), &AutoApprover, &mut ctx()).unwrap();
        assert_eq!(report.actions[0].outcome, Outcome::Done("oi".into()));
        assert!(!report.attempted_changes());
    }

    #[test]
    fn same_change_is_not_applied_twice_in_one_turn() {
        let r = root("dedupe");
        let create = || call("create_file", json!({"path": "a.txt", "content": "x"}));
        let model = fake(vec![create(), create(), text("ok")]);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx()).unwrap();
        assert_eq!(report.actions.len(), 1);
        assert!(report.changed());
    }

    #[test]
    fn step_limit_stops_the_turn() {
        // Cada resposta chama uma ferramenta diferente, então a repetição não dispara; só o limite de passos.
        let r = root("steps");
        let replies: Vec<Value> = (0..MAX_STEPS + 2)
            .map(|i| call("list_dir", json!({"path": format!("d{i}")})))
            .collect();
        let model = fake(replies);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx()).unwrap();
        assert_eq!(report.actions.len(), MAX_STEPS);
        assert!(report.error.as_deref().is_some_and(|e| e.contains("limite")));
    }

    #[test]
    fn model_error_propagates() {
        let r = root("model-error");
        let model = fake(vec![]); // roteiro vazio: o modelo "falha" na primeira chamada
        let result = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx());
        assert!(result.is_err());
    }

    #[test]
    fn repeated_call_stops_the_turn() {
        let r = root("repeat");
        let same = || call("list_dir", json!({"path": "."}));
        let model = fake(vec![same(), same(), same()]);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx()).unwrap();
        assert!(report.error.is_some());
    }

    #[test]
    fn long_history_is_trimmed_before_calling_the_model() {
        // Histórico antigo grande, orçamento pequeno: o loop deve descartar turnos antes de chamar o modelo.
        let r = root("trim");
        let mut msgs = vec![json!({"role": "system", "content": "sistema"})];
        for i in 0..6 {
            msgs.push(json!({"role": "user", "content": format!("antiga {i}")}));
            msgs.push(json!({"role": "assistant", "content": "y".repeat(2_000)}));
        }
        msgs.push(json!({"role": "user", "content": "atual"}));
        let mut small = ContextManager::new(1_200, 0); // ~3.600 caracteres de orçamento
        let model = fake(vec![text("ok")]);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut msgs, &AutoApprover, &mut small).unwrap();
        assert!(!report.trims.is_empty(), "deveria ter descartado turnos");
        assert_eq!(msgs[0]["content"], "sistema");
        assert!(msgs.iter().any(|m| m["content"] == "atual"));
    }

    #[test]
    fn oversized_current_turn_stops_with_error() {
        let r = root("oversized");
        let mut msgs = vec![json!({"role": "system", "content": "sistema"}), json!({"role": "user", "content": "z".repeat(10_000)})];
        let mut small = ContextManager::new(1_200, 0);
        let model = fake(vec![text("ok")]);
        let result = run_turn(&model, &Registry::default_set(), &r, &mut msgs, &AutoApprover, &mut small);
        assert!(result.is_err());
    }

    #[test]
    fn calibration_uses_prompt_tokens_from_provider() {
        struct Counted;
        impl ChatModel for Counted {
            fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
                Ok(Reply { message: json!({"role": "assistant", "content": "ok"}), prompt_tokens: Some(500) })
            }
        }
        let r = root("calib");
        let mut c = ctx();
        run_turn(&Counted, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut c).unwrap();
        assert_ne!(c.chars_per_token(), 3.0, "a calibração deveria ter mudado a estimativa");
    }
}

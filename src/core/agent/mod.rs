//! Ciclo modelo → ferramentas → modelo. Não conhece o Ollama nem nomes de ferramentas:
//! recebe um `ChatModel`, um `Registry`, um `ContextManager` e um `Progress`.

use crate::core::context::{self, ContextManager, Trim};
use crate::core::tool::{Approver, Outcome, Registry};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const MAX_STEPS: usize = 8;
// Quantas falhas seguidas da mesma ferramenta encerram o turno.
pub const MAX_CONSECUTIVE_FAILURES: usize = 2;

// Palavras que indicam que o texto final afirma uma alteração. Heurística: gera aviso, não bloqueia.
// Só formas que afirmam algo já feito. Infinitivos ("posso criar", "quer remover") são ofertas.
const CHANGE_WORDS: [&str; 10] = ["criei", "criou", "criado", "criada", "alterei", "alterou", "editei", "editou", "alterado", "editado"];
const DELETE_WORDS: [&str; 8] = ["removi", "removeu", "removido", "removida", "apaguei", "apagou", "excluí", "excluiu"];

// Resposta de um provedor de modelo.
pub struct Reply {
    pub message: Value,
    // Tokens de entrada avaliados pelo provedor, quando informados. Usado para calibrar o contexto.
    pub prompt_tokens: Option<u64>,
    // Tokens gerados pelo modelo, quando informados.
    pub output_tokens: Option<u64>,
}

// Um pedaço de texto que o modelo acabou de gerar: raciocínio ou resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    Thinking(&'a str),
    Content(&'a str),
    // Nada chegou ainda: o provedor só acorda para o sink conferir se o turno foi cancelado. É o que
    // deixa o Esc valer enquanto o modelo carrega ou processa o prompt, antes do primeiro texto.
    Idle,
}

// Qualquer provedor capaz de completar uma conversa com ferramentas.
pub trait ChatModel {
    fn complete(&self, messages: &[Value], tools: &Value) -> Result<Reply, String>;

    // Igual a `complete`, mas entrega o texto à medida que o modelo o gera. Se `on_chunk` devolve
    // `false`, o provedor para de gerar e devolve o que já tem. Quem não faz streaming usa este padrão.
    fn complete_streaming(
        &self,
        messages: &[Value],
        tools: &Value,
        _on_chunk: &mut dyn FnMut(Chunk) -> bool,
    ) -> Result<Reply, String> {
        self.complete(messages, tools)
    }
}

// Recebe sinais de progresso do loop, para a interface mostrar que o modelo está trabalhando.
pub trait Progress {
    fn model_waiting(&self, waiting: bool);

    // Uma ferramenta foi chamada, com os argumentos enviados pelo modelo.
    fn tool_call(&self, _name: &str, _args: &Value) {}

    // A ferramenta terminou (ou foi recusada): a interface mostra o resultado na hora.
    fn tool_done(&self, _name: &str, _args: &Value, _outcome: &Outcome) {}

    // O usuário pediu para interromper. O loop confere entre uma chamada ao modelo e outra:
    // uma requisição em andamento não é abortada, mas o turno para assim que ela volta.
    fn cancelled(&self) -> bool {
        false
    }

    // Uma chamada ao modelo terminou: tokens de entrada e de saída, quando o provedor informou.
    fn model_tokens(&self, _prompt: Option<u64>, _output: Option<u64>) {}

    // O modelo raciocinou antes de responder: o texto do raciocínio, quando ele o enviou.
    fn model_reasoning(&self, _text: &str) {}

    // Texto que o modelo acabou de gerar, ainda no meio da chamada.
    fn model_chunk(&self, _chunk: Chunk) {}
}

// Progresso que não mostra nada (útil em testes e em agentes sem terminal).
pub struct Silent;

impl Progress for Silent {
    fn model_waiting(&self, _waiting: bool) {}
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

    // Algum arquivo foi removido de fato neste turno.
    pub fn deleted(&self) -> bool {
        self.actions.iter().any(|a| a.name == "delete_file" && matches!(a.outcome, Outcome::Done(_)))
    }

    // O modelo tentou alterar algo, com sucesso ou não.
    pub fn attempted_changes(&self) -> bool {
        self.actions.iter().any(|a| a.mutates)
    }

    // Avisos sobre o texto final: ele afirma algo que as ações não confirmam.
    pub fn warnings(&self) -> Vec<String> {
        let text = self.text.to_lowercase();
        let claims_delete = DELETE_WORDS.iter().any(|w| text.contains(w));
        let claims_change = claims_delete || CHANGE_WORDS.iter().any(|w| text.contains(w));
        let mut out = Vec::new();
        if !self.changed() && (self.attempted_changes() || claims_change) {
            out.push("nenhuma alteração foi aplicada neste turno.".to_string());
        }
        if claims_delete && !self.deleted() {
            out.push("o texto diz que algo foi removido, mas nenhum arquivo foi removido.".to_string());
        }
        out
    }
}

// Limites de um turno. Os padrões são as constantes acima.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_steps: usize,
    pub max_consecutive_failures: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_steps: MAX_STEPS, max_consecutive_failures: MAX_CONSECUTIVE_FAILURES }
    }
}

// Roda o ciclo com os limites padrão. Ver `run_turn_with`.
pub fn run_turn(
    model: &dyn ChatModel,
    registry: &Registry,
    root: &Path,
    messages: &mut Vec<Value>,
    approver: &dyn Approver,
    ctx: &mut ContextManager,
    progress: &dyn Progress,
) -> Result<TurnReport, String> {
    run_turn_with(model, registry, root, messages, approver, ctx, progress, Limits::default())
}

// Motivo registrado quando o usuário interrompe o turno.
pub const INTERRUPTED: &str = "interrompido pelo usuário";

// Fim de turno quando o modelo repete o mesmo trecho sem parar.
pub const LOOPING: &str =
    "o modelo entrou em laço e repetiu o mesmo trecho; interrompi a geração. Reformule o pedido ou desligue o raciocínio com /think.";

// Uma linha com pelo menos isto de letras e dígitos, repetida este número de vezes, é laço. Contar só
// letras e dígitos deixa de fora separadores como `| --- | --- |` e `=======`, que se repetem à toa.
const LOOP_MIN_ALNUM: usize = 20;
// No raciocínio, repetir uma linha longa 3 vezes já é sinal de laço. Na resposta final é mais comum
// repetir de verdade (linhas de código, itens parecidos), então pede mais.
const LOOP_REPEATS_THINKING: usize = 3;
const LOOP_REPEATS_CONTENT: usize = 5;
// O fim do texto, com este tamanho, já ter aparecido este número de vezes também é laço: é um bloco se repetindo.
const LOOP_TAIL_CHARS: usize = 160;
const LOOP_TAIL_REPEATS: usize = 3;
// De quantos em quantos bytes novos o texto é examinado.
const LOOP_CHECK_EVERY: usize = 200;

// O modelo está repetindo o mesmo trecho sem parar? Olha linhas iguais e blocos que se repetem no fim.
//
// As repetições não precisam ser seguidas: no laço real que medimos, o modelo chega à resposta,
// duvida e recomeça a conta, repetindo as mesmas linhas espalhadas pelo texto (a maior repetição
// seguida era de 1 linha). Exigir linhas consecutivas deixaria esse laço passar.
pub fn is_looping(text: &str, line_repeats: usize) -> bool {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for line in text.lines() {
        let line = line.trim().to_lowercase();
        if line.chars().filter(|c| c.is_alphanumeric()).count() < LOOP_MIN_ALNUM {
            continue;
        }
        let count = seen.entry(line).or_insert(0);
        *count += 1;
        if *count >= line_repeats {
            return true;
        }
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() >= LOOP_TAIL_CHARS * LOOP_TAIL_REPEATS {
        let tail: String = chars[chars.len() - LOOP_TAIL_CHARS..].iter().collect();
        return text.matches(&tail).count() >= LOOP_TAIL_REPEATS;
    }
    false
}

// Acompanha o texto que chega do modelo e avisa quando ele entra em laço.
#[derive(Default)]
struct LoopGuard {
    thinking: String,
    content: String,
    unchecked: usize,
    looping: bool,
}

impl LoopGuard {
    // Devolve `true` se o texto até aqui já é um laço.
    fn feed(&mut self, chunk: Chunk) -> bool {
        let text = match chunk {
            Chunk::Thinking(t) => {
                self.thinking.push_str(t);
                t
            }
            Chunk::Content(t) => {
                self.content.push_str(t);
                t
            }
            Chunk::Idle => return self.looping,
        };
        self.unchecked += text.len();
        if self.unchecked >= LOOP_CHECK_EVERY {
            self.unchecked = 0;
            self.looping = is_looping(&self.thinking, LOOP_REPEATS_THINKING) || is_looping(&self.content, LOOP_REPEATS_CONTENT);
        }
        self.looping
    }
}

// Roda o ciclo até o modelo responder sem chamar ferramentas.
// Erros de ferramenta e de nome desconhecido voltam ao modelo como texto e ficam no relatório.
#[allow(clippy::too_many_arguments)]
pub fn run_turn_with(
    model: &dyn ChatModel,
    registry: &Registry,
    root: &Path,
    messages: &mut Vec<Value>,
    approver: &dyn Approver,
    ctx: &mut ContextManager,
    progress: &dyn Progress,
    limits: Limits,
) -> Result<TurnReport, String> {
    let tools = registry.schemas();
    let mut report = TurnReport::default();
    let mut last_call: Option<String> = None;
    let mut repeats = 0;
    let mut failing: Option<(String, usize)> = None;
    // Alterações já executadas neste turno. Repetir a mesma alteração não a reaplica.
    let mut applied: HashSet<String> = HashSet::new();

    for _ in 0..limits.max_steps {
        if progress.cancelled() {
            report.error = Some(INTERRUPTED.into());
            return Ok(report);
        }
        if let Some(trim) = ctx.fit(messages, &tools)? {
            report.trims.push(trim);
        }
        let sent_chars = context::size(messages) + context::value_size(&tools);
        progress.model_waiting(true);
        let mut guard = LoopGuard::default();
        let reply = model.complete_streaming(messages, &tools, &mut |chunk| {
            progress.model_chunk(chunk);
            !progress.cancelled() && !guard.feed(chunk)
        });
        progress.model_waiting(false);
        let reply = reply?;
        progress.model_tokens(reply.prompt_tokens, reply.output_tokens);
        if let Some(text) = reply.message.get("thinking").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
            progress.model_reasoning(text);
        }
        // Laço: o que veio é descartado, mas o raciocínio parcial já foi mostrado acima.
        if guard.looping {
            report.error = Some(LOOPING.into());
            return Ok(report);
        }
        // Interrompido durante a espera: descarta a resposta, sem deixar chamadas sem resultado no histórico.
        if progress.cancelled() {
            report.error = Some(INTERRUPTED.into());
            return Ok(report);
        }
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
            progress.tool_call(&name, &args);

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
            progress.tool_done(&name, &args, &outcome);

            // Falhas seguidas da mesma ferramenta, mesmo com argumentos diferentes, encerram o turno.
            if matches!(outcome, Outcome::Error(_)) {
                failing = match failing {
                    Some((n, count)) if n == name => Some((n, count + 1)),
                    _ => Some((name.clone(), 1)),
                };
            } else {
                failing = None;
            }

            messages.push(json!({"role": "tool", "tool_name": name.clone(), "content": outcome.text()}));
            // Recusa do usuário encerra o turno: insistir na mesma alteração só o irrita e gasta passos.
            if matches!(outcome, Outcome::Refused(_)) {
                report.actions.push(Action { name: name.clone(), args, outcome, mutates });
                report.error = Some("alteração recusada pelo usuário; turno encerrado sem aplicar mais nada".into());
                return Ok(report);
            }
            report.actions.push(Action { name: name.clone(), args, outcome, mutates });

            if let Some((_, count)) = &failing
                && *count >= limits.max_consecutive_failures
            {
                report.error = Some(format!(
                    "a ferramenta {name} falhou {count} vezes seguidas; interrompendo para não repetir o erro"
                ));
                return Ok(report);
            }
        }
    }

    report.error = Some(format!("limite de {} passos atingido sem resposta final", limits.max_steps));
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
    use crate::core::tool::{AutoApprover, Preview, Tool};
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
            Ok(Reply { message, prompt_tokens: None, output_tokens: None })
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
        fn approve(&self, _preview: &Preview) -> bool {
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

    fn run(model: &dyn ChatModel, registry: &Registry, r: &Path, msgs: &mut Vec<Value>, approver: &dyn Approver) -> Result<TurnReport, String> {
        run_turn(model, registry, r, msgs, approver, &mut ctx(), &Silent)
    }

    #[test]
    fn create_then_answer_reports_change() {
        let r = root("create");
        let model = fake(vec![
            call("create_file", json!({"path": "fib.py", "content": "print(1)\n"})),
            text("Pronto."),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.changed());
        assert_eq!(report.text, "Pronto.");
        assert!(report.warnings().is_empty());
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
        let report = run(&model, &Registry::default_set(), &r, &mut msgs, &AutoApprover).unwrap();
        assert!(report.attempted_changes());
        assert!(!report.changed());
        assert!(matches!(report.actions[0].outcome, Outcome::Error(_)));
        assert!(!r.join("fib.py").exists());
        assert!(msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.starts_with("ERRO:"))));
        assert!(report.warnings().iter().any(|w| w.contains("nenhuma alteração")));
    }

    #[test]
    fn claim_of_removal_without_any_action_is_flagged() {
        // Caso real: o modelo disse "foi removido" sem chamar nenhuma ferramenta.
        let r = root("claim-no-action");
        let model = fake(vec![text("O arquivo mean.cpp foi removido.")]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.actions.is_empty());
        let w = report.warnings();
        assert!(w.iter().any(|x| x.contains("nenhuma alteração")), "{w:?}");
        assert!(w.iter().any(|x| x.contains("nenhum arquivo foi removido")), "{w:?}");
    }

    #[test]
    fn removal_claim_after_only_an_edit_is_flagged() {
        // Caso real: o arquivo foi só esvaziado por edição, mas o texto diz "removido".
        let r = root("claim-edit");
        fs::write(r.join("a.md"), "# nota\n").unwrap();
        let model = fake(vec![
            call("edit_file", json!({"path": "a.md", "old": "# nota", "new": ""})),
            text("Arquivo a.md removido."),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.changed());
        assert!(report.warnings().iter().any(|x| x.contains("nenhum arquivo foi removido")));
    }

    #[test]
    fn delete_then_claim_has_no_warning() {
        let r = root("delete-ok");
        fs::write(r.join("a.md"), "# nota\n").unwrap();
        let model = fake(vec![
            call("delete_file", json!({"path": "a.md"})),
            text("Arquivo a.md removido."),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.deleted());
        assert!(report.warnings().is_empty(), "{:?}", report.warnings());
        assert!(!r.join("a.md").exists());
    }

    #[test]
    fn question_answered_in_text_has_no_warning() {
        let r = root("question");
        let model = fake(vec![text("No diretório há calc.py e fib.py.")]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.warnings().is_empty());
    }

    #[test]
    fn unknown_tool_in_text_is_reported_not_dropped() {
        let r = root("unknown");
        let model = fake(vec![
            text(r#"{"name": "make_file", "arguments": {"path": "a.py"}}"#),
            text("ok"),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert_eq!(report.actions.len(), 1);
        assert!(matches!(&report.actions[0].outcome, Outcome::Error(e) if e.contains("ferramenta desconhecida: make_file")));
        assert!(!report.attempted_changes());
        assert!(!report.changed());
    }

    #[test]
    fn refusal_ends_the_turn_without_more_attempts() {
        let r = root("refuse-stops");
        fs::write(r.join("a.py"), "x = 1\n").unwrap();
        let model = fake(vec![
            call("edit_file", json!({"path": "a.py", "old": "x = 1", "new": "x = 2"})),
            call("create_file", json!({"path": "b.py", "content": "y"})),
            text("insisti"),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &Refuse).unwrap();
        assert_eq!(report.actions.len(), 1, "nenhuma nova tentativa depois da recusa");
        assert!(report.error.as_deref().unwrap_or("").contains("recusada"));
        assert!(!r.join("b.py").exists());
    }

    #[test]
    fn refused_change_is_not_reported_as_done() {
        let r = root("refuse");
        fs::write(r.join("a.py"), "x = 1\n").unwrap();
        let model = fake(vec![
            call("edit_file", json!({"path": "a.py", "old": "x = 1", "new": "x = 2"})),
            text("Pronto."),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &Refuse).unwrap();
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
        let report = run(&model, &registry, &r, &mut start(), &AutoApprover).unwrap();
        assert_eq!(report.actions[0].outcome, Outcome::Done("oi".into()));
        assert!(!report.attempted_changes());
    }

    #[test]
    fn same_change_is_not_applied_twice_in_one_turn() {
        let r = root("dedupe");
        let create = || call("create_file", json!({"path": "a.txt", "content": "x"}));
        let model = fake(vec![create(), create(), text("ok")]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert_eq!(report.actions.len(), 1);
        assert!(report.changed());
    }

    #[test]
    fn consecutive_failures_of_same_tool_stop_the_turn() {
        // Argumentos diferentes a cada vez: a proteção antiga (chamada idêntica) não pegaria isso.
        let r = root("failures");
        let model = fake(vec![
            call("edit_file", json!({"path": "x.md", "old": "a", "new": "b"})),
            call("edit_file", json!({"path": "x.md", "old": "c", "new": "d"})),
            call("edit_file", json!({"path": "x.md", "old": "e", "new": "f"})),
            text("nunca chega aqui"),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert_eq!(report.actions.len(), MAX_CONSECUTIVE_FAILURES);
        assert!(report.error.as_deref().is_some_and(|e| e.contains("falhou 2 vezes")));
    }

    #[test]
    fn failures_of_different_tools_do_not_count_together() {
        let r = root("mixed-failures");
        let model = fake(vec![
            call("edit_file", json!({"path": "x.md", "old": "a", "new": "b"})),
            call("read_file", json!({"path": "nao.md"})),
            text("ok"),
        ]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.error.is_none());
    }

    #[test]
    fn step_limit_stops_the_turn() {
        // Chamadas bem-sucedidas e distintas (o argumento extra muda a chave): só o limite de passos para o turno.
        let r = root("steps");
        let replies: Vec<Value> = (0..MAX_STEPS + 2)
            .map(|i| call("list_dir", json!({"path": ".", "n": i})))
            .collect();
        let model = fake(replies);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert_eq!(report.actions.len(), MAX_STEPS);
        assert!(report.error.as_deref().is_some_and(|e| e.contains("limite")));
    }

    #[test]
    fn model_error_propagates() {
        let r = root("model-error");
        let model = fake(vec![]); // roteiro vazio: o modelo "falha" na primeira chamada
        let result = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover);
        assert!(result.is_err());
    }

    #[test]
    fn repeated_call_stops_the_turn() {
        let r = root("repeat");
        let same = || call("list_dir", json!({"path": "."}));
        let model = fake(vec![same(), same(), same()]);
        let report = run(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover).unwrap();
        assert!(report.error.is_some());
    }

    #[test]
    fn long_history_is_trimmed_before_calling_the_model() {
        let r = root("trim");
        let mut msgs = vec![json!({"role": "system", "content": "sistema"})];
        for i in 0..6 {
            msgs.push(json!({"role": "user", "content": format!("antiga {i}")}));
            msgs.push(json!({"role": "assistant", "content": "y".repeat(2_000)}));
        }
        msgs.push(json!({"role": "user", "content": "atual"}));
        let mut small = ContextManager::new(1_200, 0);
        let model = fake(vec![text("ok")]);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut msgs, &AutoApprover, &mut small, &Silent).unwrap();
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
        let result = run_turn(&model, &Registry::default_set(), &r, &mut msgs, &AutoApprover, &mut small, &Silent);
        assert!(result.is_err());
    }

    #[test]
    fn calibration_uses_prompt_tokens_from_provider() {
        struct Counted;
        impl ChatModel for Counted {
            fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
                Ok(Reply { message: json!({"role": "assistant", "content": "ok"}), prompt_tokens: Some(500), output_tokens: None })
            }
        }
        let r = root("calib");
        let mut c = ctx();
        run_turn(&Counted, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut c, &Silent).unwrap();
        assert_ne!(c.chars_per_token(), 3.0, "a calibração deveria ter mudado a estimativa");
    }

    #[test]
    fn repetition_is_detected_but_ordinary_text_is_not() {
        let think = LOOP_REPEATS_THINKING;
        assert!(!is_looping("", think));
        assert!(!is_looping("Taxa por máquina por dia = 800 / 144.\nTotal de máquina-dias = 12 * 12 = 144.\nLogo, 16,2 dias.", think));
        let same_line = "Taxa: 144 máquina-dias produzem 800 apostilas.\n".repeat(3);
        assert!(is_looping(&same_line, think), "a mesma linha longa três vezes");
        assert!(!is_looping(&"Taxa: 144 máquina-dias produzem 800 apostilas.\n".repeat(2), think), "repetir o enunciado uma vez é normal");
        let block = "Então calculamos de novo o total de dias, passo a passo, sem pressa alguma, e comparamos com o resultado anterior para conferir. ";
        assert!(is_looping(&block.repeat(6), think), "um bloco inteiro se repetindo, mesmo sem quebra de linha");
        assert!(!is_looping(&"curto\n".repeat(50), think), "linhas curtas repetidas não contam");
    }

    #[test]
    fn separators_and_repeated_code_in_an_answer_are_not_a_loop() {
        let content = LOOP_REPEATS_CONTENT;
        let table = "| item | valor |\n| --- | --- | --- | --- | --- |\n| a | 1 |\n| --- | --- | --- | --- | --- |\n| b | 2 |\n| --- | --- | --- | --- | --- |\n";
        assert!(!is_looping(table, LOOP_REPEATS_THINKING), "separador sem letras não conta, nem no raciocínio");
        assert!(!is_looping(&"echo \"==============================\"\n".repeat(10), content), "régua de sinais não conta");
        let prose = "Cada máquina imprime a mesma quantidade de apostilas por dia.\n";
        assert!(!is_looping(&prose.repeat(3), content), "na resposta final, 3 linhas iguais são aceitáveis");
        assert!(is_looping(&prose.repeat(5), content), "mas 5 já é repetição");
    }

    #[test]
    fn the_real_loop_is_scattered_so_consecutive_repeats_would_miss_it() {
        // Reproduz o formato do laço medido: as mesmas linhas voltam entre outras, nunca em sequência.
        let round = "12 máquinas imprimem 800 em 12 dias.\nVamos conferir de outro jeito, com calma.\nPortanto, a resposta é 16.2 dias.\nMas talvez o problema espere um número inteiro de dias?\n";
        let mut text = String::new();
        for n in 0..3 {
            text.push_str(&round.replace("jeito", &format!("jeito {n}")));
        }
        assert!(is_looping(&text, LOOP_REPEATS_THINKING), "linhas repetidas espalhadas são laço");
    }

    #[test]
    fn a_cancel_while_waiting_for_the_first_chunk_stops_the_call() {
        use std::cell::Cell;
        // Um modelo que ainda está carregando: só acorda o sink, nunca entrega texto.
        struct Loading(Cell<usize>);
        impl ChatModel for Loading {
            fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
                Err("só o streaming é usado".into())
            }
            fn complete_streaming(&self, _m: &[Value], _t: &Value, on_chunk: &mut dyn FnMut(Chunk) -> bool) -> Result<Reply, String> {
                for _ in 0..1000 {
                    self.0.set(self.0.get() + 1);
                    if !on_chunk(Chunk::Idle) {
                        break;
                    }
                }
                Ok(Reply { message: json!({"role": "assistant", "content": ""}), prompt_tokens: None, output_tokens: None })
            }
        }
        // O usuário aperta Esc na terceira conferência.
        struct EscOnThird<'a>(&'a Cell<usize>);
        impl Progress for EscOnThird<'_> {
            fn model_waiting(&self, _waiting: bool) {}
            fn cancelled(&self) -> bool {
                self.0.get() >= 3
            }
        }
        let r = root("idle");
        let model = Loading(Cell::new(0));
        let progress = EscOnThird(&model.0);
        let report = run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx(), &progress).unwrap();
        assert_eq!(report.error.as_deref(), Some(INTERRUPTED));
        assert!(model.0.get() <= 4, "parou logo depois do Esc, sem esperar texto: {} conferências", model.0.get());
    }

    #[test]
    fn a_looping_model_is_cut_off_early_with_a_clear_reason() {
        use std::cell::Cell;
        struct Stuck(Cell<usize>);
        impl ChatModel for Stuck {
            fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
                Err("só o streaming é usado".into())
            }
            fn complete_streaming(&self, _m: &[Value], _t: &Value, on_chunk: &mut dyn FnMut(Chunk) -> bool) -> Result<Reply, String> {
                let mut said = String::new();
                for _ in 0..500 {
                    let piece = "Taxa: 144 máquina-dias produzem 800 apostilas.\n";
                    self.0.set(self.0.get() + 1);
                    said.push_str(piece);
                    if !on_chunk(Chunk::Thinking(piece)) {
                        break;
                    }
                }
                Ok(Reply { message: json!({"role": "assistant", "content": "", "thinking": said}), prompt_tokens: None, output_tokens: None })
            }
        }
        let r = root("looping");
        let stuck = Stuck(Cell::new(0));
        let report = run_turn(&stuck, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx(), &Silent).unwrap();
        assert_eq!(report.error.as_deref(), Some(LOOPING));
        assert!(stuck.0.get() < 20, "parou cedo: {} pedaços de 500", stuck.0.get());
    }

    #[test]
    fn the_reasoning_of_each_call_reaches_the_progress() {
        use std::cell::RefCell;
        struct Thinker;
        impl ChatModel for Thinker {
            fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
                let message = json!({"role": "assistant", "content": "ok", "thinking": "300 / 8 = 37,5; 720 / 37,5 = 19,2"});
                Ok(Reply { message, prompt_tokens: None, output_tokens: None })
            }
        }
        struct Recorder(RefCell<Vec<String>>);
        impl Progress for Recorder {
            fn model_waiting(&self, _waiting: bool) {}
            fn model_reasoning(&self, text: &str) {
                self.0.borrow_mut().push(text.to_string());
            }
        }
        let r = root("reasoning");
        let rec = Recorder(RefCell::new(Vec::new()));
        run_turn(&Thinker, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx(), &rec).unwrap();
        assert_eq!(rec.0.borrow().as_slice(), ["300 / 8 = 37,5; 720 / 37,5 = 19,2"]);
    }

    #[test]
    fn each_tool_reports_its_outcome_as_it_finishes() {
        use std::cell::RefCell;
        struct Log(RefCell<Vec<String>>);
        impl Progress for Log {
            fn model_waiting(&self, _waiting: bool) {}
            fn tool_done(&self, name: &str, _args: &Value, outcome: &Outcome) {
                self.0.borrow_mut().push(format!("{name}:{}", outcome.label()));
            }
        }
        let r = root("tool-done");
        let registry = Registry::new(vec![Box::new(Echo)]);
        let model = fake(vec![call("echo", json!({"texto": "oi"})), text("pronto")]);
        let p = Log(RefCell::new(Vec::new()));
        run_turn(&model, &registry, &r, &mut start(), &AutoApprover, &mut ctx(), &p).unwrap();
        assert_eq!(*p.0.borrow(), vec!["echo:ok".to_string()]);
    }

    #[test]
    fn progress_is_signalled_around_each_model_call() {
        use std::cell::Cell;
        struct Counter(Cell<u32>, Cell<bool>);
        impl Progress for Counter {
            fn model_waiting(&self, waiting: bool) {
                // Cada chamada deve abrir e fechar a espera, nessa ordem.
                assert_ne!(self.1.get(), waiting, "sinal repetido");
                self.1.set(waiting);
                if waiting {
                    self.0.set(self.0.get() + 1);
                }
            }
        }
        let r = root("progress");
        let model = fake(vec![text("oi")]);
        let p = Counter(Cell::new(0), Cell::new(false));
        run_turn(&model, &Registry::default_set(), &r, &mut start(), &AutoApprover, &mut ctx(), &p).unwrap();
        assert_eq!(p.0.get(), 1);
        assert!(!p.1.get(), "a espera deve terminar");
    }

    // Progresso que pede interrupção logo após a primeira espera pelo modelo.
    struct CancelAfterFirstReply(std::cell::Cell<usize>);

    impl Progress for CancelAfterFirstReply {
        fn model_waiting(&self, waiting: bool) {
            if !waiting {
                self.0.set(self.0.get() + 1);
            }
        }
        fn cancelled(&self) -> bool {
            self.0.get() >= 1
        }
    }

    #[test]
    fn cancelling_stops_the_turn_without_dangling_tool_calls() {
        let dir = std::env::temp_dir().join(format!("bitclaw-cancel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let model = fake(vec![json!({"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "echo", "arguments": {"text": "oi"}}}
        ]})]);
        let registry = Registry::new(vec![Box::new(Echo)]);
        let mut messages = vec![json!({"role": "user", "content": "x"})];
        let report = run_turn_with(
            &model, &registry, &dir, &mut messages, &AutoApprover, &mut ctx(),
            &CancelAfterFirstReply(std::cell::Cell::new(0)), Limits::default(),
        )
        .unwrap();
        assert_eq!(report.error.as_deref(), Some(INTERRUPTED));
        assert!(report.actions.is_empty());
        assert_eq!(messages.len(), 1, "a resposta descartada não entra no histórico");
    }
    #[test]
    fn offers_are_not_claims() {
        // Oferta com "remover" não é afirmação de remoção.
        let report = TurnReport { text: "Posso apenas criar, ler, editar ou remover arquivos. Deseja criar um?".into(), ..Default::default() };
        assert!(report.warnings().is_empty(), "{:?}", report.warnings());
        // Afirmação no passado sem a ação real continua sendo avisada.
        let claim = TurnReport { text: "Pronto, removi o arquivo.".into(), ..Default::default() };
        assert_eq!(claim.warnings(), vec!["nenhuma alteração foi aplicada neste turno.", "o texto diz que algo foi removido, mas nenhum arquivo foi removido."]);
    }
}

use super::guard::*;
use super::*;
use crate::core::context::ContextManager;
use crate::core::tool::{Approver, Outcome, Registry, ToolContext, ToolSpec};
use serde_json::{json, Value};
use std::path::Path;
use crate::core::tool::{AutoApprover, Preview, Tool};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;

// Modelo falso com respostas roteirizadas. Não usa rede.
struct FakeModel {
    replies: RefCell<VecDeque<Value>>,
}

impl ChatModel for FakeModel {
    fn complete(&self, _messages: &[Value], _tools: &[ToolSpec]) -> Result<Reply, String> {
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

    fn spec(&self) -> ToolSpec {
        ToolSpec::new("echo", "ecoa", json!({"type": "object", "properties": {}}))
    }

    fn run(&self, _ctx: &ToolContext, args: &Value) -> Outcome {
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert!(report.changed());
    assert_eq!(report.text, "Pronto.");
    assert!(crate::toolkit::claims::warnings(&report).is_empty());
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    assert!(report.attempted_changes());
    assert!(!report.changed());
    assert!(matches!(report.actions[0].outcome, Outcome::Error(_)));
    assert!(!r.join("fib.py").exists());
    assert!(msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.starts_with("ERRO:"))));
    assert!(crate::toolkit::claims::warnings(&report).iter().any(|w| w.contains("nenhuma alteração")));
}

#[test]
fn claim_of_removal_without_any_action_is_flagged() {
    // Caso real: o modelo disse "foi removido" sem chamar nenhuma ferramenta.
    let r = root("claim-no-action");
    let model = fake(vec![text("O arquivo mean.cpp foi removido.")]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert!(report.actions.is_empty());
    let w = crate::toolkit::claims::warnings(&report);
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert!(report.changed());
    assert!(crate::toolkit::claims::warnings(&report).iter().any(|x| x.contains("nenhum arquivo foi removido")));
}

#[test]
fn delete_then_claim_has_no_warning() {
    let r = root("delete-ok");
    fs::write(r.join("a.md"), "# nota\n").unwrap();
    let model = fake(vec![
        call("delete_file", json!({"path": "a.md"})),
        text("Arquivo a.md removido."),
    ]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert!(crate::toolkit::claims::deleted(&report));
    assert!(crate::toolkit::claims::warnings(&report).is_empty(), "{:?}", crate::toolkit::claims::warnings(&report));
    assert!(!r.join("a.md").exists());
}

#[test]
fn question_answered_in_text_has_no_warning() {
    let r = root("question");
    let model = fake(vec![text("No diretório há calc.py e fib.py.")]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert!(crate::toolkit::claims::warnings(&report).is_empty());
}

#[test]
fn unknown_tool_in_text_is_reported_not_dropped() {
    let r = root("unknown");
    let model = fake(vec![
        text(r#"{"name": "make_file", "arguments": {"path": "a.py"}}"#),
        text("ok"),
    ]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &Refuse).unwrap();
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &Refuse).unwrap();
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
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
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), MAX_STEPS);
    assert!(report.error.as_deref().is_some_and(|e| e.contains("limite")));
}

#[test]
fn model_error_propagates() {
    let r = root("model-error");
    let model = fake(vec![]); // roteiro vazio: o modelo "falha" na primeira chamada
    let result = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover);
    assert!(result.is_err());
}

#[test]
fn repeated_call_stops_the_turn() {
    let r = root("repeat");
    let same = || call("list_dir", json!({"path": "."}));
    let model = fake(vec![same(), same(), same()]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
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
    let report = run_turn(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover, &mut small, &Silent).unwrap();
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
    let result = run_turn(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover, &mut small, &Silent);
    assert!(result.is_err());
}

#[test]
fn calibration_uses_prompt_tokens_from_provider() {
    struct Counted;
    impl ChatModel for Counted {
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
            Ok(Reply { message: json!({"role": "assistant", "content": "ok"}), prompt_tokens: Some(500), output_tokens: None })
        }
    }
    let r = root("calib");
    let mut c = ctx();
    run_turn(&Counted, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover, &mut c, &Silent).unwrap();
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
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
            Err("só o streaming é usado".into())
        }
        fn complete_streaming(&self, _m: &[Value], _t: &[ToolSpec], on_chunk: &mut dyn FnMut(Chunk) -> bool) -> Result<Reply, String> {
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
    let report = run_turn(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover, &mut ctx(), &progress).unwrap();
    assert_eq!(report.error.as_deref(), Some(INTERRUPTED));
    assert!(model.0.get() <= 4, "parou logo depois do Esc, sem esperar texto: {} conferências", model.0.get());
}

#[test]
fn a_looping_model_is_cut_off_early_with_a_clear_reason() {
    use std::cell::Cell;
    struct Stuck(Cell<usize>);
    impl ChatModel for Stuck {
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
            Err("só o streaming é usado".into())
        }
        fn complete_streaming(&self, _m: &[Value], _t: &[ToolSpec], on_chunk: &mut dyn FnMut(Chunk) -> bool) -> Result<Reply, String> {
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
    let report = run_turn(&stuck, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover, &mut ctx(), &Silent).unwrap();
    assert_eq!(report.error.as_deref(), Some(LOOPING));
    assert!(stuck.0.get() < 20, "parou cedo: {} pedaços de 500", stuck.0.get());
}

#[test]
fn the_reasoning_of_each_call_reaches_the_progress() {
    use std::cell::RefCell;
    struct Thinker;
    impl ChatModel for Thinker {
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
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
    run_turn(&Thinker, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover, &mut ctx(), &rec).unwrap();
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
    run_turn(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover, &mut ctx(), &p).unwrap();
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
    assert!(crate::toolkit::claims::warnings(&report).is_empty(), "{:?}", crate::toolkit::claims::warnings(&report));
    // Afirmação no passado sem a ação real continua sendo avisada.
    let claim = TurnReport { text: "Pronto, removi o arquivo.".into(), ..Default::default() };
    assert_eq!(crate::toolkit::claims::warnings(&claim), vec!["nenhuma alteração foi aplicada neste turno.", "o texto diz que algo foi removido, mas nenhum arquivo foi removido."]);
}

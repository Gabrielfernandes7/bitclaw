use super::guard::*;
use super::*;
use crate::core::context::{self, ContextManager};
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

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: false }
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
    // A última resposta é o pedido final sem ferramentas; vazia, o turno termina com o erro de repetição.
    let model = fake(vec![same(), same(), same(), text("")]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.error.as_deref(), Some("o modelo repetiu a mesma chamada; interrompendo"));
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
    let mut small = ContextManager::new(1_800, 0);
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
    let mut small = ContextManager::new(1_800, 0);
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
        &CancelAfterFirstReply(std::cell::Cell::new(0)), Limits::default(), None,
    )
    .unwrap();
    assert_eq!(report.error.as_deref(), Some(INTERRUPTED));
    assert!(report.actions.is_empty());
    assert_eq!(messages.len(), 1, "a resposta descartada não entra no histórico");
}
#[test]
fn a_tool_output_written_by_the_model_is_flagged_when_no_tool_ran() {
    let faked = TurnReport { text: "<｜tool▁output｜begin｜>\narquivo removido: a.md\n<｜tool｜output｜end｜>\nPronto.".into(), ..Default::default() };
    let w = crate::toolkit::claims::warnings(&faked);
    assert!(w.iter().any(|x| x.contains("saída de ferramenta escrita pelo modelo")), "{w:?}");
    let plain = TurnReport { text: "Nada a fazer.".into(), ..Default::default() };
    assert!(crate::toolkit::claims::warnings(&plain).is_empty());
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

#[test]
fn tool_usage_groups_calls_by_tool_heaviest_first() {
    use super::report::{Action, TurnReport};
    use crate::core::tool::Outcome;
    let act = |name: &str, text: &str| Action {
        name: name.into(),
        args: serde_json::Value::Null,
        outcome: Outcome::Done(text.into()),
        mutates: false,
    };
    let report = TurnReport {
        actions: vec![act("list_dir", "ab"), act("read_file", "12345"), act("read_file", "123")],
        ..Default::default()
    };
    assert_eq!(report.tool_usage(), vec![("read_file".into(), 2, 8), ("list_dir".into(), 1, 2)]);
    assert_eq!(report.tool_usage_line(), "[ferramentas] read_file 2× 8 car., list_dir 1× 2 car.");
    assert_eq!(TurnReport::default().tool_usage_line(), "");
}

#[test]
fn a_run_claimed_without_a_successful_run_is_flagged() {
    use crate::core::tool::Outcome;
    let failed = Action { name: "run_script".into(), args: Value::Null, outcome: Outcome::Error("saiu com código 2".into()), mutates: true };
    let claim = TurnReport { text: "O script foi executado. A saída é 240.".into(), actions: vec![failed], ..Default::default() };
    let w = crate::toolkit::claims::warnings(&claim);
    assert!(w.iter().any(|x| x.contains("nenhuma execução deu certo")), "{w:?}");
    let ok = Action { name: "run_script".into(), args: Value::Null, outcome: Outcome::Done("código de saída: 0\n2880".into()), mutates: true };
    let real = TurnReport { text: "Executei o script: a saída é 2880.".into(), actions: vec![ok], ..Default::default() };
    assert!(crate::toolkit::claims::warnings(&real).iter().all(|x| !x.contains("execução")));
}

#[test]
fn tagged_text_call_from_the_model_runs_as_a_real_tool() {
    let r = root("tagged-call");
    std::fs::write(r.join("a.txt"), "oi").unwrap();
    let model = fake(vec![
        text(r#"list_dir{path:<|"|>.<|"|>}"#),
        text("Há um arquivo: a.txt."),
    ]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 1);
    assert!(matches!(report.actions[0].outcome, Outcome::Done(_)));
    assert_eq!(report.text, "Há um arquivo: a.txt.");
}

#[test]
fn native_tool_calls_take_precedence_over_text() {
    let r = root("native-precedence");
    let mut msg = call("list_dir", json!({"path": "."}));
    msg["content"] = json!(r#"read_file{path:<|"|>x<|"|>}"#);
    let model = fake(vec![msg, text("ok")]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 1);
    assert_eq!(report.actions[0].name, "list_dir");
}

fn tagged(name: &str, path: &str) -> Value {
    text(&format!("{name}{{path:<|\"|>{path}<|\"|>}}"))
}

#[test]
fn text_call_is_rewritten_as_a_structured_call_in_the_history() {
    // O resultado da ferramenta precisa responder a uma chamada do modelo no histórico; texto solto não serve.
    let r = root("rewrite-history");
    let model = fake(vec![tagged("list_dir", "."), text("Pronto.")]);
    let mut msgs = start();
    run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    let assistant = &msgs[2];
    assert_eq!(assistant["tool_calls"][0]["function"]["name"], "list_dir");
    assert_eq!(assistant["tool_calls"][0]["function"]["arguments"], json!({"path": "."}));
    assert_eq!(assistant["content"], "", "o texto da chamada não fica no histórico");
    assert_eq!(msgs[3]["role"], "tool");
}

#[test]
fn prose_around_a_text_call_stays_in_the_history() {
    let r = root("rewrite-prose");
    let model = fake(vec![text("Vou listar.\nlist_dir{path:<|\"|>.<|\"|>}"), text("Pronto.")]);
    let mut msgs = start();
    run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    assert_eq!(msgs[2]["content"], "Vou listar.");
}

#[test]
fn glued_text_calls_all_run() {
    let r = root("glued");
    fs::write(r.join("a.txt"), "um").unwrap();
    fs::write(r.join("b.txt"), "dois").unwrap();
    let both = text(r#"read_file{path:<|"|>a.txt<|"|>}read_file{path:<|"|>b.txt<|"|>}"#);
    let model = fake(vec![both, text("um e dois")]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 2);
    assert_eq!(report.text, "um e dois");
}

#[test]
fn repeated_read_is_not_run_again_and_the_model_gets_a_hint() {
    let r = root("repeat-read");
    let model = fake(vec![call("list_dir", json!({"path": "."})), call("list_dir", json!({"path": "."})), text("Está vazio.")]);
    let mut msgs = start();
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 1, "a repetição não conta como ação nova");
    assert_eq!(report.text, "Está vazio.");
    assert!(report.error.is_none());
    let hints = msgs.iter().filter(|m| m["content"].as_str().is_some_and(|c| c.contains("já fez esta chamada"))).count();
    assert_eq!(hints, 1);
}

#[test]
fn repeated_read_between_other_calls_is_also_caught() {
    let r = root("repeat-read-gap");
    fs::create_dir(r.join("src")).unwrap();
    let model = fake(vec![
        call("list_dir", json!({"path": "src"})),
        call("list_dir", json!({"path": "."})),
        call("list_dir", json!({"path": "src"})),
        text("ok"),
    ]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 2);
    assert_eq!(report.text, "ok");
}

#[test]
fn a_model_that_keeps_repeating_reads_is_asked_for_an_answer_without_tools() {
    let r = root("repeat-finalize");
    let same = || call("list_dir", json!({"path": "."}));
    let model = fake(vec![same(), same(), same(), text("O projeto está vazio.")]);
    let mut msgs = start();
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    assert_eq!(report.text, "O projeto está vazio.");
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.actions.len(), 1);
    assert_eq!(msgs.last().unwrap()["content"], "O projeto está vazio.");
    assert!(
        !msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("sem chamar mais ferramentas"))),
        "o pedido final não fica no histórico"
    );
}

// Modelo falso que guarda o que recebeu em cada chamada.
struct Recording {
    replies: RefCell<VecDeque<Value>>,
    seen: RefCell<Vec<Vec<Value>>>,
}

impl ChatModel for Recording {
    fn complete(&self, messages: &[Value], _tools: &[ToolSpec]) -> Result<Reply, String> {
        self.seen.borrow_mut().push(messages.to_vec());
        let message = self.replies.borrow_mut().pop_front().ok_or_else(|| "roteiro esgotado".to_string())?;
        Ok(Reply { message, prompt_tokens: None, output_tokens: None })
    }
}

// Leitura mínima e idempotente: esquema curto, para o pedido final pesar mais que o que a falta de esquemas libera.
struct Peek;

impl Tool for Peek {
    fn name(&self) -> &'static str {
        "peek"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new("peek", "lê", json!({"type": "object", "properties": {}}))
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: true }
    }

    fn run(&self, _ctx: &ToolContext, _args: &Value) -> Outcome {
        Outcome::Done("ok".into())
    }
}

fn stuck_on_reads(filler: &str) -> (Recording, Vec<Value>) {
    let same = || call("peek", json!({}));
    let model = Recording {
        replies: RefCell::new(vec![same(), same(), same(), text("O projeto está vazio.")].into()),
        seen: RefCell::new(Vec::new()),
    };
    let msgs = vec![
        json!({"role": "system", "content": "sistema"}),
        json!({"role": "user", "content": "pedido antigo"}),
        json!({"role": "assistant", "content": filler}),
        json!({"role": "user", "content": "tarefa"}),
    ];
    (model, msgs)
}

#[test]
fn the_forced_final_request_is_fitted_to_the_context_window() {
    let registry = Registry::new(vec![Box::new(Peek)]);
    let tools_json = crate::core::tool::specs_value(&registry.specs());
    let budget = ctx().budget_chars(&tools_json);
    let r = root("forced-final-fit");

    // Mede o histórico enviado no 3º passo (o último ajustado antes do pedido final) sem enchimento.
    let (probe, mut msgs) = stuck_on_reads("x");
    run(&probe, &registry, &r, &mut msgs, &AutoApprover).unwrap();
    let third = context::size(&probe.seen.borrow()[2]);

    // Enchimento que deixa esse 3º passo quase no limite: o que for acrescentado depois estoura o orçamento.
    let filler = "x".repeat(budget - third - 10);
    let (model, mut msgs) = stuck_on_reads(&filler);
    let report = run(&model, &registry, &r, &mut msgs, &AutoApprover).unwrap();

    assert_eq!(report.text, "O projeto está vazio.");
    let seen = model.seen.borrow();
    assert!(seen[2].iter().any(|m| m["content"] == filler.as_str()), "o enchimento ainda cabia no 3º passo");
    let last = seen.last().unwrap();
    assert!(
        context::size(last) + context::value_size(&json!([])) <= ctx().budget_chars(&json!([])),
        "o pedido final cabe na janela: {} caracteres",
        context::size(last)
    );
    assert!(!last.iter().any(|m| m["content"] == filler.as_str()), "o histórico antigo saiu para dar lugar ao pedido");
    assert!(last.iter().any(|m| m["content"] == "tarefa"), "o pedido original continua");
    assert!(last.last().unwrap()["content"].as_str().unwrap().contains("sem chamar mais ferramentas"));
}

#[test]
fn an_error_from_the_forced_final_call_is_not_reported_as_a_repeated_call() {
    let r = root("forced-final-error");
    let same = || call("list_dir", json!({"path": "."}));
    // O roteiro acaba antes do pedido final: o provedor falha nessa chamada.
    let model = fake(vec![same(), same(), same()]);
    let result = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover);
    assert_eq!(result.err().as_deref(), Some("roteiro esgotado"));
}

#[test]
fn a_looping_forced_final_is_cut_off_early() {
    use std::cell::Cell;
    struct StuckAtTheEnd {
        calls: Cell<usize>,
        pieces: Cell<usize>,
    }
    impl ChatModel for StuckAtTheEnd {
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
            Err("só o streaming é usado".into())
        }
        fn complete_streaming(&self, _m: &[Value], tools: &[ToolSpec], on_chunk: &mut dyn FnMut(Chunk) -> bool) -> Result<Reply, String> {
            self.calls.set(self.calls.get() + 1);
            if !tools.is_empty() {
                return Ok(Reply { message: call("list_dir", json!({"path": "."})), prompt_tokens: None, output_tokens: None });
            }
            let mut said = String::new();
            for _ in 0..500 {
                let piece = "Taxa: 144 máquina-dias produzem 800 apostilas.\n";
                self.pieces.set(self.pieces.get() + 1);
                said.push_str(piece);
                if !on_chunk(Chunk::Content(piece)) {
                    break;
                }
            }
            Ok(Reply { message: text(&said), prompt_tokens: None, output_tokens: None })
        }
    }
    let r = root("forced-final-loop");
    let model = StuckAtTheEnd { calls: Cell::new(0), pieces: Cell::new(0) };
    let mut msgs = start();
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    assert_eq!(report.error.as_deref(), Some(LOOPING));
    assert!(report.text.is_empty());
    assert!(model.pieces.get() < 20, "parou cedo: {} pedaços de 500", model.pieces.get());
    assert!(!msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("Taxa:"))), "o laço não entra no histórico");
}

#[test]
fn reading_again_after_a_change_runs_again() {
    let r = root("read-after-write");
    let model = fake(vec![
        call("list_dir", json!({"path": "."})),
        call("create_file", json!({"path": "novo.txt", "content": "x"})),
        call("list_dir", json!({"path": "."})),
        text("Criei novo.txt."),
    ]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 3);
    assert!(report.error.is_none());
}

#[test]
fn non_idempotent_tools_are_not_deduplicated() {
    let r = root("echo-twice");
    let reg = Registry::new(vec![Box::new(Echo)]);
    let model = fake(vec![call("echo", json!({"texto": "a"})), call("echo", json!({"texto": "a"})), text("fim")]);
    let report = run(&model, &reg, &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.actions.len(), 2, "uma ferramenta que não se declara idempotente roda sempre");
}

#[test]
fn an_empty_reply_is_asked_again_and_leaves_no_trace_in_the_history() {
    let r = root("empty-retry");
    let model = fake(vec![text(""), call("create_file", json!({"path": "a.txt", "content": "x"})), text("Criei a.txt.")]);
    let mut msgs = start();
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover).unwrap();
    assert!(report.changed());
    assert_eq!(report.text, "Criei a.txt.");
    assert!(
        !msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("veio vazia")) || (m["role"] == "assistant" && m["content"] == "" && m.get("tool_calls").is_none())),
        "a resposta vazia e o empurrão não entram no histórico: {msgs:?}"
    );
}

#[test]
fn the_retry_nudge_is_sent_only_with_the_retry() {
    use std::cell::Cell;
    struct Spy(FakeModel, Cell<usize>, RefCell<Vec<bool>>);
    impl ChatModel for Spy {
        fn complete(&self, messages: &[Value], tools: &[ToolSpec]) -> Result<Reply, String> {
            let nudged = messages.last().is_some_and(|m| m["content"].as_str().is_some_and(|c| c.contains("veio vazia")));
            self.2.borrow_mut().push(nudged);
            self.1.set(self.1.get() + 1);
            self.0.complete(messages, tools)
        }
    }
    let r = root("empty-nudge");
    let spy = Spy(fake(vec![text(""), text("Olá.")]), Cell::new(0), RefCell::new(vec![]));
    let report = run(&spy, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert_eq!(report.text, "Olá.");
    assert_eq!(*spy.2.borrow(), vec![false, true]);
}

#[test]
fn a_model_that_stays_silent_ends_with_an_empty_report_after_two_retries() {
    let r = root("empty-forever");
    let model = fake(vec![text(""), text(""), text(""), text("nunca chega")]);
    let report = run(&model, &crate::toolkit::default_registry(), &r, &mut start(), &AutoApprover).unwrap();
    assert!(report.text.is_empty());
    assert!(report.actions.is_empty());
}

// ---- Fases, teto de saída e verificação automática ----

fn limits(phased: bool, max_tool_output: usize) -> Limits {
    Limits { phased, max_tool_output, ..Limits::default() }
}

#[test]
fn a_destructive_call_before_looking_is_refused_when_phased() {
    let r = root("phased");
    fs::write(r.join("a.txt"), "x").unwrap();
    let model = fake(vec![call("delete_file", json!({"path": "a.txt"})), text("ok")]);
    let mut msgs = start();
    let report = run_turn_with(
        &model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover, &mut ctx(), &Silent, limits(true, 0), None,
    )
    .unwrap();
    assert!(r.join("a.txt").exists(), "o arquivo não pode ter sido removido");
    assert!(report.actions.is_empty());
    assert!(msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("ainda não está disponível"))));
}

#[test]
fn clip_marks_what_was_left_out_and_zero_means_no_cap() {
    let long = "a".repeat(50);
    assert_eq!(super::turn::clip(long.clone(), 0), long);
    let cut = super::turn::clip(long, 10);
    assert!(cut.starts_with("aaaaaaaaaa\n[… 40 caracteres omitidos"), "{cut}");
}

struct Once(RefCell<bool>);

impl Verifier for Once {
    fn verify(&self, _root: &Path, _actions: &[Action]) -> Option<String> {
        let first = !*self.0.borrow();
        *self.0.borrow_mut() = true;
        first.then(|| "error: não compila".to_string())
    }
}

#[test]
fn a_failed_verification_goes_back_to_the_model_once() {
    let r = root("verify");
    let model = fake(vec![
        call("create_file", json!({"path": "a.txt", "content": "x"})),
        text("Pronto."),
        text("Corrigido."),
    ]);
    let mut msgs = start();
    let report = run_turn_with(
        &model, &crate::toolkit::default_registry(), &r, &mut msgs, &AutoApprover, &mut ctx(), &Silent, Limits::default(),
        Some(&Once(RefCell::new(false))),
    )
    .unwrap();
    assert_eq!(report.text, "Corrigido.");
    assert!(msgs.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("verificação automática"))));
}

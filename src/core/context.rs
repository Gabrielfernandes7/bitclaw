//! Gerenciador de contexto: mantém o histórico dentro do orçamento da janela do modelo.
//! Descarta turnos antigos inteiros. Nunca descarta o prompt do sistema nem o turno atual.
//!
//! Tamanho medido em caracteres do JSON de cada mensagem. A estimativa de caracteres por token
//! começa conservadora e é calibrada com `prompt_eval_count`, que o Ollama devolve a cada chamada.

use serde_json::Value;
use std::collections::VecDeque;

const DEFAULT_CHARS_PER_TOKEN: f64 = 3.0;
const MIN_CHARS_PER_TOKEN: f64 = 1.0;
const MAX_CHARS_PER_TOKEN: f64 = 6.0;
// Resultados de ferramenta menores que isso não valem a poda: o marcador quase não os encolheria.
const MIN_PRUNE_CHARS: usize = 200;
// Começo do texto que substitui um resultado podado. Também evita podar duas vezes o mesmo.
const PRUNED_MARK: &str = "[resultado antigo de ";

// Registro de um ajuste feito antes de uma chamada ao modelo.
#[derive(Debug, Clone, PartialEq)]
pub struct Trim {
    // Turnos antigos removidos.
    pub turns: usize,
    // Resultados antigos de ferramenta trocados por um marcador curto (antes de remover turnos).
    pub pruned: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub budget: usize,
}

impl Trim {
    // Uma linha para o terminal dizer o que foi tirado do histórico.
    pub fn summary(&self) -> String {
        let sizes = format!("caracteres {} → {} (orçamento {})", self.chars_before, self.chars_after, self.budget);
        match (self.turns, self.pruned) {
            (0, p) => format!("[contexto] {p} resultado(s) antigo(s) de ferramenta resumido(s); {sizes}"),
            (t, 0) => format!("[contexto] descartados {t} turno(s) antigo(s); {sizes}"),
            (t, p) => format!("[contexto] {p} resultado(s) resumido(s) e {t} turno(s) antigo(s) descartado(s); {sizes}"),
        }
    }
}

// Ocupação atual da janela de contexto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub tokens: usize,
    pub window: usize,
    pub percent: f64,
}

pub struct ContextManager {
    num_ctx: usize,
    reserve_output_tokens: usize,
    chars_per_token: f64,
}

impl ContextManager {
    // `num_ctx` é a janela total em tokens. `reserve_output_tokens` fica livre para a resposta do modelo.
    pub fn new(num_ctx: usize, reserve_output_tokens: usize) -> Self {
        Self { num_ctx, reserve_output_tokens, chars_per_token: DEFAULT_CHARS_PER_TOKEN }
    }

    // Troca a janela total. Precisa sobrar espaço além do que fica reservado para a resposta.
    pub fn set_num_ctx(&mut self, num_ctx: usize) -> Result<(), String> {
        if num_ctx <= self.reserve_output_tokens {
            return Err(format!(
                "o contexto precisa ser maior que os {} tokens reservados para a resposta",
                self.reserve_output_tokens
            ));
        }
        self.num_ctx = num_ctx;
        Ok(())
    }

    #[cfg(test)]
    pub fn chars_per_token(&self) -> f64 {
        self.chars_per_token
    }

    // Caracteres disponíveis para mensagens, descontado o que é fixo (esquemas das ferramentas).
    pub fn budget_chars(&self, tools: &Value) -> usize {
        let window_tokens = self.num_ctx.saturating_sub(self.reserve_output_tokens);
        let window_chars = (window_tokens as f64 * self.chars_per_token) as usize;
        window_chars.saturating_sub(value_size(tools))
    }

    // Quanto da janela está ocupado, em tokens estimados e em percentual.
    pub fn usage(&self, messages: &[Value], tools: &Value) -> Usage {
        let chars = size(messages) + value_size(tools);
        let tokens = (chars as f64 / self.chars_per_token).ceil() as usize;
        Usage { tokens, window: self.num_ctx, percent: tokens as f64 * 100.0 / self.num_ctx as f64 }
    }

    // Ajusta a estimativa com o número real de tokens de entrada devolvido pelo Ollama.
    // `sent_chars` deve ser o tamanho medido pela mesma função `size` sobre o que foi enviado.
    pub fn calibrate(&mut self, sent_chars: usize, prompt_tokens: u64) {
        if prompt_tokens == 0 {
            return;
        }
        let ratio = sent_chars as f64 / prompt_tokens as f64;
        self.chars_per_token = ratio.clamp(MIN_CHARS_PER_TOKEN, MAX_CHARS_PER_TOKEN);
    }

    // Garante que o histórico cabe no orçamento: primeiro troca resultados antigos de ferramenta por um
    // marcador curto e, se ainda não bastar, descarta turnos antigos inteiros.
    // Devolve `None` se nada foi descartado. Erra se o turno atual sozinho não cabe.
    pub fn fit(&self, messages: &mut Vec<Value>, tools: &Value) -> Result<Option<Trim>, String> {
        self.fit_reserving(messages, tools, 0)
    }

    // Igual a `fit`, mas deixa `reserve` caracteres livres no orçamento para uma mensagem que ainda será
    // acrescentada. Ajustar depois de acrescentá-la faria dela o turno atual e descartaria o pedido original.
    pub fn fit_reserving(&self, messages: &mut Vec<Value>, tools: &Value, reserve: usize) -> Result<Option<Trim>, String> {
        let budget = self.budget_chars(tools).saturating_sub(reserve);
        let Some(mut current) = messages.iter().rposition(|m| is_role(m, "user")) else {
            return Ok(None);
        };
        let head = usize::from(messages.first().is_some_and(|m| is_role(m, "system")));

        let protected = size(&messages[..head]) + size(&messages[current..]);
        if protected > budget {
            return Err(format!(
                "o turno atual ocupa {protected} caracteres e o orçamento de contexto é {budget}. \
                 Reduza a tarefa ou o tamanho das leituras."
            ));
        }

        let before = size(messages.as_slice());
        if before <= budget {
            return Ok(None);
        }

        let mut total = before;
        // Primeiro encolhe o que é mais barato de perder: resultados de ferramenta de turnos antigos.
        let pruned = prune_old_results(messages, head, current, &mut total, budget);
        if total <= budget {
            return Ok(Some(Trim { turns: 0, pruned, chars_before: before, chars_after: total, budget }));
        }
        let mut turns = 0;
        while total > budget {
            // Um turno vai de uma mensagem de usuário até a próxima (ou até o turno atual).
            // Assim chamadas de ferramenta e seus resultados saem sempre juntos.
            if current <= head {
                return Err("não há histórico para descartar e o orçamento ainda não é suficiente".into());
            }
            let end = messages[head + 1..current]
                .iter()
                .position(|m| is_role(m, "user"))
                .map_or(current, |p| head + 1 + p);
            let removed = size(&messages[head..end]);
            messages.drain(head..end);
            total -= removed;
            current -= end - head;
            turns += 1;
        }

        Ok(Some(Trim { turns, pruned, chars_before: before, chars_after: total, budget }))
    }
}

// A ferramenta e o caminho de uma chamada, para saber a que resultado ela pertence.
#[derive(Clone)]
struct Target {
    name: String,
    path: Option<String>,
}

fn target_of(call: &Value) -> Target {
    let func = call.get("function").unwrap_or(&Value::Null);
    let name = func.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let args = match func.get("arguments") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => Value::Null,
    };
    Target { name, path: args.get("path").and_then(Value::as_str).map(str::to_string) }
}

// Para cada mensagem de resultado (`role: tool`), a chamada que a originou. Os resultados vêm na ordem
// das chamadas da mensagem do assistente que os precede.
fn tool_targets(messages: &[Value]) -> Vec<Option<Target>> {
    let mut out = vec![None; messages.len()];
    let mut pending: VecDeque<Target> = VecDeque::new();
    for (i, m) in messages.iter().enumerate() {
        if is_role(m, "assistant") {
            pending = m
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| calls.iter().map(target_of).collect())
                .unwrap_or_default();
        } else if is_role(m, "tool") {
            out[i] = pending.pop_front();
        }
    }
    out
}

// Troca resultados de ferramenta anteriores a `current` por um marcador, do mais antigo ao mais novo,
// até caber no orçamento. Resultados já superados (uma chamada posterior mexeu no mesmo caminho: leu de
// novo, editou ou removeu) vão primeiro, porque o modelo já tem a versão nova. Devolve quantos podou.
fn prune_old_results(messages: &mut [Value], head: usize, current: usize, total: &mut usize, budget: usize) -> usize {
    let targets = tool_targets(messages);
    let candidate = |i: usize, m: &Value| {
        is_role(m, "tool")
            && m.get("content").and_then(Value::as_str).is_some_and(|c| c.chars().count() > MIN_PRUNE_CHARS && !c.starts_with(PRUNED_MARK))
            && targets[i].is_some()
            && i < current
    };
    let superseded = |i: usize| {
        let Some(Target { path: Some(path), .. }) = &targets[i] else { return false };
        targets[i + 1..].iter().flatten().any(|t| t.path.as_ref() == Some(path))
    };
    let mut order: Vec<usize> = (head..current).filter(|&i| candidate(i, &messages[i])).collect();
    // Estável: dentro de cada grupo, o mais antigo primeiro.
    order.sort_by_key(|&i| !superseded(i));

    let mut pruned = 0;
    for i in order {
        if *total <= budget {
            break;
        }
        let Some(target) = &targets[i] else { continue };
        let old = messages[i].get("content").and_then(Value::as_str).unwrap_or("").chars().count();
        let what = match &target.path {
            Some(p) => format!("{} {p}", target.name),
            None => target.name.clone(),
        };
        let before = value_size(&messages[i]);
        messages[i]["content"] = Value::String(format!("{PRUNED_MARK}{what} omitido ({old} car.); chame de novo se precisar]"));
        *total = *total - before + value_size(&messages[i]);
        pruned += 1;
    }
    pruned
}

// Tamanho de um valor JSON (mensagem ou esquemas), em caracteres.
pub fn value_size(value: &Value) -> usize {
    value.to_string().chars().count()
}

// Tamanho de uma lista de mensagens, em caracteres.
pub fn size(messages: &[Value]) -> usize {
    messages.iter().map(value_size).sum()
}

// Apaga todo o histórico, mantendo só o prompt do sistema. A calibração é do modelo, não da conversa, e fica.
pub fn clear(messages: &mut Vec<Value>) {
    let keep = usize::from(messages.first().is_some_and(|m| is_role(m, "system")));
    messages.truncate(keep);
}

fn is_role(message: &Value, role: &str) -> bool {
    message.get("role").and_then(Value::as_str) == Some(role)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_can_change_but_must_leave_room_for_the_answer() {
        let mut ctx = ContextManager::new(16384, 1024);
        assert!(ctx.set_num_ctx(32768).is_ok());
        assert_eq!(ctx.usage(&[], &serde_json::json!([])).window, 32768);
        let err = ctx.set_num_ctx(1000).unwrap_err();
        assert!(err.contains("1024"), "{err}");
        assert_eq!(ctx.usage(&[], &serde_json::json!([])).window, 32768, "a janela não muda se o valor é recusado");
    }

    use super::*;
    use serde_json::json;

    fn msg(role: &str, content: &str) -> Value {
        json!({"role": role, "content": content})
    }

    fn call(name: &str) -> Value {
        json!({"role": "assistant", "content": "", "tool_calls": [{"function": {"name": name, "arguments": {}}}]})
    }

    fn system() -> Value {
        msg("system", "você é o agente")
    }

    fn no_tools() -> Value {
        json!([])
    }

    // Turno com chamada e resultado de ferramenta. `pad` controla o tamanho.
    fn turn(task: &str, pad: usize) -> Vec<Value> {
        vec![
            msg("user", task),
            call("read_file"),
            json!({"role": "tool", "tool_name": "read_file", "content": "x".repeat(pad)}),
            msg("assistant", "feito"),
        ]
    }

    // Histórico: sistema + turnos antigos + turno atual.
    fn history(old_turns: usize, pad: usize) -> Vec<Value> {
        let mut m = vec![system()];
        for i in 0..old_turns {
            m.extend(turn(&format!("tarefa antiga {i}"), pad));
        }
        m.push(msg("user", "tarefa atual"));
        m
    }

    fn manager_with_budget(chars: usize) -> ContextManager {
        // Janela de `chars` caracteres com cpt 1.0, sem reserva e sem esquemas.
        let mut cm = ContextManager::new(chars, 0);
        cm.calibrate(chars, chars as u64);
        cm
    }

    #[test]
    fn below_limit_changes_nothing() {
        let cm = manager_with_budget(100_000);
        let mut m = history(2, 100);
        let before = m.clone();
        assert_eq!(cm.fit(&mut m, &no_tools()).unwrap(), None);
        assert_eq!(m, before);
    }

    #[test]
    fn above_limit_drops_oldest_whole_turns() {
        let mut m = history(5, 400);
        let budget = size(&m) - 3000; // mais do que a poda dos resultados recupera: força descarte de turnos
        let cm = manager_with_budget(budget);
        let trim = cm.fit(&mut m, &no_tools()).unwrap().expect("deveria descartar");
        assert!(trim.turns >= 1);
        assert!(trim.chars_after <= budget);
        assert!(trim.chars_before > trim.chars_after);
        // O turno mais recente antigo ainda deve estar presente, e os mais antigos saíram.
        assert!(!m.iter().any(|x| x["content"] == "tarefa antiga 0"));
    }

    #[test]
    fn drops_several_turns_when_needed() {
        let mut m = history(6, 300);
        let budget = size(&m) / 3;
        let cm = manager_with_budget(budget);
        let trim = cm.fit(&mut m, &no_tools()).unwrap().unwrap();
        assert!(trim.turns >= 3, "esperava descartar vários turnos, descartou {}", trim.turns);
        assert!(size(&m) <= budget);
    }

    #[test]
    fn tool_calls_and_results_are_removed_together() {
        let mut m = history(4, 200);
        let cm = manager_with_budget(size(&m) / 2);
        cm.fit(&mut m, &no_tools()).unwrap();
        // Toda mensagem "tool" precisa ser precedida por uma chamada de ferramenta ou outra "tool".
        for i in 0..m.len() {
            if is_role(&m[i], "tool") {
                assert!(i > 0, "resultado sem chamada");
                let prev = &m[i - 1];
                assert!(
                    prev.get("tool_calls").is_some() || is_role(prev, "tool"),
                    "resultado de ferramenta órfão na posição {i}"
                );
            }
        }
    }

    #[test]
    fn system_prompt_is_never_dropped() {
        let mut m = history(5, 400);
        let cm = manager_with_budget(size(&m) / 2);
        cm.fit(&mut m, &no_tools()).unwrap();
        assert!(is_role(&m[0], "system"));
        assert_eq!(m[0]["content"], "você é o agente");
    }

    #[test]
    fn current_turn_is_never_dropped() {
        let mut m = history(5, 400);
        let cm = manager_with_budget(size(&m) / 2);
        cm.fit(&mut m, &no_tools()).unwrap();
        assert_eq!(m.last().unwrap()["content"], "tarefa atual");
        assert!(m.iter().any(|x| x["content"] == "tarefa atual"));
    }

    #[test]
    fn current_turn_over_limit_is_explicit_error() {
        let mut m = vec![system(), msg("user", &"y".repeat(5_000))];
        let cm = manager_with_budget(1_000);
        let err = cm.fit(&mut m, &no_tools()).unwrap_err();
        assert!(err.contains("turno atual"), "{err}");
        // Nada foi descartado: o erro não pode mexer no histórico.
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn history_cannot_shrink_below_protected_part() {
        // Sistema + turno atual já ocupam mais do que o orçamento: erro, sem descartar nada.
        let mut m = vec![system(), msg("user", "atual")];
        let cm = manager_with_budget(5);
        assert!(cm.fit(&mut m, &no_tools()).is_err());
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn calibration_updates_chars_per_token() {
        let mut cm = ContextManager::new(8192, 1024);
        assert_eq!(cm.chars_per_token(), DEFAULT_CHARS_PER_TOKEN);
        cm.calibrate(4_000, 1_000);
        assert!((cm.chars_per_token() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn calibration_is_clamped_and_ignores_zero() {
        let mut cm = ContextManager::new(8192, 1024);
        cm.calibrate(1_000_000, 1);
        assert_eq!(cm.chars_per_token(), MAX_CHARS_PER_TOKEN);
        cm.calibrate(10, 0);
        assert_eq!(cm.chars_per_token(), MAX_CHARS_PER_TOKEN);
    }

    #[test]
    fn usage_grows_with_history() {
        let cm = ContextManager::new(8192, 1024);
        let small = cm.usage(&history(1, 10), &no_tools());
        let big = cm.usage(&history(5, 2_000), &no_tools());
        assert!(big.tokens > small.tokens);
        assert_eq!(big.window, 8192);
        assert!(big.percent > small.percent);
    }

    #[test]
    fn clear_keeps_only_system_prompt() {
        let mut m = history(3, 100);
        clear(&mut m);
        assert_eq!(m.len(), 1);
        assert!(is_role(&m[0], "system"));
        assert_eq!(m[0]["content"], "você é o agente");
    }

    #[test]
    fn clear_on_empty_history_is_noop() {
        let mut m: Vec<Value> = vec![];
        clear(&mut m);
        assert!(m.is_empty());
    }

    #[test]
    fn budget_reserves_output_and_tools() {
        let cm = ContextManager::new(8192, 1024);
        let tools = json!([{"name": "x".repeat(1_000)}]);
        let without = cm.budget_chars(&no_tools());
        let with = cm.budget_chars(&tools);
        assert!(with < without);
        // Reserva de saída: (8192 - 1024) tokens × 3 caracteres, menos o JSON dos esquemas ("[]" = 2).
        assert_eq!(without, (8192 - 1024) * 3 - value_size(&no_tools()));
    }

    // Turno em que `tool` devolveu `pad` caracteres para o caminho `path`.
    fn turn_on(task: &str, tool: &str, path: &str, pad: usize) -> Vec<Value> {
        vec![
            msg("user", task),
            json!({"role": "assistant", "content": "", "tool_calls": [{"function": {"name": tool, "arguments": {"path": path}}}]}),
            json!({"role": "tool", "tool_name": tool, "content": "x".repeat(pad)}),
            msg("assistant", "feito"),
        ]
    }

    fn tool_text(m: &Value) -> &str {
        m["content"].as_str().unwrap()
    }

    #[test]
    fn old_tool_results_are_shrunk_before_any_turn_is_dropped() {
        let mut m = vec![system()];
        m.extend(turn_on("lê a", "read_file", "a.rs", 3_000));
        m.extend(turn_on("lê b", "read_file", "b.rs", 3_000));
        m.push(msg("user", "agora"));
        // Cabe se um dos dois resultados virar marcador, e só o mais antigo precisa virar.
        let cm = manager_with_budget(size(&m) - 2_500);
        let trim = cm.fit(&mut m, &no_tools()).unwrap().expect("deveria ter ajustado");
        assert_eq!((trim.turns, trim.pruned), (0, 1), "{trim:?}");
        assert!(tool_text(&m[3]).starts_with("[resultado antigo de read_file a.rs omitido (3000 car.)"), "{}", tool_text(&m[3]));
        assert_eq!(tool_text(&m[7]).len(), 3_000, "o mais novo fica intacto");
        assert_eq!(m.len(), 10, "nenhum turno foi descartado");
        assert!(trim.summary().contains("1 resultado(s) antigo(s)"), "{}", trim.summary());
    }

    #[test]
    fn a_result_superseded_by_a_later_call_on_the_same_path_goes_first() {
        let mut m = vec![system()];
        m.extend(turn_on("lê a", "read_file", "a.rs", 3_000));
        m.extend(turn_on("lê b", "read_file", "b.rs", 3_000));
        m.extend(turn_on("edita a", "edit_file", "a.rs", 10));
        m.push(msg("user", "agora"));
        // O mais antigo (a.rs) já foi superado pela edição, mas o b.rs não: poda a.rs e só ele.
        let cm = manager_with_budget(size(&m) - 2_500);
        cm.fit(&mut m, &no_tools()).unwrap();
        assert!(tool_text(&m[3]).starts_with(PRUNED_MARK));
        assert_eq!(tool_text(&m[7]).len(), 3_000);

        // Invertendo a ordem: o mais antigo (b.rs) NÃO foi superado e o mais novo (a.rs) foi.
        let mut m = vec![system()];
        m.extend(turn_on("lê b", "read_file", "b.rs", 3_000));
        m.extend(turn_on("lê a", "read_file", "a.rs", 3_000));
        m.extend(turn_on("edita a", "edit_file", "a.rs", 10));
        m.push(msg("user", "agora"));
        let cm = manager_with_budget(size(&m) - 2_500);
        cm.fit(&mut m, &no_tools()).unwrap();
        assert_eq!(tool_text(&m[3]).len(), 3_000, "b.rs ainda vale");
        assert!(tool_text(&m[7]).starts_with(PRUNED_MARK), "a.rs foi superado pela edição");
    }

    #[test]
    fn pruning_is_not_enough_then_turns_are_dropped_too() {
        let mut m = history(3, 2_000);
        let cm = manager_with_budget(size(&m) / 8);
        let trim = cm.fit(&mut m, &no_tools()).unwrap().unwrap();
        assert!(trim.pruned > 0 && trim.turns > 0, "{trim:?}");
        assert!(size(&m) <= cm.budget_chars(&no_tools()));
        assert!(trim.summary().contains("resumido(s) e"), "{}", trim.summary());
    }

    #[test]
    fn small_results_and_the_current_turn_are_never_pruned() {
        let mut m = vec![system()];
        m.extend(turn_on("antiga", "read_file", "a.rs", 100));
        m.push(msg("user", "atual"));
        m.push(json!({"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "read_file", "arguments": {"path": "b.rs"}}}]}));
        m.push(json!({"role": "tool", "tool_name": "read_file", "content": "y".repeat(5_000)}));
        let before = m.clone();
        let cm = manager_with_budget(size(&m) - 50);
        // O resultado pequeno não rende e o grande é do turno atual: sobra descartar o turno antigo.
        let trim = cm.fit(&mut m, &no_tools()).unwrap().unwrap();
        assert_eq!(trim.pruned, 0, "{trim:?}");
        assert_eq!(m.last(), before.last());
    }
}

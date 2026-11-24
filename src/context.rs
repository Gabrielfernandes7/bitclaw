// Gerenciador de contexto: mantém o histórico dentro do orçamento da janela do modelo.
// Descarta turnos antigos inteiros. Nunca descarta o prompt do sistema nem o turno atual.
//
// Tamanho medido em caracteres do JSON de cada mensagem. A estimativa de caracteres por token
// começa conservadora e é calibrada com `prompt_eval_count`, que o Ollama devolve a cada chamada.

use serde_json::Value;

const DEFAULT_CHARS_PER_TOKEN: f64 = 3.0;
const MIN_CHARS_PER_TOKEN: f64 = 1.0;
const MAX_CHARS_PER_TOKEN: f64 = 6.0;

// Registro de um ajuste feito antes de uma chamada ao modelo.
#[derive(Debug, Clone, PartialEq)]
pub struct Trim {
    // Turnos antigos removidos.
    pub turns: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub budget: usize,
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

    // Ajusta a estimativa com o número real de tokens de entrada devolvido pelo Ollama.
    // `sent_chars` deve ser o tamanho medido pela mesma função `size` sobre o que foi enviado.
    pub fn calibrate(&mut self, sent_chars: usize, prompt_tokens: u64) {
        if prompt_tokens == 0 {
            return;
        }
        let ratio = sent_chars as f64 / prompt_tokens as f64;
        self.chars_per_token = ratio.clamp(MIN_CHARS_PER_TOKEN, MAX_CHARS_PER_TOKEN);
    }

    // Garante que o histórico cabe no orçamento, descartando turnos antigos inteiros.
    // Devolve `None` se nada foi descartado. Erra se o turno atual sozinho não cabe.
    pub fn fit(&self, messages: &mut Vec<Value>, tools: &Value) -> Result<Option<Trim>, String> {
        let budget = self.budget_chars(tools);
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

        Ok(Some(Trim { turns, chars_before: before, chars_after: total, budget }))
    }
}

// Tamanho de um valor JSON (mensagem ou esquemas), em caracteres.
pub fn value_size(value: &Value) -> usize {
    value.to_string().chars().count()
}

// Tamanho de uma lista de mensagens, em caracteres.
pub fn size(messages: &[Value]) -> usize {
    messages.iter().map(value_size).sum()
}

fn is_role(message: &Value, role: &str) -> bool {
    message.get("role").and_then(Value::as_str) == Some(role)
}

#[cfg(test)]
mod tests {
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
        let budget = size(&m) - 1500; // força descarte de alguns turnos
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
    fn budget_reserves_output_and_tools() {
        let cm = ContextManager::new(8192, 1024);
        let tools = json!([{"name": "x".repeat(1_000)}]);
        let without = cm.budget_chars(&no_tools());
        let with = cm.budget_chars(&tools);
        assert!(with < without);
        // Reserva de saída: (8192 - 1024) tokens × 3 caracteres, menos o JSON dos esquemas ("[]" = 2).
        assert_eq!(without, (8192 - 1024) * 3 - value_size(&no_tools()));
    }
}

//! O loop do turno: modelo → ferramentas → modelo, até a resposta final.

use super::guard::{LOOPING, LoopGuard};
use super::model::ChatModel;
use super::report::{Action, Progress, TurnReport};
use super::text_calls::{calls_from_text, normalize_args};
use crate::core::context::{self, ContextManager};
use crate::core::tool::{Approver, Outcome, Registry, ToolContext, specs_value};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

pub const MAX_STEPS: usize = 8;
// Quantas vezes por turno uma resposta totalmente vazia (sem texto e sem chamada) é pedida de novo.
const MAX_EMPTY_RETRIES: usize = 2;
// Dito ao modelo, só na nova tentativa, quando a resposta veio vazia. Não entra no histórico.
const EMPTY_REPLY_NUDGE: &str = "Sua resposta anterior veio vazia. Se o pedido exige uma alteração ou leitura, chame a ferramenta agora, \
                                 com todos os argumentos preenchidos; se não, responda em texto.";
// Quantas falhas seguidas da mesma ferramenta encerram o turno.
pub const MAX_CONSECUTIVE_FAILURES: usize = 2;

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
    let tools = registry.specs();
    let tools_json = specs_value(&tools);
    let mut report = TurnReport::default();
    let mut last_call: Option<String> = None;
    let mut repeats = 0;
    let mut failing: Option<(String, usize)> = None;
    // Alterações já executadas neste turno. Repetir a mesma alteração não a reaplica.
    let mut applied: HashSet<String> = HashSet::new();
    // Leituras já feitas desde a última alteração ou execução. Repeti-las devolveria o mesmo resultado.
    let mut seen_reads: HashSet<String> = HashSet::new();
    // Passos seguidos em que o modelo só pediu leituras que já tinha.
    let mut stalled = 0;
    let mut empty_retries = 0;
    let mut nudge = false;

    for _ in 0..limits.max_steps {
        if progress.cancelled() {
            report.error = Some(INTERRUPTED.into());
            return Ok(report);
        }
        if let Some(trim) = ctx.fit(messages, &tools_json)? {
            report.trims.push(trim);
        }
        let sent_chars = context::size(messages) + context::value_size(&tools_json);
        progress.model_waiting(true);
        let mut guard = LoopGuard::default();
        // Na nova tentativa de uma resposta vazia, o empurrão vai só nesta chamada.
        let retry_messages = nudge.then(|| {
            let mut sent = messages.clone();
            sent.push(json!({"role": "user", "content": EMPTY_REPLY_NUDGE}));
            sent
        });
        nudge = false;
        let reply = model.complete_streaming(retry_messages.as_deref().unwrap_or(messages), &tools, &mut |chunk| {
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
        let empty = msg.get("content").and_then(Value::as_str).unwrap_or("").trim().is_empty()
            && msg.get("tool_calls").and_then(Value::as_array).is_none_or(|c| c.is_empty());
        if empty && empty_retries < MAX_EMPTY_RETRIES {
            // Nada utilizável (alguns provedores descartam uma chamada que não conseguem interpretar):
            // a resposta vazia não entra no histórico, e o modelo tenta de novo.
            empty_retries += 1;
            nudge = true;
            continue;
        }
        messages.push(msg.clone());

        let mut calls = msg.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        if calls.is_empty() {
            let known = registry.names();
            let (from_text, rest) = calls_from_text(msg.get("content").and_then(Value::as_str).unwrap_or(""), &known);
            if !from_text.is_empty() {
                // Regrava a chamada no histórico no formato estruturado. Como texto solto, o resultado da
                // ferramenta não responde a nenhuma chamada do modelo, e ele a repete; assim o template do
                // modelo mostra o par chamada/resultado do jeito nativo.
                if let Some(last) = messages.last_mut() {
                    last["content"] = Value::String(rest);
                    last["tool_calls"] = Value::Array(from_text.clone());
                }
                calls = from_text;
            }
        }
        if calls.is_empty() {
            report.text = msg.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string();
            return Ok(report);
        }

        let mut suppressed = 0;
        let total_calls = calls.len();
        for call in calls {
            let func = call.get("function").cloned().unwrap_or(Value::Null);
            let name = func.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let args = normalize_args(func.get("arguments"));

            // Leitura idêntica a uma já feita: não executa de novo; devolve o aviso e o resultado segue acima.
            let read_key = format!("{name}{args}");
            if registry.idempotent(&name) && seen_reads.contains(&read_key) {
                suppressed += 1;
                messages.push(json!({
                    "role": "tool",
                    "tool_name": name.clone(),
                    "content": "você já fez esta chamada neste turno e o resultado está acima na conversa. \
                                Não repita: use-o e responda ao usuário, ou chame outra ferramenta."
                }));
                continue;
            }
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

            let outcome = registry.execute(&ToolContext::new(root, approver), &name, &args);
            progress.tool_done(&name, &args, &outcome);
            if registry.idempotent(&name) {
                seen_reads.insert(read_key);
            } else {
                // Alteração ou execução pode mudar o que as leituras devolvem: valem de novo.
                seen_reads.clear();
            }

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

        // O modelo só repetiu leituras que já tinha: avisou uma vez; se insiste, pede a resposta sem ferramentas.
        if suppressed == total_calls {
            stalled += 1;
            if stalled >= 2 {
                return finish_without_tools(model, messages, ctx, progress, report);
            }
        } else {
            stalled = 0;
        }
    }

    report.error = Some(format!("limite de {} passos atingido sem resposta final", limits.max_steps));
    Ok(report)
}

// Última tentativa quando o modelo não sai das mesmas leituras: pede a resposta com o que já foi lido,
// sem oferecer ferramentas. Se nem assim vier texto, o turno termina com o erro de repetição.
// Como nas outras chamadas, o histórico é ajustado ao contexto, o detector de laço vigia a geração e um
// erro do provedor sobe como está, em vez de virar um diagnóstico de repetição que não aconteceu.
fn finish_without_tools(
    model: &dyn ChatModel,
    messages: &mut Vec<Value>,
    ctx: &ContextManager,
    progress: &dyn Progress,
    mut report: TurnReport,
) -> Result<TurnReport, String> {
    let prompt = json!({
        "role": "user",
        "content": "Você já tem os resultados das ferramentas acima. Responda agora ao pedido usando só o que eles trouxeram, sem chamar mais ferramentas."
    });
    // Ajusta antes de acrescentar o pedido: depois, ele seria o turno atual e o pedido original sairia do histórico.
    let mut ask = messages.clone();
    if let Some(trim) = ctx.fit_reserving(&mut ask, &json!([]), context::value_size(&prompt))? {
        report.trims.push(trim);
    }
    ask.push(prompt);

    progress.model_waiting(true);
    let mut guard = LoopGuard::default();
    let reply = model.complete_streaming(&ask, &[], &mut |chunk| {
        progress.model_chunk(chunk);
        !progress.cancelled() && !guard.feed(chunk)
    });
    progress.model_waiting(false);
    let reply = reply?;
    progress.model_tokens(reply.prompt_tokens, reply.output_tokens);
    if guard.looping {
        report.error = Some(LOOPING.into());
    } else if progress.cancelled() {
        report.error = Some(INTERRUPTED.into());
    } else {
        let text = reply.message.get("content").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if text.is_empty() {
            report.error = Some("o modelo repetiu a mesma chamada; interrompendo".into());
        } else {
            messages.push(reply.message);
            report.text = text;
        }
    }
    Ok(report)
}

//! Passos do agente na conversa: o que ele está fazendo agora e o que já fez.
//!
//! Vêm das chamadas de ferramenta que o próprio BitClaw executa, então não custam token: o modelo
//! não narra nada a mais. Enquanto o passo roda, a linha de status mostra `lendo AGENTS.md…`; quando
//! termina, ele entra na conversa como `✓ leu AGENTS.md`.

use super::theme::*;
use bitclaw::tools::Outcome;
use ratatui::text::{Line, Span};
use serde_json::Value;

// Quantos caracteres de uma falha aparecem na linha do passo.
const DETAIL_CHARS: usize = 80;

// Quantas linhas da saída de um script aparecem na conversa.
const OUTPUT_LINES: usize = 20;

// Verbos de uma ferramenta: no gerúndio, enquanto roda, e no passado, quando termina.
struct Verbs {
    doing: &'static str,
    done: &'static str,
}

fn verbs(name: &str) -> Option<Verbs> {
    Some(match name {
        "list_dir" => Verbs { doing: "listando", done: "listou" },
        "read_file" => Verbs { doing: "lendo", done: "leu" },
        "edit_file" => Verbs { doing: "editando", done: "editou" },
        "create_file" => Verbs { doing: "criando", done: "criou" },
        "create_dir" => Verbs { doing: "criando pasta", done: "criou pasta" },
        "delete_file" => Verbs { doing: "apagando", done: "apagou" },
        "run_script" => Verbs { doing: "executando", done: "executou" },
        _ => return None,
    })
}

// Verbo mais o alvo, como `lendo AGENTS.md`. Sem alvo, fica só o verbo.
fn phrase(verb: &str, args: &Value) -> String {
    let target = args.get("path").and_then(Value::as_str).unwrap_or("").trim();
    if target.is_empty() { verb.to_string() } else { format!("{verb} {target}") }
}

// Texto do passo enquanto roda: `lendo AGENTS.md`.
pub(super) fn doing(name: &str, args: &Value) -> String {
    match verbs(name) {
        Some(v) => phrase(v.doing, args),
        None => format!("executando {name}"),
    }
}

// O que a ferramenta devolveu, na forma que a interface precisa.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum StepResult {
    Done,
    Refused(String),
    Error(String),
    // Um script rodou: se deu certo, a primeira linha do resultado (código de saída) e a saída.
    Ran { ok: bool, summary: String, output: String },
}

impl StepResult {
    // Como `from_outcome`, mas guarda a saída de um script para a conversa mostrá-la.
    pub(super) fn from_tool(name: &str, outcome: &Outcome) -> Self {
        let text = match outcome {
            Outcome::Done(t) | Outcome::Error(t) if name == "run_script" => t,
            _ => return Self::from_outcome(outcome),
        };
        // Falhas antes de rodar (arquivo ausente, recusa de caminho) têm uma linha só e vão como erro comum.
        let Some((summary, output)) = text.split_once('\n') else { return Self::from_outcome(outcome) };
        StepResult::Ran {
            ok: matches!(outcome, Outcome::Done(_)),
            summary: first_line(summary),
            output: output.trim_end().to_string(),
        }
    }

    pub(super) fn from_outcome(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Done(_) => StepResult::Done,
            Outcome::Refused(t) => StepResult::Refused(first_line(t)),
            Outcome::Error(t) => StepResult::Error(first_line(t)),
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").chars().take(DETAIL_CHARS).collect()
}

// Linha do passo que terminou: `✓ leu AGENTS.md`, `⊘ editando x · recusado` ou `✗ lendo x · motivo`.
pub(super) fn done(name: &str, args: &Value, result: &StepResult) -> Line<'static> {
    match result {
        StepResult::Done => {
            let finished = match verbs(name) {
                Some(v) => phrase(v.done, args),
                None => format!("executou {name}"),
            };
            Line::from(vec![Span::raw("  "), Span::styled("✓ ", fg(GREEN)), Span::raw(finished)])
        }
        StepResult::Refused(_) => Line::from(vec![
            Span::raw("  "),
            Span::styled("⊘ ", fg(YELLOW)),
            Span::raw(doing(name, args)),
            Span::styled(" · recusado", fg(GRAY)),
        ]),
        StepResult::Ran { ok: true, .. } => {
            let finished = phrase(verbs(name).map_or("executou", |v| v.done), args);
            Line::from(vec![Span::raw("  "), Span::styled("✓ ", fg(GREEN)), Span::raw(finished)])
        }
        StepResult::Ran { ok: false, summary, .. } => Line::from(vec![
            Span::raw("  "),
            Span::styled("✗ ", fg(RED)),
            Span::raw(doing(name, args)),
            Span::styled(format!(" · {summary}"), fg(GRAY)),
        ]),
        StepResult::Error(detail) => Line::from(vec![
            Span::raw("  "),
            Span::styled("✗ ", fg(RED)),
            Span::raw(doing(name, args)),
            Span::styled(format!(" · {detail}"), fg(GRAY)),
        ]),
    }
}

// Linhas da saída de um script, logo abaixo do passo: o que o programa realmente escreveu, sem
// depender do que o modelo conta depois. Vazio para os demais passos.
pub(super) fn output_lines(result: &StepResult, width: usize) -> Vec<Line<'static>> {
    let StepResult::Ran { output, .. } = result else { return Vec::new() };
    let room = width.saturating_sub(6).max(10);
    let all: Vec<&str> = output.lines().collect();
    let mut lines: Vec<Line<'static>> = all
        .iter()
        .take(OUTPUT_LINES)
        .map(|l| {
            let shown: String = l.chars().take(room).collect();
            Line::from(vec![Span::styled("    │ ", fg(GRAY)), Span::raw(shown)])
        })
        .collect();
    if all.len() > OUTPUT_LINES {
        lines.push(Line::from(Span::styled(format!("    … mais {} linhas", all.len() - OUTPUT_LINES), fg(GRAY))));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn doing_names_the_action_and_its_target() {
        assert_eq!(doing("read_file", &json!({"path": "AGENTS.md"})), "lendo AGENTS.md");
        assert_eq!(doing("list_dir", &json!({"path": "docs/"})), "listando docs/");
        assert_eq!(doing("list_dir", &json!({})), "listando", "sem alvo, fica o verbo");
        assert_eq!(doing("unknown_tool", &json!({})), "executando unknown_tool");
    }

    #[test]
    fn a_finished_step_says_what_happened() {
        let read = json!({"path": "AGENTS.md"});
        assert_eq!(text(&done("read_file", &read, &StepResult::Done)), "  ✓ leu AGENTS.md");
        assert_eq!(text(&done("edit_file", &read, &StepResult::Refused(String::new()))), "  ⊘ editando AGENTS.md · recusado");
        assert_eq!(
            text(&done("read_file", &read, &StepResult::Error("No such file".into()))),
            "  ✗ lendo AGENTS.md · No such file"
        );
    }

    #[test]
    fn a_script_run_shows_its_real_output_under_the_step() {
        let args = json!({"path": "ola.sh"});
        let ok = StepResult::from_tool("run_script", &Outcome::Done("código de saída: 0\nOlá, mundo!\nlinha 2".into()));
        assert_eq!(text(&done("run_script", &args, &ok)), "  ✓ executou ola.sh");
        let shown: Vec<String> = output_lines(&ok, 80).iter().map(text).collect();
        assert_eq!(shown, vec!["    │ Olá, mundo!", "    │ linha 2"]);

        let bad = StepResult::from_tool("run_script", &Outcome::Error("saiu com código 3\nantes".into()));
        assert_eq!(text(&done("run_script", &args, &bad)), "  ✗ executando ola.sh · saiu com código 3");
        assert_eq!(output_lines(&bad, 80).len(), 1, "a saída da falha também aparece");

        let missing = StepResult::from_tool("run_script", &Outcome::Error("ola.sh não existe: crie o script antes com create_file".into()));
        assert!(matches!(missing, StepResult::Error(_)), "falha antes de rodar é um erro comum");
        assert!(output_lines(&missing, 80).is_empty());
        let other = StepResult::from_tool("read_file", &Outcome::Done("a\nb".into()));
        assert_eq!(other, StepResult::Done, "só run_script mostra saída");
    }

    #[test]
    fn a_long_output_is_cut_with_a_count_of_what_is_missing() {
        let out = (1..=30).map(|n| format!("l{n}")).collect::<Vec<_>>().join("\n");
        let r = StepResult::from_tool("run_script", &Outcome::Done(format!("código de saída: 0\n{out}")));
        let lines = output_lines(&r, 80);
        assert_eq!(lines.len(), OUTPUT_LINES + 1);
        assert_eq!(text(lines.last().unwrap()), "    … mais 10 linhas");
    }

    #[test]
    fn long_failures_are_cut_to_one_line() {
        let long = "x".repeat(200);
        let line = text(&done("read_file", &json!({}), &StepResult::Error(first_line(&long))));
        assert!(line.chars().count() <= DETAIL_CHARS + 20, "{line}");
    }
}

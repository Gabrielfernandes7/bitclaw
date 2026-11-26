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

// Verbo de uma ferramenta no gerúndio, enquanto roda (`lendo AGENTS.md`).
struct Verbs {
    doing: &'static str,
}

fn verbs(name: &str) -> Option<Verbs> {
    Some(match name {
        "list_dir" => Verbs { doing: "listando" },
        "read_file" => Verbs { doing: "lendo" },
        "edit_file" => Verbs { doing: "editando" },
        "create_file" => Verbs { doing: "criando" },
        "create_dir" => Verbs { doing: "criando pasta" },
        "delete_file" => Verbs { doing: "apagando" },
        "run_script" => Verbs { doing: "executando" },
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

// Cabeçalho de um grupo: o verbo no passado e o substantivo, no plural quando há mais de um.
fn group_header(name: &str, n: usize) -> String {
    let (verb, one, many) = match name {
        "run_script" => ("executou", "script", "scripts"),
        "read_file" => ("leu", "arquivo", "arquivos"),
        "list_dir" => ("listou", "pasta", "pastas"),
        "edit_file" => ("editou", "arquivo", "arquivos"),
        "create_file" => ("criou", "arquivo", "arquivos"),
        "create_dir" => ("criou", "pasta", "pastas"),
        "delete_file" => ("apagou", "arquivo", "arquivos"),
        _ => return format!("executou {n} {name}"),
    };
    format!("{verb} {n} {}", if n == 1 { one } else { many })
}

// Uma chamada de ferramenta do grupo: o alvo (ou o nome, sem alvo), com a marca do resultado.
fn item(name: &str, args: &Value, result: &StepResult) -> Line<'static> {
    let target = args.get("path").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).unwrap_or(name).to_string();
    let (mark, color, tail) = match result {
        StepResult::Done | StepResult::Ran { ok: true, .. } => ("✓ ", GREEN, String::new()),
        StepResult::Refused(_) => ("⊘ ", YELLOW, " · recusado".to_string()),
        StepResult::Ran { ok: false, summary, .. } => ("✗ ", RED, format!(" · {summary}")),
        StepResult::Error(detail) => ("✗ ", RED, format!(" · {detail}")),
    };
    Line::from(vec![
        Span::raw("    ⎿ "),
        Span::styled(mark, fg(color)),
        Span::raw(target),
        Span::styled(tail, fg(GRAY)),
    ])
}

// Chamadas seguidas da mesma ferramenta, mostradas como um bloco só: `● executou 2 scripts`, uma linha
// por chamada e, em script, a saída embaixo. Um bloco fecha quando chega outra coisa na conversa.
pub(super) struct Group {
    name: String,
    items: Vec<(Value, StepResult)>,
}

impl Group {
    pub(super) fn new(name: &str) -> Self {
        Self { name: name.to_string(), items: Vec::new() }
    }

    pub(super) fn is(&self, name: &str) -> bool {
        self.name == name
    }

    pub(super) fn push(&mut self, args: Value, result: StepResult) {
        self.items.push((args, result));
    }

    pub(super) fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut out = vec![Line::from(vec![
            Span::raw("  "),
            Span::styled("● ", fg(GRAY)),
            Span::raw(capitalize(&group_header(&self.name, self.items.len()))),
        ])];
        for (args, result) in &self.items {
            out.push(item(&self.name, args, result));
            out.extend(output_lines(result, width));
        }
        out
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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
    fn a_group_counts_its_calls_and_lists_each_target() {
        let mut g = Group::new("read_file");
        g.push(json!({"path": "AGENTS.md"}), StepResult::Done);
        g.push(json!({"path": "README.md"}), StepResult::Refused(String::new()));
        let shown: Vec<String> = g.lines(80).iter().map(text).collect();
        assert_eq!(shown, vec!["  ● Leu 2 arquivos", "    ⎿ ✓ AGENTS.md", "    ⎿ ⊘ README.md · recusado"]);
        let mut one = Group::new("list_dir");
        one.push(json!({"path": "docs/"}), StepResult::Done);
        assert_eq!(one.lines(80)[0].spans.last().unwrap().content, "Listou 1 pasta");
        assert!(g.is("read_file") && !g.is("list_dir"));
    }

    #[test]
    fn a_failed_call_says_why() {
        let mut g = Group::new("read_file");
        g.push(json!({"path": "AGENTS.md"}), StepResult::Error("No such file".into()));
        assert_eq!(text(&g.lines(80)[1]), "    ⎿ ✗ AGENTS.md · No such file");
    }

    #[test]
    fn a_script_run_shows_its_real_output_under_the_step() {
        let args = json!({"path": "ola.sh"});
        let ok = StepResult::from_tool("run_script", &Outcome::Done("código de saída: 0\nOlá, mundo!\nlinha 2".into()));
        let mut g = Group::new("run_script");
        g.push(args.clone(), ok.clone());
        let shown: Vec<String> = g.lines(80).iter().map(text).collect();
        assert_eq!(shown, vec!["  ● Executou 1 script", "    ⎿ ✓ ola.sh", "    │ Olá, mundo!", "    │ linha 2"]);

        let bad = StepResult::from_tool("run_script", &Outcome::Error("saiu com código 3\nantes".into()));
        let mut g = Group::new("run_script");
        g.push(args.clone(), bad.clone());
        assert_eq!(text(&g.lines(80)[1]), "    ⎿ ✗ ola.sh · saiu com código 3");
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
        let mut g = Group::new("read_file");
        g.push(json!({}), StepResult::Error(first_line(&long)));
        let line = text(&g.lines(80)[1]);
        assert!(line.chars().count() <= DETAIL_CHARS + 20, "{line}");
    }
}

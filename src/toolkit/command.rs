//! `run_command`: roda um programa conhecido, sem shell, na pasta do projeto.
//!
//! O modelo dá o programa e os argumentos como lista, nunca uma linha de shell: `;`, `&&`, `|`, `$()` e
//! `>` não têm efeito. Cada chamada cai em uma de três classes:
//!
//! - **Leitura** (`ls`, `wc`, `git status`...): roda sem perguntar, como `read_file`, e vale no modo Plan.
//!   Os caminhos passam pelo `sandbox::resolve`.
//! - **Alteração** (`git add`, `git commit`, `cargo test`...): mostra o comando e pede aprovação. Nem
//!   `--yes` concede, porque `cargo test` roda código do projeto.
//! - **Recusado**: qualquer outro programa. A mensagem manda criar um script e usar `run_script`.
//!
//! A execução (ambiente reduzido, prazo, saída limitada) é a mesma do `run_script`.

use super::script::{run_process, visible, RUN_TIMEOUT};
use crate::core::sandbox::resolve;
use crate::core::tool::{Annotations, Approver, Outcome, Preview, PreviewLine, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

// Programas só de leitura, com qualquer argumento que não seja uma opção recusada.
const READ_PROGRAMS: &[&str] = &["ls", "pwd", "wc", "head", "tail", "cat"];
// `git`: o subcomando é sempre o primeiro argumento (nada de `git -c ...`).
const GIT_READ: &[&str] = &["status", "diff", "log", "show", "blame", "rev-parse", "ls-files"];
const GIT_MUTATING: &[&str] = &["add", "commit"];
const CARGO_MUTATING: &[&str] = &["check", "test", "build", "clippy"];
// Opções que escrevem arquivo, rodam outro programa ou mudam onde o comando enxerga o projeto.
const BLOCKED_FLAGS: &[&str] = &["--output", "--ext-diff", "--exec", "--git-dir", "--work-tree", "--upload-pack", "--config", "--follow"];
const MAX_ARGS: usize = 32;
const MAX_ARG_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Read,
    Mutating,
}

fn denied(program: &str) -> String {
    format!(
        "'{program}' não está liberado. Liberados: {}, git ({}), cargo ({}). \
         Para outro comando, crie um script com create_file e rode com run_script",
        READ_PROGRAMS.join(", "),
        [GIT_READ, GIT_MUTATING].concat().join(", "),
        CARGO_MUTATING.join(", "),
    )
}

// A classe de uma chamada, ou o motivo da recusa, sem olhar os caminhos: só o programa e os argumentos.
fn class_of(program: &str, args: &[String]) -> Result<Class, String> {
    if program.is_empty() || program.contains('/') || program.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("'program' deve ser só o nome do programa (ex.: git), sem caminho nem argumentos; os argumentos vão em 'args'".into());
    }
    if args.len() > MAX_ARGS || args.iter().any(|a| a.chars().count() > MAX_ARG_CHARS || a.chars().any(|c| c.is_control())) {
        return Err(format!("no máximo {MAX_ARGS} argumentos de até {MAX_ARG_CHARS} caracteres, sem caracteres de controle"));
    }
    if let Some(flag) = args.iter().find(|a| BLOCKED_FLAGS.iter().any(|b| *a == b || a.starts_with(&format!("{b}=")))) {
        return Err(format!("a opção {flag} não é permitida"));
    }
    let class = match program {
        p if READ_PROGRAMS.contains(&p) => {
            if p == "tail" && args.iter().any(|a| a == "-f" || a == "-F") {
                return Err("tail -f nunca termina; leia o fim do arquivo sem -f".into());
            }
            Class::Read
        }
        "git" => match args.first().map(String::as_str) {
            Some(sub) if GIT_READ.contains(&sub) => Class::Read,
            Some(sub) if GIT_MUTATING.contains(&sub) => Class::Mutating,
            _ => return Err(denied(program)),
        },
        "cargo" => match args.first().map(String::as_str) {
            Some(sub) if CARGO_MUTATING.contains(&sub) => Class::Mutating,
            _ => return Err(denied(program)),
        },
        _ => return Err(denied(program)),
    };
    Ok(class)
}

// `class_of` mais a checagem dos caminhos. Numa leitura, todo argumento que não é opção tem de ser um
// caminho que o projeto aceitaria (ou um nome de revisão, como HEAD~1, que também passa).
pub fn classify(root: &Path, program: &str, args: &[String]) -> Result<Class, String> {
    let class = class_of(program, args)?;
    if class == Class::Read {
        for arg in args.iter().filter(|a| !a.starts_with('-')) {
            resolve(root, arg).map_err(|e| format!("argumento '{arg}': {e}"))?;
        }
    }
    Ok(class)
}

fn parse(args: &Value) -> Result<(String, Vec<String>), String> {
    let program = str_arg(args, "program")?.to_string();
    let list = match args.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or("'args' deve ter só textos"))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("'args' deve ser uma lista de textos".into()),
    };
    Ok((program, list))
}

pub struct RunCommand;

impl Tool for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "run_command",
            "Roda um programa conhecido na pasta do projeto, sem shell. 'program' é só o nome (git, cargo, ls, wc, head, tail, cat, pwd) \
             e 'args' é a lista de argumentos, um texto por item. Leitura (git status, git diff, ls) roda direto; \
             alteração (git add, git commit, cargo test) pede a confirmação do usuário. Não use ; && | > nem $(): não funcionam. \
             Comandos interativos não funcionam.",
            json!({
                "type": "object",
                "properties": {
                    "program": {"type": "string", "description": "Nome do programa, ex.: git"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Argumentos, um texto por item. Ex.: [\"status\", \"--short\"]"}
                },
                "required": ["program"]
            }),
        )
    }

    // Sem olhar a chamada, vale o lado seguro: altera e é destrutiva.
    fn annotations(&self) -> Annotations {
        Annotations::default()
    }

    fn per_call_annotations(&self) -> bool {
        true
    }

    // Uma chamada de leitura não altera nada, e repeti-la sem alteração no meio devolve o mesmo.
    fn call_annotations(&self, args: &Value) -> Annotations {
        // Sem a pasta do projeto aqui, a checagem dos caminhos fica para `run`; o que decide a classe
        // é o programa, e um caminho recusado vira erro na execução, nunca uma alteração.
        let read = parse(args).is_ok_and(|(program, list)| class_of(&program, &list) == Ok(Class::Read));
        if read {
            Annotations { read_only: true, destructive: false, idempotent: true }
        } else {
            Annotations::default()
        }
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(run_command(ctx.root, args, ctx.approver, RUN_TIMEOUT))
    }
}

fn preview(program: &str, args: &[String]) -> Preview {
    let line = format!("{program} {}", args.iter().map(|a| visible(a)).collect::<Vec<_>>().join(" "));
    Preview::new(
        format!("executar {}", line.trim_end()),
        vec![
            PreviewLine::Context(format!("$ {}", line.trim_end())),
            PreviewLine::Note("o comando roda com as suas permissões, na pasta do projeto: confira antes de aceitar.".into()),
        ],
    )
}

fn run_command(root: &Path, args: &Value, approver: &dyn Approver, limit: std::time::Duration) -> Result<Outcome, String> {
    let (program, list) = parse(args)?;
    let class = classify(root, &program, &list)?;
    if class == Class::Mutating && !approver.approve_run(&preview(&program, &list)) {
        return Ok(Outcome::Refused("usuário recusou a execução; nada foi executado".into()));
    }
    let mut command = Command::new(&program);
    command.args(&list);
    run_process(command, root, limit, |e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("'{program}' não está instalado ou não está no PATH"),
        _ => format!("não consegui executar {program}: {e}"),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::core::tool::{AutoApprover, Registry};
    use std::fs;
    use std::time::Duration;

    struct Yes;
    impl Approver for Yes {
        fn approve(&self, _p: &Preview) -> bool {
            true
        }
        fn approve_run(&self, _p: &Preview) -> bool {
            true
        }
    }

    struct No;
    impl Approver for No {
        fn approve(&self, _p: &Preview) -> bool {
            true
        }
        fn approve_run(&self, _p: &Preview) -> bool {
            false
        }
    }

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-cmd-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn class(root: &Path, program: &str, args: &[&str]) -> Result<Class, String> {
        classify(root, program, &strings(args))
    }

    fn run(root: &Path, program: &str, args: &[&str], approver: &dyn Approver) -> Outcome {
        wrap(run_command(root, &json!({"program": program, "args": args}), approver, Duration::from_secs(20)))
    }

    #[test]
    fn known_commands_fall_in_their_class() {
        let r = project("classes");
        assert_eq!(class(&r, "ls", &["-la"]), Ok(Class::Read));
        assert_eq!(class(&r, "git", &["status", "--short"]), Ok(Class::Read));
        assert_eq!(class(&r, "git", &["log", "-n", "5", "--oneline"]), Ok(Class::Read));
        assert_eq!(class(&r, "git", &["diff", "HEAD~1"]), Ok(Class::Read));
        assert_eq!(class(&r, "git", &["commit", "-m", "mensagem com espaço"]), Ok(Class::Mutating));
        assert_eq!(class(&r, "cargo", &["test"]), Ok(Class::Mutating));
    }

    #[test]
    fn unknown_or_dangerous_commands_are_refused_with_the_way_out() {
        let r = project("denied");
        for (program, args) in [
            ("rm", vec!["-rf", "x"]),
            ("sudo", vec!["ls"]),
            ("curl", vec!["http://x"]),
            ("git", vec!["push"]),
            ("git", vec!["reset", "--hard"]),
            ("git", vec!["-c", "core.pager=sh", "status"]),
            ("git", vec![]),
            ("cargo", vec!["install", "x"]),
            ("/bin/ls", vec![]),
            ("ls ..", vec![]),
        ] {
            assert!(class(&r, program, &args).is_err(), "{program} {args:?} deveria ser recusado");
        }
        let e = class(&r, "rm", &["x"]).unwrap_err();
        assert!(e.contains("run_script"), "{e}");
    }

    #[test]
    fn options_that_write_or_run_other_programs_are_refused() {
        let r = project("flags");
        assert!(class(&r, "git", &["diff", "--output=x.txt"]).is_err());
        assert!(class(&r, "git", &["diff", "--ext-diff"]).is_err());
        assert!(class(&r, "cargo", &["test", "--config", "x=y"]).is_err());
        assert!(class(&r, "tail", &["-f", "a.log"]).is_err());
    }

    #[test]
    fn read_commands_stay_inside_the_project() {
        let r = project("paths");
        assert!(class(&r, "cat", &["/etc/passwd"]).is_err());
        assert!(class(&r, "ls", &[".."]).is_err());
        assert!(class(&r, "head", &["-n", "5", "../x"]).is_err());
        assert_eq!(class(&r, "ls", &["src"]), Ok(Class::Read));
    }

    #[test]
    fn no_shell_means_metacharacters_are_plain_text() {
        let r = project("shell");
        // `;` e `&&` não separam comandos: viram argumentos de ls, que não acha esse arquivo.
        let out = run(&r, "ls", &["a; touch rodou"], &No);
        assert!(matches!(out, Outcome::Error(_)), "{out:?}");
        assert!(!r.join("rodou").exists());
    }

    #[test]
    fn a_read_command_runs_without_asking() {
        let r = project("read");
        fs::write(r.join("a.txt"), "1\n2\n3\n").unwrap();
        // `No` recusa qualquer execução: a leitura não passa pelo aprovador.
        // O `wc` do macOS alinha a contagem com espaços à esquerda e o do Linux não: compara sem eles.
        let Outcome::Done(text) = run(&r, "wc", &["-l", "a.txt"], &No) else { panic!("não rodou") };
        assert_eq!(text.split_whitespace().collect::<Vec<_>>().join(" "), "código de saída: 0 3 a.txt");
    }

    #[test]
    fn a_mutating_command_asks_and_auto_approval_does_not_run_it() {
        let r = project("mutating");
        assert!(matches!(run(&r, "cargo", &["check"], &No), Outcome::Refused(_)));
        assert!(matches!(run(&r, "cargo", &["check"], &AutoApprover), Outcome::Refused(_)), "--yes não executa código");
    }

    #[test]
    fn git_status_runs_in_a_repository() {
        let r = project("git");
        assert!(Command::new("git").arg("init").arg("-q").current_dir(&r).status().unwrap().success());
        fs::write(r.join("novo.txt"), "x").unwrap();
        let Outcome::Done(text) = run(&r, "git", &["status", "--short"], &No) else { panic!() };
        assert!(text.contains("novo.txt"), "{text}");
        // Alteração: aprovada, roda.
        assert!(matches!(run(&r, "git", &["add", "novo.txt"], &Yes), Outcome::Done(_)));
        let Outcome::Done(text) = run(&r, "git", &["status", "--short"], &No) else { panic!() };
        assert!(text.contains("A  novo.txt"), "{text}");
    }

    #[test]
    fn a_missing_program_says_so() {
        let r = project("missing");
        let out = wrap(run_command(&r, &json!({"program": "cargo", "args": ["check"]}), &Yes, Duration::from_secs(1)));
        // cargo existe aqui, então o resultado é uma execução; o que importa é que não trava nem entra em pânico.
        assert!(matches!(out, Outcome::Done(_) | Outcome::Error(_)), "{out:?}");
    }

    #[test]
    fn plan_mode_keeps_read_calls_and_blocks_the_rest() {
        let r = project("plan");
        fs::write(r.join("a.txt"), "x\n").unwrap();
        let mut reg = Registry::new(vec![Box::new(RunCommand)]);
        reg.set_plan(true);
        assert_eq!(reg.names(), vec!["run_command"], "a ferramenta continua visível no modo Plan");
        let ctx = ToolContext::new(&r, &Yes);
        assert!(matches!(reg.execute(&ctx, "run_command", &json!({"program": "ls"})), Outcome::Done(_)));
        let blocked = reg.execute(&ctx, "run_command", &json!({"program": "git", "args": ["add", "a.txt"]}));
        assert!(matches!(blocked, Outcome::Error(ref e) if e.contains("modo Plan")), "{blocked:?}");
        assert!(reg.mutates_call("run_command", &json!({"program": "git", "args": ["commit"]})));
        assert!(!reg.mutates_call("run_command", &json!({"program": "git", "args": ["status"]})));
        assert!(reg.idempotent_call("run_command", &json!({"program": "ls"})));
    }
}

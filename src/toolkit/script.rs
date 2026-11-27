//! Execução de scripts: `run_script` aplica `chmod u+x` num script do projeto e o roda com prazo,
//! saída limitada e ambiente reduzido.
//!
//! É a única ferramenta que executa código, então pede aprovação sempre: `Approver::approve_run`
//! não é concedido por `--yes`. O script roda com as permissões do usuário, sem isolamento. O que
//! esta ferramenta garante é que o usuário só aprova o que vê e que o processo não fica pendurado.

use crate::core::sandbox::resolve;
use crate::core::tool::{Approver, Outcome, Preview, PreviewLine, Tool, ToolContext, ToolSpec, str_arg, wrap};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::time::Instant;

// Tempo máximo de um script. Passou disso, o processo e o grupo dele são encerrados.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(60);
// Quanto da saída volta ao modelo (e à conversa). O resto é cortado com um aviso.
const MAX_OUTPUT_BYTES: usize = 6_000;
// Tamanho máximo do script: o usuário precisa conseguir revisá-lo inteiro antes de aprovar.
const MAX_SCRIPT_LINES: usize = 120;
const MAX_SCRIPT_BYTES: u64 = 32 * 1024;
// Variáveis de ambiente que o script herda. O resto (chaves de API, tokens) não passa.
#[cfg(unix)]
const KEPT_ENV: &[&str] = &["PATH", "HOME", "USER", "LANG", "LC_ALL", "TMPDIR"];
// Quanto de cada fluxo o BitClaw guarda na memória; o excedente é lido e descartado.
#[cfg(unix)]
const MAX_CAPTURE_BYTES: usize = 64 * 1024;

pub struct RunScript;

impl Tool for RunScript {
    fn name(&self) -> &'static str {
        "run_script"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "run_script",
            "Executa um script que JÁ EXISTE no projeto: aplica chmod u+x e o roda com a pasta do projeto como diretório atual. Crie o script antes com create_file, começando por um shebang como #!/bin/bash. O usuário vê o script inteiro e confirma antes de executar. Devolve o código de saída e a saída real (stdout e stderr); o tempo máximo é de 60 s. Chamada exata, com estes nomes: {\"path\": \"scripts/contar.sh\", \"args\": [\".\"]}. Sem argumentos, omita args.",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Caminho do script, relativo ao projeto"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Argumentos opcionais, um texto por item"}
                },
                "required": ["path"]
            }),
        )
    }

    fn run(&self, ctx: &ToolContext, args: &Value) -> Outcome {
        wrap(run_script(ctx.root, args, ctx.approver, RUN_TIMEOUT))
    }
}

// Troca caracteres de controle por `\xNN`: um script não pode esconder texto na prévia com `\r`
// ou sequências de escape do terminal.
pub(super) fn visible(line: &str) -> String {
    line.chars()
        .map(|c| if c.is_control() && c != '\t' { format!("\\x{:02x}", c as u32) } else { c.to_string() })
        .collect()
}

fn run_preview(rel: &str, extra: &[String], content: &str) -> Preview {
    let interpreter = interpreter_for(rel, content);
    let program = match interpreter {
        Some(i) => format!("{i} {rel}"),
        None => format!("./{rel}"),
    };
    let command = if extra.is_empty() { program } else { format!("{program} {}", visible(&extra.join(" "))) };
    let mut lines = Vec::new();
    if interpreter.is_none() {
        lines.push(PreviewLine::Context(format!("$ chmod u+x {rel}")));
    }
    lines.push(PreviewLine::Context(format!("$ {command}")));
    lines.push(PreviewLine::Context(String::new()));
    lines.extend(content.lines().map(|l| PreviewLine::Context(visible(l))));
    lines.push(PreviewLine::Note("o script roda com as suas permissões, sem isolamento: confira o conteúdo antes de aceitar.".into()));
    Preview::new(format!("executar {rel}"), lines)
}

// Script sem shebang de uma linguagem conhecida: roda com o interpretador da extensão. Sem isso, o sistema
// o entregaria ao `sh`, e um `.py` daria erro de sintaxe de shell (os modelos pequenos esquecem o shebang).
// Com shebang, ele decide.
fn interpreter_for(rel: &str, content: &str) -> Option<&'static str> {
    if content.starts_with("#!") {
        return None;
    }
    match Path::new(rel).extension().and_then(|e| e.to_str()) {
        Some("py") => Some("python3"),
        Some("js" | "mjs") => Some("node"),
        Some("rb") => Some("ruby"),
        Some("sh") => Some("bash"),
        _ => None,
    }
}

// Shebang sugerido pela extensão do script, para o erro dizer a linha exata que falta.
fn shebang_for(rel: &str) -> &'static str {
    match Path::new(rel).extension().and_then(|e| e.to_str()) {
        Some("py") => "#!/usr/bin/env python3",
        Some("js" | "mjs") => "#!/usr/bin/env node",
        Some("rb") => "#!/usr/bin/env ruby",
        _ => "#!/bin/bash",
    }
}

fn run_script(root: &Path, args: &Value, approver: &dyn Approver, limit: Duration) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let extra: Vec<String> = match args.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or("'args' deve ter só textos"))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("'args' deve ser uma lista de textos".into()),
    };
    let path = resolve(root, rel)?;
    let meta = fs::metadata(&path).map_err(|_| format!("{rel} não existe: crie o script antes com create_file"))?;
    if !meta.is_file() {
        return Err(format!("{rel} não é um arquivo"));
    }
    if meta.len() > MAX_SCRIPT_BYTES {
        return Err(format!("{rel} tem {} bytes; o máximo é {MAX_SCRIPT_BYTES}. Divida em scripts menores", meta.len()));
    }
    let content = fs::read_to_string(&path).map_err(|_| format!("{rel} não é um arquivo de texto"))?;
    let total = content.lines().count();
    if total > MAX_SCRIPT_LINES {
        return Err(format!("{rel} tem {total} linhas; o máximo é {MAX_SCRIPT_LINES} para o usuário conseguir revisar. Divida em scripts menores"));
    }

    if !approver.approve_run(&run_preview(rel, &extra, &content)) {
        return Ok(Outcome::Refused("usuário recusou a execução; nada foi executado".into()));
    }
    // O que foi aprovado é o que roda: se o arquivo mudou no meio do caminho, não executa.
    if fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
        return Err(format!("{rel} mudou depois da aprovação; nada foi executado"));
    }
    execute(root, rel, &path, interpreter_for(rel, &content), &extra, limit)
}

#[cfg(not(unix))]
pub(super) fn run_process(
    _command: std::process::Command,
    _root: &Path,
    _limit: Duration,
    _spawn_error: impl FnOnce(&std::io::Error) -> String,
) -> Result<Outcome, String> {
    Err("executar comandos só funciona em macOS e Linux".into())
}

#[cfg(not(unix))]
fn execute(_root: &Path, _rel: &str, _path: &Path, _interpreter: Option<&str>, _extra: &[String], _limit: Duration) -> Result<Outcome, String> {
    Err("run_script só funciona em macOS e Linux".into())
}

#[cfg(unix)]
fn execute(root: &Path, rel: &str, path: &Path, interpreter: Option<&str>, extra: &[String], limit: Duration) -> Result<Outcome, String> {
    use std::os::unix::fs::PermissionsExt;

    let mut command = match interpreter {
        // Pelo interpretador: o arquivo é lido, não executado, então não precisa de chmod.
        Some(program) => {
            let mut c = Command::new(program);
            c.arg(path);
            c
        }
        None => {
            // `chmod u+x`: só o dono ganha permissão de execução.
            let mut permissions = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
            permissions.set_mode(permissions.mode() | 0o100);
            fs::set_permissions(path, permissions).map_err(|e| format!("não consegui aplicar chmod em {rel}: {e}"))?;
            Command::new(path)
        }
    };
    command.args(extra);
    run_process(command, root, limit, |e| match e.raw_os_error() {
        // ENOEXEC: sem shebang, o sistema não sabe qual interpretador usar.
        Some(8) => format!("{rel} não pôde ser executado: a primeira linha precisa ser um shebang, como {}", shebang_for(rel)),
        _ if interpreter.is_some() => format!("não consegui executar {rel} com {}: {e}", interpreter.unwrap_or("")),
        _ => format!("não consegui executar {rel}: {e}"),
    })
}

// Roda um processo já montado (programa e argumentos) como o `run_script` roda um script: na pasta do
// projeto, com ambiente reduzido, sem stdin, em grupo de processos próprio, com prazo e saída limitada.
// `spawn_error` diz, em português, por que o processo não subiu.
#[cfg(unix)]
pub(super) fn run_process(
    mut command: Command,
    root: &Path,
    limit: Duration,
    spawn_error: impl FnOnce(&std::io::Error) -> String,
) -> Result<Outcome, String> {
    use std::os::unix::process::CommandExt;

    command
        .current_dir(root)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Grupo próprio: no prazo, o processo e o que ele iniciou saem juntos.
        .process_group(0);
    for key in KEPT_ENV {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn().map_err(|e| spawn_error(&e))?;

    let stdout = capture(child.stdout.take());
    let stderr = capture(child.stderr.take());
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if started.elapsed() >= limit => {
                timed_out = true;
                kill_group(&mut child);
                break child.wait().map_err(|e| e.to_string())?;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // Os leitores terminam quando os fluxos fecham. Um processo em segundo plano pode mantê-los
    // abertos, então a espera tem prazo e o que já foi lido é aproveitado.
    let grace = Instant::now();
    while !(stdout.finished() && stderr.finished()) && grace.elapsed() < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = join_output(&stdout.bytes(), &stderr.bytes());

    Ok(match (status.code(), timed_out) {
        (_, true) => Outcome::Error(format!("tempo esgotado ({} s): o processo foi interrompido\n{output}", limit.as_secs())),
        (Some(0), _) => Outcome::Done(format!("código de saída: 0\n{output}")),
        (Some(code), _) => Outcome::Error(format!("saiu com código {code}\n{output}")),
        (None, _) => Outcome::Error(format!("encerrado por um sinal\n{output}")),
    })
}

// Leitura de um fluxo numa thread: guarda até `MAX_CAPTURE_BYTES` e descarta o resto, para que
// um script muito falante não trave por falta de quem leia.
#[cfg(unix)]
struct Capture {
    buffer: Arc<Mutex<Vec<u8>>>,
    handle: std::thread::JoinHandle<()>,
}

#[cfg(unix)]
impl Capture {
    fn finished(&self) -> bool {
        self.handle.is_finished()
    }

    fn bytes(&self) -> Vec<u8> {
        self.buffer.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(unix)]
fn capture<R: Read + Send + 'static>(pipe: Option<R>) -> Capture {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buffer);
    let handle = std::thread::spawn(move || {
        let Some(mut pipe) = pipe else { return };
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut buf = sink.lock().unwrap_or_else(|e| e.into_inner());
                    let room = MAX_CAPTURE_BYTES.saturating_sub(buf.len());
                    buf.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
        }
    });
    Capture { buffer, handle }
}

// Encerra o grupo de processos do script. `kill -KILL -- -PID` atinge o grupo inteiro.
#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).status();
    let _ = child.kill();
}

// Junta stdout e stderr num texto só, cortado em `MAX_OUTPUT_BYTES`.
fn join_output(stdout: &[u8], stderr: &[u8]) -> String {
    let out = String::from_utf8_lossy(stdout);
    let err = String::from_utf8_lossy(stderr);
    let mut text = out.trim_end().to_string();
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("[stderr]\n");
        text.push_str(err.trim_end());
    }
    if text.is_empty() {
        return "(sem saída)".into();
    }
    if text.len() > MAX_OUTPUT_BYTES {
        let mut cut = MAX_OUTPUT_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let omitted = text.len() - cut;
        text.truncate(cut);
        text.push_str(&format!("\n[saída cortada: {omitted} bytes omitidos]"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tool::AutoApprover;
    use std::cell::RefCell;

    // Aprova a execução e guarda a prévia que viu.
    struct Yes(RefCell<Option<Preview>>);
    impl Approver for Yes {
        fn approve(&self, _p: &Preview) -> bool {
            true
        }
        fn approve_run(&self, p: &Preview) -> bool {
            *self.0.borrow_mut() = Some(p.clone());
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

    fn yes() -> Yes {
        Yes(RefCell::new(None))
    }

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-run-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn run(root: &Path, args: Value, approver: &dyn Approver, limit: Duration) -> Outcome {
        wrap(run_script(root, &args, approver, limit))
    }

    fn script(root: &Path, name: &str, body: &str) {
        fs::write(root.join(name), body).unwrap();
    }

    #[test]
    fn it_makes_the_script_executable_runs_it_and_returns_the_real_output() {
        let root = project("ok");
        script(&root, "ola.sh", "#!/bin/bash\necho \"Olá, mundo!\"\n");
        let approver = yes();
        let out = run(&root, json!({"path": "ola.sh"}), &approver, RUN_TIMEOUT);
        assert_eq!(out, Outcome::Done("código de saída: 0\nOlá, mundo!".into()));
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(fs::metadata(root.join("ola.sh")).unwrap().permissions().mode() & 0o100, 0, "chmod u+x aplicado");
        let preview = approver.0.borrow().clone().expect("o usuário viu uma prévia");
        assert_eq!(preview.title, "executar ola.sh");
        assert!(preview.lines.contains(&PreviewLine::Context("echo \"Olá, mundo!\"".into())), "a prévia traz o script");
        assert!(preview.lines.contains(&PreviewLine::Context("$ chmod u+x ola.sh".into())));
    }

    #[test]
    fn the_script_runs_in_the_project_folder_with_its_arguments() {
        let root = project("args");
        script(&root, "info.sh", "#!/bin/sh\npwd\necho \"$1-$2\"\n");
        let out = run(&root, json!({"path": "info.sh", "args": ["a", "b"]}), &yes(), RUN_TIMEOUT);
        assert_eq!(out, Outcome::Done(format!("código de saída: 0\n{}\na-b", root.display())));
    }

    #[test]
    fn a_failing_script_reports_the_exit_code_and_stderr() {
        let root = project("fail");
        script(&root, "f.sh", "#!/bin/sh\necho antes\necho oops >&2\nexit 3\n");
        let Outcome::Error(text) = run(&root, json!({"path": "f.sh"}), &yes(), RUN_TIMEOUT) else { panic!("deveria falhar") };
        assert!(text.starts_with("saiu com código 3\n"), "{text}");
        assert!(text.contains("antes") && text.contains("[stderr]\noops"), "{text}");
    }

    #[test]
    fn a_script_past_the_deadline_is_killed_with_its_children() {
        let root = project("slow");
        script(&root, "s.sh", "#!/bin/sh\nsleep 30 &\nsleep 30\n");
        let started = Instant::now();
        let Outcome::Error(text) = run(&root, json!({"path": "s.sh"}), &yes(), Duration::from_millis(400)) else { panic!("deveria estourar") };
        assert!(text.starts_with("tempo esgotado"), "{text}");
        assert!(started.elapsed() < Duration::from_secs(5), "não esperou os 30 s: {:?}", started.elapsed());
    }

    #[test]
    fn the_user_can_refuse_and_nothing_runs() {
        let root = project("no");
        script(&root, "x.sh", "#!/bin/sh\ntouch rodou\n");
        let out = run(&root, json!({"path": "x.sh"}), &No, RUN_TIMEOUT);
        assert!(matches!(out, Outcome::Refused(_)), "{out:?}");
        assert!(!root.join("rodou").exists(), "o script não executou");
    }

    #[test]
    fn auto_approval_never_runs_code() {
        let root = project("auto");
        script(&root, "x.sh", "#!/bin/sh\ntouch rodou\n");
        let out = run(&root, json!({"path": "x.sh"}), &AutoApprover, RUN_TIMEOUT);
        assert!(matches!(out, Outcome::Refused(_)), "--yes não executa código: {out:?}");
        assert!(!root.join("rodou").exists());
    }

    #[test]
    fn only_scripts_inside_the_project_that_exist_can_run() {
        let root = project("paths");
        assert!(matches!(run(&root, json!({"path": "../fora.sh"}), &yes(), RUN_TIMEOUT), Outcome::Error(_)));
        assert!(matches!(run(&root, json!({"path": "/bin/ls"}), &yes(), RUN_TIMEOUT), Outcome::Error(_)));
        let Outcome::Error(text) = run(&root, json!({"path": "nao.sh"}), &yes(), RUN_TIMEOUT) else { panic!() };
        assert!(text.contains("create_file"), "diz como criar: {text}");
        assert!(matches!(run(&root, json!({}), &yes(), RUN_TIMEOUT), Outcome::Error(_)));
        assert!(matches!(run(&root, json!({"path": "a.sh", "args": "texto"}), &yes(), RUN_TIMEOUT), Outcome::Error(_)));
    }

    #[test]
    fn a_path_with_control_characters_never_reaches_the_approval() {
        let root = project("ctrl");
        let approver = yes();
        let out = run(&root, json!({"path": "x\x1b[2J.sh"}), &approver, RUN_TIMEOUT);
        assert!(matches!(out, Outcome::Error(_)), "{out:?}");
        assert!(approver.0.borrow().is_none(), "o usuário não chegou a ver uma prévia com o caminho forjado");
    }

    #[test]
    fn a_script_too_long_to_review_is_not_run() {
        let root = project("long");
        script(&root, "g.sh", &format!("#!/bin/sh\n{}", "echo oi\n".repeat(MAX_SCRIPT_LINES)));
        let Outcome::Error(text) = run(&root, json!({"path": "g.sh"}), &yes(), RUN_TIMEOUT) else { panic!("deveria recusar") };
        assert!(text.contains("revisar"), "{text}");
    }

    #[test]
    fn control_characters_cannot_hide_text_in_the_preview() {
        assert_eq!(visible("ls\r# inocente"), "ls\\x0d# inocente");
        assert_eq!(visible("\x1b[2Joculto"), "\\x1b[2Joculto");
        assert_eq!(visible("com\ttab"), "com\ttab", "tab é normal");
        let preview = run_preview("a.sh", &[], "echo ok\x1b[8m");
        assert!(preview.lines.contains(&PreviewLine::Context("echo ok\\x1b[8m".into())));
    }

    #[test]
    fn a_script_without_shebang_runs_with_the_interpreter_of_its_extension() {
        let root = project("interp");
        script(&root, "conta.py", "taxa = 960 / (8 * 6 * 10)\nprint(int(12 * 8 * 15 * taxa))\n");
        let out = run(&root, json!({"path": "conta.py"}), &yes(), RUN_TIMEOUT);
        match out {
            Outcome::Done(text) => assert!(text.contains("2880"), "{text}"),
            other => panic!("esperava Done: {other:?}"),
        }
        let preview = run_preview("conta.py", &[], "print(1)");
        assert!(preview.lines.contains(&PreviewLine::Context("$ python3 conta.py".into())), "{:?}", preview.lines);
        assert!(!preview.lines.iter().any(|l| matches!(l, PreviewLine::Context(t) if t.contains("chmod"))));
        // Com shebang, quem decide é ele.
        assert_eq!(interpreter_for("a.py", "#!/usr/bin/env python3\nprint(1)"), None);
        assert_eq!(interpreter_for("dados.txt", "x"), None);
    }

    #[test]
    fn the_missing_shebang_is_suggested_by_extension() {
        assert_eq!(shebang_for("problema.py"), "#!/usr/bin/env python3");
        assert_eq!(shebang_for("app.js"), "#!/usr/bin/env node");
        assert_eq!(shebang_for("info.sh"), "#!/bin/bash");
        assert_eq!(shebang_for("sem_extensao"), "#!/bin/bash");
    }

    #[test]
    fn a_script_without_shebang_explains_what_is_missing() {
        let root = project("noshebang");
        script(&root, "n.sh", "echo sem shebang\n");
        let out = run(&root, json!({"path": "n.sh"}), &yes(), RUN_TIMEOUT);
        // Alguns sistemas recusam (ENOEXEC) e outros caem no `sh`; os dois são aceitáveis, mas a recusa tem de ser clara.
        match out {
            Outcome::Error(text) => assert!(text.contains("shebang"), "{text}"),
            Outcome::Done(text) => assert!(text.contains("sem shebang"), "{text}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn long_output_is_cut_and_says_how_much_was_dropped() {
        let root = project("big");
        script(&root, "b.sh", "#!/bin/sh\nyes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 20000\n");
        let Outcome::Done(text) = run(&root, json!({"path": "b.sh"}), &yes(), RUN_TIMEOUT) else { panic!() };
        assert!(text.len() < MAX_OUTPUT_BYTES + 200, "{}", text.len());
        assert!(text.contains("[saída cortada:"), "{}", &text[text.len() - 80..]);
    }

    #[test]
    fn the_script_does_not_inherit_secrets_from_the_environment() {
        let root = project("env");
        script(&root, "e.sh", "#!/bin/sh\nenv | cut -d= -f1 | sort\n");
        let Outcome::Done(text) = run(&root, json!({"path": "e.sh"}), &yes(), RUN_TIMEOUT) else { panic!() };
        let names: Vec<&str> = text.lines().skip(1).collect();
        assert!(names.contains(&"PATH"), "{names:?}");
        assert!(names.iter().all(|n| KEPT_ENV.contains(n) || ["PWD", "SHLVL", "_", "OLDPWD", "__CF_USER_TEXT_ENCODING", "XPC_FLAGS", "XPC_SERVICE_NAME", "LOGNAME", "SHELL", "TERM"].contains(n)), "variáveis inesperadas: {names:?}");
    }
}

mod ollama;
mod sandbox;
mod tools;

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const DEFAULT_MODEL: &str = "qwen2.5-coder:3b";
const DEFAULT_URL: &str = "http://localhost:11434";
const MAX_STEPS: usize = 8;

const SYSTEM_PROMPT: &str = "Você é o BitClaw, um assistente que lê e edita arquivos de um projeto local. \
Use as ferramentas para inspecionar o projeto antes de responder. \
Antes de usar edit_file, leia o arquivo com read_file e copie o trecho exato. \
Faça uma edição por vez e explique o que mudou. Responda em português, de forma curta.";

struct Args {
    model: String,
    url: String,
    dir: PathBuf,
    auto_yes: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        model: DEFAULT_MODEL.into(),
        url: DEFAULT_URL.into(),
        dir: std::env::current_dir().map_err(|e| e.to_string())?,
        auto_yes: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--model" => args.model = it.next().ok_or("--model precisa de valor")?,
            "--url" => args.url = it.next().ok_or("--url precisa de valor")?,
            "--dir" => args.dir = PathBuf::from(it.next().ok_or("--dir precisa de valor")?),
            "--yes" => args.auto_yes = true,
            "-h" | "--help" => {
                println!("uso: bitclaw [--model M] [--url U] [--dir D] [--yes]");
                std::process::exit(0);
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(args)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("erro: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let root = args.dir.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
    let client = ollama::Client::new(args.url.clone(), args.model.clone())?;
    let tools = tools::schemas();

    println!("BitClaw · modelo {} · projeto {}", args.model, root.display());
    if args.auto_yes {
        println!("aviso: --yes ativo, edições serão aplicadas sem confirmação");
    }
    println!("Digite uma tarefa. /sair encerra.\n");

    let mut messages = vec![json!({"role": "system", "content": SYSTEM_PROMPT})];
    let stdin = io::stdin();
    loop {
        print!("bitclaw> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break; // EOF
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/sair" {
            break;
        }
        messages.push(json!({"role": "user", "content": input}));

        if let Err(e) = agent_turn(&client, &tools, &root, &mut messages, args.auto_yes) {
            eprintln!("erro: {e}");
        }
    }
    Ok(())
}

/// Roda o ciclo modelo → ferramentas → modelo até a resposta final.
fn agent_turn(
    client: &ollama::Client,
    tools: &Value,
    root: &PathBuf,
    messages: &mut Vec<Value>,
    auto_yes: bool,
) -> Result<(), String> {
    let mut last_call: Option<String> = None;
    let mut repeats = 0;
    let mut applied_edits = std::collections::HashSet::new();
    for _ in 0..MAX_STEPS {
        let msg = client.chat(messages, tools)?;
        messages.push(msg.clone());

        let mut calls = msg.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        if calls.is_empty() {
            calls = calls_from_text(msg.get("content").and_then(Value::as_str).unwrap_or(""));
        }
        if calls.is_empty() {
            let text = msg.get("content").and_then(Value::as_str).unwrap_or("").trim();
            println!("\n{}\n", if text.is_empty() { "(sem resposta)" } else { text });
            return Ok(());
        }

        for call in calls {
            let func = call.get("function").cloned().unwrap_or(Value::Null);
            let name = func.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let args = normalize_args(func.get("arguments"));
            eprintln!("· {name} {}", args);
            // Mesma chamada repetida: o modelo está preso, interrompe.
            let key = format!("{name}{args}");
            if last_call.as_deref() == Some(key.as_str()) {
                repeats += 1;
                if repeats >= 2 {
                    return Err("o modelo repetiu a mesma chamada; interrompendo".into());
                }
            } else {
                repeats = 0;
            }
            last_call = Some(key.clone());
            // Edição idêntica já feita neste turno: não reaplica.
            let result = if name == "edit_file" && !applied_edits.insert(key.clone()) {
                "esta edição já foi aplicada neste turno. Responda ao usuário sem chamar mais ferramentas.".to_string()
            } else {
                tools::execute(root, &name, &args, auto_yes)
            };
            messages.push(json!({"role": "tool", "tool_name": name, "content": result}));
        }
    }
    Err(format!("limite de {MAX_STEPS} passos atingido sem resposta final"))
}

const KNOWN_TOOLS: [&str; 3] = ["list_dir", "read_file", "edit_file"];

/// Fallback para modelos pequenos que escrevem a chamada como texto JSON,
/// ex.: {"name": "read_file", "arguments": {"path": "a.txt"}}.
/// Só aceita nomes de ferramentas conhecidas, então texto comum não vira ação.
fn calls_from_text(content: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let name = v.get("name").and_then(Value::as_str).unwrap_or("");
        if KNOWN_TOOLS.contains(&name) {
            if let Some(args) = v.get("arguments").cloned() {
                out.push(json!({"function": {"name": name, "arguments": args}}));
            }
        }
    }
    out
}

/// Alguns modelos devolvem `arguments` como objeto, outros como string JSON.
fn normalize_args(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
        Some(v) => v.clone(),
        None => json!({}),
    }
}

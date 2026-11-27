//! Chamadas de ferramenta que o modelo escreve como texto, e a normalização dos argumentos.

use serde_json::{json, Map, Value};

const QUOTE: &str = "<|\"|>";

// Fallback para modelos que escrevem a chamada como texto. Três formatos:
//
// 1. JSON, ex.: {"name": "read_file", "arguments": {"path": "a.txt"}}.
//    Aceita qualquer nome: se a ferramenta não existir, o erro é devolvido ao modelo e ao usuário.
//
// 2. Formato de tokens especiais sem os delimitadores do modelo, ex.:
//    read_file{path:<|"|>a.txt<|"|>}. Só vale para nomes de ferramentas conhecidas
//    e exige a linha inteira como chamadas, para que uma frase comum não vire ação.
//    Várias chamadas coladas na mesma linha (`a{..}b{..}`) valem, na ordem.
//
// 3. Estilo Python, ex.: create_file(path="a.py", content="print(1)\n"). Só vale para ferramentas
//    conhecidas e quando a resposta inteira são chamadas, podendo ocupar várias linhas.
//
// Devolve as chamadas e o texto das linhas que não eram chamadas.
pub(super) fn calls_from_text(content: &str, known: &[&str]) -> (Vec<Value>, String) {
    let mut calls = Vec::new();
    let mut rest: Vec<&str> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let found = if trimmed.starts_with('{') { json_call(trimmed).map(|c| vec![c]) } else { tagged_calls(trimmed, known) };
        match found {
            Some(mut found) => calls.append(&mut found),
            None => rest.push(line),
        }
    }
    if calls.is_empty()
        && let Some(found) = paren_calls(strip_fence(content.trim()), known)
    {
        return (found, String::new());
    }
    (calls, rest.join("\n").trim().to_string())
}

// Tira a cerca ``` em volta da resposta, se houver: alguns modelos embrulham a chamada em um bloco de código.
fn strip_fence(text: &str) -> &str {
    let Some(inner) = text.strip_prefix("```").and_then(|t| t.strip_suffix("```")) else { return text };
    // A primeira linha de dentro é só o nome da linguagem (ou vazia).
    inner.split_once('\n').map_or(inner, |(_, body)| body).trim()
}

// Uma ou mais chamadas `nome(chave="valor", ...)` que são o texto todo, separadas por espaço ou quebra de linha.
fn paren_calls(text: &str, known: &[&str]) -> Option<Vec<Value>> {
    let mut rest = text;
    let mut calls = Vec::new();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim();
        if !known.contains(&name) {
            return None;
        }
        let (args, after) = parse_kwargs(&rest[open + 1..])?;
        calls.push(call(name, Value::Object(args)));
        rest = after.trim_start();
    }
    (!calls.is_empty()).then_some(calls)
}

// Lê `chave=valor, ...)` e devolve os argumentos e o que vem depois do `)`.
fn parse_kwargs(mut body: &str) -> Option<(Map<String, Value>, &str)> {
    let mut args = Map::new();
    body = body.trim_start();
    if let Some(rest) = body.strip_prefix(')') {
        return Some((args, rest));
    }
    loop {
        let eq = body.find('=')?;
        let key = body[..eq].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        body = body[eq + 1..].trim_start();
        let (value, rest) = if body.starts_with(['"', '\'']) {
            let (text, rest) = quoted(body)?;
            (Value::String(text), rest)
        } else {
            let end = body.find([',', ')'])?;
            (python_value(body[..end].trim())?, &body[end..])
        };
        args.insert(key.to_string(), value);
        body = rest.trim_start();
        if let Some(after) = body.strip_prefix(')') {
            return Some((args, after));
        }
        body = body.strip_prefix(',')?.trim_start();
    }
}

// Texto entre aspas simples ou duplas, com os escapes comuns. Quebra de linha de verdade dentro do
// texto vale como está. Devolve o texto e o que vem depois da aspa de fechar.
fn quoted(text: &str) -> Option<(String, &str)> {
    let mut chars = text.char_indices();
    let (_, quote) = chars.next()?;
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            c if c == quote => return Some((out, &text[i + c.len_utf8()..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                c @ ('\\' | '"' | '\'' | '/') => out.push(c),
                // Escape que não conhecemos (ex.: `\d` num regex): fica como o modelo escreveu.
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

fn python_value(raw: &str) -> Option<Value> {
    match raw {
        "True" => Some(Value::Bool(true)),
        "False" => Some(Value::Bool(false)),
        "None" => Some(Value::Null),
        _ => plain_value(raw),
    }
}

fn json_call(line: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    let name = v.get("name")?.as_str()?;
    let args = v.get("arguments")?.clone();
    Some(call(name, args))
}

// Uma ou mais chamadas `nome{chave:valor,...}` que ocupam a linha toda. Valores entre <|"|> são
// texto; sem aspas, valores simples (número, true, false, null) viram JSON. Outra forma é recusada.
fn tagged_calls(line: &str, known: &[&str]) -> Option<Vec<Value>> {
    let mut rest = line;
    let mut calls = Vec::new();
    while !rest.is_empty() {
        let (call, after) = tagged_call(rest, known)?;
        calls.push(call);
        rest = after.trim_start();
    }
    (!calls.is_empty()).then_some(calls)
}

fn tagged_call<'a>(text: &'a str, known: &[&str]) -> Option<(Value, &'a str)> {
    let open = text.find('{')?;
    let name = text[..open].trim();
    if !known.contains(&name) {
        return None;
    }
    let (args, rest) = parse_args(&text[open + 1..])?;
    Some((call(name, Value::Object(args)), rest))
}

// Lê `chave:valor,...}` e devolve os argumentos e o que vem depois do `}`.
fn parse_args(mut body: &str) -> Option<(Map<String, Value>, &str)> {
    let mut args = Map::new();
    body = body.trim_start();
    if let Some(rest) = body.strip_prefix('}') {
        return Some((args, rest));
    }
    loop {
        let colon = body.find(':')?;
        let key = body[..colon].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        body = body[colon + 1..].trim_start();
        let (value, rest) = if let Some(after) = body.strip_prefix(QUOTE) {
            let end = after.find(QUOTE)?;
            (Value::String(after[..end].to_string()), &after[end + QUOTE.len()..])
        } else {
            let end = body.find([',', '}'])?;
            (plain_value(body[..end].trim())?, &body[end..])
        };
        args.insert(key.to_string(), value);
        body = rest.trim_start();
        if let Some(after) = body.strip_prefix('}') {
            return Some((args, after));
        }
        // Vírgula final (`a:1,}`) cai aqui e falha: a próxima chave não existe.
        body = body.strip_prefix(',')?.trim_start();
    }
}

fn plain_value(raw: &str) -> Option<Value> {
    match raw {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        "null" => Some(Value::Null),
        _ => raw.parse::<i64>().ok().map(Value::from),
    }
}

fn call(name: &str, args: Value) -> Value {
    json!({"function": {"name": name, "arguments": args}})
}

// Alguns modelos devolvem `arguments` como objeto, outros como string JSON.
pub(super) fn normalize_args(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(json!({})),
        Some(v) => v.clone(),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &["list_dir", "read_file", "create_file"];

    fn one(line: &str) -> Option<Value> {
        calls_from_text(line, KNOWN).0.into_iter().next()
    }

    #[test]
    fn nemotron_style_call_is_recognized() {
        let c = one(r#"list_dir{path:<|"|>src/<|"|>}"#).unwrap();
        assert_eq!(c["function"]["name"], "list_dir");
        assert_eq!(c["function"]["arguments"], json!({"path": "src/"}));
    }

    #[test]
    fn several_arguments_and_commas_inside_values() {
        let c = one(r#"create_file{path:<|"|>a.txt<|"|>,content:<|"|>oi, tudo bem<|"|>}"#).unwrap();
        assert_eq!(c["function"]["arguments"], json!({"path": "a.txt", "content": "oi, tudo bem"}));
    }

    #[test]
    fn plain_scalars_are_typed() {
        let c = one("read_file{start_line:10,end_line:20,mode:<|\"|>outline<|\"|>}").unwrap();
        assert_eq!(c["function"]["arguments"], json!({"start_line": 10, "end_line": 20, "mode": "outline"}));
    }

    #[test]
    fn unknown_name_in_tagged_form_is_not_a_call() {
        assert!(one(r#"make_file{path:<|"|>a.py<|"|>}"#).is_none());
    }

    #[test]
    fn sentence_mentioning_a_call_is_not_a_call() {
        assert!(one(r#"Use list_dir{path:<|"|>src/<|"|>} para listar."#).is_none());
        assert!(one(r#"list_dir{path:<|"|>src/<|"|>} e depois responda"#).is_none());
    }

    #[test]
    fn malformed_tagged_call_is_ignored() {
        assert!(one(r#"list_dir{path:<|"|>src/}"#).is_none());
        assert!(one(r#"list_dir{path src}"#).is_none());
        assert!(one(r#"list_dir{path:<|"|>src/<|"|>,}"#).is_none());
    }

    #[test]
    fn json_fallback_still_accepts_any_name() {
        let c = one(r#"{"name": "make_file", "arguments": {"path": "a.py"}}"#).unwrap();
        assert_eq!(c["function"]["name"], "make_file");
    }

    #[test]
    fn ordinary_text_yields_no_calls() {
        let (calls, rest) = calls_from_text("Há três arquivos no projeto.\nNenhuma alteração.", KNOWN);
        assert!(calls.is_empty());
        assert_eq!(rest, "Há três arquivos no projeto.\nNenhuma alteração.");
    }

    #[test]
    fn glued_calls_on_one_line_run_in_order() {
        let (calls, rest) = calls_from_text(
            r#"read_file{path:<|"|>a.md<|"|>}read_file{path:<|"|>b.md<|"|>,start_line:2} list_dir{}"#,
            KNOWN,
        );
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0]["function"]["arguments"], json!({"path": "a.md"}));
        assert_eq!(calls[1]["function"]["arguments"], json!({"path": "b.md", "start_line": 2}));
        assert_eq!(calls[2]["function"]["name"], "list_dir");
        assert!(rest.is_empty());
    }

    #[test]
    fn glued_calls_with_trailing_junk_are_refused() {
        assert!(one(r#"read_file{path:<|"|>a<|"|>}read_file{path:<|"|>b<|"|>} ok"#).is_none());
        assert!(one(r#"read_file{path:<|"|>a<|"|>}unknown{path:<|"|>b<|"|>}"#).is_none());
    }

    #[test]
    fn braces_inside_quoted_values_do_not_end_the_call() {
        let c = one(r#"create_file{path:<|"|>a.rs<|"|>,content:<|"|>fn main() {}<|"|>}"#).unwrap();
        assert_eq!(c["function"]["arguments"]["content"], "fn main() {}");
    }

    #[test]
    fn python_style_call_with_escaped_code_is_recognized() {
        // Resposta real do nemotron-3.5-bolt:e4b depois de uma resposta vazia.
        let text = r##"create_file(path="fib.py", content="#!/usr/bin/env python3\ndef f(n):\n    return n\nif __name__ == '__main__':\n    print(f(1))")"##;
        let (calls, rest) = calls_from_text(text, KNOWN);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "create_file");
        let a = &calls[0]["function"]["arguments"];
        assert_eq!(a["path"], "fib.py");
        assert_eq!(a["content"], "#!/usr/bin/env python3\ndef f(n):\n    return n\nif __name__ == '__main__':\n    print(f(1))");
        assert!(rest.is_empty());
    }

    #[test]
    fn python_style_call_may_span_lines_and_use_single_quotes_and_scalars() {
        let text = "read_file(path='a.py',\n  start_line=2, mode=\"outline\", full=True)\nlist_dir(path=\".\")";
        let (calls, _) = calls_from_text(text, KNOWN);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["function"]["arguments"], json!({"path": "a.py", "start_line": 2, "mode": "outline", "full": true}));
        assert_eq!(calls[1]["function"]["name"], "list_dir");
        let (raw_newline, _) = calls_from_text("create_file(path=\"a.txt\", content=\"um\ndois\")", KNOWN);
        assert_eq!(raw_newline[0]["function"]["arguments"]["content"], "um\ndois");
    }

    #[test]
    fn python_style_call_inside_a_code_fence_is_recognized() {
        let (calls, _) = calls_from_text("```python\nlist_dir(path=\".\")\n```", KNOWN);
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn python_style_call_inside_a_sentence_is_not_a_call() {
        for text in [
            r#"Use read_file(path="a.py") para ler."#,
            r#"read_file(path="a.py") e depois resuma"#,
            r#"Vou chamar list_dir(path=".")"#,
            r#"make_file(path="a.py")"#,
            r#"read_file(path="a.py""#,
            r#"read_file(path="a.py",)"#,
            r#"read_file(path=a.py)"#,
        ] {
            assert!(calls_from_text(text, KNOWN).0.is_empty(), "{text}");
        }
    }

    #[test]
    fn lines_that_are_not_calls_are_kept_as_text() {
        let (calls, rest) = calls_from_text("Vou listar.\nlist_dir{path:<|\"|>.<|\"|>}", KNOWN);
        assert_eq!(calls.len(), 1);
        assert_eq!(rest, "Vou listar.");
    }
}

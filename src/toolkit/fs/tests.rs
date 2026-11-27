use super::*;
use crate::core::tool::{Approver, AutoApprover, MAX_READ_BYTES, Outcome, Preview, Registry, ToolContext};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

struct Refuse;

impl Approver for Refuse {
    fn approve(&self, _preview: &Preview) -> bool {
        false
    }
}

fn root(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bitclaw-tools-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(root: &Path, name: &str, args: Value) -> Outcome {
    crate::toolkit::default_registry().execute(&ToolContext::new(root, &AutoApprover), name, &args)
}

#[test]
fn create_file_creates_new_file() {
    let r = root("create");
    let out = run(&r, "create_file", json!({"path": "a.py", "content": "x = 1\n"}));
    assert!(matches!(out, Outcome::Done(_)), "{out:?}");
    assert_eq!(fs::read_to_string(r.join("a.py")).unwrap(), "x = 1\n");
}

#[test]
fn create_dir_creates_nested_folders() {
    let r = root("mkdir");
    let out = run(&r, "create_dir", json!({"path": "skills/guias"}));
    assert!(matches!(out, Outcome::Done(_)), "{out:?}");
    assert!(r.join("skills/guias").is_dir());
}

#[test]
fn create_dir_refuses_an_existing_folder() {
    let r = root("mkdir-exists");
    fs::create_dir(r.join("skills")).unwrap();
    let out = run(&r, "create_dir", json!({"path": "skills"}));
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
}

#[test]
fn create_file_refuses_to_overwrite() {
    let r = root("overwrite");
    fs::write(r.join("a.py"), "antigo").unwrap();
    let out = run(&r, "create_file", json!({"path": "a.py", "content": "novo"}));
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
    assert_eq!(fs::read_to_string(r.join("a.py")).unwrap(), "antigo");
}

#[test]
fn edit_file_on_missing_file_is_error() {
    let r = root("missing");
    let out = run(&r, "edit_file", json!({"path": "fib.py", "old": "a", "new": "b"}));
    assert!(matches!(out, Outcome::Error(_)), "{out:?}");
    assert!(!r.join("fib.py").exists());
}

#[test]
fn refused_edit_leaves_file_unchanged() {
    let r = root("refused");
    fs::write(r.join("a.py"), "x = 1\n").unwrap();
    let out = crate::toolkit::default_registry().execute(
        &ToolContext::new(&r, &Refuse),
        "edit_file",
        &json!({"path": "a.py", "old": "x = 1", "new": "x = 2"}),
    );
    assert!(matches!(out, Outcome::Refused(_)), "{out:?}");
    assert_eq!(fs::read_to_string(r.join("a.py")).unwrap(), "x = 1\n");
}

#[test]
fn read_file_truncates_large_files() {
    let r = root("big");
    fs::write(r.join("grande.txt"), "a".repeat(MAX_READ_BYTES * 3)).unwrap();
    let out = run(&r, "read_file", json!({"path": "grande.txt"}));
    let Outcome::Done(text) = out else { panic!("esperava Done") };
    assert!(text.contains("truncado em"));
    assert!(text.len() < MAX_READ_BYTES + 300);
}

#[test]
fn read_file_returns_the_outline_for_a_large_file_without_range() {
    let r = root("big-outline");
    let body = format!("//! modulo\npub fn alvo() {{}}\n{}", "// enchimento\n".repeat(MAX_READ_BYTES));
    fs::write(r.join("grande.rs"), body).unwrap();
    let Outcome::Done(text) = run(&r, "read_file", json!({"path": "grande.rs"})) else { panic!() };
    assert!(text.starts_with("[outline"), "{text}");
    assert!(text.contains("2: pub fn alvo"));
    assert!(text.contains("start_line"));
    assert!(text.len() < MAX_READ_BYTES);
}

#[test]
fn read_file_keeps_small_files_intact() {
    let r = root("small");
    fs::write(r.join("p.txt"), "oi").unwrap();
    assert_eq!(run(&r, "read_file", json!({"path": "p.txt"})), Outcome::Done("oi".into()));
}

#[test]
fn unknown_tool_is_error() {
    let r = root("unknown");
    let out = run(&r, "make_file", json!({}));
    assert!(matches!(out, Outcome::Error(ref e) if e.starts_with("ferramenta desconhecida: make_file")));
}

#[test]
fn delete_file_removes_file_and_keeps_backup() {
    let r = root("delete");
    fs::write(r.join("mean.cpp"), "int main() {}\n").unwrap();
    let out = run(&r, "delete_file", json!({"path": "mean.cpp"}));
    assert!(matches!(out, Outcome::Done(_)), "{out:?}");
    assert!(!r.join("mean.cpp").exists());
    let backups: Vec<_> = fs::read_dir(r.join(BACKUP_DIR)).unwrap().collect();
    assert_eq!(backups.len(), 1);
}

#[test]
fn refused_delete_keeps_file() {
    let r = root("delete-refused");
    fs::write(r.join("a.md"), "# nota\n").unwrap();
    let out = crate::toolkit::default_registry().execute(&ToolContext::new(&r, &Refuse), "delete_file", &json!({"path": "a.md"}));
    assert!(matches!(out, Outcome::Refused(_)), "{out:?}");
    assert!(r.join("a.md").exists());
}

#[test]
fn delete_file_refuses_directories_and_missing_files() {
    let r = root("delete-bad");
    fs::create_dir_all(r.join("pasta")).unwrap();
    assert!(matches!(run(&r, "delete_file", json!({"path": "pasta"})), Outcome::Error(_)));
    assert!(matches!(run(&r, "delete_file", json!({"path": "nao.md"})), Outcome::Error(_)));
    assert!(r.join("pasta").exists());
}

#[test]
fn mutating_tools_are_marked() {
    let reg = crate::toolkit::default_registry();
    assert!(reg.mutates("create_file"));
    assert!(reg.mutates("edit_file"));
    assert!(reg.mutates("delete_file"));
    assert!(!reg.mutates("read_file"));
    assert!(!reg.mutates("list_dir"));
}

#[test]
fn plan_mode_keeps_only_read_only_tools() {
    let names = crate::toolkit::default_registry().read_only().names();
    assert_eq!(names, vec!["list_dir", "read_file", "grep_files", "find_files", "run_command"], "só leitura, na ordem do catálogo");
    assert!(!names.contains(&"run_script"), "script pode alterar qualquer coisa: fica fora do modo Plan");
}

#[test]
fn unannotated_tools_count_as_changing_and_destructive() {
    let d = crate::core::tool::Annotations::default();
    assert!(!d.read_only && d.destructive && !d.idempotent);
    let reg = crate::toolkit::default_registry();
    assert!(reg.mutates("run_script"), "sem declaração de leitura, script conta como alteração");
}

#[test]
#[should_panic(expected = "duplicados")]
fn duplicate_names_are_rejected() {
    Registry::new(vec![Box::new(ListDir), Box::new(ListDir)]);
}

#[test]
fn read_file_returns_only_the_requested_lines() {
    let r = root("range");
    fs::write(r.join("a.txt"), "um\ndois\ntrês\nquatro\ncinco\n").unwrap();
    let out = run(&r, "read_file", json!({"path": "a.txt", "start_line": 2, "end_line": 3}));
    assert_eq!(out, Outcome::Done("[linhas 2-3 de 5]\ndois\ntrês".into()));
    // Aceita número como texto e fim omitido.
    let out = run(&r, "read_file", json!({"path": "a.txt", "start_line": "4"}));
    assert_eq!(out, Outcome::Done("[linhas 4-5 de 5]\nquatro\ncinco".into()));
}

#[test]
fn read_file_rejects_a_bad_line_number_and_reports_past_the_end() {
    let r = root("range-bad");
    fs::write(r.join("a.txt"), "um\ndois\n").unwrap();
    assert!(matches!(run(&r, "read_file", json!({"path": "a.txt", "start_line": 0})), Outcome::Error(_)));
    assert_eq!(
        run(&r, "read_file", json!({"path": "a.txt", "start_line": 9})),
        Outcome::Done("[o arquivo tem só 2 linhas]".into())
    );
}

#[test]
fn read_file_outline_lists_declarations_with_line_numbers() {
    let r = root("outline");
    fs::write(
        r.join("a.rs"),
        "//! Módulo de teste.\n\nuse std::fs;\n\npub struct A;\n\nimpl A {\n    pub fn novo() -> Self {\n        let x = 1;\n        A\n    }\n}\n\nfn solta() {}\n",
    )
    .unwrap();
    let Outcome::Done(text) = run(&r, "read_file", json!({"path": "a.rs", "mode": "outline"})) else { panic!() };
    assert!(text.contains("1: //! Módulo de teste."), "{text}");
    assert!(text.contains("5: pub struct A;"), "{text}");
    assert!(text.contains("8:     pub fn novo() -> Self {"), "{text}");
    assert!(text.contains("14: fn solta() {}"), "{text}");
    assert!(!text.contains("let x"), "{text}");
    assert!(!text.contains("use std"), "{text}");
}

#[test]
fn read_file_truncation_tells_how_to_read_the_rest() {
    let r = root("trunc");
    fs::write(r.join("big.txt"), "linha\n".repeat(MAX_READ_BYTES)).unwrap();
    let Outcome::Done(text) = run(&r, "read_file", json!({"path": "big.txt"})) else { panic!() };
    assert!(text.contains("start_line"), "{text}");
    assert!(text.contains(&format!("{} linhas", MAX_READ_BYTES)), "{text}");
}

#[test]
fn grep_files_finds_lines_and_skips_target() {
    let r = root("grep");
    fs::create_dir_all(r.join("src")).unwrap();
    fs::create_dir_all(r.join("target")).unwrap();
    fs::write(r.join("src/a.rs"), "fn main() {}\nlet Token = 1;\n").unwrap();
    fs::write(r.join("target/b.rs"), "token oculto\n").unwrap();
    let out = run(&r, "grep_files", json!({"pattern": "token"}));
    assert_eq!(out, Outcome::Done("1 ocorrências em 1 arquivos:\nsrc/a.rs\n  2: let Token = 1;".into()));
    let Outcome::Done(none) = run(&r, "grep_files", json!({"pattern": "inexistente"})) else { panic!() };
    assert!(none.starts_with("nada encontrado para \"inexistente\""), "{none}");
    assert!(none.contains("Não conclua que não existe"), "{none}");
    assert!(none.contains("ocultos ou ignorados"), "avisa que target ficou de fora: {none}");
    // Pasta ignorada passada em path é percorrida.
    let Outcome::Done(inside) = run(&r, "grep_files", json!({"pattern": "token", "path": "target"})) else { panic!() };
    assert!(inside.contains("target/b.rs"), "{inside}");
}

#[test]
fn grep_files_caps_results_and_refuses_paths_outside_the_project() {
    let r = root("grep-cap");
    for i in 0..12 {
        fs::write(r.join(format!("f{i:02}.txt")), "alvo\n".repeat(10)).unwrap();
    }
    let Outcome::Done(text) = run(&r, "grep_files", json!({"pattern": "alvo"})) else { panic!() };
    assert!(text.starts_with("120 ocorrências em 12 arquivos:"), "{text}");
    assert_eq!(text.lines().filter(|l| l.starts_with("  ") && !l.contains('[')).count(), 40, "{text}");
    assert!(text.contains("  [+4 neste arquivo]"), "teto por arquivo: {text}");
    assert!(text.contains("[também em: f07.txt (10)"), "lista o que sobrou: {text}");
    assert!(matches!(run(&r, "grep_files", json!({"pattern": "x", "path": "../"})), Outcome::Error(_)));
    assert!(matches!(run(&r, "grep_files", json!({"pattern": "  "})), Outcome::Error(_)));
    assert!(matches!(run(&r, "grep_files", json!({"pattern": " | "})), Outcome::Error(_)));
}

// O caso real que motivou a mudança: "troque a cor amarela da TUI para vermelho". O código diz `Yellow`,
// a cor é definida num arquivo e usada em vários, e assets e a pasta de build também têm "yellow".
fn color_project(name: &str) -> std::path::PathBuf {
    let r = root(name);
    fs::create_dir_all(r.join("assets")).unwrap();
    fs::create_dir_all(r.join("src/ui/widgets")).unwrap();
    fs::create_dir_all(r.join("build")).unwrap();
    fs::create_dir_all(r.join("gerado")).unwrap();
    fs::write(r.join(".gitignore"), "/gerado\n*.log\n").unwrap();
    fs::write(r.join(".env"), "YELLOW_TOKEN=segredo\n").unwrap();
    fs::write(r.join("app.log"), "yellow\n").unwrap();
    fs::write(r.join("gerado/theme.rs"), "pub const ACCENT: Color = Color::Yellow;\n").unwrap();
    fs::write(r.join("build/theme.rs"), "pub const ACCENT: Color = Color::Yellow;\n").unwrap();
    fs::write(r.join("assets/a.svg"), format!("<svg>{}<rect fill=\"yellow\"/></svg>\n", "<g/>".repeat(100))).unwrap();
    fs::write(r.join("src/ui/widgets/status.rs"), "let a = fg(Color::Yellow);\nlet b = fg(Color::Yellow);\n").unwrap();
    fs::write(r.join("src/ui/theme.rs"), "// Destaque.\npub const ACCENT: Color = Color::Yellow;\n").unwrap();
    fs::write(r.join("logo.png"), b"\x89PNG\0\0yellow").unwrap();
    r
}

#[test]
fn grep_files_puts_the_definition_first_and_skips_hidden_ignored_and_binary_files() {
    let r = color_project("grep-color");
    let Outcome::Done(text) = run(&r, "grep_files", json!({"pattern": "yellow"})) else { panic!() };
    let first = text.lines().nth(1).unwrap();
    assert_eq!(first, "src/ui/theme.rs", "{text}");
    assert!(text.contains("  2: pub const ACCENT: Color = Color::Yellow;  ← definição"), "{text}");
    assert!(text.contains("src/ui/widgets/status.rs") && text.contains("assets/a.svg"), "{text}");
    for hidden in [".env", "gerado/", "build/", "app.log", "logo.png"] {
        assert!(!text.contains(hidden), "{hidden} não devia aparecer: {text}");
    }
    let svg = text.lines().find(|l| l.contains("fill")).unwrap();
    assert!(svg.contains("yellow") && svg.chars().count() < 200, "trecho em volta da ocorrência: {svg}");
    let Outcome::Done(rs) = run(&r, "grep_files", json!({"pattern": "yellow", "ext": ".rs"})) else { panic!() };
    assert!(!rs.contains("a.svg"), "{rs}");
}

#[test]
fn grep_files_broadens_a_portuguese_request_that_finds_nothing() {
    let r = color_project("grep-broaden");
    let Outcome::Done(text) = run(&r, "grep_files", json!({"pattern": "amarela"})) else { panic!() };
    assert!(text.starts_with("nada encontrado para \"amarela\"; busca ampliada com \"yellow\""), "{text}");
    assert!(text.contains("pub const ACCENT: Color = Color::Yellow;  ← definição"), "{text}");
    let Outcome::Done(accents) = run(&r, "grep_files", json!({"pattern": "DESTAQUE|azul"})) else { panic!() };
    assert!(accents.contains("src/ui/theme.rs\n  1: // Destaque."), "{accents}");
}

#[test]
fn find_files_matches_names_and_paths() {
    let r = color_project("find");
    assert_eq!(run(&r, "find_files", json!({"pattern": "status"})), Outcome::Done("src/ui/widgets/status.rs".into()));
    let Outcome::Done(text) = run(&r, "find_files", json!({"pattern": "theme|widgets"})) else { panic!() };
    assert_eq!(text, "src/ui/theme.rs\nsrc/ui/widgets/\nsrc/ui/widgets/status.rs", "sem gerado/ nem build/");
    let Outcome::Done(wide) = run(&r, "find_files", json!({"pattern": "tema"})) else { panic!() };
    assert!(wide.contains("busca ampliada com \"theme|palette\"") && wide.contains("src/ui/theme.rs"), "{wide}");
    let Outcome::Done(none) = run(&r, "find_files", json!({"pattern": "zzz"})) else { panic!() };
    assert!(none.contains("grep_files") && none.contains("ocultos ou ignorados"), "{none}");
    assert!(matches!(run(&r, "find_files", json!({"pattern": "a", "path": "/etc"})), Outcome::Error(_)));
}

#[test]
fn read_file_outline_stops_at_the_test_module() {
    let r = root("outline-tests");
    fs::write(r.join("a.rs"), "pub fn real() {}\n\n#[cfg(test)]\nmod tests {\n    fn um_teste() {}\n}\n").unwrap();
    let Outcome::Done(text) = run(&r, "read_file", json!({"path": "a.rs", "mode": "outline"})) else { panic!() };
    assert!(text.contains("1: pub fn real() {}"), "{text}");
    assert!(!text.contains("um_teste") && !text.contains("mod tests"), "{text}");
    assert!(text.contains("[testes a partir da linha 3 omitidos]"), "{text}");
}

#[test]
fn edit_file_answers_with_the_line_counts_and_not_the_diff() {
    let r = root("edit-short");
    fs::write(r.join("a.txt"), "um\ndois\ntrês\n").unwrap();
    let out = run(&r, "edit_file", json!({"path": "a.txt", "old": "dois\ntrês", "new": "2"}));
    let Outcome::Done(text) = out else { panic!("{out:?}") };
    assert!(text.contains("+1 −2 linhas"), "{text}");
    assert!(text.len() < 120, "a resposta ao modelo deve ser curta: {text}");
    assert_eq!(fs::read_to_string(r.join("a.txt")).unwrap(), "um\n2\n");
}

#[test]
fn read_file_errors_say_what_to_do_instead() {
    let r = root("read-hints");
    fs::create_dir(r.join("src")).unwrap();
    let Outcome::Error(dir) = run(&r, "read_file", json!({"path": "src"})) else { panic!() };
    assert!(dir.contains("é uma pasta") && dir.contains("list_dir"), "{dir}");
    let Outcome::Error(missing) = run(&r, "read_file", json!({"path": "novo.py"})) else { panic!() };
    assert!(missing.contains("não existe") && missing.contains("list_dir"), "{missing}");
    assert!(!missing.contains("create_file"), "o erro de leitura não mistura instruções de outra ferramenta: {missing}");
    assert!(!missing.contains("Você quis dizer"), "sem nome parecido, não sugere nada: {missing}");
}

#[test]
fn read_file_suggests_the_name_with_extension() {
    let r = root("read-suggest");
    fs::write(r.join("README.md"), "oi").unwrap();
    fs::create_dir(r.join("docs")).unwrap();
    fs::write(r.join("docs/Guia.md"), "oi").unwrap();
    let Outcome::Error(a) = run(&r, "read_file", json!({"path": "README"})) else { panic!() };
    assert!(a.contains("Você quis dizer: README.md?"), "{a}");
    let Outcome::Error(b) = run(&r, "read_file", json!({"path": "docs/guia"})) else { panic!() };
    assert!(b.contains("docs/Guia.md"), "{b}");
    let Outcome::Error(c) = run(&r, "read_file", json!({"path": "readme"})) else { panic!() };
    assert!(c.contains("README.md"), "{c}");
}

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
    assert!(text.len() < MAX_READ_BYTES + 100);
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
#[should_panic(expected = "duplicados")]
fn duplicate_names_are_rejected() {
    Registry::new(vec![Box::new(ListDir), Box::new(ListDir)]);
}

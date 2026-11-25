//! `/init`: cria um `AGENTS.md` com o que é verificável no projeto (comandos, estrutura, skills e
//! agentes). O rascunho é determinístico: nada é inventado nem resumido por um modelo pequeno. As
//! notas que só a pessoa sabe (convenções, o que não mexer) ficam para ela preencher.

use crate::tools::{Approver, Preview, PreviewLine};
use std::fs;
use std::path::Path;

pub const FILE: &str = "AGENTS.md";

// Entradas que não são do projeto ou são geradas: não entram na estrutura.
const SKIP: &[&str] = &["target", "node_modules", "dist", "build"];

// Limite da listagem de pastas da raiz. O arquivo precisa caber em poucas linhas.
const MAX_ENTRIES: usize = 30;

// Rascunho do arquivo, sem gravar nada.
pub fn draft(root: &Path) -> Result<String, String> {
    let mut out = format!("# {}\n\n", project_name(root));
    out.push_str("Instruções para agentes de código neste projeto. Só fatos verificados no repositório.\n\n");

    let commands = commands(root);
    if !commands.is_empty() {
        out.push_str("## Comandos\n\n```sh\n");
        for c in &commands {
            out.push_str(c);
            out.push('\n');
        }
        out.push_str("```\n\n");
    }

    let entries = top_level(root)?;
    if !entries.is_empty() {
        out.push_str("## Estrutura\n\n");
        for e in &entries {
            out.push_str(&format!("- `{e}`\n"));
        }
        out.push('\n');
    }

    let skills = child_dirs_with(root, "skills", |p| p.join("SKILL.md").is_file());
    if !skills.is_empty() {
        out.push_str("## Skills\n\n");
        for s in &skills {
            out.push_str(&format!("- `skills/{s}/SKILL.md`\n"));
        }
        out.push('\n');
    }

    let agents = toml_files(root, "agents");
    if !agents.is_empty() {
        out.push_str("## Agentes\n\n");
        for a in &agents {
            out.push_str(&format!("- `agents/{a}`\n"));
        }
        out.push('\n');
    }

    out.push_str("## Notas\n\n<!-- Acrescente o que não é óbvio no código: convenções, limites, o que não mexer. -->\n");
    Ok(out)
}

// Mostra o rascunho ao usuário e só grava se ele aprovar. Nunca sobrescreve um AGENTS.md existente.
pub fn write(root: &Path, approver: &dyn Approver) -> Result<String, String> {
    let path = root.join(FILE);
    if path.symlink_metadata().is_ok() {
        return Ok(format!("{FILE} já existe; nada foi alterado."));
    }
    let text = draft(root)?;
    let preview = Preview {
        title: format!("criar {FILE}"),
        lines: text.lines().map(|l| PreviewLine::Added(l.to_string())).collect(),
    };
    if !approver.approve(&preview) {
        return Ok(format!("usuário recusou; {FILE} não foi criado."));
    }
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    Ok(format!("{FILE} criado com {} linhas. Revise e preencha as notas.", text.lines().count()))
}

// Nome do pacote: do `Cargo.toml` ou do `package.json`; sem nenhum, o nome da pasta.
fn project_name(root: &Path) -> String {
    if let Ok(text) = fs::read_to_string(root.join("Cargo.toml"))
        && let Ok(value) = toml::from_str::<toml::Value>(&text)
        && let Some(name) = value.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str)
    {
        return name.to_string();
    }
    if let Ok(text) = fs::read_to_string(root.join("package.json"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(name) = value.get("name").and_then(serde_json::Value::as_str)
    {
        return name.to_string();
    }
    root.file_name().and_then(|n| n.to_str()).unwrap_or("projeto").to_string()
}

// Comandos que o projeto de fato suporta, pelos arquivos que existem.
fn commands(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if root.join("Cargo.toml").is_file() {
        out.push("cargo build".into());
        out.push("cargo test".into());
        if root.join("src/main.rs").is_file() {
            out.push("cargo run".into());
        }
    }
    if let Ok(text) = fs::read_to_string(root.join("package.json"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object)
    {
        out.push("npm install".into());
        for name in scripts.keys() {
            out.push(format!("npm run {name}"));
        }
    }
    out
}

// Pastas e arquivos da raiz, em ordem, sem os ocultos nem os gerados.
fn top_level(root: &Path) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == FILE || SKIP.contains(&name.as_str()) {
            continue;
        }
        let is_dir = entry.file_type().map_err(|e| e.to_string())?.is_dir();
        names.push(if is_dir { format!("{name}/") } else { name });
    }
    names.sort();
    names.truncate(MAX_ENTRIES);
    Ok(names)
}

// Subpastas de `dir` que satisfazem `keep`, em ordem.
fn child_dirs_with(root: &Path, dir: &str, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join(dir)) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir() && keep(&e.path()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// Arquivos `.toml` diretamente dentro de `dir`, em ordem.
fn toml_files(root: &Path, dir: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root.join(dir)) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml") && e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Answer(bool, Cell<bool>);

    impl Approver for Answer {
        fn approve(&self, _preview: &Preview) -> bool {
            self.1.set(true);
            self.0
        }
    }

    fn project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-init-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(dir.join("skills/alfa")).unwrap();
        fs::write(dir.join("skills/alfa/SKILL.md"), "---\nname: alfa\n---\n").unwrap();
        fs::create_dir_all(dir.join("agents")).unwrap();
        fs::write(dir.join("agents/bitclaw.toml"), "name = \"x\"\n").unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    #[test]
    fn draft_lists_only_verifiable_facts() {
        let dir = project("draft");
        let text = draft(&dir).unwrap();
        assert!(text.starts_with("# demo\n"), "{text}");
        assert!(text.contains("cargo test") && text.contains("cargo run"), "{text}");
        assert!(text.contains("- `skills/alfa/SKILL.md`"), "{text}");
        assert!(text.contains("- `agents/bitclaw.toml`"), "{text}");
        assert!(text.contains("- `src/`"), "{text}");
        assert!(!text.contains("target") && !text.contains(".git"), "gerados e ocultos ficam de fora:\n{text}");
        assert!(text.lines().count() < 40, "o arquivo precisa caber em poucas linhas");
    }

    #[test]
    fn draft_is_the_same_every_time() {
        let dir = project("stable");
        assert_eq!(draft(&dir).unwrap(), draft(&dir).unwrap());
    }

    #[test]
    fn write_creates_the_file_only_after_approval() {
        let dir = project("approved");
        let yes = Answer(true, Cell::new(false));
        let msg = write(&dir, &yes).unwrap();
        assert!(yes.1.get(), "a prévia é mostrada antes de gravar");
        assert!(msg.contains("criado"), "{msg}");
        assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), draft(&dir).unwrap());
    }

    #[test]
    fn refusal_writes_nothing() {
        let dir = project("refused");
        let msg = write(&dir, &Answer(false, Cell::new(false))).unwrap();
        assert!(msg.contains("não foi criado"), "{msg}");
        assert!(!dir.join(FILE).exists());
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let dir = project("existing");
        fs::write(dir.join(FILE), "meu conteúdo\n").unwrap();
        let approver = Answer(true, Cell::new(false));
        let msg = write(&dir, &approver).unwrap();
        assert!(msg.contains("já existe"), "{msg}");
        assert!(!approver.1.get(), "nem chega a pedir aprovação");
        assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), "meu conteúdo\n");
    }
}

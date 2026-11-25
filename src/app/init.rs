//! `/init`: cria o `AGENTS.md` do projeto com fatos extraídos do próprio repositório.
//!
//! Entram a descrição do README, a stack, os comandos, o que cada módulo faz (pela primeira frase
//! do `//!` de cada arquivo), a documentação, as skills e os agentes. O rascunho é determinístico:
//! nada é inventado nem resumido por um modelo, então sai na hora e sem gastar tokens. As notas que
//! só a pessoa sabe (convenções, o que não mexer) ficam para ela preencher.

use bitclaw::tools::{Approver, Preview, PreviewLine};
use std::fs;
use std::path::Path;

pub const FILE: &str = "AGENTS.md";

// Entradas que não são do projeto ou são geradas: não entram na estrutura.
const SKIP: &[&str] = &["target", "node_modules", "dist", "build"];

// Limite da listagem de pastas da raiz.
const MAX_ENTRIES: usize = 30;

// Tamanho máximo de cada descrição, em caracteres.
const MAX_DESC: usize = 200;

// Um trecho do arquivo, sob um título `##`.
struct Section {
    title: &'static str,
    body: String,
}

// Rascunho do arquivo, sem gravar nada.
pub fn draft(root: &Path) -> Result<String, String> {
    let mut out = format!("# {}\n\n", project_name(root));
    match about(root) {
        Some(text) => out.push_str(&format!("{text}\n\n")),
        None => out.push_str("Instruções para agentes de código neste projeto. Só fatos verificados no repositório.\n\n"),
    }
    for section in sections(root)? {
        out.push_str(&format!("## {}\n\n{}\n\n", section.title, section.body.trim_end()));
    }
    out.push_str("## Notas\n\n<!-- Acrescente o que não é óbvio no código: convenções, limites, o que não mexer. -->\n");
    Ok(out)
}

fn sections(root: &Path) -> Result<Vec<Section>, String> {
    let mut all = Vec::new();
    let mut add = |title: &'static str, body: String| {
        if !body.trim().is_empty() {
            all.push(Section { title, body: format!("{}\n", body.trim_end()) });
        }
    };
    add("Stack", stack(root));
    let commands = commands(root);
    if !commands.is_empty() {
        add("Comandos", format!("```sh\n{}\n```\n", commands.join("\n")));
    }
    add("Código", code(root));
    add("Documentação", docs(root));
    add("Scripts", scripts(root));
    add("Skills", skills(root));
    add("Agentes", agents(root));
    let entries = top_level(root)?;
    add("Estrutura", entries.iter().map(|e| format!("- `{e}`\n")).collect());
    Ok(all)
}

// Mostra o rascunho ao usuário e só grava se ele aprovar. Nunca sobrescreve um AGENTS.md existente:
// nele, só acrescenta as seções que faltam.
pub fn write(root: &Path, approver: &dyn Approver) -> Result<String, String> {
    let path = root.join(FILE);
    if path.symlink_metadata().is_ok() {
        return complete(root, &path, approver);
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

// Acrescenta ao fim de um AGENTS.md existente as seções que ele ainda não tem. O que já está
// escrito não é tocado.
fn complete(root: &Path, path: &Path, approver: &dyn Approver) -> Result<String, String> {
    let meta = path.symlink_metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Ok(format!("{FILE} já existe e não é um arquivo comum; nada foi alterado."));
    }
    let current = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let missing: Vec<Section> = sections(root)?
        .into_iter()
        .filter(|s| !current.lines().any(|l| l.trim().eq_ignore_ascii_case(&format!("## {}", s.title))))
        .collect();
    if missing.is_empty() {
        return Ok(format!("{FILE} já tem todas as seções que o /init sabe preencher; nada foi alterado."));
    }
    let mut added = String::new();
    for s in &missing {
        added.push_str(&format!("## {}\n\n{}\n\n", s.title, s.body.trim_end()));
    }
    let preview = Preview {
        title: format!("acrescentar ao {FILE}"),
        lines: added.lines().map(|l| PreviewLine::Added(l.to_string())).collect(),
    };
    if !approver.approve(&preview) {
        return Ok(format!("usuário recusou; {FILE} não foi alterado."));
    }
    let mut next = current;
    if !next.ends_with('\n') {
        next.push('\n');
    }
    next.push('\n');
    next.push_str(&added);
    fs::write(path, next).map_err(|e| e.to_string())?;
    let names: Vec<&str> = missing.iter().map(|s| s.title).collect();
    Ok(format!("{FILE}: acrescentei {} (o que já estava escrito ficou como estava).", names.join(", ")))
}

// ---------- fontes ----------

// Nome do pacote: do `Cargo.toml` ou do `package.json`; sem nenhum, o nome da pasta.
fn project_name(root: &Path) -> String {
    if let Some(v) = cargo(root)
        && let Some(name) = v.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str)
    {
        return name.to_string();
    }
    if let Some(v) = package_json(root)
        && let Some(name) = v.get("name").and_then(serde_json::Value::as_str)
    {
        return name.to_string();
    }
    root.file_name().and_then(|n| n.to_str()).unwrap_or("projeto").to_string()
}

fn cargo(root: &Path) -> Option<toml::Value> {
    toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).ok()?).ok()
}

fn package_json(root: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(root.join("package.json")).ok()?).ok()
}

// O que o projeto é: os primeiros parágrafos do README (sem imagens nem HTML) ou, sem README, a
// descrição do `Cargo.toml` / `package.json`.
fn about(root: &Path) -> Option<String> {
    if let Ok(text) = fs::read_to_string(root.join("README.md")) {
        let paragraphs = readme_intro(&text);
        if !paragraphs.is_empty() {
            return Some(paragraphs.join("\n\n"));
        }
    }
    cargo(root)
        .and_then(|v| v.get("package")?.get("description")?.as_str().map(str::to_string))
        .or_else(|| package_json(root)?.get("description")?.as_str().map(str::to_string))
}

// Até dois parágrafos de texto logo depois do título do README.
fn readme_intro(text: &str) -> Vec<String> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut seen_title = false;
    let skip = |l: &str| l.starts_with('<') || l.starts_with("![") || l.starts_with("[![") || l.starts_with("```");
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("## ") {
            break;
        }
        if line.starts_with("# ") {
            seen_title = true;
            continue;
        }
        if skip(line) {
            continue;
        }
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
            if paragraphs.len() == 2 {
                break;
            }
            continue;
        }
        if seen_title || !text.lines().any(|l| l.starts_with("# ")) {
            current.push(line);
        }
    }
    if !current.is_empty() && paragraphs.len() < 2 {
        paragraphs.push(current.join(" "));
    }
    paragraphs.truncate(2);
    paragraphs
}

// Linguagem, edição e dependências diretas.
fn stack(root: &Path) -> String {
    let mut out = String::new();
    if let Some(v) = cargo(root) {
        let edition = v.get("package").and_then(|p| p.get("edition")).and_then(toml::Value::as_str);
        out.push_str(&format!("- Rust{}\n", edition.map(|e| format!(", edição {e}")).unwrap_or_default()));
        if let Some(deps) = v.get("dependencies").and_then(toml::Value::as_table) {
            let names: Vec<&str> = deps.keys().map(String::as_str).collect();
            if !names.is_empty() {
                out.push_str(&format!("- Dependências: {}\n", names.join(", ")));
            }
        }
    }
    if let Some(v) = package_json(root) {
        out.push_str("- Node.js\n");
        if let Some(deps) = v.get("dependencies").and_then(serde_json::Value::as_object) {
            let names: Vec<&str> = deps.keys().map(String::as_str).collect();
            if !names.is_empty() {
                out.push_str(&format!("- Dependências: {}\n", names.join(", ")));
            }
        }
    }
    out
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
    if let Some(value) = package_json(root)
        && let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object)
    {
        out.push("npm install".into());
        for name in scripts.keys() {
            out.push(format!("npm run {name}"));
        }
    }
    out
}

// O que cada arquivo e pasta de `src/` faz, pela primeira frase do comentário `//!` de abertura.
// Pastas listam os arquivos de dentro, um nível.
fn code(root: &Path) -> String {
    let src = root.join("src");
    let Ok(read) = fs::read_dir(&src) else { return String::new() };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let mut out = String::new();
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if path.is_dir() {
            let desc = ["mod.rs", "lib.rs"].iter().find_map(|f| rust_doc(&path.join(f)));
            out.push_str(&bullet(&format!("src/{name}/"), desc.as_deref()));
            let Ok(inner) = fs::read_dir(&path) else { continue };
            let mut files: Vec<_> = inner.flatten().filter(|f| f.path().extension().is_some_and(|x| x == "rs")).collect();
            files.sort_by_key(|f| f.file_name());
            for f in files {
                let inner_name = f.file_name().to_string_lossy().into_owned();
                if inner_name == "mod.rs" {
                    continue;
                }
                out.push_str(&format!("  {}", bullet(&format!("src/{name}/{inner_name}"), rust_doc(&f.path()).as_deref())));
            }
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push_str(&bullet(&format!("src/{name}"), rust_doc(&path).as_deref()));
        }
    }
    out
}

fn bullet(path: &str, desc: Option<&str>) -> String {
    match desc {
        Some(d) => format!("- `{path}`: {d}\n"),
        None => format!("- `{path}`\n"),
    }
}

// Primeira frase do comentário `//!` no topo de um arquivo Rust.
fn rust_doc(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut parts: Vec<&str> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("//!") else { break };
        let rest = rest.trim();
        if rest.is_empty() {
            break;
        }
        parts.push(rest);
    }
    (!parts.is_empty()).then(|| first_sentence(&parts.join(" ")))
}

// Corta no primeiro ponto final seguido de espaço, e em `MAX_DESC` caracteres no máximo.
fn first_sentence(text: &str) -> String {
    let text = text.trim();
    let cut = text.find(". ").map_or(text.len(), |i| i + 1);
    let sentence = &text[..cut];
    let sentence = sentence.trim_end_matches(':').trim();
    if sentence.chars().count() > MAX_DESC {
        let short: String = sentence.chars().take(MAX_DESC).collect();
        format!("{}…", short.trim_end())
    } else {
        sentence.to_string()
    }
}

// README e documentos de `docs/`: o título de cada um, e as seções do README.
fn docs(root: &Path) -> String {
    let mut out = String::new();
    if let Ok(text) = fs::read_to_string(root.join("README.md")) {
        let sections: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("## ")).map(str::trim).take(10).collect();
        if sections.is_empty() {
            out.push_str("- `README.md`\n");
        } else {
            out.push_str(&format!("- `README.md`: seções {}\n", sections.join(", ")));
        }
    }
    if let Ok(read) = fs::read_dir(root.join("docs")) {
        let mut files: Vec<_> = read.flatten().filter(|f| f.path().extension().is_some_and(|x| x == "md")).collect();
        files.sort_by_key(|f| f.file_name());
        for f in files {
            let name = f.file_name().to_string_lossy().into_owned();
            let title = fs::read_to_string(f.path())
                .ok()
                .and_then(|t| t.lines().find_map(|l| l.strip_prefix("# ").map(|h| first_sentence(h))));
            out.push_str(&bullet(&format!("docs/{name}"), title.as_deref()));
        }
    }
    out
}

// Arquivos de `scripts/`, com a primeira linha de comentário depois do shebang.
fn scripts(root: &Path) -> String {
    let Ok(read) = fs::read_dir(root.join("scripts")) else { return String::new() };
    let mut files: Vec<_> = read.flatten().filter(|f| f.path().is_file()).collect();
    files.sort_by_key(|f| f.file_name());
    let mut out = String::new();
    for f in files {
        let name = f.file_name().to_string_lossy().into_owned();
        let desc = fs::read_to_string(f.path()).ok().and_then(|t| {
            t.lines()
                .filter(|l| !l.starts_with("#!"))
                .find_map(|l| l.strip_prefix('#').map(str::trim).filter(|c| !c.is_empty()).map(first_sentence))
        });
        out.push_str(&bullet(&format!("scripts/{name}"), desc.as_deref()));
    }
    out
}

// Skills: o `description` do cabeçalho de cada `SKILL.md`.
fn skills(root: &Path) -> String {
    let Ok(read) = fs::read_dir(root.join("skills")) else { return String::new() };
    let mut dirs: Vec<_> = read.flatten().filter(|e| e.path().join("SKILL.md").is_file()).collect();
    dirs.sort_by_key(|e| e.file_name());
    let mut out = String::new();
    for d in dirs {
        let name = d.file_name().to_string_lossy().into_owned();
        let text = fs::read_to_string(d.path().join("SKILL.md")).unwrap_or_default();
        out.push_str(&bullet(&format!("skills/{name}/SKILL.md"), frontmatter(&text, "description").map(|d| first_sentence(&d)).as_deref()));
    }
    out
}

// Valor de uma chave do cabeçalho `---` de um arquivo Markdown.
fn frontmatter(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    (lines.next()? == "---").then_some(())?;
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix(&format!("{key}:")) {
            return Some(v.trim().to_string());
        }
    }
    None
}

// Agentes `.toml`: nome, modelo e ferramentas.
fn agents(root: &Path) -> String {
    let Ok(read) = fs::read_dir(root.join("agents")) else { return String::new() };
    let mut files: Vec<_> = read.flatten().filter(|f| f.path().extension().is_some_and(|x| x == "toml") && f.path().is_file()).collect();
    files.sort_by_key(|f| f.file_name());
    let mut out = String::new();
    for f in files {
        let name = f.file_name().to_string_lossy().into_owned();
        let value: Option<toml::Value> = fs::read_to_string(f.path()).ok().and_then(|t| toml::from_str(&t).ok());
        let mut facts: Vec<String> = Vec::new();
        if let Some(v) = &value {
            if let Some(n) = v.get("name").and_then(toml::Value::as_str) {
                facts.push(n.to_string());
            }
            if let Some(m) = v.get("model").and_then(|m| m.get("name")).and_then(toml::Value::as_str) {
                facts.push(format!("modelo {m}"));
            }
            if let Some(t) = v.get("tools").and_then(toml::Value::as_array) {
                let tools: Vec<&str> = t.iter().filter_map(toml::Value::as_str).collect();
                if !tools.is_empty() {
                    facts.push(format!("ferramentas {}", tools.join(", ")));
                }
            }
        }
        let desc = (!facts.is_empty()).then(|| facts.join("; "));
        out.push_str(&bullet(&format!("agents/{name}"), desc.as_deref()));
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
        fs::create_dir_all(dir.join("src/ui")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\ntoml = \"1\"\n",
        )
        .unwrap();
        fs::write(dir.join("src/main.rs"), "//! Binário do demo. Lê a configuração e abre a tela.\n\nfn main() {}\n").unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        fs::write(dir.join("src/ui/mod.rs"), "//! Interface do demo, em tela cheia.\n").unwrap();
        fs::write(dir.join("src/ui/theme.rs"), "//! Cores e estilos.\n").unwrap();
        fs::write(
            dir.join("README.md"),
            "# Demo\n\n<p align=\"center\"><img src=\"a.svg\"></p>\n\nAgente de exemplo, local e enxuto. Lê e edita arquivos.\n\nO objetivo é provar o conceito.\n\n## Como rodar\n\ntexto\n\n## Testes\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/PLANO.md"), "# Plano do demo\n\ntexto\n").unwrap();
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("scripts/eval.sh"), "#!/usr/bin/env bash\n# Avalia o demo com modelos reais.\nset -e\n").unwrap();
        fs::create_dir_all(dir.join("skills/alfa")).unwrap();
        fs::write(dir.join("skills/alfa/SKILL.md"), "---\nname: alfa\ndescription: Faz a coisa alfa. Use quando pedirem alfa.\n---\n# Alfa\n").unwrap();
        fs::create_dir_all(dir.join("agents")).unwrap();
        fs::write(
            dir.join("agents/bitclaw.toml"),
            "name = \"Demo\"\ntools = [\"list_dir\", \"read_file\"]\n\n[model]\nname = \"m:1b\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    #[test]
    fn draft_says_what_the_project_is_from_the_readme() {
        let text = draft(&project("about")).unwrap();
        assert!(text.starts_with("# demo\n\nAgente de exemplo, local e enxuto. Lê e edita arquivos.\n\nO objetivo é provar o conceito.\n"), "{text}");
        assert!(!text.contains("<p align") && !text.contains("a.svg"), "HTML e imagens do README ficam de fora:\n{text}");
    }

    #[test]
    fn draft_describes_the_stack_and_the_code() {
        let text = draft(&project("stack")).unwrap();
        assert!(text.contains("- Rust, edição 2024"), "{text}");
        assert!(text.contains("- Dependências: serde, toml"), "{text}");
        assert!(text.contains("- `src/main.rs`: Binário do demo."), "{text}");
        assert!(text.contains("- `src/ui/`: Interface do demo, em tela cheia."), "{text}");
        assert!(text.contains("  - `src/ui/theme.rs`: Cores e estilos."), "{text}");
        assert!(text.contains("- `src/lib.rs`\n"), "arquivo sem comentário aparece sem descrição:\n{text}");
    }

    #[test]
    fn draft_describes_docs_scripts_skills_and_agents() {
        let text = draft(&project("rest")).unwrap();
        assert!(text.contains("- `README.md`: seções Como rodar, Testes"), "{text}");
        assert!(text.contains("- `docs/PLANO.md`: Plano do demo"), "{text}");
        assert!(text.contains("- `scripts/eval.sh`: Avalia o demo com modelos reais."), "{text}");
        assert!(text.contains("- `skills/alfa/SKILL.md`: Faz a coisa alfa."), "{text}");
        assert!(!text.contains("Use quando pedirem alfa"), "só a primeira frase:\n{text}");
        assert!(text.contains("- `agents/bitclaw.toml`: Demo; modelo m:1b; ferramentas list_dir, read_file"), "{text}");
        assert!(text.contains("cargo test") && text.contains("cargo run"), "{text}");
        assert!(!text.contains("target") && !text.contains(".git"), "gerados e ocultos ficam de fora:\n{text}");
    }

    #[test]
    fn a_project_with_nothing_still_gets_a_valid_draft() {
        let dir = std::env::temp_dir().join(format!("bitclaw-init-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let text = draft(&dir).unwrap();
        assert!(text.contains("Só fatos verificados"), "{text}");
        assert!(text.contains("## Notas"), "{text}");
    }

    #[test]
    fn sections_are_separated_by_a_blank_line() {
        let text = draft(&project("spacing")).unwrap();
        for line in text.lines().enumerate().filter(|(i, l)| *i > 0 && l.starts_with("## ")) {
            let before = text.lines().nth(line.0 - 1).unwrap();
            assert_eq!(before, "", "falta linha em branco antes de `{}`:\n{text}", line.1);
        }
    }

    #[test]
    fn draft_is_the_same_every_time() {
        let dir = project("stable");
        assert_eq!(draft(&dir).unwrap(), draft(&dir).unwrap());
    }

    #[test]
    fn first_sentence_cuts_at_the_first_full_stop_and_drops_a_trailing_colon() {
        assert_eq!(first_sentence("Ciclo A → B. Não conhece o resto."), "Ciclo A → B.");
        assert_eq!(first_sentence("Mostra o rascunho:"), "Mostra o rascunho");
        assert_eq!(first_sentence("Abre `AGENTS.md` sem ponto"), "Abre `AGENTS.md` sem ponto");
        assert!(first_sentence(&"x".repeat(500)).chars().count() <= MAX_DESC + 1);
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
    fn an_existing_file_gets_only_the_missing_sections_and_keeps_its_text() {
        let dir = project("existing");
        fs::write(dir.join(FILE), "# demo\n\nMinha descrição.\n\n## Comandos\n\nmeus comandos\n").unwrap();
        let yes = Answer(true, Cell::new(false));
        let msg = write(&dir, &yes).unwrap();
        assert!(yes.1.get(), "pede aprovação antes de acrescentar");
        assert!(msg.contains("acrescentei") && msg.contains("Código"), "{msg}");
        let after = fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(after.starts_with("# demo\n\nMinha descrição.\n\n## Comandos\n\nmeus comandos\n"), "o texto antigo fica intacto:\n{after}");
        assert!(after.contains("## Código") && after.contains("## Skills"), "{after}");
        assert_eq!(after.matches("## Comandos").count(), 1, "seção que já existia não se repete:\n{after}");
        assert!(!after.contains("## Notas"), "o espaço de notas não é acrescentado");
        // Rodar de novo não acrescenta nada.
        let again = write(&dir, &Answer(true, Cell::new(false))).unwrap();
        assert!(again.contains("todas as seções"), "{again}");
        assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), after);
    }

    #[test]
    fn refusing_to_complete_leaves_the_file_as_it_was() {
        let dir = project("existing-refused");
        fs::write(dir.join(FILE), "meu conteúdo\n").unwrap();
        let msg = write(&dir, &Answer(false, Cell::new(false))).unwrap();
        assert!(msg.contains("não foi alterado"), "{msg}");
        assert_eq!(fs::read_to_string(dir.join(FILE)).unwrap(), "meu conteúdo\n");
    }

    #[test]
    #[ignore]
    fn dump_this_repo() {
        println!("{}", draft(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap());
    }
}

//! Skills: instruções em pastas com `SKILL.md`, carregadas sob demanda.
//!
//! O prompt do sistema recebe só a lista de nomes e descrições. O corpo da skill (e os arquivos
//! de referência) chega ao modelo quando ele chama `load_skill`. Assim, um modelo pequeno só gasta
//! contexto com a skill que de fato usa.

use crate::core::tool::{Outcome, Tool, ToolContext, ToolSpec, MAX_READ_BYTES};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

// Nomes das skills que o usuário desligou no `/skills`. Compartilhado entre a interface e a sessão.
pub type Disabled = Arc<Mutex<HashSet<String>>>;

pub fn new_disabled() -> Disabled {
    Arc::new(Mutex::new(HashSet::new()))
}

// Snapshot do conjunto de desligadas (lido com segurança mesmo se a outra ponta falhou).
pub fn disabled_now(disabled: &Disabled) -> HashSet<String> {
    disabled.lock().map(|s| s.clone()).unwrap_or_else(|e| e.into_inner().clone())
}

// Custo aproximado, em tokens, da entrada da skill no catálogo do prompt (~4 caracteres por token).
pub fn catalog_tokens(skill: &Skill) -> usize {
    (skill.name.chars().count() + skill.description.chars().count()) / 4 + 4
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    // Pasta da skill, absoluta.
    pub dir: PathBuf,
    // Conteúdo do SKILL.md sem o frontmatter.
    pub body: String,
    // Arquivos da pasta além do SKILL.md, relativos a ela (ex.: "references/x.md").
    pub files: Vec<String>,
    // Skill embutida no binário (não vem de uma pasta do projeto).
    pub builtin: bool,
}

// Uma skill que acompanha o BitClaw: o SKILL.md e os arquivos de referência, embutidos no binário.
// Assim funcionam num binário instalado, longe do repositório.
struct Bundled {
    text: &'static str,
    // (caminho relativo à pasta da skill, conteúdo)
    files: &'static [(&'static str, &'static str)],
}

// Skills que acompanham o BitClaw. Entram no catálogo quando o projeto não tem skill de mesmo nome.
// Só entram as que servem a qualquer projeto; `small-model-optimization` é pesquisa deste repositório.
const BUILTIN: &[Bundled] = &[
    Bundled { text: include_str!("../../skills/create-skill/SKILL.md"), files: &[] },
    Bundled {
        text: include_str!("../../skills/markdown/SKILL.md"),
        files: &[("references/microsoft-learn.md", include_str!("../../skills/markdown/references/microsoft-learn.md"))],
    },
];

pub fn builtins() -> Vec<Skill> {
    BUILTIN
        .iter()
        .filter_map(|b| {
            let parsed = parse(b.text).ok()?;
            Some(Skill {
                name: parsed.name,
                description: parsed.description,
                dir: PathBuf::new(),
                body: parsed.body,
                files: b.files.iter().map(|(rel, _)| rel.to_string()).collect(),
                builtin: true,
            })
        })
        .collect()
}

// Conteúdo de um arquivo de referência de uma skill embutida.
fn bundled_file(skill: &str, file: &str) -> Option<&'static str> {
    BUILTIN
        .iter()
        .filter(|b| parse(b.text).is_ok_and(|p| p.name == skill))
        .flat_map(|b| b.files.iter())
        .find(|(rel, _)| *rel == file)
        .map(|(_, text)| *text)
}

// Junta skills do projeto com as embutidas. Uma skill do projeto com o mesmo nome prevalece.
pub fn with_builtins(mut project: Vec<Skill>) -> Vec<Skill> {
    for skill in builtins() {
        if !project.iter().any(|s| s.name == skill.name) {
            project.push(skill);
        }
    }
    project
}

#[derive(Debug, PartialEq)]
struct Parsed {
    name: String,
    description: String,
    body: String,
}

// Lê o frontmatter YAML das chaves de primeiro nível `name` e `description`, e separa o corpo.
// Entende valores de uma linha, valores em várias linhas indentadas e blocos `>`/`>-` (dobrado) e
// `|`/`|-` (literal). Chaves aninhadas (ex.: `metadata:`) são ignoradas.
fn parse(text: &str) -> Result<Parsed, String> {
    // Linhas em branco antes do frontmatter são ignoradas.
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    if lines.next().map(str::trim_end) != Some("---") {
        return Err("SKILL.md precisa começar com frontmatter '---'".into());
    }
    let mut front: Vec<&str> = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        front.push(line);
    }
    if !closed {
        return Err("frontmatter do SKILL.md não fecha com '---'".into());
    }
    let body: Vec<&str> = lines.collect();

    let mut name = None;
    let mut description = None;
    let mut i = 0;
    while i < front.len() {
        let line = front[i];
        i += 1;
        // Só chaves sem indentação são de primeiro nível.
        if line.starts_with(char::is_whitespace) || line.trim().is_empty() {
            continue;
        }
        let Some((key, raw)) = line.split_once(':') else { continue };
        // Continuação: linhas seguintes indentadas (ou em branco entre elas).
        let mut cont: Vec<&str> = Vec::new();
        while i < front.len() && (front[i].starts_with(char::is_whitespace) || front[i].trim().is_empty()) {
            cont.push(front[i].trim());
            i += 1;
        }
        let raw = raw.trim();
        let value = if raw.starts_with('>') || raw.starts_with('|') {
            // Bloco: linhas indentadas. Dobrado (`>`) junta com espaço; literal (`|`) mantém quebras.
            let sep = if raw.starts_with('|') { "\n" } else { " " };
            cont.iter().filter(|l| !l.is_empty()).copied().collect::<Vec<_>>().join(sep)
        } else {
            let mut parts = vec![raw.trim_matches('"')];
            parts.extend(cont.iter().filter(|l| !l.is_empty()).copied());
            parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
        };
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }

    let name = name.filter(|n| !n.trim().is_empty()).ok_or("SKILL.md sem 'name'")?;
    let description = description.filter(|d| !d.trim().is_empty()).ok_or("SKILL.md sem 'description'")?;
    Ok(Parsed { name, description, body: body.join("\n").trim().to_string() })
}

// Lista os arquivos de uma pasta, recursivamente, exceto o SKILL.md. Caminhos com "/".
fn list_files(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| e.to_string())?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{rel}{name}");
        if entry.path().is_dir() {
            list_files(&entry.path(), &format!("{path}/"), out)?;
        } else if path != "SKILL.md" {
            out.push(path);
        }
    }
    Ok(())
}

// Carrega as skills de uma pasta: cada subpasta com um SKILL.md é uma skill.
// Pasta inexistente significa "sem skills". Um SKILL.md inválido é erro, para não ser ignorado em silêncio.
pub fn load(dir: &Path) -> Result<Vec<Skill>, String> {
    load_with_ignored(dir).map(|(skills, _)| skills)
}

// Como `load`, e também devolve os nomes de entradas ignoradas (arquivos soltos ou pastas sem SKILL.md),
// para avisar o usuário em vez de deixar a skill desaparecer em silêncio.
pub fn load_with_ignored(dir: &Path) -> Result<(Vec<Skill>, Vec<String>), String> {
    if !dir.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut ignored: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("não consegui ler {}: {e}", dir.display()))?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").is_file() {
            paths.push(path);
        } else {
            ignored.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    paths.sort();
    ignored.sort();

    let mut skills: Vec<Skill> = Vec::new();
    for path in paths {
        let folder = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        // Uma skill inválida não derruba o programa: ela é ignorada e o motivo aparece no aviso.
        let text = match fs::read_to_string(path.join("SKILL.md")) {
            Ok(t) => t,
            Err(e) => {
                ignored.push(format!("{folder} ({e})"));
                continue;
            }
        };
        let parsed = match parse(&text) {
            Ok(p) => p,
            Err(e) => {
                ignored.push(format!("{folder} ({e})"));
                continue;
            }
        };
        if skills.iter().any(|s| s.name == parsed.name) {
            ignored.push(format!("{folder} (nome repetido: {})", parsed.name));
            continue;
        }
        let mut files = Vec::new();
        list_files(&path, "", &mut files)?;
        skills.push(Skill {
            name: parsed.name,
            description: parsed.description,
            dir: path,
            body: parsed.body,
            files,
            builtin: false,
        });
    }
    Ok((skills, ignored))
}

// Lê uma única skill: `dir` é a pasta dela, com o SKILL.md dentro. Usado para validar uma skill
// antes de instalá-la.
pub fn load_one(dir: &Path) -> Result<Skill, String> {
    let text = fs::read_to_string(dir.join("SKILL.md")).map_err(|e| format!("não consegui ler SKILL.md: {e}"))?;
    let parsed = parse(&text)?;
    let mut files = Vec::new();
    list_files(dir, "", &mut files)?;
    Ok(Skill { name: parsed.name, description: parsed.description, dir: dir.to_path_buf(), body: parsed.body, files, builtin: false })
}

// Carrega as skills de uma configuração. `dir` é relativo a `base` (normalmente o diretório de execução).
pub fn load_configured(dir: Option<&str>, base: &Path) -> Result<Vec<Skill>, String> {
    match dir {
        Some(d) => load(&base.join(d)),
        None => Ok(Vec::new()),
    }
}

// Uma linha por skill, para exibir ao usuário (`/skills`).
pub fn summary(skills: &[Skill]) -> Vec<String> {
    skills
        .iter()
        .map(|s| {
            let origin = if s.builtin { " (embutida)" } else { "" };
            format!("{}{origin} — {}", s.name, s.description)
        })
        .collect()
}

// Trecho do prompt do sistema que apresenta as skills. Vazio quando não há skills.
pub fn catalog(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut text = String::from(
        "Skills disponíveis. Antes de aplicar uma delas, chame load_skill com o nome para ler as instruções:",
    );
    for skill in skills {
        text.push_str(&format!("\n- {}: {}", skill.name, skill.description));
    }
    text
}

// Devolve uma parte do texto a partir de `offset` (em bytes), com até MAX_READ_BYTES.
// Quebra em uma linha quando possível e avisa como continuar. Assim, uma skill grande é lida em partes.
fn page(text: &str, offset: usize) -> Result<String, String> {
    if offset > text.len() {
        return Err(format!("offset {offset} além do fim do texto ({} bytes)", text.len()));
    }
    let mut start = offset;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (start + MAX_READ_BYTES).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end < text.len() {
        if let Some(nl) = text[start..end].rfind('\n').filter(|&n| n > MAX_READ_BYTES / 2) {
            end = start + nl + 1;
        }
    }
    let mut out = text[start..end].to_string();
    if end < text.len() {
        out.push_str(&format!(
            "\n[parte de {} bytes; continua. Chame load_skill de novo com offset={end} para ler o restante]",
            text.len()
        ));
    }
    Ok(out)
}

// Ferramenta `load_skill`: devolve as instruções de uma skill, ou um arquivo de referência dela.
pub struct LoadSkill {
    skills: Vec<Skill>,
    disabled: Disabled,
}

impl LoadSkill {
    pub fn new(skills: Vec<Skill>, disabled: Disabled) -> Self {
        Self { skills, disabled }
    }

    fn find(&self, name: &str) -> Result<&Skill, String> {
        if disabled_now(&self.disabled).contains(name) {
            return Err(format!("skill desativada pelo usuário: {name}"));
        }
        self.skills.iter().find(|s| s.name == name).ok_or_else(|| {
            let names: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
            format!("skill desconhecida: {name}. Disponíveis: {}", names.join(", "))
        })
    }

    fn read_reference(skill: &Skill, file: &str, offset: usize) -> Result<String, String> {
        let rel = Path::new(file.trim());
        if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir)) {
            return Err("caminho de arquivo inválido na skill".into());
        }
        // Skill embutida não tem pasta: o arquivo vem do binário.
        if skill.builtin {
            let text = bundled_file(&skill.name, file.trim())
                .ok_or_else(|| format!("arquivo '{file}' não existe na skill {}; os dela são: {}", skill.name, skill.files.join(", ")))?;
            return page(text, offset);
        }
        let path = skill.dir.join(rel);
        // Confere também links simbólicos: o arquivo final precisa continuar dentro da pasta da skill.
        let real = path.canonicalize().map_err(|e| format!("arquivo '{file}': {e}"))?;
        let base = skill.dir.canonicalize().map_err(|e| e.to_string())?;
        if !real.starts_with(&base) {
            return Err("arquivo fora da pasta da skill".into());
        }
        let text = fs::read_to_string(&real).map_err(|e| format!("arquivo '{file}': {e}"))?;
        page(&text, offset)
    }
}

impl Tool for LoadSkill {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn annotations(&self) -> crate::core::tool::Annotations {
        crate::core::tool::Annotations { read_only: true, destructive: false, idempotent: false }
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "load_skill",
            "Lê as instruções de uma skill listada no prompt. Com 'file', lê um arquivo de referência dela. Textos grandes vêm em partes: use 'offset' para continuar.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Nome da skill"},
                    "file": {"type": "string", "description": "Arquivo de referência dentro da skill, ex.: references/x.md"},
                    "offset": {"type": "integer", "description": "Byte onde começar, para ler a partir de uma parte anterior (padrão 0)"}
                },
                "required": ["name"]
            }),
        )
    }

    fn run(&self, _ctx: &ToolContext, args: &Value) -> Outcome {
        let result = (|| {
            let name = args.get("name").and_then(Value::as_str).ok_or("argumento 'name' ausente ou inválido")?;
            let skill = self.find(name.trim())?;
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            match args.get("file").and_then(Value::as_str).map(str::trim).filter(|f| !f.is_empty()) {
                Some(file) => Self::read_reference(skill, file, offset),
                None => {
                    let mut text = page(&skill.body, offset)?;
                    if !skill.files.is_empty() {
                        text.push_str(&format!(
                            "\n\nArquivos de referência (chame load_skill com 'file'): {}",
                            skill.files.join(", ")
                        ));
                    }
                    Ok(text)
                }
            }
        })();
        match result {
            Ok(text) => Outcome::Done(text),
            Err(e) => Outcome::Error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tool::AutoApprover;
    use std::fs;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-skills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_skill(root: &Path, folder: &str, text: &str) {
        let dir = root.join(folder);
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(dir.join("SKILL.md"), text).unwrap();
        fs::write(dir.join("references/extra.md"), "conteúdo extra").unwrap();
    }

    const VALID: &str = "---\nname: demo\ndescription: Faz demonstrações: com dois pontos.\n---\n\n# Demo\n\nPasso um.\n";

    #[test]
    fn parses_frontmatter_and_body() {
        let p = parse(VALID).unwrap();
        assert_eq!(p.name, "demo");
        assert_eq!(p.description, "Faz demonstrações: com dois pontos.");
        assert_eq!(p.body, "# Demo\n\nPasso um.");
    }

    #[test]
    fn folded_block_description_and_nested_metadata_are_read() {
        let text = "---\nname: migrate-to-rust\nmetadata:\n  version: \"1.0.0\"\n  category: ApplicationDevelopment\ndescription: >-\n  Clones, analyzes and migrates legacy codebases.\n  Use when porting C++ to Rust.\n---\n\n# Corpo\n";
        let p = parse(text).unwrap();
        assert_eq!(p.name, "migrate-to-rust");
        assert_eq!(p.description, "Clones, analyzes and migrates legacy codebases. Use when porting C++ to Rust.");
        assert_eq!(p.body, "# Corpo");
    }

    #[test]
    fn multi_line_plain_description_is_joined() {
        let p = parse("---\nname: a\ndescription: primeira parte\n  segunda parte\n---\n").unwrap();
        assert_eq!(p.description, "primeira parte segunda parte");
    }

    #[test]
    fn leading_blank_lines_before_frontmatter_are_allowed() {
        assert_eq!(parse(&format!("\n\n{VALID}")).unwrap().name, "demo");
    }

    #[test]
    fn rejects_missing_frontmatter_pieces() {
        assert!(parse("# sem frontmatter").is_err());
        assert!(parse("---\nname: a\ndescription: b\n").is_err(), "frontmatter sem fechamento");
        assert!(parse("---\ndescription: b\n---\n").is_err(), "sem name");
        assert!(parse("---\nname: a\n---\n").is_err(), "sem description");
    }

    #[test]
    fn load_finds_skills_and_their_files_and_ignores_other_folders() {
        let root = tempdir("load");
        write_skill(&root, "demo", VALID);
        fs::create_dir_all(root.join("sem-skill")).unwrap();
        let skills = load(&root).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "demo");
        assert_eq!(skills[0].files, vec!["references/extra.md".to_string()]);
    }

    #[test]
    fn load_of_missing_dir_is_empty_and_bad_skill_is_ignored_with_reason() {
        let root = tempdir("bad");
        assert!(load(&root.join("nao-existe")).unwrap().is_empty());
        write_skill(&root, "ruim", "sem frontmatter");
        let (skills, ignored) = load_with_ignored(&root).unwrap();
        assert!(skills.is_empty());
        assert_eq!(ignored.len(), 1);
        assert!(ignored[0].starts_with("ruim (") && ignored[0].contains("frontmatter"), "{ignored:?}");
    }

    #[test]
    fn loose_files_and_folders_without_skill_md_are_reported_as_ignored() {
        let root = tempdir("ignored");
        write_skill(&root, "demo", VALID);
        fs::write(root.join("DREAM.md"), "solto").unwrap();
        fs::create_dir_all(root.join("sem-arquivo")).unwrap();
        let (skills, ignored) = load_with_ignored(&root).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(ignored, vec!["DREAM.md".to_string(), "sem-arquivo".to_string()]);
    }

    #[test]
    fn builtin_create_skill_is_available_and_project_skill_overrides_it() {
        assert!(builtins().iter().any(|s| s.name == "create-skill" && s.builtin));
        let root = tempdir("override");
        write_skill(&root, "create-skill", "---\nname: create-skill\ndescription: versão do projeto\n---\n\nCorpo do projeto.\n");
        let all = with_builtins(load(&root).unwrap());
        let found: Vec<&Skill> = all.iter().filter(|s| s.name == "create-skill").collect();
        assert_eq!(found.len(), 1);
        assert!(!found[0].builtin);
        assert_eq!(found[0].description, "versão do projeto");
    }

    // Num binário instalado não há `skills/` por perto: a skill e a referência vêm do binário.
    #[test]
    fn builtin_markdown_skill_serves_its_reference_without_a_folder() {
        let skills = builtins();
        let md = skills.iter().find(|s| s.name == "markdown").expect("markdown embutida");
        assert!(md.builtin && md.dir.as_os_str().is_empty());
        assert_eq!(md.files, vec!["references/microsoft-learn.md".to_string()]);
        let empty = tempdir("builtin-ref");
        let tool = LoadSkill::new(skills.clone(), new_disabled());
        let ctx = ToolContext::new(&empty, &AutoApprover);
        assert!(tool.run(&ctx, &json!({"name": "markdown"})).text().contains("references/microsoft-learn.md"));
        let Outcome::Done(text) = tool.run(&ctx, &json!({"name": "markdown", "file": "references/microsoft-learn.md"})) else { panic!() };
        assert!(text.contains(":::"), "conteúdo do arquivo de referência: {text}");
        let Outcome::Error(e) = tool.run(&ctx, &json!({"name": "markdown", "file": "references/nada.md"})) else { panic!() };
        assert!(e.contains("microsoft-learn.md"), "o erro diz quais arquivos existem: {e}");
        assert!(matches!(tool.run(&ctx, &json!({"name": "markdown", "file": "../x"})), Outcome::Error(_)));
    }

    #[test]
    fn duplicate_names_are_error() {
        let root = tempdir("dup");
        write_skill(&root, "a", VALID);
        write_skill(&root, "b", VALID);
        let (skills, ignored) = load_with_ignored(&root).unwrap();
        assert_eq!(skills.len(), 1);
        assert!(ignored.iter().any(|i| i.contains("nome repetido")), "{ignored:?}");
    }

    #[test]
    fn summary_has_one_line_per_skill() {
        let root = tempdir("summary");
        write_skill(&root, "demo", VALID);
        assert_eq!(summary(&load(&root).unwrap()), vec!["demo — Faz demonstrações: com dois pontos.".to_string()]);
        assert!(summary(&[]).is_empty());
    }

    #[test]
    fn catalog_lists_names_and_descriptions_only() {
        let root = tempdir("catalog");
        write_skill(&root, "demo", VALID);
        let text = catalog(&load(&root).unwrap());
        assert!(text.contains("- demo: Faz demonstrações"));
        assert!(!text.contains("Passo um"), "o corpo não vai para o prompt");
        assert!(catalog(&[]).is_empty());
    }

    #[test]
    fn load_skill_returns_body_then_reference_files() {
        let root = tempdir("tool");
        write_skill(&root, "demo", VALID);
        let tool = LoadSkill::new(load(&root).unwrap(), new_disabled());
        let out = tool.run(&ToolContext::new(&root, &AutoApprover), &json!({"name": "demo"}));
        let text = out.text();
        assert!(text.starts_with("# Demo"));
        assert!(text.contains("references/extra.md"));
        let file = tool.run(&ToolContext::new(&root, &AutoApprover), &json!({"name": "demo", "file": "references/extra.md"}));
        assert_eq!(file, Outcome::Done("conteúdo extra".into()));
    }

    #[test]
    fn large_references_are_read_in_parts() {
        let root = tempdir("paging");
        write_skill(&root, "demo", VALID);
        let big = "linha\n".repeat(3000); // ~18 KB
        fs::write(root.join("demo/references/big.md"), &big).unwrap();
        let tool = LoadSkill::new(load(&root).unwrap(), new_disabled());
        let first = tool.run(&ToolContext::new(&root, &AutoApprover), &json!({"name": "demo", "file": "references/big.md"})).text();
        assert!(first.contains("offset="), "avisa como continuar: {first}");
        let marker = first.split("offset=").nth(1).unwrap();
        let offset: usize = marker.split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap();
        let second = tool.run(&ToolContext::new(&root, &AutoApprover), &json!({"name": "demo", "file": "references/big.md", "offset": offset})).text();
        assert!(second.starts_with("linha"));
        assert!(matches!(
            tool.run(&ToolContext::new(&root, &AutoApprover), &json!({"name": "demo", "file": "references/big.md", "offset": 10_000_000})),
            Outcome::Error(_)
        ));
    }

    #[test]
    fn load_skill_rejects_unknown_skill_and_escaping_paths() {
        let root = tempdir("reject");
        write_skill(&root, "demo", VALID);
        fs::write(root.join("secreto.txt"), "não pode").unwrap();
        let tool = LoadSkill::new(load(&root).unwrap(), new_disabled());
        assert!(matches!(tool.run(&ToolContext::new(&root, &AutoApprover), &json!({"name": "outra"})), Outcome::Error(_)));
        let escape = json!({"name": "demo", "file": "../secreto.txt"});
        assert!(matches!(tool.run(&ToolContext::new(&root, &AutoApprover), &escape), Outcome::Error(_)));
        let abs = json!({"name": "demo", "file": "/etc/hosts"});
        assert!(matches!(tool.run(&ToolContext::new(&root, &AutoApprover), &abs), Outcome::Error(_)));
    }
}

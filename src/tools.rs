use crate::sandbox::resolve;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Limite do resultado de `read_file`. Com num_ctx de 8192 tokens, 100 KB estouraria o contexto de um modelo de 4B.
pub const MAX_READ_BYTES: usize = 8_000;
const BACKUP_DIR: &str = ".bitclaw/backup";
const CONTEXT_LINES: usize = 2;

// O que realmente aconteceu com uma chamada de ferramenta.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    // A ferramenta executou e o resultado foi aplicado (ou lido).
    Done(String),
    // O usuário recusou a alteração na confirmação.
    Refused(String),
    // A ferramenta falhou; nada foi alterado.
    Error(String),
}

impl Outcome {
    // Texto devolvido ao modelo.
    pub fn text(&self) -> String {
        match self {
            Outcome::Done(s) | Outcome::Refused(s) => s.clone(),
            Outcome::Error(e) => format!("ERRO: {e}"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Done(_) => "ok",
            Outcome::Refused(_) => "recusada",
            Outcome::Error(_) => "erro",
        }
    }
}

// Decide se uma alteração proposta pode ser aplicada. Injetado para não ler o stdin dentro da ferramenta.
pub trait Approver {
    fn approve(&self) -> bool;
}

// Aprova tudo (uso com `--yes` e testes).
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn approve(&self) -> bool {
        true
    }
}

// Pergunta no terminal.
pub struct StdinApprover;

impl Approver for StdinApprover {
    fn approve(&self) -> bool {
        confirm().unwrap_or(false)
    }
}

// Contrato de uma ferramenta: nome, esquema e execução juntos.
// Adicionar uma ferramenta é implementar esta trait e registrá-la; o loop do agente não muda.
pub trait Tool {
    fn name(&self) -> &'static str;

    // Entrada completa no formato `{"type": "function", "function": {...}}` que o Ollama espera.
    fn schema(&self) -> Value;

    // Se a ferramenta altera arquivos. Usado pelo relatório do turno e pela proteção contra repetição.
    fn mutates(&self) -> bool {
        false
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome;
}

// Conjunto de ferramentas disponíveis para um agente. É o único lugar que o loop consulta.
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total, "nomes de ferramenta duplicados");
        Self { tools }
    }

    // Ferramentas que o BitClaw traz de fábrica.
    pub fn default_set() -> Self {
        Self::from_names(BUILTIN_TOOLS).expect("as ferramentas embutidas existem")
    }

    // Monta o registro a partir de nomes de ferramentas embutidas (ex.: vindos de uma configuração).
    // Nome desconhecido é erro, para que um erro de digitação não passe despercebido.
    pub fn from_names<S: AsRef<str>>(names: &[S]) -> Result<Self, String> {
        let mut tools: Vec<Box<dyn Tool>> = Vec::with_capacity(names.len());
        for name in names {
            let name = name.as_ref();
            if tools.iter().any(|t| t.name() == name) {
                return Err(format!("ferramenta repetida na configuração: {name}"));
            }
            let tool = builtin(name).ok_or_else(|| {
                format!("ferramenta desconhecida na configuração: {name}. Disponíveis: {}", BUILTIN_TOOLS.join(", "))
            })?;
            tools.push(tool);
        }
        if tools.is_empty() {
            return Err("nenhuma ferramenta configurada".into());
        }
        Ok(Self::new(tools))
    }

    pub fn schemas(&self) -> Value {
        Value::Array(self.tools.iter().map(|t| t.schema()).collect())
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn mutates(&self, name: &str) -> bool {
        self.find(name).is_some_and(|t| t.mutates())
    }

    pub fn execute(&self, root: &Path, name: &str, args: &Value, approver: &dyn Approver) -> Outcome {
        match self.find(name) {
            Some(tool) => tool.run(root, args, approver),
            None => Outcome::Error(format!(
                "ferramenta desconhecida: {name}. Use uma destas: {}",
                self.names().join(", ")
            )),
        }
    }

    fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }
}

// Nomes das ferramentas embutidas, na ordem padrão.
pub const BUILTIN_TOOLS: &[&str] = &["list_dir", "read_file", "edit_file", "create_file", "delete_file"];

// Constrói uma ferramenta embutida pelo nome. Nome desconhecido devolve `None`.
fn builtin(name: &str) -> Option<Box<dyn Tool>> {
    match name {
        "list_dir" => Some(Box::new(ListDir)),
        "read_file" => Some(Box::new(ReadFile)),
        "edit_file" => Some(Box::new(EditFile)),
        "create_file" => Some(Box::new(CreateFile)),
        "delete_file" => Some(Box::new(DeleteFile)),
        _ => None,
    }
}

fn wrap(result: Result<Outcome, String>) -> Outcome {
    result.unwrap_or_else(Outcome::Error)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("argumento '{key}' ausente ou inválido"))
}

pub struct ListDir;

impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "list_dir",
                "description": "Lista arquivos e pastas de um diretório do projeto. Use \".\" para a raiz.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"}
                    },
                    "required": ["path"]
                }
            }
        })
    }

    fn run(&self, root: &Path, args: &Value, _approver: &dyn Approver) -> Outcome {
        wrap(list_dir(root, args).map(Outcome::Done))
    }
}

fn list_dir(root: &Path, args: &Value) -> Result<String, String> {
    let path = resolve(root, str_arg(args, "path")?)?;
    let mut entries: Vec<String> = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() { format!("{name}/") } else { name }
        })
        .filter(|n| !n.starts_with(".bitclaw"))
        .collect();
    entries.sort();
    Ok(entries.join("\n"))
}

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Lê o conteúdo de um arquivo de texto do projeto.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"}
                    },
                    "required": ["path"]
                }
            }
        })
    }

    fn run(&self, root: &Path, args: &Value, _approver: &dyn Approver) -> Outcome {
        wrap(read_file(root, args).map(Outcome::Done))
    }
}

fn read_file(root: &Path, args: &Value) -> Result<String, String> {
    let path = resolve(root, str_arg(args, "path")?)?;
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if content.len() > MAX_READ_BYTES {
        let mut cut = MAX_READ_BYTES;
        while !content.is_char_boundary(cut) {
            cut -= 1;
        }
        return Ok(format!("{}\n[... truncado em {MAX_READ_BYTES} bytes]", &content[..cut]));
    }
    Ok(content)
}

pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Substitui um trecho EXATO (`old`) por `new` em um arquivo existente. `old` deve aparecer exatamente uma vez no arquivo. O usuário confirma antes da alteração. Leia o arquivo antes de editar.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto"},
                        "old": {"type": "string", "description": "Trecho exato a ser substituído"},
                        "new": {"type": "string", "description": "Novo texto"}
                    },
                    "required": ["path", "old", "new"]
                }
            }
        })
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome {
        wrap(edit_file(root, args, approver))
    }
}

fn edit_file(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let old = str_arg(args, "old")?;
    let new = str_arg(args, "new")?;
    if old.is_empty() {
        return Err("'old' não pode ser vazio".into());
    }
    let path = resolve(root, rel)?;
    if !path.is_file() {
        return Err("arquivo não existe (edit_file só altera arquivos existentes; use create_file para criar)".into());
    }

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let at = match content.matches(old).count() {
        0 => return Err("trecho 'old' não encontrado no arquivo. Use read_file e copie o trecho exato, caractere por caractere".into()),
        1 => content.find(old).unwrap_or(0),
        n => return Err(format!("trecho 'old' aparece {n} vezes; torne-o mais específico")),
    };
    let updated = content.replacen(old, new, 1);

    show_edit_preview(rel, &updated, content[..at].matches('\n').count(), old, new);
    if !approver.approve() {
        return Ok(Outcome::Refused("usuário recusou a edição; nada foi alterado".into()));
    }

    // Backup antes de escrever.
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let backup_name = rel.replace(['/', '\\'], "_");
    let backup_dir = root.join(BACKUP_DIR);
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let backup_path = backup_dir.join(format!("{backup_name}.{ts}.bak"));
    fs::write(&backup_path, &content).map_err(|e| e.to_string())?;

    fs::write(&path, &updated).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!(
        "edição aplicada em {rel} (backup: {BACKUP_DIR}/). Não repita esta edição; se a tarefa estiver completa, responda ao usuário sem chamar mais ferramentas."
    )))
}

pub struct CreateFile;

impl Tool for CreateFile {
    fn name(&self) -> &'static str {
        "create_file"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "create_file",
                "description": "Cria um arquivo NOVO no projeto com o conteúdo dado. Falha se o arquivo já existir (use edit_file nesse caso). O usuário confirma antes da criação.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto, ex.: fib.py"},
                        "content": {"type": "string", "description": "Conteúdo completo do arquivo"}
                    },
                    "required": ["path", "content"]
                }
            }
        })
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome {
        wrap(create_file(root, args, approver))
    }
}

pub struct DeleteFile;

impl Tool for DeleteFile {
    fn name(&self) -> &'static str {
        "delete_file"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "delete_file",
                "description": "Remove um arquivo existente do projeto. Guarda cópia em .bitclaw/backup/. Não remove pastas. O usuário confirma antes da remoção.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Caminho relativo ao projeto do arquivo a remover"}
                    },
                    "required": ["path"]
                }
            }
        })
    }

    fn mutates(&self) -> bool {
        true
    }

    fn run(&self, root: &Path, args: &Value, approver: &dyn Approver) -> Outcome {
        wrap(delete_file(root, args, approver))
    }
}

fn delete_file(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let path = resolve(root, rel)?;
    if path.is_dir() {
        return Err(format!("{rel} é uma pasta; delete_file remove só arquivos"));
    }
    if !path.is_file() {
        return Err(format!("{rel} não existe"));
    }

    let content = fs::read_to_string(&path).unwrap_or_default();
    eprintln!("\n\x1b[1m── remover arquivo {rel} ──\x1b[0m");
    eprintln!("\x1b[31m{rel} será removido ({} linhas)\x1b[0m", content.lines().count());
    if !approver.approve() {
        return Ok(Outcome::Refused(format!("usuário recusou a remoção; {rel} foi mantido")));
    }

    // Backup antes de remover, no mesmo formato da edição.
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let backup_name = rel.replace(['/', '\\'], "_");
    let backup_dir = root.join(BACKUP_DIR);
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let backup_path = backup_dir.join(format!("{backup_name}.{ts}.bak"));
    fs::write(&backup_path, &content).map_err(|e| e.to_string())?;

    fs::remove_file(&path).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!("arquivo removido: {rel} (cópia em {BACKUP_DIR}/)")))
}

fn create_file(root: &Path, args: &Value, approver: &dyn Approver) -> Result<Outcome, String> {
    let rel = str_arg(args, "path")?;
    let content = str_arg(args, "content")?;
    let path = resolve(root, rel)?;
    // symlink_metadata também pega links quebrados, que `exists()` não enxerga.
    if path.symlink_metadata().is_ok() {
        return Err(format!("{rel} já existe; use edit_file para alterar um arquivo existente"));
    }

    eprintln!("\n\x1b[1m── novo arquivo {rel} ──\x1b[0m");
    for line in content.lines() {
        eprintln!("\x1b[32m+ {line}\x1b[0m");
    }
    if !approver.approve() {
        return Ok(Outcome::Refused("usuário recusou a criação; nenhum arquivo foi criado".into()));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(Outcome::Done(format!("arquivo criado: {rel}")))
}

// Mostra a edição com algumas linhas de contexto do arquivo resultante,
// para o usuário ver onde o trecho cai, não só o que foi removido e adicionado.
fn show_edit_preview(rel: &str, updated: &str, start_line: usize, old: &str, new: &str) {
    let lines: Vec<&str> = updated.lines().collect();
    let len = lines.len();
    let before_end = start_line.min(len);
    let before_begin = before_end.saturating_sub(CONTEXT_LINES);
    let after_begin = (start_line + new.lines().count()).min(len);
    let after_end = (after_begin + CONTEXT_LINES).min(len);

    eprintln!("\n\x1b[1m── edição proposta em {rel} ──\x1b[0m");
    for line in &lines[before_begin..before_end] {
        eprintln!("  {line}");
    }
    for line in old.lines() {
        eprintln!("\x1b[31m- {line}\x1b[0m");
    }
    for line in new.lines() {
        eprintln!("\x1b[32m+ {line}\x1b[0m");
    }
    for line in &lines[after_begin..after_end] {
        eprintln!("  {line}");
    }
}

fn confirm() -> Result<bool, String> {
    eprint!("Aplicar? [s/N] ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "s" | "sim" | "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Refuse;

    impl Approver for Refuse {
        fn approve(&self) -> bool {
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
        Registry::default_set().execute(root, name, &args, &AutoApprover)
    }

    #[test]
    fn create_file_creates_new_file() {
        let r = root("create");
        let out = run(&r, "create_file", json!({"path": "a.py", "content": "x = 1\n"}));
        assert!(matches!(out, Outcome::Done(_)), "{out:?}");
        assert_eq!(fs::read_to_string(r.join("a.py")).unwrap(), "x = 1\n");
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
        let out = Registry::default_set().execute(
            &r,
            "edit_file",
            &json!({"path": "a.py", "old": "x = 1", "new": "x = 2"}),
            &Refuse,
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
        let out = Registry::default_set().execute(&r, "delete_file", &json!({"path": "a.md"}), &Refuse);
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
        let reg = Registry::default_set();
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
}

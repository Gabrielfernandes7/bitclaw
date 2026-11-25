//! Toolkit: ferramentas prontas e opcionais, construídas sobre o contrato `Tool` do core.

pub mod fs;
pub mod script;

use crate::core::tool::Tool;

// Nomes das ferramentas embutidas, na ordem padrão.
pub const BUILTIN_TOOLS: &[&str] =
    &["list_dir", "read_file", "edit_file", "create_file", "create_dir", "delete_file", "run_script"];

// Constrói uma ferramenta embutida pelo nome. Nome desconhecido devolve `None`.
pub(crate) fn builtin(name: &str) -> Option<Box<dyn Tool>> {
    match name {
        "list_dir" => Some(Box::new(fs::ListDir)),
        "read_file" => Some(Box::new(fs::ReadFile)),
        "edit_file" => Some(Box::new(fs::EditFile)),
        "create_file" => Some(Box::new(fs::CreateFile)),
        "create_dir" => Some(Box::new(fs::CreateDir)),
        "delete_file" => Some(Box::new(fs::DeleteFile)),
        "run_script" => Some(Box::new(script::RunScript)),
        _ => None,
    }
}

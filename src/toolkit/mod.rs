//! Toolkit: ferramentas prontas e opcionais, construídas sobre o contrato `Tool` do core.
//!
//! `builtin_catalog` resolve nomes (os do TOML) em ferramentas. Um fork soma as suas ao registro
//! com `Registry::with_tool`, sem tocar no core.

pub mod claims;
pub mod fs;
pub mod script;

use crate::core::tool::{Registry, Tool};

// Nomes das ferramentas embutidas, na ordem padrão.
pub const BUILTIN_TOOLS: &[&str] =
    &["list_dir", "read_file", "grep_files", "find_files", "edit_file", "create_file", "create_dir", "delete_file", "run_script"];

// Constrói uma ferramenta embutida pelo nome. Nome desconhecido devolve `None`.
pub fn builtin_catalog(name: &str) -> Option<Box<dyn Tool>> {
    match name {
        "list_dir" => Some(Box::new(fs::ListDir)),
        "read_file" => Some(Box::new(fs::ReadFile)),
        "grep_files" => Some(Box::new(fs::GrepFiles)),
        "find_files" => Some(Box::new(fs::FindFiles)),
        "edit_file" => Some(Box::new(fs::EditFile)),
        "create_file" => Some(Box::new(fs::CreateFile)),
        "create_dir" => Some(Box::new(fs::CreateDir)),
        "delete_file" => Some(Box::new(fs::DeleteFile)),
        "run_script" => Some(Box::new(script::RunScript)),
        _ => None,
    }
}

// Monta um registro com as ferramentas embutidas pelos nomes dados (ex.: o campo `tools` do TOML).
// Nome desconhecido ou repetido é erro, para que um erro de digitação não passe despercebido.
pub fn registry_from_names<S: AsRef<str>>(names: &[S]) -> Result<Registry, String> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::with_capacity(names.len());
    for name in names {
        let name = name.as_ref();
        if tools.iter().any(|t| t.name() == name) {
            return Err(format!("ferramenta repetida na configuração: {name}"));
        }
        let tool = builtin_catalog(name).ok_or_else(|| {
            format!("ferramenta desconhecida na configuração: {name}. Disponíveis: {}", BUILTIN_TOOLS.join(", "))
        })?;
        tools.push(tool);
    }
    if tools.is_empty() {
        return Err("nenhuma ferramenta configurada".into());
    }
    Ok(Registry::new(tools))
}

// Todas as ferramentas embutidas.
pub fn default_registry() -> Registry {
    registry_from_names(BUILTIN_TOOLS).expect("as ferramentas embutidas existem")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::AgentConfig;

    #[test]
    fn unknown_tool_name_is_an_error() {
        let err = registry_from_names(&["read_fil"]).err().expect("deveria falhar");
        assert!(err.contains("read_fil"), "{err}");
    }

    #[test]
    fn repeated_and_empty_tool_lists_are_errors() {
        assert!(registry_from_names(&["read_file", "read_file"]).is_err());
        assert!(registry_from_names::<&str>(&[]).is_err());
    }

    #[test]
    fn the_bundled_config_names_only_known_tools() {
        let cfg = AgentConfig::from_toml(include_str!("../../agents/bitclaw.toml")).unwrap();
        assert_eq!(registry_from_names(&cfg.tools).unwrap().names().len(), BUILTIN_TOOLS.len());
    }

    #[test]
    fn every_builtin_has_a_spec_named_after_it() {
        for name in BUILTIN_TOOLS {
            let spec = builtin_catalog(name).unwrap().spec();
            assert_eq!(&spec.name, name);
            assert!(!spec.description.is_empty());
            assert_eq!(spec.parameters["type"], "object");
        }
    }
}

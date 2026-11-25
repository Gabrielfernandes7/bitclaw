//! O conjunto de ferramentas de um agente: o único lugar que o loop consulta.

use super::{Approver, Outcome, Tool};
use serde_json::Value;
use std::path::Path;

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
        Self::from_names(crate::toolkit::BUILTIN_TOOLS).expect("as ferramentas embutidas existem")
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
            let tool = crate::toolkit::builtin(name).ok_or_else(|| {
                format!("ferramenta desconhecida na configuração: {name}. Disponíveis: {}", crate::toolkit::BUILTIN_TOOLS.join(", "))
            })?;
            tools.push(tool);
        }
        if tools.is_empty() {
            return Err("nenhuma ferramenta configurada".into());
        }
        Ok(Self::new(tools))
    }

    // Acrescenta uma ferramenta que não é embutida (ex.: `load_skill`). Nome repetido é erro.
    pub fn with_tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        Self::new(self.tools)
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

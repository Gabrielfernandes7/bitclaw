//! O conjunto de ferramentas de um agente: o único lugar que o loop consulta.

use super::{Outcome, Tool, ToolContext, ToolSpec};
use serde_json::Value;

// Conjunto de ferramentas disponíveis para um agente. Aceita ferramentas de qualquer origem:
// as do toolkit, as de um fork, as que a sessão soma (`load_skill`).
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    // Nome repetido é erro de programação: derruba aqui, não no meio de um turno.
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total, "nomes de ferramenta duplicados");
        Self { tools }
    }

    // Acrescenta uma ferramenta (ex.: `load_skill`). Nome repetido é erro.
    pub fn with_tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        Self::new(self.tools)
    }

    // Junta dois registros (ex.: as ferramentas do toolkit com as de um fork).
    pub fn merge(mut self, other: Registry) -> Self {
        self.tools.extend(other.tools);
        Self::new(self.tools)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    // Ferramenta que altera algo (tudo que não declara `read_only`).
    pub fn mutates(&self, name: &str) -> bool {
        self.find(name).is_some_and(|t| !t.annotations().read_only)
    }

    // Ferramenta que remove ou sobrescreve conteúdo (e altera).
    pub fn destructive(&self, name: &str) -> bool {
        self.find(name).is_some_and(|t| {
            let a = t.annotations();
            a.destructive && !a.read_only
        })
    }

    // As especificações sem as ferramentas destrutivas: o que o modelo vê antes de olhar o projeto.
    pub fn specs_without_destructive(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .filter(|t| {
                let a = t.annotations();
                a.read_only || !a.destructive
            })
            .map(|t| t.spec())
            .collect()
    }

    pub fn idempotent(&self, name: &str) -> bool {
        self.find(name).is_some_and(|t| t.annotations().idempotent)
    }

    // O modo Plan: só as ferramentas que declaram `read_only`. Nada que altera chega ao modelo.
    pub fn read_only(self) -> Registry {
        Self::new(self.tools.into_iter().filter(|t| t.annotations().read_only).collect())
    }

    pub fn execute(&self, ctx: &ToolContext, name: &str, args: &Value) -> Outcome {
        match self.find(name) {
            Some(tool) => tool.run(ctx, args),
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

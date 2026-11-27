//! O conjunto de ferramentas de um agente: o único lugar que o loop consulta.

use super::{Outcome, Tool, ToolContext, ToolSpec};
use serde_json::Value;

// Conjunto de ferramentas disponíveis para um agente. Aceita ferramentas de qualquer origem:
// as do toolkit, as de um fork, as que a sessão soma (`load_skill`).
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
    // Modo Plan: só as ferramentas que declaram `read_only` aparecem e executam. Liga e desliga em
    // tempo de execução (`/plan`), sem perder as outras ferramentas.
    plan: bool,
}

impl Registry {
    // Nome repetido é erro de programação: derruba aqui, não no meio de um turno.
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total, "nomes de ferramenta duplicados");
        Self { tools, plan: false }
    }

    // Acrescenta uma ferramenta (ex.: `load_skill`). Nome repetido é erro.
    pub fn with_tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        let plan = self.plan;
        Self { plan, ..Self::new(self.tools) }
    }

    // Junta dois registros (ex.: as ferramentas do toolkit com as de um fork).
    pub fn merge(mut self, other: Registry) -> Self {
        self.tools.extend(other.tools);
        let plan = self.plan || other.plan;
        Self { plan, ..Self::new(self.tools) }
    }

    // As ferramentas que o modelo pode usar agora: todas, ou só as de leitura no modo Plan.
    fn available(&self) -> impl Iterator<Item = &Box<dyn Tool>> {
        self.tools.iter().filter(|t| !self.plan || t.annotations().read_only)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.available().map(|t| t.spec()).collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.available().map(|t| t.name()).collect()
    }

    pub fn set_plan(&mut self, on: bool) {
        self.plan = on;
    }

    pub fn plan(&self) -> bool {
        self.plan
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
        self.available()
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
    pub fn read_only(mut self) -> Registry {
        self.plan = true;
        self
    }

    pub fn execute(&self, ctx: &ToolContext, name: &str, args: &Value) -> Outcome {
        match self.find(name) {
            Some(tool) if self.plan && !tool.annotations().read_only => Outcome::Error(format!(
                "modo Plan: {name} altera o projeto e está desligada. Use só ferramentas de leitura"
            )),
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

//! O que um turno produz e como ele avisa o mundo: `Progress`, `Silent`, `Action` e `TurnReport`.

use super::model::Chunk;
use crate::core::context::Trim;
use crate::core::tool::Outcome;
use serde_json::Value;

// Recebe sinais de progresso do loop, para a interface mostrar que o modelo está trabalhando.
pub trait Progress {
    fn model_waiting(&self, waiting: bool);

    // Uma ferramenta foi chamada, com os argumentos enviados pelo modelo.
    fn tool_call(&self, _name: &str, _args: &Value) {}

    // A ferramenta terminou (ou foi recusada): a interface mostra o resultado na hora.
    fn tool_done(&self, _name: &str, _args: &Value, _outcome: &Outcome) {}

    // O usuário pediu para interromper. O loop confere entre uma chamada ao modelo e outra:
    // uma requisição em andamento não é abortada, mas o turno para assim que ela volta.
    fn cancelled(&self) -> bool {
        false
    }

    // Uma chamada ao modelo terminou: tokens de entrada e de saída, quando o provedor informou.
    fn model_tokens(&self, _prompt: Option<u64>, _output: Option<u64>) {}

    // O modelo raciocinou antes de responder: o texto do raciocínio, quando ele o enviou.
    fn model_reasoning(&self, _text: &str) {}

    // Texto que o modelo acabou de gerar, ainda no meio da chamada.
    fn model_chunk(&self, _chunk: Chunk) {}
}

// Progresso que não mostra nada (útil em testes e em agentes sem terminal).
pub struct Silent;

impl Progress for Silent {
    fn model_waiting(&self, _waiting: bool) {}
}

// Uma chamada de ferramenta feita pelo modelo e o que aconteceu com ela.
pub struct Action {
    pub name: String,
    pub args: Value,
    pub outcome: Outcome,
    // Veio do registro: a ferramenta altera arquivos.
    pub mutates: bool,
}

// Registro do turno. O texto do modelo é só uma parte; as ações são o que o programa executou.
#[derive(Default)]
pub struct TurnReport {
    pub text: String,
    pub actions: Vec<Action>,
    pub error: Option<String>,
    // Descartes de histórico feitos durante o turno, para o terminal mostrar.
    pub trims: Vec<Trim>,
    // Avisos sobre o turno, preenchidos pelos `TurnHook` da sessão depois que ele termina.
    pub warnings: Vec<String>,
}

impl TurnReport {
    // Quanto cada ferramenta devolveu ao modelo neste turno: (nome, chamadas, caracteres), das que
    // mais pesaram para as que menos. É o que mostra onde o contexto está sendo gasto.
    pub fn tool_usage(&self) -> Vec<(String, usize, usize)> {
        let mut rows: Vec<(String, usize, usize)> = Vec::new();
        for a in &self.actions {
            let chars = a.outcome.text().chars().count();
            match rows.iter_mut().find(|r| r.0 == a.name) {
                Some(r) => {
                    r.1 += 1;
                    r.2 += chars;
                }
                None => rows.push((a.name.clone(), 1, chars)),
            }
        }
        rows.sort_by(|a, b| b.2.cmp(&a.2));
        rows
    }

    // Uma linha com `tool_usage`, ou vazio se nenhuma ferramenta rodou.
    pub fn tool_usage_line(&self) -> String {
        let rows = self.tool_usage();
        if rows.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = rows.iter().map(|(n, c, ch)| format!("{n} {c}× {ch} car.")).collect();
        format!("[ferramentas] {}", parts.join(", "))
    }

    // Houve alteração de fato no disco neste turno.
    pub fn changed(&self) -> bool {
        self.actions.iter().any(|a| a.mutates && matches!(a.outcome, Outcome::Done(_)))
    }

    // O modelo tentou alterar algo, com sucesso ou não.
    pub fn attempted_changes(&self) -> bool {
        self.actions.iter().any(|a| a.mutates)
    }
}

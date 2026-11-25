//! LegalClaw: um agente novo construído só com a API pública do core, sem editar o BitClaw.
//!
//! Traz a própria ferramenta (`buscar_jurisprudencia`), o próprio aprovador e o próprio modelo
//! (roteirizado, para rodar sem rede). Não usa o toolkit nem o Ollama.
//!
//! Rode com: `cargo run --example legalclaw`

use bitclaw::core::agent::{ChatModel, Reply, Silent};
use bitclaw::core::config::AgentConfig;
use bitclaw::core::session::Session;
use bitclaw::core::tool::{Approver, Outcome, Preview, Registry, Tool, ToolContext, ToolSpec};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;

const CONFIG: &str = r#"
name = "LegalClaw"
system_prompt = "Você é o LegalClaw, assistente de pesquisa jurídica. Responda em português."
tools = []

[model]
name = "roteiro"
"#;

// Uma ferramenta de domínio: não toca em arquivos, então ignora `ctx.root`.
struct BuscarJurisprudencia;

impl Tool for BuscarJurisprudencia {
    fn name(&self) -> &'static str {
        "buscar_jurisprudencia"
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "buscar_jurisprudencia",
            "Busca decisões sobre um tema.",
            json!({
                "type": "object",
                "properties": {"tema": {"type": "string", "description": "Assunto da busca"}},
                "required": ["tema"]
            }),
        )
    }

    fn run(&self, _ctx: &ToolContext, args: &Value) -> Outcome {
        match args.get("tema").and_then(Value::as_str) {
            Some(tema) => Outcome::Done(format!("REsp 0000/XX: decisão de exemplo sobre {tema}")),
            None => Outcome::Error("argumento 'tema' ausente".into()),
        }
    }
}

// Este agente não altera nada, então qualquer pedido de aprovação é recusado.
struct SomenteLeitura;

impl Approver for SomenteLeitura {
    fn approve(&self, _preview: &Preview) -> bool {
        false
    }
}

// Um modelo de mentira: devolve as respostas do roteiro, em ordem.
struct Roteiro(RefCell<VecDeque<Value>>);

impl ChatModel for Roteiro {
    fn complete(&self, _messages: &[Value], tools: &[ToolSpec]) -> Result<Reply, String> {
        assert_eq!(tools[0].name, "buscar_jurisprudencia", "o modelo recebe a ferramenta do fork");
        let message = self.0.borrow_mut().pop_front().ok_or("roteiro esgotado")?;
        Ok(Reply { message, prompt_tokens: None, output_tokens: None })
    }
}

fn main() -> Result<(), String> {
    let cfg = AgentConfig::from_toml(CONFIG)?;
    let modelo = Roteiro(RefCell::new(
        vec![
            json!({"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "buscar_jurisprudencia", "arguments": {"tema": "prescrição intercorrente"}}}
            ]}),
            json!({"role": "assistant", "content": "Encontrei uma decisão de exemplo sobre prescrição intercorrente."}),
        ]
        .into(),
    ));

    let mut sessao = Session::builder(&cfg)
        .model(Box::new(modelo))
        .approver(Box::new(SomenteLeitura))
        .tools(Registry::new(vec![Box::new(BuscarJurisprudencia)]))
        .root(".")
        .build()?;

    let relatorio = sessao.submit("pesquise prescrição intercorrente", &Silent)?;
    for acao in &relatorio.actions {
        println!("ferramenta {}: {}", acao.name, acao.outcome.text());
    }
    println!("{}", relatorio.text);
    Ok(())
}

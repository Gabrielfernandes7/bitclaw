---
description: Monte um agente novo sobre o core do BitClaw com Session, ChatModel, Tool e Approver.
---
# Usar como biblioteca

O crate `bitclaw` expõe o **core**. Um agente novo monta uma `Session` com um modelo, um aprovador e um registro de ferramentas, **sem editar o core**. Essa é a régua do projeto: veja a [Filosofia](../inicio/filosofia.md).

## A montagem

```rust
use bitclaw::{agent::Silent, config::AgentConfig, session::Session, toolkit, tools::AutoApprover};

let cfg = AgentConfig::from_toml(include_str!("agente.toml"))?;
let tools = toolkit::registry_from_names(&cfg.tools)?
    .with_tool(Box::new(MinhaFerramenta));
let mut sessao = Session::builder(&cfg)
    .model(modelo)
    .approver(Box::new(AutoApprover))
    .tools(tools)
    .root(".")
    .build()?;
let relatorio = sessao.submit("liste os arquivos", &Silent)?;
println!("{}", relatorio.text);
```

## As peças que você traz

| Peça | Contrato | Para quê |
|---|---|---|
| **Modelo** | `bitclaw::agent::ChatModel` | Qualquer provedor. O adaptador do Ollama está em `bitclaw::ollama`, mas o core não depende dele. |
| **Ferramenta** | `bitclaw::tools::Tool` | `name`, `spec` e `run`; entra no `Registry`. `spec` descreve a ferramenta sem formato de provedor, e cada adaptador a traduz para a sua API. |
| **Aprovador** | `bitclaw::tools::Approver` | Decide sobre cada `Preview` de alteração. `AutoApprover` aprova tudo; você pode recusar tudo, perguntar numa interface própria… |
| **Observador** | `bitclaw::agent::Progress` | Recebe os passos de ferramenta e a espera do modelo. `Silent` ignora. |
| **Configuração** | `AgentConfig` (TOML) | Prompt, ferramentas, modelo e limites como dado. |

As ferramentas de arquivo e `run_script` vêm do `bitclaw::toolkit` e são **opcionais**: um agente de domínio pode não usar nenhuma.

## Um agente completo em um arquivo

[`examples/legalclaw.rs`](../../../examples/legalclaw.rs) é um agente novo construído só com a API pública: traz a própria ferramenta (`buscar_jurisprudencia`), o próprio aprovador e o próprio modelo (roteirizado, para rodar sem rede). Não usa o toolkit nem o Ollama.

```sh
cargo run --example legalclaw
```

O núcleo da ferramenta é pequeno:

```rust
impl Tool for BuscarJurisprudencia {
    fn name(&self) -> &'static str { "buscar_jurisprudencia" }

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
            Some(tema) => Outcome::Done(format!("decisão de exemplo sobre {tema}")),
            None => Outcome::Error("argumento 'tema' ausente".into()),
        }
    }
}
```

## Declare as anotações

Uma ferramenta nova **declara** o que ela é, via `Tool::annotations()`:

| Anotação | Significado |
|---|---|
| `read_only` | Não altera nada. Só ferramentas assim aparecem no modo Plan. |
| `destructive` | Pode perder dados. |
| `idempotent` | Repetir a chamada não muda o resultado. |

Sem declaração, a ferramenta vale como **alteradora e destrutiva**. Se a natureza depende dos argumentos, implemente `Tool::call_annotations(args)` e `per_call_annotations()`. O agente consulta `Registry::mutates_call` e `idempotent_call`, não os métodos por nome.

## Prévias tipadas

`Preview::new` é uma alteração; `Preview::removal` é uma remoção e **nunca entra em aprovação em lote**. O tipo não vem do título, então um aprovador com interface própria pode tratar cada caso como precisa.

## Teste sem rede

Cada funcionalidade do projeto tem teste que não depende de rede nem de modelo real: um `ChatModel` falso com respostas roteirizadas basta. O exemplo `legalclaw` mostra o padrão. Veja [Testes e releases](testes-e-releases.md).

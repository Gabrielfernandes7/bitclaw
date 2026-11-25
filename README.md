# BitClaw

<p align="center"><img src="assets/bitclaw-crab.svg" alt="Mascote do BitClaw" width="160"></p>

Núcleo experimental de agente de IA, local e enxuto, escrito em Rust. Hoje ele funciona como um agente de linha de comando que lê e edita arquivos de um projeto, usando um modelo servido pelo [Ollama](https://ollama.com).

O objetivo do projeto é um **core** reutilizável para construir agentes, não um produto. O plano e as decisões de arquitetura estão em [`docs/PROJETO.md`](docs/PROJETO.md).

## Pré-requisitos

- Rust e Cargo.
- Ollama rodando em `http://localhost:11434`.
- Um modelo com suporte a ferramentas (*tools*). O padrão é `nemotron-3-nano:4b-mlx`:

```sh
ollama pull nemotron-3-nano:4b-mlx
```

## Como rodar

```sh
cargo run -- --dir /caminho/do/projeto
```

Opções:

| Opção | Padrão | Função |
|---|---|---|
| `--dir D` | diretório atual | Projeto que o agente pode ler e alterar. |
| `--model M` | `nemotron-3-nano:4b-mlx` | Modelo do Ollama. |
| `--url U` | `http://localhost:11434` | Endereço do Ollama. |
| `--yes` | desligado | Aplica edições, criações e remoções sem pedir confirmação. Use com cuidado. |
| `--no-color` | desligado | Desliga as cores. Também é desligado com `NO_COLOR` e quando a saída não é um terminal. |
| `--think` | desligado | Liga o raciocínio (thinking) do modelo. Gasta mais tokens e enche a janela de contexto mais rápido; nem todo modelo suporta. Também dá para ligar com `think = true` na configuração ou, durante a sessão, com `/think`. |
| `-h`, `--help` | | Mostra a ajuda. |

Ao iniciar, o programa mostra o modelo em uso e a janela de contexto. Se o modelo não estiver instalado no Ollama, ele avisa antes de começar.

## Configuração do agente

O comportamento do agente vem de um arquivo TOML. O padrão é [`agents/bitclaw.toml`](agents/bitclaw.toml), embutido no binário; para usar outro:

```sh
cargo run -- --config meu-agente.toml
```

```toml
name = "BitClaw"
system_prompt = "Você é ..."
tools = ["list_dir", "read_file", "edit_file", "create_file", "delete_file"]

[model]
name = "nemotron-3-nano:4b-mlx"
url = "http://localhost:11434"
num_ctx = 16384       # opcional
temperature = 0.1     # opcional

[limits]              # opcional
max_steps = 8
max_consecutive_failures = 2
reserve_output_tokens = 1024
```

Campos desconhecidos, ferramentas inexistentes ou repetidas e valores incoerentes (ex.: `num_ctx` menor que `reserve_output_tokens`) são erros com mensagem clara. `--model` e `--url` sobrescrevem o arquivo.

## Usar como biblioteca

O crate `bitclaw` expõe o core. Um agente novo monta uma `Session` a partir de uma configuração e de um modelo, sem editar o core:

```rust
use bitclaw::{agent::Silent, config::AgentConfig, session::Session, tools::AutoApprover};

let cfg = AgentConfig::from_toml(include_str!("agente.toml"))?;
let mut sessao = Session::from_config(&cfg, modelo, Box::new(AutoApprover), Path::new("."))?;
let relatorio = sessao.submit("liste os arquivos", &Silent)?;
```

`modelo` é qualquer implementação de `bitclaw::agent::ChatModel`. O adaptador do Ollama está em `bitclaw::ollama`, mas o core não depende dele. Ferramentas novas são implementações de `bitclaw::tools::Tool` registradas em `Registry`.

## Uso no prompt

Digite a tarefa em linguagem natural e pressione Enter. A conversa fica no histórico normal do terminal (você rola, seleciona e copia como sempre); só o status, o editor e o rodapé ficam fixos embaixo.

| Tecla | Efeito |
|---|---|
| Enter | Envia a tarefa. |
| Shift+Enter, Alt+Enter ou Ctrl+J | Nova linha no editor, que cresce até 6 linhas. Colar texto com várias linhas funciona. |
| Esc ou Ctrl+C durante um turno | Interrompe. Uma requisição ao modelo em andamento não é abortada: o turno para assim que ela volta. |
| Ctrl+C no editor | Limpa o editor; com o editor vazio, encerra. Ctrl+D com o editor vazio também encerra. |
| `s` / `n` num pedido de aprovação | Aplica ou recusa a alteração. Enter e Esc recusam. |

Digite `/` para abrir o menu de comandos. Setas escolhem, Tab completa, Enter executa e Esc fecha.

- `/clear`: limpa a tela e apaga o histórico da conversa, como no Claude Code. O prompt do sistema é mantido.
- `/think`: liga ou desliga o raciocínio do modelo e mostra o aviso de custo. O rodapé mostra `thinking` enquanto estiver ligado. O raciocínio de turnos anteriores sai do histórico, para não gastar contexto.
- `/help`: mostra os comandos e as teclas.
- `/exit`: encerra o programa.

Os nomes antigos em português (`/limpar-contexto`, `/ajuda`, `/sair`) e `/quit` continuam funcionando.

O rodapé mostra o modelo, o modo (`Manual`, ou `Automático` com `--yes`) e o uso da janela de contexto. Quando a entrada ou a saída não é um terminal (pipes, scripts), o BitClaw usa um modo simples: uma tarefa por linha, texto puro e sem cores.

Exemplos de tarefas:

```
liste os arquivos do projeto
leia notas.md e resuma em três frases
crie um arquivo fib.py com uma função fibonacci
troque o nome da função soma por adiciona em calc.py
```

## Ferramentas

- `list_dir`: lista um diretório do projeto.
- `read_file`: lê um arquivo. Arquivos com mais de 8 KB são truncados e o programa avisa.
- `edit_file`: troca um trecho exato que aparece uma única vez. Mostra o diff com contexto, pede confirmação e salva um backup em `.bitclaw/backup/`. Só altera arquivos existentes.
- `create_file`: cria um arquivo novo com o conteúdo dado. Recusa sobrescrever um arquivo existente e pede confirmação.
- `delete_file`: remove um arquivo, pede confirmação e salva um backup em `.bitclaw/backup/`. Não remove diretórios.

Ao fim de cada tarefa, o programa lista as ações executadas: `✓` (feita), `✗` (erro) ou `⊘` (recusada pelo usuário). As ações aparecem antes do texto do modelo. Se nenhuma alteração foi aplicada, ele avisa, mesmo que o modelo tenha dito o contrário. Se o texto final afirma que algo foi removido sem que nenhum arquivo tenha sido removido, o aviso também aparece.

## Contexto

O histórico da conversa tem orçamento. Quando ele excede a janela (`num_ctx` = 16384 tokens no padrão, com 1024 reservados para a resposta), o programa descarta os turnos mais antigos inteiros e registra cada descarte no terminal. O prompt do sistema e o turno atual nunca são descartados. Se o próprio turno atual não couber, a tarefa é recusada com erro.

## Avaliação com modelos reais

```sh
scripts/eval.sh                          # padrão: qwen2.5-coder:3b e nemotron-3-nano:4b-mlx
scripts/eval.sh nemotron-3-nano:4b-mlx   # um ou mais modelos
REPEATS=5 scripts/eval.sh ...            # repetições (padrão: 3)
```

O script roda cinco tarefas fixas (criar `.txt`, criar `.py`, editar, ler e responder, não alterar nada). Cada tarefa roda num projeto novo, e o resultado é verificado no disco, não pelo texto do modelo.

## Testes

```sh
cargo test
```

Os testes não dependem de rede nem de modelo: usam um modelo falso com respostas roteirizadas.

## Segurança

- Caminhos absolutos, `..` e symlinks que saiam do diretório do projeto são recusados (`src/sandbox.rs`, com testes).
- Edições, criações e remoções pedem confirmação, a menos que `--yes` seja usado.
- Não há acesso a shell nem à rede, além do Ollama local.

## Limites conhecidos

- Modelos pequenos às vezes escrevem a chamada de ferramenta como texto JSON. Há um fallback que reconhece esse formato (`calls_from_text` em `src/agent.rs`).
- O modelo pode afirmar que concluiu uma tarefa sem ter aplicado todas as edições. Confira o arquivo. O programa avisa nos casos mais óbvios, mas não é garantia.
- Ferramentas ainda são só as embutidas: não há carregamento de ferramentas externas nem de skills.
- A biblioteca ainda não tem um segundo agente que a use de fora do repositório. Essa é a prova da Fase 6 de `docs/PROJETO.md`.

## Estrutura

| Arquivo | Papel |
|---|---|
| `src/lib.rs` | Ponto de entrada da biblioteca (o core). |
| `src/session.rs` | `Session`: API pública para usar o agente (histórico, turnos, contexto). |
| `src/config.rs` | Leitura e validação da configuração em TOML. |
| `src/agent.rs` | Ciclo modelo → ferramentas → modelo, com limites de passos e de repetição. |
| `src/tools.rs` | Trait `Tool`, registro de ferramentas e `Approver` (confirmação). |
| `src/context.rs` | Orçamento e descarte do histórico. |
| `src/ollama.rs` | Adaptador do Ollama (implementa `ChatModel`). |
| `src/sandbox.rs` | Validação de caminhos dentro do projeto. |
| `src/main.rs` | Binário: lê a configuração, monta as peças e escolhe a interface. |
| `src/tui.rs` | Interface de terminal (ratatui, viewport inline). O agente roda numa thread e fala por canais. |
| `src/markdown.rs` | Renderização leve de Markdown para as respostas. |
| `src/brand.rs` | Mascote e cabeçalho. |
| `src/plain.rs` | Modo simples, sem terminal interativo. |
| `agents/bitclaw.toml` | Configuração do agente padrão. |
| `scripts/eval.sh` | Avaliação com modelos reais. |
| `docs/PROJETO.md` | Propósito, decisões e plano. |

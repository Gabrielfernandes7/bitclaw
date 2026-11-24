# BitClaw

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
| `--yes` | desligado | Aplica edições e criações sem pedir confirmação. Use com cuidado. |
| `-h`, `--help` | | Mostra a ajuda. |

Ao iniciar, o programa mostra o modelo em uso e a janela de contexto. Se o modelo não estiver instalado no Ollama, ele avisa antes de começar.

## Uso no prompt

Digite a tarefa em linguagem natural e pressione Enter. Comandos:

- `/limpar-contexto`: apaga o histórico da conversa. O prompt do sistema é mantido.
- `/sair`: encerra o programa.

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

Ao fim de cada tarefa, o programa lista as ações executadas com `[ok]`, `[erro]` ou `[recusada]`. Se nenhuma alteração foi aplicada, ele avisa, mesmo que o modelo tenha dito o contrário. Se o texto final afirma que algo foi removido sem que nenhum arquivo tenha sido removido, o aviso também aparece.

## Contexto

O histórico da conversa tem orçamento. Quando ele excede a janela (`num_ctx` = 8192 tokens, com 1024 reservados para a resposta), o programa descarta os turnos mais antigos inteiros e registra cada descarte no terminal. O prompt do sistema e o turno atual nunca são descartados. Se o próprio turno atual não couber, a tarefa é recusada com erro.

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
- A configuração do agente (prompt, ferramentas, modelo) ainda está no próprio binário. A separação em biblioteca e configuração é a Fase 4 de `docs/PROJETO.md`.

## Estrutura

| Arquivo | Papel |
|---|---|
| `src/agent.rs` | Ciclo modelo → ferramentas → modelo, com limites de passos e de repetição. |
| `src/tools.rs` | Trait `Tool`, registro de ferramentas e `Approver` (confirmação). |
| `src/context.rs` | Orçamento e descarte do histórico. |
| `src/ollama.rs` | Adaptador do Ollama (implementa `ChatModel`). |
| `src/sandbox.rs` | Validação de caminhos dentro do projeto. |
| `src/main.rs` | Interface de terminal e montagem das peças. |
| `scripts/eval.sh` | Avaliação com modelos reais. |
| `docs/PROJETO.md` | Propósito, decisões e plano. |

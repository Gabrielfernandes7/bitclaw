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
context_window = 16384   # opcional

[ollama]                 # opcional: dados do provedor
url = "http://localhost:11434"
temperature = 0.1

[limits]              # opcional
max_steps = 8
max_consecutive_failures = 2
reserve_output_tokens = 1024
```

Campos desconhecidos, ferramentas inexistentes ou repetidas e valores incoerentes (ex.: `context_window` menor que `reserve_output_tokens`) são erros com mensagem clara. `--model` e `--url` sobrescrevem o arquivo.

O core só conhece `[model]` e `[limits]`; `url` e `temperature` pertencem à seção do provedor (`[ollama]`). O formato antigo (`url`, `temperature` e `num_ctx` dentro de `[model]`) ainda é lido; `url` e `temperature` geram um aviso.

### Usar o OpenRouter

O padrão é o Ollama local. Para usar modelos hospedados no [OpenRouter](https://openrouter.ai), escolha o provedor na configuração e exporte a chave no ambiente:

```toml
[model]
provider = "openrouter"
name = "anthropic/claude-sonnet-4"
context_window = 200000

[openrouter]
base_url = "https://openrouter.ai/api/v1"   # opcional
api_key_env = "OPENROUTER_API_KEY"          # o NOME da variável, nunca a chave
temperature = 0.1
```

```sh
export OPENROUTER_API_KEY=...
cargo run -- --config meu-agente.toml
```

- A chave **nunca** vai no TOML (ele é versionado e lido pelo `/init`). Se `api_key_env` parecer uma chave, a configuração é recusada sem repetir o valor. Se a variável faltar, o erro diz qual é.
- `/model` lista os modelos do OpenRouter que chamam ferramentas. `/effort` vale para os que aceitam raciocínio com níveis.
- `/context` só muda o orçamento do histórico: no OpenRouter a janela real é a do modelo.
- Erros 401 (chave), 402 (créditos) e 429 (limite) viram mensagens legíveis. Nenhuma mensagem, `/status` ou log mostra a chave.
- **Privacidade:** com um provedor remoto, o conteúdo dos arquivos que o agente ler e o histórico da conversa saem da sua máquina. O BitClaw avisa na abertura.

## Usar como biblioteca

O crate `bitclaw` expõe o core. Um agente novo monta uma `Session` com um modelo, um aprovador e um registro de ferramentas, sem editar o core:

```rust
use bitclaw::{agent::Silent, config::AgentConfig, session::Session, toolkit, tools::AutoApprover};

let cfg = AgentConfig::from_toml(include_str!("agente.toml"))?;
let tools = toolkit::registry_from_names(&cfg.tools)?.with_tool(Box::new(MinhaFerramenta));
let mut sessao = Session::builder(&cfg)
    .model(modelo)
    .approver(Box::new(AutoApprover))
    .tools(tools)
    .root(".")
    .build()?;
let relatorio = sessao.submit("liste os arquivos", &Silent)?;
```

`modelo` é qualquer implementação de `bitclaw::agent::ChatModel`. O adaptador do Ollama está em `bitclaw::ollama`, mas o core não depende dele. Uma ferramenta nova implementa `bitclaw::tools::Tool` (`name`, `spec` e `run`) e entra no `Registry`; `spec` descreve a ferramenta sem formato de provedor, e cada adaptador a traduz para a sua API. As ferramentas de arquivo e `run_script` vêm do `bitclaw::toolkit` e são opcionais. Um agente completo, com ferramenta, aprovador e modelo próprios, está em `examples/legalclaw.rs` (`cargo run --example legalclaw`).

## Uso no prompt

Digite a tarefa em linguagem natural e pressione Enter. A conversa fica no histórico normal do terminal (você rola, seleciona e copia como sempre); só o status, o editor e o rodapé ficam fixos embaixo.

| Tecla | Efeito |
|---|---|
| Enter | Envia a tarefa. |
| Shift+Enter, Alt+Enter ou Ctrl+J | Nova linha no editor, que cresce até 6 linhas. Colar texto com várias linhas funciona. |
| Esc ou Ctrl+C durante um turno | Interrompe. Uma requisição ao modelo em andamento não é abortada: o turno para assim que ela volta. |
| Ctrl+C no editor | Limpa o editor; com o editor vazio, encerra. Ctrl+D com o editor vazio também encerra. |
| ↑ / ↓ no editor | Recupera tarefas anteriores, como no shell. Depois da última, volta ao texto que estava digitado. Com o menu `/` aberto, escolhem o comando. |
| `s` / `n` num pedido de aprovação | Aplica ou recusa a alteração. Enter e Esc recusam. |

Digite `/` para abrir o menu de comandos. Setas escolhem, Tab completa, Enter executa e Esc fecha.

- `/clear`: limpa a tela e apaga o histórico da conversa, como no Claude Code. O prompt do sistema é mantido.
- Comandos e o subcomando de `/model` (`load`, `show`) aparecem destacados em azul no editor.
- A tela é dividida em três partes: o mascote com o nome, o projeto e o modelo no topo, a conversa no meio e o editor embaixo. A conversa rola com PageUp/PageDown. Como é tela cheia, a conversa some ao sair e o terminal não guarda o histórico. Por padrão o mouse não é capturado, então dá para selecionar e copiar texto normalmente; com `--mouse`, a roda rola a conversa, mas a seleção só funciona segurando Option (macOS).
- O mascote fica no topo, ao lado do nome, do projeto e do modelo (que viram o status enquanto o agente trabalha), e cumprimenta ao abrir (pisca, olha para os lados e estala as pinças). Cada clique nele mostra um número: estalo, pinos contando em binário, pinças batucando e o cumprimento de novo. Com `--mouse`, o clique usa o mouse do terminal, o que impede selecionar texto sem uma tecla modificadora (Option no macOS, em muitos terminais).
- Enquanto o agente trabalha, o status mostra uma lâmpada que pulsa e o passo atual (`lendo AGENTS.md…`). Cada ferramenta aparece na conversa quando termina (`✓ leu AGENTS.md`, `⊘` se recusada, `✗` se falhou). Isso vem das próprias chamadas de ferramenta, então não gasta token. O tempo vem em `(59s · modelo 52s)`: total do turno e quanto foi espera pelo modelo.
- `@` no editor lista os arquivos e pastas do projeto (`@docs/` mostra o conteúdo de `docs`). ↑/↓ escolhem, Tab ou Enter completam (pasta termina em `/` e continua descendo) e Esc fecha. Pastas ocultas, `target` e `node_modules` ficam de fora, e nada sai da pasta do projeto. O texto `@caminho` vai para o modelo como escrito: o conteúdo do arquivo não é anexado, o modelo o lê com `read_file` se precisar.
- `/model`: abre a lista dos modelos instalados no Ollama, com busca (digite para filtrar, ↑/↓ e enter para escolher). Mostra o ativo, o padrão da configuração e as capacidades de cada um (ferramentas, raciocínio, janela). `/model load <id>` troca direto (`/model <id>` continua valendo). `/model show [id]` mostra a ficha do modelo pedido, ou do ativo, e funciona até durante uma tarefa. A troca mantém a conversa; a próxima tarefa vai para o novo modelo. Não é possível trocar durante uma tarefa. A escolha vale só para a sessão: o padrão continua o da configuração.
- `/effort`: mostra o esforço de raciocínio atual e os níveis que o modelo aceita. `/effort <nível>` aplica, e só aceita níveis que o modelo tem (`off`, `on`, `low`, `medium` ou `high`). Modelos sem raciocínio só aceitam `off`. Ao trocar de modelo, um esforço que não existe no novo modelo cai para o nível mais próximo.
- `/init`: cria um `AGENTS.md` com o que o próprio repositório diz sobre si: a descrição do README, a stack, os comandos, o que cada arquivo de `src/` faz (a primeira frase do comentário `//!` de abertura), a documentação, os scripts, as skills e os agentes. Não usa modelo: sai na hora, sem gastar tokens e sem inventar. Mostra a prévia e só grava depois da sua aprovação. Se o `AGENTS.md` já existe, ele nunca é sobrescrito: o `/init` só acrescenta ao fim as seções que faltam, e o que você escreveu fica como está. Para o `/init` descrever bem um módulo, comece o arquivo com `//!` e uma primeira frase que diga o que ele faz. As notas que só você sabe ficam para você preencher.
- `/context [tokens|reset]`: mostra ou troca a janela de contexto (`16384`, `16k`). Sem argumento, explica o que muda ao aumentá-la: memória, recarga do modelo, velocidade, tokens gastos e qualidade. `reset` volta ao valor da configuração (`model.context_window`).
- `/temp [0 a 2|reset]`: mostra ou troca a temperatura de geração e explica o efeito. `reset` volta ao valor da configuração (`ollama.temperature`).
- `/status`: painel em abas (Status, Config, Usage e Stats), com versão, modelo, esforço, contexto e contagens da sessão. ←/→ troca de aba, esc fecha.
- `/think`: atalho de `/effort`. Liga no padrão do modelo ou desliga, e mostra o aviso de custo. O rodapé mostra `esforço <nível>` enquanto estiver ligado. O raciocínio de turnos anteriores sai do histórico, para não gastar contexto.
- `/help`: mostra os comandos e as teclas.
- `/exit`: encerra o programa.

`/quit` também encerra o programa, como alias de `/exit`.

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
- `create_dir`: cria uma pasta nova e vazia, com as pastas pais que faltarem. Recusa pasta existente e pede confirmação.
- `delete_file`: remove um arquivo, pede confirmação e salva um backup em `.bitclaw/backup/`. Não remove diretórios.
- `run_script`: executa um script que já existe no projeto. Aplica `chmod u+x`, roda com a pasta do projeto como diretório atual e devolve o código de saída e a saída real (stdout e stderr). O fluxo é `create_file` e depois `run_script`: por exemplo, pedir "as especificações do hardware" faz o agente criar um `info.sh`, mostrar o script inteiro para você aprovar e só então executá-lo. Veja os limites em [Segurança](#segurança).

Ao fim de cada tarefa, o programa lista as ações executadas: `✓` (feita), `✗` (erro) ou `⊘` (recusada pelo usuário). As ações aparecem antes do texto do modelo. Se nenhuma alteração foi aplicada, ele avisa, mesmo que o modelo tenha dito o contrário. Se o texto final afirma que algo foi removido sem que nenhum arquivo tenha sido removido, o aviso também aparece.

## Contexto

O histórico da conversa tem orçamento. Quando ele excede a janela (`context_window` = 16384 tokens no padrão, com 1024 reservados para a resposta), o programa descarta os turnos mais antigos inteiros e registra cada descarte no terminal. O prompt do sistema e o turno atual nunca são descartados. Se o próprio turno atual não couber, a tarefa é recusada com erro.

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
- `run_script` **sempre** pede confirmação, mesmo com `--yes`: o `--yes` vale só para edições. A confirmação mostra o script inteiro, com caracteres de controle escritos como `\xNN` (para não esconderem texto no terminal) e linhas longas quebradas. Scripts com mais de 120 linhas ou 32 KB são recusados, porque o usuário precisa conseguir revisá-los. Se o arquivo muda entre a confirmação e a execução, nada é executado.
- O script roda com **as suas permissões e sem isolamento**: ele pode ler e apagar o que você pode, e usar a rede. Só a confirmação protege. Para um agente que não executa nada, tire `run_script` da lista `tools` da configuração.
- O processo tem limite de 60 s (o grupo de processos inteiro é encerrado), saída limitada a 6 KB para o modelo e ambiente reduzido: só `PATH`, `HOME`, `USER`, `LANG`, `LC_ALL` e `TMPDIR` passam, então chaves de API e tokens do seu ambiente não chegam ao script.
- Fora o que um script aprovado fizer, não há acesso à rede além do provedor escolhido: o Ollama local, por padrão. Com o OpenRouter, o conteúdo lido dos arquivos é enviado a um serviço remoto; a chave vem de uma variável de ambiente e não chega aos scripts (o ambiente deles é reduzido).

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
| `src/script.rs` | `run_script`: `chmod u+x` e execução de scripts, com prazo, saída limitada e ambiente reduzido. |
| `src/context.rs` | Orçamento e descarte do histórico. |
| `src/ollama.rs` | Adaptador do Ollama (implementa `ChatModel`). |
| `src/sandbox.rs` | Validação de caminhos dentro do projeto. |
| `src/main.rs` | Binário: lê a configuração, monta as peças e escolhe a interface. |
| `src/tui/mod.rs` | Interface de terminal (ratatui, viewport inline): estado, laço de eventos e `run`. |
| `src/tui/input.rs` | Teclas, painéis e comandos com `/`. |
| `src/tui/print.rs` | Saída para o scrollback: cabeçalho, avisos, prévias e relatório. |
| `src/tui/worker.rs` | Thread do agente e canais entre ela e a interface. |
| `src/tui/commands.rs`, `history.rs` | Lista de comandos com `/` e histórico de entrada (↑ ↓). |
| `src/tui/view/` | Desenho da área fixa e dos painéis `/skills`, `/model` e `/status`. |
| `src/tui/theme.rs` | Cores e estilos. |
| `src/tui/brand.rs` | Mascote e cabeçalho. |
| `src/tui/markdown.rs` | Renderização leve de Markdown para as respostas. |
| `src/tui/plain.rs` | Modo simples, sem terminal interativo. |
| `agents/bitclaw.toml` | Configuração do agente padrão. |
| `scripts/eval.sh` | Avaliação com modelos reais. |
| `docs/PROJETO.md` | Propósito, decisões e plano. |
| `docs/MODELOS.md` | Modelos em uso e candidatos a testar, com diagnósticos. |

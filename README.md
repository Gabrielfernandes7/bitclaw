# BitClaw

<p align="center"><img src="assets/bitclaw-bricks.svg" alt="O mascote do BitClaw encaixando peças coloridas" width="560"></p>

Núcleo experimental de agente de IA, local e enxuto, escrito em Rust. Hoje ele funciona como um agente de linha de comando que lê e edita arquivos de um projeto, usando um modelo servido pelo [Ollama](https://ollama.com).

O objetivo do projeto é um **core** reutilizável para construir agentes, não um produto. O plano e as decisões de arquitetura estão em [`docs/PROJETO.md`](docs/PROJETO.md). A documentação completa, com a filosofia do projeto, os guias e a identidade visual, está no site: <https://gabrielfernandes7.github.io/bitclaw/> (fonte em [`site/`](site/)).

## Instalação

Binário pronto para macOS (Apple Silicon e Intel) e Linux (x86_64 e arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh
```

O script baixa o binário do último release, confere o SHA-256 e o instala em `~/.local/bin`, sem `sudo`. Ele avisa se essa pasta não estiver no `PATH` e se o Ollama não estiver instalado ou não responder. Outra pasta: `BITCLAW_INSTALL_DIR=/outra/pasta`. Outra versão: `BITCLAW_VERSION=v0.2.0`.

Com Rust instalado, também dá para compilar direto do repositório:

```sh
cargo install --git https://github.com/Gabrielfernandes7/bitclaw
```

Para desinstalar, `bitclaw uninstall` apaga o binário e todo o resíduo: `~/.config/bitclaw` (SOUL.md, caches e marcas), restos de instalação e atualização (`.bitclaw.install`, `.bitclaw.update`, `bitclaw-update-*` no temp). Mostra o que vai apagar e pergunta (`--yes` não pergunta). As pastas `.bitclaw/` dos projetos (backup, recibos, `/undo`, skills instaladas) ficam, a menos que você diga que não: `--purge` as apaga junto, e sem `--purge` o comando pergunta à parte. Elas são encontradas por um registro dos projetos onde o BitClaw rodou (`~/.config/bitclaw/projects`, só caminhos) e pela pasta atual. O `AGENTS.md` e o código nunca são tocados. Sem o binário à mão: `curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh -s -- --uninstall` (acrescente `--purge` depois de `--uninstall` para apagar as pastas dos projetos). Um binário do `cargo install` ou de desenvolvimento (`target/`) não é apagado, mas o resíduo é: para o primeiro, `cargo uninstall bitclaw`.

Para atualizar, `bitclaw update` troca o binário pelo do último release, depois de conferir a soma. Quando sai uma versão nova, o BitClaw avisa na abertura; a consulta ao GitHub é feita em segundo plano, no máximo uma vez por dia, e fica em `~/.config/bitclaw/`. `BITCLAW_NO_UPDATE_CHECK=1` desliga a consulta.

Na primeira abertura aparece uma tela de boas-vindas, que mostra o que foi conferido (provedor, modelo e projeto). `BITCLAW_WELCOME=1` a mostra de novo.

## Pré-requisitos

- Rust e Cargo, para compilar (o binário instalado não precisa).
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
| `--plan` | desligado | Modo Plan: só as ferramentas de leitura. Nada é editado, criado, removido nem executado. |
| `--openrouter` | desligado | Abre com o agente gerado por `bitclaw openrouter` (`~/.config/bitclaw/openrouter.toml`), no lugar de `--config`. |
| `--fresh` | desligado | Abre como na primeira vez (boas-vindas, perguntas do SOUL, sem aviso de versão em cache): o estado global é uma pasta temporária, apagada ao sair, e o seu `~/.config/bitclaw` não é lido nem tocado. Um `.bitclaw/SOUL.md` já existente no projeto ainda vale; use `--dir` numa pasta nova. Funciona também com `bitclaw --fresh soul`. |
| `-V`, `--version` | | Mostra a versão. |
| `update` | | `bitclaw update` troca o binário pelo do último release. |
| `openrouter` | | `bitclaw openrouter` configura o OpenRouter (chave, modelo, agente); `status` confere; `logout` desfaz. |
| `uninstall` | | `bitclaw uninstall` apaga o binário e o resíduo (`~/.config/bitclaw`, restos de instalação). `--purge` apaga também as pastas `.bitclaw/` dos projetos; `--yes` não pergunta. |
| `soul` | | `bitclaw soul` refaz as cinco perguntas do SOUL.md: nome, idioma, tamanho das respostas, se explica antes de alterar e o que você faz. A primeira abertura já pergunta; `pular` dispensa. O arquivo fica em `~/.config/bitclaw/SOUL.md`, um `.bitclaw/SOUL.md` no projeto vale mais, e `/soul` mostra o que está em vigor. Entra no prompt cortado em 1200 caracteres e vale na próxima abertura. |
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

O caminho curto é um comando só:

```sh
bitclaw openrouter          # chave, modelo e agente, em três passos
bitclaw --openrouter        # abre com o agente gerado
```

`bitclaw openrouter` lê a chave **sem mostrá-la** (um `*` por caractere), confere no OpenRouter (mostra o nome da chave e o crédito que resta), lista os modelos que chamam ferramentas (gratuitos primeiro; um número escolhe, um texto filtra, Enter fica com o sugerido) e grava `~/.config/bitclaw/openrouter.toml`. A chave pode ser guardada em `~/.config/bitclaw/openrouter.key` (permissão 600; o BitClaw recusa o arquivo se outras pessoas puderem lê-lo) ou fica só na variável `OPENROUTER_API_KEY`, que sempre vale mais que o arquivo. Nada é gravado antes do fim, e a chave nunca vai para o TOML. `bitclaw openrouter status` confere o que está em vigor e `bitclaw openrouter logout` apaga a chave guardada e o agente gerado. `bitclaw uninstall` também os apaga.

À mão, o mesmo se faz assim:

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

Digite a tarefa em linguagem natural e pressione Enter. O BitClaw abre em tela cheia, como o Claude Code: a conversa ocupa a parte de cima e rola, e o editor, o rodapé e os painéis ficam fixos embaixo. O que estava no terminal antes (o `cargo run`, o shell) some ao abrir e volta intacto ao sair.

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
- Rolar a conversa: roda do mouse ou trackpad, PageUp/PageDown (fn+↑/↓ no Mac), Shift+↑/↓ linha a linha, Ctrl+Home e Ctrl+End para o começo e o fim. Rolada para cima, o que chega não a move, e um aviso diz quantas linhas há abaixo. Enviar uma tarefa volta ao fim.
- Copiar: arraste na conversa para selecionar (o trecho fica em destaque) e solte para copiar; o rodapé confirma com "copiado". A cópia usa `pbcopy` (macOS), `wl-copy` ou `xclip` e, sem nenhum deles, a sequência OSC 52 do terminal. Ctrl+C com algo selecionado copia de novo em vez de interromper. Com `--no-mouse`, o mouse volta para o terminal (seleção nativa), e a conversa rola só pelo teclado.
- Redimensionar a janela reflui a conversa na nova largura.
- O cabeçalho (mascote, nome, projeto e modelo) é a primeira coisa da conversa e rola junto com ela. Ao abrir, o mascote faz um dos números (pisca, olha para os lados e estala as pinças) ali mesmo; quando acaba, ou na primeira tecla, ele para. Os painéis (`/status`, `/model`, `/skills`, `/effort`) abrem embaixo, no lugar do editor, com a conversa visível acima.
- Limitação do modo inline: se você redimensionar a janela, o terminal reflui as linhas já desenhadas, e uma cópia antiga e quebrada da área de baixo pode ficar no histórico. A conversa em si não é afetada.
- Enquanto o agente trabalha, o status mostra uma lâmpada que pulsa e o passo atual (`lendo AGENTS.md…`). Cada ferramenta aparece na conversa quando termina (`✓ leu AGENTS.md`, `⊘` se recusada, `✗` se falhou). Isso vem das próprias chamadas de ferramenta, então não gasta token. O tempo vem em `(59s · modelo 52s)`: total do turno e quanto foi espera pelo modelo.
- `@` no editor lista os arquivos e pastas do projeto (`@docs/` mostra o conteúdo de `docs`). ↑/↓ escolhem, Tab ou Enter completam (pasta termina em `/` e continua descendo) e Esc fecha. Pastas ocultas, `target` e `node_modules` ficam de fora, e nada sai da pasta do projeto. O texto `@caminho` vai para o modelo como escrito: o conteúdo do arquivo não é anexado, o modelo o lê com `read_file` se precisar.
- `/model`: abre a lista dos modelos instalados no Ollama, com busca (digite para filtrar, ↑/↓ e enter para escolher). Mostra o ativo, o padrão da configuração e as capacidades de cada um (ferramentas, raciocínio, janela). `/model load <id>` troca direto (`/model <id>` continua valendo). `/model show [id]` mostra a ficha do modelo pedido, ou do ativo, e funciona até durante uma tarefa. A troca mantém a conversa; a próxima tarefa vai para o novo modelo. Não é possível trocar durante uma tarefa. A escolha vale só para a sessão: o padrão continua o da configuração.
- `/effort`: mostra o esforço de raciocínio atual e os níveis que o modelo aceita. `/effort <nível>` aplica, e só aceita níveis que o modelo tem (`off`, `on`, `low`, `medium` ou `high`). Modelos sem raciocínio só aceitam `off`. Ao trocar de modelo, um esforço que não existe no novo modelo cai para o nível mais próximo.
- Cabeçalho: `Projeto (nome) /caminho` e, na linha do modelo, `modelo with high effort · Ollama · 9.0B · nvfp4 · janela 16384 tok`. A linha é refeita a cada quadro, então muda na hora com `/effort`, `/think`, `/model` e `/context`. Sem raciocínio aparece `thinking off`.
- `/skill install <dono/repo | https://…> [--path pasta] [--ref tag]`: baixa uma skill de um repositório git (`git clone --depth 1`), mostra a origem, o commit, os arquivos, os scripts e o corpo do `SKILL.md`, e só instala depois de `/skill yes` (`/skill no` descarta). A skill vai para `.bitclaw/skills/<nome>/`, separada do `skills/` do projeto, e a origem fica em `.bitclaw/skills.lock`. O formato é o mesmo de qualquer skill (pasta com `SKILL.md`). Se o repositório tem várias skills, diz quais e pede `--path` (ex.: `/skill install anthropics/skills --path skills/doc-coauthoring`). Recusa origem que não seja `https://` ou `dono/repo`, link simbólico, mais de 200 arquivos ou 2 MB, e nunca sobrescreve uma skill existente. A skill baixada aparece em `/skills` na próxima abertura. Uma skill é texto que o modelo vai seguir: leia a prévia.
- `/init`: cria um `AGENTS.md` com o que o próprio repositório diz sobre si: a descrição do README, a stack, os comandos, o que cada arquivo de `src/` faz (a primeira frase do comentário `//!` de abertura), a documentação, os scripts, as skills e os agentes. Não usa modelo: sai na hora, sem gastar tokens e sem inventar. Mostra a prévia e só grava depois da sua aprovação. Se o `AGENTS.md` já existe, ele nunca é sobrescrito: o `/init` só acrescenta ao fim as seções que faltam, e o que você escreveu fica como está. Para o `/init` descrever bem um módulo, comece o arquivo com `//!` e uma primeira frase que diga o que ele faz. As notas que só você sabe ficam para você preencher.
- `/context [tokens|reset]`: mostra ou troca a janela de contexto (`16384`, `16k`). Sem argumento, explica o que muda ao aumentá-la: memória, recarga do modelo, velocidade, tokens gastos e qualidade. `reset` volta ao valor da configuração (`model.context_window`).
- `/temp [0 a 2|reset]`: mostra ou troca a temperatura de geração e explica o efeito. `reset` volta ao valor da configuração (`ollama.temperature`).
- `/status`: painel em abas (Status, Config, Usage e Stats), com versão, modelo, esforço, contexto e contagens da sessão. ←/→ troca de aba, esc fecha.
- `/think`: atalho de `/effort`. Liga no padrão do modelo ou desliga, e mostra o aviso de custo. O rodapé mostra `esforço <nível>` enquanto estiver ligado. O raciocínio de turnos anteriores sai do histórico, para não gastar contexto.
- `/help`: mostra os comandos e as teclas.
- `/restart-dev`: recompila (`cargo build`) e reabre o BitClaw no lugar, com os mesmos argumentos, para ver alterações do código sem sair do terminal. Se a compilação falhar, mostra o erro e continua na sessão atual. A conversa não é preservada. Só funciona em desenvolvimento, com o código-fonte onde foi compilado.
- `/welcome-dev`: reabre o BitClaw como na primeira vez, com a tela de boas-vindas. Não apaga nada; o SOUL.md e o agente ficam como estão.
- `/openrouter`: faz o fluxo de `bitclaw openrouter` (chave, modelo, agente) no próprio terminal e reabre com `--openrouter`. Se o fluxo for cancelado ou falhar, o BitClaw fecha com a mensagem; depois, rode `bitclaw openrouter` e `bitclaw --openrouter`.
- `/exit`: encerra o programa.

`/quit` também encerra o programa, como alias de `/exit`.

O rodapé mostra o modelo, o uso da janela de contexto e, só com `--yes`, o aviso `Automático`. Quando a entrada ou a saída não é um terminal (pipes, scripts), o BitClaw usa um modo simples: uma tarefa por linha, texto puro e sem cores.

Exemplos de tarefas:

```
liste os arquivos do projeto
leia notas.md e resuma em três frases
crie um arquivo fib.py com uma função fibonacci
troque o nome da função soma por adiciona em calc.py
```

## Ferramentas

- `list_dir`: lista um diretório do projeto.
- `read_file`: lê um arquivo. Arquivos com mais de 8 KB são truncados e o aviso diz quantas linhas ele tem. Para gastar menos contexto, `mode: "outline"` devolve só a estrutura (declarações e `//!`, com o número da linha, sem o módulo de testes) e `start_line`/`end_line` leem só um trecho.
- `grep_files`: procura texto no conteúdo dos arquivos, em todas as subpastas (sem regex; ignora maiúsculas e acentos; alternativas com `|`, ex.: `yellow|amarelo`; filtro opcional `ext`). Agrupa por arquivo, põe primeiro quem tem uma definição (`const`, `fn`, `struct`...), marcada com `← definição`, e corta em 40 linhas (6 por arquivo), listando os arquivos que sobraram. Sem resultado, amplia a busca com as palavras soltas e a tradução de termos comuns (amarelo → yellow, tema → theme) e, se ainda assim nada vier, diz o que foi pulado e como procurar de outro jeito.
- `find_files`: acha arquivos e pastas pelo nome ou caminho (não lê o conteúdo), com as mesmas regras de `grep_files`.
- As duas buscas pulam o que o `.gitignore` da raiz ignora (nomes, caminhos e `*.ext`), arquivos e pastas ocultos (`.env`, `.git`, `.bitclaw`), `target`, `node_modules`, `dist`, `build` e arquivos binários. Uma pasta dessas passada em `path` é percorrida.
- `edit_file`: troca um trecho exato que aparece uma única vez. Mostra o diff com contexto, pede confirmação e salva um backup em `.bitclaw/backup/`. Só altera arquivos existentes.
- `create_file`: cria um arquivo novo com o conteúdo dado. Recusa sobrescrever um arquivo existente e pede confirmação.
- `create_dir`: cria uma pasta nova e vazia, com as pastas pais que faltarem. Recusa pasta existente e pede confirmação.
- `delete_file`: remove um arquivo, pede confirmação e salva um backup em `.bitclaw/backup/`. Não remove diretórios.
- `run_script`: executa um script que já existe no projeto. Aplica `chmod u+x`, roda com a pasta do projeto como diretório atual e devolve o código de saída e a saída real (stdout e stderr). O fluxo é `create_file` e depois `run_script`: por exemplo, pedir "as especificações do hardware" faz o agente criar um `info.sh`, mostrar o script inteiro para você aprovar e só então executá-lo. Veja os limites em [Segurança](#segurança).
- `run_command`: roda um programa conhecido, **sem shell**: `program` é só o nome e `args` é a lista (`;`, `&&`, `|`, `>` e `$()` não têm efeito). Três classes. **Leitura** (`ls`, `pwd`, `wc`, `head`, `tail`, `cat`, `git status|diff|log|show|blame|rev-parse|ls-files`) roda direto, com os caminhos presos ao projeto, e também vale no modo Plan. **Alteração** (`git add|commit`, `cargo check|test|build|clippy`) mostra o comando e pede confirmação, e nem `--yes` a concede (`cargo test` roda código do projeto). **Qualquer outro** (`rm`, `sudo`, `curl`, `git push`...) é recusado, e a mensagem manda criar um script e usar `run_script`. Opções que escrevem arquivo ou rodam outro programa (`--output`, `--ext-diff`, `--config`...) e `git -c` são recusadas. Programas interativos não funcionam. Mesmo ambiente reduzido, prazo de 60 s e limite de 6 KB do `run_script`.

Ao fim de cada tarefa, o programa lista as ações executadas: `✓` (feita), `✗` (erro) ou `⊘` (recusada pelo usuário). As ações aparecem antes do texto do modelo. Se nenhuma alteração foi aplicada, ele avisa, mesmo que o modelo tenha dito o contrário. Se o texto final afirma que algo foi removido sem que nenhum arquivo tenha sido removido, o aviso também aparece.

## Contexto

O histórico da conversa tem orçamento. Quando ele excede a janela (`context_window` = 16384 tokens no padrão, com 1024 reservados para a resposta), o programa primeiro troca resultados antigos de ferramenta por um marcador curto (os já superados por uma leitura ou edição posterior do mesmo caminho vão primeiro) e, se ainda não bastar, descarta os turnos mais antigos inteiros. Cada ajuste aparece no terminal. Com `BITCLAW_STATS=1`, cada turno mostra quantos caracteres cada ferramenta devolveu ao modelo. O prompt do sistema e o turno atual nunca são descartados. Se o próprio turno atual não couber, a tarefa é recusada com erro.

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

Os testes não dependem de rede nem de modelo: usam um modelo falso com respostas roteirizadas. A CI (`.github/workflows/ci.yml`) os roda em Linux e macOS a cada push e pull request.

## Publicar uma versão

1. Suba `version` no `Cargo.toml` e faça o commit.
2. Crie e envie a tag com a mesma versão: `git tag v0.2.0 && git push origin v0.2.0`.

O workflow `.github/workflows/release.yml` compila e testa em cada plataforma, num runner nativo, e publica `bitclaw-<alvo>.tar.gz` e `SHA256SUMS` no release. Ele falha se a tag não bater com o `Cargo.toml`. O `install.sh` e o `bitclaw update` leem esses mesmos arquivos; o contrato está no topo de `src/app/update.rs`.

O `curl` do `install.sh` e a consulta do `bitclaw update` não usam credenciais: só funcionam com o repositório público.

## Segurança

- Caminhos absolutos, `..` e symlinks que saiam do diretório do projeto são recusados (`src/sandbox.rs`, com testes).
- Edições, criações e remoções pedem confirmação, a menos que `--yes` seja usado.
- `run_script` **sempre** pede confirmação, mesmo com `--yes`: o `--yes` vale só para edições. A confirmação mostra o script inteiro, com caracteres de controle escritos como `\xNN` (para não esconderem texto no terminal) e linhas longas quebradas. Scripts com mais de 120 linhas ou 32 KB são recusados, porque o usuário precisa conseguir revisá-los. Se o arquivo muda entre a confirmação e a execução, nada é executado.
- O script roda com **as suas permissões e sem isolamento**: ele pode ler e apagar o que você pode, e usar a rede. Só a confirmação protege. Para um agente que não executa nada, tire `run_script` da lista `tools` da configuração.
- `run_command` só roda a lista fixa de programas do código (`src/toolkit/command.rs`); a leitura não pergunta, a alteração sempre pergunta. Nada passa por um shell.
- O processo tem limite de 60 s (o grupo de processos inteiro é encerrado), saída limitada a 6 KB para o modelo e ambiente reduzido: só `PATH`, `HOME`, `USER`, `LANG`, `LC_ALL` e `TMPDIR` passam, então chaves de API e tokens do seu ambiente não chegam ao script.
- Fora o que um script aprovado fizer, não há acesso à rede além do provedor escolhido: o Ollama local, por padrão. Com o OpenRouter, o conteúdo lido dos arquivos é enviado a um serviço remoto; a chave vem de uma variável de ambiente e não chega aos scripts (o ambiente deles é reduzido).

## Limites conhecidos

- Modelos pequenos às vezes escrevem a chamada de ferramenta como texto. Há um fallback que reconhece três formatos (`calls_from_text` em `src/core/agent/text_calls.rs`): JSON (`{"name": ..., "arguments": ...}`), tokens especiais (`read_file{path:<|"|>a.txt<|"|>}`, inclusive várias coladas na linha) e estilo Python (`create_file(path="a.py", content="...")`). A chamada é regravada no histórico como `tool_calls`, para o resultado responder a uma chamada do modelo.
- Leituras idênticas (`list_dir`, `read_file`, `grep_files`, `find_files`) não rodam duas vezes no mesmo turno: o modelo recebe um aviso e, se insistir, uma última rodada sem ferramentas para responder. Resposta totalmente vazia é pedida de novo, até duas vezes por turno.
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
| `src/tui/print.rs` | Saída para a conversa: cabeçalho, avisos, prévias e relatório. |
| `src/tui/transcript.rs` | A conversa em tela cheia: rolagem, refluxo e seleção com o mouse. |
| `src/tui/clipboard.rs` | Cópia da seleção (`pbcopy`, `wl-copy`, `xclip` ou OSC 52). |
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

## Licença

BitClaw é distribuído sob a [PolyForm Noncommercial License 1.0.0](LICENSE). Em resumo: você pode **usar, estudar, modificar e compartilhar** o código para fins não comerciais (pessoal, pesquisa, educação, hobby, organizações sem fins lucrativos). **Uso comercial exige uma licença à parte**, concedida pelo autor: abra uma [issue](https://github.com/Gabrielfernandes7/bitclaw/issues) para conversar. É código com fonte disponível, não software livre no sentido da OSI. O texto em [`LICENSE`](LICENSE) é o que vale; este resumo não o substitui.

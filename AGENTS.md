# BitClaw

Instruções para agentes de código neste projeto. Só fatos verificados no repositório.

> [!NOTE]
> Documento criado pelo BitClaw

## Comandos

```sh
cargo build
cargo test
cargo run
```

Flags do binário: `--config`, `--model`, `--url`, `--dir`, `--yes`, `--no-color`, `--no-mouse`, `--think`, `--plan` (modo Plan: só ferramentas que declaram `read_only`) `--openrouter` (usa o agente gerado por `bitclaw openrouter`) e `--fresh` (primeira abertura simulada, com estado global temporário que some ao sair).

Subcomandos `bitclaw openrouter [status|logout]`, `bitclaw update`, `bitclaw uninstall [--yes] [--purge]` e `bitclaw soul`, e `--version`. Instalação por `install.sh` (releases do GitHub); o contrato dos arquivos do release está no topo de `src/app/update.rs`, e `install.sh` e `.github/workflows/release.yml` o seguem.

## Estrutura

- `Cargo.lock`
- `Cargo.toml`
- `README.md`
- `agents/`
- `assets/`
- `docs/`
- `scripts/`
- `site/`
- `skills/`
- `src/`

## Skills

- `skills/create-skill/SKILL.md`
- `skills/create-tool/SKILL.md` (cria ferramenta como script dentro de uma skill)
- `skills/markdown/SKILL.md`
- `skills/small-model-optimization/SKILL.md`

## Agentes

- `agents/bitclaw.toml` (padrão, embutido no binário)
- `agents/openrouter.toml`

## Notas

<!-- Acrescente o que não é óbvio no código: convenções, limites, o que não mexer. -->

- **Ferramenta nova declara suas anotações.** `Tool::annotations()` tem `read_only`, `destructive` e `idempotent`. Sem declaração, a ferramenta vale como alteradora e destrutiva. Isso vale para `run_script`, que por isso fica fora do modo Plan.
- **Anotação pode depender da chamada.** `Tool::call_annotations(args)` e `per_call_annotations()`: `run_command` é só leitura para `git status` e alteradora para `git commit`. No modo Plan ela continua visível e cada chamada é julgada. O agente usa `Registry::mutates_call`/`idempotent_call`, não os métodos por nome.
- **Prévia tem tipo.** `Preview::new` é uma alteração; `Preview::removal` é uma remoção e nunca entra em aprovação em lote. O tipo não vem do título.
- **Skills instaladas carregam só com digest aprovado.** `/skill install` grava o SHA-256 da pasta em `.bitclaw/skills.lock`. Skill cujo conteúdo mudou, ou sem digest, não é carregada, e a lista de ignoradas diz por quê. Skill instalada antes do digest precisa ser apagada e instalada de novo.
- **Recibos de aprovação** ficam em `.bitclaw/approvals.jsonl`: tipo, título, decisão e quem decidiu. Aprovação em lote (`a`) aparece como do usuário.
- **`/undo` desfaz o último turno com alterações** a partir de `.bitclaw/undo.jsonl`. Só mexe em arquivo que ainda está como o turno deixou. Não desfaz criação de pasta, nem o `/init`. No modo simples (sem terminal) não há `/undo`.
- **SOUL.md** é quem o agente é para a pessoa, não fato do projeto. O de `.bitclaw/SOUL.md` vale mais que o de `~/.config/bitclaw/SOUL.md`. É montado por molde (sem modelo) e só vale na próxima abertura.
- **Estado global** fica em `~/.config/bitclaw/` (ou `$XDG_CONFIG_HOME/bitclaw`): o cache do aviso de versão e a marca da tela de boas-vindas. `projects` é o registro dos projetos onde o BitClaw rodou (só caminhos; `src/app/projects.rs`), para `uninstall --purge` achar as pastas `.bitclaw/`. O resto continua em `.bitclaw/` do projeto. `uninstall` apaga o binário, o estado global e os restos de instalação; `--purge` apaga também as `.bitclaw/` dos projetos, e só um diretório de verdade com esse nome, nunca um link.
- **Arte de terminal** segue `docs/identidade/ARTE-TERMINAL.md`: uma cor por desenho, olho como buraco, só caracteres de largura 1, a grade num SVG em `assets/`.
- **Mascote.** A arte vem de `assets/bitclaw-crab.svg`, a fonte única. Em terminais gráficos (como iTerm2 no macOS), o SVG é rasterizado diretamente em RGBA e exibido via protocolo gráfico nativo; em terminais sem suporte a imagens (como Terminal.app) ou em modo texto, há fallback com meios-blocos.

- **Licença: PolyForm Noncommercial 1.0.0** (`LICENSE`). Uso comercial só com licença à parte do autor. Não adicione dependência ou copie código com licença incompatível, e não troque a licença sem o autor pedir.

## Stack

- Rust, edição 2024
- Dependências: crossterm, image, ratatui, ratatui-image, ratatui-textarea, reqwest, resvg, serde, serde_json, sha2, toml

## Código

- `src/main.rs`: binário do BitClaw. Lê a configuração, escolhe o provedor, monta a sessão (`--plan` filtra as ferramentas; o aprovador é embrulhado para registrar recibos) e abre a TUI ou o modo simples.
- `src/lib.rs`: BitClaw como biblioteca. Expõe `core`, `toolkit` e `adapters`.
- `src/core/`: o agente, sem rede, sem terminal e sem provedor.
  - `src/core/agent/`: `turn.rs` (laço modelo → ferramentas → modelo), `guard.rs` (detector de laço), `text_calls.rs` (chamadas escritas como texto), `hook.rs` (gancho pós-turno), `model.rs` (contrato `ChatModel`), `report.rs` (`TurnReport` e `Progress`).
  - `src/core/tool/`: contrato `Tool` com `Annotations`, `Registry` (inclui `read_only()` para o modo Plan) e `Approver` com `Preview` tipada.
  - `src/core/session.rs`: sessão de um agente, a API pública do core.
  - `src/core/config.rs`: configuração de um agente, lida de TOML.
  - `src/core/context.rs`: mantém o histórico dentro do orçamento da janela do modelo.
  - `src/core/skill.rs`: skills, instruções em pastas com `SKILL.md`, carregadas sob demanda pela ferramenta `load_skill`.
  - `src/core/sandbox.rs`: resolve caminhos dentro da pasta do projeto (recusa absolutos, `..` e symlinks que saem dela).
  - `src/core/models.rs`, `src/core/options.rs`: modelos e capacidades; opções de geração que a interface muda.
- `src/toolkit/`: ferramentas prontas, opcionais.
  - `src/toolkit/fs/`: `list_dir`, `read_file`, `grep_files`, `find_files`, `edit_file`, `create_file`, `create_dir` e `delete_file`. `backup.rs` guarda o conteúdo anterior em `.bitclaw/backup`; `journal.rs` é o diário do `/undo`.
  - `src/toolkit/script.rs`: `run_script`, com aprovação, prazo, saída limitada e ambiente reduzido. `run_process` é a execução comum.
  - `src/toolkit/command.rs`: `run_command`, programas conhecidos sem shell. `classify` põe cada chamada em Leitura, Alteração ou Recusada.
  - `src/toolkit/claims.rs`: confere se o texto final diz que criou, alterou ou removeu algo que as ações não confirmam.
- `src/adapters/`: provedores de modelo. `ollama/` traduz o núcleo para `/api/chat`; `openrouter/` traduz para `/chat/completions`; `lines.rs` lê respostas em streaming.
- `src/app/`: o BitClaw em si.
  - `src/app/provider.rs`: o único lugar que sabe quais adaptadores existem.
  - `src/app/skill_install.rs`: `/skill install` (git, prévia, aprovação, lock com digest) e a carga das skills instaladas.
  - `src/app/soul.rs`: SOUL.md, as cinco perguntas da primeira abertura (`bitclaw soul` refaz) e o texto que entra no prompt do sistema (`Session::builder().persona`, teto de 1200 caracteres).
  - `src/app/openrouter_setup.rs`: `bitclaw openrouter` (chave sem eco e conferida em `/auth/key`, escolha do modelo, grava `~/.config/bitclaw/openrouter.toml` a partir de `agents/openrouter.toml`; chave opcional em `openrouter.key`, 0600, e a variável vale mais). A chave nunca vai para o TOML.
  - `src/app/projects.rs`: o registro de projetos; `src/app/uninstall.rs`: o `uninstall` (binário, estado global, restos e, com `--purge`, as `.bitclaw/`).
  - `src/app/soul_art.rs`: a tela do SOUL, o BitClaw feliz segurando um coração que pulsa (meios-blocos, uma thread repinta só a arte enquanto você responde; sem terminal ou com janela pequena não aparece).
  - `src/app/receipts.rs`: `Recorded`, o aprovador que grava os recibos.
  - `src/app/init.rs`: `/init`, que cria o `AGENTS.md` com fatos do próprio repositório.
  - `src/app/header.rs`: texto do cabeçalho.
  - `src/app/plain.rs`: modo simples, uma tarefa por linha, texto puro.
  - `src/app/tui/`: interface de terminal com ratatui.
    - `brand.rs`: mascote (lido do SVG) e os textos de aviso.
    - `commands.rs`: comandos com `/` e o menu que os lista.
    - `input.rs`: teclas, painéis e despacho dos comandos.
    - `worker.rs`: ponte com a sessão, que roda numa thread e conversa por canais. Marca o começo de cada turno no diário.
    - `mod.rs`, `transcript.rs`, `steps.rs`, `print.rs`, `markdown.rs`, `mention.rs`, `history.rs`, `clipboard.rs`, `restart.rs`, `theme.rs`: a conversa, os passos, a saída, o Markdown, a menção `@`, o histórico de entradas, a área de transferência, o `/restart-dev` e as cores.
    - `view/`: desenho dos painéis e da barra de status.

## Documentação

- `README.md`: seções Pré-requisitos, Como rodar, Configuração do agente, Usar como biblioteca, Uso no prompt, Ferramentas, Contexto, Avaliação com modelos reais, Testes, Segurança, Limites conhecidos, Estrutura
- `docs/ARCHITECTURE.md`: arquitetura alvo do `src/` e o plano de refatoração
- `docs/CHOICE.md`: escolha: política para o histórico que excede o contexto
- `docs/AVALIACAO.md`: os sete critérios de avaliação (escolha e argumentos de ferramenta, chamadas desnecessárias, não chamar, sequência, latência e memória, português) e o que o `eval.sh` cobre
- `docs/MODELOS.md`: modelos para testar e diagnósticos
- `docs/MASCOTE.md`: mascote do BitClaw: versões e variações
- `docs/PROJETO.md`: BitClaw — documento de projeto
- `docs/TEST.md`: pilares do teste
- `docs/identidade/`: SVGs da identidade visual e o script que os gera

## Site

- `site/build.py`: gera o site (GitHub Pages) em `_site/` a partir de `site/content/`, `docs/` e dos SVGs de `assets/` e `docs/identidade/`. `pip install -r site/requirements.txt` e `python3 site/build.py`.
- `site/tools/render_png.py`: gera os PNGs (og-image, favicons, ilustrações) a partir dos SVGs; só macOS, e os PNGs ficam commitados.
- `.github/workflows/pages.yml` publica o site a cada push na `main` que mexa em `site/`, `docs/` ou `assets/`.

## Scripts

- `scripts/eval.sh`: avaliação com modelos reais (Fase 5 de docs/PROJETO.md).

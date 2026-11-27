# HINT.md — O que aproveitar de referências externas

> Documento de pesquisa. Fontes lidas em 2026-11-26. Foco da primeira rodada: **ferramentas (tools)** e **skills**. Coisas básicas primeiro.

Fontes:

- Lista curada: [ai-boost/awesome-harness-engineering](https://github.com/ai-boost/awesome-harness-engineering), seções *Tool Design* e *Skills & MCP*.
- Projeto: [codewhale-hq/Codewhale](https://github.com/codewhale-hq/Codewhale), agente de código open source escrito em Rust. Lidos: `README.md`, `docs/ARCHITECTURE.md`, `docs/SKILLS.md`, `docs/MODES.md`, `docs/RECEIPTS.md`, `docs/SANDBOX.md`, `docs/HOOKS.md`.

---

## 1. Lista awesome — Tool Design

Recursos da seção, agrupados pelo que ensinam:

**Design de interface de ferramenta (o básico)**

- [Writing Effective Tools for Agents](https://www.anthropic.com/engineering/writing-tools-for-agents) (Anthropic): nomes, schemas, mensagens de erro e formato de retorno. A ideia central: *tool design é UX do agente*.
- [A failed network check is a finding, not a tool crash](https://gist.github.com/seifip/fa05e77888fc8ec0e99463084f056ae6#file-readme-md): distinguir "a checagem achou um problema" de "a ferramenta quebrou".
- [Tool Use — Claude API Docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview) e [Function Calling — OpenAI](https://platform.openai.com/docs/guides/function-calling): contrato de `tool_result` e de erro.

**Anotações de risco**

- [Tool Annotations as Risk Vocabulary](https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations/) (MCP): `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`. São *entradas* para a decisão de permissão, não contratos aplicados.

**Tamanho e forma do conjunto de ferramentas**

- [The Devil Is in the Interface](https://arxiv.org/abs/2608.11386) (2026): a arquitetura de exposição das ferramentas muda o comportamento do agente, mesmo com as mesmas capacidades.
- [Playwright MCP](https://github.com/microsoft/playwright-mcp): snapshot por árvore de acessibilidade em vez de screenshot, muito menos tokens. Mesmo princípio do nosso limite de 8 KB em `read_file`.

**Saída estruturada e validação**

- [outlines](https://github.com/dottxt-ai/outlines): restringe a amostragem por JSON Schema. Útil para modelos locais pequenos.
- [instructor](https://python.useinstructor.com/): valida a saída contra um modelo e devolve o erro de validação ao modelo para nova tentativa.

**Ferramentas e paralelismo (mais adiante)**

- [Scaling Parallel Tool Calling](https://arxiv.org/abs/2602.07359): chamadas paralelas reduzem latência. Não é prioridade para modelos pequenos.

## 2. Lista awesome — Skills & MCP

- [Shell + Skills + Compaction](https://developers.openai.com/blog/skills-shell-tips) (OpenAI): skills versionadas, com manifesto `SKILL.md`. Confirma que carregar a skill sob demanda vale a pena.
- [You can't whisper at an AI agent](https://stripe.dev/blog/ai-steering-experiments) (Stripe, 2026): documentação passiva é ignorada pelo agente; instruções *duras* no contexto carregado funcionam. **Implicação direta:** nosso `AGENTS.md` é passivo; regras críticas devem ir para o corpo da skill (carregada com `load_skill`) ou para a mensagem de erro da ferramenta.
- [Agent Skills Can Be Harmful](https://arxiv.org/abs/2608.11888) (2026): 307 falhas causadas por skills. Argumento a favor de avaliar cada skill.
- [skillgrade](https://github.com/mgechev/skillgrade): gera testes a partir do `SKILL.md`, roda várias tentativas e reporta. Modelo para nosso `scripts/eval.sh`.
- [SkillTester](https://arxiv.org/abs/2603.28815): avalia skills em capacidade, robustez e segurança antes de uso.
- [Mitchell Hashimoto, *My AI Adoption Journey*](https://mitchellh.com/writing/my-ai-adoption-journey): todo erro do agente vira uma correção permanente (uma linha no `AGENTS.md` ou uma ferramenta nova). Isto é uma **prática de processo**, não código.

---

## 3. Codewhale — o que é e o que vale a pena

**O que é:** agente de código para terminal, em Rust, com um único motor de turno (`Engine::run_turn`) usado pela TUI, pelo modo headless (`exec`), pelo servidor HTTP e pelo ACP. Tem um workspace grande, com muitas crates separadas por responsabilidade (aprovação, sandbox, skills, hooks, recibos, etc.).

Nosso projeto já compartilha algumas decisões com ele: `Tool` como trait com `spec` + `run`, `Registry` como único ponto de consulta, aprovação injetada (`Approver`), backup antes de sobrescrever, skills carregadas sob demanda via `load_skill`, e um modo simples sem TUI.

### 3.1 Permissões em camadas: "aprovação não é sandbox"

O `docs/SANDBOX.md` deixa explícito que **aprovação** (perguntar ao usuário), **ferramentas que conhecem o workspace** e **sandbox do SO** são controles separados. Hoje, no BitClaw:

- A aprovação existe (`src/core/tool/approval.rs`).
- A confinação de caminho existe (`src/core/sandbox.rs`: recusa absoluto, `..` e symlink que sai da raiz).
- **Não há sandbox do SO.** Scripts rodam depois de aprovação (`approve_run`), sem confinamento de processo.

Codewhale usa `sandbox-exec` no macOS e `bwrap` no Linux. Não é prioridade agora, mas é a referência se um dia rodarmos scripts sem supervisão.

### 3.2 Modos de permissão (Plan = somente leitura)

O `docs/MODES.md` tem um dial de permissão (Ask → Accept Edits → Plan → Auto-Review → Full Access), e no modo Plan a recusa de mutação é **central**, não ad hoc.

Nós já temos o ingrediente: `Tool::mutates()` e `Registry::mutates(name)`. Falta um modo que filtre o registro. Isso é pequeno e útil para revisão de código sem risco de alteração.

### 3.3 Recibos e registro de aprovações

O `docs/RECEIPTS.md` registra, por sessão: arquivos alterados, comandos, cada aprovação **e quem decidiu** (usuário, regra de sessão ou automático), e falhas. Hoje temos `TurnReport` em `src/core/agent/report.rs`, que mostra o turno na tela, mas **nada é persistido**.

Ideia: gravar um `approvals.jsonl` em `.bitclaw/` (a pasta já existe e é ocultada por `list_dir`).

### 3.4 Desfazer (`/undo`)

O `docs/RECEIPTS.md` cita `/undo` (reverte o último turno e suas edições) e `/restore` (volta a um snapshot). Nós **não temos** `undo`, mas `src/toolkit/fs/backup.rs` já guarda o conteúdo anterior antes de cada sobrescrita. Falta indexar esses backups por turno.

### 3.5 Skills: raízes, precedência e confiança

O `docs/SKILLS.md` tem três ideias que nos faltam:

1. **Raízes com precedência e aviso de sombra.** Se o mesmo nome existe em dois lugares, o aviso mostra os dois.
2. **Confiança antes de carregar.** Skills dentro do workspace só carregam depois de `/trust on`, e a confiança é **vinculada ao digest** do conteúdo. Se o arquivo mudar, a confiança cai.
3. **Skill não confiável não muda sozinha.** Installs e updates passam por um controlador único.

Nosso `src/core/skill.rs` carrega de uma pasta configurada no TOML (`load_configured`), sem etapa de confirmação. Se um repositório clonado trouxer uma skill, ela entra no prompt. Isso é um risco real de injeção de instrução. Vale tratar isso antes de expandir o uso de skills.

### 3.6 Hooks

O `docs/HOOKS.md` define pontos de ciclo de vida (ex.: `tool_call_before`), em que um comando de shell pode **negar** a chamada (código de saída 2). É um jeito simples de impor regra sem mudar o agente. Baixa prioridade.

### 3.7 O que NÃO copiar

- Catálogo de 50+ provedores, roteamento por modelo, telemetria, LSP, sub-agentes, fleets, RLM, app-server: escopo muito maior que o BitClaw, que é pequeno e focado em modelo local pequeno.
- Estrutura de muitas crates. Para o nosso tamanho (~13 mil linhas de Rust), módulos bastam.

---

## 4. Prioridades sugeridas (básico primeiro)

| # | Ação | Origem | Tamanho | Onde no código |
|---|---|---|---|---|
| 1 | Trocar `mutates() -> bool` por uma struct de anotações (`read_only`, `destructive`) | MCP annotations | pequeno | `src/core/tool/mod.rs` |
| 2 | Substituir `Preview::is_removal()` (checa se o título começa com `"remover "`) por um campo tipado | Idem; hoje é frágil | pequeno | `src/core/tool/approval.rs` |
| 3 | Modo somente-leitura (Plan) que filtra o `Registry` por mutação | Codewhale MODES | pequeno | `src/core/tool/registry.rs` |
| 4 | Colocar regras críticas no corpo da skill e nas mensagens de erro, não só no `AGENTS.md` | Stripe | pequeno (processo) | `skills/*/SKILL.md`, `src/toolkit/fs/*` |
| 5 | Persistir aprovações por turno em `.bitclaw/approvals.jsonl` | Codewhale RECEIPTS | médio | `src/core/agent/report.rs` |
| 6 | Comando `/undo` usando os backups indexados por turno | Codewhale RECEIPTS | médio | `src/toolkit/fs/backup.rs` |
| 7 | Confiança e digest para skills de pasta do projeto | Codewhale SKILLS | médio | `src/core/skill.rs` |
| 8 | Avaliação por skill (estilo skillgrade) em `scripts/eval.sh` | awesome / skillgrade | médio | `scripts/eval.sh` |
| 9 | Validação de saída com retorno do erro ao modelo (estilo instructor) | awesome | médio | `src/core/agent/` |
| 10 | Sandbox do SO para scripts (seatbelt/bwrap) | Codewhale SANDBOX | grande | `src/toolkit/script.rs` |

Começo sugerido: **1 → 2 → 3**. São pequenas, não mudam o comportamento visível e reforçam o contrato da ferramenta. Depois **4**, que não custa código.

---

## 5. Observações sobre o repositório

- **`AGENTS.md` está desatualizado.** A seção *Código* cita arquivos que não existem: `src/agent.rs`, `src/tools.rs`, `src/sandbox.rs`, `src/context.rs`, `src/session.rs`, `src/tui/`. Os caminhos reais são `src/core/*` (agent, tool, sandbox, context, session, skill, config, models), `src/toolkit/*` (ferramentas de arquivo e scripts) e `src/app/*` (interface, provedores, init). Não alterei o `AGENTS.md`; a correção pode ser feita em seguida, se você quiser.
- O `AGENTS.md` também lista `agents/bitclaw.toml`, mas a pasta tem `agents/openrouter.toml` a mais. Não afeta nada, só vale saber.

---

## 6. Instalação de skills de terceiros

**Problema:** o BitClaw só lê skills de uma pasta local (`skills_dir`, no TOML). Não há nada que busque, valide ou registre skills vindas de fora. `load_with_ignored` em `src/core/skill.rs` trata cada subpasta com `SKILL.md` como skill, então o caminho mais simples é preencher essa pasta de forma segura.

### Proposta

1. **Fonte por repositório git.** `/skill install <url> [--path pasta] [--ref tag-ou-commit]` roda `git clone --depth 1` como subprocesso (sem dependência nova), copia a pasta da skill para `.bitclaw/skills/<nome>/` e fixa o commit.
2. **Pasta separada para terceiros.** `skills/` continua sendo só nosso. Terceiros vão para `.bitclaw/skills/`, e `skills_dir` passa a aceitar uma lista de pastas.
3. **Confiança vinculada ao conteúdo** (inspirado em Codewhale `docs/SKILLS.md`, que usa trust vinculado ao digest). Na instalação gravamos `.bitclaw/skills.lock` com URL, commit e SHA-256 da pasta. Na carga, a skill só entra se o digest bater com o lock; se o conteúdo mudar, fica desativada até nova aprovação.
4. **Aprovação com prévia.** A instalação mostra o `SKILL.md` e a lista de arquivos pelo `Approver`/`Preview` que já existe em `src/core/tool/approval.rs`. Nada é copiado sem confirmação.
5. **Scripts continuam sob aprovação.** Uma skill pode trazer scripts; a execução já passa por `approve_run`, então não muda.
6. **Atualizar e remover.** `/skill update <nome>` refaz o clone no commit de origem e mostra o diff antes de aplicar; `/skill remove <nome>` apaga a pasta e a linha do lock.

Fora do escopo inicial: registro central, busca por nome e atualização automática. Podem vir depois, mas aumentam a superfície de ataque.

### Objeções e riscos

- **Injeção de instrução.** Uma skill é texto que vai para o prompt do modelo. Um repositório de terceiros pode tentar mudar o comportamento do agente. O lock com digest impede mudança silenciosa, mas não impede conteúdo malicioso aprovado pelo usuário. A prévia é a principal defesa, e precisa mostrar o corpo completo, não um resumo.
- **Scripts dentro de skills.** A aprovação é por execução, mas o usuário pode aprovar sem ler o script. Sem sandbox do SO (item 10 da tabela), a proteção é só a confirmação humana.
- **Dependência de `git` no PATH.** Se não estiver instalado, o comando falha. Tarball via `reqwest` evitaria isso, mas exigiria crates para descompactar.
- **Escopo.** A proposta cria três conceitos novos ao mesmo tempo (fonte, lock e comandos). Faz mais sentido entregar em etapas: primeiro instalação local, depois git com lock.
- **Dependência de rede.** Instalar por URL introduz uma rede que o projeto hoje não usa para skills. Precisa ficar explícito que só acontece por comando do usuário.

### Decisão pendente

A fonte deve ser **só URL de git**, ou **também um caminho local**?

- **Git + caminho local (recomendado).** O caminho local serve para testar uma skill de uma pasta vizinha sem rede e sem confiança extra, e é a primeira etapa natural antes do git.
- **Só git.** Mais simples de explicar, mas força qualquer teste a passar por um repositório publicado.

Recomendação: **git + caminho local**, entregando o caminho local primeiro.

---

## 7. Fontes de skills de referência

- **[agentcraft.dev](https://agentcraft.dev)** — biblioteca de skills de agente para Claude Code, organizada por categoria, com prévias de cada skill. Citada na lista awesome como ponto de partida para skills de agente.
  - Relevância para nós: fonte de skills para testar o fluxo de instalação (seção 6) e exemplos de como estruturar um `SKILL.md`.
  - Cuidado: é conteúdo de terceiros. Antes de instalar qualquer skill dali, vale ler o `SKILL.md` e os scripts, pelo mesmo motivo da seção 6.
- **[Claude Design Skills Tested](https://agentcraft.dev/en/blog/claude-design-skills-tested/)** — teste do mesmo briefing com as 8 skills de design mais instaladas, comparadas com um controle sem skill (Claude Haiku, landing page e portfólio, 18 execuções). Útil como modelo para avaliar nossas próprias skills (item 8 da tabela da seção 4).

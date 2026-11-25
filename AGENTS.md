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

## Estrutura

- `Cargo.lock`
- `Cargo.toml`
- `README.md`
- `agents/`
- `assets/`
- `docs/`
- `scripts/`
- `skills/`
- `src/`

## Skills

- `skills/create-skill/SKILL.md`
- `skills/markdown/SKILL.md`
- `skills/small-model-optimization/SKILL.md`

## Agentes

- `agents/bitclaw.toml`

## Notas

<!-- Acrescente o que não é óbvio no código: convenções, limites, o que não mexer. -->

## Stack

- Rust, edição 2024
- Dependências: crossterm, ratatui, ratatui-textarea, reqwest, serde, serde_json, toml

## Código

- `src/agent.rs`: Ciclo modelo → ferramentas → modelo.
- `src/config.rs`: Configuração de um agente, lida de TOML.
- `src/context.rs`: Gerenciador de contexto: mantém o histórico dentro do orçamento da janela do modelo.
- `src/init.rs`: `/init`: cria o `AGENTS.md` do projeto com fatos extraídos do próprio repositório.
- `src/lib.rs`: BitClaw como biblioteca: o core de agente.
- `src/main.rs`: Binário do BitClaw: lê a configuração, monta as peças (adaptador do Ollama, aprovação, interface) e abre o terminal.
- `src/models.rs`: Modelos e suas capacidades, sem depender de provedor nem de interface.
- `src/ollama.rs`: Adaptador do Ollama: traduz o núcleo (modelos, esforço, ferramentas) para a API `/api/chat`.
- `src/sandbox.rs`: Resolve caminhos do agente dentro da pasta do projeto: recusa caminhos absolutos, `..` e symlinks que saem dela.
- `src/session.rs`: Sessão de um agente: histórico, orçamento de contexto e um turno por vez.
- `src/skills.rs`: Skills: instruções em pastas com `SKILL.md`, carregadas sob demanda.
- `src/tools.rs`: Ferramentas de arquivo do agente (list_dir, read_file, edit_file, create_file, create_dir e delete_file), a aprovação das alterações e o backup do que é sobrescrito.
- `src/tui/`: Interface de terminal do BitClaw, com ratatui em tela cheia.
  - `src/tui/brand.rs`: Identidade visual do BitClaw: o mascote e o cabeçalho de texto.
  - `src/tui/commands.rs`: Comandos com `/` e o menu que os lista.
  - `src/tui/history.rs`: Histórico das tarefas enviadas, navegado com ↑ e ↓ como no shell.
  - `src/tui/input.rs`: Entrada do usuário: teclas, painéis e comandos com `/`.
  - `src/tui/markdown.rs`: Renderização leve de Markdown para o terminal: títulos, listas, `código`, **negrito** e blocos de código.
  - `src/tui/mention.rs`: Menção de arquivos com `@`: digitar `@docs/` lista o que há na pasta, e Tab ou Enter completa.
  - `src/tui/plain.rs`: Modo simples: quando a entrada ou a saída não é um terminal (pipes, scripts), o BitClaw lê uma tarefa por linha e escreve texto puro, sem cores nem interface interativa.
  - `src/tui/print.rs`: Saída para a conversa: cabeçalho, avisos, prévias e relatório do turno.
  - `src/tui/theme.rs`: Cores e estilos da interface.
  - `src/tui/worker.rs`: Ponte entre a interface e o agente: a sessão roda numa thread, e os dois conversam por canais.

## Documentação

- `README.md`: seções Pré-requisitos, Como rodar, Configuração do agente, Usar como biblioteca, Uso no prompt, Ferramentas, Contexto, Avaliação com modelos reais, Testes, Segurança
- `docs/CHOICE.md`: Escolha: política para o histórico que excede o contexto
- `docs/MODELOS.md`: Modelos para testar e diagnósticos
- `docs/PROJETO.md`: BitClaw — documento de projeto

## Scripts

- `scripts/eval.sh`: Avaliação com modelos reais (Fase 5 de docs/PROJETO.md).


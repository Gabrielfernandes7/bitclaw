---
description: O agente do BitClaw é um arquivo TOML: modelo, ferramentas, limites. Ollama e OpenRouter.
---
# Configuração

O comportamento do agente vem de um arquivo **TOML**. O padrão é [`agents/bitclaw.toml`](../../../agents/bitclaw.toml), embutido no binário. Para usar outro:

```sh
bitclaw --config meu-agente.toml
```

O formato foi escolhido por ser legível e não exigir código para trocar prompt, ferramentas, modelo ou limites.

## Um agente completo

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

[limits]                 # opcional
max_steps = 8
max_consecutive_failures = 2
reserve_output_tokens = 1024
```

Campos desconhecidos, ferramentas inexistentes ou repetidas e valores incoerentes (por exemplo, `context_window` menor que `reserve_output_tokens`) são **erros com mensagem clara**. `--model` e `--url` sobrescrevem o arquivo.

O core só conhece `[model]` e `[limits]`; `url` e `temperature` pertencem à seção do provedor (`[ollama]`). O formato antigo (`url`, `temperature` e `num_ctx` dentro de `[model]`) ainda é lido, mas `url` e `temperature` geram um aviso.

## Ferramentas embutidas

`list_dir`, `read_file`, `grep_files`, `find_files`, `edit_file`, `create_file`, `create_dir`, `delete_file`, `run_script` e `run_command`. Para um agente que não executa nada, tire `run_script` e `run_command` da lista `tools`. Detalhes em [Ferramentas](ferramentas.md).

## Usar o OpenRouter

O padrão é o Ollama local. Para usar modelos hospedados no [OpenRouter](https://openrouter.ai), escolha o provedor e exporte a chave no ambiente:

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
bitclaw --config meu-agente.toml
```

Há um exemplo pronto em [`agents/openrouter.toml`](../../../agents/openrouter.toml).

- A chave **nunca** vai no TOML (ele é versionado e lido pelo `/init`). Se `api_key_env` parecer uma chave, a configuração é recusada sem repetir o valor. Se a variável faltar, o erro diz qual é.
- `/model` lista os modelos do OpenRouter que chamam ferramentas. `/effort` vale para os que aceitam raciocínio com níveis.
- `/context` só muda o orçamento do histórico: no OpenRouter a janela real é a do modelo.
- Erros 401 (chave), 402 (créditos) e 429 (limite) viram mensagens legíveis. Nenhuma mensagem, `/status` ou log mostra a chave.

> **Privacidade.** Com um provedor remoto, o conteúdo dos arquivos que o agente ler e o histórico da conversa saem da sua máquina. O BitClaw avisa na abertura.

## Variáveis de ambiente

| Variável | Efeito |
|---|---|
| `NO_COLOR` | Desliga as cores. |
| `BITCLAW_WELCOME=1` | Mostra a tela de boas-vindas de novo. |
| `BITCLAW_NO_UPDATE_CHECK=1` | Desliga a consulta de versão nova. |
| `BITCLAW_STATS=1` | Mostra, a cada turno, quantos caracteres cada ferramenta devolveu ao modelo. |
| `BITCLAW_INSTALL_DIR`, `BITCLAW_VERSION` | Só para o `install.sh`. |

## Onde ficam os arquivos

| Lugar | O que guarda |
|---|---|
| `~/.config/bitclaw/` (ou `$XDG_CONFIG_HOME/bitclaw`) | Estado global: cache do aviso de versão, marca da boas-vindas e o SOUL.md global. |
| `.bitclaw/` no projeto | Backups, diário do `/undo`, recibos de aprovação, skills instaladas e SOUL.md do projeto. |
| `AGENTS.md` no projeto | Fatos do repositório para agentes (criado pelo `/init`). |

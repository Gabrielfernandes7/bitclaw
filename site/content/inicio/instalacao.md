---
description: Instale o BitClaw com uma linha de curl, ou compile com cargo. Atualizar e desinstalar.
---
# Instalação

## Binário pronto

Disponível para macOS (Apple Silicon e Intel) e Linux (x86_64 e arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh
```

O script baixa o binário do último release, **confere o SHA-256** e o instala em `~/.local/bin`, sem `sudo`. Ele avisa se essa pasta não estiver no `PATH` e se o Ollama não estiver instalado ou não responder.

| Variável | Efeito |
|---|---|
| `BITCLAW_INSTALL_DIR=/outra/pasta` | Instala em outra pasta. |
| `BITCLAW_VERSION=v0.2.0` | Instala outra versão. |

## Compilar do código-fonte

Com Rust e Cargo instalados:

```sh
cargo install --git https://github.com/Gabrielfernandes7/bitclaw
```

Ou clonando o repositório:

```sh
git clone https://github.com/Gabrielfernandes7/bitclaw
cd bitclaw
cargo run -- --dir /caminho/do/projeto
```

## Pré-requisitos

- **Rust e Cargo**, só para compilar (o binário instalado não precisa).
- **Ollama** rodando em `http://localhost:11434`.
- **Um modelo com suporte a ferramentas** (*tools*). O padrão é `nemotron-3-nano:4b-mlx`:

```sh
ollama pull nemotron-3-nano:4b-mlx
```

Não quer rodar um modelo local? Veja como usar o [OpenRouter](../guia/configuracao.md#usar-o-openrouter).

## Atualizar

```sh
bitclaw update
```

Troca o binário pelo do último release, depois de conferir a soma. Quando sai uma versão nova, o BitClaw avisa na abertura. A consulta ao GitHub é feita em segundo plano, no máximo uma vez por dia, e fica em `~/.config/bitclaw/`. `BITCLAW_NO_UPDATE_CHECK=1` desliga a consulta.

## Desinstalar

```sh
bitclaw uninstall
```

Apaga o binário e `~/.config/bitclaw`, e pergunta antes (`--yes` não pergunta). Sem o binário à mão:

```sh
curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh -s -- --uninstall
```

As pastas `.bitclaw/` dos seus projetos ficam.

## Primeira abertura

Na primeira abertura aparece uma tela de boas-vindas, que mostra o que foi conferido (provedor, modelo e projeto), seguida de cinco perguntas que montam o [SOUL.md](../guia/skills.md#soulmd). `BITCLAW_WELCOME=1` mostra a tela de novo.

Próximo passo: [Primeiros passos](primeiros-passos.md).

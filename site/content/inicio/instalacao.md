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

Mostra o que vai apagar e pergunta antes (`--yes` não pergunta). Apaga:

- o binário;
- `~/.config/bitclaw`: o SOUL.md global, o cache do aviso de versão, a marca da boas-vindas e o registro de projetos;
- restos de instalação e atualização interrompidas: `.bitclaw.install` e `.bitclaw.update` ao lado do binário, e `bitclaw-update-*` na pasta temporária.

As pastas `.bitclaw/` dos seus projetos (backup, recibos, `/undo`, skills instaladas) ficam, a menos que você peça:

```sh
bitclaw uninstall --purge
```

Sem `--purge`, o comando pergunta à parte. Ele acha essas pastas por um registro dos projetos onde o BitClaw rodou (`~/.config/bitclaw/projects`, só os caminhos) e pela pasta atual, e só apaga um diretório de verdade chamado `.bitclaw`, nunca um link. O `AGENTS.md` e o código dos seus projetos nunca são tocados.

Sem o binário à mão:

```sh
curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh -s -- --uninstall
```

Acrescente `--purge` depois de `--uninstall` para apagar também as pastas dos projetos. Um binário instalado com `cargo install` (ou o de `target/`) não é apagado, mas o resíduo é: para o primeiro, rode `cargo uninstall bitclaw`.

## Testar como primeiro acesso

```sh
bitclaw --fresh
```

Abre o BitClaw como se você tivesse acabado de instalar: tela de boas-vindas, as cinco perguntas do SOUL (com o coração batendo) e nenhum cache. O estado global dessa sessão é uma pasta temporária, **apagada ao sair**; o seu `~/.config/bitclaw` não é lido nem tocado. Funciona também com `bitclaw --fresh soul`.

Um `.bitclaw/SOUL.md` que já exista no projeto ainda vale e pula as perguntas: para testar de ponta a ponta, use uma pasta nova (`bitclaw --fresh --dir /tmp/novo`).

## Primeira abertura

Na primeira abertura aparece uma tela de boas-vindas, que mostra o que foi conferido (provedor, modelo e projeto), seguida de cinco perguntas que montam o [SOUL.md](../guia/skills.md#soulmd). `BITCLAW_WELCOME=1` mostra a tela de novo.

Próximo passo: [Primeiros passos](primeiros-passos.md).

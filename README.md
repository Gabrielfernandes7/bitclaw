# BitClaw (MVP)

Agente de linha de comando, local e enxuto, que lê e edita arquivos de um projeto usando Ollama. Escrito em Rust.

## Pré-requisitos
- Rust (cargo)
- Ollama rodando (`http://localhost:11434`) com um modelo que suporte tools. O padrão é `nemotron-3-nano:4b-mlx` (`ollama pull nemotron-3-nano:4b-mlx`).

## Uso
```sh
cargo run -- --dir /caminho/do/projeto
```

Avaliação com modelos reais (tarefas fixas, verificadas no disco): `scripts/eval.sh [modelo ...]`.
Opções: `--model`, `--url`, `--dir` (padrão: diretório atual), `--yes` (aplica edições sem confirmar; use com cuidado).

No prompt, digite a tarefa. `/sair` encerra.

## Ferramentas
- `list_dir`: lista um diretório do projeto.
- `read_file`: lê um arquivo (até 100 KB).
- `edit_file`: troca um trecho exato que aparece uma única vez. Mostra o diff com contexto, pede confirmação e salva backup em `.bitclaw/backup/`. Só altera arquivos existentes.
- `create_file`: cria um arquivo novo com o conteúdo dado. Recusa sobrescrever um arquivo existente e pede confirmação.

Ao fim de cada tarefa, o programa lista as ações executadas com `[ok]`, `[erro]` ou `[recusada]`. Se nenhuma alteração foi aplicada, ele avisa, mesmo que o modelo tenha dito o contrário.

## Segurança
- Caminhos absolutos, `..` e symlinks que saiam do diretório do projeto são recusados (`src/sandbox.rs`, com testes).
- Edições exigem confirmação, a menos que `--yes` seja usado.
- Não há acesso a shell nem à rede, além do Ollama local.

## Limites conhecidos (MVP)
- Modelos pequenos às vezes escrevem a chamada de ferramenta como texto. Há um fallback que reconhece esse formato (`src/main.rs`, `calls_from_text`).
- O modelo pode afirmar que concluiu uma tarefa sem ter aplicado todas as edições. Confira o arquivo.

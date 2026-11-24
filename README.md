# BitClaw (MVP)

Agente de linha de comando, local e enxuto, que lê e edita arquivos de um projeto usando Ollama. Escrito em Rust.

## Pré-requisitos
- Rust (cargo)
- Ollama rodando (`http://localhost:11434`) com um modelo que suporte tools, ex.: `qwen2.5-coder:3b` ou `nemotron-3-nano:4b-mlx`

## Uso
```sh
cargo run -- --dir /caminho/do/projeto --model qwen2.5-coder:3b
```
Opções: `--model`, `--url`, `--dir` (padrão: diretório atual), `--yes` (aplica edições sem confirmar; use com cuidado).

No prompt, digite a tarefa. `/sair` encerra.

## Ferramentas
- `list_dir`: lista um diretório do projeto.
- `read_file`: lê um arquivo (até 100 KB).
- `edit_file`: troca um trecho exato que aparece uma única vez. Mostra o diff, pede confirmação e salva backup em `.bitclaw/backup/`.

## Segurança
- Caminhos absolutos, `..` e symlinks que saiam do diretório do projeto são recusados (`src/sandbox.rs`, com testes).
- Edições exigem confirmação, a menos que `--yes` seja usado.
- Não há acesso a shell nem à rede, além do Ollama local.

## Limites conhecidos (MVP)
- Modelos pequenos às vezes escrevem a chamada de ferramenta como texto. Há um fallback que reconhece esse formato (`src/main.rs`, `calls_from_text`).
- O modelo pode afirmar que concluiu uma tarefa sem ter aplicado todas as edições. Confira o arquivo.

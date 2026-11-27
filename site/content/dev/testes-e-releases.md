---
description: Como rodar os testes, a avaliação com modelos reais e como publicar uma versão do BitClaw.
---
# Testes e releases

## Testes

```sh
cargo test
```

Os testes **não dependem de rede nem de modelo**: usam um modelo falso com respostas roteirizadas. A CI (`.github/workflows/ci.yml`) os roda em Linux e macOS a cada push e pull request, com `cargo build --locked` e `cargo test --locked`.

Regra do projeto: cada funcionalidade nova precisa de teste automatizado que não dependa de rede nem de modelo real.

## Avaliação com modelos reais

```sh
scripts/eval.sh                          # padrão: qwen2.5-coder:3b e nemotron-3-nano:4b-mlx
scripts/eval.sh nemotron-3-nano:4b-mlx   # um ou mais modelos
REPEATS=5 scripts/eval.sh ...            # repetições (padrão: 3)
```

O script roda tarefas fixas (criar `.txt`, criar `.py`, editar, ler e responder, não alterar nada, entre outras). Cada tarefa roda num **projeto novo**, e o resultado é verificado **no disco, não pelo texto do modelo**.

Os resultados e os diagnósticos estão em [Modelos e diagnósticos](../projeto/modelos.md).

## Publicar uma versão

1. Suba `version` no `Cargo.toml` e faça o commit.
2. Crie e envie a tag com a mesma versão:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

O workflow `.github/workflows/release.yml` compila e testa em cada plataforma, num runner nativo, e publica `bitclaw-<alvo>.tar.gz` e `SHA256SUMS` no release. Ele **falha se a tag não bater com o `Cargo.toml`**.

O `install.sh` e o `bitclaw update` leem esses mesmos arquivos; o contrato está no topo de `src/app/update.rs`. Nem o `curl` do `install.sh` nem a consulta do `bitclaw update` usam credenciais: só funcionam com o repositório público.

## Este site

A documentação que você está lendo é gerada pelo script `site/build.py` e publicada no GitHub Pages pelo workflow `.github/workflows/pages.yml` a cada push na `main` que mexa em `site/`, `docs/`, `assets/` ou nos SVGs da identidade. Para ver localmente:

```sh
python3 -m venv .venv && . .venv/bin/activate
pip install -r site/requirements.txt
python3 site/build.py --base ./
python3 -m http.server -d _site 8000
```

As páginas de `docs/` (projeto, arquitetura alvo, modelos, mascote) entram como estão: editar o arquivo no repositório atualiza o site.

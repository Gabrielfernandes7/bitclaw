---
description: As ferramentas embutidas do BitClaw: leitura, busca, edição, scripts e comandos conhecidos.
---
# Ferramentas

O agente age no projeto por **ferramentas**. Cada uma é uma implementação da trait `Tool`, registrada no `Registry`; o laço do agente não conhece nomes de ferramenta. Quais ficam disponíveis é decidido pela lista `tools` da [configuração](configuracao.md).

## Resumo

| Ferramenta | O que faz | Pede aprovação |
|---|---|---|
| `list_dir` | Lista um diretório do projeto. | Não |
| `read_file` | Lê um arquivo (ou só a estrutura, ou só um trecho). | Não |
| `grep_files` | Procura texto no conteúdo dos arquivos. | Não |
| `find_files` | Acha arquivos e pastas pelo nome ou caminho. | Não |
| `edit_file` | Troca um trecho exato, que aparece uma única vez. | Sim |
| `create_file` | Cria um arquivo novo. | Sim |
| `create_dir` | Cria uma pasta nova e vazia. | Sim |
| `delete_file` | Remove um arquivo. | Sim (nunca em lote) |
| `run_script` | Executa um script que já existe no projeto. | **Sempre**, até com `--yes` |
| `run_command` | Roda um programa conhecido, sem shell. | Depende da chamada |

## Leitura e busca

**`read_file`** limita a 8 KB: arquivos maiores são truncados e o aviso diz quantas linhas ele tem. Para gastar menos contexto, `mode: "outline"` devolve só a estrutura (declarações e `//!`, com o número da linha, sem o módulo de testes), e `start_line`/`end_line` leem só um trecho.

**`grep_files`** procura texto no conteúdo, em todas as subpastas. Sem regex; ignora maiúsculas e acentos; aceita alternativas com `|` (por exemplo, `yellow|amarelo`) e um filtro opcional `ext`. Agrupa por arquivo, põe primeiro quem tem uma definição (`const`, `fn`, `struct`…), marcada com `← definição`, e corta em 40 linhas (6 por arquivo), listando os arquivos que sobraram. Sem resultado, amplia a busca com as palavras soltas e a tradução de termos comuns (amarelo → yellow, tema → theme).

**`find_files`** acha arquivos e pastas pelo nome ou caminho, sem ler o conteúdo, com as mesmas regras de `grep_files`.

As duas buscas pulam o que o `.gitignore` da raiz ignora, arquivos e pastas ocultos (`.env`, `.git`, `.bitclaw`), `target`, `node_modules`, `dist`, `build` e arquivos binários. Uma pasta dessas passada em `path` é percorrida.

## Alteração

- **`edit_file`** troca um trecho exato que aparece uma única vez. Mostra o diff com contexto, pede confirmação e salva um backup em `.bitclaw/backup/`. Só altera arquivos existentes.
- **`create_file`** recusa sobrescrever um arquivo existente.
- **`create_dir`** cria as pastas pais que faltarem e recusa pasta existente.
- **`delete_file`** salva um backup, não remove diretórios, e sua prévia é do tipo *remoção*: nunca entra numa aprovação em lote (`a`).

## Executar

### `run_script`

Executa um script que **já existe** no projeto: aplica `chmod u+x`, roda com a pasta do projeto como diretório atual e devolve o código de saída e a saída real (stdout e stderr). O fluxo é `create_file` e depois `run_script`: pedir "as especificações do hardware" faz o agente criar um `info.sh`, mostrar o script inteiro para você aprovar e só então executá-lo.

### `run_command`

Roda um programa conhecido **sem shell**: `program` é só o nome e `args` é a lista (`;`, `&&`, `|`, `>` e `$()` não têm efeito). Há três classes:

| Classe | Programas | Comportamento |
|---|---|---|
| **Leitura** | `ls`, `pwd`, `wc`, `head`, `tail`, `cat`, `git status\|diff\|log\|show\|blame\|rev-parse\|ls-files` | Roda direto, com os caminhos presos ao projeto. Também vale no modo Plan. |
| **Alteração** | `git add\|commit`, `cargo check\|test\|build\|clippy` | Mostra o comando e pede confirmação; nem `--yes` a concede (`cargo test` roda código do projeto). |
| **Recusada** | `rm`, `sudo`, `curl`, `git push`… | Recusada; a mensagem manda criar um script e usar `run_script`. |

Opções que escrevem arquivo ou rodam outro programa (`--output`, `--ext-diff`, `--config`…) e `git -c` são recusadas. Programas interativos não funcionam. O ambiente é reduzido, o prazo é de 60 s e a saída é limitada a 6 KB.

## Anotações: o que cada ferramenta declara

Toda ferramenta nova declara as suas anotações (`Tool::annotations()`): `read_only`, `destructive` e `idempotent`. Sem declaração, vale como alteradora e destrutiva. É por isso que `run_script` fica fora do modo Plan.

A anotação pode depender da chamada (`Tool::call_annotations(args)`): `run_command` é só leitura para `git status` e alteradora para `git commit`. No modo Plan ela continua visível e cada chamada é julgada.

## O que o relatório diz

Ao fim de cada tarefa, o programa lista as ações executadas: `✓` (feita), `✗` (erro) ou `⊘` (recusada). Se o texto final do modelo afirma que algo foi criado, alterado ou removido sem que a ação tenha acontecido, o aviso também aparece.

Para criar a sua própria ferramenta, veja [Usar como biblioteca](../dev/biblioteca.md) e a skill `create-tool` em [Skills e SOUL](skills.md).

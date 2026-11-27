---
description: O que protege você no BitClaw, o que não protege, e os limites conhecidos.
---
# Segurança

O BitClaw é honesto sobre o que protege e o que não protege. Esta página separa as duas coisas.

## O que protege

**Sandbox de caminhos.** Caminhos absolutos, `..` e symlinks que saiam do diretório do projeto são recusados, com testes.

**Aprovação antes de alterar.** Edições, criações e remoções mostram a prévia e esperam sua decisão, a menos que você use `--yes`. A prévia tem tipo: `Preview::removal` nunca entra numa aprovação em lote (`a`).

**Recibos.** Cada decisão fica em `.bitclaw/approvals.jsonl`: tipo, título, decisão e quem decidiu. Uma aprovação em lote aparece como do usuário.

**Backup e `/undo`.** Antes de editar ou remover, o conteúdo anterior vai para `.bitclaw/backup/`. `/undo` desfaz o último turno com alterações e só mexe em arquivo que ainda está como o turno deixou.

**Modo Plan.** `bitclaw --plan` deixa só as ferramentas que declaram `read_only`. Nada é editado, criado, removido nem executado.

**Skills com digest.** Skill instalada só carrega se o SHA-256 da pasta bate com o aprovado em `.bitclaw/skills.lock`.

**Segredos fora do TOML.** A chave do OpenRouter vem de uma variável de ambiente ou do arquivo `~/.config/bitclaw/openrouter.key` que `bitclaw openrouter` grava com permissão 600 (e que o BitClaw recusa se outras pessoas puderem lê-lo); se `api_key_env` parecer uma chave, a configuração é recusada sem repetir o valor. A digitação da chave não aparece na tela.

## Scripts: a confirmação é a única proteção

`run_script` **sempre** pede confirmação, mesmo com `--yes`: o `--yes` vale só para edições.

- A confirmação mostra o **script inteiro**, com caracteres de controle escritos como `\xNN` (para não esconderem texto no terminal) e linhas longas quebradas.
- Scripts com mais de **120 linhas ou 32 KB** são recusados, porque você precisa conseguir revisá-los.
- Se o arquivo muda entre a confirmação e a execução, **nada é executado**.
- O processo tem limite de **60 s** (o grupo de processos inteiro é encerrado) e saída limitada a **6 KB** para o modelo.
- O **ambiente é reduzido**: só `PATH`, `HOME`, `USER`, `LANG`, `LC_ALL` e `TMPDIR` passam, então chaves de API e tokens do seu ambiente não chegam ao script.

> **Atenção.** O script roda com **as suas permissões e sem isolamento**: pode ler e apagar o que você pode (inclusive a chave guardada em `~/.config/bitclaw/openrouter.key`, se você a guardou), e usar a rede. Só a confirmação protege. Para um agente que não executa nada, tire `run_script` da lista `tools`.

## Comandos

`run_command` só roda a lista fixa de programas do código. A leitura não pergunta; a alteração sempre pergunta, e nem `--yes` a concede. Nada passa por um shell. Detalhes em [Ferramentas](ferramentas.md#run_command).

## Rede e privacidade

Fora o que um script aprovado fizer, não há acesso à rede além do provedor escolhido: o **Ollama local**, por padrão. Com o OpenRouter, o conteúdo dos arquivos lidos e o histórico saem da sua máquina; o BitClaw avisa na abertura.

A consulta de versão nova vai ao GitHub, em segundo plano, no máximo uma vez por dia, sem credenciais. `BITCLAW_NO_UPDATE_CHECK=1` a desliga.

## Limites conhecidos

- Modelos pequenos às vezes escrevem a chamada de ferramenta como texto. Há um fallback que reconhece três formatos: JSON, tokens especiais e estilo Python. Mesmo assim, um erro de sintaxe pode perder a ação.
- O modelo pode afirmar que concluiu uma tarefa sem ter aplicado todas as edições. **Confira o arquivo.** O programa avisa nos casos mais óbvios, mas não é garantia: a verificação de alegações é heurística.
- Leituras idênticas não rodam duas vezes no mesmo turno; se o modelo insistir, ele recebe uma última rodada sem ferramentas.
- Resposta totalmente vazia é pedida de novo, até duas vezes por turno.
- `/undo` não desfaz criação de pasta nem o `/init`.

Encontrou uma falha de segurança? Abra uma [issue](https://github.com/Gabrielfernandes7/bitclaw/issues) descrevendo o cenário, sem incluir segredos.

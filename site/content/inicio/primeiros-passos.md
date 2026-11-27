---
description: Abra o BitClaw num projeto, faça a primeira tarefa e entenda o que aparece na tela.
---
# Primeiros passos

## 1. Abra no seu projeto

```sh
bitclaw --dir ~/meu-projeto
```

`--dir` é o projeto que o agente pode ler e alterar. Sem a opção, vale a pasta atual. O agente **não sai** dessa pasta: caminhos absolutos, `..` e symlinks que escapam dela são recusados.

Ao iniciar, o programa mostra o modelo em uso e a janela de contexto. Se o modelo não estiver instalado no Ollama, ele avisa antes de começar.

## 2. Peça em linguagem natural

Digite a tarefa e pressione <kbd>Enter</kbd>:

```text
liste os arquivos do projeto
leia notas.md e resuma em três frases
crie um arquivo fib.py com uma função fibonacci
troque o nome da função soma por adiciona em calc.py
```

O BitClaw abre em tela cheia: a conversa ocupa a parte de cima e rola; o editor, o rodapé e os painéis ficam fixos embaixo. O que estava no terminal antes volta intacto ao sair.

## 3. Aprove (ou recuse) as alterações

Quando o agente quer criar, editar ou remover um arquivo, aparece uma **prévia** (um diff, no caso de edição). Responda:

| Tecla | Efeito |
|---|---|
| <kbd>s</kbd> | Aplica a alteração. |
| <kbd>n</kbd>, <kbd>Enter</kbd> ou <kbd>Esc</kbd> | Recusa. |

Antes de qualquer edição ou remoção, o conteúdo anterior é guardado em `.bitclaw/backup/`.

## 4. Leia o relatório

Ao fim de cada tarefa, o BitClaw lista as ações executadas, antes do texto do modelo:

- `✓` feita
- `✗` erro
- `⊘` recusada por você

Se nenhuma alteração foi aplicada, ele avisa, mesmo que o modelo tenha dito o contrário.

## 5. Desfaça, se precisar

`/undo` desfaz o último turno com alterações, a partir de `.bitclaw/undo.jsonl`. Só mexe em arquivo que ainda está como o turno deixou.

## Só olhar, sem tocar

Para explorar um projeto sem risco, abra em modo Plan:

```sh
bitclaw --plan
```

Só as ferramentas de leitura ficam disponíveis. Nada é editado, criado, removido nem executado.

## Para onde ir agora

- [No prompt](../guia/prompt.md): teclas, `@arquivo` e todos os comandos `/`.
- [Configuração](../guia/configuracao.md): trocar de modelo, de provedor, de agente.
- [Ferramentas](../guia/ferramentas.md): o que o agente consegue fazer.
- [Segurança](../guia/seguranca.md): o que protege você e o que não protege.

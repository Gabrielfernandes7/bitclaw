---
name: create-skill
description: Cria uma nova skill do BitClaw no projeto, como uma pasta com SKILL.md. Use quando o usuário pedir para criar, escrever ou montar uma skill.
---

# Criar uma skill

Uma skill é uma pasta com um arquivo `SKILL.md`. Ela fica na pasta de skills do projeto (normalmente `skills/`).

## Passos

1. Se o nome e a finalidade já estiverem na mensagem do usuário, use-os e não pergunte de novo. Só pergunte o que estiver faltando.
2. Escolha o nome seguindo as regras de `name` abaixo. Esse nome vira o nome da pasta.
3. Confira se a pasta já existe com `list_dir` na pasta de skills. Se existir, não sobrescreva: pergunte ao usuário.
4. Crie a pasta com `create_dir`, por exemplo `skills/revisar-testes`.
5. Crie `SKILL.md` com `create_file`, no formato abaixo.
6. Leia o arquivo de volta com `read_file` e confira: frontmatter entre `---`, `name` igual ao nome da pasta, `description` preenchida.
7. Teste o gatilho: escreva um pedido que deveria acionar a skill e veja se a descrição basta para identificá-la. Se não bastar, reescreva a `description` com termos do próprio pedido.
8. Responda ao usuário com o caminho criado e avise que a skill aparece no `/skills` na próxima vez que o BitClaw for iniciado.

## Formato do SKILL.md

```
---
name: nome-da-skill
description: O que a skill faz e quando usá-la, em uma ou duas frases.
---

# Título

Instruções objetivas em passos, na ordem em que devem ser seguidas.
```

## Regras de `name`

- Só letras minúsculas, números e hífens. Ex.: `revisar-testes`.
- No máximo 64 caracteres.
- Igual ao nome da pasta.
- Não use as palavras reservadas `anthropic` nem `claude`.

## Regras de `description`

- Obrigatória e não vazia.
- No máximo 1024 caracteres, sem tags XML (nada de `<...>`).
- Diz o que a skill faz e quando usá-la. É o principal critério para o modelo escolher a skill, então use as palavras que o usuário usaria.

## Regras do corpo

- Até 500 linhas. Conteúdo longo vai para `references/`, linkado diretamente do `SKILL.md`, sem subpastas dentro de subpastas.
- Instruções curtas e verificáveis. Não copie documentação longa.
- Scripts ficam em `scripts/` e só são citados quando forem necessários.

---
name: create-tool
description: Cria uma ferramenta nova para o projeto como um script (shell ou Python) dentro de uma skill, que o agente roda com run_script. Use quando o usuário pedir para criar, montar ou automatizar uma ferramenta, comando ou script reutilizável.
---

# Criar uma ferramenta

Uma ferramenta criada aqui é uma **skill com um script**: uma pasta com `SKILL.md` e `scripts/<nome>.sh` (ou `.py`). O agente a roda com `run_script`, que sempre pede aprovação. Não é uma ferramenta embutida em Rust: essas só entram mudando o código do BitClaw.

## Passos

1. Se o nome e o que a ferramenta faz já estiverem no pedido, não pergunte de novo. Pergunte só o que faltar: o que entra (argumentos) e o que sai.
2. Escolha o nome (só minúsculas, números e hífens, ex.: `contar-linhas`). Ele vira a pasta da skill.
3. Veja se `skills/<nome>` já existe com `list_dir`. Se existir, não sobrescreva: pergunte.
4. Escolha a linguagem: **shell** para encadear comandos (`grep`, `find`, `git`, `wc`); **Python** para contas, JSON e texto. Em dúvida, shell.
5. Crie `skills/<nome>` e `skills/<nome>/scripts` com `create_dir`.
6. Crie o script com `create_file`, seguindo `references/modelos.md`. Regras do script abaixo.
7. Crie `skills/<nome>/SKILL.md` com `create_file`: frontmatter (`name` igual à pasta, `description` com o que faz e quando usar) e os passos: "rode `run_script` com `path` `skills/<nome>/scripts/<nome>.sh` e `args` ...".
8. Teste: chame `run_script` com um argumento de exemplo e confira a saída. Se falhar, corrija o script e teste de novo; não diga que funciona sem ter rodado.
9. Responda com o caminho criado, como chamar e o que o teste mostrou. Avise que a skill aparece no `/skills` na próxima abertura do BitClaw.

## Regras do script

- **Uma tarefa por script.** Nome e saída dizem o que ele faz.
- **Argumentos pelo `$1`, `$2`** (shell) ou `sys.argv` (Python). Sem argumentos obrigatórios, explique o uso e saia com erro.
- **Só leitura por padrão.** Nada de `rm`, `mv`, `git push`, `curl | sh` nem escrita fora da pasta do projeto, a menos que o usuário peça e o `SKILL.md` diga isso com todas as letras.
- **Saída curta.** O modelo só vê 6 KB: filtre com `head`, `tail`, `grep` ou resuma.
- **Erro que ensina.** Em falha, imprima em uma linha o que deu errado e como corrigir, e saia com código diferente de zero.
- **Sem segredos** no código nem na saída. O ambiente do script só traz `PATH`, `HOME`, `USER`, `LANG`, `LC_ALL` e `TMPDIR`.
- Prazo de 60 s: nada de laços sem fim nem de esperar entrada do teclado.

## Limites

- O script roda com `run_script`, então **fica fora do modo Plan** (`--plan`), que só aceita ferramentas de leitura.
- Skill criada aqui é local do projeto. Para compartilhar, o usuário publica a pasta em um repositório e outros instalam com `/skill install`.

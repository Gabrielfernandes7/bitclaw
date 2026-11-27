---
description: Skills (instruções em pastas), instalação com digest aprovado, SOUL.md e o /init.
---
# Skills e SOUL

## Skills

Uma **skill** é uma pasta com um `SKILL.md`: instruções que o modelo segue em tarefas específicas. Elas são carregadas **sob demanda** pela ferramenta `load_skill`, então não gastam contexto até serem necessárias.

O repositório traz quatro, em `skills/`:

| Skill | Para quê |
|---|---|
| `create-skill` | Criar uma skill nova. |
| `create-tool` | Criar uma ferramenta como script dentro de uma skill. |
| `markdown` | Escrever Markdown. |
| `small-model-optimization` | Ajustar instruções para modelos pequenos. |

`/skills` lista as carregadas e as ignoradas, com o motivo.

### Instalar uma skill

```text
/skill install anthropics/skills --path skills/doc-coauthoring
```

Forma geral: `/skill install <dono/repo | https://…> [--path pasta] [--ref tag]`.

1. O BitClaw baixa a skill de um repositório git (`git clone --depth 1`).
2. Mostra a **origem, o commit, os arquivos, os scripts e o corpo do `SKILL.md`**.
3. Só instala depois de `/skill yes` (`/skill no` descarta).

A skill vai para `.bitclaw/skills/<nome>/`, separada do `skills/` do projeto. Se o repositório tem várias skills, o BitClaw diz quais e pede `--path`.

Recusas: origem que não seja `https://` ou `dono/repo`, link simbólico, mais de 200 arquivos ou 2 MB. Nunca sobrescreve uma skill existente.

### Só carrega com digest aprovado

`/skill install` grava o **SHA-256 da pasta** em `.bitclaw/skills.lock`. Uma skill instalada cujo conteúdo mudou, ou sem digest, **não é carregada**, e a lista de ignoradas diz por quê. Uma skill instalada antes do digest existir precisa ser apagada e instalada de novo.

> Uma skill é texto que o modelo vai seguir. Leia a prévia antes de aprovar.

## SOUL.md

O **SOUL.md** é quem o agente é *para você*: nome, idioma, tamanho das respostas, se explica antes de alterar e o que você faz. Não é fato do projeto, e sim da relação.

- A primeira abertura faz **cinco perguntas** (`pular` dispensa). `bitclaw soul` refaz.
- O arquivo fica em `~/.config/bitclaw/SOUL.md`; um `.bitclaw/SOUL.md` no projeto vale mais.
- `/soul` mostra o que está em vigor.
- É montado por molde, **sem modelo**, e entra no prompt do sistema cortado em 1200 caracteres.
- Só vale na próxima abertura.

## `/init` e o AGENTS.md

`/init` cria um `AGENTS.md` com o que o **próprio repositório diz sobre si**: a descrição do README, a stack, os comandos, o que cada arquivo de `src/` faz (a primeira frase do comentário `//!` de abertura), a documentação, os scripts, as skills e os agentes.

- Não usa modelo: sai na hora, sem gastar tokens e sem inventar.
- Mostra a prévia e só grava depois da sua aprovação.
- Se o `AGENTS.md` já existe, **nunca o sobrescreve**: só acrescenta as seções que faltam; o que você escreveu fica como está.
- As notas que só você sabe ficam para você preencher.

Dica: para o `/init` descrever bem um módulo, comece o arquivo com `//!` e uma primeira frase que diga o que ele faz.

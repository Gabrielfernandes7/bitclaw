---
description: Teclas, comandos com barra, menção @ de arquivos e opções de linha de comando do BitClaw.
---
# No prompt

Digite a tarefa em linguagem natural e pressione <kbd>Enter</kbd>. O BitClaw abre em tela cheia: a conversa ocupa a parte de cima e rola, e o editor, o rodapé e os painéis ficam fixos embaixo.

## Opções de linha de comando

| Opção | Padrão | Função |
|---|---|---|
| `--dir D` | diretório atual | Projeto que o agente pode ler e alterar. |
| `--config ARQ` | agente embutido | Outro agente, em TOML. |
| `--model M` | `nemotron-3-nano:4b-mlx` | Modelo do provedor. |
| `--url U` | `http://localhost:11434` | Endereço do Ollama. |
| `--yes` | desligado | Aplica edições, criações e remoções sem pedir confirmação. Use com cuidado. Não vale para `run_script` nem para comandos de alteração. |
| `--plan` | desligado | Modo Plan: só ferramentas de leitura. |
| `--fresh` | desligado | Abre como na primeira vez, com estado global temporário que some ao sair. Veja [Testar como primeiro acesso](../inicio/instalacao.md#testar-como-primeiro-acesso). |
| `--think` | desligado | Liga o raciocínio (*thinking*) do modelo. Gasta mais tokens; nem todo modelo suporta. |
| `--no-color` | desligado | Desliga as cores. Também vale com `NO_COLOR` e quando a saída não é um terminal. |
| `--no-mouse` | desligado | Devolve o mouse ao terminal (seleção nativa); a conversa rola só pelo teclado. |
| `-V`, `--version` | | Mostra a versão. |
| `-h`, `--help` | | Mostra a ajuda. |

Subcomandos: `bitclaw update`, `bitclaw uninstall [--yes] [--purge]` e `bitclaw soul` (refaz as perguntas do [SOUL.md](skills.md#soulmd)).

## Teclas

| Tecla | Efeito |
|---|---|
| <kbd>Enter</kbd> | Envia a tarefa. |
| <kbd>Shift</kbd>+<kbd>Enter</kbd>, <kbd>Alt</kbd>+<kbd>Enter</kbd> ou <kbd>Ctrl</kbd>+<kbd>J</kbd> | Nova linha no editor, que cresce até 6 linhas. Colar texto com várias linhas funciona. |
| <kbd>Esc</kbd> ou <kbd>Ctrl</kbd>+<kbd>C</kbd> durante um turno | Interrompe. Uma requisição ao modelo em andamento não é abortada: o turno para assim que ela volta. |
| <kbd>Ctrl</kbd>+<kbd>C</kbd> no editor | Limpa o editor; com o editor vazio, encerra. <kbd>Ctrl</kbd>+<kbd>D</kbd> com o editor vazio também encerra. |
| <kbd>↑</kbd> / <kbd>↓</kbd> no editor | Recupera tarefas anteriores. Com o menu `/` aberto, escolhem o comando. |
| <kbd>s</kbd> / <kbd>n</kbd> num pedido de aprovação | Aplica ou recusa a alteração. <kbd>Enter</kbd> e <kbd>Esc</kbd> recusam. |

**Rolar a conversa:** roda do mouse ou trackpad, <kbd>PageUp</kbd>/<kbd>PageDown</kbd>, <kbd>Shift</kbd>+<kbd>↑</kbd>/<kbd>↓</kbd> linha a linha, <kbd>Ctrl</kbd>+<kbd>Home</kbd> e <kbd>Ctrl</kbd>+<kbd>End</kbd> para o começo e o fim.

**Copiar:** arraste na conversa para selecionar e solte para copiar; o rodapé confirma com "copiado". A cópia usa `pbcopy` (macOS), `wl-copy` ou `xclip` e, sem nenhum deles, a sequência OSC 52 do terminal.

## Mencionar arquivos com `@`

`@` no editor lista os arquivos e pastas do projeto (`@docs/` mostra o conteúdo de `docs`). <kbd>↑</kbd>/<kbd>↓</kbd> escolhem, <kbd>Tab</kbd> ou <kbd>Enter</kbd> completam e <kbd>Esc</kbd> fecha. Pastas ocultas, `target` e `node_modules` ficam de fora, e nada sai da pasta do projeto.

O texto `@caminho` vai para o modelo como escrito: o conteúdo do arquivo não é anexado; o modelo o lê com `read_file` se precisar.

## Comandos com `/`

Digite `/` para abrir o menu. Setas escolhem, <kbd>Tab</kbd> completa, <kbd>Enter</kbd> executa e <kbd>Esc</kbd> fecha.

| Comando | O que faz |
|---|---|
| `/help` | Mostra os comandos e as teclas. |
| `/clear` | Limpa a tela e apaga o histórico da conversa. O prompt do sistema é mantido. |
| `/status` | Painel em abas (Status, Config, Usage e Stats) com versão, modelo, esforço, contexto e contagens da sessão. |
| `/model` | Lista os modelos instalados, com busca. `/model load <id>` troca direto; `/model show [id]` mostra a ficha. A conversa é mantida. |
| `/effort [nível]` | Mostra ou muda o esforço de raciocínio (`off`, `on`, `low`, `medium`, `high`), só nos níveis que o modelo tem. |
| `/think` | Atalho de `/effort`: liga no padrão do modelo ou desliga. |
| `/context [tokens\|reset]` | Mostra ou troca a janela de contexto (`16384`, `16k`). |
| `/temp [0 a 2\|reset]` | Mostra ou troca a temperatura de geração. |
| `/skills` | Lista as skills carregadas e as ignoradas (com o motivo). |
| `/skill install <dono/repo>` | Baixa uma skill de um repositório git, mostra a prévia e só instala depois de `/skill yes`. |
| `/soul` | Mostra o SOUL.md em vigor. |
| `/init` | Cria um `AGENTS.md` com fatos do próprio repositório, sem usar modelo. |
| `/undo` | Desfaz o último turno com alterações. |
| `/restart-dev` | Recompila e reabre o BitClaw no lugar (só em desenvolvimento). |
| `/exit`, `/quit` | Encerra o programa. |

### Detalhes que valem a pena

- **`/model`** mostra o ativo, o padrão da configuração e as capacidades de cada modelo (ferramentas, raciocínio, janela). Não dá para trocar durante uma tarefa, e a escolha vale só para a sessão.
- **`/effort`**: modelos sem raciocínio só aceitam `off`. Ao trocar de modelo, um esforço que não existe no novo modelo cai para o nível mais próximo. O raciocínio de turnos anteriores sai do histórico para não gastar contexto.
- **`/init`** mostra a prévia e só grava depois da sua aprovação. Se o `AGENTS.md` já existe, nunca o sobrescreve: só acrescenta as seções que faltam.
- **`/undo`** não desfaz criação de pasta nem o `/init`. No modo simples (sem terminal) não há `/undo`.

## O que a tela mostra

- Enquanto o agente trabalha, o status mostra uma lâmpada que pulsa e o passo atual (`lendo AGENTS.md…`).
- Cada ferramenta aparece na conversa quando termina: `✓ leu AGENTS.md`, `⊘` se recusada, `✗` se falhou. Isso vem das próprias chamadas de ferramenta, então não gasta token.
- O tempo vem em `(59s · modelo 52s)`: total do turno e quanto foi espera pelo modelo.
- O rodapé mostra o modelo, o uso da janela de contexto e, só com `--yes`, o aviso `Automático`.

## Modo simples

Quando a entrada ou a saída não é um terminal (pipes, scripts), o BitClaw usa um modo simples: uma tarefa por linha, texto puro e sem cores.

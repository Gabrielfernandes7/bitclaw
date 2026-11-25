# BitClaw — documento de projeto

Este documento registra o propósito, as decisões e o plano do BitClaw. Serve para retomar o trabalho sem perder o rumo. Atualize-o quando uma decisão mudar.

## 1. Propósito

BitClaw é um **Agent Core** experimental, modular e extensível: uma fundação para construir agentes de IA sem reimplementar a infraestrutura básica. Ele não é um agente especializado nem um clone do OpenClaw.

Projetos previstos sobre o core: DevClaw, ResearchClaw, BusinessClaw. Outras pessoas também devem poder construir agentes sobre ele.

## 2. Critério de sucesso

> Alguém consegue construir um novo agente usando só a API pública do Core, sem modificar o Core.

Esse critério é a métrica principal. Quantidade de funcionalidades não mede sucesso. Para avaliar uma mudança, pergunte:

1. Isso pertence ao Core ou a um agente específico?
2. Isso facilita construir o próximo agente sem editar o Core?

## 3. Regras de fronteira

- **Código específico de agente nunca entra no Core**, mesmo que seja conveniente.
- **O Core não depende de um modelo específico.** Modelos entram por implementação de uma trait, nunca por código fixo.
- **Ferramentas são extensões.** Adicionar uma ferramenta não deve exigir editar o loop do agente.
- **Não há dependência de Ollama no loop.** O Ollama é um adaptador, como qualquer outro provedor.
- **Cada funcionalidade nova precisa de teste automatizado** que não dependa de rede nem de modelo real.

## 4. Fora do escopo por agora

Cada item só entra quando sua dependência existir.

| Item | Depende de |
|---|---|
| Skills | Trait `Tool`, configuração de agente como dado e um segundo agente para validar o formato |
| Memória persistente | Gestão de contexto funcionando |
| Dreaming (consolidação de conhecimento) | Memória persistente, e uma definição de contrato ainda inexistente |
| Orquestração de modelos heterogêneos | Um único modelo validado no ciclo básico |
| Interface gráfica ou web | Core estável e API pública documentada |

## 5. Estado atual (MVP)

Binário Rust em `src/`:

- `main.rs`: CLI, loop modelo → ferramentas → modelo, prompt do sistema e limites como constantes.
- `tools.rs`: três ferramentas (`list_dir`, `read_file`, `edit_file`), com esquemas e execução em `match`es fixos.
- `ollama.rs`: cliente HTTP acoplado ao Ollama. Não define `num_ctx`.
- `sandbox.rs`: resolução de caminhos, com testes contra `..`, caminhos absolutos e symlinks.

Atualização: o ciclo agora está em `agent.rs`, separado do Ollama por uma trait `ChatModel`, com testes sem rede. Há também `create_file`, relatório de ações com o resultado real de cada ferramenta, `num_ctx` explícito (8192) e `read_file` limitado a 8 KB.

Lacunas conhecidas:

- Não há ferramenta de busca. A "pesquisa em arquivos" depende de `list_dir` e `read_file`.
- O histórico de mensagens ainda cresce sem limite. Só o resultado de `read_file` é limitado. A política para o histórico que excede o contexto é a parte que falta da Fase 3.
- Modelo e prompt ainda estão acoplados ao binário (Fase 4).
- Chamadas de ferramenta escritas como texto só são aceitas se o JSON for válido. Um modelo que erra a sintaxe perde a ação sem aviso além do texto da resposta.

Modelos instalados localmente: `nemotron-3-nano:4b-mlx` (2,9 GB) e `qwen2.5-coder:3b` (1,9 GB). Máquina: Mac com M4 e 16 GB de memória unificada.

## 6. Plano em fases

Cada fase tem um critério de saída. Não avance sem cumprir o anterior.

**Fase 1 — Loop testável.**
Trait `Model` e um modelo falso com respostas roteirizadas. Testes para limite de passos, detecção de repetição, fallback de texto e erro do modelo.
*Saída:* `cargo test` passa sem Ollama.

**Fase 2 — Extensão por contrato.**
Trait `Tool` (nome, esquema e execução juntos), registro consultado pelo loop, e `Approver` injetado no lugar da leitura direta do stdin. As três ferramentas atuais viram implementações.
*Saída:* uma ferramenta de teste pode ser adicionada sem editar o loop.

**Fase 3 — Contexto sob controle.**
`num_ctx` explícito na chamada ao Ollama, limite de tamanho para resultados de ferramentas, e política para o histórico que excede o limite. Testado com o modelo falso.
*Saída:* uma conversa longa não estoura o contexto nem perde o prompt do sistema sem aviso.

**Fase 4 — Agente como dado e separação lib/bin.**
Prompt, ferramentas, modelo e limites viram configuração. O Core vira biblioteca, e o binário só monta as peças.
*Saída:* o binário não contém lógica de agente.

**Fase 5 — Avaliação com modelos reais.**
Conjunto fixo de 10 a 20 tarefas (listar, ler, editar, sequência de ferramentas, várias etapas), com verificação do resultado no disco. Rodar com `qwen2.5-coder:3b`, `nemotron-3-nano:4b-mlx` e `functiongemma`.
Métricas: taxa de conclusão, chamadas inválidas, chamadas desnecessárias, número de passos, latência e uso de RAM.
*Saída:* tabela comparativa que define o modelo padrão.
*Pré-requisito a verificar:* a versão do Ollama (o FunctionGemma pode exigir 0.13.5 ou superior). Use `ollama --version`.

**Fase 6 — Segundo agente como prova.**
Um agente diferente do primeiro, construído só pela API pública. Exemplo: um agente que resume arquivos, sem ferramentas de escrita.
*Saída:* o critério de sucesso é medido. Cada mudança que o Core precisou sofrer é registrada na seção de decisões.

**Depois da Fase 6:** avaliar skills com base nos casos reais dos dois agentes; depois, memória; Dreaming por último.

Smoke test manual com `qwen2.5-coder:3b` pode ser feito a qualquer momento pelo CLI atual, mas não substitui a Fase 5.

## 7. Decisões tomadas

- **Modelo inicial de referência:** Nemotron 3 Nano 4B, por ser de propósito geral com foco em agentes.
- **Modelo padrão:** `nemotron-3-nano:4b-mlx`. Na avaliação de 5 tarefas, única execução, passou em 5/5 (`scripts/eval.sh`).
- **Controle da avaliação:** `qwen2.5-coder:3b`, 2/5. Falhas: repetiu chamadas de leitura até a proteção de repetição interromper o turno, e editou um arquivo criado sem ser pedido. Não é padrão, mas fica como referência.
- **FunctionGemma: não validado nesta integração, fora da avaliação padrão.** Em teste direto na API (Ollama 0.40.18, que declara suporte a `tools`), gerou 22 tokens e devolveu `content` vazio, sem `tool_calls`. O parser do Ollama não reconheceu a saída. Isso indica problema de template ou formato, não de capacidade de escolher ferramentas; a causa não foi investigada. Reabrir só depois de resolver o parser e rodar a avaliação de novo.
- **Avaliação com 3 execuções por tarefa (5 tarefas, `scripts/eval.sh`):** Nemotron 15/15; Qwen 3/15 (só passou em `editar`, com 4 a 5 erros por execução). Falhas do Qwen: chamada de ferramenta como texto com JSON malformado (chave de fechamento faltando, descartada pelo fallback), repetição de `read_file`, e edição além do pedido. A execução única anterior tinha dado 2/5 para o Qwen, o que mostra a variância.
- **Fase 2 (trait `Tool`):** ferramentas são implementações de `Tool` (nome, esquema, `mutates`, execução) guardadas em um `Registry`. A confirmação é injetada por `Approver` (`AutoApprover` com `--yes`, `StdinApprover` no terminal). O loop não conhece nomes de ferramenta. A proteção contra repetir uma alteração no mesmo turno é genérica (usa `mutates`).
- **Política de histórico (Fase 3): opção A**, em `docs/CHOICE.md`, com os ajustes registrados ali. Orçamento = (num_ctx − 1024 de saída) × caracteres por token − esquemas. Começa em 3 caracteres/token e é calibrado com `prompt_eval_count`.
- **Correções do diagnóstico de remoção:** `delete_file` (com backup e confirmação); regra no prompt para perguntas e remoções; avisos quando o texto final afirma criação, alteração ou remoção que as ações não confirmam (heurística por palavras, gera aviso e não bloqueia); parada após duas falhas seguidas da mesma ferramenta, mesmo com argumentos diferentes.
- **Experiência no terminal:** informações do modelo na inicialização (via `/api/show`), uso da janela em percentual no prompt, indicador "modelo pensando" com tempo decorrido, e `/clear` para apagar o contexto.
- **Fronteira corrigida:** `agent.rs` não depende mais do Ollama. A implementação de `ChatModel` para o Ollama está em `ollama.rs`.
- **Janela de contexto:** `num_ctx` = 16384 no padrão atual (configuração), antes 8192. `read_file` limitado a 8 KB. Esses valores são escolhas iniciais, não medidas; reavaliar com a Fase 5 quando houver tarefas de várias etapas.
- **Números de benchmark de terceiros** (ex.: BFCL do Google) não entram na decisão sem verificação. Vale a avaliação própria.
- **FunctionGemma não substitui o Nemotron como modelo principal**, mesmo que volte a funcionar: planejamento e tarefas com várias etapas são outra capacidade.
- **Não construir orquestração multimodelo agora.** Primeiro validar um único modelo no ciclo básico.
- **Ordem de trabalho:** Fase 1 antes de qualquer nova funcionalidade.

## 8. Próximos passos

1. ~~Trait `Model` e modelo falso (Fase 1).~~ Feito como `ChatModel` em `agent.rs`.
2. ~~Testes do loop.~~ Feito: repetição, fallback de texto, ferramenta desconhecida, falso sucesso, limite de passos (`MAX_STEPS`) e erro do modelo. `cargo test`: 52 testes passam sem Ollama. **Fase 1 fechada.**
3. ~~Versão do Ollama e contexto.~~ Ollama 0.40.18 confirmado. `num_ctx` explícito. Falta verificar o efeito real de 8192 na memória (16 GB).
4. ~~Fase 2: trait `Tool`, `Registry`, `Approver`.~~ Feito. Critério de saída cumprido: um teste registra uma ferramenta nova (`Echo`) sem editar o loop (`agent.rs`).
5. ~~Fase 3: política para o histórico.~~ Feito: `ContextManager` em `context.rs` (opção A de `docs/CHOICE.md`). Descarta turnos antigos inteiros, protege sistema e turno atual, calibra com `prompt_eval_count` e registra cada descarte no terminal. 39 testes sem rede.
   Medição de RAM com `num_ctx` 8192: `nemotron-3-nano:4b-mlx` ocupa **5,2 GB** (100% GPU) em um Mac de 16 GB. Medido com o modelo carregado e sem histórico cheio; um histórico no limite deve ser medido de novo na Fase 5.
   **Estado atual:** o padrão da configuração (`agents/bitclaw.toml`) é `num_ctx = 16384`. Esse valor ainda não foi medido na memória. A constante `NUM_CTX` em `src/ollama.rs` só serve de padrão interno e não define a janela do binário.
6. Ferramenta de busca: nome e escopo ainda em aberto. Agora é só registrar uma implementação de `Tool`.
7. **Fase 4 (agente como dado e separação lib/bin): feita.**
   - `src/lib.rs` expõe o core. `src/main.rs` só lê a configuração, monta o adaptador do Ollama e abre o terminal.
   - Configuração em TOML (`agents/bitclaw.toml`), com `deny_unknown_fields`, validação de valores coerentes e de ferramentas. Escolha: TOML, por ser legível e não exigir código para trocar prompt, ferramentas, modelo ou limites. O binário embute o arquivo padrão; `--config` usa outro.
   - `Session` é a API pública: recebe o modelo como `Box<dyn ChatModel>`, então o core não cria adaptadores.
   - Limites viraram parâmetro (`Limits`, `run_turn_with`); `run_turn` continua com os padrões.
   - Opções do Ollama (`num_ctx`, `temperature`) vêm da configuração.
   - `cargo test`: 64 testes unitários e 1 doctest, sem Ollama. Avaliação com nemotron: 5/5 PASS, igual antes da refatoração.
   - Critério de saída: o binário não tem lógica de agente (só monta peças). A prova de que a API basta é a Fase 6.
8. **Fase 6 (segundo agente como prova): feita com lacunas registradas.**
   - ResearcherClaw (`../ResearcherClaw`) é um agente de pesquisa somente leitura, em projeto separado, dependendo do `bitclaw` por caminho. Usa só a API pública (`Session`, `AgentConfig`, `ollama::Client`, `AutoApprover`, `Silent`).
   - Nenhuma alteração no core foi necessária para o agente compilar nem para rodar. Teste real: respondeu sobre a configuração do ClawBit citando `agents/bitclaw.toml`. Ao tentar `create_file` (não configurada), a ferramenta foi recusada como desconhecida e nenhum arquivo foi criado.
   - Somente leitura é garantida pela configuração (`tools = ["list_dir", "read_file"]`), não por código do agente.
   - **Lacuna 1 (resolvida depois, ver abaixo):** a biblioteca imprime no terminal por conta própria: `eprintln!` em `agent.rs` (cada chamada de ferramenta) e em `tools.rs` (diffs e confirmação de edição, criação e remoção). Um agente sem terminal, ou que mostra a própria interface, não consegue silenciar isso. Correção prevista: eventos de execução por uma trait de observação, e o terminal fica no binário.
   - **Lacuna 2 (core, menor):** `Progress` só sinaliza espera do modelo. Os passos das ferramentas chegam apenas no `TurnReport`, depois do turno. Resolve-se junto com a lacuna 1.
   - Montagem do Ollama repetida em cada binário (opções, cliente). Aceitável por enquanto; um helper no core só se o terceiro agente repetir o código.
9. **Interface de terminal (feita).** A biblioteca não imprime mais nada. Prévias de alteração viram dados (`Preview`), entregues ao `Approver`, e o progresso de ferramentas vai para `Progress::tool_call`. O binário tem o módulo `src/ui.rs`, com paleta amarela inspirada no crabe, cabeçalho em caixa, barra de contexto no prompt, relatório com `✓ ✗ ⊘` e ações antes do texto do modelo. Sem cor fora de terminal, com `NO_COLOR` ou `--no-color`. Não há dependência nova.

## 9. Questões em aberto

- Qual o formato de configuração de agente (TOML, JSON ou código)? Decidir na Fase 4.
- Política de licença do projeto? Não definida.
- Nome e escopo da ferramenta de busca (Fase 2 ou depois).
- O conjunto de tarefas da Fase 5 precisa de um projeto de exemplo fixo. Qual será?

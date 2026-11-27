# Avaliação: o que o BitClaw precisa acertar

Sete critérios que valem para qualquer mudança no agente, no prompt, nas ferramentas ou na escolha do modelo. Para cada um: a pergunta que ele responde, como medir e **o que `scripts/eval.sh` já cobre hoje**. A lista é de intenção; a coluna "Hoje" é o que existe de fato, e as lacunas estão marcadas como lacunas.

O princípio é o de sempre (ver `docs/PROJETO.md`, Fase 5): o resultado se verifica **no disco**, não pelo texto do modelo, e cada tarefa roda num projeto novo.

| # | Critério | Pergunta |
|---|---|---|
| 1 | Precisão na escolha da ferramenta | Entre as ferramentas disponíveis, o modelo escolheu a certa? |
| 2 | Precisão dos argumentos | A chamada veio com os argumentos certos (caminho, trecho, padrão)? |
| 3 | Chamadas desnecessárias | Quantas chamadas além do mínimo para a tarefa? |
| 4 | Não chamar ferramenta quando não precisa | Em pergunta que se responde sozinha, ele respondeu sem ferramenta? |
| 5 | Chamadas sequenciais | Em tarefa de várias etapas, a ordem e o encadeamento estavam certos? |
| 6 | Latência e consumo de memória | Quanto tempo e quanta memória o turno custou? |
| 7 | Taxa de erros em português | O desempenho cai quando o pedido e a resposta são em português? |

## 1. Escolha da ferramenta

Exemplo: "qual arquivo implementa o widget de status?" pede busca (`grep_files` ou `find_files`), não `edit_file`; "liste os arquivos" pede `list_dir`.

- **Como medir:** por tarefa, a lista de ferramentas aceitáveis; o log de cada execução já traz uma linha por ferramenta chamada.
- **Hoje:** só **indireto**. As tarefas `achar_arquivo`, `achar_funcao`, `navegar_cor` e `nao_alterar` falham se a ferramenta errada leva a um resultado errado, mas o script não confere qual ferramenta foi usada.
- **Defesas no código:** `Registry` só expõe as ferramentas da configuração; no modo Plan, só as de leitura; `run_command` classifica cada chamada (Leitura, Alteração, Recusada).

## 2. Argumentos

Exemplo: `edit_file` com um trecho que não existe, ou que aparece duas vezes; caminho absoluto; `grep_files` com regex (a ferramenta não aceita).

- **Como medir:** chamadas que terminaram em erro de argumento, por tarefa.
- **Hoje:** a coluna `erros` do `eval.sh` conta ações com `[erro]` e turnos interrompidos. Não separa erro de argumento de outros erros.
- **Defesas no código:** a ferramenta devolve um erro legível ao modelo; parada após duas falhas seguidas da mesma ferramenta (`max_consecutive_failures`); fallback que reconhece chamadas escritas como texto em três formatos.

## 3. Chamadas desnecessárias

Exemplo: ler o mesmo arquivo duas vezes, listar a pasta antes de criar um arquivo cujo nome já é dado.

- **Como medir:** número de chamadas por execução, contra o mínimo conhecido da tarefa.
- **Hoje:** **lacuna.** O script não conta chamadas. `BITCLAW_STATS=1` mostra quantos caracteres cada ferramenta devolveu ao modelo, o que ajuda a achar o desperdício.
- **Defesas no código:** leitura idêntica não roda duas vezes no mesmo turno (aviso e, se insistir, uma última rodada sem ferramentas); limite de passos (`max_steps`).

## 4. Não chamar ferramenta quando não precisa

Exemplo: "qual é o meu nome?" se responde com o SOUL.md que já está no prompt. "execute `bitclaw soul`" não se resolve procurando no código: quem roda é a pessoa.

- **Como medir:** em tarefas de resposta direta, **zero** chamadas e a resposta certa.
- **Hoje:** `perguntar_nome` e `pedir_comando` conferem a resposta e que nada mudou no projeto. Elas **não** exigem zero chamadas de leitura. `nao_alterar` confere que nada foi alterado.
- **Em sentido contrário** (a regra 1 do prompt): pergunta sobre o projeto exige inspecionar antes de responder. `ler_responder` e `responder_fiel` conferem que a resposta usa o conteúdo lido.

## 5. Chamadas sequenciais

Exemplo: `grep_files` → `read_file` do trecho → `edit_file`; ou `create_file` → `run_script`.

- **Como medir:** tarefas cujo sucesso depende de duas ou mais chamadas em ordem, verificadas no disco.
- **Hoje:** `editar`, `renomear`, `corrigir_py`, `navegar_cor` e `navegar_real` (o pedido real no próprio código do BitClaw: só a definição do amarelo muda).
- **Lacuna:** não há tarefa com `create_file` seguido de `run_script`, que é o fluxo das perguntas sobre o computador.

## 6. Latência e memória

- **Latência:** a coluna `seg` do `eval.sh` é o tempo de cada execução. Na interface, o tempo vem em `(59s · modelo 52s)`: total do turno e espera pelo modelo.
- **Memória:** **fora do script.** Medições feitas à mão: `nemotron-3-nano:4b-mlx` ocupa 5,2 GB com `num_ctx` 8192 num Mac de 16 GB (ver `docs/PROJETO.md`). O padrão atual, 16384, ainda não foi medido; `ollama ps` mostra o uso com o modelo carregado.
- **Ao medir memória, meça com histórico cheio**, não só com o modelo carregado: é aí que a janela pesa.

## 7. Português

Interpretação adotada: **o pedido vem em português e o código está em inglês**, e modelos pequenos erram mais nesse cruzamento (buscar "cor amarela" quando o código diz `yellow`; responder em outro idioma; perder acentos).

- **Como medir:** a mesma tarefa em português e em inglês, com os mesmos arquivos, e comparar a taxa de acerto; mais o idioma e a ortografia da resposta final.
- **Hoje:** **todas as tarefas do `eval.sh` já são em português**, então a taxa de acerto é, na prática, a taxa em português. **Falta a comparação em inglês** para saber quanto o idioma custa.
- **Defesas no código:** `grep_files` aceita alternativas (`yellow|amarelo`) e, sem resultado, traduz termos comuns (amarelo → yellow, tema → theme); o prompt manda buscar em inglês e em português; o SOUL.md fixa o idioma da resposta.

## Como usar esta lista

Antes de aceitar uma mudança que mexe no prompt, nas ferramentas ou no modelo padrão, rode `scripts/eval.sh` com `REPEATS` de 3 ou mais (uma execução só engana: o Qwen deu 2/5 numa execução e 3/15 em três) e compare com a rodada anterior. Quando uma lacuna acima atrapalhar uma decisão, vale fechá-la no script antes de decidir: contar chamadas por execução (critérios 1, 3 e 4) é a maior.

Resultados e diagnósticos dos modelos: [`docs/MODELOS.md`](MODELOS.md).

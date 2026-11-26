# Modelos para testar e diagnósticos

Registro dos modelos que o BitClaw usa hoje e dos candidatos a testar. Cada entrada separa o que foi **medido** do que foi **lido em fontes** e do que ainda é **hipótese**.

## Critério de escolha

- Basta o modelo ser **somente texto**. Imagem é opcional e, se existir, é um bônus.
- Precisa chamar ferramentas (*tools*) com confiabilidade, porque o BitClaw é um agente.
- Precisa rodar local, em memória de laptop (alvo: cerca de 4 GB).
- Janela de contexto: o BitClaw usa 16384 tokens hoje. Contexto longo é desejável, mas não é requisito.

## Modelo atual: `nemotron-3-nano:4b-mlx`

Servido pelo Ollama. Cabeçalho do BitClaw: 4,0B parâmetros, `nvfp4`, janela de 16384 tokens.

### Diagnóstico (sessão de 25/11/2026)

**Medido**
- Pedido: `/skills markdown [Crie um markdown sobre a receita de bolo de fubá]`. O BitClaw ficou em "pensando…" por mais de 99 s sem responder, e o processo foi encerrado à mão.
- O `ollama ps` não listava modelo carregado, então esse modelo MLX não parece passar pelo Ollama como esperado. **A investigar.**

**Hipóteses (não confirmadas)**
- O nome da skill (`markdown`) apareceu no pedido, e `with_named_skills` (`src/session.rs`) anexou o corpo inteiro da skill à mensagem, o que aumenta o prompt em 1,5 a 2 mil tokens.
- O modelo raciocina antes de responder, e um prompt maior pode alongar esse raciocínio.
- Parte do tempo pode ser só a velocidade do modelo, sem relação com a skill.
- `/skills markdown [...]` pode ter sido enviado como texto comum, e não como comando do painel de skills.

**Ação já tomada:** `skills/markdown/SKILL.md` foi reduzido de 601 palavras para 14 linhas de regras.

**Como confirmar**
1. Rodar `Crie um arquivo .md com a receita de bolo de fubá`, sem a palavra "markdown", e anotar o tempo.
2. Rodar o mesmo pedido com a palavra "markdown" e a skill enxuta, e comparar.
3. Rodar sem a palavra de novo, para medir a variação entre execuções.
4. Conferir com `ollama ps` e `ollama list` onde o modelo está sendo servido.

### Observação (sessão de 26/11/2026)

**Medido**
- Pedido para listar os arquivos do projeto: o modelo chamou a ferramenta de script, o BitClaw executou `list_dir.sh` depois da aprovação e a saída voltou inteira para o modelo. Gastou 3045 tokens de entrada e 25 de saída no turno.
- A resposta final disse que o script exibiu "a lista completa de diretórios e arquivos". Na tela, a saída apareceu cortada em 20 linhas (`… mais 4 linhas`), então o modelo viu mais do que o usuário.

**A testar de novo**
- Os testes de lentidão de cima, que continuam abertos.
- Se ele segue pedindo ferramentas certas em tarefas de várias etapas (ler, editar e conferir), medindo quantas chamadas erram o nome ou o caminho.
- Se ele respeita a aprovação em lote (`a`): tarefas com várias edições seguidas.
- Repetir a listagem de arquivos com a saída expansível na tela, para comparar o que o usuário vê com o que o modelo diz.

## Testado e reprovado: `deepseek-r1:7b`

**Medido (sessão de 26/11/2026)**
- `ollama show deepseek-r1:7b` informa as capacidades `tools`, `thinking` e `completion`. O BitClaw, portanto, enviou as ferramentas ao modelo.
- Pedido: `apague: ideias.md`. O modelo **não chamou `delete_file`**. No raciocínio ele escreveu o comando como texto, com um caminho inventado (`/path/to/src/ideias.md`), e na resposta escreveu uma "saída de ferramenta" falsa (`<｜tool▁output｜begin｜> arquivo removido: ideias.md …`) seguida de "removido com sucesso". O arquivo não foi tocado.
- O BitClaw avisou corretamente que nenhuma alteração foi aplicada e que nada foi removido.

**Conclusão:** declarar `tools` no Ollama não garante que o modelo as use. Este modelo não serve como agente. Para os próximos candidatos, o teste mínimo é pedir uma remoção e conferir se há uma ação `delete_file` no relatório do turno, e não só o texto.

## Candidato: `NVIDIA-Nemotron-3.5-Bolt-E4B-4GB`

Status: **não testado.**

Repositório: `nvidia/NVIDIA-Nemotron-3.5-Bolt-E4B-4GB` (Hugging Face), com a versão quantizada Q4_K_M em GGUF.

### O que as fontes dizem

Fontes: [relatório técnico](https://research.nvidia.com/labs/nemotron/files/NVIDIA-Nemotron-3.5-Bolt-E4B-4GB-Technical-Report.pdf), [página da NVIDIA](https://research.nvidia.com/labs/nemotron/NVIDIA-Nemotron-3.5-Bolt-E4B-4GB/), [tabela de benchmarks](https://research.nvidia.com/labs/nemotron/Nemotron-3.5-Bolt-E4B-4GB/). Fontes secundárias: [tilnote.io](https://tilnote.io/pages/6adc895d7244e77059d7ea38), [buildmvpfast.com](https://www.buildmvpfast.com/blog/nvidia-nemotron-3-5-super-vl-bolt-open-models-2026).

| Item | O que consta |
| :--- | :--- |
| Lançamento | 23 de outubro de 2026. |
| Finalidade | Modelo pequeno e aberto para agentes conversacionais, com meta de cerca de 4 GB de memória. |
| Base | Gemma 4 E4B com pós-treino Nemotron. |
| Tamanho | Cerca de 8B de parâmetros totais e 4B efetivos (fonte secundária). |
| Modalidade | **Somente texto.** Sem imagem nem vídeo. Atende ao critério acima. |
| Licença | OpenMDW-1.1 sobre Apache 2.0 do Gemma 4 (fonte secundária). |
| Extra | Drafter DFlash para decodificação especulativa. |

Benchmarks divulgados pela própria NVIDIA:

| Teste | Bolt E4B | Observação |
| :--- | ---: | :--- |
| RoleMRC | 80,6 | Acima de Nemotron 3 Nano 4B, Qwen 3.5 4B e Gemma 4 E4B. |
| When2Call-MT | 63,4 | Mede quando chamar ferramenta. Relevante para o BitClaw. |
| LongBench v2 | 50,5 | Abaixo do Qwen 3.5 4B (59,9). Contexto longo é ponto fraco. |

Os números são da NVIDIA, não foram reproduzidos por nós.

### O que falta saber

- Se existe build compatível com o backend do BitClaw (hoje o Ollama): o nome no Ollama, ou um GGUF importável, ou MLX.
- Se o `tool_calls` do Ollama funciona com ele sem ajuste de template.
- Se o raciocínio do modelo (*thinking*) pode ser desligado ou limitado, já que o tempo de resposta foi o problema do modelo atual.

### Plano de teste

Rodar `scripts/eval.sh` com o modelo atual e com o candidato, no mesmo pedido, e registrar:

| Métrica | Atual (`nemotron-3-nano:4b-mlx`) | Candidato (`Bolt-E4B-4GB`) |
| :--- | :--- | :--- |
| Tempo até a primeira resposta | | |
| Tempo total do turno | | |
| Memória usada (pico) | | |
| Chama a ferramenta certa, com argumentos válidos | | |
| Segue a skill `markdown` | | |
| Estoura a janela de 16384 tokens | | |

Pedidos de teste:
1. `Crie um arquivo .md com a receita de bolo de fubá` (escrita simples, sem skill nomeada).
2. O mesmo pedido citando a skill `markdown`.
3. Uma tarefa que exija ler dois arquivos e editar um (ferramentas encadeadas).

Preencher a tabela antes de decidir a troca.

## Modelos disponíveis no Ollama

Listados abaixo são os modelos que podem ser baixados e utilizados:

| Nome | ID | Tamanho | Modificado |
|------|----|---------|------------|
| qwen3.5:0.8b-mlx | 12f40a7f8473 | 1.2 GB | About an hour ago |
| deepseek-r1:7b | 755ced02ce7b | 4.7 GB | 3 hours ago |
| bonsai-2:9b | c8b5462f8688 | 3.2 GB | 3 hours ago |
| lfm3:latest | f609fc0ab12a | 1.7 GB | 13 hours ago |
| nemotron-3-nano:4b-mlx | 87e6cab38edd | 2.9 GB | 4 days ago |

## Modelos em execução

| Nome | Tamanho | Processador | Contexto | Runner |
|------|---------|-------------|----------|--------|
| bonsai-2:9b | 3.3 GB | 100% GPU | 16384 | llamacpp |

## Modelos para testar e diagnósticos

Registro dos modelos que o BitClaw usa hoje e dos candidatos a testar. Cada entrada separa o que foi **medido** do que foi **lido em fontes** e do que ainda é **hipótese**.

## Critério de escolha

- Basta o modelo ser **somente texto**. Imagem é opcional e, se existir, é um bônus.
- Precisa chamar ferramentas (*tools*) com confiabilidade, porque o BitClaw é um agente.
- Precisa rodar local, em memória de laptop (alvo: cerca de 4 GB).
- Janela de contexto: o BitClaw usa 16384 tokens hoje. Contexto longo é desejável, mas não é requisito.

## Modelo atual: `nemotron-3-nano:4b-mlx`

Servido pelo Ollama. Cabeçalho do BitClaw: 4,0B parâmetros, `nvfp4`, janela de 16384 tokens.

### Diagnóstico (sessão de 25/11/2026)

**Medido**
- Pedido: `/skills markdown [Crie um markdown sobre a receita de bolo de fubá]`. O BitClaw ficou em "pensando…" por mais de 99 s sem responder, e o processo foi encerrado à mão.
- O `ollama ps` não listava modelo carregado, então esse modelo MLX não parece passar pelo Ollama como esperado. **A investigar.**

**Hipóteses (não confirmadas)**
- O nome da skill (`markdown`) apareceu no pedido, e `with_named_skills` (`src/session.rs`) anexou o corpo inteiro da skill à mensagem, o que aumenta o prompt em 1,5 a 2 mil tokens.
- O modelo raciocina antes de responder, e um prompt maior pode alongar esse raciocínio.
- Parte do tempo pode ser só a velocidade do modelo, sem relação com a skill.
- `/skills markdown [...]` pode ter sido enviado como texto comum, e não como comando do painel de skills.

**Ação já tomada:** `skills/markdown/SKILL.md` foi reduzido de 601 palavras para 14 linhas de regras.

**Como confirmar**
1. Rodar `Crie um arquivo .md com a receita de bolo de fubá`, sem a palavra "markdown", e anotar o tempo.
2. Rodar o mesmo pedido com a palavra "markdown" e a skill enxuta, e comparar.
3. Rodar sem a palavra de novo, para medir a variação entre execuções.
4. Conferir com `ollama ps` e `ollama list` onde o modelo está sendo servido.

### Observação (sessão de 26/11/2026)

**Medido**
- Pedido para listar os arquivos do projeto: o modelo chamou a ferramenta de script, o BitClaw executou `list_dir.sh` depois da aprovação e a saída voltou inteira para o modelo. Gastou 3045 tokens de entrada e 25 de saída no turno.
- A resposta final disse que o script exibiu "a lista completa de diretórios e arquivos". Na tela, a saída apareceu cortada em 20 linhas (`… mais 4 linhas`), então o modelo viu mais do que o usuário.

**A testar de novo**
- Os testes de lentidão de cima, que continuam abertos.
- Se ele segue pedindo ferramentas certas em tarefas de várias etapas (ler, editar e conferir), medindo quantas chamadas erram o nome ou o caminho.
- Se ele respeita a aprovação em lote (`a`): tarefas com várias edições seguidas.
- Repetir a listagem de arquivos com a saída expansível na tela, para comparar o que o usuário vê com o que o modelo diz.

## Candidato: `NVIDIA-Nemotron-3.5-Bolt-E4B-4GB`

Status: **não testado.**

Repositório: `nvidia/NVIDIA-Nemotron-3.5-Bolt-E4B-4GB` (Hugging Face), com a versão quantizada Q4_K_M em GGUF.

### O que as fontes dizem

Fontes: [relatório técnico](https://research.nvidia.com/labs/nemotron/files/NVIDIA-Nemotron-3.5-Bolt-E4B-4GB-Technical-Report.pdf), [página da NVIDIA](https://research.nvidia.com/labs/nemotron/Nemotron-Nemotron-3.5-Bolt-E4B-4GB/), [tabela de benchmarks](https://research.nvidia.com/labs/nemotron/Nemotron-Nemotron-3.5-Bolt-E4B-4GB/). Fontes secundárias: [tilnote.io](https://tilnote.io/pages/6adc895d7244e77059d7ea38), [buildmvpfast.com](https://www.buildmvpfast.com/blog/nvidia-nemotron-3-5-super-vl-bolt-open-models-2026).

| Item | O que consta |
| :--- | :--- |
| Lançamento | 23 de outubro de 2026. |
| Finalidade | Modelo pequeno e aberto para agentes conversacionais, com meta de cerca de 4 GB de memória. |
| Base | Gemma 4 E4B com pós-treino Nemotron. |
| Tamanho | Cerca de 8B de parâmetros totais e 4B efetivos (fonte secundária). |
| Modalidade | **Somente texto.** Sem imagem nem vídeo. Atende ao critério acima. |
| Licença | OpenMDW-1.1 sobre Apache 2.0 do Gemma 4 (fonte secundária). |
| Extra | Drafter DFlash para decodificação especulativa. |

Benchmarks divulgados pela própria NVIDIA:

| Teste | Bolt E4B | Observação |
| :--- | :---: | :--- |
| RoleMRC | 80,6 | Acima de Nemotron 3 Nano 4B, Qwen 3.5 4B e Gemma 4 E4B. |
| When2Call-MT | 63,4 | Mede quando chamar ferramenta. Relevante para o BitClaw. |
| LongBench v2 | 50,5 | Abaixo do Qwen 3.5 4B (59,9). Contexto longo é ponto fraco. |

Os números são da NVIDIA, não foram reproduzidos por nós.

### O que falta saber

- Se existe build compatível com o backend do BitClaw (hoje o Ollama): o nome no Ollama, ou um GGUF importável, ou MLX.
- Se o `tool_calls` do Ollama funciona com ele sem ajuste de template.
- Se o raciocínio do modelo (*thinking*) pode ser desligado ou limitado, já que o tempo de resposta foi o problema do modelo atual.

### Plano de teste

Rodar `scripts/eval.sh` com o modelo atual e com o candidato, no mesmo pedido, e registrar:

| Métrica | Atual (`nemotron-3-nano:4b-mlx`) | Candidato (`Bolt-E4B-4GB`) |
| :--- | :--- | :--- |
| Tempo até a primeira resposta | | |
| Tempo total do turno | | |
| Memória usada (pico) | | |
| Chama a ferramenta certa, com argumentos válidos | | |
| Segue a skill `markdown` | | |
| Estoura a janela de 16384 tokens | | |

Pedidos de teste:
1. `Crie um arquivo .md com a receita de bolo de fubá` (escrita simples, sem skill nomeada).
2. O mesmo pedido citando a skill `markdown`.
3. Uma tarefa que exija ler dois arquivos e editar um (ferramentas encadeadas).

Preencher a tabela antes de decidir a troca.

## Outros candidatos

Acrescente aqui, com status e fontes, antes de testar.

- https://ollama.com/library/nemotron-3.5-bolt

## FunctionGemma (`functiongemma`, Ollama)

Status: **testado, reprovado na integração atual.** Não é modelo principal.

Fontes: [visão geral](https://gemma.ai/models/functiongemma/), [model card](https://gemma.ai/models/functiongemma/model-card/), [formato](https://gemma.ai/models/functiongemma/formatting/), [blog do Google](https://blog.google/technology/developers/functiongemma/), [Ollama](https://ollama.com/library/functiongemma).

### O que as fontes dizem

- Versão do Gemma 3 270M ajustada para chamada de função. Base para treinar agentes locais que transformam linguagem natural em chamadas de API. Não é feito para diálogo direto.
- Treinado para chamadas únicas e paralelas. Não há indicação de treino explícito para encadear várias etapas.
- Usa tokens de controle próprios (`<start_function_call>`, `<end_function_call>` etc.) e declarações de ferramenta em um turno `developer`.
- Corte de dados: agosto de 2024. Avaliação em inglês. Janela de 32K.
- Benchmarks (BFCL) são do próprio Google e não foram verificados por nós.

### Diagnóstico (sessão de 25/11/2026)

**Medido**, com Ollama 0.40.18 e `functiongemma` (268M, Q8_0):

| Teste | Resultado |
| :--- | :--- |
| `scripts/eval.sh`, 1 execução por tarefa | 1/5. Passou só `nao_alterar`, que passa sem fazer nada. Falhas: ferramenta de nome vazio, recusa em texto ("não posso ajudar com programação"), 79 s sem resultado. |
| API de chat, prompt mínimo no formato oficial | Sem chamada válida. Loop de `model` e nome de arquivo inventado. |
| API de chat, prompt do BitClaw com 7 ferramentas | 0 a 1 chamada válida, com argumentos sujos e texto solto. |
| API crua (`/api/generate`, `raw`), formato oficial, temperatura 0 e 0,1 | **Chamada correta** (`create_file{path: hello.txt, content: ola mundo}`), seguida de repetição em loop. As paradas configuradas (`<end_function_call>`, `<end_of_turn>`) não dispararam. |

**Hipótese principal (não verificada):** o modelo gera o formato certo, mas os tokens de controle não saem como tokens especiais. Por isso a parada não ocorre e o parser do Ollama não extrai a chamada. Verificar no GGUF se esses tokens estão marcados como especiais.

**Não medido:** multi-etapas (`read_file` → `edit_file`). O teste foi inválido, porque o modelo recusou no primeiro passo.

### Decisão

Não entra na avaliação padrão. Reabrir só se a integração com o Ollama passar a gerar chamadas limpas (com parada correta) e, depois, rodar `scripts/eval.sh` de novo.

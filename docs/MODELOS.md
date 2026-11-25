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

## Outros candidatos

Nenhum registrado ainda. Acrescente aqui, com status e fontes, antes de testar.

- https://ollama.com/library/nemotron-3.5-bolt

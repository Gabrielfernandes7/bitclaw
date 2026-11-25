# Escolha: política para o histórico que excede o contexto

Status: **aguardando decisão.** Nada foi implementado. Fase 3 de `PROJETO.md`.

## O problema

O loop em `agent.rs` acrescenta a `messages` todas as mensagens de todos os turnos: tarefas do usuário, respostas do modelo e resultados de ferramenta. Nada é removido. Ao mesmo tempo, `num_ctx` está em 8192 tokens, e um `read_file` pode trazer até 8 KB (cerca de 2 a 3 mil tokens em português).

Consequência: depois de poucos turnos com leituras, a conversa excede a janela. Ainda não medimos o que o Ollama faz nesse caso (se trunca em silêncio, se retorna erro ou se degrada a resposta). Essa medição faz parte da Fase 3.

## Requisitos (valem para qualquer opção)

1. O prompt do sistema nunca sai do histórico.
2. A tarefa atual nunca sai do histórico.
3. Uma chamada de ferramenta e o seu resultado saem juntos. Um resultado sem a chamada que o originou confunde o modelo.
4. Quando algo é descartado, o programa avisa no terminal. O agente não pode perder contexto em silêncio.
5. Testável com o modelo falso, sem rede.

## Opções

### A. Descarte de turnos inteiros por orçamento de caracteres

Antes de cada chamada ao modelo, se o histórico passar do orçamento, remove os turnos mais antigos, um turno inteiro por vez, até caber. O orçamento é uma estimativa: cerca de 4 caracteres por token.

- **Prós:** simples, determinístico, fácil de testar. Respeita o requisito 3 por construção.
- **Contras:** o modelo esquece tarefas antigas. Se o usuário referir algo de três turnos atrás, o agente não sabe.

### B. Janela fixa de N turnos

Mantém os últimos N turnos, independentemente do tamanho.

- **Prós:** a mais simples de todas.
- **Contras:** ignora o tamanho real. Um único `read_file` grande pode estourar a janela mesmo com N pequeno. Não resolve o problema que motivou a Fase 3.

### C. Resumo do histórico pelo modelo

Quando o histórico passa do orçamento, o próprio modelo resume os turnos antigos, e o resumo substitui esses turnos.

- **Prós:** mantém informação que A e B descartam.
- **Contras:**
  - uma chamada extra ao modelo a cada compactação, com custo de latência;
  - o resumo pode inventar fatos, e o agente passa a agir sobre um resumo falso sem que ninguém perceba;
  - o comportamento depende do modelo, o que dificulta a avaliação da Fase 5.
  - Testável só com modelo falso, e o teste não prova a qualidade do resumo.

### D. Sem política, só aviso

Não descarta nada. Quando passar do orçamento, avisa o usuário para começar uma conversa nova.

- **Prós:** nenhum risco de perda silenciosa; zero código novo além do aviso.
- **Contras:** não resolve o problema. O agente deixa de servir para sessões longas.

## Recomendação

**A**, com orçamento configurável e aviso no terminal.

Motivos:
- É a única opção que cumpre todos os requisitos sem depender do modelo.
- Mantém o comportamento previsível, o que a Fase 5 precisa para comparar modelos.
- C pode ser reavaliado depois, só se a avaliação mostrar que A perde informação em tarefas de várias etapas. Hoje não temos esse dado.

Valor inicial sugerido para o orçamento: 20.000 caracteres (cerca de 5 mil tokens), deixando espaço para o prompt do sistema, o esquema das ferramentas e a resposta do modelo dentro dos 8192. É um ponto de partida; deve ser ajustado com medição.

## Revisão após análise externa

Recomendação revisada: **A**, com os ajustes abaixo. Concordo com a direção e com a separação entre gerenciamento de contexto e memória, mas corrijo três pontos.

### Ajustes aceitos

1. **Reservar espaço para a resposta e para o que é fixo.** O orçamento do histórico é o que sobra de `num_ctx`:
   `orçamento = (num_ctx − reserva_de_saída − fixo) × chars_por_token`
   - `num_ctx` = 8192 tokens.
   - `reserva_de_saída` = 1024 tokens (valor inicial, ajustável).
   - `fixo` = prompt do sistema + esquemas das ferramentas. O prompt tem 504 caracteres; os esquemas precisam ser medidos a partir do registro.
   - `chars_por_token` = 3, de forma conservadora (o português e o código variam bastante).
   Com esses valores, o orçamento fica em torno de **19.000 caracteres**, não 20.000. O número 20.000 foi um chute, e a conta acima o substitui.
2. **Calibrar com a própria resposta do Ollama.** A resposta de `/api/chat` traz `prompt_eval_count`, que é o número real de tokens de entrada. Usar essa métrica para ajustar `chars_por_token` depois de cada chamada é mais confiável do que um tokenizador externo, que o Ollama não expõe de forma uniforme entre modelos.
3. **O turno atual é protegido, mas precisa caber.** Se o turno atual sozinho exceder o orçamento, o agente falha com erro explícito de contexto. Vale notar que isso pode acontecer cedo: duas leituras de 8 KB já somam mais de 16.000 caracteres. Se isso ocorrer na prática, a correção é reduzir o limite de `read_file` ou o número de passos por turno, não descartar parte do turno.

### Ajustes que não aceito como estão

- **Estrutura de diretórios com `context/`, `providers/` e quatro arquivos.** Hoje há uma única política. Um módulo `context.rs` com um `ContextManager` e uma função de ajuste é suficiente. Só separar em `policy` quando existir uma segunda política; até lá é abstração sem uso.
- **Menções a `write_file`, llama.cpp e outros provedores.** O projeto não tem essas ferramentas nem esses provedores. Não entram no escopo desta decisão.

### Pontos mantidos da análise

- Preservar sempre o prompt do sistema e o turno atual.
- Um turno é a unidade de descarte: a mensagem do usuário, as respostas do modelo e os resultados de ferramenta daquele turno saem juntos.
- Aviso no terminal sempre que algo for descartado. Exemplo: `[contexto] descartados 2 turnos antigos; tokens estimados: antes 6840, depois 4120`.
- Sem sumarização automática e sem Memory Core nesta fase.

## Decisão

- [ ] A — descarte de turnos inteiros por orçamento (recomendada)
- [ ] B — janela fixa de N turnos
- [ ] C — resumo pelo modelo
- [ ] D — sem política, só aviso

Orçamento em caracteres: `__________` (sugerido: 20000)

Observações:


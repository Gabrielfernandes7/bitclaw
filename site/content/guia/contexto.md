---
description: Como o BitClaw mantém o histórico dentro da janela do modelo, sem perder o prompt do sistema.
---
# Contexto

Modelos pequenos têm janelas pequenas, então o histórico da conversa tem **orçamento**. O padrão é `context_window = 16384` tokens, com 1024 reservados para a resposta.

## O que acontece quando o histórico excede a janela

O programa age em duas etapas, e cada ajuste aparece no terminal:

1. **Troca resultados antigos de ferramenta por um marcador curto.** Os já superados por uma leitura ou edição posterior do mesmo caminho vão primeiro.
2. **Se ainda não bastar, descarta os turnos mais antigos inteiros.**

O **prompt do sistema e o turno atual nunca são descartados**. Se o próprio turno atual não couber, a tarefa é recusada com erro, em vez de seguir truncada.

## Orçamento em caracteres, calibrado

O orçamento é calculado como `(janela − saída reservada) × caracteres por token − esquemas das ferramentas`. Começa em 3 caracteres por token e é **calibrado** com o `prompt_eval_count` que o Ollama devolve. A decisão e as alternativas descartadas estão em [Escolha: histórico](../projeto/escolha-contexto.md).

## Outras defesas de contexto

- `read_file` limita a leitura a 8 KB e tem `mode: "outline"` e `start_line`/`end_line`.
- `grep_files` corta em 40 linhas (6 por arquivo).
- O resultado de `run_script` e `run_command` é limitado a 6 KB.
- Leituras idênticas não rodam duas vezes no mesmo turno: o modelo recebe um aviso e, se insistir, uma última rodada sem ferramentas para responder.
- O raciocínio (*thinking*) de turnos anteriores sai do histórico.

## Ajustar na sessão

| Comando | Efeito |
|---|---|
| `/context` | Mostra a janela e explica o que muda ao aumentá-la: memória, recarga do modelo, velocidade, tokens gastos e qualidade. |
| `/context 32k` | Troca o orçamento (`16384`, `16k`…). |
| `/context reset` | Volta ao valor da configuração (`model.context_window`). |

Para ver quanto cada ferramenta devolveu ao modelo em cada turno, rode com `BITCLAW_STATS=1`.

> Com o OpenRouter, `/context` só muda o orçamento do histórico: a janela real é a do modelo.

## Memória

Uma medição: `nemotron-3-nano:4b-mlx` com `num_ctx` 8192 ocupa **5,2 GB** (100% GPU) num Mac de 16 GB, com o modelo carregado e sem histórico cheio. O valor padrão atual (16384) ainda não foi medido na memória; veja [Documento de projeto](../projeto/documento.md).

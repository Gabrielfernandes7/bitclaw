---
description: Por que o BitClaw existe, a régua que mede o projeto e o que ele recusa ser.
---
# Filosofia

BitClaw é um **Agent Core** experimental, modular e extensível: uma fundação para construir agentes de IA sem reimplementar a infraestrutura básica. Ele não é um agente especializado, nem um clone do OpenClaw.

O nome e o mascote contam a ideia. O BitClaw é um caranguejo **feito de peças de montar**, e o software segue o mesmo princípio: cada parte (modelo, ferramenta, interface) é uma peça que se encaixa e se troca sem desmontar o resto.

<figure class="fig">
  <img src="@img/bitclaw-bricks.svg" alt="O BitClaw, em pixels, encaixando peças coloridas">
  <figcaption>As peças coloridas são o que encaixa no corpo amarelo: um modelo, uma ferramenta, um agente novo.</figcaption>
</figure>

## A régua

> Alguém consegue construir um novo agente usando só a API pública do Core, sem modificar o Core.

Esse critério é a métrica principal. Quantidade de funcionalidades não mede sucesso. Para avaliar qualquer mudança, o projeto faz duas perguntas:

1. Isso pertence ao Core ou a um agente específico?
2. Isso facilita construir o próximo agente sem editar o Core?

Projetos previstos sobre o core: DevClaw, ResearchClaw e BusinessClaw. Outras pessoas também devem poder construir agentes sobre ele.

## Regras de fronteira

- **Código específico de agente nunca entra no Core**, mesmo que seja conveniente.
- **O Core não depende de um modelo específico.** Modelos entram por implementação de uma trait (`ChatModel`), nunca por código fixo.
- **Ferramentas são extensões.** Adicionar uma ferramenta não exige editar o laço do agente.
- **Não há dependência de Ollama no laço.** O Ollama é um adaptador, como qualquer outro provedor.
- **Cada funcionalidade nova tem teste automatizado** que não depende de rede nem de modelo real.

Na arquitetura isso vira uma regra de dependência: o core não conhece ninguém; quem conhece o core são os adaptadores, o toolkit e o app. Veja [Arquitetura do código](../dev/arquitetura.md).

## Princípios de produto

**Local primeiro.** O padrão é um modelo pequeno rodando na sua máquina, via Ollama. Com um provedor remoto (OpenRouter), o BitClaw avisa que o conteúdo lido sai da máquina.

**Você aprova, o agente propõe.** Edições, criações e remoções mostram uma prévia e esperam sua decisão. Scripts sempre pedem confirmação. Cada decisão deixa um recibo em `.bitclaw/approvals.jsonl`, e `/undo` desfaz o último turno com alterações.

**Honestidade antes de simpatia.** Ao fim de cada tarefa, o BitClaw lista as ações que de fato aconteceram (`✓`, `✗`, `⊘`). Se o texto do modelo diz que criou, alterou ou removeu algo que as ações não confirmam, o programa avisa.

**Pequeno não é frágil.** O BitClaw é pensado para modelos de poucos bilhões de parâmetros: o contexto tem orçamento, as buscas cortam a saída, um detector de laço interrompe repetição e chamadas de ferramenta escritas como texto são reconhecidas.

**Verifique no disco, não no texto.** A avaliação com modelos reais confere o resultado nos arquivos, não no que o modelo afirma.

## O que fica fora, por enquanto

Cada item só entra quando a sua dependência existir.

| Item | Depende de |
|---|---|
| Memória persistente | Gestão de contexto funcionando |
| Dreaming (consolidação de conhecimento) | Memória persistente e uma definição de contrato ainda inexistente |
| Orquestração de modelos heterogêneos | Um único modelo validado no ciclo básico |
| Interface gráfica ou web | Core estável e API pública documentada |

Skills já existem; entraram quando a trait `Tool`, a configuração de agente como dado e um segundo agente validaram o formato.

## Para ir além

- [Documento de projeto](../projeto/documento.md): propósito, plano em fases e decisões.
- [Arquitetura alvo](../projeto/arquitetura-alvo.md): o princípio de dependência em detalhe.
- [Modelos e diagnósticos](../projeto/modelos.md): como o modelo padrão foi escolhido.

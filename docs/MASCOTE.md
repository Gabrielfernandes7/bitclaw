# Mascote do BitClaw: versões e variações

O mascote é um caranguejo-tijolo desenhado num mapa de pixels e mostrado no terminal com meios-blocos
(`█ ▀ ▄`): cada célula do terminal junta dois pixels, o de cima e o de baixo. O código fica em
`src/app/tui/brand.rs` (`base_map`, `frame`). As versões SVG ficam em `docs/identidade/`.

Os quadros da seção 1 e 2 foram gerados pelo próprio `brand::frame` e são exatamente o que o terminal mostra.
As variações da seção 3 são propostas: não estão no código.

Legenda dos mapas: `Y` corpo, `T` pino, `S` pé (preenchidos); `E` olho (buraco); `.` vazio.

## 1. Versão atual (cabeçalho)

10 colunas × 3 linhas, mapa 10×6. A grade vem de `assets/bitclaw-crab.svg` (`base_map`, lido uma vez):
cada retângulo preenchido é um pixel, e um buraco no SVG é um olho. Os pinos e os pés são nomes sobre
esses mesmos pixels, que os quadros movem por posição.

```text
██▄█▄▄█▄██
██▄████▄██
 ▄▀█▀▀█▀▄
```

```text
YY.T..T.YY
YYYYYYYYYY
YY.YYYY.YY
YYYYYYYYYY
..YYYYYY..
.S.S..S.S.
```

Os olhos são pixels de uma linha só, então cada um cai na metade de cima de uma célula e aparece como meio
buraco. A base também fica mais fina, porque a última linha da grade ocupa só meia célula.

## 2. Quadros da animação

Abertura (sorteada) e cliques mostram estes números. Cada um dura `TRICK_MS` (2,2 s).

| Quadro | Desenho |
|---|---|
| Parado | <pre>██▄█▄▄█▄██<br>██▄████▄██<br> █▀█▀▀█▀█ </pre> |
| Piscando | <pre>██▄█▄▄█▄██<br>██████████<br> █▀█▀▀█▀█ </pre> |
| Olhando à esquerda | <pre>██▄█▄▄█▄██<br>█▄████▄███<br> █▀█▀▀█▀█ </pre> |
| Olhando à direita | <pre>██▄█▄▄█▄██<br>███▄████▄█<br> █▀█▀▀█▀█ </pre> |
| Feliz: estalo | <pre>▄███▄▄███▄<br>██████████<br> █▀█▀▀█▀█ </pre> |
| Feliz: pinças abertas | <pre>██▄█▄▄█▄██<br>██████████<br>▄▀█▀▀▀▀█▀▄</pre> |
| Batucando: esquerda | <pre>▄▄▄█▄▄█▄██<br>██▄████▄██<br> █▀█▀▀█▀█ </pre> |
| Batucando: direita | <pre>██▄█▄▄█▄▄▄<br>██▄████▄██<br>▄▀█▀▀▀▀█▀▄</pre> |
| Pensando `00` | <pre>██▄▄▄▄▄▄██<br>█▄████▄███<br> █▀█▀▀█▀█ </pre> |
| Pensando `01` | <pre>██▄▄▄▄█▄██<br>█▄████▄███<br>▄▀█▀▀▀▀█▀▄</pre> |
| Pensando `10` | <pre>██▄█▄▄▄▄██<br>█▄████▄███<br> █▀█▀▀█▀█ </pre> |
| Pensando `11` | <pre>██▄█▄▄█▄██<br>███▄████▄█<br>▄▀█▀▀▀▀█▀▄</pre> |

Ao pensar, os dois pinos do topo contam em binário (`00`, `01`, `10`, `11`).

## 3. Variações propostas

### 3.1 Versão anterior (pés suspensos)

Era a do cabeçalho até a troca pela atual. O corpo afina (`▀`) e os pés ficam na metade de baixo (`▄`),
o que deixa metade da última linha vazia e dá a impressão de que o mascote foi cortado embaixo.

```text
██▄█▄▄█▄██
██▄████▄██
 ▄▀█▀▀█▀▄
```

Mapa: igual ao atual, com a linha 5 `..YYYYYY..` no lugar de `.YYYYYYYY.`.

### 3.2 Boas-vindas: o mesmo mascote, ampliado, com as peças

A tela de boas-vindas usa o mascote do cabeçalho na escala 2 (20 colunas × 6 linhas,
`brand::crab_scaled`), cercado pelas peças coloridas de `assets/bitclaw-bricks.svg`, com as mesmas cores.
A vermelha fica junto da pinça, sendo encaixada, como na referência. Num terminal comum (escala 1) é o
mascote do cabeçalho com peças de uma linha.

Uma versão anterior usava um mascote grande feito do mestre da identidade (15 × 11, pinças erguidas). Saiu
porque ficava "monstruosa" ao lado do mascote do cabeçalho: era outro bicho, não o mesmo maior.

Para fundos claros: `docs/identidade/bitclaw-ambar.svg`, o mesmo mapa em âmbar `#D9A400`.

### 3.3 Compacto

10 colunas × 2 linhas, mapa 10×4. Para cabeçalhos de uma ou duas linhas de texto.

```text
██ █▄▄█ ██
▀█▀█▀▀█▀█▀
```

```text
YY.T..T.YY
YYEYYYYEYY
YYYYYYYYYY
.S.S..S.S.
```

### 3.4 Só ASCII

Para terminais e fontes sem os caracteres de bloco, ou logs que passam por ferramentas que só aceitam ASCII.

```text
[]_||__||_[]
 |  o  o  |
 /_/ \/ \_\
```

## 4. Por que o mascote pode parecer cortado

Em testes com o `TestBackend` do ratatui (80×24, 100×30, 120×40, 40×12, 30×10 e 24×8), as três linhas e
as dez colunas aparecem inteiras e o texto não invade o mascote. As larguras já são contadas em colunas
(`Span::width`), não em bytes. Abaixo de 24 colunas o mascote some de propósito (`mascot_rect`).

O que pode dar a impressão de corte:

- **O desenho da terceira linha** (já corrigido). Na versão anterior (3.1), corpo em `▀` e pés em `▄`
  deixavam metade da linha vazia; parecia faltar um pedaço embaixo. A versão atual usa blocos inteiros nas pernas.
- **Espaçamento de linha da fonte.** Com espaçamento de linha acima de 1,0 (Terminal.app, VS Code) ou
  fontes que não desenham os blocos ocupando a célula inteira, aparecem faixas entre as linhas do mascote.
  Isso vem do terminal, não do BitClaw.
- **Rolagem.** O cabeçalho é a primeira linha da conversa e rola com ela; com a conversa maior que a tela,
  só a parte de baixo do mascote fica visível no topo.

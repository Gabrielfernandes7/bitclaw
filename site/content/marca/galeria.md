---
description: O caranguejo de peças de montar do BitClaw: ilustrações aprovadas, paleta e regras de uso.
---
# Galeria

O BitClaw é um **caranguejo formado por brinquedo de montar amarelo**. Olho é buraco, não pintura; os dois pinos no topo do corpo são a assinatura: sem eles é "um caranguejo", com eles é o BitClaw.

## O mascote, de peças

<figure class="fig dark">
  <img src="@img/bricks-cena.svg" alt="O caranguejo de peças amarelas encaixando uma peça vermelha na pinça">
  <figcaption>Cena de abertura. A peça vermelha vem de <code>bricks-cena.svg</code>.</figcaption>
</figure>

<figure class="fig dark">
  <img src="@img/bricks-feliz.svg" alt="Variações do mascote: feliz, pinças abertas e piscando">
  <figcaption>Expressões: parado, piscando, feliz, pinças abertas. <code>bricks-feliz.svg</code>.</figcaption>
</figure>

<figure class="fig dark">
  <img src="@img/bricks-pecas.svg" alt="As peças de montar do mascote, uma a uma">
  <figcaption>As peças, uma a uma. <code>bricks-pecas.svg</code>.</figcaption>
</figure>

## Em pixels: o mapa do terminal

O mascote do terminal vem de um mapa de 10 × 6 pixels (`assets/bitclaw-crab.svg`), que a TUI lê uma vez. Cada retângulo preenchido é um pixel; um buraco é um olho.

<div class="cols">
  <figure class="fig dark">
    <img src="@img/bitclaw-bricks.svg" alt="O mascote em pixels ao lado de peças coloridas">
    <figcaption>Mascote e peças. <code>bitclaw-bricks.svg</code>.</figcaption>
  </figure>
  <figure class="fig dark">
    <img src="@img/bitclaw-bricks-dark.svg" alt="Versão do mascote de peças para fundo escuro">
    <figcaption>Versão para fundo escuro. <code>bitclaw-bricks-dark.svg</code>.</figcaption>
  </figure>
  <figure class="fig dark">
    <img src="@img/bitclaw-crab.svg" alt="O mapa de pixels do caranguejo" width="260">
    <figcaption>A fonte única da arte. <code>bitclaw-crab.svg</code>.</figcaption>
  </figure>
  <figure class="fig">
    <img src="@img/bitclaw-ambar.svg" alt="O mascote em âmbar, para fundos claros" width="260">
    <figcaption>Âmbar <code>#D9A400</code>, para fundos claros. <code>bitclaw-ambar.svg</code>.</figcaption>
  </figure>
</div>

No terminal, o mascote se move: pisca, olha para os lados, estala as pinças, bate com elas e, enquanto o modelo pensa, os dois pinos do topo contam em binário (`00`, `01`, `10`, `11`). Os quadros estão em [O mascote](mascote.md).

## Paleta

<div class="swatches">
  <div class="sw"><i style="background:#FFCC00"></i><span><b>Amarelo</b><code>#FFCC00</code></span></div>
  <div class="sw"><i style="background:#FFE477"></i><span><b>Topo</b><code>#FFE477</code></span></div>
  <div class="sw"><i style="background:#D9AB00"></i><span><b>Lateral</b><code>#D9AB00</code></span></div>
  <div class="sw"><i style="background:#171717"></i><span><b>Tinta</b><code>#171717</code></span></div>
  <div class="sw"><i style="background:#30343B"></i><span><b>Grafite</b><code>#30343B</code></span></div>
  <div class="sw"><i style="background:#F5F3EB"></i><span><b>Papel</b><code>#F5F3EB</code></span></div>
  <div class="sw"><i style="background:#F0453A"></i><span><b>Vermelha</b><code>#F0453A</code></span></div>
  <div class="sw"><i style="background:#2C7CF0"></i><span><b>Azul</b><code>#2C7CF0</code></span></div>
  <div class="sw"><i style="background:#46B832"></i><span><b>Verde</b><code>#46B832</code></span></div>
</div>

As peças coloridas (vermelha, azul, verde e roxa) são os outros agentes, módulos e ferramentas que encaixam no corpo amarelo.

## Regras que valem

1. **Uma cor por desenho** na arte de terminal; a profundidade vem dos pinos e do contorno das peças, não de gradiente.
2. **Olho é buraco.** O fundo do terminal aparece em qualquer tema.
3. **Os dois pinos do topo são a assinatura.**
4. **Amarelo `#FFCC00` em fundo claro tem pouco contraste.** Em tema claro, use o âmbar `#D9A400`.
5. **Só ASCII não carrega a marca.** Fica legível, mas sem a silhueta.

O estudo completo, com as técnicas comparadas, está em [Arte de terminal](arte-terminal.md).

## Baixar

Todos os SVGs acima estão no repositório, em [`assets/`](../../../assets) e [`docs/identidade/`](../../../docs/identidade). Versões em PNG, para redes sociais e favicon:

- [Imagem de compartilhamento (1200 × 630)](@img/og-image.png)
- [Cena de abertura (PNG)](@img/png/bricks-cena.png)
- [Expressões (PNG)](@img/png/bricks-feliz.png)
- [Peças (PNG)](@img/png/bricks-pecas.png)

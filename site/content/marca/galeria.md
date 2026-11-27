---
description: O caranguejo de peças de montar do BitClaw: a arte de assets/, a paleta e as regras de uso.
---
# Galeria

O BitClaw é um **caranguejo formado por brinquedo de montar amarelo**, desenhado em pixels. Olho é buraco, não pintura; os dois pinos no topo do corpo são a assinatura: sem eles é "um caranguejo", com eles é o BitClaw.

## O mascote e as peças

<figure class="fig">
  <img src="@img/bitclaw-bricks.svg" alt="O BitClaw, em pixels, encaixando peças coloridas">
  <figcaption>O mascote encaixando a peça vermelha, com as peças azul, roxa e verde esperando. <code>assets/bitclaw-bricks.svg</code>.</figcaption>
</figure>

## O mapa de pixels

O mascote do terminal e o do site vêm do mesmo mapa de 10 × 6 pixels, `assets/bitclaw-crab.svg`: cada retângulo preenchido é um pixel, e um buraco é um olho. É a fonte única da arte.

<div class="cols">
  <figure class="fig dark">
    <img src="@img/bitclaw-crab.svg" alt="O mapa de pixels do caranguejo" width="260">
    <figcaption>O caranguejo. <code>assets/bitclaw-crab.svg</code>.</figcaption>
  </figure>
  <figure class="fig dark">
    <img src="@img/lua.svg" alt="Lua crescente em pixels" width="190">
    <figcaption>A lua da tela de boas-vindas. <code>assets/lua.svg</code>.</figcaption>
  </figure>
</div>

No terminal, o mascote se move: pisca, olha para os lados, estala as pinças, bate com elas e, enquanto o modelo pensa, os dois pinos do topo contam em binário (`00`, `01`, `10`, `11`). No SOUL, ele aparece feliz segurando um coração que pulsa. Os quadros estão em [O mascote](mascote.md).

## Paleta

<div class="swatches">
  <div class="sw"><i style="background:#FFCC00"></i><span><b>Amarelo</b><code>#FFCC00</code></span></div>
  <div class="sw"><i style="background:#171717"></i><span><b>Tinta</b><code>#171717</code></span></div>
  <div class="sw"><i style="background:#30343B"></i><span><b>Grafite</b><code>#30343B</code></span></div>
  <div class="sw"><i style="background:#F5F3EB"></i><span><b>Papel</b><code>#F5F3EB</code></span></div>
  <div class="sw"><i style="background:#F0453A"></i><span><b>Vermelha</b><code>#F0453A</code></span></div>
  <div class="sw"><i style="background:#2C7CF0"></i><span><b>Azul</b><code>#2C7CF0</code></span></div>
  <div class="sw"><i style="background:#46B832"></i><span><b>Verde</b><code>#46B832</code></span></div>
  <div class="sw"><i style="background:#A67BE0"></i><span><b>Roxa</b><code>#A67BE0</code></span></div>
</div>

As peças coloridas (vermelha, azul, verde e roxa) são os outros agentes, módulos e ferramentas que encaixam no corpo amarelo.

## Regras que valem

1. **Uma cor por desenho** na arte de terminal.
2. **Olho é buraco.** O fundo do terminal aparece em qualquer tema.
3. **Os dois pinos do topo são a assinatura.**
4. **Amarelo `#FFCC00` em fundo claro tem pouco contraste.** Em tema claro, use o âmbar `#D9A400`.
5. **Só ASCII não carrega a marca.** Fica legível, mas sem a silhueta.

O estudo completo, com as técnicas comparadas, está em [Arte de terminal](arte-terminal.md).

## Baixar

Os SVGs estão no repositório, em [`assets/`](../../../assets). Em PNG, para redes sociais e favicon:

- [Imagem de compartilhamento (1200 × 630)](@img/og-image.png)
- [Mascote e peças (PNG)](@img/png/bitclaw-bricks.png)
- [Ícone (512 × 512)](@img/icon-512.png)

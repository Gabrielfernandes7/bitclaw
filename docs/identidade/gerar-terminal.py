#!/usr/bin/env python3
"""Gera as variações do mascote de terminal a partir das grades em assets/ (a fonte única).

Uso: python3 docs/identidade/gerar-terminal.py

- bitclaw-grande-ambar.svg: o mascote grande em âmbar, para fundos claros (o amarelo some neles;
  ver ARTE-TERMINAL.md).
- cena-noturna.svg: a cena da tela de boas-vindas, para o README. Cada pixel do mascote é um
  tijolo, com um vão entre as fileiras, como no terminal com espaçamento de linha maior que 1.
"""
import re
from pathlib import Path

HERE = Path(__file__).parent
ASSETS = HERE.parent.parent / "assets"
PIXEL = 24
YELLOW, AMBER, MOON, SEA, STAR, GROUND, NIGHT = "#FFCC00", "#D9A400", "#E6E2D3", "#2F4F78", "#8A8A8A", "#555555", "#141414"


def grid(name):
    svg = (ASSETS / name).read_text()
    w = int(re.search(r'<svg[^>]* width="(\d+)"', svg).group(1)) // PIXEL
    h = int(re.search(r'<svg[^>]* height="(\d+)"', svg).group(1)) // PIXEL
    g = [[False] * w for _ in range(h)]
    for x, y in re.findall(r'<rect x="(\d+)" y="(\d+)"', svg):
        g[int(y) // PIXEL][int(x) // PIXEL] = True
    return g


def rects(g, x0, y0, px, color, gap=0):
    return "".join(
        f'<rect x="{x0 + x * px}" y="{y0 + y * px}" width="{px}" height="{px - gap}" fill="{color}"/>'
        for y, row in enumerate(g) for x, on in enumerate(row) if on
    )


def svg(w, h, title, body, bg=None):
    back = f'<rect width="{w}" height="{h}" fill="{bg}"/>' if bg else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" '
            f'shape-rendering="crispEdges">\n  <title>{title}</title>\n  {back}{body}\n</svg>\n')


crab, moon = grid("bitclaw-crab-grande.svg"), grid("lua.svg")
cw, ch = len(crab[0]), len(crab)
(HERE / "bitclaw-grande-ambar.svg").write_text(svg(cw * PIXEL, ch * PIXEL, "BitClaw grande, âmbar", rects(crab, 0, 0, PIXEL, AMBER)))

# Cena: 72 colunas de 12 px (um pixel do mascote na escala 2 = 2 colunas = 24 px).
W, H, C = 864, 440, 12
stars = [(4, 1), (17, 3), (30, 0), (42, 2), (56, 1), (69, 4), (94, 2), (11, 5), (52, 4), (89, 6), (37, 6)]
body = ""
for fx, y in stars:
    x, yy = fx * 72 // 100 * C, 24 + y * 24
    s = 3 if (fx + y) % 3 == 0 else 5
    body += f'<rect x="{x}" y="{yy}" width="{s}" height="{s}" fill="{STAR}"/>'
body += rects(moon, W - 14 * C, 24, 12, MOON)
crab_x, crab_y = 9 * C, 6 * 24
sea_y = crab_y + ch * PIXEL // 2
body += "".join(f'<rect x="{x}" y="{sea_y}" width="8" height="3" fill="{SEA}"/>' for x in range(0, W, 2 * C))
body += rects(crab, crab_x, crab_y, PIXEL, YELLOW, gap=4)
body += "".join(f'<rect x="{x}" y="{crab_y + ch * PIXEL + 16}" width="3" height="3" fill="{GROUND}"/>' for x in range(0, W, C))
(HERE / "cena-noturna.svg").write_text(svg(W, H, "BitClaw: cena noturna", body, NIGHT))
print("ok: bitclaw-grande-ambar.svg, cena-noturna.svg")

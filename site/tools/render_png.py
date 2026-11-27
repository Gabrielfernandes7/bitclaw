#!/usr/bin/env python3
"""Gera os PNGs do site a partir dos SVGs de `assets/`. Só macOS (usa `qlmanage` e `sips`).

Saída (commitada, para o build do Pages não depender de macOS):
  site/static/img/og-image.png        1200x630, imagem de compartilhamento
  site/static/img/favicon-32.png      32x32
  site/static/img/apple-touch-icon.png 180x180
  site/static/img/icon-512.png        512x512
  site/static/img/png/bitclaw-bricks.png  assets/bitclaw-bricks.svg em 2x

O `qlmanage` só gera miniaturas quadradas; por isso cada SVG é embrulhado num SVG
quadrado e depois cortado com `sips` na proporção original.
"""
from __future__ import annotations

import re
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
OUT = ROOT / "site" / "static" / "img"
CRAB = ROOT / "assets" / "bitclaw-crab.svg"


def inner(svg_path: Path) -> tuple[float, float, str]:
    """(largura, altura, conteúdo) do SVG, sem a tag externa."""
    s = svg_path.read_text()
    vb = re.search(r'viewBox="([\d.\s-]+)"', s)
    _, _, w, h = (float(x) for x in vb.group(1).split())
    body = s[s.index(">", s.index("<svg")) + 1: s.rindex("</svg>")]
    return w, h, body


def run(*cmd: str) -> None:
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def render(svg_markup: str, width: int, height: int, dest: Path, scale_to: int | None = None) -> None:
    """Rasteriza um SVG `width`x`height` centrado num quadrado e corta de volta."""
    side = max(width, height)
    scale_to = scale_to or width
    k = scale_to / width
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        px = int(side * k)
        hi = max(px, 1024)
        sq = (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" width="{hi}" height="{hi}">'
            f'<rect width="{side}" height="{side}" fill="#171717"/>'
            f'<svg x="0" y="{(side - height) / 2}" width="{width}" height="{height}" viewBox="0 0 {width} {height}">'
            f"{svg_markup}</svg></svg>"
        )
        src = t / "in.svg"
        src.write_text(sq)
        # O qlmanage usa o tamanho do próprio SVG e deixa uma faixa branca se ele for menor que a miniatura:
        # o SVG já nasce grande (`hi`), é cortado na proporção e só então reduzido ao tamanho pedido.
        hi_h = round(height * hi / side)
        run("qlmanage", "-t", "-s", str(hi), "-o", str(t), str(src))
        png = t / "in.svg.png"
        run("sips", "--cropToHeightWidth", str(hi_h), str(hi), str(png))
        if hi != px:
            run("sips", "-z", str(round(height * k)), str(px), str(png))
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(png, dest)


def crab_runs() -> str:
    """O mascote de `assets/bitclaw-crab.svg` com as células de uma linha fundidas, sem fresta entre elas."""
    s = CRAB.read_text()
    cells = {(int(m[2]) // 24, int(m[1]) // 24) for m in re.finditer(r'x="(\d+)" y="(\d+)" width="24" height="24" fill="#FFCC00"', s)}
    out = []
    for r in range(6):
        c = 0
        while c < 10:
            if (r, c) not in cells:
                c += 1
                continue
            e = c
            while (r, e) in cells:
                e += 1
            out.append(f'<rect x="{c * 24 - 0.5}" y="{r * 24 - 0.5}" width="{(e - c) * 24 + 1}" height="25" fill="#FFCC00"/>')
            c = e
    return "".join(out)


ICON_SIDE = 264


def icon_svg() -> str:
    """O mascote inteiro, centrado num quadrado escuro com folga igual dos quatro lados."""
    ox, oy = (ICON_SIDE - 240) / 2, (ICON_SIDE - 144) / 2
    return (
        f'<rect width="{ICON_SIDE}" height="{ICON_SIDE}" rx="48" fill="#171717"/>'
        f'<g transform="translate({ox} {oy})">{crab_runs()}</g>'
    )


def main() -> None:
    w, h, body = inner(ROOT / "assets" / "bitclaw-bricks.svg")
    render(body, int(w), int(h), OUT / "png" / "bitclaw-bricks.png", scale_to=int(w) * 2)

    font = "Helvetica Neue, Arial, sans-serif"
    panel_w = 600
    scale = panel_w / w
    og = (
        '<rect width="1200" height="630" fill="#171717"/>'
        '<g fill="#FFCC00"><rect x="64" y="70" width="34" height="14" rx="5"/><rect x="108" y="70" width="34" height="14" rx="5"/></g>'
        f'<text x="60" y="270" font-family="{font}" font-weight="900" font-size="132" letter-spacing="-6" fill="#F5F3EB">bit<tspan fill="#FFCC00">claw</tspan></text>'
        f'<text x="64" y="345" font-family="{font}" font-weight="700" font-size="34" fill="#F5F3EB">Um agente de IA local,</text>'
        f'<text x="64" y="390" font-family="{font}" font-weight="700" font-size="34" fill="#FFCC00">montado peça por peça.</text>'
        '<text x="64" y="560" font-family="Menlo, monospace" font-size="24" fill="#A8A399">Rust · Ollama · documentação</text>'
        f'<g transform="translate({1200 - panel_w - 40} {(630 - h * scale) / 2}) scale({scale})"><svg width="{w}" height="{h}" viewBox="0 0 {w:g} {h:g}">{body}</svg></g>'
    )
    render(og, 1200, 630, OUT / "og-image.png")

    for name, size in (("favicon-32.png", 32), ("apple-touch-icon.png", 180), ("icon-512.png", 512)):
        render(icon_svg(), ICON_SIDE, ICON_SIDE, OUT / name, scale_to=size)
    print("PNGs em", OUT)


if __name__ == "__main__":
    main()

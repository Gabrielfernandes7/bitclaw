#!/usr/bin/env python3
"""Gera os PNGs do site a partir dos SVGs aprovados. Só macOS (usa `qlmanage` e `sips`).

Saída (commitada, para o build do Pages não depender de macOS):
  site/static/img/og-image.png        1200x630, imagem de compartilhamento
  site/static/img/favicon-32.png      32x32
  site/static/img/apple-touch-icon.png 180x180
  site/static/img/png/bricks-*.png    as ilustrações, em 2x

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
        sq = (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" width="{side}" height="{side}">'
            f'<rect width="{side}" height="{side}" fill="#171717"/>'
            f'<svg x="0" y="{(side - height) / 2}" width="{width}" height="{height}" viewBox="0 0 {width} {height}">'
            f"{svg_markup}</svg></svg>"
        )
        src = t / "in.svg"
        src.write_text(sq)
        px = int(side * k)
        run("qlmanage", "-t", "-s", str(px), "-o", str(t), str(src))
        png = t / "in.svg.png"
        out_h = int(height * k)
        run("sips", "--cropToHeightWidth", str(out_h), str(px), str(png))
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(png, dest)


def crab_cells() -> str:
    s = CRAB.read_text()
    return "".join(
        f'<rect x="{m[1]}" y="{m[2]}" width="24" height="24" fill="#FFCC00" stroke="#FFCC00" stroke-width="1"/>'
        for m in re.finditer(r'x="(\d+)" y="(\d+)" width="24" height="24" fill="#FFCC00"', s)
    )


def main() -> None:
    # ilustrações aprovadas
    for name in ("bricks-cena", "bricks-feliz", "bricks-pecas"):
        w, h, body = inner(ROOT / "docs" / "identidade" / f"{name}.svg")
        render(body, int(w), int(h), OUT / "png" / f"{name}.png", scale_to=int(w) * 2 if w < 1000 else int(w))

    # og-image: cena à direita, nome e frase à esquerda
    w, h, body = inner(ROOT / "docs" / "identidade" / "bricks-cena.svg")
    og = (
        '<rect width="1200" height="630" fill="#131316"/>'
        '<g transform="translate(470 20) scale(0.8)">'
        f'<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}">{body}</svg></g>'
        '<rect width="560" height="630" fill="#171717"/>'
        '<g fill="#FFCC00"><rect x="64" y="70" width="34" height="14" rx="5"/><rect x="108" y="70" width="34" height="14" rx="5"/></g>'
        '<text x="60" y="270" font-family="Helvetica Neue, Arial, sans-serif" font-weight="900" font-size="132" letter-spacing="-6" fill="#F5F3EB">bit<tspan fill="#FFCC00">claw</tspan></text>'
        '<text x="64" y="345" font-family="Helvetica Neue, Arial, sans-serif" font-weight="700" font-size="34" fill="#F5F3EB">Um agente de IA local,</text>'
        '<text x="64" y="390" font-family="Helvetica Neue, Arial, sans-serif" font-weight="700" font-size="34" fill="#FFCC00">montado peça por peça.</text>'
        '<text x="64" y="560" font-family="Menlo, monospace" font-size="24" fill="#A8A399">Rust · Ollama · documentação</text>'
    )
    render(og, 1200, 630, OUT / "og-image.png")

    # favicons: o caranguejo em pixels (240x144) centrado num quadrado escuro
    icon = f'<rect width="288" height="288" fill="#171717"/><g transform="translate(24 72)">{crab_cells()}</g>'
    for name, size in (("favicon-32.png", 32), ("apple-touch-icon.png", 180)):
        render(icon, 288, 288, OUT / name, scale_to=size)
    print("PNGs em", OUT)


if __name__ == "__main__":
    main()

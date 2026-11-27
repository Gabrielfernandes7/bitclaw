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


MASCOT = ROOT / "docs" / "identidade" / "bitclaw-bricks-dark.svg"
ICON_SIDE = 760  # o mascote (684x624) com folga de ~6% em cada lado


def icon_svg() -> str:
    """O mascote de peças no meio de um quadrado escuro, para favicon e ícones."""
    w, h, body = inner(MASCOT)
    ox, oy = (ICON_SIDE - w) / 2, (ICON_SIDE - h) / 2 + 6
    return (
        f'<rect width="{ICON_SIDE}" height="{ICON_SIDE}" rx="120" fill="#141416"/>'
        f'<svg x="{ox}" y="{oy}" width="{w}" height="{h}" viewBox="0 0 {w:g} {h:g}">{body}</svg>'
    )


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

    # og-image: nome e frase à esquerda; à direita a cena aprovada INTEIRA (caranguejo, peça vermelha,
    # faíscas e a pilha), dentro de um painel do tamanho dela, sem nada cortado.
    w, h, body = inner(ROOT / "docs" / "identidade" / "bricks-cena.svg")
    panel_w, panel_h = 640, 630
    vb_w = 950.0  # largura da cena que cabe no painel (a linha do chão sai pelos lados, de propósito)
    vb_x = 80.0
    vb_h = vb_w * panel_h / panel_w
    vb_y = (h - vb_h) / 2
    font = "Helvetica Neue, Arial, sans-serif"
    og = (
        '<rect width="1200" height="630" fill="#171717"/>'
        '<g fill="#FFCC00"><rect x="64" y="70" width="34" height="14" rx="5"/><rect x="108" y="70" width="34" height="14" rx="5"/></g>'
        f'<text x="60" y="270" font-family="{font}" font-weight="900" font-size="132" letter-spacing="-6" fill="#F5F3EB">bit<tspan fill="#FFCC00">claw</tspan></text>'
        f'<text x="64" y="345" font-family="{font}" font-weight="700" font-size="34" fill="#F5F3EB">Um agente de IA local,</text>'
        f'<text x="64" y="390" font-family="{font}" font-weight="700" font-size="34" fill="#FFCC00">montado peça por peça.</text>'
        f'<text x="64" y="560" font-family="Menlo, monospace" font-size="24" fill="#A8A399">Rust · Ollama · documentação</text>'
        f'<svg x="{1200 - panel_w}" y="0" width="{panel_w}" height="{panel_h}" viewBox="{vb_x} {vb_y} {vb_w} {vb_h}">'
        f'<rect x="-2000" y="-2000" width="6000" height="6000" fill="#0C0C0E"/>{body}</svg>'
    )
    render(og, 1200, 630, OUT / "og-image.png")

    # ícones: o caranguejo de peças (o mesmo da galeria), inteiro, num quadrado com folga ao redor
    for name, size in (("favicon-32.png", 32), ("apple-touch-icon.png", 180), ("icon-512.png", 512)):
        render(icon_svg(), ICON_SIDE, ICON_SIDE, OUT / name, scale_to=size)
    (OUT / "favicon.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {ICON_SIDE} {ICON_SIDE}">{icon_svg()}</svg>\n'
    )
    print("PNGs em", OUT)


if __name__ == "__main__":
    main()

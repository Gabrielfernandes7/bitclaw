#!/usr/bin/env python3
"""Gera os SVGs do mascote BitClaw construído com peças de montar.

Uso: python3 docs/identidade/gerar-bricks.py

Modelo de peça (projeção oblíqua, vista de cima e da esquerda):
  - passo de pino P; altura da peça = 1,2 P (proporção de um tijolo real)
  - profundidade D pinos: o topo mostra D fileiras de pinos
  - três faces: frente (cor base), topo (clara) e lateral direita (escura)
  - contorno Core Black com junta arredondada
O caranguejo é um mapa de células (a grade continua sendo o master);
cada fileira é partida em peças de 1 a 4 pinos, com as juntas escalonadas
em relação à fileira de baixo (aparelho de tijolo).
"""
import random
from pathlib import Path

OUT = Path(__file__).parent
INK = "#171717"

# cor: (frente topo, frente base, topo, lateral)
COLORS = {
    "yellow": ("#FFD426", "#F2B800", "#FFEA8A", "#C99700"),
    "red":    ("#F0453A", "#D92B20", "#FF7C70", "#A81F17"),
    "blue":   ("#2C7CF0", "#1B5FCF", "#6FADFF", "#1245A0"),
    "green":  ("#46B832", "#2F9A22", "#86DB6B", "#237519"),
    "purple": ("#9A4DDB", "#7F34BE", "#C285F2", "#5E2490"),
}

CRAB = [  # de cima para baixo; # = peça, . = vazio
    "##...........##",
    "##.##.....##.##",
    "#####.....#####",
    ".###.......###.",
    "..###.....###..",
    "...#########...",
    "...#########...",
    "...#########...",
    "....#######....",
    "..##.#...#.##..",
    ".##..#...#..##.",
]
EYES = [(5, 6), (9, 6)]  # (coluna, linha a partir do topo)
SMILE = (7, 7)           # centro do sorriso


class Geo:
    def __init__(self, p=40.0, depth=2):
        self.p = p
        self.h = p * 1.2
        self.d = depth
        self.dx = p * 0.30
        self.dy = p * 0.30
        self.ox = depth * self.dx
        self.oy = depth * self.dy
        self.sw = max(3.0, p * 0.11)  # espessura do contorno


def defs(prefix="g"):
    out = []
    for name, (f1, f2, t, s) in COLORS.items():
        out.append(
            f'<linearGradient id="{prefix}-{name}" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{f1}"/><stop offset="1" stop-color="{f2}"/></linearGradient>'
        )
    return "\n".join(out)


def stud(g, cx, cy, color, prefix):
    _, _, top, side = COLORS[color]
    r = g.p * 0.29
    ry = r * 0.46
    hs = g.p * 0.2
    sw = g.sw * 0.8
    return (
        f'<path d="M{cx-r:.1f} {cy-hs:.1f} L{cx-r:.1f} {cy:.1f} A{r:.1f} {ry:.1f} 0 0 0 {cx+r:.1f} {cy:.1f} '
        f'L{cx+r:.1f} {cy-hs:.1f} Z" fill="{side}" stroke="{INK}" stroke-width="{sw:.1f}" stroke-linejoin="round"/>'
        f'<ellipse cx="{cx:.1f}" cy="{cy-hs:.1f}" rx="{r:.1f}" ry="{ry:.1f}" fill="{top}" '
        f'stroke="{INK}" stroke-width="{sw:.1f}"/>'
    )


def brick(g, x, yb, w, color, prefix, studs=None):
    """Peça de w pinos com a quina frontal inferior esquerda em (x, yb).
    studs: lista de bool por coluna (pinos visíveis); None = todos."""
    f1, f2, top, side = COLORS[color]
    p, h, ox, oy = g.p, g.h, g.ox, g.oy
    W = w * p
    yt = yb - h
    sw = g.sw
    s = []
    s.append(
        f'<polygon points="{x+W:.1f},{yt:.1f} {x+W+ox:.1f},{yt-oy:.1f} {x+W+ox:.1f},{yb-oy:.1f} {x+W:.1f},{yb:.1f}" '
        f'fill="{side}" stroke="{INK}" stroke-width="{sw:.1f}" stroke-linejoin="round"/>'
    )
    s.append(
        f'<polygon points="{x:.1f},{yt:.1f} {x+W:.1f},{yt:.1f} {x+W+ox:.1f},{yt-oy:.1f} {x+ox:.1f},{yt-oy:.1f}" '
        f'fill="{top}" stroke="{INK}" stroke-width="{sw:.1f}" stroke-linejoin="round"/>'
    )
    for j in range(g.d - 1, -1, -1):  # fileira de trás primeiro
        for i in range(w):
            if studs is not None and not studs[i]:
                continue
            cx = x + (i + 0.5) * p + (j + 0.5) * g.dx
            cy = yt - (j + 0.5) * g.dy
            s.append(stud(g, cx, cy, color, prefix))
    s.append(
        f'<rect x="{x:.1f}" y="{yt:.1f}" width="{W:.1f}" height="{h:.1f}" rx="{p*0.07:.1f}" '
        f'fill="url(#{prefix}-{color})" stroke="{INK}" stroke-width="{sw:.1f}" stroke-linejoin="round"/>'
    )
    # brilho de plástico na quina superior da frente
    s.append(
        f'<rect x="{x+p*0.1:.1f}" y="{yt+p*0.09:.1f}" width="{W-p*0.2:.1f}" height="{p*0.08:.1f}" '
        f'rx="{p*0.04:.1f}" fill="#FFFFFF" opacity="0.38"/>'
    )
    return "".join(s)


def segment(runs_len, start, prev_seams, rng):
    """Parte uma sequência contínua em peças de 1..4 evitando juntas alinhadas."""
    best, best_score = None, 10**9
    for _ in range(300):
        pos, parts = 0, []
        while pos < runs_len:
            left = runs_len - pos
            opts = [n for n in (4, 3, 2, 1) if n <= left]
            weights = {4: 3, 3: 4, 2: 4, 1: 0.3}
            n = rng.choices(opts, [weights[o] for o in opts])[0]
            if left - n == 1:  # evita sobra de 1 pino
                n = left if left <= 4 else n + 0
            parts.append(n)
            pos += n
        seams, acc = set(), 0
        for n in parts[:-1]:
            acc += n
            seams.add(start + acc)
        score = len(seams & prev_seams) * 10 + sum(1 for n in parts if n == 1)
        if score < best_score:
            best, best_score = parts, score
            if score == 0:
                break
    return best


def crab(g, ox, oy, color="yellow", prefix="g", eyes=True):
    """Desenha o caranguejo com a quina inferior esquerda em (ox, oy)."""
    rng = random.Random(7)
    rows = CRAB[::-1]  # de baixo para cima
    n = len(rows)
    out = []
    prev = set()
    layout = []
    for r, line in enumerate(rows):
        row_seams = set()
        bricks_row = []
        c = 0
        while c < len(line):
            if line[c] != "#":
                c += 1
                continue
            e = c
            while e < len(line) and line[e] == "#":
                e += 1
            parts = segment(e - c, c, prev, rng)
            pos = c
            for k in parts:
                bricks_row.append((pos, k))
                pos += k
                row_seams.add(pos)
            c = e
        layout.append(bricks_row)
        prev = row_seams
    for r in range(n):
        yb = oy - r * g.h
        above = rows[r + 1] if r + 1 < n else ""
        for (c0, k) in layout[r]:
            vis = [not (c0 + i < len(above) and above[c0 + i] == "#") for i in range(k)]
            out.append(brick(g, ox + c0 * g.p, yb, k, color, prefix, vis))
    if eyes:
        top_rows = n
        def cell(col, row_from_top):
            x = ox + col * g.p
            yb = oy - (top_rows - 1 - row_from_top) * g.h
            return x, yb
        for col, rw in EYES:
            x, yb = cell(col, rw)
            ew, eh = g.p * 0.58, g.p * 0.86
            out.append(
                f'<rect x="{x+g.p/2-ew/2:.1f}" y="{yb-g.h/2-eh/2:.1f}" width="{ew:.1f}" height="{eh:.1f}" '
                f'rx="{ew*0.3:.1f}" fill="{INK}"/>'
            )
        x, yb = cell(*SMILE)
        cxm, cym = x + g.p / 2, yb - g.h * 0.62
        rw_ = g.p * 0.85
        out.append(
            f'<path d="M{cxm-rw_:.1f} {cym:.1f} Q{cxm:.1f} {cym+g.p*0.95:.1f} {cxm+rw_:.1f} {cym:.1f} Z" fill="{INK}"/>'
        )
    return "".join(out)


def scene():
    g = Geo(p=38)
    W, H = 1200, 760
    ground = 660
    cx0 = 150
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" '
        f'aria-label="BitClaw, caranguejo de peças de montar, comemorando ao lado de uma pilha de peças">',
        "<defs>",
        defs("s"),
        '<filter id="s-blur" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="46"/></filter>',
        '<filter id="s-blur2" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="9"/></filter>',
        "</defs>",
        f'<rect width="{W}" height="{H}" fill="#0C0C0E"/>',
        '<ellipse cx="460" cy="400" rx="330" ry="270" fill="#FFB800" opacity="0.20" filter="url(#s-blur)"/>',
        '<ellipse cx="700" cy="170" rx="130" ry="90" fill="#FFB800" opacity="0.30" filter="url(#s-blur)"/>',
        f'<ellipse cx="460" cy="{ground+6}" rx="380" ry="16" fill="#000" opacity="0.55" filter="url(#s-blur2)"/>',
        f'<line x1="60" y1="{ground+4}" x2="{W-60}" y2="{ground+4}" stroke="{INK}" stroke-width="7" stroke-linecap="round"/>',
        f'<line x1="60" y1="{ground+4}" x2="{W-60}" y2="{ground+4}" stroke="#2A2A30" stroke-width="2" stroke-linecap="round"/>',
    ]
    parts.append(crab(g, cx0, ground, "yellow", "s"))
    # pilha de peças
    sx = 830
    for k, col in enumerate(["purple", "green", "blue"]):
        parts.append(brick(g, sx, ground - k * g.h, 4, col, "s"))
    # peça vermelha na pinça direita, levemente inclinada
    rx = cx0 + 11 * g.p + 6
    ry = ground - 10 * g.h + 6
    parts.append(f'<g transform="rotate(-14 {rx+g.p:.1f} {ry:.1f})">{brick(g, rx, ry, 3, "red", "s")}</g>')
    # faíscas
    for (x1, y1, x2, y2) in [(760, 120, 772, 78), (808, 140, 838, 106), (850, 176, 886, 160)]:
        parts.append(
            f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#FFC400" stroke-width="12" stroke-linecap="round"/>'
        )
    parts.append("</svg>")
    return "\n".join(parts)


def mascot(bg=None, p=40, pad=30):
    g = Geo(p=p)
    n = len(CRAB)
    w = len(CRAB[0]) * g.p + g.ox + pad * 2
    h = n * g.h + g.oy + pad * 2 + g.p * 0.3
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0f} {h:.0f}" width="{w:.0f}" height="{h:.0f}" '
        f'role="img" aria-label="BitClaw, caranguejo de peças de montar">',
        f"<defs>{defs('m')}</defs>",
    ]
    if bg:
        parts.append(f'<rect width="{w:.0f}" height="{h:.0f}" fill="{bg}"/>')
    parts.append(crab(g, pad, h - pad, "yellow", "m"))
    parts.append("</svg>")
    return "\n".join(parts)


def family():
    """Faixa com o caranguejo em quatro cores (aplicações), sobre papel."""
    g = Geo(p=15)
    cw = len(CRAB[0]) * g.p + g.ox
    ch = len(CRAB) * g.h + g.oy
    gap = 54
    W = 4 * cw + 3 * gap + 100
    H = ch + 90
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W:.0f} {H:.0f}" width="{W:.0f}" height="{H:.0f}" '
        f'role="img" aria-label="BitClaw em amarelo, vermelho, azul e verde">',
        f"<defs>{defs('f')}</defs>",
        f'<rect width="{W:.0f}" height="{H:.0f}" fill="#F5F3EB"/>',
    ]
    for i, col in enumerate(["yellow", "red", "blue", "green"]):
        parts.append(crab(g, 50 + i * (cw + gap), H - 45, col, "f"))
    parts.append("</svg>")
    return "\n".join(parts)


def pieces():
    """Catálogo de peças: 1x1, 1x2, 1x3, 1x4 em cores diferentes (como o pote)."""
    g = Geo(p=34)
    items = [(1, "red"), (2, "blue"), (3, "yellow"), (4, "green"), (2, "purple")]
    gap = 40
    W = sum(w * g.p + g.ox for w, _ in items) + gap * (len(items) + 1)
    H = g.h + g.oy + 80
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W:.0f} {H:.0f}" width="{W:.0f}" height="{H:.0f}" '
        f'role="img" aria-label="Peças 1x1, 1x2, 1x3, 1x4 e 1x2">',
        f"<defs>{defs('p')}</defs>",
        f'<rect width="{W:.0f}" height="{H:.0f}" fill="#F5F3EB"/>',
    ]
    x = gap
    for w, col in items:
        parts.append(brick(g, x, H - 40, w, col, "p"))
        x += w * g.p + g.ox + gap
    parts.append("</svg>")
    return "\n".join(parts)


if __name__ == "__main__":
    files = {
        "bricks-cena.svg": scene(),
        "bitclaw-bricks.svg": mascot(),
        "bitclaw-bricks-dark.svg": mascot(bg="#141416"),
        "bricks-feliz.svg": family(),
        "bricks-pecas.svg": pieces(),
    }
    for name, svg in files.items():
        (OUT / name).write_text(svg, encoding="utf-8")
        print("ok", name, len(svg) // 1024, "KB")

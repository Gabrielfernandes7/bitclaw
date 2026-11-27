#!/usr/bin/env python3
"""Gera o site do BitClaw (GitHub Pages) em `_site/`.

Fontes:
  - `site/content/*.md`: páginas escritas para o site.
  - `docs/*.md`: documentos do próprio repositório, publicados como estão
    (uma única fonte: o que muda em `docs/` muda no site).
  - `assets/` e `docs/identidade/`: os SVGs aprovados da identidade visual.

Uso: python3 site/build.py [--out _site] [--base /bitclaw/]
Dependências: `pip install -r site/requirements.txt` (markdown, pygments).
"""
from __future__ import annotations

import argparse
import html
import json
import posixpath
import re
import shutil
import sys
import unicodedata
from pathlib import Path

import markdown
from pygments.formatters import HtmlFormatter

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
REPO_URL = "https://github.com/Gabrielfernandes7/bitclaw"
BLOB = f"{REPO_URL}/blob/main/"
TREE = f"{REPO_URL}/tree/main/"
INSTALL_CMD = (
    "curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh"
)

# ---------------------------------------------------------------- navegação
# (pasta, título da seção, [(slug, título no menu, fonte)])
NAV = [
    ("Começar", [
        ("inicio/filosofia", "Filosofia", "site/content/inicio/filosofia.md"),
        ("inicio/instalacao", "Instalação", "site/content/inicio/instalacao.md"),
        ("inicio/primeiros-passos", "Primeiros passos", "site/content/inicio/primeiros-passos.md"),
    ]),
    ("Usar", [
        ("guia/prompt", "No prompt", "site/content/guia/prompt.md"),
        ("guia/configuracao", "Configuração", "site/content/guia/configuracao.md"),
        ("guia/ferramentas", "Ferramentas", "site/content/guia/ferramentas.md"),
        ("guia/skills", "Skills e SOUL", "site/content/guia/skills.md"),
        ("guia/contexto", "Contexto", "site/content/guia/contexto.md"),
        ("guia/seguranca", "Segurança", "site/content/guia/seguranca.md"),
    ]),
    ("Construir", [
        ("dev/biblioteca", "Usar como biblioteca", "site/content/dev/biblioteca.md"),
        ("dev/arquitetura", "Arquitetura do código", "site/content/dev/arquitetura.md"),
        ("dev/testes-e-releases", "Testes e releases", "site/content/dev/testes-e-releases.md"),
    ]),
    ("Projeto", [
        ("projeto/documento", "Documento de projeto", "docs/PROJETO.md"),
        ("projeto/arquitetura-alvo", "Arquitetura alvo", "docs/ARCHITECTURE.md"),
        ("projeto/escolha-contexto", "Escolha: histórico", "docs/CHOICE.md"),
        ("projeto/avaliacao", "Avaliação", "docs/AVALIACAO.md"),
        ("projeto/modelos", "Modelos e diagnósticos", "docs/MODELOS.md"),
        ("projeto/testes", "Pilares do teste", "docs/TEST.md"),
    ]),
    ("Marca", [
        ("marca/galeria", "Galeria", "site/content/marca/galeria.md"),
        ("marca/mascote", "O mascote", "docs/MASCOTE.md"),
        ("marca/arte-terminal", "Arte de terminal", "docs/identidade/ARTE-TERMINAL.md"),
    ]),
]

# SVGs aprovados copiados para `img/` (nome publicado -> fonte).
SVGS = {
    "bitclaw-crab.svg": "assets/bitclaw-crab.svg",
    "bitclaw-bricks.svg": "assets/bitclaw-bricks.svg",
    "bricks-cena.svg": "docs/identidade/bricks-cena.svg",
    "bricks-feliz.svg": "docs/identidade/bricks-feliz.svg",
    "bricks-pecas.svg": "docs/identidade/bricks-pecas.svg",
    "bitclaw-bricks-dark.svg": "docs/identidade/bitclaw-bricks-dark.svg",
    "bitclaw-ambar.svg": "docs/identidade/bitclaw-ambar.svg",
    "lua.svg": "assets/lua.svg",
}


MERMAID = """<script type="module">
import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11.4.0/dist/mermaid.esm.min.mjs";
const dark = document.documentElement.dataset.theme === "dark" ||
  (!document.documentElement.dataset.theme && matchMedia("(prefers-color-scheme: dark)").matches);
mermaid.initialize({ startOnLoad: true, theme: dark ? "dark" : "neutral", securityLevel: "strict", themeVariables: { fontSize: "16px" } });
</script>"""


def slugify(text: str, sep: str = "-") -> str:
    """Âncoras como no GitHub: minúsculas, acentos mantidos."""
    text = re.sub(r"[^\w\s-]", "", text.lower(), flags=re.UNICODE).strip()
    return re.sub(r"[\s_-]+", sep, text)


def fold(text: str) -> str:
    """Minúsculas sem acento, para a busca."""
    nfkd = unicodedata.normalize("NFKD", text.lower())
    return "".join(c for c in nfkd if not unicodedata.combining(c))


class Page:
    def __init__(self, slug: str, title: str, source: str, section: str):
        self.slug, self.menu_title, self.source, self.section = slug, title, source, section
        self.url = f"{slug}.html"
        self.body = ""
        self.toc: list[dict] = []
        self.title = title
        self.description = ""
        self.text = ""
        self.mermaid = False


def parse_front(raw: str) -> tuple[dict, str]:
    meta: dict[str, str] = {}
    if raw.startswith("---\n"):
        end = raw.find("\n---\n", 4)
        if end != -1:
            for line in raw[4:end].splitlines():
                if ":" in line:
                    k, v = line.split(":", 1)
                    meta[k.strip()] = v.strip()
            raw = raw[end + 5:]
    return meta, raw


def root_prefix(url: str) -> str:
    return "../" * url.count("/")


# ------------------------------------------------------------------ marca
def logo(cls: str, prefix: str) -> str:
    """O caranguejo de peças aprovado (`bitclaw-bricks-dark.svg`), sem pixel art."""
    return f'<img class="{cls}" src="{prefix}img/bitclaw-bricks-dark.svg" alt="" width="36" height="33">'


# ---------------------------------------------------------- Markdown -> HTML
class Builder:
    def __init__(self, out: Path, base: str):
        self.out, self.base = out, base
        self.pages: list[Page] = []
        self.by_source: dict[Path, Page] = {}
        for section, items in NAV:
            for slug, title, source in items:
                p = Page(slug, title, source, section)
                self.pages.append(p)
                self.by_source[(ROOT / source).resolve()] = p
        self.copied_images: dict[Path, str] = {}

    # -- links e imagens
    def rewrite(self, page: Page, body: str) -> str:
        src_dir = (ROOT / page.source).parent
        prefix = root_prefix(page.url)

        def fix_href(m: re.Match) -> str:
            url = m.group(1)
            if url.startswith("@img/"):
                return f'href="{prefix}img/{url[5:]}"'
            if re.match(r"^(https?:|mailto:|#|/)", url):
                return m.group(0)
            path, _, frag = url.partition("#")
            target = (src_dir / path).resolve()
            slug_hit = None
            if path.endswith(".md") and page.source.startswith("site/content/"):
                # em páginas do site, o link é pelo caminho publicado (../projeto/documento.md)
                want = posixpath.normpath(posixpath.join(posixpath.dirname(page.slug), path[:-3]))
                slug_hit = next((q for q in self.pages if q.slug == want), None)
            if slug_hit:
                new = prefix + slug_hit.url
            elif target in self.by_source:
                new = prefix + self.by_source[target].url
            elif target.exists():
                try:
                    rel = target.relative_to(ROOT)
                except ValueError:
                    return m.group(0)
                new = (TREE if target.is_dir() else BLOB) + rel.as_posix()
            else:
                return m.group(0)
            return f'href="{new}{"#" + frag if frag else ""}"'

        def fix_src(m: re.Match) -> str:
            url = m.group(1)
            if url.startswith("@img/"):
                return f'src="{prefix}img/{url[5:]}"'
            if re.match(r"^(https?:|data:|/)", url):
                return m.group(0)
            target = (src_dir / url).resolve()
            if not target.exists():
                return m.group(0)
            if target.name in SVGS and (ROOT / SVGS[target.name]).resolve() == target:
                return f'src="{prefix}img/{target.name}"'
            name = self.copied_images.setdefault(target, f"ext/{target.name}")
            return f'src="{prefix}img/{name}"'

        body = re.sub(r'href="([^"]*)"', fix_href, body)
        body = re.sub(r'src="([^"]*)"', fix_src, body)
        body = body.replace("@root/", prefix)
        return body

    def render(self, page: Page) -> None:
        raw = (ROOT / page.source).read_text()
        meta, raw = parse_front(raw)
        m = re.match(r"\s*# (.+)\n", raw)
        if m:
            page.title = re.sub(r"[`*]", "", m.group(1)).strip()
            raw = raw[m.end():]
        page.title = meta.get("title", page.title)
        md = markdown.Markdown(
            extensions=["extra", "toc", "admonition", "sane_lists", "codehilite"],
            extension_configs={
                "toc": {"slugify": slugify, "toc_depth": "2-3", "permalink": "#", "permalink_title": "Link para esta seção"},
                "codehilite": {"guess_lang": False, "css_class": "hl", "noclasses": False},
            },
        )
        # ```mermaid vira <pre class="mermaid">, desenhado no navegador
        def mermaid(m: re.Match) -> str:
            page.mermaid = True
            return f'\n<pre class="mermaid">\n{html.escape(m.group(1))}</pre>\n'

        raw = re.sub(r"```mermaid\n(.*?)```", mermaid, raw, flags=re.S)
        body = md.convert(raw)
        page.toc = [
            {"id": t["id"], "name": re.sub(r"<[^>]+>", "", t["name"]), "level": t["level"],
             "children": t.get("children", [])}
            for t in md.toc_tokens  # type: ignore[attr-defined]
        ]
        first_p = re.search(r"<p>(.+?)</p>", body, re.S)
        desc = meta.get("description") or (re.sub(r"<[^>]+>", "", first_p.group(1)) if first_p else "")
        page.description = html.unescape(re.sub(r"\s+", " ", desc)).strip()[:200]
        body = body.replace("<table>", '<div class="tw"><table>').replace("</table>", "</table></div>")
        page.body = self.rewrite(page, body)
        text = re.sub(r"<[^>]+>", " ", body)
        page.text = re.sub(r"\s+", " ", html.unescape(text)).strip()

    # -- templates
    def header(self, prefix: str, current: str = "") -> str:
        return f"""<header class="top">
  <button class="burger" aria-label="Abrir menu" aria-controls="side" data-burger>
    <span></span><span></span><span></span></button>
  <a class="brand" href="{prefix}index.html" aria-label="BitClaw, início">
    {logo("brand-crab", prefix)}<span class="wordmark">bit<b>claw</b></span></a>
  <nav class="top-nav" aria-label="Principal">
    <a href="{prefix}inicio/filosofia.html"{' aria-current="page"' if current == 'Começar' else ''}>Começar</a>
    <a href="{prefix}guia/prompt.html"{' aria-current="page"' if current == 'Usar' else ''}>Usar</a>
    <a href="{prefix}dev/biblioteca.html"{' aria-current="page"' if current == 'Construir' else ''}>Construir</a>
    <a href="{prefix}marca/galeria.html"{' aria-current="page"' if current == 'Marca' else ''}>Marca</a>
  </nav>
  <div class="tools">
    <button class="search-btn" data-search-open aria-label="Buscar na documentação">
      <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true"><circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M10.5 10.5 14 14" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
      <span>Buscar</span><kbd>/</kbd></button>
    <a class="gh" href="{REPO_URL}" aria-label="BitClaw no GitHub" rel="noopener">
      <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M8 0a8 8 0 0 0-2.53 15.59c.4.07.55-.17.55-.38v-1.34c-2.23.48-2.7-1.07-2.7-1.07-.36-.92-.89-1.17-.89-1.17-.73-.5.05-.49.05-.49.8.06 1.23.83 1.23.83.72 1.22 1.88.87 2.34.66.07-.52.28-.87.5-1.07-1.78-.2-3.65-.89-3.65-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.6 7.6 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.19c0 .21.15.46.55.38A8 8 0 0 0 8 0z"/></svg></a>
    <button class="theme-btn" data-theme-toggle aria-label="Alternar tema claro e escuro">
      <svg class="i-sun" viewBox="0 0 16 16" width="17" height="17" aria-hidden="true"><circle cx="8" cy="8" r="3" fill="currentColor"/><g stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.4 1.4M11.6 11.6 13 13M13 3l-1.4 1.4M4.4 11.6 3 13"/></g></svg>
      <svg class="i-moon" viewBox="0 0 16 16" width="17" height="17" aria-hidden="true"><path fill="currentColor" d="M6.2 1.5a6.5 6.5 0 1 0 8.3 8.3A5.5 5.5 0 0 1 6.2 1.5z"/></svg></button>
  </div>
</header>"""

    def search_dialog(self, prefix: str) -> str:
        return f"""<dialog class="search" id="search" data-root="{prefix}">
  <form method="dialog" class="search-box">
    <input type="search" placeholder="Buscar na documentação…" aria-label="Buscar" autocomplete="off" data-search-input>
    <button value="close" aria-label="Fechar busca">esc</button>
  </form>
  <ul class="results" data-search-results></ul>
</dialog>"""

    def head(self, title: str, desc: str, prefix: str, url: str) -> str:
        canonical = f"{self.base}{url}" if self.base.startswith("http") else ""
        return f"""<!doctype html>
<html lang="pt-BR">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{html.escape(title)}</title>
<meta name="description" content="{html.escape(desc, quote=True)}">
<meta name="theme-color" content="#FFCC00">
<meta property="og:type" content="website">
<meta property="og:title" content="{html.escape(title, quote=True)}">
<meta property="og:description" content="{html.escape(desc, quote=True)}">
<meta property="og:image" content="{prefix}img/og-image.png">
<meta name="twitter:card" content="summary_large_image">
{f'<link rel="canonical" href="{canonical}">' if canonical else ''}
<link rel="icon" href="{prefix}img/favicon.svg" type="image/svg+xml">
<link rel="icon" href="{prefix}img/favicon-32.png" sizes="32x32" type="image/png">
<link rel="apple-touch-icon" href="{prefix}img/apple-touch-icon.png">
<script>try{{var t=localStorage.getItem("bc-theme");if(t)document.documentElement.dataset.theme=t}}catch(e){{}}</script>
<link rel="stylesheet" href="{prefix}css/site.css">
</head>"""

    def footer(self, prefix: str) -> str:
        return f"""<footer class="foot">
  <div class="studs" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
  <div class="foot-in">
    <div>{logo("foot-crab", prefix)}<span class="wordmark">bit<b>claw</b></span></div>
    <p>Um agente de IA local e enxuto, montado peça por peça. Feito em Rust.</p>
    <p class="muted"><a href="{REPO_URL}" rel="noopener">GitHub</a> · <a href="{REPO_URL}/releases" rel="noopener">Releases</a> · <a href="{REPO_URL}/issues" rel="noopener">Issues</a> · <a href="{BLOB}LICENSE" rel="noopener">Licença (PolyForm Noncommercial)</a></p>
  </div>
</footer>
<script src="{prefix}js/site.js" defer></script>"""

    def sidebar(self, page: Page, prefix: str) -> str:
        out = ['<aside class="side" id="side" aria-label="Documentação"><nav>']
        for section, items in NAV:
            out.append(f'<div class="grp"><h2>{section}</h2><ul>')
            for slug, title, _ in items:
                cur = ' aria-current="page"' if slug == page.slug else ""
                out.append(f'<li><a href="{prefix}{slug}.html"{cur}>{html.escape(title)}</a></li>')
            out.append("</ul></div>")
        out.append("</nav></aside>")
        return "".join(out)

    def toc_html(self, page: Page) -> str:
        items = []
        for t in page.toc:
            items.append((t["id"], t["name"], 2))
            for c in t["children"]:
                items.append((c["id"], re.sub(r"<[^>]+>", "", c["name"]), 3))
        if len(items) < 2:
            return '<aside class="toc" aria-hidden="true"></aside>'
        lis = "".join(
            f'<li class="l{lv}"><a href="#{i}">{html.escape(n)}</a></li>' for i, n, lv in items
        )
        return f'<aside class="toc"><h2>Nesta página</h2><ul>{lis}</ul></aside>'

    def doc_page(self, idx: int, page: Page) -> str:
        prefix = root_prefix(page.url)
        prev = self.pages[idx - 1] if idx > 0 else None
        nxt = self.pages[idx + 1] if idx + 1 < len(self.pages) else None
        pn = ""
        if prev or nxt:
            pn = '<nav class="pn" aria-label="Navegação entre páginas">'
            pn += (f'<a class="prev" href="{prefix}{prev.url}"><small>Anterior</small>{html.escape(prev.menu_title)}</a>'
                   if prev else "<span></span>")
            pn += (f'<a class="next" href="{prefix}{nxt.url}"><small>Próxima</small>{html.escape(nxt.menu_title)}</a>'
                   if nxt else "<span></span>")
            pn += "</nav>"
        edit = f"{BLOB}{page.source}"
        title = f"{page.title} · BitClaw"
        return (
            self.head(title, page.description, prefix, page.url)
            + f"""
<body class="doc">
<a class="skip" href="#main">Pular para o conteúdo</a>
{self.header(prefix, page.section)}
<div class="shell">
{self.sidebar(page, prefix)}
<main id="main" class="main">
  <article class="prose">
    <p class="crumb">{html.escape(page.section)} <span aria-hidden="true">›</span> {html.escape(page.menu_title)}</p>
    <h1>{html.escape(page.title)}</h1>
    {page.body}
  </article>
  <p class="edit"><a href="{edit}" rel="noopener">Ver esta página no GitHub ↗</a></p>
  {pn}
</main>
{self.toc_html(page)}
</div>
{self.footer(prefix)}
{self.search_dialog(prefix)}
{MERMAID if page.mermaid else ""}
</body>
</html>
"""
        )

    # -- página inicial
    def home(self) -> str:
        prefix = ""
        cards = [
            ("y", "Local primeiro", "Roda com um modelo no seu Mac via Ollama. Nada sai da máquina, a menos que você escolha o OpenRouter."),
            ("r", "Pede antes de alterar", "Edições, criações e remoções mostram a prévia e esperam o seu <kbd>s</kbd>. Scripts sempre pedem, até com <code>--yes</code>."),
            ("b", "Um core, vários agentes", "O Core não conhece modelo nem agente específico. Quem constrói o próximo agente não edita o Core."),
            ("g", "Skills sob demanda", "Instruções em pastas com <code>SKILL.md</code>, carregadas só quando precisam. Instaladas só com digest aprovado."),
            ("p", "Modo Plan e /undo", "<code>--plan</code> deixa só ferramentas de leitura. <code>/undo</code> desfaz o último turno com alterações."),
            ("y", "Feito para modelos pequenos", "Contexto com orçamento, buscas que cortam a saída e um detector de laço: pequeno não quer dizer frágil."),
        ]
        cards_html = "".join(
            f'<article class="brick {c}"><h3>{t}</h3><p>{d}</p></article>' for c, t, d in cards
        )
        hubs = [
            ("inicio/filosofia.html", "Filosofia", "Por que o BitClaw existe e o que ele recusa ser."),
            ("inicio/instalacao.html", "Instalação", "Uma linha de <code>curl</code>, ou <code>cargo install</code>."),
            ("guia/prompt.html", "No prompt", "Teclas, comandos <code>/</code> e o <code>@</code> de arquivos."),
            ("guia/configuracao.html", "Configuração", "O agente é um TOML. Ollama, OpenRouter e limites."),
            ("guia/ferramentas.html", "Ferramentas", "Do <code>list_dir</code> ao <code>run_command</code>."),
            ("guia/seguranca.html", "Segurança", "O que protege você e o que não protege."),
            ("dev/biblioteca.html", "Biblioteca", "Monte um agente novo com <code>Session</code>."),
            ("dev/arquitetura.html", "Arquitetura", "Core, toolkit, adaptadores e app."),
            ("marca/galeria.html", "Marca", "O caranguejo de peças e a identidade visual."),
        ]
        hubs_html = "".join(
            f'<a class="hub" href="{u}"><strong>{t}</strong><span>{d}</span></a>' for u, t, d in hubs
        )
        title = "BitClaw · um agente de IA local, montado peça por peça"
        desc = "Documentação do BitClaw: núcleo experimental de agente de IA, local e enxuto, escrito em Rust."
        return (
            self.head(title, desc, prefix, "index.html")
            + f"""
<body class="home">
<a class="skip" href="#main">Pular para o conteúdo</a>
{self.header(prefix)}
<main id="main">
<section class="hero">
  <div class="hero-in">
    <div class="hero-copy">
      <p class="eyebrow">Agent Core · Rust · Local primeiro</p>
      <h1>Um caranguejo feito de peças.<br><span>Um agente feito de peças.</span></h1>
      <p class="lead">BitClaw é um núcleo experimental de agente de IA, local e enxuto. Lê e edita os arquivos do seu projeto com um modelo que roda na sua máquina, e cada peça (modelo, ferramenta, interface) se troca sem mexer no resto.</p>
      <div class="cta">
        <a class="btn primary" href="inicio/instalacao.html">Instalar</a>
        <a class="btn" href="inicio/filosofia.html">Ler a filosofia</a>
      </div>
      <div class="cmd" data-copy-box>
        <span class="prompt" aria-hidden="true">$</span>
        <code>{html.escape(INSTALL_CMD)}</code>
        <button class="copy" data-copy="{html.escape(INSTALL_CMD, quote=True)}" aria-label="Copiar o comando de instalação">copiar</button>
      </div>
      <p class="muted small">macOS (Apple Silicon e Intel) e Linux (x86_64 e arm64). Sem <code>sudo</code>; confere o SHA-256.</p>
    </div>
    <div class="hero-art">
      <div class="stage">
        <img class="hero-img" src="img/bricks-cena.svg" alt="O BitClaw, caranguejo de peças de montar amarelas, encaixando uma peça vermelha na pinça" width="1200" height="760">
      </div>
      <p class="caption">O BitClaw encaixando a próxima peça: o core não muda, o agente novo entra.</p>
    </div>
  </div>
  <div class="studs wide" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
</section>

<section class="band">
  <div class="wrap">
    <h2 class="sec">Peças, não monólito</h2>
    <div class="bricks">{cards_html}</div>
  </div>
</section>

<section class="band alt">
  <div class="wrap two">
    <div>
      <h2 class="sec">A régua do projeto</h2>
      <blockquote class="rule">Alguém consegue construir um novo agente usando só a API pública do Core, sem modificar o Core.</blockquote>
      <p>Quantidade de funcionalidades não mede sucesso. Cada mudança passa por duas perguntas: isso pertence ao Core ou a um agente específico? Isso facilita construir o próximo agente sem editar o Core?</p>
      <p><a class="more" href="inicio/filosofia.html">Filosofia completa →</a></p>
    </div>
    <figure class="shot">
      <img src="img/bricks-pecas.svg" alt="Peças de montar coloridas do BitClaw" loading="lazy">
    </figure>
  </div>
</section>

<section class="band">
  <div class="wrap">
    <h2 class="sec">Do zero ao primeiro turno</h2>
    <ol class="steps">
      <li><h3>Instale</h3><pre class="mini"><code>curl -fsSL …/install.sh | sh</code></pre></li>
      <li><h3>Baixe um modelo</h3><pre class="mini"><code>ollama pull nemotron-3-nano:4b-mlx</code></pre></li>
      <li><h3>Abra no seu projeto</h3><pre class="mini"><code>bitclaw --dir ~/meu-projeto</code></pre></li>
      <li><h3>Peça em português</h3><pre class="mini"><code>leia notas.md e resuma em três frases</code></pre></li>
    </ol>
  </div>
</section>

<section class="band dark">
  <div class="wrap">
    <h2 class="sec">A marca</h2>
    <p class="sec-sub">Um caranguejo formado por brinquedo de montar amarelo. Olho é buraco; os dois pinos do topo são a assinatura.</p>
    <div class="gallery">
      <figure><img src="img/bricks-cena.svg" alt="O caranguejo de peças encaixando uma peça vermelha" loading="lazy"></figure>
      <figure><img src="img/bricks-feliz.svg" alt="Variações do mascote: feliz, pinças abertas e piscando" loading="lazy"></figure>
    </div>
    <p><a class="more" href="marca/galeria.html">Ver a galeria →</a></p>
  </div>
</section>

<section class="band">
  <div class="wrap">
    <h2 class="sec">Documentação</h2>
    <div class="hubs">{hubs_html}</div>
  </div>
</section>
</main>
{self.footer(prefix)}
{self.search_dialog(prefix)}
</body>
</html>
"""
        )

    def not_found(self) -> str:
        prefix = self.base if self.base.startswith(("/", "http")) else ""
        if prefix and not prefix.endswith("/"):
            prefix += "/"
        return (
            self.head("Página não encontrada · BitClaw", "Página não encontrada.", prefix, "404.html")
            + f"""
<body class="home">
{self.header(prefix)}
<main id="main"><section class="band"><div class="wrap nf">
  {logo("hero-crab small", prefix)}
  <h1>404: faltou uma peça.</h1>
  <p>Esta página não existe (ou mudou de lugar).</p>
  <p><a class="btn primary" href="{prefix}index.html">Voltar ao início</a></p>
</div></section></main>
{self.footer(prefix)}
{self.search_dialog(prefix)}
</body></html>
"""
        )

    # -- execução
    def build(self) -> None:
        if self.out.exists():
            shutil.rmtree(self.out)
        self.out.mkdir(parents=True)

        for p in self.pages:
            self.render(p)

        for i, p in enumerate(self.pages):
            dest = self.out / p.url
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(self.doc_page(i, p))

        (self.out / "index.html").write_text(self.home())
        (self.out / "404.html").write_text(self.not_found())
        (self.out / ".nojekyll").write_text("")

        # estáticos
        shutil.copytree(SITE / "static", self.out, dirs_exist_ok=True)
        img = self.out / "img"
        img.mkdir(exist_ok=True)
        for name, src in SVGS.items():
            shutil.copy(ROOT / src, img / name)
        for src, rel in self.copied_images.items():
            dest = img / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(src, dest)

        css = HtmlFormatter(style="monokai", cssclass="hl").get_style_defs(".hl")
        (self.out / "css").mkdir(exist_ok=True)
        with open(self.out / "css" / "site.css", "a") as f:
            f.write("\n/* pygments */\n" + css + "\n.hl{background:transparent!important}\n")

        index = [
            {"t": p.title, "s": p.section, "u": p.url,
             "h": [t["name"] for t in p.toc],
             "x": fold(p.text)[:6000], "o": p.text[:6000], "d": p.description}
            for p in self.pages
        ]
        (self.out / "search-index.json").write_text(json.dumps(index, ensure_ascii=False))
        print(f"{len(self.pages)} páginas + início em {self.out}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(ROOT / "_site"))
    ap.add_argument("--base", default="/bitclaw/", help="caminho público do site (para o 404)")
    args = ap.parse_args()
    Builder(Path(args.out), args.base).build()
    return 0


if __name__ == "__main__":
    sys.exit(main())

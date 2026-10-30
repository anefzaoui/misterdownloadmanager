#!/usr/bin/env python3
"""Builds misterdownloadmanager.com into target/site (GitHub Pages publishes it, .github/workflows/
pages.yml). No framework: website/ holds the hand-written home page, styles, scripts and assets;
website/content/ the pages written as Markdown (FAQ, legal); the rest comes from the repository
itself so the site never drifts from it: docs/*.md become the help pages, CHANGELOG.md the news
page, i18n/languages.tsv the languages page. Every page gets the shared header and footer, its
own title, description, canonical address, social card and structured data, and goes into
sitemap.xml.

    python tools/website/build.py            # build target/site
    python tools/website/build.py --serve    # build, then serve it on http://127.0.0.1:8765

Needs: pip install markdown
"""
from __future__ import annotations

import datetime
import html
import json
import re
import shutil
import sys
from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "website"
CONTENT = WEB / "content"
OUT = ROOT / "target" / "site"
SITE = "https://misterdownloadmanager.com"
REPO = "https://github.com/anefzaoui/misterdownloadmanager"
NAME = "Mister Download Manager"
VERSION = re.search(r'(?m)^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text("utf-8")).group(1)
TODAY = datetime.date.today().isoformat()
RELEASE = f"{REPO}/releases/download/v{VERSION}/MisterDownloadManager-{VERSION}"
FILES = {
    "setup": f"{RELEASE}-setup.exe",
    "portable": f"{RELEASE}-portable.zip",
    "zip": f"{RELEASE}-x64.zip",
    "dmg": f"{RELEASE}-macos-universal-experimental.dmg",
    "appimage": f"{RELEASE}-linux-x86_64-experimental.AppImage",
    "deb": f"{RELEASE}-linux-x86_64-experimental.deb",
    "rpm": f"{RELEASE}-linux-x86_64-experimental.rpm",
    "tar": f"{RELEASE}-linux-x86_64-experimental.tar.gz",
    "chromium": f"{RELEASE}-extension-chromium.zip",
    "firefox": f"{RELEASE}-extension-firefox.xpi",
    "sums": f"{REPO}/releases/download/v{VERSION}/SHA256SUMS.txt",
}

# Every page built, for the sitemap: (path, priority).
PAGES: list[tuple[str, str]] = []


def esc(s: str) -> str:
    return html.escape(s, quote=True)


def md(text: str) -> str:
    return markdown.markdown(
        text, extensions=["tables", "fenced_code", "toc", "attr_list", "sane_lists", "md_in_html"],
        extension_configs={"toc": {"permalink": False}},
    )


def plain(text: str) -> str:
    """Markdown or HTML to one line of plain text (descriptions)."""
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", text)
    text = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"[*_`>#]", "", text)
    return " ".join(html.unescape(text).split())


def clip(text: str, n: int = 158) -> str:
    text = plain(text)
    if len(text) <= n:
        return text
    cut = text[: n - 1].rsplit(" ", 1)[0].rstrip(",;:—-")
    return cut + "…"


# ---- The frame every page shares ---------------------------------------------------------------

ICONS = """<svg width="0" height="0" class="defs" aria-hidden="true">
  <symbol id="os-windows" viewBox="0 0 24 24"><path d="M2 4.6 10 3.5v7.9H2zM11 3.4 22 2v9.4H11zM2 12.4h8v7.9l-8-1.1zM11 12.4h11V22l-11-1.5z"/></symbol>
  <symbol id="os-macos" viewBox="0 0 24 24"><path d="M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701"/></symbol>
  <symbol id="os-linux" viewBox="0 0 24 24"><path d="M12 1.5c-2.6 0-4.3 2-4.3 4.6 0 1.4.3 2.3-.6 3.6C5.6 11.9 4.6 14 4.6 16.3c0 3.2 3.1 5.7 7.4 5.7s7.4-2.5 7.4-5.7c0-2.3-1-4.4-2.5-6.6-.9-1.3-.6-2.2-.6-3.6 0-2.6-1.7-4.6-4.3-4.6z"/><path d="M12 9.6c-2.4 0-4.1 2.9-4.1 6.2 0 2.6 1.8 4.4 4.1 4.4s4.1-1.8 4.1-4.4c0-3.3-1.7-6.2-4.1-6.2z" fill="var(--paper)"/><circle cx="10.5" cy="5.6" r=".95" fill="var(--paper)"/><circle cx="13.5" cy="5.6" r=".95" fill="var(--paper)"/><path d="M10.3 7.3h3.4L12 9.2z" fill="#f2b630"/><path d="M3.6 21.6c.4-2.2 2.6-3.3 4.6-2.3-.2 2-2.6 3.3-4.6 2.3zm16.8 0c-.4-2.2-2.6-3.3-4.6-2.3.2 2 2.6 3.3 4.6 2.3z" fill="#f2b630"/></symbol>
  <symbol id="i-github" viewBox="0 0 24 24"><path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.8 1.19 1.83 1.19 3.09 0 4.42-2.69 5.39-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5z"/></symbol>
</svg>"""

NAV = [
    ("Features", "/#engine", "home"),
    ("Download", "/download.html", "download"),
    ("Help", "/help/", "help"),
    ("FAQ", "/faq/", "faq"),
    ("News", "/news.html", "news"),
    ("vs IDM", "/idm-alternative.html", "compare"),
]


def header(section: str = "") -> str:
    links = "\n        ".join(
        f'<a href="{href}"{" aria-current=\"page\"" if key == section else ""}>{label}</a>'
        for label, href, key in NAV
    )
    return f"""<a class="skip-link" href="#main">Skip to content</a>
  {ICONS}
  <header class="top">
    <div class="wrap top-inner">
      <a class="brand" href="/">
        <img src="/assets/logo-64.png" width="28" height="28" alt="">
        <span>{NAME}</span>
      </a>
      <nav class="nav" aria-label="Main">
        {links}
      </nav>
      <div class="top-actions">
        <button class="theme" type="button" aria-label="Switch between light and dark" title="Light / dark">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a9 9 0 1 0 0 18z"/><circle cx="12" cy="12" r="8.25" fill="none" stroke="currentColor" stroke-width="1.5"/></svg>
        </button>
        <a class="gh" href="{REPO}" aria-label="Source code on GitHub">
          <svg viewBox="0 0 24 24" aria-hidden="true"><use href="#i-github"/></svg>
        </a>
        <a class="btn btn-primary btn-small" href="/download.html">Download</a>
      </div>
    </div>
  </header>"""


def footer() -> str:
    year = datetime.date.today().year
    return f"""<footer class="foot">
    <div class="wrap">
      <div class="foot-cols">
        <div>
          <a class="brand small" href="/"><img src="/assets/logo-64.png" width="20" height="20" alt=""><span>{NAME}</span></a>
          <p class="foot-tag">A free, open-source download manager: every connection at once, crash-safe resume, torrents and video, in 86 languages.</p>
        </div>
        <div>
          <h2>Product</h2>
          <ul>
            <li><a href="/download.html">Download</a></li>
            <li><a href="/#engine">Features</a></li>
            <li><a href="/#bench">Benchmarks</a></li>
            <li><a href="/idm-alternative.html">MDM vs IDM</a></li>
            <li><a href="/news.html">What's new</a></li>
          </ul>
        </div>
        <div>
          <h2>Help</h2>
          <ul>
            <li><a href="/help/">User guide</a></li>
            <li><a href="/help/#tutorials">Tutorials</a></li>
            <li><a href="/faq/">FAQ</a></li>
            <li><a href="/help/troubleshooting.html">Troubleshooting</a></li>
            <li><a href="{REPO}/issues">Report a bug</a></li>
          </ul>
        </div>
        <div>
          <h2>Project</h2>
          <ul>
            <li><a href="{REPO}">Source code</a></li>
            <li><a href="/languages.html">Languages</a></li>
            <li><a href="{REPO}/blob/main/CONTRIBUTING.md">Contribute</a></li>
            <li><a href="/news.xml">News feed</a></li>
          </ul>
        </div>
        <div>
          <h2>Legal</h2>
          <ul>
            <li><a href="/privacy.html">Privacy</a></li>
            <li><a href="/terms.html">Terms of use</a></li>
            <li><a href="/license.html">License</a></li>
            <li><a href="/security.html">Security</a></li>
          </ul>
        </div>
      </div>
      <p class="legal">© {year} Ahmed Nefzaoui and contributors · Free software under the GNU GPL, version 3. Not affiliated with Tonec Inc.; Internet Download Manager and IDM are trademarks of their owner, named here only to compare.</p>
    </div>
  </footer>"""


THEME_SCRIPT = """<script>
    (function () {
      var d = document.documentElement;
      d.classList.remove('js-off');
      var q = /[?&]theme=(light|dark)/.exec(location.search);
      try {
        var t = q ? q[1] : localStorage.getItem('mdm-theme');
        if (t === 'light' || t === 'dark') d.setAttribute('data-theme', t);
      } catch (e) { if (q) d.setAttribute('data-theme', q[1]); }
    })();
  </script>"""


def head(path: str, title: str, desc: str, *, jsonld: list | None = None, og_type: str = "website",
         extra: str = "", noindex: bool = False) -> str:
    url = SITE + path
    ld = "".join(
        f'\n  <script type="application/ld+json">{json.dumps(x, ensure_ascii=False, separators=(",", ":"))}</script>'
        for x in (jsonld or [])
    )
    robots = '\n  <meta name="robots" content="noindex">' if noindex else ""
    return f"""<meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{esc(title)}</title>
  <meta name="description" content="{esc(desc)}">{robots}
  <meta name="theme-color" content="#f3f3f3" media="(prefers-color-scheme: light)">
  <meta name="theme-color" content="#1c1c1c" media="(prefers-color-scheme: dark)">
  <meta name="color-scheme" content="light dark">
  <link rel="canonical" href="{url}">
  <meta property="og:type" content="{og_type}">
  <meta property="og:site_name" content="{NAME}">
  <meta property="og:title" content="{esc(title)}">
  <meta property="og:description" content="{esc(desc)}">
  <meta property="og:url" content="{url}">
  <meta property="og:image" content="{SITE}/assets/social.png">
  <meta property="og:image:width" content="1280">
  <meta property="og:image:height" content="640">
  <meta property="og:image:alt" content="{NAME}: the download manager you already know, rebuilt to be faster">
  <meta name="twitter:card" content="summary_large_image">
  <link rel="icon" href="/favicon.ico" sizes="16x16 32x32 48x48 64x64">
  <link rel="icon" href="/assets/favicon-32.png" type="image/png" sizes="32x32">
  <link rel="apple-touch-icon" href="/assets/apple-touch-icon.png">
  <link rel="manifest" href="/site.webmanifest">
  <link rel="alternate" type="application/atom+xml" title="{NAME} news" href="/news.xml">
  <link rel="stylesheet" href="/styles.css">
  {THEME_SCRIPT}
  <script src="/main.js" defer></script>{ld}{extra}"""


def crumbs_ld(trail: list[tuple[str, str]]) -> dict:
    return {
        "@context": "https://schema.org", "@type": "BreadcrumbList",
        "itemListElement": [
            {"@type": "ListItem", "position": i + 1, "name": n, "item": SITE + p}
            for i, (n, p) in enumerate(trail)
        ],
    }


def crumbs_html(trail: list[tuple[str, str]]) -> str:
    items = []
    for i, (n, p) in enumerate(trail):
        last = i == len(trail) - 1
        items.append(f'<li aria-current="page">{esc(n)}</li>' if last else f'<li><a href="{p}">{esc(n)}</a></li>')
    return f'<ol class="crumbs" aria-label="Breadcrumb">{"".join(items)}</ol>'


def write(path: str, title: str, desc: str, body: str, *, section: str = "", jsonld: list | None = None,
          og_type: str = "website", priority: str = "0.6", noindex: bool = False, extra: str = "") -> None:
    doc = f"""<!doctype html>
<html lang="en" class="js-off">
<head>
  {head(path, title, desc, jsonld=jsonld, og_type=og_type, noindex=noindex, extra=extra)}
</head>
<body>
  {header(section)}
  <main id="main">
{body}
  </main>
  {footer()}
</body>
</html>
"""
    target = OUT / (path.lstrip("/") + ("index.html" if path.endswith("/") else ""))
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(doc, "utf-8", newline="\n")
    if not noindex:
        PAGES.append((path, priority))


def page_head(title: str, lede: str = "", trail: list[tuple[str, str]] | None = None, fine: str = "") -> str:
    c = crumbs_html(trail) if trail else ""
    l = f'\n        <p class="lede small">{lede}</p>' if lede else ""
    f = f'\n        <p class="fine">{fine}</p>' if fine else ""
    return f"""    <section class="page-head">
      <div class="wrap">
        {c}
        <h1>{title}</h1>{l}{f}
      </div>
    </section>"""


def cta_band(text: str = "Free, open source, no ads, no trial.") -> str:
    return f"""<div class="cta-band"><p>{text}</p>
          <a class="btn btn-primary" href="/download.html"><svg class="os-ico" aria-hidden="true"><use href="#os-windows"/></svg> Download {NAME}</a></div>"""


SOFTWARE_LD = {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    "name": NAME,
    "alternateName": "MDM",
    "applicationCategory": "UtilitiesApplication",
    "applicationSubCategory": "Download manager",
    "operatingSystem": "Windows 10, Windows 11, macOS (experimental), Linux (experimental)",
    "softwareVersion": VERSION,
    "offers": {"@type": "Offer", "price": "0", "priceCurrency": "USD"},
    "license": "https://www.gnu.org/licenses/gpl-3.0.html",
    "url": SITE + "/",
    "downloadUrl": SITE + "/download.html",
    "installUrl": FILES["setup"],
    "screenshot": SITE + "/assets/screens/main-light.webp",
    "image": SITE + "/assets/logo-640.png",
    "author": {"@type": "Person", "name": "Ahmed Nefzaoui", "url": "https://github.com/anefzaoui"},
    "codeRepository": REPO,
    "featureList": [
        "Up to 32 connections per download with dynamic re-splitting",
        "Crash-safe resume", "HTTP, HTTPS, FTP and FTPS", "BitTorrent and magnet links",
        "Video downloads from web pages (yt-dlp)", "Browser extension for Chrome, Edge, Brave, Opera, Vivaldi and Firefox",
        "Queues and scheduler", "Speed limiter", "86 interface languages", "Portable mode",
        "Command line and automation API",
    ],
    "inLanguage": "en",
    "isAccessibleForFree": True,
}


# ---- Help: docs/*.md -------------------------------------------------------------------------

def doc_link(target: str, here: Path) -> str:
    """A link inside a docs page, made to work on the site."""
    if re.match(r"^[a-z]+:", target) or target.startswith("#"):
        return target
    path, _, frag = target.partition("#")
    frag = "#" + frag if frag else ""
    if path.endswith(".md") and "/" not in path:
        return ("/help/" if path == "README.md" else f"/help/{path[:-3]}.html") + frag
    if path.startswith("images/"):
        return f"/help/{path}"
    resolved = (here.parent / path).resolve()
    try:
        rel = resolved.relative_to(ROOT).as_posix()
    except ValueError:
        return target
    kind = "tree" if resolved.is_dir() else "blob"
    return f"{REPO}/{kind}/main/{rel}{frag}"


def doc_sections() -> list[tuple[str, list[tuple[str, str]]]]:
    """docs/README.md's sections and their pages: the help sidebar."""
    out: list[tuple[str, list[tuple[str, str]]]] = []
    for line in (ROOT / "docs" / "README.md").read_text("utf-8").splitlines():
        if line.startswith("## "):
            out.append((line[3:].strip(), []))
        elif out:
            m = re.match(r"- \*\*\[([^\]]+)\]\(([^)#]+\.md)\)\*\*", line)
            if m and m.group(2) not in [p for _, p in out[-1][1]]:
                out[-1][1].append((m.group(1), m.group(2)))
    return [(s, pages) for s, pages in out if pages]


def build_help() -> None:
    sections = doc_sections()
    docs = sorted((ROOT / "docs").glob("*.md"))
    titles = {}
    for f in docs:
        first = f.read_text("utf-8").splitlines()[0]
        titles[f.name] = first.lstrip("# ").strip()
    used_images: set[str] = set()
    for f in docs:
        text = f.read_text("utf-8")
        text = re.sub(r"(?m)^\[← Back to docs index\]\(README\.md\)\s*\n", "", text)
        text = re.sub(r"(!?\[[^\]]*\])\(([^)\s]+)\)", lambda m: f"{m.group(1)}({doc_link(m.group(2), f)})", text)
        used_images.update(re.findall(r"/help/images/([\w./-]+)", text))
        lines = text.splitlines()
        h1 = lines[0].lstrip("# ").strip()
        rest = "\n".join(lines[1:])
        first_para = next((p for p in rest.split("\n\n") if p.strip() and not p.lstrip().startswith(("!", "#", "-", "|", ">"))), h1)
        is_index = f.name == "README.md"
        path = "/help/" if is_index else f"/help/{f.stem}.html"
        heading = "Help and tutorials" if is_index else re.sub(r"^Tutorial: (.)", lambda m: m.group(1).upper(), h1)
        title = (f"{NAME} help: user guide and tutorials" if is_index
                 else f"{heading} — {NAME} help")
        desc = clip(first_para)
        trail = [("Home", "/"), ("Help", "/help/")] + ([] if is_index else [(heading, path)])
        nav = ['<nav class="doc-nav" aria-label="Help pages">']
        for sec, pages in sections:
            nav.append(f"<p>{esc(sec)}</p>")
            for label, p in pages:
                href = f"/help/{p[:-3]}.html"
                cur = ' aria-current="page"' if p == f.name else ""
                nav.append(f'<a href="{href}"{cur}>{esc(label)}</a>')
        nav.append("</nav>")
        related = ""
        if not is_index:
            sib = next((pages for _, pages in sections if any(p == f.name for _, p in pages)), [])
            others = [(l, p) for l, p in sib if p != f.name][:5]
            if others:
                related = '<aside class="related"><h2>Related</h2><ul>' + "".join(
                    f'<li><a href="/help/{p[:-3]}.html">{esc(l)}</a></li>' for l, p in others) + "</ul></aside>"
        body = f"""{page_head(esc(heading), trail=trail)}
    <div class="wrap doc">
      {"".join(nav)}
      <article class="prose">
        {md(rest)}
        {related}
        {cta_band()}
      </article>
    </div>"""
        ld = [crumbs_ld(trail)]
        if not is_index:
            ld.append({"@context": "https://schema.org", "@type": "TechArticle", "headline": heading,
                       "description": desc, "inLanguage": "en", "dateModified": TODAY,
                       "about": {"@type": "SoftwareApplication", "name": NAME},
                       "author": {"@type": "Person", "name": "Ahmed Nefzaoui"}})
        write(path, title, desc, body, section="help", jsonld=ld, og_type="article",
              priority="0.7" if f.stem.startswith("guide-") or is_index else "0.6")
    dst = OUT / "help" / "images"
    for name in used_images:
        src = ROOT / "docs" / "images" / name
        if src.is_file():
            (dst / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst / name)


# ---- FAQ: website/content/faq.md ----------------------------------------------------------------

def slug(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")


def build_faq() -> None:
    text = (CONTENT / "faq.md").read_text("utf-8").replace("{{REPO}}", REPO)
    groups: list[tuple[str, list[tuple[str, str, str]]]] = []  # (group, [(q, slug, answer md)])
    q = None
    for block in re.split(r"(?m)^(#{2,3} .+)$", text)[1:]:
        if block.startswith("## "):
            groups.append((block[3:].strip(), []))
        elif block.startswith("### "):
            m = re.match(r"### (.+?)(?:\s*\{#([\w-]+)\})?$", block.strip())
            q = (m.group(1).strip(), m.group(2) or slug(m.group(1)))
        elif q:
            groups[-1][1].append((q[0], q[1], block.strip()))
            q = None
    allq = [x for _, qs in groups for x in qs]
    trail = [("Home", "/"), ("FAQ", "/faq/")]
    parts = []
    for g, qs in groups:
        parts.append(f'<h2 id="{slug(g)}">{esc(g)}</h2><ul class="faq-list">')
        parts += [f'<li><a href="/faq/{s}.html">{esc(question)}</a></li>' for question, s, _ in qs]
        parts.append("</ul>")
    ld = [crumbs_ld(trail), {
        "@context": "https://schema.org", "@type": "FAQPage",
        "mainEntity": [{"@type": "Question", "name": question,
                        "acceptedAnswer": {"@type": "Answer", "text": md(a)}} for question, _, a in allq],
    }]
    write("/faq/", f"Questions and answers — {NAME}",
          f"Answers about {NAME}: is it free, is it safe, how it compares with IDM, browsers, videos, torrents, speed, Mac and Linux, and more.",
          f"""{page_head("Questions and answers", "Short answers to what people ask most, each with a link to the full guide.", trail)}
    <div class="wrap narrow prose" style="padding:40px 24px 96px">
      {"".join(parts)}
      {cta_band()}
    </div>""", section="faq", jsonld=ld, priority="0.8")
    for gi, (g, qs) in enumerate(groups):
        for question, s, answer in qs:
            path = f"/faq/{s}.html"
            t = trail + [(question, path)]
            others = [x for x in qs if x[1] != s][:6]
            rel = '<aside class="related"><h2>More questions</h2><ul>' + "".join(
                f'<li><a href="/faq/{o[1]}.html">{esc(o[0])}</a></li>' for o in others) + "</ul></aside>"
            ld = [crumbs_ld(t), {"@context": "https://schema.org", "@type": "QAPage",
                                 "mainEntity": {"@type": "Question", "name": question, "answerCount": 1,
                                                "acceptedAnswer": {"@type": "Answer", "text": plain(md(answer))}}}]
            write(path, f"{question} — {NAME}", clip(md(answer)), f"""{page_head(esc(question), trail=t)}
    <div class="wrap narrow prose" style="padding:40px 24px 96px">
      {md(answer)}
      {rel}
      {cta_band()}
    </div>""", section="faq", jsonld=ld, og_type="article")


# ---- News: CHANGELOG.md -------------------------------------------------------------------------

def build_news() -> None:
    text = (ROOT / "CHANGELOG.md").read_text("utf-8")
    releases = []
    for m in re.finditer(r"(?ms)^## \[([^\]]+)\](?: - (\d{4}-\d{2}-\d{2}))?\s*\n(.*?)(?=^## |\Z)", text):
        ver, date, body = m.group(1), m.group(2), m.group(3).strip()
        body = re.sub(r"\]\((docs/[^)]+)\)", lambda x: "](" + doc_link(x.group(1).split("/", 1)[1], ROOT / "docs" / "x.md") + ")", body)
        releases.append((ver, date, body))
    trail = [("Home", "/"), ("News", "/news.html")]
    parts = []
    for ver, date, body in releases:
        when = f'<time datetime="{date}">{date}</time>' if date else '<time>Coming soon</time>'
        parts.append(f"""<section class="release" id="v{esc(ver)}">
        <h2>What's new in version {esc(ver)} {when}</h2>
        {md(body)}
        <p><a href="{REPO}/releases/tag/v{esc(ver)}">Release page and files on GitHub</a></p>
      </section>""")
    write("/news.html", f"What's new — {NAME} version history",
          f"Every {NAME} release and what it changed, newest first: new features, fixes and downloads.",
          f"""{page_head("What's new", f"Every release and what it changed, newest first. Follow along with the <a href=\"/news.xml\">news feed</a> or watch the project on GitHub.", trail)}
    <div class="wrap narrow prose" style="padding:40px 24px 96px">
      {"".join(parts)}
      {cta_band()}
    </div>""", section="news", jsonld=[crumbs_ld(trail)], priority="0.8")
    # Atom feed
    entries = []
    for ver, date, body in releases:
        updated = (date or TODAY) + "T00:00:00Z"
        entries.append(f"""  <entry>
    <title>{esc(NAME)} {esc(ver)}</title>
    <id>{SITE}/news.html#v{esc(ver)}</id>
    <link href="{SITE}/news.html#v{esc(ver)}"/>
    <updated>{updated}</updated>
    <content type="html">{esc(md(body))}</content>
  </entry>""")
    feed = f"""<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>{esc(NAME)} news</title>
  <id>{SITE}/news.xml</id>
  <link href="{SITE}/news.xml" rel="self"/>
  <link href="{SITE}/news.html"/>
  <updated>{TODAY}T00:00:00Z</updated>
  <author><name>Ahmed Nefzaoui</name></author>
{chr(10).join(entries)}
</feed>
"""
    (OUT / "news.xml").write_text(feed, "utf-8", newline="\n")


# ---- Languages: i18n/languages.tsv ---------------------------------------------------------------

def build_languages() -> None:
    rows = [l.split("\t") for l in (ROOT / "i18n" / "languages.tsv").read_text("utf-8").splitlines()
            if l and not l.startswith("#")]
    flags = ROOT / "crates" / "app" / "assets" / "flags"
    (OUT / "assets" / "flags").mkdir(parents=True, exist_ok=True)
    items = []
    for code, english, native, flag, *_ in sorted(rows, key=lambda r: r[1]):
        src = flags / f"{flag}.svg"
        img = ""
        if src.is_file():
            shutil.copy2(src, OUT / "assets" / "flags" / src.name)
            img = f'<img src="/assets/flags/{flag}.svg" width="24" height="18" alt="" loading="lazy">'
        tsv = f"{REPO}/blob/main/i18n/{code}.tsv"
        items.append(f'<li>{img}<span><a href="{tsv}" lang="{esc(code)}">{esc(native)}</a><small>{esc(english)}</small></span></li>')
    trail = [("Home", "/"), ("Languages", "/languages.html")]
    body = f"""{page_head(f"{NAME} in {len(rows)} languages", "Every display language Windows 11 offers, chosen by flag on the first start or any time in View → Language. Arabic, Hebrew, Persian, Urdu and Uyghur are laid out right to left.", trail)}
    <div class="wrap prose" style="padding:40px 24px 96px;max-width:1180px">
      <h2>Every language</h2>
      <ul class="lang-grid">{"".join(items)}</ul>
      <h2>Help translate</h2>
      <p>Most translations were made by machine and are waiting for a native speaker. If MDM speaks your language a little oddly, you can fix it in a few minutes, no programming needed:</p>
      <ol>
        <li>Click your language above: it opens its file on GitHub, one text per row.</li>
        <li>Correct the translation in the third column and delete the row's <code>#, machine</code> line.</li>
        <li>Propose the change with GitHub's pencil button. Your name goes into the release notes.</li>
      </ol>
      <p>The full guide is in <a href="{REPO}/blob/main/CONTRIBUTING.md">CONTRIBUTING.md</a>, and <a href="/help/languages.html">Languages</a> in the help explains the picker and right-to-left layouts.</p>
      {cta_band()}
    </div>"""
    write("/languages.html", f"{NAME} in {len(rows)} languages — translations",
          f"{NAME} speaks {len(rows)} languages, every one Windows 11 offers, right to left included. See them all and help improve your own.",
          body, jsonld=[crumbs_ld(trail)], priority="0.6")


# ---- Pages written as Markdown (legal) -----------------------------------------------------------

def front(text: str) -> tuple[dict, str]:
    m = re.match(r"(?s)^---\n(.*?)\n---\n(.*)$", text)
    if not m:
        return {}, text
    meta = dict(l.split(": ", 1) for l in m.group(1).splitlines() if ": " in l)
    return meta, m.group(2)


def build_content_pages() -> None:
    for f in sorted(CONTENT.glob("*.md")):
        if f.name == "faq.md":
            continue
        raw = f.read_text("utf-8").replace("{{VERSION}}", VERSION).replace("{{REPO}}", REPO)
        meta, text = front(raw.replace("{{TODAY}}", TODAY))
        if "{{THIRD_PARTY}}" in text:
            text = text.replace("{{THIRD_PARTY}}", third_party_table())
        path = f"/{f.stem}.html"
        trail = [("Home", "/"), (meta["title"], path)]
        body = f"""{page_head(esc(meta["title"]), meta.get("lede", ""), trail, meta.get("fine", ""))}
    <div class="wrap narrow prose" style="padding:40px 24px 96px">
      {md(text)}
    </div>"""
        write(path, f'{meta["title"]} — {NAME}', meta["description"], body,
              jsonld=[crumbs_ld(trail)], priority=meta.get("priority", "0.3"))


def third_party_table() -> str:
    rows = [l.split("\t") for l in (CONTENT / "third-party.tsv").read_text("utf-8").splitlines()
            if l and not l.startswith("#")]
    body = "".join(
        f'<tr><td>{f"<a href={chr(34)}{esc(r[3])}{chr(34)}>{esc(r[0])}</a>" if len(r) > 3 and r[3] else esc(r[0])}</td>'
        f"<td>{esc(r[1])}</td><td>{esc(r[2])}</td></tr>" for r in rows)
    return (f'<div class="table-wrap"><table class="crates"><thead><tr><th>Library</th><th>Version</th>'
            f'<th>Licence</th></tr></thead><tbody>{body}</tbody></table></div>\n\n'
            f'{len(rows)} libraries.')


# ---- Hand-written pages: the home page, download, comparison, 404 --------------------------------

def fill(text: str) -> str:
    for k, v in FILES.items():
        text = text.replace("{{FILE_" + k.upper() + "}}", v)
    return (text.replace("{{VERSION}}", VERSION).replace("{{REPO}}", REPO).replace("{{SITE}}", SITE)
            .replace("{{SOFTWARE_LD}}", json.dumps(SOFTWARE_LD, ensure_ascii=False, separators=(",", ":"))))


def build_templated(name: str, path: str, section: str, priority: str) -> None:
    """website/<name>: a full page whose <!--@header--> and <!--@footer--> are filled in."""
    text = fill((WEB / name).read_text("utf-8"))
    text = text.replace("<!--@header-->", header(section)).replace("<!--@footer-->", footer())
    text = text.replace("<!--@theme-->", THEME_SCRIPT)
    target = OUT / (path.lstrip("/") or "index.html")
    if path.endswith("/"):
        target = OUT / path.lstrip("/") / "index.html"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, "utf-8", newline="\n")
    if name != "404.html":
        PAGES.append((path, priority))


# ---- Static files, sitemap, robots ------------------------------------------------------------

def copy_static() -> None:
    # Emptied rather than removed: a preview server may be running in it.
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.iterdir():
        shutil.rmtree(old) if old.is_dir() else old.unlink()
    for item in WEB.iterdir():
        if item.name in ("content",) or item.suffix == ".html":
            continue
        if item.is_dir():
            shutil.copytree(item, OUT / item.name)
        else:
            shutil.copy2(item, OUT / item.name)
    shutil.copy2(ROOT / "docs" / "images" / "social-preview.png", OUT / "assets" / "social.png")
    (OUT / "CNAME").write_text("misterdownloadmanager.com\n", "utf-8")
    (OUT / ".nojekyll").write_text("", "utf-8")
    (OUT / "site.webmanifest").write_text(json.dumps({
        "name": NAME, "short_name": "MDM", "start_url": "/", "display": "browser",
        "background_color": "#f3f3f3", "theme_color": "#0f7b0f",
        "icons": [{"src": "/assets/logo-640.png", "sizes": "640x640", "type": "image/png"},
                  {"src": "/assets/apple-touch-icon.png", "sizes": "180x180", "type": "image/png"}],
    }, indent=2), "utf-8")


def finish() -> None:
    urls = "\n".join(
        f"  <url><loc>{SITE}{p}</loc><lastmod>{TODAY}</lastmod><priority>{pr}</priority></url>"
        for p, pr in sorted(PAGES, key=lambda x: (-float(x[1]), x[0])))
    (OUT / "sitemap.xml").write_text(
        f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n{urls}\n</urlset>\n',
        "utf-8", newline="\n")
    (OUT / "robots.txt").write_text(f"User-agent: *\nAllow: /\n\nSitemap: {SITE}/sitemap.xml\n", "utf-8")


def main() -> None:
    copy_static()
    build_templated("index.html", "/", "home", "1.0")
    build_templated("download.html", "/download.html", "download", "0.9")
    build_templated("idm-alternative.html", "/idm-alternative.html", "compare", "0.9")
    build_templated("404.html", "/404.html", "", "0")
    build_help()
    build_faq()
    build_news()
    build_languages()
    build_content_pages()
    finish()
    print(f"{len(PAGES)} pages -> {OUT}")
    if "--serve" in sys.argv:
        import functools
        import http.server
        handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(OUT))
        print("http://127.0.0.1:8765/")
        http.server.ThreadingHTTPServer(("127.0.0.1", 8765), handler).serve_forever()


if __name__ == "__main__":
    main()

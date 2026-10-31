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

The repository's stars, forks and open issues are read from GitHub's API while building (with
GITHUB_TOKEN when set), so visitors' browsers never call GitHub; offline, the numbers are left
out. Every page carries the Statcounter visit counter; MDM_SITE_NO_STATS=1 leaves it out of a
local build so previews aren't counted.

Needs: pip install markdown
"""
from __future__ import annotations

import datetime
import html
import json
import os
import re
import shutil
import sys
import urllib.parse
import urllib.request
from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parents[2]
WEB = ROOT / "website"
CONTENT = WEB / "content"
OUT = ROOT / "target" / "site"
SITE = "https://misterdownloadmanager.com"
REPO = "https://github.com/anefzaoui/misterdownloadmanager"
NAME = "Mister Download Manager"
CARGO_VERSION = re.search(r'(?m)^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text("utf-8")).group(1)


def github(path: str):
    """GET api.github.com/<path> as JSON, or None (offline, rate-limited)."""
    req = urllib.request.Request(f"https://api.github.com/{path}", headers={
        "Accept": "application/vnd.github+json", "User-Agent": "misterdownloadmanager-site"})
    if os.environ.get("GITHUB_TOKEN"):
        req.add_header("Authorization", f"Bearer {os.environ['GITHUB_TOKEN']}")
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            return json.load(r)
    except Exception as e:  # noqa: BLE001 — the site builds without the numbers
        print(f"github {path}: {e}", file=sys.stderr)
        return None


def published_version() -> str:
    """The latest published release's version: what the download links and the version shown
    point at. Cargo.toml's is ahead of it between a merged version bump and its release's
    files (release.yml rebuilds the site once they are published); used offline."""
    latest = github(f"repos/{REPO.split('github.com/')[1]}/releases/latest")
    return (latest or {}).get("tag_name", "").lstrip("v") or CARGO_VERSION


VERSION = published_version()
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
# For llms.txt and llms-full.txt: the help pages as Markdown (path, title, description, text)
# and the FAQ (question, path, answer), as the builders read them.
DOCS_MD: list[tuple[str, str, str, str]] = []
FAQ_MD: list[tuple[str, str, str]] = []

# The visit counter (privacy.md says what it records). Statcounter's own code, as given.
STATCOUNTER = """<!-- Default Statcounter code for Mister Download Manager
https://misterdownloadmanager.com/ -->
<script type="text/javascript">
var sc_project=13364153;
var sc_invisible=1;
var sc_security="c94415aa";
</script>
<script type="text/javascript"
src="https://www.statcounter.com/counter/counter.js"
async></script>
<noscript><div class="statcounter"><a title="Web Analytics"
href="https://statcounter.com/" target="_blank"><img
class="statcounter"
src="https://c.statcounter.com/13364153/0/c94415aa/1/"
alt="Web Analytics"
referrerPolicy="no-referrer-when-downgrade"></a></div></noscript>
<!-- End of Statcounter Code -->"""


def stats_code() -> str:
    return "" if os.environ.get("MDM_SITE_NO_STATS") == "1" else STATCOUNTER


def short_num(n: int) -> str:
    return f"{n / 1000:.1f}k".replace(".0k", "k") if n >= 1000 else str(n)


def repo_stats() -> dict[str, str]:
    """The repository's numbers for the header pill and the open-source card; empty offline."""
    slug_ = REPO.split("github.com/")[1]
    repo = github(f"repos/{slug_}")
    if not repo:
        return {}
    issues = github(f"search/issues?q=repo:{slug_}+type:issue+state:open&per_page=1")
    out = {
        "stars": short_num(repo.get("stargazers_count", 0)),
        "forks": short_num(repo.get("forks_count", 0)),
        "watchers": short_num(repo.get("subscribers_count", 0)),
    }
    if issues is not None:
        out["issues"] = short_num(issues.get("total_count", 0))
    return out


STATS: dict[str, str] = {}


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
  <symbol id="i-star" viewBox="0 0 24 24"><path d="M12 2.8l2.84 5.76 6.36.92-4.6 4.49 1.08 6.33L12 17.31l-5.68 2.99 1.08-6.33-4.6-4.49 6.36-.92z"/></symbol>
  <symbol id="i-fork" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M6 3v4a4 4 0 0 0 4 4h4a4 4 0 0 0 4-4V3M12 11v10"/><circle cx="6" cy="4" r="2"/><circle cx="18" cy="4" r="2"/><circle cx="12" cy="20" r="2"/></symbol>
  <symbol id="i-issue" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="12" cy="12" r="2.2"/></symbol>
  <symbol id="i-chevron" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" d="M6 9l6 6 6-6"/></symbol>
  <symbol id="i-out" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M7 17L17 7M9 7h8v8"/></symbol>
  <symbol id="i-edit" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"/></symbol>
  <symbol id="i-code" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M8 7l-5 5 5 5M16 7l5 5-5 5M13.5 4l-3 16"/></symbol>
  <symbol id="i-heart" viewBox="0 0 24 24"><path d="M12 20.5s-8.5-5-8.5-11A4.5 4.5 0 0 1 12 6.6a4.5 4.5 0 0 1 8.5 2.9c0 6-8.5 11-8.5 11z"/></symbol>
  <symbol id="i-tag" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" d="M3 12V4h8l10 10-8 8z"/><circle cx="7.5" cy="8.5" r="1.5"/></symbol>
  <symbol id="i-moon" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/></symbol>
  <symbol id="i-sun" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></g></symbol>
  <symbol id="p-https" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5M12 14.5v2.5"/></g></symbol>
  <symbol id="p-ftp" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7h6l2 2h10v10H3z"/><path d="M12 16v-5m-2.5 2.5L12 11l2.5 2.5"/></g></symbol>
  <symbol id="p-torrent" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="5" r="2.2"/><circle cx="5" cy="17" r="2.2"/><circle cx="19" cy="17" r="2.2"/><path d="M10.8 6.9 6.2 15M13.2 6.9 17.8 15M7.2 17h9.6"/></g></symbol>
  <symbol id="p-magnet" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 4v8a6 6 0 0 0 12 0V4h-4v8a2 2 0 0 1-4 0V4z"/><path d="M6 8h4M14 8h4"/></g></symbol>
  <symbol id="p-proxy" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8-8 9-4.6-1-8-4.5-8-9V6z"/><path d="M9 12h6m-2-2 2 2-2 2"/></g></symbol>
  <symbol id="p-pac" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M8 4c-2 0-2 1.5-2 3s0 3-2 5c2 2 2 3.5 2 5s0 3 2 3M16 4c2 0 2 1.5 2 3s0 3 2 5c-2 2-2 3.5-2 5s0 3-2 3"/></g></symbol>
  <symbol id="p-import" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v11m-4-4 4 4 4-4"/><path d="M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4"/></g></symbol>
</svg>"""

NAV = [
    ("Features", "/#features", "home"),
    ("Download", "/download.html", "download"),
    ("Help", "/help/", "help"),
    ("FAQ", "/faq/", "faq"),
    ("Benchmark", "/benchmark.html", "bench"),
    ("News", "/news.html", "news"),
]


def gh_pill(cls: str = "gh-pill") -> str:
    """The GitHub link with its star count (when known): header, hero, open-source card."""
    stars = STATS.get("stars")
    count = f'<span class="gh-count">{stars}</span>' if stars else ""
    return (f'<a class="{cls}" href="{REPO}" aria-label="Mister Download Manager on GitHub'
            f'{f", {stars} stars" if stars else ""}">'
            f'<svg aria-hidden="true"><use href="#i-github"/></svg><span class="gh-label">GitHub</span>{count}</a>')


def os_menu() -> str:
    """The split download button's list: every system and file (hero, final call)."""
    rows = [
        ("windows", "Windows installer", "setup.exe · Windows 10 and 11", FILES["setup"]),
        ("windows", "Windows portable", "portable.zip · runs from a folder or USB stick", FILES["portable"]),
        ("macos", "macOS", ".dmg · Apple Silicon and Intel · experimental", FILES["dmg"]),
        ("linux", "Linux AppImage", "any distribution · experimental", FILES["appimage"]),
        ("linux", "Linux .deb / .rpm", "Debian, Ubuntu · Fedora, openSUSE", "/download.html#linux"),
    ]
    items = "".join(
        f'<li><a href="{href}"><svg class="os-ico" aria-hidden="true"><use href="#os-{os_}"/></svg>'
        f'<span><b>{label}</b><small>{sub}</small></span></a></li>' for os_, label, sub, href in rows)
    return (f'<ul class="split-menu" role="list">{items}'
            f'<li class="split-all"><a href="/download.html">All files, extensions and checksums →</a></li></ul>')


def split_button(extra_class: str = "") -> str:
    """Download for <your system>, with the other systems under its arrow."""
    return f"""<div class="split{extra_class}">
          <a class="btn btn-primary split-main" href="/download.html" data-os-cta>
            <svg class="os-ico" aria-hidden="true"><use href="#os-windows" data-os-cta-icon/></svg>
            <span data-os-label>Download for Windows</span>
          </a>
          <details class="split-more">
            <summary class="btn btn-primary" aria-label="Other systems and files"><svg aria-hidden="true"><use href="#i-chevron"/></svg></summary>
            {os_menu()}
          </details>
        </div>"""


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
        <a class="btn btn-ghost top-contrib" href="/contributing.html">Contribute</a>
        {gh_pill()}
        <a class="btn btn-primary top-dl" href="/download.html">Download</a>
        <button class="theme" type="button" aria-label="Switch between light and dark" title="Light / dark">
          <svg class="for-light-i" aria-hidden="true"><use href="#i-moon"/></svg><svg class="for-dark-i" aria-hidden="true"><use href="#i-sun"/></svg>
        </button>
        <details class="menu">
          <summary class="theme" aria-label="Menu"><svg aria-hidden="true" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M4 7h16M4 12h16M4 17h16"/></svg></summary>
          <nav class="menu-panel" aria-label="Main">
            {links}
            <a href="/languages.html">Languages</a>
            <a href="/contributing.html">Contribute</a>
            <a class="btn btn-primary" href="/download.html">Download</a>
          </nav>
        </details>
      </div>
    </div>
  </header>"""


def footer() -> str:
    year = datetime.date.today().year
    return f"""<footer class="foot">
    <div class="wrap">
      <div class="foot-cols">
        <div class="foot-brand">
          <a class="brand" href="/"><img src="/assets/logo-64.png" width="28" height="28" alt=""><span>{NAME}</span></a>
          <p class="foot-tag">The download manager you already know, rebuilt to be faster. Free, open source and written from scratch in Rust.</p>
          <a class="btn btn-ghost btn-small foot-star" href="{REPO}"><svg class="os-ico" aria-hidden="true"><use href="#i-star"/></svg> Star it on GitHub</a>
        </div>
        <div>
          <h2>Product</h2>
          <ul>
            <li><a href="/download.html">Download</a></li>
            <li><a href="/download.html#windows">Windows</a></li>
            <li><a href="/download.html#macos">macOS</a></li>
            <li><a href="/download.html#linux">Linux</a></li>
            <li><a href="/download.html#extension">Browser extension</a></li>
            <li><a href="/#features">Features</a></li>
            <li><a href="/benchmark.html">Benchmark</a></li>
            <li><a href="/idm-alternative.html">MDM vs IDM</a></li>
          </ul>
        </div>
        <div>
          <h2>Help</h2>
          <ul>
            <li><a href="/help/">User guide</a></li>
            <li><a href="/help/#tutorials">Tutorials</a></li>
            <li><a href="/faq/">FAQ</a></li>
            <li><a href="/help/troubleshooting.html">Troubleshooting</a></li>
            <li><a href="/help/keyboard-shortcuts.html">Keyboard shortcuts</a></li>
            <li><a href="/languages.html">Languages</a></li>
          </ul>
        </div>
        <div>
          <h2>Project</h2>
          <ul>
            <li><a href="{REPO}">Source code</a></li>
            <li><a href="{REPO}/issues">Issues</a></li>
            <li><a href="{REPO}/issues/new?template=bug.yml">Report a bug</a></li>
            <li><a href="{REPO}/releases">Releases</a></li>
            <li><a href="/news.html">What's new</a></li>
            <li><a href="/contributing.html">Contributing</a></li>
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
      <div class="foot-bottom">
        <p>GPL-3.0 · built in the open on <a href="{REPO}">GitHub</a></p>
        <p>© {year} Ahmed Nefzaoui and contributors</p>
      </div>
      <p class="legal">Not affiliated with Tonec Inc.; Internet Download Manager and IDM are trademarks of their owner, named here only to compare.</p>
    </div>
    <div class="foot-mark" aria-hidden="true">MDM</div>
  </footer>
  {stats_code()}"""


# In every head (templated pages get it through <!--@theme-->): the font, then the theme before
# the first paint.
THEME_SCRIPT = """<link rel="preload" href="/vendor/inter/InterVariable.woff2" as="font" type="font/woff2" crossorigin>
  <script>
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
  <meta name="theme-color" content="#ffffff" media="(prefers-color-scheme: light)">
  <meta name="theme-color" content="#0a0a0b" media="(prefers-color-scheme: dark)">
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


def page_tools(src: str, path: str, title: str) -> str:
    """Under an article written in the repository: edit its source, or report a problem with it."""
    q = urllib.parse.quote
    # The Website problem form (.github/ISSUE_TEMPLATE/website.yml), its page filled in.
    issue = f"{REPO}/issues/new?template=website.yml&title={q('Website: ' + title)}&page={q(SITE + path)}"
    return f"""<div class="page-tools">
          <a href="{REPO}/edit/main/{src}"><svg aria-hidden="true"><use href="#i-edit"/></svg>Edit this page on GitHub</a>
          <a href="{issue}"><svg aria-hidden="true"><use href="#i-issue"/></svg>Report a problem with this page</a>
          <a href="{REPO}/commits/main/{src}"><svg aria-hidden="true"><use href="#i-code"/></svg>Page history</a>
        </div>"""


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
        "Command line and automation API", "MCP server for AI assistants",
    ],
    "inLanguage": "en",
    "isAccessibleForFree": True,
}


# ---- Help: docs/*.md -------------------------------------------------------------------------

# Repository files that have a page here: links to them stay on the site.
ON_SITE = {
    "CONTRIBUTING.md": "/contributing.html",
    "SECURITY.md": "/security.html",
    "LICENSE": "/license.html",
    "CHANGELOG.md": "/news.html",
    "docs/bench/run5/REPORT.md": "/benchmark.html#report",
    "docs/bench": "/benchmark.html",
    "docs/automation-api.schema.json": "/automation-api.schema.json",
}


def doc_link(target: str, here: Path) -> str:
    """A link inside a docs page, made to work on the site."""
    if re.match(r"^[a-z]+:", target) or target.startswith("#"):
        return target
    path, _, frag = target.partition("#")
    frag = "#" + frag if frag else ""
    if path.endswith(".md") and "/" not in path and here.parent == ROOT / "docs":
        return ("/help/" if path == "README.md" else f"/help/{path[:-3]}.html") + frag
    if path.startswith("images/"):
        return f"/help/{path}"
    resolved = (here.parent / path).resolve()
    try:
        rel = resolved.relative_to(ROOT).as_posix()
    except ValueError:
        return target
    if rel in ON_SITE:
        return ON_SITE[rel] + frag
    if rel.startswith("docs/") and rel.endswith(".md") and rel.count("/") == 1:
        return doc_link(rel[5:], ROOT / "docs" / "x.md") + frag
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
        {page_tools(f"docs/{f.name}", path, heading)}
        {cta_band()}
      </article>
    </div>"""
        md_path = "/help/index.md" if is_index else f"/help/{f.stem}.md"
        text_md = re.sub(r"\]\((/[^)\s]*)\)", lambda m: f"]({SITE}{m.group(1)})", f"# {heading}\n{rest}")
        (OUT / md_path.lstrip("/")).parent.mkdir(parents=True, exist_ok=True)
        (OUT / md_path.lstrip("/")).write_text(text_md.strip() + "\n", "utf-8", newline="\n")
        DOCS_MD.append((md_path, heading, desc, text_md))
        ld = [crumbs_ld(trail)]
        if not is_index:
            ld.append({"@context": "https://schema.org", "@type": "TechArticle", "headline": heading,
                       "description": desc, "inLanguage": "en", "dateModified": TODAY,
                       "about": {"@type": "SoftwareApplication", "name": NAME},
                       "author": {"@type": "Person", "name": "Ahmed Nefzaoui"}})
        write(path, title, desc, body, section="help", jsonld=ld, og_type="article",
              priority="0.7" if f.stem.startswith("guide-") or is_index else "0.6",
              extra=f'\n  <link rel="alternate" type="text/markdown" href="{md_path}">')
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
    FAQ_MD.extend((question, f"/faq/{s}.html", a) for question, s, a in allq)
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
      {page_tools("website/content/faq.md", "/faq/", "Questions and answers")}
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
      {page_tools("website/content/faq.md", path, question)}
      {cta_band()}
    </div>""", section="faq", jsonld=ld, og_type="article")


# ---- News: CHANGELOG.md -------------------------------------------------------------------------

def build_news() -> None:
    text = (ROOT / "CHANGELOG.md").read_text("utf-8")
    releases = []
    for m in re.finditer(r"(?ms)^## \[([^\]]+)\](?: - (\d{4}-\d{2}-\d{2}))?\s*\n(.*?)(?=^## |\Z)", text):
        ver, date, body = m.group(1), m.group(2), m.group(3).strip()
        # Changes not released yet aren't news.
        if ver.lower() == "unreleased":
            continue
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
      {page_tools("CHANGELOG.md", "/news.html", "What's new")}
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
      <p>The full guide is in the <a href="/contributing.html">contributing guide</a>, and <a href="/help/languages.html">Languages</a> in the help explains the picker and right-to-left layouts.</p>
      {cta_band()}
    </div>"""
    write("/languages.html", f"{NAME} in {len(rows)} languages — translations",
          f"{NAME} speaks {len(rows)} languages, every one Windows 11 offers, right to left included. See them all and help improve your own.",
          body, jsonld=[crumbs_ld(trail)], priority="0.6")


# ---- Repository documents shown as pages --------------------------------------------------------

def build_repo_pages() -> None:
    """CONTRIBUTING.md as /contributing.html, its links kept working on the site."""
    f = ROOT / "CONTRIBUTING.md"
    text = f.read_text("utf-8")
    text = re.sub(r"(!?\[[^\]]*\])\(([^)\s]+)\)", lambda m: f"{m.group(1)}({doc_link(m.group(2), f)})", text)
    lines = text.splitlines()
    # The page's lede says what the file's first paragraph does.
    rest = "\n".join(lines[1:]).strip().split("\n\n", 1)[1]
    path = "/contributing.html"
    trail = [("Home", "/"), ("Contributing", path)]
    lede = ("Bug reports, ideas, translation fixes and pull requests are all welcome. "
            f'Everything happens in the open on <a href="{REPO}">GitHub</a>.')
    body = f"""{page_head("Contribute to Mister Download Manager", lede, trail)}
    <div class="wrap narrow prose" style="padding:40px 24px 96px">
      <div class="contrib-ways">
        <a href="{REPO}/issues/new?template=bug.yml"><b>Report a bug</b><span>Something broken or odd? Tell us what you saw.</span></a>
        <a href="/languages.html"><b>Fix a translation</b><span>Five minutes on GitHub, no programming.</span></a>
        <a href="{REPO}/issues"><b>Pick an issue</b><span>See what's open and lend a hand.</span></a>
        <a href="{REPO}"><b>Star the project</b><span>It helps other people find it.</span></a>
      </div>
      {md(rest)}
      {page_tools("CONTRIBUTING.md", path, "Contributing")}
    </div>"""
    write(path, f"Contribute — {NAME}",
          "How to help Mister Download Manager: report bugs, fix translations, build it from source and send pull requests. The checks CI runs and the project's rules.",
          body, jsonld=[crumbs_ld(trail)], priority="0.5")


def bench_report() -> str:
    """docs/bench/run5/REPORT.md's table, for the benchmark page."""
    text = (ROOT / "docs" / "bench" / "run5" / "REPORT.md").read_text("utf-8")
    table = "\n".join(l for l in text.splitlines() if l.startswith("|"))
    env = next((l for l in text.splitlines()[1:] if l.strip()), "")
    return md(table).replace("<table>", '<div class="table-wrap"><table>').replace("</table>", "</table></div>") + \
        f'<p class="fine">{esc(env)}</p>'


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
      {page_tools(f"website/content/{f.name}", path, meta["title"])}
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
    if "{{BENCH_REPORT}}" in text:
        text = text.replace("{{BENCH_REPORT}}", bench_report())
    for k in ("stars", "forks", "issues", "watchers"):
        text = text.replace("{{" + k.upper() + "}}", STATS.get(k, "–"))
    text = text.replace("<!--@split-->", split_button()).replace("<!--@gh-->", gh_pill("btn btn-ghost btn-big gh-big"))
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


# ---- For AI assistants and search: llms.txt, llms-full.txt (llmstxt.org) ----------------------

def build_llms() -> None:
    """llms.txt: website/llms.txt (what MDM is, the main pages) and, made here, every help page
    (as Markdown) and question. llms-full.txt: all of it in one file. Built from the same
    sources as the pages, so it never drifts from them."""
    head_md = (WEB / "llms.txt").read_text("utf-8").strip()
    order = [p for _, pages in doc_sections() for _, p in pages]
    rank = {f"/help/{p[:-3]}.md": i for i, p in enumerate(order)}
    docs = sorted((d for d in DOCS_MD if d[0] in rank), key=lambda d: rank[d[0]])
    lines = [head_md, "", "## Help pages", ""]
    lines += [f"- [{t}]({SITE}{p}): {d}" for p, t, d, _ in docs]
    lines += ["", "## Questions and answers", ""]
    lines += [f"- [{q}]({SITE}{p})" for q, p, _ in FAQ_MD]
    lines += ["", "## Optional", "",
              f"- [Everything above in one file]({SITE}/llms-full.txt): the help pages and answers, as Markdown"]
    if (OUT / "automation-api.schema.json").is_file():
        lines.append(f"- [Automation API schema]({SITE}/automation-api.schema.json): JSON Schema of the local "
                     "channel other programs use to list, add and control downloads")
    (OUT / "llms.txt").write_text("\n".join(lines) + "\n", "utf-8", newline="\n")
    full = [head_md, ""]
    for p, _, _, text in docs:
        full += ["", "---", "", f"Source: {SITE}{p}", "", text.strip(), ""]
    full += ["", "---", "", "# Questions and answers", ""]
    for q, p, a in FAQ_MD:
        a = re.sub(r"\]\((/[^)\s]*)\)", lambda m: f"]({SITE}{m.group(1)})", a.replace("{{REPO}}", REPO))
        full += [f"## {q}", "", f"Source: {SITE}{p}", "", a.strip(), ""]
    (OUT / "llms-full.txt").write_text("\n".join(full) + "\n", "utf-8", newline="\n")


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
    # The automation API's JSON Schema, at the address its "$id" names.
    schema = ROOT / "docs" / "automation-api.schema.json"
    if schema.is_file():
        shutil.copy2(schema, OUT / "automation-api.schema.json")
    (OUT / "CNAME").write_text("misterdownloadmanager.com\n", "utf-8")
    (OUT / ".nojekyll").write_text("", "utf-8")
    (OUT / "site.webmanifest").write_text(json.dumps({
        "name": NAME, "short_name": "MDM", "start_url": "/", "display": "browser",
        "background_color": "#ffffff", "theme_color": "#0f7b0f",
        "icons": [{"src": "/assets/logo-640.png", "sizes": "640x640", "type": "image/png"},
                  {"src": "/assets/apple-touch-icon.png", "sizes": "180x180", "type": "image/png"}],
    }, indent=2), "utf-8")


def bust_caches() -> None:
    """Every page asks for /styles.css, /main.js and /bench.js by their content hash, so a
    browser never pairs a new page with an old cached stylesheet."""
    import hashlib
    tags = {}
    for name in ("styles.css", "main.js", "bench.js"):
        tags[name] = hashlib.sha256((OUT / name).read_bytes()).hexdigest()[:10]
    for page in OUT.rglob("*.html"):
        text = page.read_text("utf-8")
        new = text
        for name, h in tags.items():
            new = new.replace(f'"/{name}"', f'"/{name}?v={h}"')
        if new != text:
            page.write_text(new, "utf-8", newline="\n")


def finish() -> None:
    bust_caches()
    urls = "\n".join(
        f"  <url><loc>{SITE}{p}</loc><lastmod>{TODAY}</lastmod><priority>{pr}</priority></url>"
        for p, pr in sorted(PAGES, key=lambda x: (-float(x[1]), x[0])))
    (OUT / "sitemap.xml").write_text(
        f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n{urls}\n</urlset>\n',
        "utf-8", newline="\n")
    (OUT / "robots.txt").write_text(f"User-agent: *\nAllow: /\n\nSitemap: {SITE}/sitemap.xml\n", "utf-8")


def main() -> None:
    STATS.update(repo_stats())
    copy_static()
    build_templated("index.html", "/", "home", "1.0")
    build_templated("download.html", "/download.html", "download", "0.9")
    build_templated("benchmark.html", "/benchmark.html", "bench", "0.8")
    build_templated("idm-alternative.html", "/idm-alternative.html", "compare", "0.9")
    build_templated("404.html", "/404.html", "", "0")
    build_help()
    build_faq()
    build_news()
    build_languages()
    build_repo_pages()
    build_content_pages()
    build_llms()
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

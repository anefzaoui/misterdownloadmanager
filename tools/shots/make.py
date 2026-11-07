#!/usr/bin/env python3
"""The help pages' pictures, made from one shot list (tools/shots/shots.toml, issue #228).

Every picture in docs/images/help/ comes from here, never by hand: the list says which window
or state to open (through the app's own `/show <window>` command line), what to keep of it (the
whole window, a rectangle, or the area around some controls), what to point at (a ring round a
control, numbered markers), and where it goes (page, heading, alt text and caption). One
command takes them all again, light and dark:

    python tools/shots/make.py                 # shoot (Windows) + build every picture
    python tools/shots/make.py --only add-url,scheduler
    python tools/shots/make.py build           # again from the last shoot's raw pictures
    python tools/shots/make.py check           # the list, the pages and the files agree
    python tools/shots/make.py snippet add-url # the Markdown for one picture
    python tools/shots/make.py docs <run>      # docs/images/*.webp from a Docs screenshots run

`shoot` runs tools/lab/docs-shots.ps1 twice (light, then dark) with the list's windows: the
app's own instance with its own data folder and download folder (MDM_DATA_DIR), off the screens
(MDM_OFFSCREEN=1), the lab server's neutral files, nothing clicked or typed. Each window's
controls come with it (tools/lab/ui-rects.ps1, the app's accessibility tree), so the list names
controls ("Start Download") instead of pixels, and a moved button needs no edit here. The raw
pictures stay in target/shots/raw/.

`check` also keeps the budget of every picture in docs/images and website/assets (#186): none
over 300 KB, no PNG where lossless WebP belongs, none that nothing shows. `docs` writes the
README's and the docs pages' own pictures (docs/images/<name>.webp) from a Docs screenshots run
(.github/workflows/docs-shots.yml's artifact) or single PNGs.

`build` crops, annotates (drawn 4x larger, then scaled down: smooth edges) and writes lossless
WebP: `cwebp -lossless -z 9 -metadata none` when cwebp is on PATH, Pillow's lossless WebP
(method 6) otherwise, the same pixels either way. <id>.webp is the light picture, <id>-dark.webp
the dark one. The site (tools/website/build.py) turns them into a <picture> that follows the
visitor's theme; on GitHub the Markdown shows the light one.

Needs: pip install pillow (Python 3.11+ for tomllib).
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LIST = ROOT / "tools" / "shots" / "shots.toml"
OUT = ROOT / "docs" / "images" / "help"
RAW = ROOT / "target" / "shots" / "raw"
DOCS = ROOT / "docs"
# #186's budget: no committed screenshot over 300 KB.
BUDGET = 300 * 1024

# The annotations' colour: an orange that stands out on the app's blue and greys, in light and
# dark, never the site's green (kept for Download and delivered). White numerals on it are
# 5.2:1; a white edge keeps it apart from a dark window.
MARK = (194, 65, 12, 255)
EDGE = (255, 255, 255, 255)
SS = 4  # supersampling for the annotations


def load() -> list[dict]:
    data = tomllib.loads(LIST.read_text("utf-8"))
    shots = data.get("shot", [])
    seen = set()
    for s in shots:
        if s["id"] in seen:
            sys.exit(f"shots.toml: {s['id']} twice")
        seen.add(s["id"])
        if not re.fullmatch(r"[a-z0-9-]+", s["id"]) or s["id"].endswith("-dark"):
            sys.exit(f"shots.toml: {s['id']}: ids are lower case, digits and dashes, not ending in -dark")
        if bool(s.get("take")) == bool(s.get("from")):
            sys.exit(f"shots.toml: {s['id']}: one of take (a window) or from (a picture)")
    return shots


def raw_name(take: str) -> str:
    """The raw picture's file name for a docs-shots -Show entry."""
    return re.sub(r"[^a-z0-9]+", "-", take.lower()).strip("-")


# ---- shoot --------------------------------------------------------------------------------------

def shoot(shots: list[dict], bin_dir: Path, work: str | None, extra: list[str]) -> None:
    if os.name != "nt":
        sys.exit("shoot runs on Windows (tools/lab/docs-shots.ps1); `build` works anywhere")
    exe = bin_dir / "misterdownloadmanager.exe"
    server = bin_dir / "labserver.exe"
    for p in (exe, server):
        if not p.is_file():
            sys.exit(f"{p} missing: cargo build --release -p mdm-app -p mdm-labserver")
    takes = []
    for s in shots:
        if s.get("take") and s["take"] not in takes:
            takes.append(s["take"])
    if not takes:
        return
    # A take that changes something first ("theme:breeze+@main") goes last, so it doesn't
    # change the other shots.
    takes.sort(key=lambda t: "+" in t)
    torrent = ROOT / "tools" / "lab" / "Open Movie Night.torrent"
    for mode in ("light", "dark"):
        out = RAW / mode
        out.mkdir(parents=True, exist_ok=True)
        args = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                str(ROOT / "tools" / "lab" / "docs-shots.ps1"), "-Exe", str(exe), "-Server", str(server),
                "-OutDir", str(out), "-Ui", "-Torrent", str(torrent),
                "-Show", ",".join(f"{raw_name(t)}={t}" for t in takes)]
        if mode == "dark":
            args.append("-Dark")
        if work:
            args += ["-Work", work]
        args += extra
        print(f"shoot {mode}: {len(takes)} windows")
        r = subprocess.run(args, cwd=ROOT)
        if r.returncode:
            sys.exit(f"docs-shots.ps1 failed ({r.returncode})")


# ---- build --------------------------------------------------------------------------------------

def controls(raw: Path) -> list[dict]:
    ui = raw.with_suffix(".ui.json")
    if not ui.is_file():
        return []
    data = json.loads(ui.read_text("utf-8-sig") or "[]")
    return data if isinstance(data, list) else [data]


def find(ctrls: list[dict], what, sid: str) -> tuple[int, int, int, int]:
    """A rectangle: [x, y, w, h], or a control by name ("Start Download"; "Button:OK" by type
    too; "OK#2" for the second of that name)."""
    if isinstance(what, list):
        return tuple(what)  # type: ignore[return-value]
    name, _, nth = str(what).partition("#")
    kind = ""
    if name.split(":", 1)[0] in KINDS and ":" in name:
        kind, name = name.split(":", 1)
    hits = [c for c in ctrls if (c.get("name") or "").strip() == name and (not kind or c.get("type") == kind)]
    if not hits:
        hits = [c for c in ctrls if (c.get("name") or "").strip().startswith(name) and (not kind or c.get("type") == kind)]
    i = int(nth) - 1 if nth else 0
    if len(hits) <= i:
        raise LookupError(f"{sid}: no control {what!r}")
    return tuple(hits[i]["rect"])  # type: ignore[return-value]


KINDS = {"Button", "CheckBox", "ComboBox", "Edit", "Text", "List", "ListItem", "Menu", "MenuItem",
         "RadioButton", "Tab", "TabItem", "Group", "Pane", "Table", "DataItem", "Hyperlink", "Slider",
         "Spinner", "Tree", "TreeItem", "ToolBar", "Header", "HeaderItem", "Image", "Custom", "Document"}


def union(rects) -> tuple[int, int, int, int]:
    x0 = min(r[0] for r in rects)
    y0 = min(r[1] for r in rects)
    x1 = max(r[0] + r[2] for r in rects)
    y1 = max(r[1] + r[3] for r in rects)
    return x0, y0, x1 - x0, y1 - y0


def crop_box(s: dict, ctrls: list[dict], size: tuple[int, int]) -> tuple[int, int, int, int]:
    W, H = size
    c = s.get("crop")
    if not c:
        return 0, 0, W, H
    if isinstance(c, list):
        x, y, w, h = c
        w = w or W - x
        h = h or H - y
    else:
        x, y, w, h = union([find(ctrls, a, s["id"]) for a in c["around"]])
        pad = c.get("pad", 16)
        x, y, w, h = x - pad, y - pad, w + 2 * pad, h + 2 * pad
        # Keep whole edges: a crop that comes within `snap` px of a side takes the side too.
        snap = c.get("snap", 24)
        if x < snap:
            w, x = w + x, 0
        if y < snap:
            h, y = h + y, 0
        if W - (x + w) < snap:
            w = W - x
        if H - (y + h) < snap:
            h = H - y
    x, y = max(0, x), max(0, y)
    return x, y, min(w, W - x), min(h, H - y)


def font(px: int):
    from PIL import ImageFont
    for f in ("segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"):
        for d in (Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts", Path("/usr/share/fonts/truetype/dejavu")):
            if (d / f).is_file():
                return ImageFont.truetype(str(d / f), px)
    return ImageFont.load_default(px)


def annotate(img, s: dict, ctrls: list[dict], box):
    """Rings and numbered markers, drawn on a 4x overlay and scaled down onto the picture."""
    from PIL import Image, ImageDraw
    rings = s.get("ring", [])
    marks = s.get("marks", [])
    if not rings and not marks:
        return img
    ox, oy = box[0], box[1]
    W, H = img.size
    over = Image.new("RGBA", (W * SS, H * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(over)
    for r in rings:
        x, y, w, h = find(ctrls, r, s["id"])
        x, y = x - ox, y - oy
        p = 4
        a = [(x - p) * SS, (y - p) * SS, (x + w + p) * SS, (y + h + p) * SS]
        d.rounded_rectangle([a[0] - SS, a[1] - SS, a[2] + SS, a[3] + SS], radius=8 * SS, outline=EDGE, width=5 * SS)
        d.rounded_rectangle(a, radius=7 * SS, outline=MARK, width=3 * SS)
    f = font(13 * SS)
    for i, m in enumerate(marks):
        m = m if isinstance(m, dict) else {"at": m}
        n = m.get("n", i + 1)
        x, y, w, h = find(ctrls, m["at"], s["id"])
        x, y = x - ox, y - oy
        side = m.get("side", "left")
        R = 11
        cx, cy = {"left": (x - R - 3, y + h / 2), "right": (x + w + R + 3, y + h / 2),
                  "top": (x + w / 2, y - R - 3), "bottom": (x + w / 2, y + h + R + 3),
                  "center": (x + w / 2, y + h / 2), "corner": (x, y)}[side]
        cx = min(max(cx, R + 2), W - R - 2)
        cy = min(max(cy, R + 2), H - R - 2)
        d.ellipse([(cx - R - 2) * SS, (cy - R - 2) * SS, (cx + R + 2) * SS, (cy + R + 2) * SS], fill=EDGE)
        d.ellipse([(cx - R) * SS, (cy - R) * SS, (cx + R) * SS, (cy + R) * SS], fill=MARK)
        d.text((cx * SS, cy * SS + SS), str(n), font=f, fill=EDGE, anchor="mm")
    over = over.resize((W, H), Image.LANCZOS)
    base = img.convert("RGBA")
    base.alpha_composite(over)
    return base.convert("RGB")


def save_webp(img, path: Path, lossy: int = 0) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if lossy:
        # A photograph (a video frame in the browser), not a window: lossless would be several
        # times larger for no visible gain.
        img.save(path, "WEBP", quality=lossy, method=6)
        return
    cwebp = shutil.which("cwebp")
    if cwebp:
        tmp = path.with_suffix(".tmp.png")
        img.save(tmp)
        subprocess.run([cwebp, "-quiet", "-lossless", "-z", "9", "-metadata", "none", str(tmp), "-o", str(path)], check=True)
        tmp.unlink()
    else:
        img.save(path, "WEBP", lossless=True, quality=100, method=6, exact=False)


def build(shots: list[dict]) -> None:
    from PIL import Image
    made = 0
    for s in shots:
        for mode in ("light", "dark"):
            if s.get("take"):
                raw = RAW / mode / f"{raw_name(s['take'])}.png"
            else:
                raw = ROOT / (s["from"] if mode == "light" else s.get("from_dark", ""))
                if mode == "dark" and not s.get("from_dark"):
                    continue
            if not raw.is_file():
                print(f"  {s['id']} ({mode}): no {raw.relative_to(ROOT)}; shoot first", file=sys.stderr)
                continue
            img = Image.open(raw).convert("RGB")
            ctrls = controls(raw)
            try:
                box = crop_box(s, ctrls, img.size)
                img = img.crop((box[0], box[1], box[0] + box[2], box[1] + box[3]))
                img = annotate(img, s, ctrls, box)
            except LookupError as e:
                sys.exit(str(e))
            if s.get("scale"):
                w, h = img.size
                img = img.resize((round(w * s["scale"]), round(h * s["scale"])), Image.LANCZOS)
            name = s["id"] + ("-dark" if mode == "dark" else "") + ".webp"
            save_webp(img, OUT / name, s.get("lossy", 0))
            made += 1
            print(f"  {name}  {img.size[0]}x{img.size[1]}  {(OUT / name).stat().st_size // 1024} KB")
    print(f"{made} pictures in {OUT.relative_to(ROOT)}")


# ---- docs: the README's and the docs pages' own pictures ----------------------------------------

IMAGES = ROOT / "docs" / "images"
# docs-shots.ps1's and progress-shots.ps1's pictures that docs/images keeps (as <name>.webp),
# from a Docs screenshots run's docs/ folder; main-dark from its site-dark/ folder.
DOCS_SHOTS = ["add-torrent", "add-url", "arabic-main", "crash-report", "grabber", "language-picker",
              "main-light", "options-connection", "options-remote", "progress", "progress-torrent",
              "progress-torrent-pieces", "progress-video", "scheduler", "speed-limiter"]
DOCS_DARK = ["main-dark"]


def docs(run: Path | None, extra: list[Path]) -> None:
    """docs/images/<name>.webp, lossless, from a Docs screenshots run (the docs-shots artifact:
    docs/*.png and site-dark/*.png) and from single PNGs (`--png`: the browser's pictures, the
    phone's, a window shot elsewhere), each by its file name. A shot the run lacks keeps its
    picture."""
    from PIL import Image
    todo = [(p, p.stem) for p in extra]
    if run:
        todo = [(run / "docs" / f"{n}.png", n) for n in DOCS_SHOTS] + \
            [(run / "site-dark" / f"{n}.png", n) for n in DOCS_DARK] + todo
    for src, name in todo:
        if not src.is_file():
            print(f"  {name}: no {src}; kept")
            continue
        img = Image.open(src).convert("RGBA")
        if img.getchannel("A").getextrema() == (255, 255):
            img = img.convert("RGB")
        dst = IMAGES / f"{name}.webp"
        # A browser's picture holds a video frame: lossless would be several times larger.
        save_webp(img, dst, 90 if name.startswith("video-button") else 0)
        print(f"  {dst.relative_to(ROOT).as_posix():44} {img.size[0]}x{img.size[1]}  {dst.stat().st_size // 1024} KB")


# ---- the budget: every picture in docs/images and website/assets ---------------------------------

# Pictures that stay PNG: the social card (og:image: some sites don't read WebP), the logo and
# the icons the README shows.
PNG_OK = {"docs/images/social-preview.png", "docs/images/logo.png"}
PNG_DIRS = ("docs/images/icons/", "docs/images/browsers/")
SCREEN_DIRS = ("website/assets/screens/", "website/assets/browser/", "website/assets/win/")
TEXT = (".md", ".html", ".py", ".ps1", ".mjs", ".js", ".css", ".toml", ".yml", ".json", ".sh", ".rs", ".txt")


def budget() -> list[str]:
    """#186: no screenshot over 300 KB, no PNG where WebP belongs, no picture that nothing shows."""
    files = subprocess.run(["git", "ls-files", "-co", "--exclude-standard"], cwd=ROOT,
                           capture_output=True, text=True, check=True).stdout.splitlines()
    pics = [f for f in files
            if (f.startswith("docs/images/") and not f.startswith("docs/images/help/") or f.startswith(SCREEN_DIRS))
            and f.rsplit(".", 1)[-1].lower() in ("png", "webp", "jpg", "jpeg", "gif") and (ROOT / f).is_file()]
    corpus = []
    for f in files:
        if f.endswith(TEXT) and not f.startswith(("i18n/", "docs/bench/")) and (ROOT / f).is_file():
            corpus.append((ROOT / f).read_text("utf-8", errors="replace"))
    text = "\n".join(corpus)
    bad = []
    for f in pics:
        size = (ROOT / f).stat().st_size
        ext = f.rsplit(".", 1)[-1].lower()
        if size > BUDGET:
            bad.append(f"{f}: {size // 1024} KB, over {BUDGET // 1024} KB")
        if ext != "webp" and f not in PNG_OK and not f.startswith(PNG_DIRS):
            bad.append(f"{f}: a screenshot in {ext.upper()}; lossless WebP belongs here")
        name = f.rsplit("/", 1)[-1]
        # A night picture is found beside its day one (x-dark.webp beside x-light.webp or x.webp).
        if not any(n in text for n in (name, name.replace("-dark.", "-light."), name.replace("-dark.", "."))):
            bad.append(f"{f}: nothing shows it (left over?)")
    return bad


# ---- check / snippet ----------------------------------------------------------------------------

def snippet(s: dict) -> str:
    return f"![{s['alt']}](images/help/{s['id']}.webp)\n*{s['caption']}*"


def check(shots: list[dict]) -> int:
    bad = []
    ids = {s["id"] for s in shots}
    for s in shots:
        # A picture of the website's own (its Themes page, its feature pages): the file that
        # lists it, which the site builds its alt text and caption from.
        if s["page"].startswith("website/"):
            site = ROOT / s["page"]
            if not site.is_file():
                bad.append(f"{s['id']}: no {s['page']}")
            elif f"images/help/{s['id']}.webp" not in site.read_text("utf-8"):
                bad.append(f"{s['id']}: not in {s['page']}")
            if not (OUT / f"{s['id']}.webp").is_file():
                bad.append(f"{s['id']}: {(OUT / (s['id'] + '.webp')).relative_to(ROOT)} missing")
            continue
        page = DOCS / s["page"]
        if not page.is_file():
            bad.append(f"{s['id']}: no page {s['page']}")
            continue
        text = page.read_text("utf-8")
        ref = f"](images/help/{s['id']}.webp)"
        if ref not in text:
            bad.append(f"{s['id']}: not in {s['page']}; its Markdown:\n{snippet(s)}")
            continue
        at = text.index(ref)
        if f"![{s['alt']}" + ref not in text:
            bad.append(f"{s['id']}: {s['page']}'s alt text differs from the list's")
        after = text[at + len(ref):].lstrip(" \n")
        if not after.startswith(f"*{s['caption']}*"):
            bad.append(f"{s['id']}: {s['page']}'s caption differs from the list's")
        heads = re.findall(r"(?m)^#+ (.+)$", text[:at])
        if s.get("under") and (not heads or heads[-1].strip() != s["under"]):
            bad.append(f"{s['id']}: in {s['page']} under {heads[-1] if heads else 'nothing'!r}, not {s['under']!r}")
        light = OUT / f"{s['id']}.webp"
        if not light.is_file():
            bad.append(f"{s['id']}: {light.relative_to(ROOT)} missing")
    for f in sorted(OUT.glob("*.webp")) if OUT.is_dir() else []:
        sid = f.stem.removesuffix("-dark")
        if sid not in ids:
            bad.append(f"{f.relative_to(ROOT)}: not in the shot list (left over?)")
        if f.stat().st_size > BUDGET:
            bad.append(f"{f.relative_to(ROOT)}: {f.stat().st_size // 1024} KB, over {BUDGET // 1024} KB")
    for page in sorted(DOCS.glob("*.md")):
        for sid in re.findall(r"\]\(images/help/([a-z0-9-]+)\.webp\)", page.read_text("utf-8")):
            if sid.endswith("-dark"):
                bad.append(f"{page.name}: links {sid}.webp; link the light picture, the site adds the dark one")
            elif sid not in ids:
                bad.append(f"{page.name}: {sid}.webp is not in the shot list")
    bad += budget()
    for b in bad:
        print(b, file=sys.stderr)
    print(f"{len(shots)} shots, {len(bad)} problems")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("step", nargs="?", default="all", choices=["all", "shoot", "build", "check", "snippet", "docs"])
    ap.add_argument("id", nargs="?", help="snippet: the shot's id; docs: a Docs screenshots run's folder")
    ap.add_argument("--png", type=Path, action="append", default=[],
                    help="docs: a single PNG to put in docs/images as <its name>.webp (again for more)")
    ap.add_argument("--only", default="", help="these shot ids (commas)")
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    ap.add_argument("--bin", type=Path, default=target / "release", help="where the release build is")
    ap.add_argument("--work", default=None, help="docs-shots' work folder (its path shows in some windows)")
    # GitHub's runner (.github/workflows/docs-shots.yml): on the screen, as nobody watches it,
    # and a neutral address for the lab server.
    ap.add_argument("--at", default="", help='docs-shots -At "x,y": on the screen at x,y (a runner\'s software OpenGL)')
    ap.add_argument("--url-host", default="", help="docs-shots -UrlHost: the lab server's name in the addresses")
    ap.add_argument("--port", type=int, default=0, help="docs-shots -Port: the lab server's port")
    a = ap.parse_args()
    shots = load()
    if a.step == "check":
        return check(shots)
    if a.step == "docs":
        docs(Path(a.id) if a.id else None, a.png)
        return 0
    if a.step == "snippet":
        hit = [s for s in shots if s["id"] == a.id]
        if not hit:
            sys.exit(f"no shot {a.id}")
        print(snippet(hit[0]))
        return 0
    if a.only:
        want = set(a.only.split(","))
        shots = [s for s in shots if s["id"] in want]
    if a.step in ("all", "shoot"):
        extra = (["-At", a.at] if a.at else []) + (["-UrlHost", a.url_host] if a.url_host else []) \
            + (["-Port", str(a.port)] if a.port else [])
        shoot(shots, a.bin, a.work, extra)
    if a.step in ("all", "build"):
        build(shots)
    return 0


if __name__ == "__main__":
    sys.exit(main())

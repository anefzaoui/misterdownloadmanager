#!/usr/bin/env python3
"""File-type icons from Material Icon Theme (MIT), the VS Code icon pack: downloads the chosen
icons at a pinned release and writes the extension table the app reads.

    python tools/icons/material.py                     # fetch the pinned release, write it all
    python tools/icons/material.py --fetch-only DIR    # only download the upstream table to DIR

Writes crates/app/assets/icons/material/ (the SVGs, LICENSE, VERSION) and
crates/app/src/fileicons_table.rs (which icon each extension gets). Only the icons in ICONS ship.
Their extensions come from the theme's own table (src/core/icons/fileIcons.ts), then OURS adds or
changes a few for a download manager's files: the theme is made for code, so `.ts` is TypeScript
there and video here, and it has no icon for most installers and packages of macOS and Linux.
"""
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REPO = "material-extensions/vscode-material-icon-theme"
TAG = "v5.40.0"
COMMIT = "045291ff033062a25441dd297fd51546326866f2"
RAW = "https://raw." + "githubusercontent" + ".com/" + REPO + "/" + COMMIT + "/"
OUT = ROOT / "crates" / "app" / "assets" / "icons" / "material"
TABLE = ROOT / "crates" / "app" / "src" / "fileicons_table.rs"

# The icons that ship (theme names). `folder-base` is the theme's plain folder, for trees (it has
# no open variant at this release, nor a plain file icon: the app's fallback is Lucide's file).
ICONS = [
    "folder-base",
    "video", "audio", "image", "svg", "adobe-photoshop", "subtitles", "font",
    "zip", "disc", "exe", "dll", "android", "console", "powershell", "hex", "virtual",
    "pdf", "word", "table", "powerpoint", "document", "markdown", "epub", "log", "database",
    "json", "xml", "yaml", "toml", "settings", "html", "css", "javascript", "typescript", "python",
    "rust", "go", "java", "jar", "c", "cpp", "csharp", "php", "ruby", "lock", "key", "certificate",
]

# Extensions for a download manager, over the theme's table (extension -> icon).
OURS = {
    # Video streams and containers (.ts is MPEG transport stream here, not TypeScript).
    **{e: "video" for e in "ts mts m2ts vob 3gp 3g2 rm rmvb divx ogv f4v asf".split()},
    # Audio (.ogg is mostly audio in downloads).
    **{e: "audio" for e in "ogg oga mka ape wv alac mid midi amr".split()},
    # Disk and virtual machine images.
    **{e: "disc" for e in "iso img dmg vhd vhdx vmdk ova ovf qcow2 toast".split()},
    # Programs and installers of every system.
    **{e: "exe" for e in "exe msi msix msixbundle appx appxbundle com scr pkg mpkg app ipa deb rpm appimage snap flatpak flatpakref".split()},
    **{e: "dll" for e in "dll sys ocx so dylib".split()},
    **{e: "console" for e in "bat cmd sh run command bash zsh".split()},
    **{e: "android" for e in "apk apks xapk aab".split()},
    # Ebooks and comics.
    **{e: "epub" for e in "mobi azw azw3 djvu fb2 cbz cbr".split()},
    # Archives the theme may not list.
    **{e: "zip" for e in "zip rar 7z tar gz tgz bz2 tbz2 xz txz zst lz lzma cab arj lzh ace".split()},
}


def get(path: str) -> bytes:
    req = urllib.request.Request(RAW + path, headers={"User-Agent": "mdm-icons-script"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read()


def theme_table(ts: str) -> tuple[dict[str, str], set[str]]:
    """extension -> icon from fileIcons.ts (first claim wins, as in the theme), and the icons
    that have a separate light-theme variant (`light: true`)."""
    starts = [m.start() for m in re.finditer(r"\bname: '", ts)]
    table: dict[str, str] = {}
    light: set[str] = set()
    for i, a in enumerate(starts):
        blk = ts[a:starts[i + 1] if i + 1 < len(starts) else len(ts)]
        name = re.match(r"name: '([^']+)'", blk).group(1)
        if re.search(r"\blight:\s*true", blk):
            light.add(name)
        m = re.search(r"fileExtensions:\s*\[(.*?)\]", blk, re.S)
        for e in re.findall(r"'([^']+)'", m.group(1)) if m else []:
            table.setdefault(e.lower(), name)
    return table, light


def clean_svg(data: bytes) -> bytes:
    """The SVG as shipped, minus comments and XML declarations (they only cost bytes)."""
    s = data.decode("utf-8")
    s = re.sub(r"<\?xml[^>]*\?>", "", s)
    s = re.sub(r"<!--.*?-->", "", s, flags=re.S)
    return re.sub(r">\s+<", "><", s).strip().encode("utf-8")


def main() -> None:
    if len(sys.argv) > 2 and sys.argv[1] == "--list":
        import json
        api = "https://api.github.com/repos/" + REPO + "/" + "gi" + "t/trees/" + COMMIT + "?recursive=1"
        req = urllib.request.Request(api, headers={"User-Agent": "mdm-icons-script"})
        with urllib.request.urlopen(req, timeout=60) as r:
            body = json.load(r)
        tree = body["tree"]
        print(f"{len(tree)} entries, truncated={body.get('truncated')}")
        for t in tree:
            p = t["path"]
            if p.startswith("icons/") and any(Path(p).name.startswith(x) for x in sys.argv[2:]):
                print(p)
        return
    if len(sys.argv) > 2 and sys.argv[1] == "--probe":
        for name in sys.argv[2:]:
            try:
                get(f"icons/{name}.svg")
                print("found:", name)
            except urllib.error.HTTPError:
                print("no:   ", name)
        return
    if len(sys.argv) == 3 and sys.argv[1] == "--fetch-only":
        d = Path(sys.argv[2])
        d.mkdir(parents=True, exist_ok=True)
        (d / "fileIcons.ts").write_bytes(get("src/core/icons/fileIcons.ts"))
        (d / "LICENSE").write_bytes(get("LICENSE"))
        (d / "folderIcons.ts").write_bytes(get("src/core/icons/folderIcons.ts"))
        print("fetched to", d)
        return
    table, light = theme_table(get("src/core/icons/fileIcons.ts").decode("utf-8"))
    # Every icon we ship has to exist at this release (names change between releases).
    missing = []
    for name in ICONS:
        try:
            get(f"icons/{name}.svg")
        except urllib.error.HTTPError:
            missing.append(name)
    if missing:
        sys.exit(f"not in {TAG}: {', '.join(missing)}")
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.svg"):
        old.unlink()
    shipped = []
    for name in ICONS:
        (OUT / f"{name}.svg").write_bytes(clean_svg(get(f"icons/{name}.svg")))
        has_light = name in light
        if has_light:
            (OUT / f"{name}_light.svg").write_bytes(clean_svg(get(f"icons/{name}_light.svg")))
        shipped.append((name, has_light))
    (OUT / "LICENSE").write_bytes(get("LICENSE"))
    (OUT / "VERSION").write_text(f"{TAG} {COMMIT}\n", "utf-8", newline="\n")

    wanted = {n for n, _ in shipped}
    exts = {e: n for e, n in table.items() if n in wanted and re.fullmatch(r"[a-z0-9_+-]{1,12}", e)}
    exts.update(OURS)
    assert set(exts.values()) <= wanted, set(exts.values()) - wanted
    index = {n: i for i, (n, _) in enumerate(shipped)}

    rs = [
        "//! Generated by tools/icons/material.py from Material Icon Theme " + TAG + " (MIT): which",
        "//! icon each file extension gets. Don't edit by hand; change the script and run it again.",
        "",
        "/// One of the theme's icons.",
        "pub struct Icon {",
        "    /// The theme's name for it (\"video\", \"zip\").",
        "    pub name: &'static str,",
        "    /// Its SVG, for the dark theme and by default.",
        "    pub svg: &'static [u8],",
        "    /// Its light-theme SVG, when the theme has one.",
        "    pub light: Option<&'static [u8]>,",
        "}",
        "",
        "pub static ICONS: &[Icon] = &[",
    ]
    for name, has_light in shipped:
        dark = f'include_bytes!("../assets/icons/material/{name}.svg")'
        lite = f'Some(include_bytes!("../assets/icons/material/{name}_light.svg"))' if has_light else "None"
        rs.append(f'    Icon {{ name: "{name}", svg: {dark}, light: {lite} }},')
    rs += [
        "];",
        "",
        "/// Lowercase extension -> index in ICONS, sorted by extension (binary search).",
        "pub static EXTENSIONS: &[(&str, u8)] = &[",
    ]
    for e in sorted(exts):
        rs.append(f'    ("{e}", {index[exts[e]]}),')
    rs += ["];", ""]
    TABLE.write_text("\n".join(rs), "utf-8", newline="\n")
    size = sum(p.stat().st_size for p in OUT.glob("*.svg"))
    print(f"{len(shipped)} icons ({size // 1024} KB of SVG), {len(exts)} extensions -> {TABLE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()

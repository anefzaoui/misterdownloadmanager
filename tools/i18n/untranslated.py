"""List string literals in the app that look like interface text but aren't in i18n/en.tsv.

    python tools/i18n/untranslated.py [file.rs ...]

A review aid for M16: ids, paths, log lines, format plumbing and `#[cfg(test)]` modules are
skipped by pattern, and so is a line ending in `// not translated: <why>`; what is left is
either a label to wrap in `t()` / `tc()` / `tn()` / `mark()` or data that stays as it is.
A literal already in the catalog is not listed even where it is drawn untranslated: wrap.py
--check finds those given to the text helpers.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "crates" / "app" / "src"
LIT = re.compile(r'"((?:[^"\\]|\\.)*)"')
SKIP_LINE = re.compile(
    r"Id::unique|unique\(\(|from_hash_of|log::|expect\(|include_|env::var|#\[|^\s*//|"
    r"\.join\(|HeaderValue|HelpPage\(|add_filter|Software|tasklist|json|egui::Id|data_mut|get_temp|"
    r"insert_temp|bytes://|i18n::t|i18n::fmt|i18n::tn|i18n::mark|i18n::ltr\(|raw_arg|"
    # Data: Windows commands and their arguments, web addresses built from parts, HTTP headers.
    r"rundll32|\.dll,|explorer|mailto:|\{SITE\}|\{REPO\}|\\r\\n|env!\(|user_agent|"
    # Marked in the code: `"…" // not translated: <why>` (an id that looks like a sentence).
    r"// not translated\b")
SKIP_FILES = {"i18n.rs", "win32.rs", "icons.rs", "store.rs", "theme.rs", "main.rs"}


def unesc_rust(s):
    """The text a Rust string literal holds."""
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t", "r": "\r"}.get(m.group(1),
                                                                         m.group(1)), s)


def unesc_tsv(s):
    """The text a catalog cell holds."""
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), s)


def catalog():
    known = set()
    for line in (ROOT / "i18n" / "en.tsv").read_text(encoding="utf-8").splitlines():
        cols = line.split("\t")
        if len(cols) != 3 or line.startswith("#"):
            continue
        if cols[0] == "@plural":
            known.update(unesc_tsv(c) for c in cols[1].split("|"))
        else:
            known.add(unesc_tsv(cols[1]))
    return known


def lines_outside_tests(text):
    """(line number, line) of the file without its `#[cfg(test)] mod … { … }` blocks."""
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        if lines[i].strip() == "#[cfg(test)]" and i + 1 < len(lines) \
                and re.match(r"\s*(pub )?mod \w+ \{", lines[i + 1]):
            indent = len(lines[i]) - len(lines[i].lstrip())
            i += 2
            while i < len(lines) and lines[i] != " " * indent + "}":
                i += 1
            i += 1
            continue
        yield i + 1, lines[i]
        i += 1


def main():
    known = catalog()
    files = [SRC / a for a in sys.argv[1:]] or sorted(SRC.glob("*.rs"))
    total = 0
    for f in files:
        if f.name in SKIP_FILES:
            continue
        found = []
        for i, line in lines_outside_tests(f.read_text(encoding="utf-8")):
            if SKIP_LINE.search(line):
                continue
            for m in LIT.finditer(line):
                text = unesc_rust(m.group(1))
                words = re.sub(r"\{[^}]*\}", "", text)  # `{t}  {text}` is format plumbing
                if text in known or not re.search(r"[A-Za-z]{2}", words):
                    continue
                if re.fullmatch(r"[a-z0-9_\-.:/{}?=#&$]+", text):
                    continue  # ids, file names, keys, address parts
                found.append((i, text))
        if found:
            total += len(found)
            print(f"== {f.name} ({len(found)})")
            for i, text in found:
                print(f"  {i}: {text[:100]!r}"[:110])
    print(f"{total} candidate(s)")


if __name__ == "__main__":
    main()

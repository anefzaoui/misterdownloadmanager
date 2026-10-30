"""Wrap literal labels passed to the app's text-drawing helpers in `crate::i18n::t(...)` (M16).

    python tools/i18n/wrap.py [--check]

Only a plain string literal given directly as an argument of `text`, `text_right`, `text_fit` or
`text_font` is wrapped: those helpers also draw data (file names, addresses), which arrive as
variables and stay as they are. Widgets that only ever take labels (buttons, checkboxes, menus,
tabs, window titles, the toolbar) translate inside themselves. `--check` lists what would change
without writing (CI: nothing may be left unwrapped).
"""

import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parents[2] / "crates" / "app" / "src"
CALLS = re.compile(r"\b(?:winui::)?(text|text_right|text_fit|text_font)\s*\(")
LITERAL = re.compile(r'^\s*"((?:[^"\\]|\\.)*)"\s*$')
SKIP_FILES = {"i18n.rs", "win32.rs", "icons.rs"}


def split_args(s, start):
    """The arguments of the call whose `(` is at `start`: [(begin, end)] spans, and the `)`."""
    depth, i, begin, spans, quote = 0, start, start + 1, [], False
    while i < len(s):
        c = s[i]
        if quote:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                quote = False
        elif c == '"':
            quote = True
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                spans.append((begin, i))
                return spans, i
        elif c == "," and depth == 1:
            spans.append((begin, i))
            begin = i + 1
        i += 1
    return spans, i


def has_letters(text):
    return any(ch.isalpha() for ch in text)


def wrap(source):
    out, last, changes = [], 0, []
    for m in CALLS.finditer(source):
        if source[max(0, m.start() - 3):m.start()] == "fn ":
            continue
        spans, _ = split_args(source, m.end() - 1)
        for a, b in spans:
            arg = source[a:b]
            lm = LITERAL.match(arg)
            if not lm or not has_letters(lm.group(1)):
                continue
            lead = arg[: len(arg) - len(arg.lstrip())]
            trail = arg[len(arg.rstrip()):]
            changes.append((a, b, f'{lead}crate::i18n::t("{lm.group(1)}"){trail}', lm.group(1)))
    changes.sort()
    for a, b, new, _ in changes:
        if a < last:
            continue
        out.append(source[last:a])
        out.append(new)
        last = b
    out.append(source[last:])
    return "".join(out), [c[3] for c in changes]


def main():
    check = "--check" in sys.argv
    total = 0
    for f in sorted(SRC.glob("*.rs")):
        if f.name in SKIP_FILES:
            continue
        s = f.read_text(encoding="utf-8")
        new, found = wrap(s)
        if found:
            total += len(found)
            print(f"{f.name}: {len(found)}")
            if not check:
                f.write_text(new, encoding="utf-8", newline="")
    print(f"{total} label(s) {'to wrap' if check else 'wrapped'}")
    return 1 if check and total else 0


if __name__ == "__main__":
    sys.exit(main())

"""What the translators know about each language, beyond the rules every language shares
(tools/i18n/translate.py's RULES): its termbase, its style guide, and the rows a person reviewed.

    i18n/terms/<code>.tsv   English term -> this language's term, with when to use it
    i18n/style/<code>.md    how this language writes software: register, commands, questions...

Both are read by translate.py (the app), tools/website/translate_site.py (the website and the
help pages) and tools/i18n/learn.py (which proposes additions to them from a person's review).
A term is sent with a text only when the text contains it, so a long termbase costs nothing.

The rows a person reviewed are the best teacher: `review_pairs` finds, in the catalog's history,
each row whose `#, machine` line a person removed, with the machine's text before and theirs
after. learn.py learns from them; eval_reviews.py measures the pipeline against them.
"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
I18N = ROOT / "i18n"
TERMS = I18N / "terms"
STYLE = I18N / "style"


def unesc(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            out.append({"n": "\n", "t": "\t", "\\": "\\"}.get(s[i + 1], s[i + 1]))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


# ---- The termbase ----------------------------------------------------------------------------------

def terms(code):
    """i18n/terms/<code>.tsv as [(english, translation, note)], the longest English first (so
    "magnet link" is matched before "link")."""
    path = TERMS / f"{code}.tsv"
    out = []
    if not path.exists():
        return out
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        cols = line.split("\t")
        if len(cols) < 2:
            continue
        out.append((cols[0].strip(), cols[1].strip(), cols[2].strip() if len(cols) > 2 else ""))
    return sorted(out, key=lambda t: -len(t[0]))


def _pattern(term):
    # A word or phrase, its plural too ("link" matches "links"; "proxy" matches "proxies").
    words = [re.escape(w) for w in term.split()]
    last = words[-1]
    if term.endswith("y"):
        last = last[:-1] + "(?:y|ies)"
    else:
        last += "(?:s|es)?"
    return re.compile(r"(?<![\w-])" + r"\s+".join(words[:-1] + [last]) + r"(?![\w-])", re.I)


def matching_terms(code, text, base=None):
    """The termbase entries `text` mentions: {english: "translation (note)"}. A longer term that
    matches hides the shorter ones inside it ("magnet link" hides "link")."""
    base = terms(code) if base is None else base
    found, covered = {}, []
    for en, tr, note in base:
        for m in _pattern(en).finditer(text):
            span = m.span()
            if any(a <= span[0] and span[1] <= b for a, b in covered):
                continue
            covered.append(span)
            found[en] = f"{tr} ({note})" if note else tr
            break
    return found


# ---- The style guide -------------------------------------------------------------------------------

def style(code):
    """i18n/style/<code>.md without its comment lines, or ''."""
    path = STYLE / f"{code}.md"
    if not path.exists():
        return ""
    lines = [l for l in path.read_text(encoding="utf-8").splitlines() if not l.startswith("<!--")]
    return "\n".join(lines).strip()


# ---- What a person reviewed ------------------------------------------------------------------------

def parse_catalog(text):
    """A catalog's text -> {(context, source): (translation, machine)}."""
    out, machine = {}, False
    for line in text.splitlines():
        if line.startswith("#, machine"):
            machine = True
            continue
        if line.startswith("#") or not line.strip():
            continue
        cols = line.split("\t")
        if len(cols) == 3:
            out[(unesc(cols[0]), unesc(cols[1]))] = (unesc(cols[2]), machine)
        machine = False
    return out


def _git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True,
                          encoding="utf-8", check=False).stdout


def review_pairs(code, ref="HEAD"):
    """Every row a person reviewed in i18n/<code>.tsv's history (on `ref`'s first-parent line):
    [{"context", "source", "machine", "human", "commit"}], oldest first. "machine" is the text
    the row had just before the person removed its `#, machine` line; equal to "human" when they
    approved it as it was."""
    commits = _git("log", "--first-parent", "--reverse", "--format=%H", ref, "--",
                   f"i18n/{code}.tsv").split()
    pairs, seen = [], set()
    for c in commits:
        before = parse_catalog(_git("show", f"{c}^:i18n/{code}.tsv"))
        after = parse_catalog(_git("show", f"{c}:i18n/{code}.tsv"))
        for key, (text, machine) in after.items():
            old = before.get(key)
            if machine or not old or not old[1] or key in seen:
                continue
            seen.add(key)
            pairs.append({"context": key[0], "source": key[1], "machine": old[0], "human": text,
                          "commit": c[:8]})
    return pairs


def notes():
    """en.tsv's translator notes: {(context, source): [note, ...]}."""
    out, pending = {}, []
    for line in (I18N / "en.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("#."):
            pending.append(line[2:].strip())
            continue
        if line.startswith("#") or not line.strip():
            continue
        cols = line.split("\t")
        if len(cols) >= 2:
            out[(unesc(cols[0]), unesc(cols[1]))] = pending
        pending = []
    return out


def split(pairs, part):
    """A stable half of `pairs` ("train" or "test", by a hash of the English), so what is learned
    from one half can be measured on the other."""
    import hashlib
    def half(p):
        h = hashlib.sha256((p["context"] + "\t" + p["source"]).encode("utf-8")).digest()[0]
        return "train" if h % 2 == 0 else "test"
    return [p for p in pairs if half(p) == part]


def _words(s):
    return set(re.findall(r"[a-z]{3,}", s.lower()))


def examples(pairs, texts, k=12):
    """Up to `k` reviewed rows to show the model beside `texts`: the ones sharing the most words
    with them, those the person corrected first (they teach the most)."""
    want = set().union(*(_words(t) for t in texts)) if texts else set()
    scored = []
    for p in pairs:
        overlap = len(_words(p["source"]) & want)
        if overlap:
            scored.append((p["machine"] != p["human"], overlap, p))
    scored.sort(key=lambda s: (-s[0], -s[1]))
    return [{"english": p["source"], "context": p["context"] or None, "reviewed": p["human"]}
            for _, _, p in scored[:k]]

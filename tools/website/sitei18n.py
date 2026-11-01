"""The website in other languages: segments, catalogs and localized pages.

build.py writes the English site into target/site; `localize()` then writes every page again
under /<lang>/ for each language in LANGS, from that language's catalog (i18n/site/<lang>.json,
made by tools/website/translate_site.py). Nothing on a page is translated by hand: the English
page is split into segments, each segment is looked up by its hash, and a segment with no
translation yet stays English (so a new English sentence never breaks a translated page).

A segment is one piece of text a reader sees as a unit: a heading, a paragraph, a list item, a
button, a table cell, a page title, an image's description. Its inline markup goes to the
translator as numbered placeholders (`<g1>…</g1>` around text, `<x1/>` for something kept as it
is: an icon, a line break, a piece of code), so links and emphasis survive and the translation
can move them to where the language needs them.

Every page also gets, in English too: `hreflang` alternates (x-default English), its own canonical
address, `og:locale`, the language menu in the header, `lang`/`dir` on <html> (right to left for
Arabic and Persian), and a sitemap that lists each page's languages.
"""
from __future__ import annotations

import functools
import hashlib
import html
import json
import re
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CATALOGS = ROOT / "i18n" / "site"

# The site's languages (user decision 2026-11-01: the 20 most used after English), in the order the
# language menu lists them. code (URL folder), hreflang, og:locale, native name, English name.
LANGS = [
    ("es", "es", "es_ES", "Español", "Spanish"),
    ("pt-br", "pt-BR", "pt_BR", "Português (Brasil)", "Portuguese (Brazil)"),
    ("fr", "fr", "fr_FR", "Français", "French"),
    ("de", "de", "de_DE", "Deutsch", "German"),
    ("ru", "ru", "ru_RU", "Русский", "Russian"),
    ("ar", "ar", "ar_AR", "العربية", "Arabic"),
    ("zh-hans", "zh-Hans", "zh_CN", "简体中文", "Chinese (Simplified)"),
    ("ja", "ja", "ja_JP", "日本語", "Japanese"),
    ("ko", "ko", "ko_KR", "한국어", "Korean"),
    ("tr", "tr", "tr_TR", "Türkçe", "Turkish"),
    ("it", "it", "it_IT", "Italiano", "Italian"),
    ("id", "id", "id_ID", "Bahasa Indonesia", "Indonesian"),
    ("vi", "vi", "vi_VN", "Tiếng Việt", "Vietnamese"),
    ("hi", "hi", "hi_IN", "हिन्दी", "Hindi"),
    ("fa", "fa", "fa_IR", "فارسی", "Persian"),
    ("pl", "pl", "pl_PL", "Polski", "Polish"),
    ("uk", "uk", "uk_UA", "Українська", "Ukrainian"),
    ("nl", "nl", "nl_NL", "Nederlands", "Dutch"),
    ("th", "th", "th_TH", "ไทย", "Thai"),
    ("bn", "bn", "bn_BD", "বাংলা", "Bengali"),
]
RTL = {"ar", "fa"}
ENGLISH = ("en", "en", "en_US", "English", "English")

# Pages that stay English only: the GPL's own text (only the English is the licence).
ENGLISH_ONLY = {"/license.html"}

# ---- Tokens ------------------------------------------------------------------------------------

TOKEN = re.compile(
    r"<!--.*?-->"
    r"|<![^>]*>"
    r"|<(/?)([a-zA-Z][\w:-]*)((?:\s+[^\s=>/]+(?:\s*=\s*(?:\"[^\"]*\"|'[^']*'|[^\s>]+))?)*)\s*(/?)>"
    r"|[^<]+"
    r"|<",
    re.S,
)
ATTR = re.compile(r"([^\s=>/]+)(?:\s*=\s*(\"[^\"]*\"|'[^']*'|[^\s>]+))?")

# Elements whose content never reaches a translator.
RAW = {"script", "style", "pre", "textarea", "template", "noscript"}
# Inline elements: inside a segment, as placeholders.
INLINE = {"a", "abbr", "b", "bdi", "bdo", "br", "cite", "data", "dfn", "em", "i", "img", "mark",
          "q", "s", "small", "span", "strong", "sub", "sup", "time", "u", "var", "wbr", "del", "ins"}
# Inline elements kept whole (their content is code, a key, a picture): one `<xN/>`.
ATOMIC = {"code", "kbd", "samp", "svg", "math", "picture", "video", "audio", "canvas", "iframe"}
VOID = {"br", "img", "wbr", "input", "hr", "meta", "link", "source", "track", "area", "base", "col",
        "embed", "param"}
# Attributes a reader meets (a picture's description, a tooltip, a spoken label).
ATTRS = ("alt", "title", "aria-label", "placeholder", "data-label")
# <meta> whose content is the page's text.
META = {("name", "description"), ("property", "og:title"), ("property", "og:description"),
        ("property", "og:image:alt"), ("name", "twitter:title"), ("name", "twitter:description")}
LETTER = re.compile(r"[^\W\d_]")


@dataclass
class Tok:
    raw: str
    kind: str  # "tag", "end", "text", "other"
    name: str = ""
    attrs: str = ""
    selfclose: bool = False


def tokenize(doc: str) -> list[Tok]:
    out = []
    for m in TOKEN.finditer(doc):
        s = m.group(0)
        if m.group(2):
            name = m.group(2).lower()
            out.append(Tok(s, "end" if m.group(1) else "tag", name, m.group(3) or "", bool(m.group(4))))
        elif s.startswith("<!") or s == "<":
            out.append(Tok(s, "other"))
        else:
            out.append(Tok(s, "text"))
    return out


def attrs_of(t: Tok) -> dict[str, str]:
    return {k.lower(): html.unescape((v or "").strip("\"'")) for k, v in ATTR.findall(t.attrs)}


def no_translate(t: Tok) -> bool:
    """translate="no", class "notranslate", or text in another language (lang="ar": a name in
    its own language)."""
    a = attrs_of(t)
    lang = a.get("lang", "en").lower()
    return (a.get("translate") == "no" or "notranslate" in a.get("class", "").split()
            or not lang.startswith("en"))


# ---- Segments ----------------------------------------------------------------------------------

@dataclass
class Segment:
    """One unit to translate: `text` with placeholders, what they stand for, where it is."""
    text: str
    marks: dict[str, str] = field(default_factory=dict)  # "g1" -> open tag, "/g1" -> close, "x1" -> raw
    where: str = "text"

    @property
    def key(self) -> str:
        return seg_key(self.where if self.where == "js" else "", self.text)


def seg_key(kind: str, text: str) -> str:
    """The catalog's key for a source text (the same text anywhere is translated once)."""
    return hashlib.sha1(f"{kind}\0{text}".encode("utf-8")).hexdigest()[:16]


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()


def text_for_model(raw: str) -> str:
    """Page text as the translator sees it: entities as characters, except the three HTML needs."""
    return html.unescape(raw).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


WHERE = {"h1": "page heading", "h2": "section heading", "h3": "heading", "h4": "heading",
         "p": "paragraph", "li": "list item", "td": "table cell", "th": "table column title",
         "button": "button", "summary": "expandable section's title", "figcaption": "picture caption",
         "label": "form label", "title": "page title (browser tab, search results)",
         "dt": "term", "dd": "definition", "blockquote": "quote", "caption": "table title",
         "option": "choice in a list"}


def build_segment(toks: list[Tok], parent: str) -> tuple[int, int, Segment] | None:
    """The run `toks` (inline content) as a segment: (first, last+1 token index kept outside
    trimmed, segment). None when it holds no words."""
    # Trim whitespace-only text and atomic/void tokens at both ends (they stay where they are).
    def edge(t: Tok) -> bool:
        if t.kind == "text":
            return not t.raw.strip()
        if t.kind == "other":
            return True
        return t.name in VOID or t.name in ATOMIC
    a, b = 0, len(toks)
    # An atomic element at the edge spans to its end tag.
    while a < b:
        t = toks[a]
        if t.kind == "tag" and t.name in ATOMIC and not t.selfclose and t.name not in VOID:
            depth, k = 0, a
            while k < b:
                if toks[k].kind == "tag" and toks[k].name == t.name and not toks[k].selfclose:
                    depth += 1
                elif toks[k].kind == "end" and toks[k].name == t.name:
                    depth -= 1
                    if depth == 0:
                        break
                k += 1
            if k >= b:
                break
            # Kept outside only if what follows isn't glued to it (code in a sentence is part of it).
            a = k + 1
            continue
        if edge(t):
            a += 1
            continue
        break
    while b > a:
        t = toks[b - 1]
        if t.kind == "end" and t.name in ATOMIC:
            depth, k = 0, b - 1
            while k >= a:
                if toks[k].kind == "end" and toks[k].name == t.name:
                    depth += 1
                elif toks[k].kind == "tag" and toks[k].name == t.name and not toks[k].selfclose:
                    depth -= 1
                    if depth == 0:
                        break
                k -= 1
            if k < a:
                break
            b = k
            continue
        if edge(t):
            b -= 1
            continue
        break
    # One element around everything (<a>…</a>, <b>…</b>): kept outside, like the edges, and
    # what is inside it trimmed the same way again.
    a0, b0 = a, b
    while b - a >= 2 and toks[a].kind == "tag" and toks[a].name in INLINE \
            and toks[a].name not in VOID and not toks[a].selfclose \
            and toks[b - 1].kind == "end" and toks[b - 1].name == toks[a].name:
        depth, k = 0, a
        while k < b:
            if toks[k].kind == "tag" and toks[k].name == toks[a].name and not toks[k].selfclose:
                depth += 1
            elif toks[k].kind == "end" and toks[k].name == toks[a].name:
                depth -= 1
                if depth == 0:
                    break
            k += 1
        if k != b - 1:
            break
        a, b = a + 1, b - 1
        # The inside's own edges (whitespace, an icon) stay outside too.
        while a < b and toks[a].kind == "text" and not toks[a].raw.strip():
            a += 1
        while b > a and toks[b - 1].kind == "text" and not toks[b - 1].raw.strip():
            b -= 1
    if (a, b) != (a0, b0):
        r = build_segment(toks[a:b], parent)
        return (a + r[0], a + r[1], r[2]) if r else None
    run = toks[a:b]
    if not any(t.kind == "text" and LETTER.search(html.unescape(t.raw)) for t in run):
        return None
    # A single pair wrapping everything (<b>…</b>) is kept outside too.
    marks: dict[str, str] = {}
    parts: list[str] = []
    stack: list[tuple[str, str]] = []
    n = 0
    i = 0
    while i < len(run):
        t = run[i]
        if t.kind == "text":
            parts.append(text_for_model(t.raw))
        elif t.kind == "other":
            n += 1
            marks[f"x{n}"] = t.raw
            parts.append(f"<x{n}/>")
        elif t.kind == "tag" and (t.name in ATOMIC or no_translate(t)) and not t.selfclose and t.name not in VOID:
            # Kept whole up to its end tag.
            depth, k = 0, i
            while k < len(run):
                if run[k].kind == "tag" and run[k].name == t.name and not run[k].selfclose:
                    depth += 1
                elif run[k].kind == "end" and run[k].name == t.name:
                    depth -= 1
                    if depth == 0:
                        break
                k += 1
            n += 1
            marks[f"x{n}"] = "".join(x.raw for x in run[i:k + 1])
            parts.append(f"<x{n}/>")
            i = k + 1
            continue
        elif t.kind == "tag" and (t.name in VOID or t.selfclose):
            n += 1
            marks[f"x{n}"] = t.raw
            parts.append(f"<x{n}/>")
        elif t.kind == "tag":
            n += 1
            marks[f"g{n}"] = t.raw
            stack.append((t.name, f"g{n}"))
            parts.append(f"<g{n}>")
        elif t.kind == "end":
            # Close the nearest open element of that name (HTML here is well formed).
            for j in range(len(stack) - 1, -1, -1):
                if stack[j][0] == t.name:
                    g = stack[j][1]
                    del stack[j:]
                    marks["/" + g] = t.raw
                    parts.append(f"</{g}>")
                    break
            else:
                n += 1
                marks[f"x{n}"] = t.raw
                parts.append(f"<x{n}/>")
        i += 1
    if stack:  # an inline element left open across a block: not ours to cut
        return None
    text = norm("".join(parts))
    return a, b, Segment(text, marks, WHERE.get(parent, "text"))


def walk(doc: str):
    """The page's tokens, its segments (token span, segment) and its attribute segments
    ((token index, attribute name, value))."""
    toks = tokenize(doc)
    segs: list[tuple[int, int, Segment]] = []
    attrs: list[tuple[int, str, str]] = []
    run: list[int] = []
    stack: list[str] = []
    skip = 0  # inside a RAW element or a block with translate="no"
    skip_name = ""
    # Inside an element kept whole (an icon's <svg>, <code>): its tokens join the run as they are.
    whole = 0
    whole_name = ""
    in_head = False

    def parent() -> str:
        for name in reversed(stack):
            if name not in INLINE:
                return name
        return ""

    def one(ids: list[int], where: str):
        r = build_segment([toks[k] for k in ids], where)
        if r:
            a, b, seg = r
            segs.append((ids[a], ids[b - 1] + 1, seg))

    def flush():
        if not run:
            return
        where = parent()
        groups = split_widgets(toks, run) if where in CONTAINERS else None
        for ids in groups or [list(run)]:
            one(ids, where)
        run.clear()

    for k, t in enumerate(toks):
        if skip:
            if t.kind == "tag" and t.name == skip_name and not t.selfclose:
                skip += 1
            elif t.kind == "end" and t.name == skip_name:
                skip -= 1
            continue
        if whole:
            run.append(k)
            if t.kind == "tag" and t.name == whole_name and not t.selfclose:
                whole += 1
            elif t.kind == "end" and t.name == whole_name:
                whole -= 1
                if not whole:
                    stack.pop()
            continue
        if t.kind == "tag":
            if t.name == "head":
                in_head = True
            a = attrs_of(t)
            if t.name == "meta" and any(a.get(x) == y for x, y in META):
                if LETTER.search(a.get("content", "")):
                    attrs.append((k, "content", a["content"]))
            for name in ATTRS:
                if name in a and LETTER.search(a[name]) and not no_translate(t):
                    attrs.append((k, name, a[name]))
            if t.name in RAW or (no_translate(t) and t.name not in INLINE and t.name not in VOID):
                flush()
                if not t.selfclose:
                    skip, skip_name = 1, t.name
                continue
            if in_head and t.name != "title":
                continue
            if t.name in INLINE or t.name in ATOMIC:
                run.append(k)
                if t.name not in VOID and not t.selfclose:
                    stack.append(t.name)
                    if t.name in ATOMIC:
                        whole, whole_name = 1, t.name
                continue
            flush()
            if t.name not in VOID and not t.selfclose:
                stack.append(t.name)
        elif t.kind == "end":
            if t.name == "head":
                in_head = False
            if t.name in INLINE or t.name in ATOMIC:
                run.append(k)
            else:
                flush()
            for j in range(len(stack) - 1, -1, -1):
                if stack[j] == t.name:
                    del stack[j:]
                    break
        elif t.kind == "text":
            if in_head and parent() != "title":
                continue
            run.append(k)
        else:
            run.append(k)
    flush()
    return toks, segs, attrs


# Elements that hold other elements rather than a sentence: inline elements side by side in them
# (the header's buttons, a card's links) are separate texts, unless words stand between them.
CONTAINERS = {"div", "nav", "header", "footer", "section", "main", "article", "aside", "ul", "ol",
              "form", "figure", "body", "details", "table", "tr", "thead", "tbody", "dl", "fieldset", ""}


def split_widgets(toks: list[Tok], run: list[int]) -> list[list[int]] | None:
    """The run's top-level elements as groups, or None when words stand at its top level
    (then it is one sentence with links in it)."""
    groups: list[list[int]] = []
    depth = 0
    cur: list[int] = []
    for k in run:
        t = toks[k]
        if depth == 0 and t.kind == "text":
            if t.raw.strip():
                return None
            continue
        cur.append(k)
        if t.kind == "tag" and t.name not in VOID and not t.selfclose:
            depth += 1
        elif t.kind == "end":
            depth -= 1
        if depth <= 0:
            depth = 0
            groups.append(cur)
            cur = []
    if cur:
        groups.append(cur)
    return groups


def page_segments(doc: str) -> list[Segment]:
    """Everything on a page to translate (text, attributes, structured data), in page order."""
    _, segs, attrs = walk(doc)
    out = [s for _, _, s in segs]
    out += [Segment(norm(text_for_model(v)), {}, "attribute") for _, _, v in attrs]
    for data in jsonld_blocks(doc):
        for v in jsonld_strings(data):
            if is_html(v):
                out += [s for _, _, s in walk(v)[1]]
            else:
                out.append(Segment(norm(text_for_model(v)), {}, "structured data"))
    return out


def is_html(s: str) -> bool:
    """A structured-data text that is HTML (a FAQ answer): split like a page, not sent whole."""
    return bool(re.search(r"<(p|ul|ol|li|a|strong|em|code)\b", s))


def translate_fragment(fragment: str, cat: "Catalog", missing) -> str:
    """An HTML fragment's segments in the catalog's language (the rest English)."""
    toks, segs, _ = walk(fragment)
    raw = [t.raw for t in toks]
    for a, b, seg in segs:
        got = cat.get(seg)
        if got is None:
            if missing is not None:
                missing.append(seg)
            continue
        raw[a] = restore(seg, got)
        for k in range(a + 1, b):
            raw[k] = ""
    return "".join(raw)


# ---- Applying translations -----------------------------------------------------------------------

PH = re.compile(r"<(/?)([gx])(\d+)(/?)>")


def check_marks(seg: Segment, translation: str) -> str | None:
    """Why `translation` can't stand in for `seg` (its placeholders), or None."""
    want = sorted(PH.findall(seg.text))
    got = sorted(PH.findall(translation))
    if want != got:
        return ("keep every placeholder exactly once: " + " ".join(sorted(set(m.group(0) for m in PH.finditer(seg.text)))))
    # Pairs open before they close, and nest.
    stack = []
    for m in PH.finditer(translation):
        close, kind, num, self_ = m.groups()
        if kind == "x":
            continue
        if close:
            if not stack or stack[-1] != num:
                return "placeholder tags must open and close in order, as pairs"
            stack.pop()
        else:
            stack.append(num)
    return "placeholder tags must open and close in order, as pairs" if stack else None


def restore(seg: Segment, translation: str) -> str:
    """The translation as HTML: placeholders back to their tags, bare & < > escaped."""
    out = []
    pos = 0
    for m in PH.finditer(translation):
        out.append(escape_text(translation[pos:m.start()]))
        close, kind, num, _ = m.groups()
        key = ("/" if close else "") + kind + num
        out.append(seg.marks.get(key, ""))
        pos = m.end()
    out.append(escape_text(translation[pos:]))
    return "".join(out)


def escape_text(s: str) -> str:
    # Already-escaped entities stay; a bare & becomes &amp;.
    s = re.sub(r"&(?!(?:[a-zA-Z]+|#\d+|#x[0-9a-fA-F]+);)", "&amp;", s)
    return s.replace("<", "&lt;").replace(">", "&gt;")


def attr_escape(s: str) -> str:
    return html.escape(html.unescape(s), quote=True)


class Catalog:
    """i18n/site/<lang>.json: key -> {"source", "text"}; only checked translations are kept."""

    def __init__(self, code: str):
        self.code = code
        self.path = CATALOGS / f"{code}.json"
        self.rows: dict[str, dict] = {}
        if self.path.exists():
            self.rows = json.loads(self.path.read_text("utf-8")).get("segments", {})

    def get(self, seg: Segment) -> str | None:
        row = self.rows.get(seg.key)
        return row["text"] if row and row.get("source") == seg.text else None

    def put(self, seg: Segment, text: str) -> None:
        self.rows[seg.key] = {"source": seg.text, "text": text}

    def save(self, keep: set[str] | None = None) -> None:
        """Write it, sorted (small diffs); `keep`: drop rows no page uses any more."""
        rows = {k: v for k, v in sorted(self.rows.items()) if keep is None or k in keep}
        CATALOGS.mkdir(parents=True, exist_ok=True)
        doc = {"language": self.code,
               "note": "Machine translation of the website (tools/website/translate_site.py); "
                       "fix a row's text to improve it.",
               "segments": rows}
        self.path.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + "\n", "utf-8", newline="\n")


def translate_doc(doc: str, cat: Catalog, missing: list[Segment] | None = None) -> str:
    """`doc` with every segment, attribute and structured-data text the catalog has in its
    language; the rest stays English (and is added to `missing`)."""
    toks, segs, attrs = walk(doc)
    raw = [t.raw for t in toks]
    for a, b, seg in segs:
        tr = cat.get(seg)
        if tr is None:
            if missing is not None:
                missing.append(seg)
            continue
        raw[a] = restore(seg, tr)
        for k in range(a + 1, b):
            raw[k] = ""
    for k, name, value in attrs:
        seg = Segment(norm(text_for_model(value)), {}, "attribute")
        tr = cat.get(seg)
        if tr is None:
            if missing is not None:
                missing.append(seg)
            continue
        raw[k] = set_attr(raw[k], name, html.unescape(tr))
    out = "".join(raw)
    return translate_jsonld(out, cat, missing)


def set_attr(tag: str, name: str, value: str) -> str:
    pat = re.compile(r"(\s" + re.escape(name) + r"\s*=\s*)(\"[^\"]*\"|'[^']*'|[^\s>]+)", re.I)
    return pat.sub(lambda m: m.group(1) + '"' + attr_escape(value) + '"', tag, count=1)


# ---- Structured data -----------------------------------------------------------------------------

LD = re.compile(r'(<script type="application/ld\+json">)(.*?)(</script>)', re.S)
# Keys whose strings are words for people (not names of the product, the author or addresses).
LD_KEYS = {"name", "description", "headline", "text", "alternativeHeadline"}
LD_KEEP_TYPES = {"Person", "Organization", "SoftwareApplication", "Offer"}


def jsonld_blocks(doc: str) -> list:
    out = []
    for m in LD.finditer(doc):
        try:
            out.append(json.loads(m.group(2)))
        except json.JSONDecodeError:
            pass
    return out


def jsonld_strings(data, parent_type: str = "") -> list[str]:
    out = []
    if isinstance(data, dict):
        t = data.get("@type", parent_type)
        for k, v in data.items():
            if isinstance(v, str) and k in LD_KEYS and t not in LD_KEEP_TYPES and LETTER.search(v):
                out.append(v)
            elif isinstance(v, (dict, list)):
                out += jsonld_strings(v, t if isinstance(t, str) else "")
    elif isinstance(data, list):
        for v in data:
            out += jsonld_strings(v, parent_type)
    return out


def translate_jsonld(doc: str, cat: Catalog, missing) -> str:
    def tr_value(data, parent_type=""):
        if isinstance(data, dict):
            t = data.get("@type", parent_type)
            new = {}
            for k, v in data.items():
                if isinstance(v, str) and k in LD_KEYS and t not in LD_KEEP_TYPES and LETTER.search(v) \
                        and is_html(v):
                    new[k] = translate_fragment(v, cat, missing)
                elif isinstance(v, str) and k in LD_KEYS and t not in LD_KEEP_TYPES and LETTER.search(v):
                    seg = Segment(norm(text_for_model(v)), {}, "structured data")
                    got = cat.get(seg)
                    if got is None and missing is not None:
                        missing.append(seg)
                    new[k] = html.unescape(got) if got is not None else v
                elif k == "inLanguage":
                    new[k] = HREFLANG.get(cat.code, cat.code)
                else:
                    new[k] = tr_value(v, t if isinstance(t, str) else "")
            return new
        if isinstance(data, list):
            return [tr_value(v, parent_type) for v in data]
        return data

    def one(m):
        try:
            data = json.loads(m.group(2))
        except json.JSONDecodeError:
            return m.group(0)
        out = json.dumps(tr_value(data), ensure_ascii=False, separators=(",", ":"))
        return m.group(1) + out.replace("</", "<\\/") + m.group(3)

    return LD.sub(one, doc)


HREFLANG = {c: h for c, h, *_ in LANGS}


# ---- Strings outside the pages: the scripts' and the language menu's -------------------------------

# Words the localized pages add around the translated text (translated like any segment).
CHROME = [
    "Language",
    "Read this page in your language?",
    "Yes",
    "No, thanks",
    "Close",
]
JS_STRING = re.compile(r"""\bt\(\s*"((?:[^"\\]|\\.)*)"\s*\)""")


def js_strings() -> list[str]:
    """Every `t("…")` in the site's scripts (main.js, bench.js): their words for people."""
    out = []
    for name in ("main.js", "bench.js"):
        for m in JS_STRING.finditer((ROOT / "website" / name).read_text("utf-8")):
            s = json.loads(f'"{m.group(1)}"')
            if s not in out:
                out.append(s)
    return out


def ui_segment(text: str) -> Segment:
    return Segment(text, {}, "ui")


def js_segment(text: str) -> Segment:
    return Segment(text, {}, "js")


# ---- Pages -------------------------------------------------------------------------------------------

def site_pages(out: Path) -> list[str]:
    """The English pages built into `out`, as site paths ("/", "/help/", "/faq/x.html")."""
    codes = {c for c, *_ in LANGS}
    paths = []
    for f in sorted(out.rglob("*.html")):
        rel = f.relative_to(out).as_posix()
        if rel.split("/", 1)[0] in codes:
            continue
        path = "/" + rel
        if path.endswith("/index.html") or path == "/index.html":
            path = path[: -len("index.html")]
        paths.append(path)
    return paths


def translatable(path: str) -> bool:
    return path not in ENGLISH_ONLY and path != "/404.html"


def lang_path(path: str, code: str) -> str:
    return path if code == "en" else f"/{code}{path}"


def file_of(out: Path, path: str) -> Path:
    return out / (path.lstrip("/") + ("index.html" if path.endswith("/") else ""))


HREF = re.compile(r'(\shref=")(/[^"]*)(")')


def rewrite_links(doc: str, code: str, pages: set[str]) -> str:
    """Links to pages this language has go to its copy; files, feeds and assets stay put. The
    language menu's own links and the note's link to the English original are left as they are."""
    def one(m):
        url = m.group(2)
        path = re.split(r"[#?]", url, maxsplit=1)[0]
        rest = url[len(path):]
        if path in pages and translatable(path):
            return m.group(1) + lang_path(path, code) + rest + m.group(3)
        return m.group(0)
    return HREF.sub(one, doc)


@functools.lru_cache(maxsize=None)
def flags() -> dict[str, str]:
    """Each site language's flag: the app's own (i18n/languages.tsv's flag column, the icons in
    crates/app/assets/flags, which build.py copies to /assets/flags/)."""
    rows = [l.split("\t") for l in (ROOT / "i18n" / "languages.tsv").read_text("utf-8").splitlines()
            if l and not l.startswith("#")]
    by_code = {r[0]: r[3] for r in rows if len(r) > 3}
    return {c: by_code.get(c, "") for c, *_ in [ENGLISH] + LANGS}


def flag_img(code: str, size: int = 20) -> str:
    f = flags().get(code)
    return (f'<img class="flag" src="/assets/flags/{f}.svg" width="{size}" height="{size * 3 // 4}" alt="">'
            if f else "")


def lang_menu(path: str, current: str, langs: list[tuple], cats: dict) -> str:
    """The header's language button and its picker: every language's copy of this page (or its
    home page when this page stays English), by flag and name, each with the words the
    suggestion banner needs in it. A <dialog> that main.js opens; without scripts the button
    is a link to the languages page."""
    if len(langs) < 2:
        return ""
    here = next(n for c, _, _, n, _ in langs if c == current)
    items = []
    for code, tag, _, native, english in langs:
        target = lang_path(path, code) if (code == "en" or translatable(path)) else f"/{code}/"
        cur = ' aria-current="true"' if code == current else ""
        data = ""
        if code != "en" and code in cats:
            words = [cats[code].get(ui_segment(s)) or s for s in CHROME[1:4]]
            data = (f' data-offer="{attr_escape(words[0])}" data-yes="{attr_escape(words[1])}"'
                    f' data-no="{attr_escape(words[2])}"')
        # Only the name runs right to left: the row keeps the picker's own direction.
        rtl = ' dir="rtl"' if code in RTL else ""
        items.append(f'<li><a href="{target}" hreflang="{tag}" lang="{tag}"{cur}{data}>'
                     f'{flag_img(code, 28)}<span><bdi{rtl}>{html.escape(native)}</bdi></span><small lang="en">'
                     f'{html.escape(english)}</small></a></li>')
    def word(s: str) -> str:
        return (cats[current].get(ui_segment(s)) if current in cats else None) or s
    label, close = word("Language"), word("Close")
    return (f'<div class="langs" translate="no">'
            f'<a class="langs-btn" href="/languages.html" data-lang-open aria-haspopup="dialog" '
            f'aria-label="{attr_escape(label)}: {attr_escape(here)}" title="{attr_escape(label)}">'
            f'{flag_img(current)}<span class="langs-cur">{html.escape(here)}</span></a>'
            f'<dialog class="lang-dialog" aria-labelledby="lang-title-{current}">'
            f'<div class="lang-sheet"><div class="lang-head"><h2 id="lang-title-{current}">{html.escape(label)}</h2>'
            f'<button type="button" class="icon-btn" data-lang-close aria-label="{attr_escape(close)}">'
            f'<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" '
            f'stroke-linecap="round" d="M6 6l12 12M18 6 6 18"/></svg></button></div>'
            f'<ul class="lang-pick" role="list">{"".join(items)}</ul></div></dialog></div>')


def head_links(doc: str, path: str, code: str, langs: list[tuple], site: str) -> str:
    """Canonical and og:url for this language's copy; hreflang alternates (x-default English);
    og:locale with the others as alternates."""
    own = site + lang_path(path, code)
    doc = re.sub(r'<link rel="canonical" href="[^"]*">', f'<link rel="canonical" href="{own}">', doc, count=1)
    doc = re.sub(r'<meta property="og:url" content="[^"]*">', f'<meta property="og:url" content="{own}">', doc, count=1)
    if len(langs) > 1:
        alt = "".join(f'\n  <link rel="alternate" hreflang="{tag}" href="{site}{lang_path(path, c)}">'
                      for c, tag, *_ in langs)
        alt += f'\n  <link rel="alternate" hreflang="x-default" href="{site}{path}">'
        locale = next(l for c, _, l, *_ in langs if c == code)
        alt += f'\n  <meta property="og:locale" content="{locale}">'
        alt += "".join(f'\n  <meta property="og:locale:alternate" content="{l}">'
                       for c, _, l, *_ in langs if c != code)
        doc = re.sub(r'(<link rel="canonical" href="[^"]*">)', lambda m: m.group(1) + alt, doc, count=1)
    return doc


def localize(out: Path, site: str, langs_wanted: list[str] | None = None,
             translated: bool = True) -> dict[str, list[str]]:
    """Write every translatable page of `out` under /<code>/ for each language with a catalog,
    and give every page (English too) its language links. Returns, per page, the languages it
    exists in (for the sitemap). Prints each language's coverage. translated=False: the English
    pages with their language picker only (a quick local preview)."""
    cats = {}
    for code, *_ in LANGS:
        if langs_wanted and code not in langs_wanted:
            continue
        cat = Catalog(code)
        if cat.rows:
            cats[code] = cat
    langs = [ENGLISH] + [l for l in LANGS if l[0] in cats]
    paths = site_pages(out)
    pages = set(paths)
    english = {p: file_of(out, p).read_text("utf-8") for p in paths}
    available: dict[str, list[str]] = {}
    for path in paths:
        doc = english[path]
        here = langs if translatable(path) else [ENGLISH]
        available[path] = [c for c, *_ in here]
        en = head_links(doc.replace("<!--@langs-->", lang_menu(path, "en", langs, cats)), path, "en", here, site)
        file_of(out, path).write_text(en, "utf-8", newline="\n")
    for code, cat in (cats.items() if translated else []):
        lang = next(l for l in LANGS if l[0] == code)
        missing: list[Segment] = []
        js = {s: cat.get(js_segment(s)) for s in js_strings()}
        js = {k: v for k, v in js.items() if v}
        for path in paths:
            if not translatable(path):
                continue
            doc = translate_doc(english[path], cat, missing)
            doc = rewrite_links(doc, code, pages)
            doc = doc.replace("<!--@langs-->", lang_menu(path, code, langs, cats))
            doc = head_links(doc, path, code, langs, site)
            dir_ = ' dir="rtl"' if code in RTL else ""
            doc = re.sub(r'<html lang="en"', f'<html lang="{lang[1]}"{dir_}', doc, count=1)
            if js:
                blob = json.dumps(js, ensure_ascii=False).replace("</", "<\\/")
                doc = doc.replace('<script src="/main.js"',
                                  f'<script>window.MDM_I18N={blob};</script>\n  <script src="/main.js"', 1)
            target = file_of(out, lang_path(path, code))
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(doc, "utf-8", newline="\n")
        uniq = {s.key for s in missing}
        print(f"  {code}: {sum(map(translatable, paths))} pages, {len(uniq)} text(s) not translated yet")
    return available


def sitemaps(out: Path, site: str, pages: list[tuple[str, str]], available: dict[str, list[str]],
             today: str) -> None:
    """sitemap.xml: an index of one sitemap per language; each URL lists all its languages."""
    tags = {c: t for c, t, *_ in [ENGLISH] + LANGS}
    codes = ["en"] + [c for c, *_ in LANGS if any(c in v for v in available.values())]
    order = sorted(pages, key=lambda x: (-float(x[1]), x[0]))
    names = []
    for code in codes:
        urls = []
        for path, pr in order:
            langs = available.get(path, ["en"])
            if code not in langs:
                continue
            alts = ""
            if len(langs) > 1:
                alts = "".join(f'\n    <xhtml:link rel="alternate" hreflang="{tags[c]}" href="{site}{lang_path(path, c)}"/>'
                               for c in langs)
                alts += f'\n    <xhtml:link rel="alternate" hreflang="x-default" href="{site}{path}"/>'
            urls.append(f"  <url>\n    <loc>{site}{lang_path(path, code)}</loc>{alts}\n"
                        f"    <lastmod>{today}</lastmod>\n    <priority>{pr}</priority>\n  </url>")
        name = f"sitemap-{code}.xml"
        (out / name).write_text(
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" '
            'xmlns:xhtml="http://www.w3.org/1999/xhtml">\n' + "\n".join(urls) + "\n</urlset>\n",
            "utf-8", newline="\n")
        names.append(name)
    index = "\n".join(f"  <sitemap><loc>{site}/{n}</loc><lastmod>{today}</lastmod></sitemap>" for n in names)
    (out / "sitemap.xml").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        f"{index}\n</sitemapindex>\n", "utf-8", newline="\n")

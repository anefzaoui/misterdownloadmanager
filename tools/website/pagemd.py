"""Each page of the built site as Markdown, for AI tools and agents (build.py's markdown_twins()).

A page at /x.html gets /x.md beside it, /x/ (x/index.html) gets /x/index.md, in every language.
The help pages already have theirs, written from docs/*.md; every other page is turned from its
built HTML: what is in <main>, as headings, paragraphs, lists, tables, links, code and image
descriptions, without the header, footer, scripts, icons, buttons or form controls.
website/markdown/ (the Worker that would answer `Accept: text/markdown` with these) maps a
page's address to its Markdown the same way.

No packages: Python's html.parser only.
"""
from __future__ import annotations

import html
import re
from html.parser import HTMLParser
from urllib.parse import urljoin

VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"}
# Never text: code, drawings, controls, and what is shown only to scripts or to sighted readers.
SKIP = {"script", "style", "svg", "noscript", "template", "button", "input", "select", "textarea",
        "nav", "dialog", "iframe", "video", "audio", "canvas", "object", "picture", "source", "form"}
BLOCK = {"p", "div", "section", "article", "aside", "header", "footer", "main", "figure", "figcaption",
         "ul", "ol", "li", "dl", "dt", "dd", "table", "thead", "tbody", "tfoot", "tr", "td", "th",
         "h1", "h2", "h3", "h4", "h5", "h6", "pre", "blockquote", "hr", "fieldset", "legend", "details",
         "summary", "address"}


class Node:
    __slots__ = ("tag", "attrs", "children", "parent")

    def __init__(self, tag: str, attrs: dict[str, str], parent: "Node | None"):
        self.tag, self.attrs, self.children, self.parent = tag, attrs, [], parent


class Tree(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.root = Node("#root", {}, None)
        self.cur = self.root

    def handle_starttag(self, tag, attrs):
        node = Node(tag, {k: (v or "") for k, v in attrs}, self.cur)
        self.cur.children.append(node)
        if tag not in VOID:
            self.cur = node

    def handle_startendtag(self, tag, attrs):
        self.cur.children.append(Node(tag, {k: (v or "") for k, v in attrs}, self.cur))

    def handle_endtag(self, tag):
        n = self.cur
        while n is not None and n.tag != tag:
            n = n.parent
        if n is not None and n.parent is not None:
            self.cur = n.parent

    def handle_data(self, data):
        self.cur.children.append(data)


def find(node: Node, pred) -> Node | None:
    for c in node.children:
        if isinstance(c, Node):
            if pred(c):
                return c
            hit = find(c, pred)
            if hit:
                return hit
    return None


# Classes that are only decoration or only make sense on screen: the breadcrumb (the address says
# it), the rating sheet's "Yours" tag, its stamp and its star picker's live word.
SKIP_CLASSES = {"crumbs", "rate-you", "rate-stamp", "rate-word", "skip"}


def skipped(n: Node) -> bool:
    """Not text for a reader that doesn't see the page: hidden, aria-hidden, decoration, controls.
    Text kept for screen readers only (sr-only) stays: an agent doesn't see the page either."""
    a = n.attrs
    return (n.tag in SKIP or "hidden" in a or a.get("aria-hidden") == "true"
            or bool(SKIP_CLASSES & set(a.get("class", "").split())) or "data-rate-pick" in a)


class Writer:
    def __init__(self, base: str):
        self.base = base

    def url(self, href: str) -> str:
        if href.startswith("#") or href.startswith("mailto:"):
            return href
        return urljoin(self.base, href)

    def inline(self, n: Node | str, pre: bool = False) -> str:
        if isinstance(n, str):
            return n if pre else re.sub(r"\s+", " ", n)
        if skipped(n):
            return ""
        t = n.tag
        if t == "br":
            return "  \n"
        if t == "img":
            alt = n.attrs.get("alt", "").strip()
            return f"![{alt}]({self.url(n.attrs.get('src', ''))})" if alt else ""
        inner = self.join([(c, self.inline(c, pre)) for c in n.children])
        if t in ("strong", "b"):
            return f"**{inner.strip()}**" if inner.strip() else inner
        if t in ("em", "i"):
            return f"*{inner.strip()}*" if inner.strip() else inner
        if t == "code" and not pre:
            return f"`{inner}`" if inner else ""
        if t == "a" and n.attrs.get("href"):
            text = inner.strip()
            return f"[{text}]({self.url(n.attrs['href'])})" if text else ""
        return inner

    @staticmethod
    def join(parts: list[tuple["Node | str", str]]) -> str:
        """Inline pieces joined; two elements side by side with nothing between them (a tag and its
        label, a number and its unit, laid out apart by the stylesheet) get a space."""
        out = ""
        prev_el = False
        for node, text in parts:
            if not text:
                continue
            el = isinstance(node, Node)
            if el and prev_el and out and not out[-1].isspace() and not text[0].isspace():
                out += " "
            out += text
            prev_el = el
        return out

    def blocks(self, n: Node, out: list[str], depth: int = 0) -> None:
        """Appends n's children as Markdown blocks (each a string, joined by blank lines)."""
        run: list[tuple[Node | str, str]] = []

        def flush():
            text = re.sub(r"[ \t]+", " ", self.join(run)).strip()
            run.clear()
            if text:
                out.append(text)

        for c in n.children:
            if isinstance(c, str) or c.tag not in BLOCK:
                run.append((c, self.inline(c)))
                continue
            flush()
            if skipped(c):
                continue
            self.block(c, out, depth)
        flush()

    def block(self, n: Node, out: list[str], depth: int) -> None:
        t = n.tag
        if t in ("h1", "h2", "h3", "h4", "h5", "h6"):
            text = re.sub(r"\s+", " ", self.inline(n)).replace("**", "").strip()
            if text:
                out.append("#" * int(t[1]) + " " + text)
        elif t == "pre":
            code = "".join(self.inline(c, pre=True) for c in n.children).strip("\n")
            out.append(f"```\n{code}\n```")
        elif t in ("ul", "ol"):
            items = [c for c in n.children if isinstance(c, Node) and c.tag == "li" and not skipped(c)]
            lines = []
            for i, li in enumerate(items, 1):
                mark = f"{i}." if t == "ol" else "-"
                sub: list[str] = []
                self.blocks(li, sub, depth + 1)
                if not sub:
                    continue
                pad = " " * (len(mark) + 1)
                body = "\n\n".join(sub).replace("\n", "\n" + pad)
                lines.append(f"{mark} {body}")
            if lines:
                out.append("\n".join(lines))
        elif t == "dl":
            items: list[list[str]] = []
            for c in n.children:
                if not isinstance(c, Node) or skipped(c):
                    continue
                for d in ([x for x in c.children if isinstance(x, Node) and not skipped(x)] if c.tag == "div" else [c]):
                    text = re.sub(r"\s+", " ", self.inline(d)).strip()
                    if not text:
                        continue
                    if d.tag == "dt" or not items:
                        items.append([text])
                    else:
                        items[-1].append(text)
            if items:
                out.append("\n".join("- " +(i[0] + (": " + "; ".join(i[1:]) if len(i) > 1 else "")) for i in items))
        elif t == "table":
            self.table(n, out)
        elif t == "blockquote":
            sub: list[str] = []
            self.blocks(n, sub, depth)
            if sub:
                out.append("\n".join("> " + l if l else ">" for l in "\n\n".join(sub).splitlines()))
        elif t == "hr":
            out.append("---")
        else:
            self.blocks(n, out, depth)

    def table(self, n: Node, out: list[str]) -> None:
        rows: list[list[str]] = []
        head = False

        def walk(x: Node):
            nonlocal head
            for c in x.children:
                if not isinstance(c, Node) or skipped(c):
                    continue
                if c.tag == "tr":
                    cells = [c2 for c2 in c.children if isinstance(c2, Node) and c2.tag in ("td", "th")]
                    if not rows and cells and all(c2.tag == "th" for c2 in cells):
                        head = True
                    rows.append([re.sub(r"\s+", " ", self.inline(c2)).strip().replace("|", "\\|")
                                 for c2 in cells])
                else:
                    walk(c)
        walk(n)
        rows = [r for r in rows if any(r)]
        if not rows:
            return
        width = max(len(r) for r in rows)
        rows = [r + [""] * (width - len(r)) for r in rows]
        if not head:
            rows.insert(0, [""] * width)
        lines = ["| " + " | ".join(rows[0]) + " |", "|" + "---|" * width]
        lines += ["| " + " | ".join(r) + " |" for r in rows[1:]]
        out.append("\n".join(lines))


def to_markdown(page: str, base: str) -> str:
    """A built page (its whole HTML) as Markdown: the content of its <main>, links made absolute
    against `base` (the page's own address)."""
    tree = Tree()
    tree.feed(page)
    main = find(tree.root, lambda n: n.tag == "main") or find(tree.root, lambda n: n.tag == "body") or tree.root
    out: list[str] = []
    Writer(base).blocks(main, out)
    text = "\n\n".join(out)
    if not text.lstrip().startswith("# "):
        title = find(tree.root, lambda n: n.tag == "title")
        if title:
            name = html.unescape("".join(c for c in title.children if isinstance(c, str))).strip()
            text = f"# {name}\n\n{text}"
    text = re.sub(r"(?m)^[ \t]+$", "", text)
    return re.sub(r"\n{3,}", "\n\n", text).strip() + "\n"


def twin(path: str) -> str | None:
    """The Markdown twin's address of a page's address (the same rule as website/markdown/):
    /x/ -> /x/index.md, /x.html -> /x.md, /x -> /x.md; None for anything else (files)."""
    if path.endswith("/"):
        return path + "index.md"
    last = path.rsplit("/", 1)[-1]
    if last.endswith(".html"):
        return path[: -len(".html")] + ".md"
    if "." not in last:
        return path + ".md"
    return None

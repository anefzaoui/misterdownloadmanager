"""The build's twin rule (tools/website/pagemd.py) against the same table as the Worker's tests
(test/paths.json): a page's Markdown is where the Worker looks for it.

    python -I -m unittest discover -s website/markdown/test -p "test_*.py"
"""
import json
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools" / "website"))
import pagemd  # noqa: E402


class Twin(unittest.TestCase):
    def test_same_rule_as_the_worker(self):
        table = json.loads((Path(__file__).parent / "paths.json").read_text("utf-8"))
        for page, md in table.items():
            want = md if page.startswith("/") else None
            self.assertEqual(pagemd.twin(page) if page.startswith("/") else None, want, page)

    def test_page_text(self):
        page = """<html><head><title>T — MDM</title></head><body><header><nav><a href="/">Home</a></nav></header>
<main id="main"><ol class="crumbs"><li><a href="/">Home</a></li></ol><h1>Download <b>it</b></h1>
<p class="lede">Free <a href="/news.html">What's new</a><span class="tag">A</span><span class="tag">B</span></p>
<svg><text>icon</text></svg><script>x()</script><p hidden>secret</p><p aria-hidden="true">deco</p>
<span class="sr-only">for readers</span><button>Click</button>
<ul><li>one</li><li><a href="https://example.com/x">two</a></li></ul>
<dl><div><dt>File</dt><dd>What it is</dd></div></dl>
<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2|3</td></tr></table>
<pre><code>cargo  build
run</code></pre><img src="/a.webp" alt="A window"><img src="/b.webp" alt=""></main>
<footer>Footer text</footer></body></html>"""
        md = pagemd.to_markdown(page, "https://misterdownloadmanager.com/download.html")
        self.assertTrue(md.startswith("# Download it\n"), md)
        self.assertIn("Free [What's new](https://misterdownloadmanager.com/news.html) A B", md)
        self.assertIn("- one\n- [two](https://example.com/x)", md)
        self.assertIn("- File: What it is", md)
        self.assertIn("| A | B |\n|---|---|\n| 1 | 2\\|3 |", md)
        self.assertIn("```\ncargo  build\nrun\n```", md)
        self.assertIn("![A window](https://misterdownloadmanager.com/a.webp)", md)
        self.assertIn("for readers", md)
        for gone in ("Home", "icon", "x()", "secret", "deco", "Click", "Footer", "b.webp"):
            self.assertNotIn(gone, md)


if __name__ == "__main__":
    unittest.main()

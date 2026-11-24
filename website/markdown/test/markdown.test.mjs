// The Markdown Worker against a stand-in site: what it answers, and that everything else reaches
// the site untouched. No packages: node --test test/*.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { handle, markdownPath, quality, wantsMarkdown, estimateTokens } from "../src/markdown.js";
import worker from "../src/worker.js";

const SITE = "https://misterdownloadmanager.com";
const PAGES = {
  "/": "<html>home</html>",
  "/index.md": "# Mister Download Manager\n\nThe home page.\n",
  "/download.html": "<html>download</html>",
  "/download.md": "# Download\n",
  "/help/": "<html>help</html>",
  "/help/index.md": "# Help\n",
  "/no-twin.html": "<html>no twin</html>",
  "/styles.css": "body{}",
};

/** A stand-in for the site: answers PAGES, 404 otherwise; records every request it saw. */
function site({ fail } = {}) {
  const seen = [];
  const impl = async (req) => {
    seen.push(req);
    const path = new URL(req.url).pathname;
    if (fail && fail(path)) throw new Error("network down");
    if (!(path in PAGES)) return new Response("Not found", { status: 404 });
    const md = path.endsWith(".md");
    return new Response(PAGES[path], {
      status: 200,
      headers: {
        "Content-Type": md ? "text/markdown; charset=utf-8" : "text/html; charset=utf-8",
        "Cache-Control": "max-age=600",
        "Last-Modified": "Mon, 23 Nov 2026 21:31:15 GMT",
      },
    });
  };
  return { impl, seen };
}

const req = (path, accept, init = {}) =>
  new Request(SITE + path, { ...init, headers: { ...(accept ? { Accept: accept } : {}), ...(init.headers || {}) } });

test("the build's twin rule, the same table as tools/website/pagemd.py's test", () => {
  const table = JSON.parse(fs.readFileSync(new URL("./paths.json", import.meta.url), "utf-8"));
  for (const [page, md] of Object.entries(table)) assert.equal(markdownPath(page), md, page);
  assert.equal(markdownPath(undefined), null);
});

test("Accept: only an explicit text/markdown at least as wanted as HTML", () => {
  assert.equal(wantsMarkdown("text/markdown"), true);
  assert.equal(wantsMarkdown("text/markdown, text/html;q=0.9"), true);
  assert.equal(wantsMarkdown("text/html, text/markdown"), true);
  assert.equal(wantsMarkdown("text/html;q=0.9, text/markdown;q=0.8"), false);
  assert.equal(wantsMarkdown("text/markdown;q=0"), false);
  assert.equal(wantsMarkdown("TEXT/Markdown ; q=0.5"), true);
  // What browsers send never counts.
  assert.equal(wantsMarkdown("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"), false);
  assert.equal(wantsMarkdown("*/*"), false);
  assert.equal(wantsMarkdown("text/*"), false);
  assert.equal(wantsMarkdown(""), false);
  assert.equal(wantsMarkdown(null), false);
  assert.equal(quality("text/markdown;q=7", "text/markdown"), 1);
  assert.equal(quality("text/markdown;q=abc", "text/markdown"), 0);
});

test("a page asked for as Markdown gets its twin", async () => {
  const s = site();
  const res = await handle(req("/", "text/markdown"), s.impl);
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("Content-Type"), "text/markdown; charset=utf-8");
  assert.equal(res.headers.get("Vary"), "Accept");
  assert.equal(res.headers.get("Content-Location"), "/index.md");
  assert.equal(res.headers.get("Link"), `<${SITE}/>; rel="canonical"`);
  assert.equal(res.headers.get("Cache-Control"), "max-age=600");
  assert.equal(res.headers.get("Last-Modified"), "Mon, 23 Nov 2026 21:31:15 GMT");
  const body = await res.text();
  assert.equal(body, PAGES["/index.md"]);
  assert.equal(res.headers.get("x-markdown-tokens"), String(estimateTokens(body)));
  assert.equal(s.seen.length, 1);
  assert.equal(new URL(s.seen[0].url).pathname, "/index.md");

  const dl = await handle(req("/download.html", "text/markdown, text/html;q=0.5"), site().impl);
  assert.equal(await dl.text(), "# Download\n");
  const help = await handle(req("/help/", "text/markdown"), site().impl);
  assert.equal(await help.text(), "# Help\n");
});

test("HEAD gets the headers and no body", async () => {
  const res = await handle(req("/", "text/markdown", { method: "HEAD" }), site().impl);
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("Content-Type"), "text/markdown; charset=utf-8");
  assert.equal(await res.text(), "");
});

test("every other request reaches the site untouched", async () => {
  const cases = [
    req("/", null),
    req("/", "text/html,application/xhtml+xml,*/*;q=0.8"),
    req("/styles.css", "text/markdown"),
    req("/index.md", "text/markdown"),
    req("/", "text/markdown", { method: "POST", body: "x" }),
    req("/", "text/markdown", { method: "OPTIONS" }),
  ];
  for (const r of cases) {
    const s = site();
    const res = await handle(r, s.impl);
    assert.equal(s.seen.length, 1, `${r.method} ${r.url}`);
    assert.equal(s.seen[0], r, "the very same request object goes on");
    // The site's own answer, as it gave it.
    assert.equal(res.headers.get("x-markdown-tokens"), null);
    assert.equal(res.headers.get("Content-Location"), null);
  }
});

test("no twin, or a twin that fails: the page itself, as HTML", async () => {
  const s = site();
  const res = await handle(req("/no-twin.html", "text/markdown"), s.impl);
  assert.equal(res.status, 200);
  assert.equal(await res.text(), "<html>no twin</html>");
  assert.equal(s.seen.length, 2);
  assert.equal(s.seen[1].url, SITE + "/no-twin.html");

  const down = site({ fail: (p) => p.endsWith(".md") });
  const res2 = await handle(req("/", "text/markdown"), down.impl);
  assert.equal(await res2.text(), "<html>home</html>");

  const missing = await handle(req("/nowhere/", "text/markdown"), site().impl);
  assert.equal(missing.status, 404);
});

test("the Worker hands the site an error-free fallback, and asks Cloudflare to fail open", async () => {
  let passed = false;
  const ctx = { passThroughOnException: () => { passed = true; } };
  const saved = globalThis.fetch;
  const seen = [];
  globalThis.fetch = async (r) => { seen.push(r); return new Response("<html>home</html>", { status: 200 }); };
  try {
    const res = await worker.fetch(req("/", null), {}, ctx);
    assert.equal(passed, true);
    assert.equal(await res.text(), "<html>home</html>");
    // fetch() throwing on the twin: the page instead.
    let calls = 0;
    globalThis.fetch = async (r) => { calls++; if (calls === 1) throw new Error("boom"); return new Response("<html>page</html>"); };
    const res2 = await worker.fetch(req("/", "text/markdown"), {}, ctx);
    assert.equal(await res2.text(), "<html>page</html>");
  } finally {
    globalThis.fetch = saved;
  }
});

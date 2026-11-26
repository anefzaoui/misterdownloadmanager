// Markdown for AI tools and agents: a request for one of the site's pages that asks for Markdown
// (`Accept: text/markdown`) gets the page's Markdown twin, which the site's build writes beside
// every page (tools/website/pagemd.py: /x.html -> /x.md, /x/ -> /x/index.md). Every other request,
// and any request whose twin is missing or fails, goes to the site untouched: this never stands in
// the way of a page.

/** The quality (0 to 1) an Accept header gives a media type named in it, or 0. Only an exact
 * mention counts: `*\/*` and `text/*` are what browsers send, never a request for Markdown. */
export function quality(accept, type) {
  let best = 0;
  for (const part of String(accept || "").split(",")) {
    const [range, ...params] = part.split(";").map((s) => s.trim().toLowerCase());
    if (range !== type) continue;
    let q = 1;
    for (const p of params) {
      const m = /^q\s*=\s*(.*)$/.exec(p);
      // A q that isn't a number counts as 0: an odd header never turns a page into Markdown.
      if (m) q = Math.min(1, Math.max(0, Number(m[1]) || 0));
    }
    best = Math.max(best, q);
  }
  return best;
}

/** Whether a request's Accept header asks for Markdown at least as much as for HTML. */
export function wantsMarkdown(accept) {
  const md = quality(accept, "text/markdown");
  return md > 0 && md >= quality(accept, "text/html");
}

/** A page's address -> its Markdown twin's (the build's rule), or null for anything that isn't a
 * page (styles, scripts, pictures, the twins themselves). */
export function markdownPath(pathname) {
  if (typeof pathname !== "string" || !pathname.startsWith("/")) return null;
  if (pathname.endsWith("/")) return pathname + "index.md";
  const last = pathname.slice(pathname.lastIndexOf("/") + 1);
  if (last.endsWith(".html")) return pathname.slice(0, -".html".length) + ".md";
  if (!last.includes(".")) return pathname + ".md";
  return null;
}

/** About four characters a token: the size an agent can expect (`x-markdown-tokens`). */
export function estimateTokens(text) {
  return Math.ceil(text.length / 4);
}

/** Answers one request; `fetchImpl` reaches the site (the global fetch in the Worker). */
export async function handle(request, fetchImpl = fetch) {
  const pass = () => fetchImpl(request);
  if (request.method !== "GET" && request.method !== "HEAD") return pass();
  if (!wantsMarkdown(request.headers.get("Accept"))) return pass();
  const url = new URL(request.url);
  const twin = markdownPath(url.pathname);
  if (!twin) return pass();
  let res;
  try {
    res = await fetchImpl(new Request(new URL(twin, url).toString(), {
      method: "GET",
      headers: { Accept: "text/markdown" },
      redirect: "manual",
    }));
  } catch {
    return pass();
  }
  if (res.status !== 200) return pass();
  let text;
  try {
    text = await res.text();
  } catch {
    return pass();
  }
  const headers = new Headers({
    "Content-Type": "text/markdown; charset=utf-8",
    "Vary": "Accept",
    "Content-Location": twin,
    "Link": `<${url.origin}${url.pathname}>; rel="canonical"`,
    "x-markdown-tokens": String(estimateTokens(text)),
    "X-Content-Type-Options": "nosniff",
  });
  const cache = res.headers.get("Cache-Control");
  if (cache) headers.set("Cache-Control", cache);
  const modified = res.headers.get("Last-Modified");
  if (modified) headers.set("Last-Modified", modified);
  return new Response(request.method === "HEAD" ? null : text, { status: 200, headers });
}

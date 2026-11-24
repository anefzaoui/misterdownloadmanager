# The website in Markdown, for AI tools and agents

AI tools and agents read Markdown far more easily than a web page's HTML. Every page of
misterdownloadmanager.com has a Markdown twin beside it, written by the site's build
(`tools/website/build.py`, `markdown_twins()`; the conversion is `tools/website/pagemd.py`):

| The page | Its Markdown |
|---|---|
| `/` | `/index.md` |
| `/download.html` | `/download.md` |
| `/help/` | `/help/index.md` |
| `/de/faq/` | `/de/faq/index.md` |

The help pages' Markdown is their source in `docs/`; every other page's is made from the built
page: what is in its `<main>`, with the header, footer, scripts, icons and form controls left out,
links made absolute. Each page names its twin in its `<head>`
(`<link rel="alternate" type="text/markdown">`), and `/llms.txt` says where they are.

## The Worker (not routed yet)

`src/` is a Cloudflare Worker for content negotiation: a request for a page with
`Accept: text/markdown` (an explicit mention, wanted at least as much as `text/html`) gets the
page's twin as `text/markdown; charset=utf-8`, with `Vary: Accept`, `Content-Location` (the
twin's address), a `Link` to the page as `rel="canonical"` and `x-markdown-tokens` (about four
characters a token). Everything else goes to the site untouched, the very same request: other
`Accept` headers (browsers' `*/*` never counts), other methods, files that aren't pages, the
twins themselves. A twin that is missing or fails, or any error, also gives the page itself
(`passThroughOnException`; the route's failure mode would be fail open).

It isn't on a route yet: on `misterdownloadmanager.com/*` it would run for every request the site
gets, pictures, styles and scripts included, well over a million a day, which Cloudflare's
Workers plans count and bill. Nothing is lost without it: the twins are there to be read at their
addresses.

## Tests

No packages and no Cloudflare account needed:

```
cd website/markdown
node --test test/markdown.test.mjs                       # the Worker
python -I -m unittest discover -s test -p "test_*.py"    # the build's side
```

Both read `test/paths.json`, the table of page addresses and their twins, so the build and the
Worker never disagree on where a page's Markdown is.

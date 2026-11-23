---
title: For AI tools and agents
description: How AI assistants, agents and search tools can read misterdownloadmanager.com: every page in Markdown, llms.txt, what the site allows, its API catalog, MDM's own MCP server, and how the site's star ratings work.
lede: This site is written to be read by people first, and by the AI tools they ask about download managers too. Here is everything a program can read, and what it may do with it.
priority: 0.4
---

## Read the site

- **Every page in Markdown.** Each page has a Markdown version beside it: the address with `.md` in place of `.html`, or `index.md` for an address that ends in `/`. So [`/download.html`](/download.html) is [`/download.md`](/download.md), [`/help/`](/help/) is [`/help/index.md`](/help/index.md), and the German home page [`/de/`](/de/) is [`/de/index.md`](/de/index.md). Each page names its own in its `<head>` (`<link rel="alternate" type="text/markdown">`). The help pages' Markdown is their source; the others are made from the page at each build, so they never drift from it.
- **[llms.txt](/llms.txt)**: what Mister Download Manager is, in a few lines, and the main pages. **[llms-full.txt](/llms-full.txt)**: the help pages and the answers to common questions in one file.
- **[sitemap.xml](/sitemap.xml)**: every page, in every language.

Asking for a page with `Accept: text/markdown` still gets its HTML for now: read its `.md` address instead.

## What you may do with it

The site's [robots.txt](/robots.txt) lets every crawler in, and says so for AI in [Content Signals](https://contentsignals.org/):

```
Content-Signal: search=yes, ai-input=yes, ai-train=yes
```

Search engines may index it, AI tools may use it to answer questions, and it may be used to train AI models. Mister Download Manager is free, open-source software (GPL-3.0); its site is here to be found and understood.

## APIs

[`/.well-known/api-catalog`](/.well-known/api-catalog) lists them (RFC 9727):

- **The site's star ratings**, at `https://ratings.misterdownloadmanager.com/api/ratings`, described in [OpenAPI](/ratings-openapi.json). Anyone may read the numbers; rating is for this site's pages (below).
- **MDM's automation API**, which isn't on this site at all: it runs inside the app, on the user's own computer, off until they turn it on, and answers only there, with a secret token. Its [documentation](/help/command-line.html#the-automation-api) and its [JSON Schema](/automation-api.schema.json).

## MDM's MCP server

Mister Download Manager comes with an [MCP](https://modelcontextprotocol.io) server, `misterdownloadmanager-mcp`, so an AI assistant (Claude, Cursor, VS Code and others) can list, add, pause, resume and remove downloads in the user's running MDM. It is a **local** server: the assistant starts it on the user's computer, next to the app, and it talks only to the MDM there. There is no MCP server on this website. Its [server card](/.well-known/mcp/server-card.json) describes it, and [AI assistants](/help/ai-assistants.html) says how to set it up.

## Star ratings {#ratings}

The home and download pages show two kinds of ratings, never mixed:

- **The browser stores' own**: the rating, the number of ratings and the users that the [Firefox add-ons store](https://addons.mozilla.org/firefox/addon/mister-download-manager/) and the [Chrome Web Store](https://chromewebstore.google.com/detail/mister-download-manager/ejoleppdfgjdndddcjoehopknmlglojp) show for MDM's extension, read again at every build of the site, with the day they were read. They stay as page text only.
- **This site's**: visitors rate MDM from one to five stars, can change their rating by picking again, or remove it. Beside the stars: the average, the number of ratings and how many gave each number of stars. Only these go into the page's structured data for search engines.

One rating per visitor, and no free text. A rating is checked by Cloudflare Turnstile (a person, not a script), and a visitor can rate or remove at most eight times in ten minutes. The ratings service keeps the stars and a salted hash of the visitor's IP address, so the same visitor changes their rating instead of adding one; it never keeps the address itself. The [privacy policy](/privacy.html#this-website) says the same.

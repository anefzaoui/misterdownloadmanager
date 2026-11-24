"""What the website publishes for AI tools and agents, besides llms.txt (build.py) and each page's
Markdown (pagemd.py): robots.txt with Content Signals, the API catalog (RFC 9727) and the MCP
server card. website/content/agents.md explains them to people.
"""
from __future__ import annotations

import json
import re
from pathlib import Path

# The site's star ratings service (website/ratings/), and its OpenAPI description, published at
# RATINGS_OPENAPI on the site.
RATINGS_API = "https://ratings.misterdownloadmanager.com/api/ratings"
RATINGS_OPENAPI = "/ratings-openapi.json"

# Content Signals (contentsignals.org): the owner's choice, everything allowed.
CONTENT_SIGNAL = "search=yes, ai-input=yes, ai-train=yes"


def robots(site: str) -> str:
    return (f"User-agent: *\nContent-Signal: {CONTENT_SIGNAL}\nAllow: /\n\n"
            f"Sitemap: {site}/sitemap.xml\n")


def api_catalog(site: str, ratings: bool) -> dict:
    """/.well-known/api-catalog: a linkset (RFC 9264) of the APIs, each with its description
    (service-desc) and documentation (service-doc). MDM's automation API runs in the app on the
    user's computer, never on the site: its documentation and schema are listed, its anchor is
    that documentation."""
    sets = []
    if ratings:
        sets.append({
            "anchor": RATINGS_API,
            "service-desc": [{"href": site + RATINGS_OPENAPI, "type": "application/vnd.oai.openapi+json;version=3.1"}],
            "service-doc": [{"href": site + "/agents.html#ratings", "type": "text/html"}],
            "status": [{"href": RATINGS_API, "type": "application/json"}],
        })
    sets.append({
        "anchor": site + "/help/command-line.html#the-automation-api",
        "service-desc": [{"href": site + "/automation-api.schema.json", "type": "application/schema+json"}],
        "service-doc": [{"href": site + "/help/command-line.html#the-automation-api", "type": "text/html"},
                        {"href": site + "/help/command-line.md", "type": "text/markdown"}],
    })
    return {"linkset": sets}


def mcp_tools(root: Path) -> list[dict]:
    """The MCP server's tools, read from its source (crates/mcp/src/lib.rs): each `#[tool(
    description = "...")]` and the `async fn` under it, so the card never drifts from the server."""
    src = (root / "crates" / "mcp" / "src" / "lib.rs").read_text("utf-8")
    tools = []
    for m in re.finditer(r'#\[tool\(\s*description\s*=\s*"((?:[^"\\]|\\.)*)".*?\)\]\s*async fn (\w+)', src, re.S):
        desc = re.sub(r"\\\n\s*", "", m.group(1)).replace('\\"', '"')
        tools.append({"name": m.group(2), "description": re.sub(r"\s+", " ", desc).strip()})
    return tools


def mcp_card(site: str, repo: str, version: str, root: Path) -> dict:
    """/.well-known/mcp/server-card.json: MDM's MCP server, as it is: a local server the
    assistant starts on the user's computer (stdio), next to the app, that talks only to the MDM
    there. No server runs on the website, so the card names no remote endpoint."""
    return {
        "name": "misterdownloadmanager",
        "title": "Mister Download Manager",
        "description": ("Lists, adds, pauses, resumes and removes downloads in the Mister Download Manager app "
                        "running on the user's own computer, and starts or stops its queues. A local server: "
                        "the assistant starts misterdownloadmanager-mcp (installed with the app) over stdio; it "
                        "talks only to the MDM on that computer, through its automation API, which the user "
                        "turns on first. There is no MCP server on this website."),
        "version": version,
        "serverInfo": {"name": "misterdownloadmanager", "title": "Mister Download Manager", "version": version},
        "websiteUrl": site + "/",
        "documentationUrl": site + "/help/ai-assistants.html",
        "repository": {"url": repo, "source": "github", "subfolder": "crates/mcp"},
        "license": "GPL-3.0-or-later",
        "icons": [{"src": site + "/assets/logo-640.png", "sizes": ["640x640"], "mimeType": "image/png"}],
        "transport": {"type": "stdio", "command": "misterdownloadmanager-mcp", "args": []},
        "remotes": [],
        "local": {
            "command": "misterdownloadmanager-mcp",
            "installedWith": site + "/download.html",
            "requires": "Mister Download Manager running on the same computer, with its automation API on "
                        "(misterdownloadmanager-cli api on)",
        },
        "capabilities": {"tools": {"listChanged": False}},
        "tools": mcp_tools(root),
    }


def write(out: Path, site: str, repo: str, version: str, root: Path, ratings: bool) -> None:
    """The well-known files, and the ratings API's OpenAPI description, into the built site."""
    wk = out / ".well-known"
    (wk / "mcp").mkdir(parents=True, exist_ok=True)
    (wk / "api-catalog").write_text(json.dumps(api_catalog(site, ratings), indent=2) + "\n", "utf-8", newline="\n")
    (wk / "mcp" / "server-card.json").write_text(
        json.dumps(mcp_card(site, repo, version, root), indent=2, ensure_ascii=False) + "\n", "utf-8", newline="\n")
    spec = root / "website" / "ratings" / "openapi.json"
    if ratings and spec.is_file():
        (out / RATINGS_OPENAPI.lstrip("/")).write_text(spec.read_text("utf-8"), "utf-8", newline="\n")

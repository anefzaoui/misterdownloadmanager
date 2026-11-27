// The live figures on misterdownloadmanager.com's home page (website/main.js, the tallies under
// "Made possible by these amazing humans"): how many times MDM's files were downloaded from
// GitHub, and how many people use the browser add-on. Read-only, nothing stored in D1.
//
//   GET /api/stats -> {downloads, chrome, firefox, checked}
//
// Each figure is {value, fetched}: the number, and when it was read from its source (ISO 8601),
// or both null when it was never read. `checked` is when the sources were last asked.
//
//   downloads  GitHub's releases API, every page: the download_count of every file of every
//              release, all of them counted (the owner's rule, as GitHub's own total and the
//              README's badge count). The site's build counts the same way (tools/website/
//              tallies.py count_downloads): change both together.
//   firefox    addons.mozilla.org's public API: average_daily_users.
//   chrome     the Chrome Web Store has no public API: the users its public listing page shows,
//              read from the data the page carries for its own script, else from its text (as
//              tallies.py parse_cws does).
//
// The sources are asked at most once every FRESH_S seconds per Cloudflare location (the Cache
// API, a stale answer served while it refreshes); a source that can't be read, or answers with
// something that doesn't look like its number (a changed page, a sudden drop), keeps its last
// good value and the time that one was read. Nothing is ever made up: a figure never read is null.

export const STATS_PATH = "/api/stats";
export const FRESH_S = 900;
// How long the last good figures are kept, for the sources that can't be read meanwhile.
export const KEEP_S = 30 * 86400;
// How long a visitor's browser may reuse an answer.
const BROWSER_S = 300;
const TIMEOUT_MS = 8000;
const MAX_PAGES = 10;

export const REPO = "anefzaoui/misterdownloadmanager";
export const CWS_ID = "ejoleppdfgjdndddcjoehopknmlglojp";
const RELEASES = `https://api.github.com/repos/${REPO}/releases?per_page=100&page=`;
const AMO_API = "https://addons.mozilla.org/api/v5/addons/addon/mister-download-manager/";
const CWS_PAGE = `https://chromewebstore.google.com/detail/mister-download-manager/${CWS_ID}?hl=en`;
const UA = "misterdownloadmanager-stats";
const SOURCES = ["downloads", "chrome", "firefox"];

// This isolate's copy of the figures (the Cache API's is per location and outlives it).
let memo = null;
let refreshing = null;

export async function respondStats(request, env, ctx) {
  const cors = corsHeaders(request, env);
  try {
    const origin = request.headers.get("Origin");
    // A page elsewhere may not use it; a request without an Origin (the site's build, curl) may.
    if (origin && !cors) return reply({ error: "origin" }, 403, null);
    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors || {} });
    if (request.method !== "GET" && request.method !== "HEAD") {
      return reply({ error: "method" }, 405, cors, { Allow: "GET, HEAD, OPTIONS" });
    }
    const body = await figures(request, ctx);
    return reply(body, 200, cors, { "Cache-Control": `public, max-age=${BROWSER_S}` }, request.method === "HEAD");
  } catch (e) {
    console.error(`stats: ${e && e.name ? e.name : "error"}`);
    return reply({ error: "server" }, 500, cors);
  }
}

// The figures to answer with: fresh ones as they are; stale ones at once while they are read
// again in the background; none yet, read now.
async function figures(request, ctx) {
  const key = cacheKey(request);
  const known = memo || (await cached(key));
  if (known) memo = known;
  if (known && age(known) < FRESH_S) return known;
  const next = refresh(known, key);
  if (known) {
    if (ctx && ctx.waitUntil) ctx.waitUntil(next.catch(() => {}));
    return known;
  }
  return next;
}

function age(state) {
  const t = Date.parse(state.checked || "");
  return Number.isFinite(t) ? (Date.now() - t) / 1000 : Infinity;
}

// One refresh at a time in this isolate.
function refresh(known, key) {
  if (!refreshing) {
    refreshing = readAll(known)
      .then(async (state) => {
        memo = state;
        await store(key, state);
        return state;
      })
      .finally(() => {
        refreshing = null;
      });
  }
  return refreshing;
}

// Every source asked at once; each keeps its last good value when it can't be read.
export async function readAll(known, now = new Date(Date.now())) {
  const old = known || {};
  const read = await Promise.all([readDownloads(), readChrome(), readFirefox()].map((p) => p.catch(() => null)));
  const out = { checked: now.toISOString() };
  SOURCES.forEach((name, i) => {
    const before = old[name] && Number.isInteger(old[name].value) ? old[name] : null;
    const value = read[i];
    if (Number.isInteger(value) && value > 0 && plausible(name, value, before)) {
      out[name] = { value, fetched: now.toISOString() };
    } else {
      if (value != null) console.error(`stats: ${name} looked wrong, the last good value is kept`);
      out[name] = before ? { value: before.value, fetched: before.fetched } : { value: null, fetched: null };
    }
  });
  return out;
}

// Figures that never fall much: a store's users can't drop to a quarter overnight (tallies.py
// _plausible), the downloads to half (a release taken down); a parse that found someone else's
// number usually does.
export function plausible(name, value, before) {
  if (!before) return true;
  return value >= Math.floor(before.value / (name === "downloads" ? 2 : 4));
}

async function get(url, accept) {
  const res = await fetch(url, {
    headers: { "User-Agent": UA, Accept: accept, "Accept-Language": "en-US,en;q=0.8" },
    signal: AbortSignal.timeout(TIMEOUT_MS),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res;
}

// The downloads: every release, every page of them (a failed page fails the whole count).
export async function readDownloads() {
  let total = 0;
  for (let page = 1; page <= MAX_PAGES; page++) {
    const res = await get(RELEASES + page, "application/vnd.github+json");
    const releases = await res.json();
    if (!Array.isArray(releases)) return null;
    total += countDownloads(releases);
    if (releases.length < 100) return total;
  }
  return null;
}

// Every file of every release counts, whatever it is (tallies.py count_downloads: the same rule).
export function countDownloads(releases) {
  let n = 0;
  for (const r of releases) {
    for (const a of (r && r.assets) || []) {
      if (a && Number.isInteger(a.download_count) && a.download_count > 0) n += a.download_count;
    }
  }
  return n;
}

export async function readFirefox() {
  const data = await (await get(AMO_API, "application/json")).json();
  const n = data && data.average_daily_users;
  return Number.isInteger(n) ? n : null;
}

export async function readChrome() {
  return parseChromeUsers(await (await get(CWS_PAGE, "text/html")).text());
}

// The listing's users: from the extension's record in the data the page carries for its own
// script (its id first, the users 15th), else from the "300,000 users" the page shows. The page
// is large: plain searches, and one small JSON parse.
export function parseChromeUsers(page) {
  if (typeof page !== "string") return null;
  const start = page.indexOf("AF_initDataCallback({key: 'ds:0'");
  if (start >= 0) {
    const from = page.indexOf("data:", start);
    const to = from >= 0 ? page.indexOf(", sideChannel: {}});", from) : -1;
    if (from >= 0 && to > from && to - from < 500000) {
      try {
        const row = JSON.parse(page.slice(from + 5, to))[0];
        if (row && row[0] === CWS_ID && Number.isInteger(row[14]) && row[14] > 0) return row[14];
      } catch {
        // The text below, then.
      }
    }
  }
  const m = />\s*([\d.,]+\s*[KkMm]?\+?)\s*(?:<\/span>\s*<span[^>]*>\s*)?users\s*</.exec(page);
  return m ? abbrev(m[1]) : null;
}

// "200K" / "1.2M" / "8,353" as a number.
export function abbrev(text) {
  const m = /^([\d.,]+)\s*([KkMm]?)\+?$/.exec(String(text).trim());
  if (!m) return null;
  const n = Number(m[1].replace(/,/g, ""));
  if (!Number.isFinite(n)) return null;
  return Math.round(n * { "": 1, k: 1000, m: 1000000 }[m[2].toLowerCase()]);
}

// ---- The Cache API (per Cloudflare location) -------------------------------------------------------

function cacheKey(request) {
  return new URL("/__cache/stats/v1", request.url).toString();
}

function cacheApi() {
  return globalThis.caches && globalThis.caches.default ? globalThis.caches.default : null;
}

async function cached(key) {
  const cache = cacheApi();
  if (!cache) return null;
  try {
    const res = await cache.match(key);
    if (!res) return null;
    const state = await res.json();
    return state && typeof state === "object" && state.checked ? state : null;
  } catch {
    return null;
  }
}

async function store(key, state) {
  const cache = cacheApi();
  if (!cache) return;
  try {
    await cache.put(key, new Response(JSON.stringify(state), {
      headers: { "Content-Type": "application/json", "Cache-Control": `public, max-age=${KEEP_S}` },
    }));
  } catch {
    // This isolate's copy still holds them.
  }
}

// For the tests: forget this isolate's copy.
export function forget() {
  memo = null;
  refreshing = null;
}

// ---- Answers ---------------------------------------------------------------------------------------

function list(value) {
  return String(value || "").split(",").map((s) => s.trim().toLowerCase()).filter(Boolean);
}

function corsHeaders(request, env) {
  const origin = request.headers.get("Origin");
  if (!origin || !list(env.ALLOWED_ORIGINS).includes(origin.toLowerCase())) return null;
  return {
    "Access-Control-Allow-Origin": origin,
    "Access-Control-Allow-Methods": "GET, HEAD, OPTIONS",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  };
}

function reply(body, status, cors, extra, head) {
  return new Response(head ? null : JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      ...(cors || { Vary: "Origin" }),
      ...(extra || {}),
    },
  });
}

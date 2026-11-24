// Mister Download Manager's own star ratings (misterdownloadmanager.com): what the Worker
// (worker.js) does, over a D1 database. ../README.md says how it is set up and deployed.
//
//   GET    /api/ratings                        -> {average, count, distribution, mine}
//   POST   /api/ratings {stars, turnstileToken} -> the same, after the visitor's rating counts
//   DELETE /api/ratings                        -> the same, the visitor's rating removed
//
// `distribution` is {"1": n, ..., "5": n}; `mine` is the asking visitor's own rating, or null.
//
// One rating per visitor: a visitor is HMAC-SHA256(IP_SALT, their IP address), an IPv6 address
// by its /64 (one connection's addresses change inside it). The address itself is never stored,
// logged or sent anywhere but to Turnstile's check. Rating again replaces the visitor's rating.
// A rating needs a Cloudflare Turnstile token, checked here with Cloudflare, and a visitor sends
// at most MAX_POSTS ratings (or removals of theirs) in WINDOW_S seconds. Only the site's own pages may call it
// (ALLOWED_ORIGINS); stars are whole numbers from 1 to 5.
//
// Its environment: DB (the D1 database), IP_SALT and TURNSTILE_SECRET (secrets), ALLOWED_ORIGINS
// and TURNSTILE_HOSTNAMES (comma-separated; an empty TURNSTILE_HOSTNAMES checks no host name, as
// with Turnstile's test keys).

export const PATH = "/api/ratings";
export const WINDOW_S = 600;
export const MAX_POSTS = 8;
// How long a rating attempt's record (the visitor's hash and its time) is kept for the limit.
export const ATTEMPTS_KEEP_S = 3600;
const MAX_BODY = 4096;
const MAX_TOKEN = 2048;
const SITEVERIFY = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
// The action the site's Turnstile widget names (website/rate.js).
const ACTION = "rate";

export async function respond(request, env) {
  const cors = corsHeaders(request, env);
  try {
    return await handle(request, env, cors);
  } catch (e) {
    // The error's kind only: nothing about the request, so never an address.
    console.error(`ratings: ${e && e.name ? e.name : "error"}`);
    return reply({ error: "server" }, 500, cors);
  }
}

async function handle(request, env, cors) {
  const url = new URL(request.url);
  if (url.pathname !== PATH) return reply({ error: "not-found" }, 404, cors);
  const origin = request.headers.get("Origin");
  // A page elsewhere may not use it; a request without an Origin (the site's build, curl) may
  // read the numbers, never rate.
  if (origin && !cors) return reply({ error: "origin" }, 403, null);
  if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors || {} });
  if (request.method === "GET") return reply(await summary(env, await voterOrNull(request, env)), 200, cors);
  if (request.method !== "POST" && request.method !== "DELETE") {
    return reply({ error: "method" }, 405, cors, { Allow: "GET, POST, DELETE, OPTIONS" });
  }
  if (!origin) return reply({ error: "origin" }, 403, cors);
  return request.method === "POST" ? rate(request, env, cors) : remove(request, env, cors);
}

// Counts an attempt to rate or remove; a reply when the visitor is over the limit, else null.
async function overLimit(env, voter, cors) {
  const now = Math.floor(Date.now() / 1000);
  const recent = await env.DB.prepare("SELECT COUNT(*) AS n FROM attempts WHERE voter = ?1 AND at > ?2")
    .bind(voter, now - WINDOW_S).first("n");
  if (recent >= MAX_POSTS) return reply({ error: "rate-limit" }, 429, cors, { "Retry-After": String(WINDOW_S) });
  await env.DB.batch([
    env.DB.prepare("INSERT INTO attempts (voter, at) VALUES (?1, ?2)").bind(voter, now),
    env.DB.prepare("DELETE FROM attempts WHERE at < ?1").bind(now - ATTEMPTS_KEEP_S),
  ]);
  return null;
}

// The visitor takes their rating back: it no longer counts, and nothing of it is kept.
async function remove(request, env, cors) {
  if (!configured(env)) return reply({ error: "not-configured" }, 503, cors);
  const ip = clientIp(request);
  if (!ip) return reply({ error: "client" }, 400, cors);
  const voter = await voterId(env, ip);
  const limited = await overLimit(env, voter, cors);
  if (limited) return limited;
  const previous = await env.DB.prepare("SELECT stars FROM ratings WHERE voter = ?1").bind(voter).first("stars");
  await env.DB.prepare("DELETE FROM ratings WHERE voter = ?1").bind(voter).run();
  const out = await summary(env, voter);
  out.previous = previous ?? null;
  return reply(out, 200, cors);
}

async function rate(request, env, cors) {
  if (!configured(env)) return reply({ error: "not-configured" }, 503, cors);
  if (!/^application\/json\b/i.test(request.headers.get("Content-Type") || "")) {
    return reply({ error: "content-type" }, 415, cors);
  }
  if (Number(request.headers.get("Content-Length") || 0) > MAX_BODY) return reply({ error: "too-large" }, 413, cors);
  const text = await request.text();
  if (text.length > MAX_BODY) return reply({ error: "too-large" }, 413, cors);
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return reply({ error: "json" }, 400, cors);
  }
  const stars = body && body.stars;
  const token = body && body.turnstileToken;
  if (typeof stars !== "number" || !Number.isInteger(stars) || stars < 1 || stars > 5) {
    return reply({ error: "stars" }, 400, cors);
  }
  if (typeof token !== "string" || !token || token.length > MAX_TOKEN) return reply({ error: "token" }, 400, cors);
  const ip = clientIp(request);
  if (!ip) return reply({ error: "client" }, 400, cors);
  const voter = await voterId(env, ip);

  // The limit first: it costs a read, Turnstile's check a request to Cloudflare.
  const limited = await overLimit(env, voter, cors);
  if (limited) return limited;

  const check = await turnstile(env, token, ip);
  if (check === "unreachable") return reply({ error: "turnstile-unreachable" }, 502, cors);
  if (check !== "ok") return reply({ error: "turnstile" }, 403, cors);

  const previous = await env.DB.prepare("SELECT stars FROM ratings WHERE voter = ?1").bind(voter).first("stars");
  // One row per visitor; the totals follow it in the same statement (the schema's triggers).
  await env.DB.prepare(
    "INSERT INTO ratings (voter, stars) VALUES (?1, ?2) " +
      "ON CONFLICT (voter) DO UPDATE SET stars = excluded.stars WHERE stars <> excluded.stars",
  ).bind(voter, stars).run();
  const out = await summary(env, voter);
  out.previous = previous ?? null;
  return reply(out, 200, cors);
}

// The numbers everyone sees, and the asking visitor's own rating.
export async function summary(env, voter) {
  const { results } = await env.DB.prepare("SELECT stars, n FROM totals").all();
  const distribution = { 1: 0, 2: 0, 3: 0, 4: 0, 5: 0 };
  for (const row of results || []) {
    if (row.stars >= 1 && row.stars <= 5) distribution[row.stars] = Math.max(0, Number(row.n) || 0);
  }
  let count = 0;
  let sum = 0;
  for (let s = 1; s <= 5; s++) {
    count += distribution[s];
    sum += s * distribution[s];
  }
  const mine = voter
    ? await env.DB.prepare("SELECT stars FROM ratings WHERE voter = ?1").bind(voter).first("stars")
    : null;
  return {
    average: count ? Math.round((sum / count) * 100) / 100 : 0,
    count,
    distribution,
    mine: mine ?? null,
  };
}

function configured(env) {
  return Boolean(env.DB && env.IP_SALT && String(env.IP_SALT).length >= 32 && env.TURNSTILE_SECRET);
}

async function voterOrNull(request, env) {
  const ip = clientIp(request);
  return ip && configured(env) ? voterId(env, ip) : null;
}

// Cloudflare sets CF-Connecting-IP on every request it passes on (a visitor can't).
function clientIp(request) {
  return (request.headers.get("CF-Connecting-IP") || "").trim();
}

// What a visitor is known by before hashing: an IPv4 address as it is, an IPv6 address by its
// first 64 bits (written in full, so every spelling of one network is the same text).
export function ipKey(raw) {
  const ip = raw.trim().toLowerCase().split("%")[0];
  const mapped = /^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/.exec(ip);
  if (mapped) return mapped[1];
  if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(ip)) return ip;
  const halves = ip.split("::");
  if (!ip.includes(":") || halves.length > 2) return ip;
  const head = halves[0] ? halves[0].split(":") : [];
  const tail = halves.length === 2 ? (halves[1] ? halves[1].split(":") : []) : null;
  let groups = head;
  if (tail) {
    const fill = 8 - head.length - tail.length;
    if (fill < 0) return ip;
    groups = [...head, ...Array(fill).fill("0"), ...tail];
  }
  if (groups.length !== 8 || !groups.every((g) => /^[0-9a-f]{1,4}$/.test(g))) return ip;
  return groups.slice(0, 4).map((g) => g.padStart(4, "0")).join(":") + "::/64";
}

export async function voterId(env, ip) {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey("raw", enc.encode(env.IP_SALT), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = new Uint8Array(await crypto.subtle.sign("HMAC", key, enc.encode(ipKey(ip))));
  let bin = "";
  for (const b of mac) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

// "ok", "failed" or "unreachable": Cloudflare's answer about the token.
async function turnstile(env, token, ip) {
  const form = new FormData();
  form.append("secret", env.TURNSTILE_SECRET);
  form.append("response", token);
  form.append("remoteip", ip);
  let data;
  try {
    const res = await fetch(SITEVERIFY, { method: "POST", body: form });
    if (!res.ok) return "unreachable";
    data = await res.json();
  } catch {
    return "unreachable";
  }
  if (!data || data.success !== true) return "failed";
  if (data.action && data.action !== ACTION) return "failed";
  const hosts = list(env.TURNSTILE_HOSTNAMES);
  if (hosts.length && !hosts.includes(String(data.hostname || "").toLowerCase())) return "failed";
  return "ok";
}

function list(value) {
  return String(value || "").split(",").map((s) => s.trim().toLowerCase()).filter(Boolean);
}

// The CORS headers for an allowed Origin, else null.
function corsHeaders(request, env) {
  const origin = request.headers.get("Origin");
  if (!origin || !list(env.ALLOWED_ORIGINS).includes(origin.toLowerCase())) return null;
  return {
    "Access-Control-Allow-Origin": origin,
    "Access-Control-Allow-Methods": "GET, POST, DELETE, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  };
}

function reply(body, status, cors, extra) {
  return new Response(JSON.stringify(body), {
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

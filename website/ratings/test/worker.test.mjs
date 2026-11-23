// The ratings Worker (src/worker.js, src/ratings.js) against a real SQLite database with the real migrations
// (node:sqlite, standing in for D1's binding) and a stand-in for Turnstile's check.
//
//   npm test (node --test test/*.test.mjs)
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { afterEach, beforeEach, describe, it } from "node:test";
import { fileURLToPath } from "node:url";

import { ATTEMPTS_KEEP_S, MAX_POSTS, PATH, WINDOW_S, ipKey, voterId } from "../src/ratings.js";
import worker from "../src/worker.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SITE = "https://misterdownloadmanager.com";
const SALT = "test-salt-0123456789abcdef0123456789abcdef";

// D1's binding over node:sqlite: prepare/bind/first/all/run, and batch in one transaction.
class Statement {
  constructor(db, sql, args = []) {
    this.db = db;
    this.sql = sql;
    this.args = args;
  }
  bind(...args) {
    return new Statement(this.db, this.sql, args);
  }
  async first(column) {
    const row = this.db.prepare(this.sql).get(...this.args);
    if (!row) return null;
    return column ? row[column] : { ...row };
  }
  async all() {
    return { success: true, results: this.db.prepare(this.sql).all(...this.args).map((r) => ({ ...r })) };
  }
  async run() {
    return this.runNow();
  }
  runNow() {
    const info = this.db.prepare(this.sql).run(...this.args);
    return { success: true, meta: { changes: Number(info.changes) } };
  }
}

class D1 {
  constructor() {
    this.db = new DatabaseSync(":memory:");
    for (const f of fs.readdirSync(path.join(HERE, "..", "migrations")).sort()) {
      this.db.exec(fs.readFileSync(path.join(HERE, "..", "migrations", f), "utf-8"));
    }
  }
  prepare(sql) {
    return new Statement(this.db, sql);
  }
  async batch(statements) {
    this.db.exec("BEGIN");
    try {
      const out = statements.map((s) => s.runNow());
      this.db.exec("COMMIT");
      return out;
    } catch (e) {
      this.db.exec("ROLLBACK");
      throw e;
    }
  }
  rows(sql) {
    return this.db.prepare(sql).all().map((r) => ({ ...r }));
  }
}

let env;
let verified; // what the Worker sent to Turnstile, per call
let turnstileAnswer; // (token) => Response | throws
const realFetch = globalThis.fetch;

beforeEach(() => {
  env = {
    DB: new D1(),
    IP_SALT: SALT,
    TURNSTILE_SECRET: "test-secret",
    ALLOWED_ORIGINS: `${SITE},http://127.0.0.1:8765`,
    TURNSTILE_HOSTNAMES: "misterdownloadmanager.com",
  };
  verified = [];
  turnstileAnswer = (token) =>
    Response.json(token === "good"
      ? { success: true, hostname: "misterdownloadmanager.com", action: "rate" }
      : { success: false, "error-codes": ["invalid-input-response"] });
  globalThis.fetch = async (url, init) => {
    assert.equal(String(url), "https://challenges.cloudflare.com/turnstile/v0/siteverify");
    const form = init.body;
    verified.push({ secret: form.get("secret"), response: form.get("response"), remoteip: form.get("remoteip") });
    return turnstileAnswer(form.get("response"));
  };
});

afterEach(() => {
  globalThis.fetch = realFetch;
});

function call(method, { ip = "203.0.113.7", origin = SITE, body, headers = {}, path: p = PATH } = {}) {
  const h = { ...headers };
  if (ip) h["CF-Connecting-IP"] = ip;
  if (origin) h.Origin = origin;
  if (body !== undefined && !("Content-Type" in h)) h["Content-Type"] = "application/json";
  const init = { method, headers: h };
  if (body !== undefined) init.body = typeof body === "string" ? body : JSON.stringify(body);
  return worker.fetch(new Request(`https://ratings.example${p}`, init), env);
}

const vote = (stars, opts = {}) => call("POST", { ...opts, body: { stars, turnstileToken: opts.token ?? "good" } });

describe("reading the numbers", () => {
  it("starts empty", async () => {
    const res = await call("GET");
    assert.equal(res.status, 200);
    assert.deepEqual(await res.json(), { average: 0, count: 0, distribution: { 1: 0, 2: 0, 3: 0, 4: 0, 5: 0 }, mine: null });
    assert.equal(res.headers.get("Access-Control-Allow-Origin"), SITE);
    assert.equal(res.headers.get("Cache-Control"), "no-store");
  });

  it("answers the site's build, which sends no Origin, without CORS headers", async () => {
    const res = await call("GET", { origin: null, ip: null });
    assert.equal(res.status, 200);
    assert.equal(res.headers.get("Access-Control-Allow-Origin"), null);
    assert.equal((await res.json()).mine, null);
  });

  it("refuses another site's page", async () => {
    const res = await call("GET", { origin: "https://evil.example" });
    assert.equal(res.status, 403);
    assert.equal(res.headers.get("Access-Control-Allow-Origin"), null);
  });

  it("answers the preflight for the site only", async () => {
    const ok = await call("OPTIONS", { origin: "http://127.0.0.1:8765" });
    assert.equal(ok.status, 204);
    assert.equal(ok.headers.get("Access-Control-Allow-Origin"), "http://127.0.0.1:8765");
    assert.match(ok.headers.get("Access-Control-Allow-Methods"), /POST/);
    assert.equal(ok.headers.get("Access-Control-Allow-Headers"), "Content-Type");
    assert.equal((await call("OPTIONS", { origin: "https://misterdownloadmanager.com.evil.example" })).status, 403);
  });

  it("knows only its own path and methods", async () => {
    assert.equal((await call("GET", { path: "/api/other" })).status, 404);
    const put = await call("PUT", { body: {} });
    assert.equal(put.status, 405);
    assert.equal(put.headers.get("Allow"), "GET, POST, DELETE, OPTIONS");
  });
});

describe("rating", () => {
  it("counts a rating and says it is the visitor's", async () => {
    const res = await vote(4);
    assert.equal(res.status, 200);
    assert.deepEqual(await res.json(), {
      average: 4, count: 1, distribution: { 1: 0, 2: 0, 3: 0, 4: 1, 5: 0 }, mine: 4, previous: null,
    });
    assert.deepEqual(verified, [{ secret: "test-secret", response: "good", remoteip: "203.0.113.7" }]);
    const mine = await (await call("GET")).json();
    assert.equal(mine.mine, 4);
    const other = await (await call("GET", { ip: "198.51.100.1" })).json();
    assert.equal(other.mine, null);
  });

  it("replaces a visitor's rating instead of adding another", async () => {
    await vote(2);
    const res = await (await vote(5)).json();
    assert.equal(res.count, 1);
    assert.equal(res.previous, 2);
    assert.equal(res.mine, 5);
    assert.deepEqual(res.distribution, { 1: 0, 2: 0, 3: 0, 4: 0, 5: 1 });
    const same = await (await vote(5)).json();
    assert.equal(same.count, 1);
    assert.equal(same.distribution[5], 1);
    assert.deepEqual(env.DB.rows("SELECT COUNT(*) AS n FROM ratings"), [{ n: 1 }]);
  });

  it("counts each visitor once and averages them", async () => {
    await vote(5, { ip: "203.0.113.1" });
    await vote(4, { ip: "203.0.113.2" });
    await vote(4, { ip: "203.0.113.3" });
    const res = await (await vote(1, { ip: "2001:db8::1" })).json();
    assert.equal(res.count, 4);
    assert.equal(res.average, 3.5);
    assert.deepEqual(res.distribution, { 1: 1, 2: 0, 3: 0, 4: 2, 5: 1 });
  });

  it("rounds the average to two decimals", async () => {
    await vote(5, { ip: "203.0.113.1" });
    await vote(5, { ip: "203.0.113.2" });
    const res = await (await vote(4, { ip: "203.0.113.3" })).json();
    assert.equal(res.average, 4.67);
  });

  it("knows an IPv6 visitor by their network's first 64 bits", async () => {
    await vote(3, { ip: "2001:db8:aa:bb::1" });
    const res = await (await vote(5, { ip: "2001:0db8:00aa:00bb:1234:5678:9abc:def0" })).json();
    assert.equal(res.count, 1);
    assert.equal(res.previous, 3);
    const other = await (await vote(5, { ip: "2001:db8:aa:bc::1" })).json();
    assert.equal(other.count, 2);
  });

  it("keeps the totals equal to the ratings through every change", async () => {
    const ips = ["10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5"];
    const sequence = [5, 3, 1, 4, 2, 2, 5, 1, 3, 4];
    for (let i = 0; i < sequence.length; i++) await vote(sequence[i], { ip: ips[i % ips.length] });
    const counted = Object.fromEntries(env.DB.rows("SELECT stars, COUNT(*) AS n FROM ratings GROUP BY stars").map((r) => [r.stars, r.n]));
    for (const { stars, n } of env.DB.rows("SELECT stars, n FROM totals")) assert.equal(n, counted[stars] || 0, `${stars} stars`);
  });

  it("stores no IP address, only its keyed hash", async () => {
    const ip = "203.0.113.77";
    await vote(4, { ip });
    await vote(2, { ip: "2001:db8:1:2::99" });
    const dump = JSON.stringify([env.DB.rows("SELECT * FROM ratings"), env.DB.rows("SELECT * FROM attempts")]);
    for (const part of [ip, "2001:db8", "203.0.113"]) assert.ok(!dump.includes(part), `found ${part}`);
    const expected = await voterId(env, ip);
    assert.deepEqual(env.DB.rows("SELECT voter FROM ratings WHERE stars = 4"), [{ voter: expected }]);
    // Another salt gives another hash: the hashes can't be matched without the secret.
    assert.notEqual(await voterId({ IP_SALT: `${SALT}x` }, ip), expected);
  });
});

describe("taking a rating back", () => {
  it("removes the visitor's own rating only", async () => {
    await vote(5, { ip: "203.0.113.1" });
    await vote(2, { ip: "203.0.113.2" });
    const res = await call("DELETE", { ip: "203.0.113.2" });
    assert.equal(res.status, 200);
    assert.deepEqual(await res.json(), {
      average: 5, count: 1, distribution: { 1: 0, 2: 0, 3: 0, 4: 0, 5: 1 }, mine: null, previous: 2,
    });
    assert.deepEqual(env.DB.rows("SELECT stars FROM ratings"), [{ stars: 5 }]);
    const again = await (await call("DELETE", { ip: "203.0.113.2" })).json();
    assert.equal(again.previous, null);
    assert.equal(again.count, 1);
    assert.equal(verified.length, 2, "no Turnstile check to remove");
  });

  it("is for the site's pages, within the limit, once configured", async () => {
    assert.equal((await call("DELETE", { origin: null })).status, 403);
    assert.equal((await call("DELETE", { origin: "https://evil.example" })).status, 403);
    for (let i = 0; i < MAX_POSTS; i++) assert.equal((await call("DELETE")).status, 200);
    assert.equal((await call("DELETE")).status, 429);
    delete env.IP_SALT;
    assert.equal((await call("DELETE", { ip: "198.51.100.3" })).status, 503);
  });
});

describe("what a rating must be", () => {
  for (const stars of [0, 6, -1, 3.5, "4", null, true, [4], 1e9]) {
    it(`refuses stars = ${JSON.stringify(stars)}`, async () => {
      const res = await vote(stars);
      assert.equal(res.status, 400);
      assert.equal((await res.json()).error, "stars");
      assert.equal(verified.length, 0, "Turnstile isn't asked");
      assert.deepEqual(env.DB.rows("SELECT COUNT(*) AS n FROM ratings"), [{ n: 0 }]);
    });
  }

  it("needs a Turnstile token", async () => {
    assert.equal((await call("POST", { body: { stars: 4 } })).status, 400);
    assert.equal((await call("POST", { body: { stars: 4, turnstileToken: "" } })).status, 400);
    assert.equal((await call("POST", { body: { stars: 4, turnstileToken: "x".repeat(2049) } })).status, 400);
  });

  it("refuses a token Turnstile refuses", async () => {
    const res = await vote(4, { token: "bad" });
    assert.equal(res.status, 403);
    assert.equal((await res.json()).error, "turnstile");
    assert.deepEqual(env.DB.rows("SELECT COUNT(*) AS n FROM ratings"), [{ n: 0 }]);
  });

  it("refuses a token for another site or action", async () => {
    turnstileAnswer = () => Response.json({ success: true, hostname: "evil.example", action: "rate" });
    assert.equal((await vote(4)).status, 403);
    turnstileAnswer = () => Response.json({ success: true, hostname: "misterdownloadmanager.com", action: "login" });
    assert.equal((await vote(4)).status, 403);
    env.TURNSTILE_HOSTNAMES = "";
    turnstileAnswer = () => Response.json({ success: true, hostname: "example.com", action: "" });
    assert.equal((await vote(4)).status, 200, "Turnstile's test keys, with no host names to check");
  });

  it("says so when Turnstile can't be asked", async () => {
    turnstileAnswer = () => new Response("down", { status: 503 });
    assert.equal((await vote(4)).status, 502);
    turnstileAnswer = () => { throw new TypeError("network"); };
    assert.equal((await vote(4)).status, 502);
    assert.deepEqual(env.DB.rows("SELECT COUNT(*) AS n FROM ratings"), [{ n: 0 }]);
  });

  it("takes JSON only, and small", async () => {
    assert.equal((await call("POST", { body: "stars=4", headers: { "Content-Type": "application/x-www-form-urlencoded" } })).status, 415);
    assert.equal((await call("POST", { body: "{not json" })).status, 400);
    assert.equal((await call("POST", { body: { stars: 4, turnstileToken: "good", pad: "x".repeat(5000) } })).status, 413);
  });

  it("takes ratings from the site's pages only", async () => {
    assert.equal((await vote(4, { origin: "https://evil.example" })).status, 403);
    assert.equal((await vote(4, { origin: null })).status, 403);
    assert.equal(verified.length, 0);
  });

  it("needs to know who is asking", async () => {
    assert.equal((await vote(4, { ip: null })).status, 400);
  });

  it("is closed until its secrets are set", async () => {
    delete env.IP_SALT;
    assert.equal((await vote(4)).status, 503);
    env.IP_SALT = "too-short";
    assert.equal((await vote(4)).status, 503);
    env.IP_SALT = SALT;
    delete env.TURNSTILE_SECRET;
    assert.equal((await vote(4)).status, 503);
    const read = await call("GET");
    assert.equal(read.status, 200);
    assert.equal((await read.json()).mine, null);
  });
});

describe("the limit", () => {
  it(`allows ${MAX_POSTS} attempts per visitor in ${WINDOW_S} s`, async () => {
    for (let i = 0; i < MAX_POSTS; i++) assert.equal((await vote(1 + (i % 5))).status, 200);
    const res = await vote(5);
    assert.equal(res.status, 429);
    assert.equal(res.headers.get("Retry-After"), String(WINDOW_S));
    assert.equal(verified.length, MAX_POSTS, "Turnstile isn't asked over the limit");
    assert.equal((await vote(5, { ip: "198.51.100.9" })).status, 200, "another visitor isn't limited");
  });

  it("counts refused attempts too", async () => {
    for (let i = 0; i < MAX_POSTS; i++) await vote(4, { token: "bad" });
    assert.equal((await vote(4)).status, 429);
  });

  it("forgets attempts after an hour", async () => {
    const old = Math.floor(Date.now() / 1000) - ATTEMPTS_KEEP_S - 5;
    env.DB.db.prepare("INSERT INTO attempts (voter, at) VALUES ('someone', ?)").run(old);
    await vote(4);
    assert.deepEqual(env.DB.rows("SELECT COUNT(*) AS n FROM attempts WHERE voter = 'someone'"), [{ n: 0 }]);
  });
});

describe("ipKey", () => {
  it("keeps IPv4 as it is and writes IPv6 networks one way", () => {
    assert.equal(ipKey("203.0.113.7"), "203.0.113.7");
    assert.equal(ipKey("::ffff:203.0.113.7"), "203.0.113.7");
    assert.equal(ipKey("2001:DB8::1"), "2001:0db8:0000:0000::/64");
    assert.equal(ipKey("2001:db8:0:0:ffff::"), "2001:0db8:0000:0000::/64");
    assert.equal(ipKey("fe80::1%eth0"), "fe80:0000:0000:0000::/64");
    assert.equal(ipKey("::1"), "0000:0000:0000:0000::/64");
    assert.equal(ipKey("not an address"), "not an address");
  });
});

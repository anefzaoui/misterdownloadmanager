// The site's live figures (GET /api/stats, src/stats.js) with GitHub, the Firefox add-ons API and
// the Chrome Web Store's listing stood in for, and the Cache API with a Map.
//
//   npm test (node --test test/*.test.mjs)
import assert from "node:assert/strict";
import { afterEach, beforeEach, describe, it } from "node:test";

import { CWS_ID, FRESH_S, STATS_PATH, abbrev, countDownloads, forget, parseChromeUsers } from "../src/stats.js";
import worker from "../src/worker.js";

const SITE = "https://misterdownloadmanager.com";
const realFetch = globalThis.fetch;
const realCaches = globalThis.caches;
const realNow = Date.now;

// The names releases really carry (0.5.x, and the data release's): every one of them counts.
const NAMES = [
  "MisterDownloadManager-0.5.3-setup.exe",
  "MisterDownloadManager-0.5.3-portable.zip",
  "MisterDownloadManager-0.5.3-x64.zip",
  "MisterDownloadManager-0.5.3-macos-arm64-experimental.dmg",
  "MisterDownloadManager-0.5.3-linux-x86_64.AppImage",
  "MisterDownloadManager-0.5.3-linux-x86_64.deb",
  "MisterDownloadManager-0.5.3-linux-x86_64.rpm",
  "MisterDownloadManager-0.5.3-linux-x86_64.tar.gz",
  "MisterDownloadManager-0.5.3-symbols-windows-x64.zip",
  "MisterDownloadManager-0.5.3-symbols-linux-x86_64.zip",
  "MisterDownloadManager-0.5.3-extension-chromium.zip",
  "MisterDownloadManager-0.5.3-extension-firefox.xpi",
  "MisterDownloadManager-0.5.3-chrome-web-store.zip",
  "MisterDownloadManager-0.5.3-linux-x86_64.AppImage.zsync",
  "SHA256SUMS.txt",
  "SHA256SUMS.txt.sig",
  "flags.png",
  "ipcountry.bin.xz",
];

const cwsPage = (users) =>
  `<html><script>AF_initDataCallback({key: 'ds:0', hash: '2', data:[["${CWS_ID}","https://lh3/x","Mister Download Manager",4.94,72,"https://lh3/y","Hands downloads",null,null,null,null,null,null,null,${users},"extra"]], sideChannel: {}});</script>` +
  `<div>Tools</a>${Number(users).toLocaleString("en-US")} users</div></html>`;

let answers; // url prefix -> () => Response
let asked; // urls fetched
let cache;
let pending; // ctx.waitUntil's promises
let now;

beforeEach(() => {
  forget();
  now = Date.parse("2026-11-27T12:00:00Z");
  Date.now = () => now;
  asked = [];
  pending = [];
  cache = new Map();
  globalThis.caches = {
    default: {
      async match(key) {
        const v = cache.get(String(key));
        return v ? new Response(v) : undefined;
      },
      async put(key, res) {
        cache.set(String(key), await res.text());
      },
    },
  };
  const releases = [
    { assets: [
      { name: "MisterDownloadManager-0.5.3-setup.exe", download_count: 1000 },
      { name: "MisterDownloadManager-0.5.3-linux-x86_64.deb", download_count: 20 },
      { name: "MisterDownloadManager-0.5.3-extension-chromium.zip", download_count: 500 },
      { name: "SHA256SUMS.txt", download_count: 300 },
    ] },
    { assets: [{ name: "MisterDownloadManager-0.4.0-portable.zip", download_count: 5 }] },
  ];
  answers = {
    "https://api.github.com/repos/anefzaoui/misterdownloadmanager/releases?per_page=100&page=1": () => Response.json(releases),
    "https://addons.mozilla.org/api/v5/addons/addon/mister-download-manager/": () => Response.json({ average_daily_users: 11286 }),
    "https://chromewebstore.google.com/detail/mister-download-manager/": () => new Response(cwsPage(300000)),
  };
  globalThis.fetch = async (url, init) => {
    const u = String(url);
    asked.push(u);
    assert.equal(init.headers["User-Agent"], "misterdownloadmanager-stats");
    assert.ok(init.signal, "every request has a timeout");
    const key = Object.keys(answers).find((k) => u.startsWith(k));
    if (!key) throw new Error(`unexpected fetch ${u}`);
    return answers[key]();
  };
});

afterEach(() => {
  globalThis.fetch = realFetch;
  globalThis.caches = realCaches;
  Date.now = realNow;
});

function call(method = "GET", { origin = SITE } = {}) {
  const headers = origin ? { Origin: origin } : {};
  const ctx = { waitUntil: (p) => pending.push(p) };
  return worker.fetch(new Request(`https://ratings.example${STATS_PATH}`, { method, headers }), {
    ALLOWED_ORIGINS: `${SITE},http://127.0.0.1:8765`,
  }, ctx);
}

const settle = () => Promise.all(pending.splice(0));
const at = (iso) => ({ fetched: iso });

describe("the live figures", () => {
  it("answers each figure with when it was read", async () => {
    const res = await call();
    assert.equal(res.status, 200);
    assert.equal(res.headers.get("Access-Control-Allow-Origin"), SITE);
    assert.equal(res.headers.get("Cache-Control"), "public, max-age=300");
    const t = "2026-11-27T12:00:00.000Z";
    assert.deepEqual(await res.json(), {
      checked: t,
      downloads: { value: 1825, ...at(t) },
      chrome: { value: 300000, ...at(t) },
      firefox: { value: 11286, ...at(t) },
    });
  });

  it("reads GitHub's releases page after page", async () => {
    const full = Array.from({ length: 100 }, () => ({ assets: [{ name: "MisterDownloadManager-0.1.0-setup.exe", download_count: 1 }] }));
    answers["https://api.github.com/repos/anefzaoui/misterdownloadmanager/releases?per_page=100&page=1"] = () => Response.json(full);
    answers["https://api.github.com/repos/anefzaoui/misterdownloadmanager/releases?per_page=100&page=2"] = () =>
      Response.json([{ assets: [{ name: "MisterDownloadManager-0.0.1-x64.zip", download_count: 7 }] }]);
    assert.equal((await (await call()).json()).downloads.value, 107);
    assert.equal(asked.filter((u) => u.includes("/releases?")).length, 2);
  });

  it("asks the sources at most once every 15 minutes, then answers the stale figures while it reads them again", async () => {
    await call();
    assert.equal(asked.length, 3);
    now += (FRESH_S - 1) * 1000;
    await call();
    assert.equal(asked.length, 3, "fresh: nothing asked");

    forget(); // another isolate in the same location: the Cache API's copy
    await call();
    assert.equal(asked.length, 3);

    now += 2000;
    answers["https://addons.mozilla.org/api/v5/addons/addon/mister-download-manager/"] = () => Response.json({ average_daily_users: 12000 });
    const stale = await (await call()).json();
    assert.equal(stale.firefox.value, 11286, "the stale answer at once");
    assert.equal(pending.length, 1, "read again in the background");
    await settle();
    assert.equal(asked.length, 6);
    const fresh = await (await call()).json();
    assert.equal(fresh.firefox.value, 12000);
    assert.equal(fresh.firefox.fetched, new Date(now).toISOString());
  });

  it("keeps a source's last good figure, and the time it was read, when it can't be read", async () => {
    const first = await (await call()).json();
    now += (FRESH_S + 1) * 1000;
    answers["https://chromewebstore.google.com/detail/mister-download-manager/"] = () => new Response("<html>a changed page</html>");
    answers["https://addons.mozilla.org/api/v5/addons/addon/mister-download-manager/"] = () => new Response("busy", { status: 503 });
    answers["https://api.github.com/repos/anefzaoui/misterdownloadmanager/releases?per_page=100&page=1"] = () => {
      throw new TypeError("network");
    };
    await call();
    await settle();
    const later = await (await call()).json();
    assert.deepEqual(later.chrome, first.chrome);
    assert.deepEqual(later.firefox, first.firefox);
    assert.deepEqual(later.downloads, first.downloads);
    assert.equal(later.checked, new Date(now).toISOString());
  });

  it("keeps the last good figure when a new one drops too far (someone else's number on a changed page)", async () => {
    await call();
    now += (FRESH_S + 1) * 1000;
    answers["https://chromewebstore.google.com/detail/mister-download-manager/"] = () => new Response(cwsPage(1200));
    await call();
    await settle();
    assert.equal((await (await call()).json()).chrome.value, 300000);
  });

  it("says null for a figure never read, and never makes one up", async () => {
    answers["https://chromewebstore.google.com/detail/mister-download-manager/"] = () => new Response("<html>consent</html>");
    const body = await (await call()).json();
    assert.deepEqual(body.chrome, { value: null, fetched: null });
    assert.equal(body.firefox.value, 11286);
  });

  it("answers the site's build (no Origin) without CORS, and refuses other sites' pages", async () => {
    const own = await call("GET", { origin: null });
    assert.equal(own.status, 200);
    assert.equal(own.headers.get("Access-Control-Allow-Origin"), null);
    const other = await call("GET", { origin: "https://example.com" });
    assert.equal(other.status, 403);
    assert.equal(other.headers.get("Access-Control-Allow-Origin"), null);
  });

  it("answers a preflight, and only reads", async () => {
    const pre = await call("OPTIONS");
    assert.equal(pre.status, 204);
    assert.equal(pre.headers.get("Access-Control-Allow-Methods"), "GET, HEAD, OPTIONS");
    for (const m of ["POST", "PUT", "DELETE"]) {
      const res = await call(m);
      assert.equal(res.status, 405);
      assert.equal(res.headers.get("Allow"), "GET, HEAD, OPTIONS");
    }
  });

  it("leaves the ratings' routes as they were", async () => {
    const res = await worker.fetch(new Request("https://ratings.example/api/stats/x", { headers: { Origin: SITE } }), { ALLOWED_ORIGINS: SITE });
    assert.equal(res.status, 404);
    assert.deepEqual(await res.json(), { error: "not-found" });
  });
});

describe("reading the sources", () => {
  it("counts every file of every release", () => {
    assert.equal(countDownloads([{ assets: NAMES.map((name) => ({ name, download_count: 1 })) }]), NAMES.length);
    assert.equal(countDownloads([{ assets: [{ name: "a", download_count: 3 }] }, { assets: [{ name: "b", download_count: 4 }] }, {}]), 7);
    // Nothing that isn't a count: a missing, negative or fractional download_count adds nothing.
    assert.equal(countDownloads([{ assets: [{ name: "a" }, { name: "b", download_count: -2 }, { name: "c", download_count: 1.5 }, null] }]), 0);
  });

  it("reads the Chrome Web Store's users from the page's data, else from its text", () => {
    assert.equal(parseChromeUsers(cwsPage(300000)), 300000);
    assert.equal(parseChromeUsers('<div>Tools</a>300,000 users</div>'), 300000);
    assert.equal(parseChromeUsers('<span>200K</span> <span class="x">users</span>'), 200000);
    assert.equal(parseChromeUsers(cwsPage(300000).replace(CWS_ID, "someoneelse").replace(/300,000 users/, "")), null);
    assert.equal(parseChromeUsers("<html>Before you continue to Google</html>"), null);
    assert.equal(parseChromeUsers(null), null);
  });

  it("reads abbreviated figures", () => {
    assert.equal(abbrev("8,353"), 8353);
    assert.equal(abbrev("1.2M"), 1200000);
    assert.equal(abbrev("200K+"), 200000);
    assert.equal(abbrev("many"), null);
  });
});

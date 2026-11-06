// Runs extension/background.js against fake Firefox and Chrome APIs (no browser needed):
//   node tools/extension/test.mjs
import { readFileSync, readdirSync } from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";

const code = readFileSync(new URL("../../extension/background.js", import.meta.url), "utf8");
const DEFAULT_TYPES = "3GP 7Z AAC EXE ISO MP3 MP4 MSI PDF R0* RAR ZIP";

// A fake extension API; `firefox` picks which download event exists. `reply` is the app's
// answer to a download; `types` its Options -> File types when the rules are asked (`host: false`:
// the host isn't registered and every native message fails, as the browser says then).
// `appName`: what runtime.getBrowserInfo says (Firefox's family only; "Zen").
// `session`: storage.session's contents (shared between two loads: the browser stopped the
// script and started it again); `frames`: what each frame's players are (mediaElements);
// `tabs`: tabs.get's answers by id.
function fakeApi({ firefox, reply = { ok: true }, types = DEFAULT_TYPES, host = true, finished, panelOn, access = true, rules = {}, appName, proxy, session, frames, tabs = {} }) {
  const raw = [];
  // Objects made inside the sandbox have its prototypes: compare plain copies.
  const calls = { push: (c) => raw.push(JSON.parse(JSON.stringify(c))) };
  const listeners = {};
  // Every listener of an event is called (background.js has two for some); the answer is the
  // last one given.
  const event = (name) => {
    const fns = [];
    return {
      addListener: (f) => {
        fns.push(f);
        listeners[name] = (...a) => fns.reduce((r, g) => g(...a) ?? r, undefined);
      },
    };
  };
  const store = {};
  const downloads = {
    pause: async (id) => calls.push(["pause", id]),
    cancel: async (id) => calls.push(["cancel", id]),
    erase: async (q) => calls.push(["erase", q.id]),
    resume: async (id) => calls.push(["resume", id]),
    // `finished`: the download completed before the extension could stop it (a small file).
    search: async ({ id }) => [{ id, state: finished ? "complete" : "interrupted", exists: true }],
    removeFile: async (id) => calls.push(["removeFile", id]),
  };
  if (firefox) downloads.onCreated = event("created");
  else downloads.onDeterminingFilename = event("determining");
  const api = {
    downloads,
    storage: {
      local: {
        get: async (defaults) => ({ ...defaults, ...JSON.parse(JSON.stringify(store)) }),
        set: async (v) => Object.assign(store, JSON.parse(JSON.stringify(v))),
      },
      ...(session
        ? {
            session: {
              get: async (defaults) => ({ ...defaults, ...JSON.parse(JSON.stringify(session)) }),
              set: async (v) => Object.assign(session, JSON.parse(JSON.stringify(v))),
            },
          }
        : {}),
    },
    cookies: {
      getAll: async (q) => {
        calls.push(["cookies", q]);
        return [{ name: "sid", value: "1" }];
      },
    },
    permissions: { contains: async () => access },
    runtime: {
      sendNativeMessage: async (name, m) => {
        if (!host) throw new Error("Specified native messaging host not found.");
        if (m.rules) {
          calls.push(["rules", m]);
          return { ok: true, version: "0.2.5", types, capture: true, excludedSites: "*.update.microsoft.com", excludedAddresses: [], opened: true, ...rules };
        }
        calls.push(["native", name, m]);
        // The video button: "may it show" follows `panelOn`.
        if (m.panel) return { ok: panelOn !== false };
        return reply;
      },
      getManifest: () => ({ version: "0.2.5", content_scripts: [{ js: ["video.js"] }, { js: ["page.js"] }] }),
      onInstalled: event("installed"),
      onStartup: event("startup"),
      onMessage: event("message"),
      id: "ours",
      getURL: (p) => `chrome-extension://ours/${p}`,
    },
    tabs: {
      onCreated: event("tabCreated"),
      onRemoved: event("tabRemoved"),
      onUpdated: event("tabUpdated"),
      get: async (id) => tabs[id] || { id, url: "about:blank", status: "complete" },
      remove: async (id) => calls.push(["closeTab", id]),
      query: async () => [],
      create: async (t) => calls.push(["openTab", t.url]),
    },
    webRequest: {
      onBeforeRequest: event("beforeRequest"),
      onBeforeSendHeaders: event("beforeSendHeaders"),
      onHeadersReceived: event("headersReceived"),
      onResponseStarted: event("responseStarted"),
      onAuthRequired: event("authRequired"),
    },
    scripting: {
      executeScript: async ({ func }) => {
        if (func && func.name === "mediaElements") {
          if (!frames) throw new Error("Cannot access contents of the page");
          return Object.entries(frames).map(([frameId, result]) => ({ frameId: Number(frameId), result }));
        }
        return func ? [{ result: ["https://x/1.zip", "https://x/2.zip"] }] : [];
      },
    },
    action: {
      setBadgeText: async (b) => calls.push(["badge", b.text]),
      setBadgeBackgroundColor: async () => {},
      setTitle: async () => {},
    },
    contextMenus: {
      create: (item, done) => {
        calls.push(["menu-item", item]);
        if (done) done();
      },
      removeAll: (done) => done(),
      onClicked: event("menu"),
    },
    // The browser's language: German here.
    i18n: { getMessage: (key) => ({ menu: "Mit Mister Download Manager herunterladen" })[key] ?? "" },
  };
  if (!firefox) api.declarativeNetRequest = { getSessionRules: async () => [], updateSessionRules: async (r) => calls.push(["dnr", r]) };
  if (appName) api.runtime.getBrowserInfo = async () => ({ name: appName, vendor: "Mozilla", version: "1.0" });
  // `proxy`: what proxy.settings.get says (#42; Chromium {mode, rules, pacScript}, Firefox {proxyType, …}).
  if (proxy) api.proxy = { settings: { get: async () => ({ value: proxy, levelOfControl: "controlled_by_other_extensions" }) } };
  return { api, calls: raw, listeners, store };
}

// The servers the extension reads manifests from (#38): address -> {status, body}.
let served = {};
const fetched = [];
async function fakeFetch(url, opts) {
  fetched.push([String(url), opts && opts.credentials]);
  const r = served[url];
  if (!r) throw new TypeError("Failed to fetch");
  // Its body as a stream too, 1 KB a chunk: `r.read` counts what was pulled (#171 reads 4 KB).
  const bytes = new TextEncoder().encode(r.body);
  r.read = 0;
  const body = new ReadableStream({
    pull(c) {
      if (r.read >= bytes.length) return c.close();
      c.enqueue(bytes.slice(r.read, r.read + 1024));
      r.read += 1024;
    },
  });
  return { ok: (r.status || 200) < 300, status: r.status || 200, headers: { get: () => null }, body, text: async () => r.body };
}

function load(api, firefox) {
  const ctx = {
    fetch: fakeFetch,
    navigator: { userAgent: "UA", userAgentData: { brands: [{ brand: "Chromium" }, { brand: "Brave" }] } },
    console,
    URL,
    URLSearchParams,
    TextDecoder,
    setTimeout,
    clearTimeout,
    Promise,
  };
  if (firefox) ctx.browser = api;
  else ctx.chrome = api;
  vm.runInNewContext(code, ctx);
}

const settle = () => new Promise((r) => setTimeout(r, 30));
const natives = (calls) => calls.filter((c) => c[0] === "native").map((c) => c[2]);
const steps = (calls) => calls.filter((c) => ["pause", "cancel", "erase", "resume", "removeFile"].includes(c[0])).map((c) => c[0]);
let passed = 0;
const ok = (what) => {
  passed++;
  console.log(`ok - ${what}`);
};

// Firefox: a .zip in a container is taken over with that container's cookies and the browser's
// name, size and type; every message says which browser and version it comes from.
{
  const { api, calls, listeners } = fakeApi({ firefox: true });
  load(api, true);
  listeners.created({
    id: 7,
    url: "https://x/a.zip",
    filename: "C:\\dl\\a.zip",
    referrer: "https://page/",
    cookieStoreId: "firefox-container-2",
    totalBytes: 2048,
    mime: "application/zip",
  });
  await settle();
  const cookies = calls.find((c) => c[0] === "cookies");
  assert.deepEqual(cookies[1], { url: "https://x/a.zip", storeId: "firefox-container-2" });
  const native = calls.find((c) => c[0] === "native");
  assert.equal(native[1], "com.misterdownloadmanager.host");
  const m = native[2];
  assert.deepEqual(
    [m.url, m.referrer, m.cookies, m.filename, m.size, m.mime, m.userAgent, m.v],
    ["https://x/a.zip", "https://page/", "sid=1", "a.zip", 2048, "application/zip", "UA", "0.2.5"],
  );
  assert.deepEqual(m.brands, ["Chromium", "Brave"]);
  assert.deepEqual(steps(calls), ["pause", "cancel", "erase"]);
  ok("Firefox: taken with its container's cookies, size and type");
}

// Zen (Firefox's family) says its own name through runtime.getBrowserInfo: it goes with the
// brands, so the app tells Zen from Firefox (#41).
{
  const { api, calls, listeners } = fakeApi({ firefox: true, appName: "Zen" });
  load(api, true);
  listeners.created({ id: 8, url: "https://x/b.zip", filename: "C:\\dl\\b.zip", totalBytes: 10, mime: "application/zip" });
  await settle();
  const m = natives(calls)[0];
  assert.deepEqual(m.brands, ["Chromium", "Brave", "Zen"]);
  ok("Zen: its own name goes with the brands");
}

// Firefox: the app doesn't take it → the browser's download resumes; nothing is lost.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, reply: { ok: false } });
  load(api, true);
  listeners.created({ id: 8, url: "https://x/b.iso", filename: "/home/u/b.iso" });
  await settle();
  assert.deepEqual(steps(calls), ["pause", "resume"]);
  ok("Firefox: declined, resumed");
}

// A type not on the list (a web page) stays with the browser.
{
  const { api, calls, listeners } = fakeApi({ firefox: true });
  load(api, true);
  listeners.created({ id: 9, url: "https://x/page.html", filename: "C:\\dl\\page.html" });
  await settle();
  assert.deepEqual(natives(calls), []);
  assert.deepEqual(steps(calls), []);
  ok("a type not listed stays with the browser");
}

// Chrome: incognito downloads use the incognito store; the address asked for goes as the
// download's (an expired signed link is renewed from it), the redirect's end beside it.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  let suggested = false;
  listeners.determining(
    { id: 3, url: "https://x/redirect", finalUrl: "https://cdn/x.msi", filename: "x.msi", incognito: true },
    () => (suggested = true),
  );
  await settle();
  assert.ok(!suggested, "a download the app took is never named: Chrome wrote none of it");
  const cookies = calls.find((c) => c[0] === "cookies");
  assert.deepEqual(cookies[1], { url: "https://x/redirect", storeId: "1" });
  const [m] = natives(calls);
  assert.deepEqual([m.url, m.finalUrl, m.private], ["https://x/redirect", "https://cdn/x.msi", true]);
  assert.deepEqual(steps(calls), ["cancel", "erase"], "held at its name, not paused and resumed");
  ok("Chrome: taken at its name, incognito cookies, both addresses");
}

// Chrome: one the app doesn't take goes on as the browser's own (its name confirmed).
{
  const { api, calls, listeners } = fakeApi({ firefox: false, reply: { ok: false } });
  load(api, false);
  let suggested = false;
  listeners.determining({ id: 4, url: "https://x/y.zip", filename: "y.zip" }, () => (suggested = true));
  await settle();
  assert.ok(suggested);
  assert.deepEqual(steps(calls), []);
  ok("Chrome: declined, the browser's own");
}

// Firefox: a small file finished before it could be stopped: the browser's copy is removed.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, finished: true });
  load(api, true);
  listeners.created({ id: 5, url: "https://x/s.zip", filename: "C:\\dl\\s.zip" });
  await settle();
  assert.ok(calls.some((c) => c[0] === "removeFile" && c[1] === 5));
  assert.ok(calls.some((c) => c[0] === "erase" && c[1] === 5));
  ok("Firefox: a finished small file is removed");
}

// Options -> File types in the app decides what is offered: its list, not a built-in one.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, types: "PNG ISO" });
  load(api, true);
  listeners.created({ id: 20, url: "https://x/pic.png", filename: "C:\\dl\\pic.png" });
  await settle();
  listeners.created({ id: 21, url: "https://x/a.zip", filename: "C:\\dl\\a.zip" });
  await settle();
  assert.deepEqual(natives(calls).map((m) => m.filename), ["pic.png"], "PNG is the app's, ZIP is not listed there");
  assert.equal(calls.filter((c) => c[0] === "rules").length, 1, "the rules kept, not asked per download");
  ok("the app's file types decide, asked once");
}

// Mister Download Manager not installed (no host): everything stays with the browser, and the
// toolbar button says something is wrong.
{
  const { api, calls, listeners, store } = fakeApi({ firefox: false, host: false });
  load(api, false);
  let suggested = false;
  listeners.determining({ id: 30, url: "https://x/a.zip", filename: "a.zip" }, () => (suggested = true));
  await settle();
  assert.ok(suggested);
  assert.deepEqual(steps(calls), []);
  assert.equal(store.status.why, "not-installed");
  assert.ok(calls.some((c) => c[0] === "badge" && c[1] === "!"));
  // Firefox isn't paused for nothing either.
  const ff = fakeApi({ firefox: true, host: false });
  load(ff.api, true);
  ff.listeners.created({ id: 31, url: "https://x/a.zip", filename: "a.zip" });
  await settle();
  assert.deepEqual(steps(ff.calls), []);
  ok("no host: nothing taken, nothing paused, the badge says so");
}

// The extension installed without the app: the help page opens at its section.
{
  const { api, calls, listeners } = fakeApi({ firefox: false, host: false });
  load(api, false);
  await listeners.installed({ reason: "install" });
  await settle();
  assert.ok(calls.some((c) => c[0] === "openTab" && c[1].endsWith("#not-installed")));
  const items = calls.filter((c) => c[0] === "menu-item").map((c) => c[1].id);
  assert.deepEqual(items, ["mdm-link", "mdm-selected", "mdm-all"]);
  ok("installed without the app: the help page, the menus");
}

// A form's download: posted again by the app with the same body (it can't be a GET).
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  listeners.beforeRequest({ method: "POST", url: "https://x/form", requestId: "9", requestBody: { formData: { token: ["abc"], note: ["hello world"] } } });
  listeners.beforeSendHeaders({ method: "POST", url: "https://x/form", requestId: "9", requestHeaders: [{ name: "Content-Type", value: "application/x-www-form-urlencoded" }] });
  listeners.determining({ id: 40, url: "https://x/form", filename: "invoice.pdf" }, () => {});
  await settle();
  const [m] = natives(calls);
  assert.deepEqual([m.post, m.postType], ["token=abc&note=hello+world", "application/x-www-form-urlencoded"]);
  // A form with a file in it can't be posted again: the browser keeps its download.
  listeners.beforeRequest({ method: "POST", url: "https://x/upload", requestId: "10", requestBody: { raw: [{ file: "C:\\a.txt" }] } });
  let suggested = false;
  listeners.determining({ id: 41, url: "https://x/upload", filename: "out.zip" }, () => (suggested = true));
  await settle();
  assert.ok(suggested);
  assert.equal(natives(calls).length, 1);
  // A multipart form: the browser gives its fields without their files, so it stays there too.
  listeners.beforeRequest({ method: "POST", url: "https://x/multi", requestId: "11", requestBody: { formData: { a: ["1"] } } });
  listeners.beforeSendHeaders({ method: "POST", url: "https://x/multi", requestId: "11", requestHeaders: [{ name: "Content-Type", value: "multipart/form-data; boundary=xyz" }] });
  let kept = false;
  listeners.determining({ id: 42, url: "https://x/multi", filename: "out.zip" }, () => (kept = true));
  await settle();
  assert.ok(kept);
  assert.equal(natives(calls).length, 1);
  ok("forms: posted again with their body, or left to the browser");
}

// Alt held on a link, a site the extension may not read, an excluded site, a blob: address:
// each stays with the browser without asking the app.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  const send = (m, sender) => new Promise((r) => listeners.message(m, sender, r));
  await send({ type: "bypass", url: "https://x/alt.zip" }, { id: "ours", url: "https://x/", tab: { url: "https://x/" } });
  const keep = async (item) => {
    let suggested = false;
    listeners.determining(item, () => (suggested = true));
    await settle();
    return suggested;
  };
  assert.ok(await keep({ id: 50, url: "https://x/alt.zip", filename: "alt.zip" }), "Alt");
  assert.ok(await keep({ id: 51, url: "https://dl.update.microsoft.com/kb.exe", filename: "kb.exe" }), "excluded site");
  assert.ok(await keep({ id: 52, url: "blob:https://x/1234", filename: "a.zip" }), "blob:");
  assert.deepEqual(natives(calls), []);
  const limited = fakeApi({ firefox: false, access: false });
  load(limited.api, false);
  let suggested = false;
  limited.listeners.determining({ id: 53, url: "https://x/a.zip", filename: "a.zip" }, () => (suggested = true));
  await settle();
  assert.ok(suggested && natives(limited.calls).length === 0, "site access withdrawn");
  ok("Alt, excluded sites, blob: and withdrawn site access stay with the browser");
}

// Paused from the toolbar button: nothing is taken until it is on again.
{
  const { api, calls, listeners, store } = fakeApi({ firefox: false });
  load(api, false);
  const send = (m) => new Promise((r) => listeners.message(m, { id: "ours" }, (v) => r(JSON.parse(JSON.stringify(v)))));
  const state = await send({ type: "state" });
  assert.deepEqual([state.connected, state.app, state.paused], [true, "0.2.5", false]);
  await send({ type: "pause", paused: true });
  assert.equal(store.paused, true);
  let suggested = false;
  listeners.determining({ id: 60, url: "https://x/a.zip", filename: "a.zip" }, () => (suggested = true));
  await settle();
  assert.ok(suggested && natives(calls).length === 0);
  assert.ok(calls.some((c) => c[0] === "badge" && c[1] === "II"));
  await send({ type: "exclude-site", host: "files.test" });
  assert.ok(natives(calls).some((m) => m.excludeSite === "files.test"));
  ok("the popup: state, pause, never this site");
}

// An app older than the extension: still connected, with a nudge to update it.
{
  const { api, store, listeners } = fakeApi({ firefox: false, rules: { version: "0.1.0" } });
  load(api, false);
  listeners.startup();
  await settle();
  assert.equal(store.status && store.status.why, "update-app");
  assert.equal(store.status.app, "0.1.0");
  ok("an older app: update-app");
}

// Files the browser would open (a PDF): a session rule makes them downloads, for the listed types
// only; none when the app says not to.
{
  const { api, calls } = fakeApi({ firefox: false, types: "PDF MP3 ZIP" });
  load(api, false);
  await settle();
  const rule = calls.filter((c) => c[0] === "dnr").pop()[1];
  const values = rule.addRules[0].condition.responseHeaders[0].values;
  assert.ok(values.includes("application/pdf*") && values.includes("audio/mpeg*"));
  assert.ok(!values.some((v) => v.startsWith("video/mp4")), "MP4 not listed");
  // Options' excluded sites keep opening in the browser: *. with its subdomains, a plain host
  // by itself (an allow rule).
  const ex = fakeApi({ firefox: false, types: "PDF", rules: { excludedSites: "*.docs.test files.test:8080" } });
  load(ex.api, false);
  await settle();
  const added = ex.calls.filter((c) => c[0] === "dnr").pop()[1].addRules;
  assert.deepEqual(added[0].condition.excludedRequestDomains, ["docs.test"]);
  assert.equal(added[1].action.type, "allow");
  assert.equal(added[1].condition.regexFilter, "^[a-z]+://files\\.test(:[0-9]+)?/", "the host alone, any port");
  const off = fakeApi({ firefox: false, rules: { opened: false } });
  load(off.api, false);
  await settle();
  assert.deepEqual(off.calls.filter((c) => c[0] === "dnr").pop()[1].addRules, []);
  ok("opened types: the session rule follows the app's list");
}

// Right-click → Download with Mister Download Manager: handed over as an explicit request, so
// the host takes any link (an .html page here); a failure opens the help page's section.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  await listeners.menu({ menuItemId: "mdm-link", linkUrl: "https://x/page.html", pageUrl: "https://x/" }, { incognito: false });
  await settle();
  const [m] = natives(calls);
  assert.deepEqual([m.url, m.explicit, m.referrer], ["https://x/page.html", true, "https://x/"]);
  await listeners.menu({ menuItemId: "mdm-all", pageUrl: "https://x/" }, { id: 1, incognito: false });
  await settle();
  assert.deepEqual(natives(calls)[1].links, ["https://x/1.zip", "https://x/2.zip"]);
  const failing = fakeApi({ firefox: false, reply: { ok: false, why: "not-running" } });
  load(failing.api, false);
  await failing.listeners.menu({ menuItemId: "mdm-link", linkUrl: "https://x/a.zip", pageUrl: "https://x/" }, {});
  await settle();
  assert.ok(failing.calls.some((c) => c[0] === "openTab" && c[1].endsWith("#not-running")));
  ok("context menus: explicit links, a page's links, failures explained");
}

// The download button over videos: the page asks once a minute per site, a click opens the
// app's video window for the page the browser says sent it (with the tab as referrer and its
// cookies), "Never on this site" goes to the app and is remembered at once; nothing from
// another extension is answered.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  const ask = (m, page = "https://tube.test/embed/1", sender) =>
    new Promise((resolve) => {
      // Plain copies: objects from the sandbox have its prototypes.
      const async_ = listeners.message(m, sender || { id: "ours", url: page, tab: { url: "https://blog.test/post", incognito: false } }, (r) =>
        resolve(JSON.parse(JSON.stringify(r))),
      );
      if (!async_) resolve(undefined);
    });
  assert.deepEqual(await ask({ type: "panel" }), { on: true });
  assert.deepEqual(await ask({ type: "panel" }, "https://tube.test/embed/2"), { on: true });
  assert.equal(natives(calls).filter((m) => m.panel).length, 1, "asked once for the site");
  assert.equal((await ask({ type: "video", page: "https://evil.test/" })).ok, true);
  const video = natives(calls).find((m) => m.video);
  assert.equal(video.url, "https://tube.test/embed/1", "the page the browser names, not the message's");
  assert.equal(video.referrer, "https://blog.test/post");
  assert.equal(video.cookies, "sid=1");
  assert.deepEqual(await ask({ type: "hide-site", host: "elsewhere.test" }), { ok: true });
  assert.ok(natives(calls).some((m) => m.hideSite === "tube.test"));
  assert.deepEqual(await ask({ type: "panel" }, "https://tube.test/embed/3"), { on: false }, "off at once");
  assert.deepEqual(await ask({ type: "panel" }, "https://tube.test:8443/embed/4"), { on: false }, "whatever the port");
  assert.deepEqual(await ask({ type: "video" }, "javascript:alert(1)"), { ok: false });
  assert.equal(await ask({ type: "panel" }, "https://x.test/", { id: "someone-else", url: "https://x.test/", tab: {} }), undefined);
}
// A player playing a stream (HLS) from a blob: the button sends the manifest its frame loaded
// first; the tab going elsewhere forgets it.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  const sender = { id: "ours", url: "https://news.test/story", frameId: 0, tab: { id: 5, url: "https://news.test/story" } };
  listeners.responseStarted({ tabId: 5, frameId: 0, url: "https://cdn.test/v/master.m3u8?t=1", responseHeaders: [] });
  listeners.responseStarted({ tabId: 5, frameId: 0, url: "https://cdn.test/v/720p.m3u8", responseHeaders: [] });
  listeners.responseStarted({ tabId: 5, frameId: 0, url: "https://cdn.test/v/seg1.ts", responseHeaders: [{ name: "Content-Type", value: "video/mp2t" }] });
  const send = (m) => new Promise((r) => listeners.message(m, sender, r));
  await send({ type: "video", src: "" });
  assert.equal(natives(calls).find((m) => m.video).src, "https://cdn.test/v/master.m3u8?t=1");
  listeners.tabUpdated(5, { url: "https://news.test/other" });
  await send({ type: "video", src: "" });
  assert.equal(natives(calls).filter((m) => m.video)[1].src, undefined, "forgotten on another page");
  ok("streams: the frame's manifest goes with the button");
}
// #155: the right-click menu on a player playing from a blob: sends the manifest its frame loaded
// (it sent the page alone, which yt-dlp answered "Unsupported URL"), with the manifest's cookies;
// a manifest's own link goes with its cookies too; the button asks for the stream's cookies.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  listeners.responseStarted({ tabId: 7, frameId: 2, url: "https://cdn.test/s/master.m3u8", responseHeaders: [] });
  listeners.responseStarted({ tabId: 7, frameId: 0, url: "https://other.test/x/main.mpd", responseHeaders: [] });
  await listeners.menu(
    { menuItemId: "mdm-link", srcUrl: "blob:https://player.test/1", frameId: 2, frameUrl: "https://player.test/embed", pageUrl: "https://blog.test/post" },
    { id: 7, incognito: false },
  );
  await settle();
  const m = natives(calls).find((x) => x.video);
  assert.deepEqual(
    [m.url, m.src, m.referrer, m.explicit],
    ["https://player.test/embed", "https://cdn.test/s/master.m3u8", "https://blog.test/post", true],
    "the frame's stream, not the page alone",
  );
  assert.ok(calls.some((c) => c[0] === "cookies" && c[1].url === "https://cdn.test/s/master.m3u8"), "the stream's cookies");
  await listeners.menu({ menuItemId: "mdm-link", linkUrl: "https://cdn.test/s/other.m3u8", pageUrl: "https://blog.test/post" }, { id: 7, incognito: false });
  await settle();
  assert.equal(natives(calls).filter((x) => x.video)[1].src, "https://cdn.test/s/other.m3u8");
  assert.ok(calls.some((c) => c[0] === "cookies" && c[1].url === "https://cdn.test/s/other.m3u8"), "the manifest's cookies");
  // Firefox names no address for a player fed by a MediaSource: the same stream goes.
  await listeners.menu({ menuItemId: "mdm-link", mediaType: "video", frameId: 2, frameUrl: "https://player.test/embed", pageUrl: "https://blog.test/post" }, { id: 7, incognito: false });
  await settle();
  assert.equal(natives(calls).filter((x) => x.video).pop().src, "https://cdn.test/s/master.m3u8", "Firefox: no srcUrl");
  // Nothing loaded in that tab: the page, as before (a page yt-dlp may know).
  await listeners.menu({ menuItemId: "mdm-link", srcUrl: "blob:https://tube.test/2", frameId: 0, pageUrl: "https://tube.test/watch" }, { id: 9, incognito: false });
  await settle();
  const page = natives(calls).filter((x) => x.video).pop();
  assert.deepEqual([page.url, page.src], ["https://tube.test/watch", undefined]);
  // The button: the stream's cookies, not the page's.
  const sender = { id: "ours", url: "https://blog.test/post", frameId: 0, tab: { id: 7, url: "https://blog.test/post" } };
  calls.length = 0;
  await new Promise((r) => listeners.message({ type: "video", src: "" }, sender, r));
  assert.ok(calls.some((c) => c[0] === "cookies" && c[1].url === "https://other.test/x/main.mpd"), "the button: the stream's cookies");
  ok("#155: the right-click menu on a stream's player sends its manifest; cookies of the stream");
}
{
  const { api, listeners } = fakeApi({ firefox: true, panelOn: false });
  load(api, true);
  const on = await new Promise((r) =>
    listeners.message({ type: "panel" }, { id: "ours", url: "https://x.test/", tab: {} }, (v) => r(JSON.parse(JSON.stringify(v)))),
  );
  assert.deepEqual(on, { on: false }, "where the app says no");
  ok("video button messages");
}

// #38: the media a tab loaded, as the popup lists them. A response is media by its type or its
// address; segments, byte ranges and small pieces a player fetched are parts of a stream.
const H = (type, more = {}) => [{ name: "Content-Type", value: type }, ...Object.entries(more).map(([name, value]) => ({ name, value }))];
const res = (url, type, headers = [], more = {}) => ({ tabId: 9, frameId: 0, url, type, statusCode: 200, responseHeaders: headers, ...more });
const popupAsk = (listeners, m) =>
  new Promise((r) => listeners.message(m, { id: "ours", url: "chrome-extension://ours/popup.html" }, (v) => r(JSON.parse(JSON.stringify(v)))));
const MASTER = `#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="English",URI="audio/index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=1280x720,AUDIO="aud"
720p/index.m3u8?tok=1
#EXT-X-STREAM-INF:BANDWIDTH=640000,RESOLUTION=640x360,AUDIO="aud"
360p/index.m3u8?tok=1
`;
const MPD = (protect) => `<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT1M30S">
<Period><AdaptationSet mimeType="video/mp4">${protect ? '<ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/>' : ""}
<Representation id="1080p" bandwidth="4000000" width="1920" height="1080"/>
<Representation id="480p" bandwidth="900000" width="854" height="480"/>
</AdaptationSet></Period></MPD>`;
{
  served = {
    "https://cdn.test/v/master.m3u8": { body: MASTER },
    "https://cdn.test/d/manifest.mpd": { body: MPD(false) },
    "https://drm.test/p/manifest.mpd": { body: MPD(true) },
    "https://fair.test/f/master.m3u8": {
      body: '#EXTM3U\n#EXT-X-SESSION-KEY:METHOD=SAMPLE-AES,URI="skd://k",KEYFORMAT="com.apple.streamingkeydelivery"\n#EXT-X-STREAM-INF:RESOLUTION=1920x1080\nhi.m3u8\n',
    },
    "https://aes.test/a/index.m3u8": { body: '#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="key.bin"\n#EXTINF:6.0,\ns0.ts\n#EXTINF:4.5,\ns1.ts\n#EXT-X-ENDLIST\n' },
  };
  const frames = {
    0: { href: "https://watch.test/v/1", players: [{ src: "https://files.test/movie.mp4", w: 1920, h: 1080, duration: 125, drm: false }] },
    3: { href: "https://player.test/embed/7", players: [{ src: "blob:https://player.test/x", w: 0, h: 0, duration: 0, drm: true }] },
  };
  const tabs = { 9: { id: 9, url: "https://watch.test/v/1", title: "A film", incognito: false } };
  const { api, calls, listeners } = fakeApi({ firefox: false, frames, tabs });
  load(api, false);
  listeners.tabUpdated(9, { url: "https://watch.test/v/1" });
  await settle();
  const seen = [
    // HLS: the master, then its variants (with a query) and segments.
    res("https://cdn.test/v/master.m3u8", "xmlhttprequest", H("application/vnd.apple.mpegurl")),
    res("https://cdn.test/v/720p/index.m3u8?tok=1", "xmlhttprequest", H("application/vnd.apple.mpegurl")),
    res("https://cdn.test/v/audio/index.m3u8", "xmlhttprequest", H("audio/mpegurl")),
    res("https://cdn.test/v/720p/seg0.ts", "xmlhttprequest", H("video/mp2t")),
    // DASH, its init segment (video/mp4, in its folder) and fragments.
    res("https://cdn.test/d/manifest.mpd", "xmlhttprequest", H("application/dash+xml")),
    res("https://cdn.test/d/1080p/init.mp4", "xmlhttprequest", H("video/mp4", { "Content-Length": "4000000" })),
    res("https://cdn.test/d/1080p/seg1.m4s", "xmlhttprequest", H("video/iso.segment")),
    // A protected DASH stream, and one in a frame whose manifest can't be read.
    res("https://drm.test/p/manifest.mpd", "xmlhttprequest", H("application/dash+xml")),
    res("https://locked.test/l/master.m3u8", "xmlhttprequest", H("application/x-mpegURL"), { frameId: 3 }),
    res("https://locked.test/l/hd/index.m3u8", "xmlhttprequest", H("application/x-mpegURL"), { frameId: 3 }),
    // FairPlay (SAMPLE-AES), and AES-128 with a plain key (not DRM).
    res("https://fair.test/f/master.m3u8", "xmlhttprequest", H("application/vnd.apple.mpegurl")),
    res("https://aes.test/a/index.m3u8", "xmlhttprequest", H("application/vnd.apple.mpegurl")),
    // A <video>'s own file in ranges (its size from Content-Range), asked twice.
    res("https://files.test/movie.mp4", "media", H("video/mp4", { "Content-Range": "bytes 0-1023/52428800" }), { statusCode: 206 }),
    res("https://files.test/movie.mp4", "media", H("video/mp4", { "Content-Range": "bytes 1024-2047/52428800" }), { statusCode: 206 }),
    // An <audio>'s file sent as octet-stream, known by its extension.
    res("https://files.test/song.mp3?x=1", "media", H("application/octet-stream", { "Content-Length": "3000000" })),
    // A player's fetch: a whole large file is listed; a range, a small piece, a 404 are not.
    res("https://files.test/whole.webm", "xmlhttprequest", H("video/webm", { "Content-Length": "9000000" })),
    res("https://files.test/part.mp4", "xmlhttprequest", H("video/mp4", { "Content-Range": "bytes 0-99/900000000" }), { statusCode: 206 }),
    res("https://yt.test/videoplayback?id=1&range=0-50000", "xmlhttprequest", H("video/mp4", { "Content-Length": "5000000" })),
    res("https://files.test/tiny.mp4", "xmlhttprequest", H("video/mp4", { "Content-Length": "40000" })),
    res("https://files.test/gone.mp4", "media", H("video/mp4"), { statusCode: 404 }),
    // Not media; not the tab's (the extension's own fetch); not a web address.
    res("https://files.test/page.html", "xmlhttprequest", H("text/html", { "Content-Length": "9000000" })),
    res("https://files.test/other.mp4", "media", H("video/mp4"), { tabId: -1 }),
    res("blob:https://files.test/1", "media", H("video/mp4")),
  ];
  for (const d of seen) listeners.responseStarted(d);
  await settle();
  const { media } = await popupAsk(listeners, { type: "media", tab: 9 });
  const by = Object.fromEntries(media.map((m) => [m.url, m]));
  assert.deepEqual(
    media.map((m) => m.url),
    [
      "https://cdn.test/v/master.m3u8",
      "https://cdn.test/d/manifest.mpd",
      "https://drm.test/p/manifest.mpd",
      "https://locked.test/l/master.m3u8",
      "https://fair.test/f/master.m3u8",
      "https://aes.test/a/index.m3u8",
      "https://files.test/movie.mp4",
      "https://files.test/song.mp3?x=1",
      "https://files.test/whole.webm",
    ],
    "streams with their variants inside, files once, no segment or piece",
  );
  const master = by["https://cdn.test/v/master.m3u8"];
  assert.deepEqual([master.kind, master.heights, master.drm, master.live], ["hls", [720, 360], false, false]);
  const dash = by["https://cdn.test/d/manifest.mpd"];
  assert.deepEqual([dash.kind, dash.heights, dash.duration, dash.drm], ["dash", [1080, 480], 90, false]);
  assert.equal(by["https://drm.test/p/manifest.mpd"].drm, true, "Widevine's ContentProtection");
  assert.equal(by["https://locked.test/l/master.m3u8"].drm, true, "unread, its frame's player has a key system");
  assert.equal(by["https://fair.test/f/master.m3u8"].drm, true, "FairPlay, SAMPLE-AES");
  const aes = by["https://aes.test/a/index.m3u8"];
  assert.deepEqual([aes.drm, aes.duration, aes.live], [false, 10.5, false], "AES-128 with its key: not DRM");
  const movie = by["https://files.test/movie.mp4"];
  assert.deepEqual([movie.kind, movie.size, movie.width, movie.heights, movie.duration], ["video", 52428800, 1920, [1080], 125]);
  const song = by["https://files.test/song.mp3?x=1"];
  assert.deepEqual([song.kind, song.size], ["audio", 3000000]);
  assert.ok(fetched.every(([, c]) => c === "include"), "manifests read with the site's cookies");
  const reads = fetched.length;
  await popupAsk(listeners, { type: "media", tab: 9 });
  assert.equal(fetched.length, reads, "each manifest read once");
  ok("media list: streams grouped, files, details, protected ones");

  // Download: a stream goes to the Download video window (explicitly), with the page that played
  // it and the cookies its own server gets; a file to the download path; a protected one nowhere.
  const sent = await popupAsk(listeners, { type: "media-download", tab: 9, url: "https://cdn.test/v/master.m3u8" });
  assert.equal(sent.ok, true);
  const v = natives(calls).find((m) => m.video);
  assert.deepEqual([v.explicit, v.src, v.url, v.title, v.cookies, v.referrer], [true, "https://cdn.test/v/master.m3u8", "https://watch.test/v/1", "A film", "sid=1", ""]);
  assert.deepEqual(calls.filter((c) => c[0] === "cookies").pop()[1], { url: "https://cdn.test/v/master.m3u8" });
  await popupAsk(listeners, { type: "media-download", tab: 9, url: "https://files.test/movie.mp4" });
  const f = natives(calls).find((m) => m.url === "https://files.test/movie.mp4");
  assert.deepEqual([f.explicit, f.referrer, f.size, f.mime, f.video], [true, "https://watch.test/v/1", 52428800, "video/mp4", undefined]);
  const before = natives(calls).length;
  assert.deepEqual(await popupAsk(listeners, { type: "media-download", tab: 9, url: "https://drm.test/p/manifest.mpd" }), { ok: false, why: "drm" });
  assert.deepEqual(await popupAsk(listeners, { type: "media-download", tab: 9, url: "https://evil.test/x.mp4" }), { ok: false, why: "gone" });
  assert.equal(natives(calls).length, before, "nothing sent for a protected stream or an address the tab didn't load");
  ok("media list: downloads, never a protected one");

  // A move within the page keeps the list; another page clears it; so does closing the tab.
  listeners.tabUpdated(9, { url: "https://watch.test/v/1#t=30" });
  await settle();
  assert.equal((await popupAsk(listeners, { type: "media", tab: 9 })).media.length, 9);
  listeners.tabUpdated(9, { url: "https://watch.test/v/2" });
  await settle();
  assert.equal((await popupAsk(listeners, { type: "media", tab: 9 })).media.length, 0);
  listeners.responseStarted(res("https://files.test/movie.mp4", "media", H("video/mp4")));
  await settle();
  listeners.tabRemoved(9);
  await settle();
  assert.equal((await popupAsk(listeners, { type: "media", tab: 9 })).media.length, 0);
  ok("media list: cleared on another page, kept within one");
}
{
  // Bounded: a page loading file after file keeps at most 60 per tab, its first stream (the one
  // the button sends) included; at most 50 tabs.
  served = {};
  const { api, listeners } = fakeApi({ firefox: true, frames: { 0: { href: "https://live.test/", players: [] } } });
  load(api, true);
  for (let tab = 100; tab < 160; tab++) listeners.responseStarted(res("https://x.test/a.mp4", "media", H("video/mp4"), { tabId: tab }));
  listeners.responseStarted(res("https://live.test/s/master.m3u8", "xmlhttprequest", H("application/vnd.apple.mpegurl")));
  for (let i = 0; i < 500; i++) listeners.responseStarted(res(`https://live.test/clip${i}.mp4`, "media", H("video/mp4")));
  await settle();
  const { media } = await popupAsk(listeners, { type: "media", tab: 9 });
  assert.equal(media.length, 60);
  assert.equal(media[0].url, "https://live.test/s/master.m3u8");
  assert.equal(media[59].url, "https://live.test/clip499.mp4");
  assert.equal((await popupAsk(listeners, { type: "media", tab: 100 })).media.length, 0, "the least recent tabs go");
  assert.equal((await popupAsk(listeners, { type: "media", tab: 159 })).media.length, 1);
  ok("media list: bounded per tab and in tabs");
}
{
  // A stream an embedded player (a frame) loaded: the frame's page goes as its page, the tab's
  // as the referrer, as the button over videos sends it.
  served = { "https://emb.test/e/master.m3u8": { body: "#EXTM3U\n#EXT-X-STREAM-INF:RESOLUTION=1280x720\nhd.m3u8\n" } };
  const frames = { 0: { href: "https://blog.test/post", players: [] }, 2: { href: "https://emb.test/player/1", players: [] } };
  const tabs = { 9: { id: 9, url: "https://blog.test/post", title: "Post" } };
  const { api, calls, listeners } = fakeApi({ firefox: false, frames, tabs });
  load(api, false);
  listeners.responseStarted(res("https://emb.test/e/master.m3u8", "xmlhttprequest", H("application/vnd.apple.mpegurl"), { frameId: 2 }));
  await settle();
  assert.equal((await popupAsk(listeners, { type: "media-download", tab: 9, url: "https://emb.test/e/master.m3u8" })).ok, true);
  const v = natives(calls).find((m) => m.video);
  assert.deepEqual([v.url, v.referrer, v.src], ["https://emb.test/player/1", "https://blog.test/post", "https://emb.test/e/master.m3u8"]);
  ok("media list: a frame's stream goes with the frame's page");
}
{
  // The browser stopped the script and started it again: what the tab loaded is read back.
  const session = {};
  const first = fakeApi({ firefox: false, session });
  load(first.api, false);
  first.listeners.responseStarted(res("https://files.test/kept.mp4", "media", H("video/mp4")));
  await new Promise((r) => setTimeout(r, 300));
  const again = fakeApi({ firefox: false, session, frames: {} });
  load(again.api, false);
  const { media } = await popupAsk(again.listeners, { type: "media", tab: 9 });
  assert.deepEqual(
    media.map((m) => m.url),
    ["https://files.test/kept.mp4"],
  );
  // The popup opened in a tab of its own (popup.html?page=) is still the popup.
  const state = await new Promise((r) =>
    again.listeners.message({ type: "media", tab: 9 }, { id: "ours", url: "chrome-extension://ours/popup.html?page=x", tab: { id: 4 } }, r),
  );
  assert.equal(JSON.parse(JSON.stringify(state)).media.length, 1);
  ok("media list: kept across the script's restarts; the popup in a tab");
}

// #171: a manifest whose address and type don't say so (text/plain, octet-stream, no .m3u8 or
// .mpd) is noted, never read on sight, and read (its first 4 KB) only when a stream is asked
// for: the popup's list for a frame whose player plays from a blob:, the button, the menu.
{
  const PL = "https://cdn.test/p/playlist?id=7";
  const PL_720 = "https://cdn.test/p/playlist?id=7&v=720";
  const DASH = "https://cdn.test/q/manifest?id=7";
  const CONFIG = "https://cdn.test/p/config?id=7";
  const FEED = "https://cdn.test/p/feed";
  const LONG = "https://cdn.test/p/log";
  served = {
    [PL]: { body: "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=1280x720\nplaylist?id=7&v=720\n" },
    [PL_720]: { body: "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg?i=0\n#EXT-X-ENDLIST\n" },
    // With a byte order mark and a comment before the MPD.
    [DASH]: { body: `﻿<?xml version="1.0"?>\n<!-- made by a packager -->\n${MPD(false).replace(/^<\?xml[^>]*\?>\n/, "")}` },
    [CONFIG]: { body: '{"autoplay":true}' },
    [FEED]: { body: '<?xml version="1.0"?><rss><channel><title>MPD news</title></channel></rss>' },
    // Text going on and on: only its start is read.
    [LONG]: { body: "x".repeat(200_000) },
  };
  const blob = { 0: { href: "https://watch.test/v/3", players: [{ src: "blob:https://watch.test/a", w: 0, h: 0, duration: 0, drm: false }] } };
  const tabs = { 9: { id: 9, url: "https://watch.test/v/3", title: "Clip", incognito: false } };
  const { api, calls, listeners } = fakeApi({ firefox: false, frames: blob, tabs });
  load(api, false);
  listeners.tabUpdated(9, { url: "https://watch.test/v/3" });
  await settle();
  fetched.length = 0;
  for (const d of [
    res(PL, "xmlhttprequest", H("text/plain; charset=utf-8")),
    res(PL_720, "xmlhttprequest", H("text/plain")),
    res("https://cdn.test/p/seg?i=0", "xmlhttprequest", H("video/mp2t")),
    res(DASH, "xmlhttprequest", H("application/octet-stream")),
    res(CONFIG, "xmlhttprequest", H("text/plain")),
    res(FEED, "other", H("application/xml")),
    res(LONG, "xmlhttprequest", []),
    // Not possible manifests: too large, a piece, a media element's own file, a page, JSON.
    res("https://cdn.test/p/huge", "xmlhttprequest", H("text/plain", { "Content-Length": String(3 << 20) })),
    res("https://cdn.test/p/part", "xmlhttprequest", H("application/octet-stream", { "Content-Range": "bytes 0-9/99" }), { statusCode: 206 }),
    res("https://cdn.test/p/el", "media", H("text/plain")),
    res("https://cdn.test/p/page", "xmlhttprequest", H("text/html")),
    res("https://cdn.test/p/api", "xmlhttprequest", H("application/json")),
  ]) {
    listeners.responseStarted(d);
  }
  await settle();
  assert.equal(fetched.length, 0, "nothing read on sight");
  const { media } = await popupAsk(listeners, { type: "media", tab: 9 });
  assert.deepEqual(
    media.map((m) => [m.kind, m.url]),
    [
      ["hls", PL],
      ["dash", DASH],
    ],
    "the manifests become streams (the variant inside its master); the rest stays unlisted",
  );
  assert.deepEqual(media[0].heights, [720]);
  assert.deepEqual([media[1].heights, media[1].duration], [[1080, 480], 90]);
  const sniffed = new Set(fetched.map(([u]) => u));
  for (const u of [PL, PL_720, DASH, CONFIG, FEED, LONG]) assert.ok(sniffed.has(u), `read: ${u}`);
  for (const u of ["https://cdn.test/p/huge", "https://cdn.test/p/part", "https://cdn.test/p/el", "https://cdn.test/p/page", "https://cdn.test/p/api"]) {
    assert.ok(!sniffed.has(u), `never read: ${u}`);
  }
  // 4 KB, and the stream's own read-ahead of a chunk or two.
  assert.ok(served[LONG].read <= 6 * 1024, `only the start read (${served[LONG].read} of 200000 bytes)`);
  assert.ok(fetched.every(([, c]) => c === "include"), "read with the site's cookies");
  const reads = fetched.length;
  await popupAsk(listeners, { type: "media", tab: 9 });
  assert.equal(fetched.length, reads, "each read once");
  ok("#171: manifests without .m3u8/.mpd or their type: noted, read only when asked, listed");
}
{
  // The popup reads nothing for a frame whose players have files of their own (no MediaSource).
  served = { "https://cdn.test/p/playlist?id=8": { body: "#EXTM3U\n#EXT-X-ENDLIST\n" } };
  const files = { 0: { href: "https://watch.test/v/4", players: [{ src: "https://files.test/movie.mp4", w: 0, h: 0, duration: 0, drm: false }] } };
  const { api, listeners } = fakeApi({ firefox: false, frames: files, tabs: { 9: { id: 9, url: "https://watch.test/v/4" } } });
  load(api, false);
  listeners.tabUpdated(9, { url: "https://watch.test/v/4" });
  await settle();
  fetched.length = 0;
  listeners.responseStarted(res("https://cdn.test/p/playlist?id=8", "xmlhttprequest", H("text/plain")));
  await settle();
  assert.deepEqual((await popupAsk(listeners, { type: "media", tab: 9 })).media, []);
  assert.equal(fetched.length, 0, "no player fed by a MediaSource: nothing read");
  ok("#171: nothing read where no player plays from a blob:");
}
{
  // The button over a player playing from a blob: and the right-click menu: no stream known, the
  // frame's possible manifests are read and the one that is goes; a frame of its own first.
  const PL = "https://emb.test/e/playlist?id=9";
  served = {
    [PL]: { body: "#EXTM3U\n#EXT-X-STREAM-INF:RESOLUTION=640x360\nplaylist?id=9&v=360\n" },
    "https://emb.test/e/settings": { body: "volume=1" },
    "https://blog.test/ads/list": { body: "#EXTM3U\n#EXTINF:5,\nad.ts\n" },
  };
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  // The page's own frame loaded another playlist (an ad's); the player's frame (2) its stream.
  listeners.responseStarted({ tabId: 7, frameId: 0, url: "https://blog.test/ads/list", type: "xmlhttprequest", statusCode: 200, responseHeaders: H("text/plain") });
  listeners.responseStarted({ tabId: 7, frameId: 2, url: "https://emb.test/e/settings", type: "xmlhttprequest", statusCode: 200, responseHeaders: H("text/plain") });
  listeners.responseStarted({ tabId: 7, frameId: 2, url: PL, type: "xmlhttprequest", statusCode: 200, responseHeaders: H("application/octet-stream") });
  await settle();
  fetched.length = 0;
  await listeners.menu(
    { menuItemId: "mdm-link", srcUrl: "blob:https://emb.test/1", frameId: 2, frameUrl: "https://emb.test/player", pageUrl: "https://blog.test/post" },
    { id: 7, incognito: false },
  );
  await settle();
  const m = natives(calls).find((x) => x.video);
  assert.deepEqual([m.url, m.src], ["https://emb.test/player", PL], "the menu: the frame's sniffed stream");
  assert.ok(!fetched.some(([u]) => u === "https://blog.test/ads/list"), "the frame's own found: the page's frame isn't read");
  assert.ok(calls.some((c) => c[0] === "cookies" && c[1].url === PL), "the stream's cookies");
  // The button in the page's frame (0): none of its own known yet; its frame's read, it goes.
  const sender = { id: "ours", url: "https://blog.test/post", frameId: 0, tab: { id: 7, url: "https://blog.test/post" } };
  await new Promise((r) => listeners.message({ type: "video", src: "" }, sender, r));
  assert.equal(natives(calls).filter((x) => x.video).pop().src, "https://blog.test/ads/list", "the button: its own frame's");
  ok("#171: the button and the menu read the frame's possible manifests when no stream is known");
}

// The button's logo is the extension's own icon, kept in video.js as data.
{
  const video = readFileSync(new URL("../../extension/video.js", import.meta.url), "utf8");
  const icon = readFileSync(new URL("../../extension/icons/icon-48.png", import.meta.url)).toString("base64");
  assert.ok(video.includes(`"data:image/png;base64,${icon}"`), "video.js's LOGO is extension/icons/icon-48.png");
  ok("video button logo");
}

// No other download manager's name in what ships (the clean-room rule, CLAUDE.md).
{
  for (const file of readdirSync(new URL("../../extension/", import.meta.url))) {
    if (!/\.(js|html|css|json)$/.test(file)) continue;
    const text = readFileSync(new URL(`../../extension/${file}`, import.meta.url), "utf8");
    assert.ok(!/\bIDM\b|Internet Download Manager|notes\//.test(text), `${file} names IDM or the private notes`);
  }
  ok("nothing of IDM's in the extension");
}

// The right-click command in the browser's language, and every locale complete: the same keys
// as English, which both manifests' __MSG_…__ names exist in.
{
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  await listeners.installed({ reason: "update" });
  const item = calls.find((c) => c[0] === "menu-item")[1];
  assert.equal(item.title, "Mit Mister Download Manager herunterladen");
  const dir = new URL("../../extension/_locales/", import.meta.url);
  const english = JSON.parse(readFileSync(new URL("en/messages.json", dir), "utf8"));
  for (const locale of readdirSync(dir)) {
    const m = JSON.parse(readFileSync(new URL(`${locale}/messages.json`, dir), "utf8"));
    assert.deepEqual(Object.keys(m).sort(), Object.keys(english).sort(), `${locale} has every text`);
  }
  for (const name of ["manifest.json", "manifest.firefox.json"]) {
    const text = readFileSync(new URL(`../../extension/${name}`, import.meta.url), "utf8");
    for (const [, key] of text.matchAll(/__MSG_(\w+)__/g)) assert.ok(english[key], `${name}: ${key}`);
  }
  // Every key the scripts use exists in English.
  for (const file of ["background.js", "popup.js", "popup.html", "video.js"]) {
    const text = readFileSync(new URL(`../../extension/${file}`, import.meta.url), "utf8");
    for (const [, key] of text.matchAll(/(?:t\(|data-i18n=)"((?:menu|status|popup|video)_\w+)"/g)) assert.ok(english[key], `${file}: ${key}`);
  }
  for (const why of ["not_installed", "forbidden", "not_running", "host_failed", "too_large", "update_app"]) {
    assert.ok(english[`status_${why}`], `status_${why}`);
  }
  ok("translations complete");
}

// #42: the browser's proxy goes with the download, so the app uses the same one.
{
  // Chromium, a VPN extension's fixed SOCKS5 proxy: remote DNS (socks5h), as Chromium does.
  const proxy = { mode: "fixed_servers", rules: { singleProxy: { scheme: "socks5", host: "vpn.example", port: 1080 }, bypassList: ["<local>"] } };
  const { api, calls, listeners } = fakeApi({ firefox: false, proxy });
  load(api, false);
  listeners.determining({ id: 31, url: "https://files.example/p.zip", filename: "p.zip" }, () => {});
  await settle();
  const m = natives(calls)[0];
  assert.equal(m.proxy, "socks5h://vpn.example:1080");
  assert.equal(m.proxyLogin, undefined);
  ok("Chrome: the extension's SOCKS5 proxy goes with the download");
}
{
  // Its bypass list: a local name goes direct, so nothing is sent.
  const proxy = { mode: "fixed_servers", rules: { singleProxy: { scheme: "http", host: "p", port: 8080 }, bypassList: ["*.intranet.example"] } };
  const { api, calls, listeners } = fakeApi({ firefox: false, proxy });
  load(api, false);
  listeners.determining({ id: 32, url: "https://files.intranet.example/p.zip", filename: "p.zip" }, () => {});
  await settle();
  assert.equal(natives(calls)[0].proxy, undefined);
  ok("Chrome: an address on the bypass list goes without the proxy");
}
{
  // A proxy that asked for a login (the VPN extension answers it): flagged for the app.
  const proxy = { mode: "fixed_servers", rules: { proxyForHttps: { scheme: "https", host: "gw.vpn.example", port: 443 } } };
  const { api, calls, listeners } = fakeApi({ firefox: false, proxy });
  load(api, false);
  listeners.authRequired({ isProxy: true, challenger: { host: "gw.vpn.example", port: 443 } });
  listeners.determining({ id: 33, url: "https://x/q.zip", filename: "q.zip" }, () => {});
  await settle();
  const m = natives(calls)[0];
  assert.deepEqual([m.proxy, m.proxyLogin], ["https://gw.vpn.example:443", true]);
  ok("Chrome: a proxy that asks for a login is flagged");
}
{
  // A PAC script set by the extension: its text goes.
  const data = 'function FindProxyForURL(u, h) { return "PROXY p:3128"; }';
  const { api, calls, listeners } = fakeApi({ firefox: false, proxy: { mode: "pac_script", pacScript: { data } } });
  load(api, false);
  listeners.determining({ id: 34, url: "https://x/r.zip", filename: "r.zip" }, () => {});
  await settle();
  assert.equal(natives(calls)[0].proxyPac, data);
  ok("Chrome: a PAC script's text goes with the download");
}
{
  // Firefox: the proxy its own request for the file took (proxy.onRequest extensions too).
  const { api, calls, listeners } = fakeApi({ firefox: true });
  load(api, true);
  listeners.responseStarted({ url: "https://x/s.zip", proxyInfo: { type: "socks", host: "10.8.0.1", port: 1080, proxyDNS: true, username: "u" } });
  listeners.created({ id: 35, url: "https://x/s.zip", filename: "C:\\dl\\s.zip" });
  await settle();
  const m = natives(calls)[0];
  assert.deepEqual([m.proxy, m.proxyLogin], ["socks5h://10.8.0.1:1080", true]);
  ok("Firefox: the request's own proxy goes, its login flagged");
}
{
  // No proxy API (or none in use): nothing extra.
  const { api, calls, listeners } = fakeApi({ firefox: false });
  load(api, false);
  listeners.determining({ id: 36, url: "https://x/t.zip", filename: "t.zip" }, () => {});
  await settle();
  const m = natives(calls)[0];
  assert.deepEqual([m.proxy, m.proxyPac, m.proxyLogin], [undefined, undefined, undefined]);
  ok("no proxy: nothing extra goes");
}

console.log(`extension: ${passed} checks passed`);

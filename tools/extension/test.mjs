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
function fakeApi({ firefox, reply = { ok: true }, types = DEFAULT_TYPES, host = true, finished, panelOn, access = true, rules = {} }) {
  const raw = [];
  // Objects made inside the sandbox have its prototypes: compare plain copies.
  const calls = { push: (c) => raw.push(JSON.parse(JSON.stringify(c))) };
  const listeners = {};
  const event = (name) => ({ addListener: (f) => (listeners[name] = f) });
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
    },
    tabs: {
      onCreated: event("tabCreated"),
      onRemoved: event("tabRemoved"),
      onUpdated: event("tabUpdated"),
      get: async (id) => ({ id, url: "about:blank", status: "complete" }),
      remove: async (id) => calls.push(["closeTab", id]),
      query: async () => [],
      create: async (t) => calls.push(["openTab", t.url]),
    },
    webRequest: {
      onBeforeRequest: event("beforeRequest"),
      onBeforeSendHeaders: event("beforeSendHeaders"),
      onHeadersReceived: event("headersReceived"),
      onResponseStarted: event("responseStarted"),
    },
    scripting: {
      executeScript: async ({ func, args }) => (func ? [{ result: ["https://x/1.zip", "https://x/2.zip"] }] : []),
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
  return { api, calls: raw, listeners, store };
}

function load(api, firefox) {
  const ctx = {
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
{
  const { api, listeners } = fakeApi({ firefox: true, panelOn: false });
  load(api, true);
  const on = await new Promise((r) =>
    listeners.message({ type: "panel" }, { id: "ours", url: "https://x.test/", tab: {} }, (v) => r(JSON.parse(JSON.stringify(v)))),
  );
  assert.deepEqual(on, { on: false }, "where the app says no");
  ok("video button messages");
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

console.log(`extension: ${passed} checks passed`);

// Runs extension/background.js against fake Firefox and Chrome APIs (no browser needed):
//   node tools/extension/test.mjs
import { readFileSync } from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";

const code = readFileSync(new URL("../../extension/background.js", import.meta.url), "utf8");

// A fake extension API; `firefox` picks which download event exists. `reply` is the app's
// answer to the native message; `types` its Options -> File types when asked (none: the host
// doesn't answer, as when it isn't registered).
function fakeApi({ firefox, reply, types, finished, panelOn }) {
  const raw = [];
  // Objects made inside the sandbox have its prototypes: compare plain copies.
  const calls = { push: (c) => raw.push(JSON.parse(JSON.stringify(c))) };
  const listeners = {};
  const event = (name) => ({ addListener: (f) => (listeners[name] = f) });
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
    storage: { local: { get: async (d) => d } },
    cookies: {
      getAll: async (q) => {
        calls.push(["cookies", q]);
        return [{ name: "sid", value: "1" }];
      },
    },
    runtime: {
      sendNativeMessage: async (host, m) => {
        if (m.rules) {
          if (!types) throw new Error("host not registered");
          return { ok: true, types };
        }
        calls.push(["native", host, m]);
        // The video button: "may it show" follows `panelOn`.
        if (m.panel) return { ok: panelOn !== false };
        return reply;
      },
      onInstalled: event("installed"),
      onMessage: event("message"),
      id: "ours",
    },
    contextMenus: { create: (item) => calls.push(["menu-item", item]), onClicked: event("menu") },
    // The browser's language: German here.
    i18n: { getMessage: (key) => ({ menu: "Mit Mister Download Manager herunterladen" })[key] ?? "" },
  };
  return { api, calls: raw, listeners, record: calls };
}

function load(api, firefox) {
  const ctx = { navigator: { userAgent: "UA" }, console, URL };
  if (firefox) ctx.browser = api;
  else ctx.chrome = api;
  vm.runInNewContext(code, ctx);
}

const settle = () => new Promise((r) => setTimeout(r, 20));

// Firefox: a .zip in a container is taken over with that container's cookies.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, reply: { ok: true } });
  load(api, true);
  listeners.created({
    id: 7,
    url: "https://x/a.zip",
    filename: "C:\\dl\\a.zip",
    referrer: "https://page/",
    cookieStoreId: "firefox-container-2",
  });
  await settle();
  const cookies = calls.find((c) => c[0] === "cookies");
  assert.deepEqual(cookies[1], { url: "https://x/a.zip", storeId: "firefox-container-2" });
  const native = calls.find((c) => c[0] === "native");
  assert.equal(native[1], "com.misterdownloadmanager.host");
  assert.deepEqual(native[2], {
    url: "https://x/a.zip",
    referrer: "https://page/",
    cookies: "sid=1",
    userAgent: "UA",
    filename: "a.zip",
  });
  assert.deepEqual(
    calls.filter((c) => ["pause", "cancel", "erase"].includes(c[0])).map((c) => c[0]),
    ["pause", "cancel", "erase"],
  );
}

// Firefox: the app doesn't take it → the browser's download resumes; nothing is lost.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, reply: { ok: false } });
  load(api, true);
  listeners.created({ id: 8, url: "https://x/b.iso", filename: "/home/u/b.iso" });
  await settle();
  assert.ok(calls.some((c) => c[0] === "resume" && c[1] === 8));
  assert.ok(!calls.some((c) => c[0] === "cancel"));
}

// A type not on the list (a web page) stays with the browser.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, reply: { ok: true } });
  load(api, true);
  listeners.created({ id: 9, url: "https://x/page.html", filename: "C:\\dl\\page.html" });
  await settle();
  assert.equal(calls.length, 0);
}

// Chrome: incognito downloads use the incognito store; the final URL wins over the first one.
{
  const { api, calls, listeners } = fakeApi({ firefox: false, reply: { ok: true } });
  load(api, false);
  let suggested = false;
  listeners.determining(
    {
      id: 3,
      url: "https://x/redirect",
      finalUrl: "https://cdn/x.msi",
      filename: "x.msi",
      incognito: true,
    },
    () => (suggested = true),
  );
  await settle();
  assert.ok(!suggested, "a download the app took is never named: Chrome wrote none of it");
  const cookies = calls.find((c) => c[0] === "cookies");
  assert.deepEqual(cookies[1], { url: "https://cdn/x.msi", storeId: "1" });
  assert.deepEqual(
    calls.filter((c) => ["pause", "cancel", "erase"].includes(c[0])).map((c) => c[0]),
    ["cancel", "erase"],
    "held at its name, not paused and resumed",
  );
}

// Chrome: one the app doesn't take goes on as the browser's own (its name confirmed).
{
  const { api, calls, listeners } = fakeApi({ firefox: false, reply: { ok: false } });
  load(api, false);
  let suggested = false;
  listeners.determining({ id: 4, url: "https://x/y.zip", filename: "y.zip" }, () => (suggested = true));
  await settle();
  assert.ok(suggested);
  assert.ok(!calls.some((c) => ["cancel", "erase"].includes(c[0])));
}

// Firefox: a small file finished before it could be stopped: the browser's copy is removed.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, reply: { ok: true }, finished: true });
  load(api, true);
  listeners.created({ id: 5, url: "https://x/s.zip", filename: "C:\dl\s.zip" });
  await settle();
  assert.ok(calls.some((c) => c[0] === "removeFile" && c[1] === 5));
  assert.ok(calls.some((c) => c[0] === "erase" && c[1] === 5));
}

console.log("extension: 6 scenarios passed");

// Options -> File types in the app decides what is offered: its list, not the built-in one.
{
  const { api, calls, listeners } = fakeApi({ firefox: true, reply: { ok: true }, types: "PNG ISO" });
  load(api, true);
  listeners.created({ id: 20, url: "https://x/pic.png", filename: "C:\\dl\\pic.png" });
  listeners.created({ id: 21, url: "https://x/a.zip", filename: "C:\\dl\\a.zip" });
  await settle();
  const offered = calls.filter((c) => c[0] === "native").map((c) => c[2].filename);
  assert.deepEqual(offered, ["pic.png"], "PNG is the app's, ZIP is no longer listed there");
}

console.log("extension: the app's file types decide what is offered");

// Right-click → Download with Mister Download Manager: handed over as an explicit request, so
// the host takes any link (an .html page here), not only Options' file types.
{
  const { api, calls, listeners } = fakeApi({ firefox: false, reply: { ok: true } });
  load(api, false);
  listeners.menu({ menuItemId: "mdm-link", linkUrl: "https://x/page.html", pageUrl: "https://x/" }, { incognito: false });
  await settle();
  const native = calls.find((c) => c[0] === "native");
  assert.equal(native[2].url, "https://x/page.html");
  assert.equal(native[2].explicit, true);
}
console.log("context menu: ok");

// The right-click command in the browser's language, and every locale complete: the same keys
// as English, which both manifests' __MSG_…__ names exist in.
{
  const { api, calls, listeners } = fakeApi({ firefox: false, reply: { ok: true } });
  load(api, false);
  listeners.installed();
  const item = calls.find((c) => c[0] === "menu-item")[1];
  assert.equal(item.title, "Mit Mister Download Manager herunterladen");
  const { readdirSync } = await import("node:fs");
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
}
console.log("translations: ok");

// The download button over videos: the page asks once a minute per site, a click opens the
// app's video window for the page (with the tab as referrer and its cookies), "Never on this
// site" goes to the app and is remembered at once; nothing from another extension is answered.
{
  const { api, calls, listeners } = fakeApi({ firefox: false, reply: { ok: true } });
  load(api, false);
  const ask = (m, sender = { id: "ours", tab: { url: "https://blog.test/post", incognito: false } }) =>
    new Promise((resolve) => {
      // Plain copies: objects from the sandbox have its prototypes.
      const async_ = listeners.message(m, sender, (r) => resolve(JSON.parse(JSON.stringify(r))));
      if (!async_) resolve(undefined);
    });
  assert.deepEqual(await ask({ type: "panel", page: "https://tube.test/embed/1" }), { on: true });
  assert.deepEqual(await ask({ type: "panel", page: "https://tube.test/embed/2" }), { on: true });
  assert.equal(calls.filter((c) => c[0] === "native" && c[2].panel).length, 1, "asked once for the site");
  assert.deepEqual(await ask({ type: "video", page: "https://tube.test/embed/1" }), { ok: true });
  const video = calls.find((c) => c[0] === "native" && c[2].video)[2];
  assert.equal(video.url, "https://tube.test/embed/1");
  assert.equal(video.referrer, "https://blog.test/post");
  assert.equal(video.cookies, "sid=1");
  assert.deepEqual(await ask({ type: "hide-site", host: "tube.test" }), { ok: true });
  assert.ok(calls.some((c) => c[0] === "native" && c[2].hideSite === "tube.test"));
  assert.deepEqual(await ask({ type: "panel", page: "https://tube.test/embed/3" }), { on: false }, "off at once");
  assert.deepEqual(await ask({ type: "panel", page: "https://tube.test:8443/embed/4" }), { on: false }, "whatever the port");
  assert.deepEqual(await ask({ type: "video", page: "javascript:alert(1)" }), { ok: false });
  assert.equal(await ask({ type: "panel", page: "https://x.test/" }, { id: "someone-else" }), undefined);
}
{
  const { api, listeners } = fakeApi({ firefox: true, reply: { ok: true }, panelOn: false });
  load(api, true);
  const on = await new Promise((r) =>
    listeners.message({ type: "panel", page: "https://x.test/" }, { id: "ours" }, (v) => r(JSON.parse(JSON.stringify(v)))),
  );
  assert.deepEqual(on, { on: false }, "where the app says no");
}
console.log("video button messages: ok");

// The button's logo is the extension's own icon, kept in video.js as data.
{
  const video = readFileSync(new URL("../../extension/video.js", import.meta.url), "utf8");
  const icon = readFileSync(new URL("../../extension/icons/icon-48.png", import.meta.url)).toString("base64");
  assert.ok(video.includes(`"data:image/png;base64,${icon}"`), "video.js's LOGO is extension/icons/icon-48.png");
}
console.log("video button logo: ok");

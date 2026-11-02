// Mister Download Manager's browser extension: the background part (Chrome, Edge, Brave, Opera
// and Vivaldi run it as a service worker, Firefox as a background script; one file for both, see
// manifest.json and manifest.firefox.json).
//
// Downloads of the file types Mister Download Manager takes are handed to it: the browser's own
// download is cancelled, and the address, referrer, cookies, User-Agent, file name, size, type
// and (for a form) the posted body go to the app through the native messaging host
// (crates/nmhost). When it can't be reached the download stays with the browser, so nothing is
// lost, and the toolbar button says why, linking the help page's section for it
// (docs/browser-help.md). The browser may stop this script whenever it is idle: what must last
// (the rules, the status, the pause) is kept in storage, not in memory.

const ext = globalThis.browser ?? globalThis.chrome;
const HOST = "com.misterdownloadmanager.host";
const VERSION = ext.runtime.getManifest().version;
const HELP = "https://misterdownloadmanager.com/help/browser-help.html";
const action = ext.action ?? ext.browserAction;
// Firefox has no pause before the file name (onDeterminingFilename): its own paths below.
const FIREFOX = !ext.downloads.onDeterminingFilename;
// Brave and Opera say who they are only in the client hints (Options lists them by name).
const BRANDS = (globalThis.navigator?.userAgentData?.brands || []).map((b) => b.brand);

const t = (key, ...subs) => ext.i18n.getMessage(key, subs) || key;

// ---- the native host ---------------------------------------------------------------------

// How long an answer may take: the host starts Mister Download Manager when it isn't running
// and waits for it (crates/nmhost START_WAIT); the browser's download waits meanwhile.
const HOST_WAIT = 30000;

function withTimeout(promise, ms) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error("no answer")), ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

// Why the browser couldn't reach the host, as the help page's sections name it.
function hostProblem(error) {
  const m = String((error && error.message) || error);
  if (/not found|no such native application/i.test(m)) return "not-installed";
  if (/forbidden/i.test(m)) return "forbidden";
  if (/no answer/i.test(m)) return "not-running";
  return "host-failed";
}

// Every message says which browser and extension version it comes from (the app keeps each
// browser's status, and the rules depend on the browser). Never throws: failures come back as
// `{ok: false, why}`.
async function native(message, ms = HOST_WAIT) {
  const sent = { ...message, userAgent: navigator.userAgent, brands: BRANDS, v: VERSION };
  try {
    const reply = (await withTimeout(ext.runtime.sendNativeMessage(HOST, sent), ms)) || {};
    if (reply.version) await noteStatus(older(reply.version, VERSION) ? "update-app" : null, reply.version);
    else if (reply.ok || reply.why === "options") await noteStatus(null);
    else if (reply.why && reply.why !== "refused") await noteStatus(reply.why);
    return reply;
  } catch (e) {
    const why = hostProblem(e);
    await noteStatus(why);
    return { ok: false, why, error: String((e && e.message) || e) };
  }
}

// a < b, both "x.y.z".
function older(a, b) {
  const pa = String(a).split(/[.-]/).map(Number);
  const pb = String(b).split(/[.-]/).map(Number);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) < (pb[i] || 0);
  }
  return false;
}

// ---- status: the toolbar button's badge and title, the popup's first line ------------------

let status = null; // {why, app, at}
async function readStatus() {
  if (!status) status = (await ext.storage.local.get({ status: { why: null } })).status;
  return status;
}

// `why`: null when connected, else the reason (the help page's section). Written only when it
// changes.
async function noteStatus(why, app) {
  const now = await readStatus();
  if (now.why === why && (!app || now.app === app)) return;
  const was = !now.why || now.why === "update-app";
  status = { why, app: app || now.app || null, at: Date.now() };
  await ext.storage.local.set({ status });
  await showStatus();
  // Unreachable, PDFs and music open in the browser again (and back once connected).
  if (was !== (!why || why === "update-app")) await syncOpenedRule();
}

async function showStatus() {
  const { why } = await readStatus();
  const { paused } = await ext.storage.local.get({ paused: false });
  try {
    await action.setBadgeBackgroundColor({ color: why ? "#C42B1C" : "#6B6B6B" });
    await action.setBadgeText({ text: why && why !== "update-app" ? "!" : paused ? "II" : "" });
    await action.setTitle({ title: why ? t(`status_${why.replace(/-/g, "_")}`) : paused ? t("status_paused") : t("status_connected") });
  } catch {
    // No toolbar button (a browser without one): the popup and the help page still say it.
  }
}

// ---- the rules: Options in the app, kept here between the browser's stops ------------------

// {types, capture, excludedSites, excludedAddresses, opened, at}; null until first asked.
let rules = null;

async function storedRules() {
  if (!rules) rules = (await ext.storage.local.get({ rules: null })).rules;
  return rules;
}

// One question at a time: callers meanwhile get its answer.
let asking = null;
function refreshRules() {
  asking = asking || askRules().finally(() => (asking = null));
  return asking;
}

async function askRules() {
  const reply = await native({ rules: true });
  if (reply.ok && typeof reply.types === "string") {
    rules = {
      types: reply.types,
      capture: reply.capture !== false,
      excludedSites: reply.excludedSites || "",
      excludedAddresses: Array.isArray(reply.excludedAddresses) ? reply.excludedAddresses : [],
      opened: reply.opened !== false,
      minKb: Number(reply.minKb) || 0,
      at: Date.now(),
    };
    await ext.storage.local.set({ rules });
    await syncOpenedRule();
    return rules;
  }
  return null;
}

// The rules to decide by now: the stored ones at once (refreshed meanwhile once a minute old;
// the host checks every offer again), else asked. None while the host can't be reached: then
// the browser keeps everything.
async function currentRules() {
  const r = await storedRules();
  const { why } = await readStatus();
  if (!r || (why && why !== "update-app")) return refreshRules();
  if (Date.now() - r.at > 60000) refreshRules();
  return r;
}

function typePatterns(list) {
  return list
    .split(/\s+/)
    .filter(Boolean)
    .map((t) => new RegExp("^" + t.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$", "i"));
}

function extensionOf(name) {
  const base = name.split(/[\\/]/).pop() || "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1) : "";
}

// As the app matches them (mdm_ipc::capture): `*.example.com` takes the domain and its
// subdomains; an address pattern's `*` any run of characters.
function hostMatches(host, pattern) {
  let p = pattern.trim().toLowerCase().replace(/^[a-z]+:\/\//, "").split("/")[0];
  if (p.startsWith("*.")) {
    p = p.slice(2);
    return host === p || host.endsWith("." + p);
  }
  return p !== "" && host === p;
}

function wildcard(pattern, text) {
  const re = new RegExp("^" + pattern.toLowerCase().split("*").map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
  return re.test(text.toLowerCase());
}

// An address pattern with a scheme is the whole address; without one, the rest after it.
function addressMatches(pattern, url) {
  const p = pattern.trim();
  if (!p) return false;
  return wildcard(p, p.includes("://") ? url : url.replace(/^[a-z][a-z0-9+.-]*:\/\//i, ""));
}

function excluded(r, url) {
  let host = "";
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return true;
  }
  return (
    r.excludedSites.split(/\s+/).some((p) => p && hostMatches(host, p)) ||
    r.excludedAddresses.some((p) => addressMatches(p, url))
  );
}

// ---- what the page told us: forms posted, links clicked with Alt -------------------------

// Forms posted in a page or frame, by address: a download they lead to is posted again by the
// app, with the same body (it can't be sent as a GET). Kept two minutes.
const posts = new Map();
const BODY_LIMIT = 512 * 1024;

function forget(map, ms) {
  const now = Date.now();
  for (const [k, v] of map) if (now - v.at > ms) map.delete(k);
}

// A form's body as the browser sent it: its fields (formData: put together again in the
// form's own encoding once its Content-Type is known, `postOf`), or the raw bytes when they
// are text. Anything else (a file upload) can't be sent again: `body: null` leaves its download
// to the browser.
function formBody(details) {
  const rb = details.requestBody;
  if (!rb) return { body: null };
  if (rb.formData) {
    const fields = Object.entries(rb.formData).flatMap(([k, values]) => values.map((v) => [k, String(v)]));
    const size = fields.reduce((n, [k, v]) => n + k.length + v.length + 2, 0);
    return size > BODY_LIMIT ? { body: null } : { fields, type: "application/x-www-form-urlencoded" };
  }
  if (Array.isArray(rb.raw) && rb.raw.every((p) => p.bytes)) {
    const size = rb.raw.reduce((n, p) => n + p.bytes.byteLength, 0);
    if (size > BODY_LIMIT) return { body: null };
    const bytes = new Uint8Array(size);
    let at = 0;
    for (const p of rb.raw) {
      bytes.set(new Uint8Array(p.bytes), at);
      at += p.bytes.byteLength;
    }
    try {
      return { body: new TextDecoder("utf-8", { fatal: true }).decode(bytes), type: null };
    } catch {
      return { body: null };
    }
  }
  return { body: null };
}

if (ext.webRequest) {
  const navigations = { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] };
  ext.webRequest.onBeforeRequest.addListener(
    (d) => {
      if (d.method !== "POST") return;
      forget(posts, 120000);
      posts.set(d.url, { ...formBody(d), at: Date.now(), id: d.requestId });
    },
    navigations,
    ["requestBody"],
  );
  // The Content-Type the form went with (a multipart boundary must stay the same).
  ext.webRequest.onBeforeSendHeaders.addListener(
    (d) => {
      if (d.method !== "POST") return;
      const p = posts.get(d.url);
      const ct = (d.requestHeaders || []).find((h) => h.name.toLowerCase() === "content-type");
      if (p && p.id === d.requestId && ct && ct.value) p.type = ct.value;
    },
    navigations,
    ["requestHeaders"],
  );
}

// The form posted to `url` (within two minutes), as the app sends it again: urlencoded fields,
// or a raw text body with its own type. null: it can't be sent again (a file in it; a multipart
// form, whose fields the browser gives without its files): the browser keeps the download.
function postOf(url) {
  const p = posts.get(url);
  if (!p || Date.now() - p.at > 120000) return undefined;
  if (p.body === null || (p.fields && /multipart/i.test(p.type || ""))) return null;
  if (!p.fields) return { body: p.body, type: p.type };
  const params = new URLSearchParams();
  for (const [k, v] of p.fields) params.append(k, v);
  return { body: params.toString(), type: "application/x-www-form-urlencoded" };
}

// Where page loads' redirects started, by where they ended (two minutes): Firefox names a
// download by its last address only, and the app keeps the first (an expired signed link is
// renewed from it).
const redirected = new Map();
if (ext.webRequest && ext.webRequest.onBeforeRedirect) {
  ext.webRequest.onBeforeRedirect.addListener(
    (d) => {
      forget(redirected, 120000);
      const first = (redirected.get(d.url) || {}).from || d.url;
      redirected.set(d.redirectUrl, { from: first, at: Date.now() });
    },
    { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] },
  );
}

// The address a download was first asked for: the browser's own (Chromium), else the start of
// the redirects that led to it.
function firstUrl(item) {
  if (item.finalUrl && item.finalUrl !== item.url) return item.url;
  const r = redirected.get(item.url);
  return r && Date.now() - r.at < 120000 ? r.from : item.url;
}

// Links clicked with Alt held (page.js): the browser keeps their download.
const bypass = new Map();

// Streams the page's players loaded (HLS, DASH), per tab, in the order seen (a master playlist
// comes before its variants): a player that plays from a blob: has no address of its own, so
// the download button sends its manifest, which yt-dlp reads (qualities, separate audio).
// Segments are never listed. Forgotten when the tab goes to another page or closes.
const streams = new Map();
const MANIFEST = /\.(m3u8|mpd)(?:[?#]|$)/i;
const MANIFEST_TYPE = /mpegurl|dash\+xml/i;

if (ext.webRequest && ext.webRequest.onResponseStarted) {
  ext.webRequest.onResponseStarted.addListener(
    (d) => {
      if (d.tabId < 0) return;
      const ct = (d.responseHeaders || []).find((h) => h.name.toLowerCase() === "content-type");
      if (!MANIFEST.test(d.url) && !(ct && MANIFEST_TYPE.test(ct.value || ""))) return;
      streamsBack.then(() => {
        const list = streams.get(d.tabId) || [];
        if (!list.some((s) => s.url === d.url)) list.push({ url: d.url, frameId: d.frameId });
        streams.set(d.tabId, list.slice(-20));
        keepStreams();
      });
    },
    { urls: ["<all_urls>"], types: ["xmlhttprequest", "media", "other"] },
    ["responseHeaders"],
  );
}
ext.tabs.onRemoved.addListener((id) => streamsBack.then(() => streams.delete(id) && keepStreams()));
ext.tabs.onUpdated.addListener((id, change) => {
  if (change.url) streamsBack.then(() => streams.delete(id) && keepStreams());
});

// Kept in the session's storage (memory only, gone with the browser): the browser stops this
// script when idle, and a tab's player loaded its stream long before the button is clicked.
// Read back once at start, merged with what was seen meanwhile; everything else waits for it.
const session = ext.storage.session;
function keepStreams() {
  if (session) session.set({ streams: [...streams] }).catch(() => {});
}
const streamsBack = session
  ? session
      .get({ streams: [] })
      .then((v) => {
        for (const [tab, list] of v.streams || []) {
          const now = streams.get(tab) || [];
          streams.set(tab, [...list, ...now.filter((s) => !list.some((o) => o.url === s.url))].slice(-20));
        }
      })
      .catch(() => {})
  : Promise.resolve();

// The stream a frame's player loaded first (its master playlist), else the tab's.
function streamOf(sender) {
  const list = (sender.tab && streams.get(sender.tab.id)) || [];
  const mine = list.find((s) => s.frameId === sender.frameId);
  return (mine || list[0] || {}).url || "";
}

// ---- the hand-off ------------------------------------------------------------------------

// Cookies from the store the download belongs to, never another: Firefox names it on the
// download (private windows and containers each have their own); Chrome's incognito store is "1".
async function cookieHeader(url, { incognito, storeId }) {
  try {
    const query = storeId ? { url, storeId } : incognito ? { url, storeId: "1" } : { url };
    const list = await ext.cookies.getAll(query);
    return list.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

// Has the user left this extension the site? With access limited ("On click", some sites), its
// cookies can't be read, and a download handed over without them would fail.
async function siteAccess(url) {
  try {
    const u = new URL(url);
    // By host name: Firefox's match patterns take no port.
    return await ext.permissions.contains({ origins: [`${u.protocol}//${u.hostname}/*`] });
  } catch {
    return false;
  }
}

// `explicit`: the user asked for this one (the right-click item), so Options' automatic-capture
// rules (file types, excluded sites) don't apply; the host checks them for everything else.
async function handOff(d) {
  const message = {
    url: d.url,
    finalUrl: d.finalUrl && d.finalUrl !== d.url ? d.finalUrl : "",
    referrer: d.referrer || "",
    cookies: await cookieHeader(d.url, d),
    filename: d.filename || "",
    ...(d.size > 0 ? { size: d.size } : {}),
    ...(d.mime ? { mime: d.mime } : {}),
    ...(d.post ? { post: d.post.body, postType: d.post.type || "" } : {}),
    ...(d.incognito ? { private: true } : {}),
    ...(d.explicit ? { explicit: true } : {}),
  };
  return native(message);
}

// The file name to offer the app the download under, or null: the browser keeps it.
async function candidate(item, url) {
  if (!/^https?:/i.test(url)) return null; // blob:, data:, file: stay the browser's
  const { paused } = await ext.storage.local.get({ paused: false });
  if (paused) return null;
  forget(bypass, 10000);
  if (bypass.has(item.url) || bypass.has(url)) return null;
  const first = firstUrl(item);
  if (bypass.has(first) || postOf(first) === null) return null; // Alt; a form not to post again
  const r = await currentRules();
  if (!r || !r.capture || excluded(r, first) || excluded(r, url)) return null;
  const name = (item.filename || "").split(/[\\/]/).pop() || url.split(/[?#]/)[0].split("/").pop() || "";
  const type = extensionOf(name);
  if (!type || !typePatterns(r.types).some((p) => p.test(type))) return null;
  // Smaller than Options' minimum (when the browser knows the size already).
  const size = item.totalBytes > 0 ? item.totalBytes : item.fileSize;
  if (r.minKb > 0 && size > 0 && size < r.minKb * 1024) return null;
  if (!(await siteAccess(first))) return null;
  return name;
}

// Offer the download to the app under `name`; true if it took it.
async function offer(item, url, name) {
  const first = firstUrl(item);
  const reply = await handOff({
    url: first,
    finalUrl: url,
    referrer: item.referrer,
    filename: name,
    size: item.totalBytes > 0 ? item.totalBytes : item.fileSize,
    mime: item.mime,
    post: postOf(first),
    incognito: item.incognito,
    storeId: item.cookieStoreId,
  });
  return Boolean(reply.ok);
}

// Tabs opened in the last seconds: one the download was opened in (a link opened in a new tab
// or window, Shift+click) is closed with it, as when the browser keeps the download.
const newTabs = new Map();
ext.tabs.onCreated.addListener((tab) => {
  forget(newTabs, 10000);
  newTabs.set(tab.id, { at: Date.now() });
});

async function closeLeftTab(urls) {
  forget(newTabs, 10000);
  for (const id of newTabs.keys()) {
    try {
      const tab = await ext.tabs.get(id);
      const at = tab.pendingUrl || tab.url || "";
      // Only a tab opened for the download's own address (never a new tab the user opened).
      if (urls.includes(at)) {
        newTabs.delete(id);
        await ext.tabs.remove(id);
      }
    } catch {
      newTabs.delete(id);
    }
  }
}

// The browser's download, gone without a trace once the app has it.
async function drop(item) {
  const id = item.id;
  try {
    await ext.downloads.cancel(id);
  } catch {
    // Already finished: its file goes.
  }
  try {
    const [d] = await ext.downloads.search({ id });
    if (d && d.state === "complete" && d.exists !== false) await ext.downloads.removeFile(id);
  } catch {
    // Gone already.
  }
  ext.downloads.erase({ id }).catch(() => {});
  closeLeftTab([firstUrl(item), item.url, item.finalUrl].filter(Boolean)).catch(() => {});
}

if (!FIREFOX) {
  // Chromium: once the final file name is known (after redirects and Content-Disposition). The
  // download waits for `suggest`, writing nothing, while the app is asked: taken, it is
  // cancelled; else it goes on as the browser's. So it is never lost or kept by both (the app
  // asks the address once more: a form's download is posted again with its body).
  ext.downloads.onDeterminingFilename.addListener((item, suggest) => {
    const url = item.finalUrl || item.url;
    candidate(item, url)
      .then((name) => name && offer(item, url, name))
      .then((taken) => (taken ? drop(item) : suggest()))
      .catch(() => suggest());
    return true; // suggest() comes later
  });
} else {
  // Firefox names the file before the download is created and has no such pause: the download
  // is paused at once when it may be the app's (the stored rules say so without asking), then
  // cancelled or resumed; one that finished meanwhile (a small file) has its file removed.
  ext.downloads.onCreated.addListener(async (item) => {
    const url = item.finalUrl || item.url;
    const name = await candidate(item, url).catch(() => null);
    if (!name) return;
    let paused = false;
    try {
      await ext.downloads.pause(item.id);
      paused = true;
    } catch {
      // Not pausable (no resume support), or finished already.
    }
    const taken = await offer(item, url, name).catch(() => false);
    if (taken) drop(item);
    else if (paused) ext.downloads.resume(item.id).catch(() => {});
  });
}

// ---- files the browser would open itself (a PDF, an MP3): made downloads -----------------

// Options → General in the app: also take the listed types the browser shows or plays when a
// link leads to one. Chromium: a session rule marks such answers as attachments, so they come
// down the download path above (where the rules and Alt apply). Firefox: the same by changing
// the answer's headers.
const OPENED = {
  PDF: ["application/pdf"],
  MP3: ["audio/mpeg", "audio/mp3"],
  MP4: ["video/mp4"],
  M4A: ["audio/mp4", "audio/x-m4a"],
  M4V: ["video/x-m4v"],
  OGG: ["audio/ogg"],
  OGV: ["video/ogg"],
  WAV: ["audio/wav", "audio/x-wav", "audio/wave"],
  AAC: ["audio/aac"],
  MOV: ["video/quicktime"],
  TIF: ["image/tiff"],
  TIFF: ["image/tiff"],
  WMA: ["audio/x-ms-wma"],
  WMV: ["video/x-ms-wmv"],
  AVI: ["video/x-msvideo", "video/avi"],
  MKV: ["video/x-matroska"],
};
const OPENED_RULE = 1;
// The allow rules for exact excluded hosts follow it (ids from 2).
const OPENED_ALLOW = 2;
let openedTypes = [];

// Options' excluded sites as the rules need them, read as the app reads them: `*.example.com`
// is the domain and its subdomains (a rule's domain condition), a plain name that host only
// (an allow rule of its own). As the browser writes a host: no port, punycode.
function excludedHosts() {
  const wide = [];
  const exact = [];
  for (const raw of ((rules && rules.excludedSites) || "").split(/\s+/)) {
    const p = raw.trim().replace(/^[a-z]+:\/\//i, "").split("/")[0];
    const sub = p.startsWith("*.");
    let host = "";
    try {
      host = new URL(`http://${sub ? p.slice(2) : p}`).hostname;
    } catch {
      continue;
    }
    if (!host || host.includes("*")) continue;
    (sub ? wide : exact).push(host);
  }
  return { wide, exact };
}

function openedMimes(r) {
  if (!r || !r.capture || !r.opened) return [];
  const listed = new Set(r.types.split(/\s+/).map((s) => s.toUpperCase()));
  return [...new Set(Object.entries(OPENED).flatMap(([k, m]) => (listed.has(k) ? m : [])))];
}

async function syncOpenedRule() {
  const { paused } = await ext.storage.local.get({ paused: false });
  const { why } = await readStatus();
  openedTypes = paused || (why && why !== "update-app") ? [] : openedMimes(rules);
  const dnr = ext.declarativeNetRequest;
  if (FIREFOX || !dnr || !dnr.updateSessionRules) return;
  const hosts = excludedHosts();
  const rule = {
    id: OPENED_RULE,
    priority: 1,
    action: {
      type: "modifyHeaders",
      responseHeaders: [{ header: "content-disposition", operation: "set", value: "attachment" }],
    },
    condition: {
      resourceTypes: ["main_frame"],
      responseHeaders: [{ header: "content-type", values: openedTypes.map((m) => `${m}*`) }],
      // Options' excluded sites keep opening in the browser.
      ...(hosts.wide.length ? { excludedRequestDomains: hosts.wide } : {}),
    },
  };
  const esc = (h) => h.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const allows = hosts.exact.slice(0, 500).map((h, i) => ({
    id: OPENED_ALLOW + i,
    priority: 2,
    action: { type: "allow" },
    condition: { resourceTypes: ["main_frame"], regexFilter: `^[a-z]+://${esc(h)}(:[0-9]+)?/` },
  }));
  try {
    const old = (await dnr.getSessionRules()).map((r) => r.id);
    await dnr.updateSessionRules({
      removeRuleIds: old.filter((id) => id >= OPENED_RULE),
      addRules: openedTypes.length ? [rule, ...allows] : [],
    });
  } catch {
    // A browser without header conditions (Chrome before 128): it opens them as before.
  }
}

if (FIREFOX && ext.webRequest && ext.webRequest.onHeadersReceived) {
  ext.webRequest.onHeadersReceived.addListener(
    (d) => {
      const first = (redirected.get(d.url) || {}).from || d.url;
      if (!openedTypes.length || bypass.has(d.url) || bypass.has(first)) return {};
      if (rules && (excluded(rules, d.url) || excluded(rules, first))) return {};
      const headers = d.responseHeaders || [];
      const get = (n) => headers.find((h) => h.name.toLowerCase() === n);
      const type = ((get("content-type") || {}).value || "").toLowerCase();
      const cd = get("content-disposition");
      if (!openedTypes.some((m) => type.startsWith(m)) || (cd && /^\s*attachment/i.test(cd.value))) return {};
      // Under Options' minimum it would only go back to the browser as a download: it opens.
      const size = Number((get("content-length") || {}).value) || 0;
      if (rules && rules.minKb > 0 && size > 0 && size < rules.minKb * 1024) return {};
      return {
        responseHeaders: [
          ...headers.filter((h) => h.name.toLowerCase() !== "content-disposition"),
          { name: "Content-Disposition", value: "attachment" },
        ],
      };
    },
    { urls: ["<all_urls>"], types: ["main_frame"] },
    ["blocking", "responseHeaders"],
  );
}

// ---- right-click menus -------------------------------------------------------------------

// Created at install and update (removed first: they persist, and a second create fails), in
// the browser's language (_locales, from the app's catalogs by tools/extension/locales.py).
function createMenus() {
  ext.contextMenus.removeAll(() => {
    const add = (id, contexts, key) =>
      ext.contextMenus.create({ id, title: t(key), contexts }, () => void ext.runtime.lastError);
    add("mdm-link", ["link", "video", "audio", "image"], "menu");
    add("mdm-selected", ["selection"], "menu_selected");
    add("mdm-all", ["page"], "menu_all");
  });
}

// The links in the page (all of them, or those in the selection), collected in the page.
function pageLinks(selectedOnly) {
  const ok = (h) => /^(https?|ftps?|ftpes):/i.test(h);
  let anchors = [...document.querySelectorAll("a[href]")];
  if (selectedOnly) {
    const sel = getSelection();
    anchors = sel.rangeCount ? anchors.filter((a) => sel.containsNode(a, true)) : [];
  }
  const seen = new Set();
  const links = [];
  for (const a of anchors) {
    if (ok(a.href) && !seen.has(a.href)) {
      seen.add(a.href);
      links.push(a.href);
    }
  }
  for (const m of document.querySelectorAll("video[src], audio[src], source[src]")) {
    if (ok(m.src) && !seen.has(m.src) && !selectedOnly) {
      seen.add(m.src);
      links.push(m.src);
    }
  }
  return links;
}

// When the user asked and it couldn't be done: the help page's section opens beside (a link
// MDM refused has none of its own: the connection's).
const SECTIONS = ["not-installed", "forbidden", "not-running", "host-failed", "too-large", "update-app", "options"];
function explain(why) {
  const at = SECTIONS.includes(why) ? why : "host-failed";
  ext.tabs.create({ url: `${HELP}#${at}` }).catch(() => {});
}

ext.contextMenus.onClicked.addListener(async (info, tab) => {
  const incognito = Boolean(tab && tab.incognito);
  const storeId = tab && tab.cookieStoreId;
  if (info.menuItemId === "mdm-link") {
    const url = info.linkUrl || info.srcUrl;
    // A player's own stream (blob:, a MediaSource) or a stream's manifest: the Download video
    // window, which reads streams (a manifest downloaded as a file is only a short list).
    if (url && MANIFEST.test(url)) {
      const reply = await native({ video: true, url: info.pageUrl, src: url, cookies: await cookieHeader(info.pageUrl, { incognito, storeId }), private: incognito });
      if (!reply.ok && reply.why !== "options") explain(reply.why);
      return;
    }
    if (url && /^(blob|mediastream):/i.test(url)) {
      const page = info.frameUrl || info.pageUrl;
      const reply = await native({ video: true, url: page, referrer: info.pageUrl, cookies: await cookieHeader(page, { incognito, storeId }), private: incognito });
      if (!reply.ok && reply.why !== "options") explain(reply.why);
      return;
    }
    // ftp links too: browsers no longer download them, MDM does.
    if (!url || !/^(https?|ftps?|ftpes):/i.test(url)) return;
    const reply = await handOff({ url, referrer: info.pageUrl, incognito, storeId, explicit: true });
    if (!reply.ok) explain(reply.why);
    return;
  }
  if ((info.menuItemId === "mdm-selected" || info.menuItemId === "mdm-all") && tab) {
    let links = [];
    try {
      const [res] = await ext.scripting.executeScript({
        target: { tabId: tab.id, frameIds: [info.frameId || 0] },
        func: pageLinks,
        args: [info.menuItemId === "mdm-selected"],
      });
      links = (res && res.result) || [];
    } catch {
      // A page extensions may not read (the browser's own, a store).
    }
    if (!links.length) return;
    const reply = await native({ links, referrer: info.pageUrl, cookies: await cookieHeader(info.pageUrl, { incognito, storeId }), private: incognito });
    if (!reply.ok) explain(reply.why);
  }
});

// ---- install, start, the toolbar popup ------------------------------------------------------

ext.runtime.onInstalled.addListener(async (details) => {
  createMenus();
  // Hello: the app learns the extension is there (its offer window says Connected).
  const r = await refreshRules();
  if (details.reason === "install" && !r) {
    const { why } = await readStatus();
    explain(why || "not-installed");
  }
  injectOpenTabs();
});

ext.runtime.onStartup.addListener(() => {
  refreshRules();
  showStatus();
});

// Pages open before the extension was installed or updated get its scripts now (else the
// button waits for a reload).
async function injectOpenTabs() {
  if (!ext.scripting) return;
  const tabs = await ext.tabs.query({ url: ["http://*/*", "https://*/*"] }).catch(() => []);
  // A match pattern's host and path as a test (`*://*.youtube.com/*`).
  const matcher = (pattern) => {
    const [, host = "", path = "/*"] = /^[^:]+:\/\/([^/]*)(\/.*)?$/.exec(pattern) || [];
    const glob = (s) => s.split("*").map((x) => x.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*");
    const h = host.startsWith("*.") ? new RegExp(`^(.*\\.)?${glob(host.slice(2))}$`, "i") : new RegExp(`^${glob(host)}$`, "i");
    const pth = new RegExp(`^${glob(path)}$`);
    return (u) => h.test(u.hostname) && pth.test(u.pathname + u.search);
  };
  for (const entry of ext.runtime.getManifest().content_scripts || []) {
    const excludes = (entry.exclude_matches || []).map(matcher);
    for (const tab of tabs) {
      let u;
      try {
        u = new URL(tab.url);
      } catch {
        continue;
      }
      if (excludes.some((m) => m(u))) continue;
      ext.scripting
        .executeScript({ target: { tabId: tab.id, allFrames: Boolean(entry.all_frames) }, files: entry.js })
        .catch(() => {});
    }
  }
}

// ---- messages from the page (video.js, page.js) and the popup (popup.js) -------------------

// The download button over videos asks: may it show on this page, the button was clicked, its
// menu. The app decides through the native host; its answer for a site is kept a minute (one
// question per page load at most). A failure to reach it isn't kept.
const panelCache = new Map();
async function panelOn(page) {
  // The host name without a port: as video.js names the site, and as the app matches sites.
  const site = new URL(page).hostname;
  forget(panelCache, 60000);
  const kept = panelCache.get(site);
  if (kept) return kept.on;
  const reply = await native({ panel: true, url: page });
  if (reply.ok || !reply.why) panelCache.set(site, { on: Boolean(reply.ok), at: Date.now() });
  return Boolean(reply.ok);
}

// The page the message came from, as the browser knows it (never what the page says).
function senderPage(sender) {
  const u = sender.url || (sender.tab && sender.tab.url) || "";
  return /^https?:/i.test(u) ? u : null;
}

async function pageMessage(m, sender) {
  const page = senderPage(sender);
  const tab = sender.tab || {};
  if (m.type === "panel") return { on: page ? await panelOn(page) : false };
  if (m.type === "bypass" && typeof m.url === "string") {
    bypass.set(m.url, { at: Date.now() });
    return { ok: true };
  }
  if (m.type === "video" && page) {
    await streamsBack;
    const src = typeof m.src === "string" && /^https?:/i.test(m.src) ? m.src : streamOf(sender);
    const reply = await native({
      video: true,
      url: page,
      ...(src ? { src } : {}),
      title: typeof m.title === "string" ? m.title.slice(0, 300) : "",
      referrer: tab.url && tab.url !== page ? tab.url : "",
      cookies: await cookieHeader(page, { incognito: tab.incognito, storeId: tab.cookieStoreId }),
      ...(tab.incognito ? { private: true } : {}),
    });
    return { ok: Boolean(reply.ok), why: reply.why };
  }
  if (m.type === "hide-site" && page) {
    const host = new URL(page).hostname;
    panelCache.set(host, { on: false, at: Date.now() });
    const reply = await native({ hideSite: host });
    return { ok: Boolean(reply.ok) };
  }
  if (m.type === "options") return { ok: Boolean((await native({ openOptions: true })).ok) };
  return { ok: false };
}

async function popupMessage(m) {
  if (m.type === "state") {
    const r = await refreshRules();
    const { paused } = await ext.storage.local.get({ paused: false });
    const s = await readStatus();
    return { connected: Boolean(r) && (!s.why || s.why === "update-app"), why: s.why, app: s.app, version: VERSION, paused };
  }
  if (m.type === "pause") {
    await ext.storage.local.set({ paused: Boolean(m.paused) });
    await syncOpenedRule();
    await showStatus();
    return { ok: true };
  }
  if (m.type === "exclude-site" && typeof m.host === "string" && m.host) {
    const reply = await native({ excludeSite: m.host });
    await refreshRules();
    return { ok: Boolean(reply.ok) };
  }
  if (m.type === "open-app") return { ok: Boolean((await native({ open: true })).ok) };
  if (m.type === "open-options") return { ok: Boolean((await native({ openOptions: true, page: "general" })).ok) };
  return { ok: false };
}

ext.runtime.onMessage.addListener((m, sender, sendResponse) => {
  // Only our own scripts (pages can't message an extension without being allowed to).
  if (!m || typeof m.type !== "string" || sender.id !== ext.runtime.id) return false;
  const fromPage = Boolean(sender.tab);
  (fromPage ? pageMessage(m, sender) : popupMessage(m)).then(sendResponse, () => sendResponse({ ok: false }));
  return true; // answered later
});

showStatus();
// Rules kept from before, else asked now (a first start, storage cleared).
storedRules().then((r) => (r ? syncOpenedRule() : refreshRules()));

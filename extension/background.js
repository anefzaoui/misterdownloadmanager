// Mister Download Manager Integration Module (Chrome/Edge/Brave service worker; Firefox
// background script — one file for both, see manifest.json and manifest.firefox.json).
//
// Like IDM's browser extension: downloads of the file types Mister Download Manager handles are
// taken over — the browser's own download is cancelled and the URL, referrer, cookies, User-Agent
// and file name go to Mister Download Manager through the native messaging host (crates/nmhost).
// If Mister Download Manager cannot be reached, the download goes back to the browser so nothing
// is lost.

const ext = globalThis.browser ?? globalThis.chrome;
const HOST = "com.misterdownloadmanager.host";

// IDM 6.42's default "Automatically start downloading the following file types"
// (notes/ui-spec/options-dialog.md). `*` matches any characters.
const DEFAULT_TYPES =
  "3GP 7Z AAC ACE AIF APK ARJ ASF AVI BIN BZ2 EXE GZ GZIP IMG ISO LZH M4A M4V MKV MOV MP3 MP4 " +
  "MPA MPE MPEG MPG MSI MSU OGG OGV PDF PLJ PPS PPT QT R0* R1* RA RAR RM RMVB SEA SIT SITX TAR " +
  "TIF TIFF WAV WMA WMV Z ZIP";

function typePatterns(list) {
  return list
    .split(/\s+/)
    .filter(Boolean)
    .map((t) => new RegExp("^" + t.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$", "i"));
}

// Options -> File types in Mister Download Manager decides which downloads are offered: asked of
// the native host (it reads the saved Options), kept a minute, the built-in list if the host
// can't be reached. The host checks the rest (excluded sites, browsers) when one is offered.
let rulesCache = { types: null, at: 0 };
async function appTypes() {
  if (rulesCache.types && Date.now() - rulesCache.at < 60000) return rulesCache.types;
  try {
    const reply = await ext.runtime.sendNativeMessage(HOST, { rules: true });
    if (reply && reply.ok && typeof reply.types === "string" && reply.types.trim()) {
      rulesCache = { types: reply.types, at: Date.now() };
      return reply.types;
    }
  } catch {
    // Host not registered: the stored or built-in list below.
  }
  return null;
}

async function settings() {
  const s = await ext.storage.local.get({ enabled: true, types: DEFAULT_TYPES });
  const types = (await appTypes()) || s.types;
  return { enabled: s.enabled, patterns: typePatterns(types) };
}

function extensionOf(name) {
  const base = name.split(/[\\/]/).pop() || "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1) : "";
}

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

// `explicit`: the user asked for this one (the right-click item), so Options' automatic-capture
// rules (file types, excluded sites) don't apply; the host checks them for everything else.
async function handOff({ url, referrer, filename, incognito, storeId, explicit }) {
  const message = {
    url,
    referrer: referrer || "",
    cookies: await cookieHeader(url, { incognito, storeId }),
    userAgent: navigator.userAgent,
    filename: filename || "",
    ...(explicit ? { explicit: true } : {}),
  };
  try {
    const reply = await ext.runtime.sendNativeMessage(HOST, message);
    return Boolean(reply && reply.ok);
  } catch {
    return false; // host not registered / Mister Download Manager not installed
  }
}

// The file name to offer the app the download under, or null: only the file types Options lists
// (the host checks the rest).
async function candidate(item, url) {
  if (!/^https?:/i.test(url)) return null;
  const { enabled, patterns } = await settings();
  if (!enabled) return null;
  const name = (item.filename || "").split(/[\\/]/).pop() || url.split(/[?#]/)[0].split("/").pop() || "";
  const ext_ = extensionOf(name);
  return ext_ && patterns.some((p) => p.test(ext_)) ? name : null;
}

// Offer the download to the app under `name`; true if it took it.
function offer(item, url, name) {
  return handOff({
    url,
    referrer: item.referrer,
    filename: name,
    incognito: item.incognito,
    storeId: item.cookieStoreId,
  });
}

// The browser's download, gone without a trace once the app has it.
async function drop(id) {
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
}

if (ext.downloads.onDeterminingFilename) {
  // Chromium: once the final file name is known (after redirects and Content-Disposition),
  // exactly when IDM's integration decides. The download waits for `suggest`, writing nothing,
  // while the app is asked: taken, it is cancelled; else it goes on as the browser's. So it is
  // never lost, requested twice (one-time links, POST downloads) or kept by both.
  ext.downloads.onDeterminingFilename.addListener((item, suggest) => {
    const url = item.finalUrl || item.url;
    candidate(item, url)
      .then((name) => name && offer(item, url, name))
      .then((taken) => (taken ? drop(item.id) : suggest()))
      .catch(() => suggest());
    return true; // suggest() comes later
  });
} else {
  // Firefox names the file before the download is created and has no such pause: the download
  // is paused while the app is asked, then cancelled or resumed; one that finished meanwhile
  // (a small file) has its file removed.
  ext.downloads.onCreated.addListener(async (item) => {
    const name = await candidate(item, item.url).catch(() => null);
    if (!name) return;
    let paused = false;
    try {
      await ext.downloads.pause(item.id);
      paused = true;
    } catch {
      // Not pausable (no resume support), or finished already.
    }
    const taken = await offer(item, item.url, name).catch(() => false);
    if (taken) drop(item.id);
    else if (paused) ext.downloads.resume(item.id).catch(() => {});
  });
}

// Right-click → "Download with Mister Download Manager" (IDM: "Download with IDM"), in the
// browser's language (_locales, from the app's catalogs by tools/extension/locales.py).
ext.runtime.onInstalled.addListener(() => {
  ext.contextMenus.create({
    id: "mdm-link",
    title: ext.i18n.getMessage("menu") || "Download with Mister Download Manager",
    contexts: ["link", "video", "audio", "image"],
  });
});

ext.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId !== "mdm-link") return;
  const url = info.linkUrl || info.srcUrl;
  // ftp links too: browsers no longer download them, MDM does.
  if (url && /^(https?|ftps?|ftpes):/i.test(url)) {
    handOff({
      url,
      referrer: info.pageUrl,
      filename: "",
      incognito: Boolean(tab && tab.incognito),
      storeId: tab && tab.cookieStoreId,
      explicit: true,
    });
  }
});

// The download button over videos (video.js) asks here: may it show on this page, the button
// was clicked, its menu. The app decides through the native host; its answer for a site is kept
// a minute (one question per page load at most).
const panelCache = new Map();
async function panelOn(page) {
  // The host name without a port: as video.js names the site, and as the app matches sites.
  const site = new URL(page).hostname;
  const kept = panelCache.get(site);
  if (kept && Date.now() - kept.at < 60000) return kept.on;
  let on = false;
  try {
    const reply = await ext.runtime.sendNativeMessage(HOST, { panel: true, url: page, userAgent: navigator.userAgent });
    on = Boolean(reply && reply.ok);
  } catch {
    // Host not registered: no button.
  }
  panelCache.set(site, { on, at: Date.now() });
  return on;
}

async function panelMessage(m, sender) {
  const page = typeof m.page === "string" && /^https?:/i.test(m.page) ? m.page : null;
  const native = (message) => ext.runtime.sendNativeMessage(HOST, message).catch(() => ({ ok: false }));
  if (m.type === "panel") return { on: page ? await panelOn(page) : false };
  if (m.type === "video" && page) {
    const tab = sender.tab || {};
    const reply = await native({
      video: true,
      url: page,
      referrer: tab.url && tab.url !== page ? tab.url : "",
      cookies: await cookieHeader(page, { incognito: tab.incognito, storeId: tab.cookieStoreId }),
      userAgent: navigator.userAgent,
    });
    return { ok: Boolean(reply && reply.ok) };
  }
  if (m.type === "hide-site" && typeof m.host === "string" && m.host) {
    panelCache.set(m.host, { on: false, at: Date.now() });
    const reply = await native({ hideSite: m.host });
    return { ok: Boolean(reply && reply.ok) };
  }
  if (m.type === "options") return { ok: Boolean((await native({ openOptions: true })).ok) };
  return { ok: false };
}

ext.runtime.onMessage.addListener((m, sender, sendResponse) => {
  // Only our own content script (pages can't message an extension without being allowed to).
  if (!m || typeof m.type !== "string" || sender.id !== ext.runtime.id) return false;
  panelMessage(m, sender).then(sendResponse, () => sendResponse({ ok: false }));
  return true; // answered later
});

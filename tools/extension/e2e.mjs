// The whole browser integration in real browsers, with nothing on the screen: each browser runs
// headless with its own new profile and loads the extension (target/extension, from pack.ps1);
// the native messaging host (target/release) hands what it sends to a separate Mister Download
// Manager instance (its own data folder, MDM_DATA_DIR; off the screens, MDM_OFFSCREEN). Each case
// checks what MDM received (the lab server's request log, the files in MDM's folders, its list
// through the automation API, its Download video window through UI Automation) and that the
// browser kept nothing.
//
//   cargo build --release -p mdm-app -p mdm-nmhost -p mdm-labserver; tools\extension\pack.ps1
//   node tools/extension/e2e.mjs [chrome] [brave] [edge] [firefox] [--only=case,case]
//
// The cases (per browser): link (a .zip link, throttled and not), cookie (cookies and referrer
// forwarded), names (Content-Disposition), redirect, form (POST, 303), alt (Alt+click leaves it
// to the browser), opened (a PDF or MP3 the browser would show or play), csp (the video button
// under a strict CSP), hls (the stream a player loaded), video (shown, clicked, dragged, kept;
// "never on the site" in the last browser), start (MDM not running: the host starts it).
//
// Windows: the host is registered for the current user while it runs (nmhost --register, as the
// installer does) and unregistered at the end. Your own browser profiles are never used, nor the
// clipboard. Every process started here is stopped at the end.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const release = path.join(root, "target", "release");
export const exe = (name) => path.join(release, process.platform === "win32" ? `${name}.exe` : name);
const work = path.join(root, "target", "e2e");
const PORT = 8101;
const LAB = `http://127.0.0.1:${PORT}`;

const pf = process.env.ProgramFiles || "C:\\Program Files";
const pf86 = process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)";
const local = process.env.LOCALAPPDATA || "";
const BROWSERS = {
  chrome: ["chromium", [`${pf}\\Google\\Chrome\\Application\\chrome.exe`, `${local}\\Google\\Chrome\\Application\\chrome.exe`]],
  brave: ["chromium", [`${pf}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe`, `${local}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe`]],
  edge: ["chromium", [`${pf86}\\Microsoft\\Edge\\Application\\msedge.exe`, `${pf}\\Microsoft\\Edge\\Application\\msedge.exe`]],
  firefox: ["firefox", [`${pf}\\Mozilla Firefox\\firefox.exe`, `${pf86}\\Mozilla Firefox\\firefox.exe`]],
};

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export async function until(what, ms, test) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await test();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out: ${what}`);
    await sleep(250);
  }
}
export const killTree = (p) => p && p.exitCode === null && spawnSync("taskkill", ["/T", "/F", "/PID", String(p.pid)]);
// Every process using a test profile: a browser's launcher may exit and leave the browser (Firefox),
// so its tree is not enough. Only processes naming that folder are stopped.
export function killProfile(profile) {
  const folder = profile.replace(/'/g, "''");
  spawnSync("powershell", [
    "-NoProfile",
    "-Command",
    `Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -and $_.CommandLine.Contains('${folder}') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`,
  ]);
}

// Chromium's DevTools protocol over --remote-debugging-pipe (needed to load an extension).
export class Pipe {
  constructor(proc) {
    this.out = proc.stdio[3];
    this.next = 1;
    this.waiting = new Map();
    let buf = "";
    proc.stdio[4].on("data", (d) => {
      buf += d.toString("utf8");
      let i;
      while ((i = buf.indexOf("\0")) >= 0) {
        const m = JSON.parse(buf.slice(0, i));
        buf = buf.slice(i + 1);
        if (process.env.E2E_DEBUG && m.method) console.error("cdp", m.method, JSON.stringify(m.params).slice(0, 300));
        const w = m.id && this.waiting.get(m.id);
        if (w) {
          this.waiting.delete(m.id);
          m.error ? w.reject(new Error(`${w.method}: ${m.error.message}`)) : w.resolve(m.result);
        }
      }
    });
  }
  send(method, params = {}, sessionId) {
    const id = this.next++;
    this.out.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + "\0");
    return new Promise((resolve, reject) => this.waiting.set(id, { resolve, reject, method }));
  }
}

// Firefox's WebDriver BiDi over its remote-debugging WebSocket.
class Bidi {
  constructor(ws) {
    this.ws = ws;
    this.next = 1;
    this.waiting = new Map();
    ws.addEventListener("message", (e) => {
      const m = JSON.parse(e.data);
      const w = m.id && this.waiting.get(m.id);
      if (w) {
        this.waiting.delete(m.id);
        m.type === "error" ? w.reject(new Error(`${w.method}: ${m.error} ${m.message}`)) : w.resolve(m.result);
      }
    });
  }
  send(method, params = {}) {
    const id = this.next++;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.waiting.set(id, { resolve, reject, method }));
  }
}

// What the button's pill looks like (in its closed shadow root, which only the browser's own
// protocol reaches): its size and background.
const PILL_STYLE = `function () {
  const p = this.querySelector ? this.querySelector(".pill") : this;
  if (!p) return null;
  const r = p.getBoundingClientRect();
  const s = getComputedStyle(p);
  return JSON.stringify({ h: r.height, w: r.width, image: s.backgroundImage, color: s.backgroundColor });
}`;

// Each driver opens `page` in the browser with the extension loaded and returns `evaluate`
// (an expression in the page, run as a user's action), `mouse` (move or click at page
// coordinates, Alt held with `{alt: true}`), `navigate` (`{download: true}`: the address
// becomes a download, the page stays), `key`, `pill`, `major` (the browser's version) and
// `close`. `ready` resolves once the extension reached the native host.
async function chromium(bin, profile, dl, page, ready) {
  // The browser's own download folder, without asking, set as a user would: the profile's
  // preferences. (DevTools' Browser.setDownloadBehavior would take downloads past extensions.)
  fs.mkdirSync(path.join(profile, "Default"), { recursive: true });
  fs.writeFileSync(
    path.join(profile, "Default", "Preferences"),
    JSON.stringify({ download: { default_directory: dl, prompt_for_download: false, directory_upgrade: true } }),
  );
  const proc = spawn(
    bin,
    [
      "--headless",
      "--remote-debugging-pipe",
      "--enable-unsafe-extension-debugging",
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--window-size=1280,900",
      "about:blank",
    ],
    { env: process.env, stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"], windowsHide: true },
  );
  const cdp = new Pipe(proc);
  try {
    const { product } = await cdp.send("Browser.getVersion");
    const major = Number((/\/(\d+)\./.exec(product) || [])[1]) || 0;
    const { id } = await cdp.send("Extensions.loadUnpacked", { path: path.join(root, "target", "extension", "chromium") });
    // Its service worker runs, and has asked the host for the rules.
    const sw = await until("the extension's service worker", 15000, async () =>
      (await cdp.send("Target.getTargets")).targetInfos.find((t) => t.type === "service_worker" && t.url.includes(id)),
    );
    await ready();
    let trace = async () => "";
    if (process.env.E2E_DEBUG) {
      // Every downloads / native call the extension makes, with its result (E2E_DEBUG).
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId: sw.targetId, flatten: true });
      await cdp.send("Runtime.evaluate", { expression: `
        globalThis.trace = []; const t0 = Date.now();
        for (const [obj, names] of [[chrome.downloads, ["pause", "resume", "cancel", "erase"]], [chrome.runtime, ["sendNativeMessage"]]]) {
          for (const n of names) {
            const orig = obj[n].bind(obj);
            obj[n] = (...a) => { trace.push([Date.now() - t0, n, JSON.stringify(a).slice(0, 300)]);
              return orig(...a).then(r => { trace.push([Date.now() - t0, n + " ok", JSON.stringify(r).slice(0, 80)]); return r; },
                                     e => { trace.push([Date.now() - t0, n + " err", e.message]); throw e; }); };
          }
        }
        chrome.downloads.onCreated.addListener(d => trace.push([Date.now() - t0, "created", d.url, d.filename]));
        chrome.downloads.onChanged.addListener(d => trace.push([Date.now() - t0, "changed", JSON.stringify(d).slice(0, 140)]));
        "ok"` }, s);
      // With the browser's downloads, the session rules and what the extension stored.
      trace = async () =>
        (
          await cdp.send(
            "Runtime.evaluate",
            {
              expression: `(async () => JSON.stringify({ trace, downloads: (await chrome.downloads.search({})).map(d => [d.url, d.filename, d.state, d.error, d.mime]),
                rules: await chrome.declarativeNetRequest.getSessionRules(), stored: await chrome.storage.local.get(null) }))()`,
              awaitPromise: true,
            },
            s,
          )
        ).result.value;
    }
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, sessionId);
    const evaluate = async (expression) => {
      const r = await cdp.send("Runtime.evaluate", { expression, userGesture: true, awaitPromise: true, returnByValue: true }, sessionId);
      if (r.exceptionDetails) throw new Error(`evaluate: ${r.exceptionDetails.text} ${expression}`);
      return r.result.value;
    };
    const navigate = async (url, { download = false } = {}) => {
      await cdp.send("Page.navigate", { url }, sessionId);
      // A download leaves the page that was there; else the new page, loaded.
      const want = new URL(url).href;
      await until(`the page ${url}`, 15000, () =>
        evaluate("[location.href, document.readyState]")
          .then(([at, state]) => state === "complete" && (download || at === want))
          .catch(() => false),
      );
    };
    await navigate(page);
    // `act`: "move" (the default), "click", or "down" / "up" for a drag.
    let held = false;
    const mouse = async (x, y, act = "move", { alt = false } = {}) => {
      const at = { x, y, button: "left", buttons: held ? 1 : 0, modifiers: alt ? 1 : 0 };
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...at }, sessionId);
      if (act === "click" || act === "down") {
        held = true;
        await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", ...at, buttons: 1, clickCount: 1 }, sessionId);
      }
      if (act === "click" || act === "up") {
        held = false;
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...at, buttons: 0, clickCount: 1 }, sessionId);
      }
    };
    const KEYS = { ArrowDown: 40, Enter: 13 };
    const key = async (k) => {
      for (const type of ["keyDown", "keyUp"]) {
        // Enter carries its text, as a real key press does.
        const text = type === "keyDown" && k === "Enter" ? { text: "\r" } : {};
        await cdp.send("Input.dispatchKeyEvent", { type, key: k, code: k, windowsVirtualKeyCode: KEYS[k], ...text }, sessionId);
      }
    };
    // The button's pill, through its closed shadow root (DevTools sees through it).
    const pill = async () => {
      const { root: doc } = await cdp.send("DOM.getDocument", { depth: -1, pierce: true }, sessionId);
      const find = (n) => {
        if (n.nodeName === "MDM-VIDEO-BUTTON") return n;
        for (const c of [...(n.children || []), ...(n.shadowRoots || []), ...(n.contentDocument ? [n.contentDocument] : [])]) {
          const f = find(c);
          if (f) return f;
        }
        return null;
      };
      const host = find(doc);
      const shadow = host && (host.shadowRoots || [])[0];
      if (!shadow) return null;
      const { object } = await cdp.send("DOM.resolveNode", { nodeId: shadow.nodeId }, sessionId);
      const r = await cdp.send("Runtime.callFunctionOn", { objectId: object.objectId, functionDeclaration: PILL_STYLE, returnByValue: true }, sessionId);
      return r.result.value ? JSON.parse(r.result.value) : null;
    };
    return {
      extension: id,
      major,
      evaluate,
      mouse,
      navigate,
      key,
      pill,
      trace,
      close: () => {
        killTree(proc);
        killProfile(profile);
      },
    };
  } catch (e) {
    killTree(proc);
    killProfile(profile);
    throw e;
  }
}

async function firefox(bin, profile, dl, page, ready) {
  const port = 9300 + Math.floor(Math.random() * 500);
  const pref = (k, v) => `user_pref(${JSON.stringify(k)}, ${JSON.stringify(v)});`;
  fs.writeFileSync(
    path.join(profile, "user.js"),
    [
      pref("browser.download.dir", dl),
      pref("browser.download.folderList", 2),
      pref("browser.download.useDownloadDir", true),
      pref("browser.download.always_ask_before_handling_new_types", false),
      pref("browser.shell.checkDefaultBrowser", false),
      pref("datareporting.policy.dataSubmissionEnabled", false),
      pref("browser.aboutwelcome.enabled", false),
      // An extension's host permissions are granted without asking (MV3 asks the user).
      pref("extensions.originControls.grantByDefault", true),
      // Alt+click saves the link, as in Chromium (Firefox's default ignores an Alt+click on a
      // link: then nothing is downloaded at all, by the browser or MDM).
      pref("browser.altClickSave", true),
    ].join("\n"),
  );
  const proc = spawn(bin, ["-headless", "-no-remote", "-profile", profile, "--remote-debugging-port", String(port), "--width=1280", "--height=900"], {
    // Headless even where its launcher restarts it without the flag.
    env: { ...process.env, MOZ_HEADLESS: "1" },
    stdio: "ignore",
    windowsHide: true,
  });
  try {
    const ws = await until("Firefox's remote protocol", 30000, () =>
      new Promise((resolve) => {
        const s = new WebSocket(`ws://127.0.0.1:${port}/session`);
        s.addEventListener("open", () => resolve(s));
        s.addEventListener("error", () => resolve(null));
      }),
    );
    const bidi = new Bidi(ws);
    const { capabilities } = await bidi.send("session.new", { capabilities: {} });
    const major = Number(String(capabilities.browserVersion || "").split(".")[0]) || 0;
    const xpi = path.join(root, "target", "extension", "mdm-firefox.xpi");
    const { extension } = await bidi.send("webExtension.install", { extensionData: { type: "archivePath", path: xpi } });
    await ready();
    const { contexts } = await bidi.send("browsingContext.getTree", {});
    const context = contexts[0].context;
    // BiDi returns objects in its own form: the value goes through JSON instead.
    const evaluate = async (expression) => {
      const json = `(async () => JSON.stringify(await (${expression})))()`;
      const r = await bidi.send("script.evaluate", { expression: json, target: { context }, awaitPromise: true, userActivation: true });
      if (r.type === "exception") throw new Error(`evaluate: ${r.exceptionDetails.text} ${expression}`);
      return r.result.value === undefined ? undefined : JSON.parse(r.result.value);
    };
    const navigate = async (url, { download = false } = {}) => {
      if (!download) return bidi.send("browsingContext.navigate", { context, url, wait: "complete" });
      // The navigation ends in a download (an error for BiDi): the page stays.
      await bidi.send("browsingContext.navigate", { context, url, wait: "none" }).catch(() => {});
      await sleep(500);
      await until("the page", 15000, () => evaluate("document.readyState === 'complete'").catch(() => false));
    };
    await navigate(page);
    // Alt held: a key source pressing it around the pointer's actions, tick for tick.
    const mouse = async (x, y, act = "move", { alt = false } = {}) => {
      const actions = [{ type: "pointerMove", x: Math.round(x), y: Math.round(y) }];
      if (act === "click" || act === "down") actions.push({ type: "pointerDown", button: 0 });
      if (act === "click" || act === "up") actions.push({ type: "pointerUp", button: 0 });
      const sources = [{ type: "pointer", id: "mouse", actions }];
      if (alt) {
        const ALT = "\uE00A";
        sources.push({
          type: "key",
          id: "keyboard",
          actions: [{ type: "keyDown", value: ALT }, ...actions.slice(1).map(() => ({ type: "pause" })), { type: "keyUp", value: ALT }],
        });
        actions.push({ type: "pause" });
      }
      await bidi.send("input.performActions", { context, actions: sources });
    };
    // WebDriver key codes.
    const KEYS = { ArrowDown: "\uE015", Enter: "\uE007" };
    const key = (k) =>
      bidi.send("input.performActions", {
        context,
        actions: [{ type: "key", id: "keyboard", actions: [{ type: "keyDown", value: KEYS[k] }, { type: "keyUp", value: KEYS[k] }] }],
      });
    // The pill, through the closed shadow root BiDi serializes with includeShadowTree "all".
    const pill = async () => {
      const r = await bidi.send("script.evaluate", {
        expression: `document.querySelector("mdm-video-button")`,
        target: { context },
        awaitPromise: false,
        resultOwnership: "root",
        serializationOptions: { includeShadowTree: "all", maxDomDepth: 1 },
      });
      const shadow = r.result && r.result.value && r.result.value.shadowRoot;
      if (!shadow || !shadow.sharedId) return null;
      const c = await bidi.send("script.callFunction", {
        functionDeclaration: PILL_STYLE,
        this: { sharedId: shadow.sharedId },
        target: { context },
        awaitPromise: false,
      });
      return c.result && c.result.value ? JSON.parse(c.result.value) : null;
    };
    return {
      extension,
      major,
      evaluate,
      mouse,
      navigate,
      key,
      pill,
      close: () => {
        ws.close();
        killTree(proc);
        killProfile(profile);
      },
    };
  } catch (e) {
    killTree(proc);
    killProfile(profile);
    throw e;
  }
}

// ---- the test instance of Mister Download Manager ----------------------------------------------

let data = "";
let downloads = "";
const appExe = exe("misterdownloadmanager");

// One command to the instance over its channel (ipc.json), its one-line answer.
function ipc(cmd) {
  return new Promise((resolve, reject) => {
    let ep;
    try {
      ep = JSON.parse(fs.readFileSync(path.join(data, "ipc.json"), "utf8"));
    } catch (e) {
      return reject(e);
    }
    const s = net.connect(ep.port, "127.0.0.1", () => s.write(JSON.stringify({ token: ep.token, ...cmd }) + "\n"));
    let answer = "";
    s.setTimeout(5000, () => s.destroy(new Error("no answer")));
    s.on("data", (d) => (answer += d));
    s.on("end", () => resolve(answer.trim()));
    s.on("error", reject);
  });
}

// The instance's list (the automation API, on in the test settings).
async function mdmList() {
  const a = JSON.parse(await ipc({ query: "list" }));
  if (!a.downloads) throw new Error(`the automation API: ${JSON.stringify(a)}`);
  return a.downloads;
}

// The running copies of target/release's app (each with its crash monitor, the same program with
// --crash-monitor): [pid, is the monitor].
function appProcesses() {
  const p = appExe.replace(/'/g, "''");
  const out = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `Get-CimInstance Win32_Process -Filter "Name='misterdownloadmanager.exe'" | Where-Object { $_.ExecutablePath -eq '${p}' } | ForEach-Object { "$($_.ProcessId) $($_.CommandLine -like '*--crash-monitor*')" }`,
    ],
    { encoding: "utf8" },
  ).stdout;
  return out
    .split(/\r?\n/)
    .filter(Boolean)
    .map((l) => l.trim().split(" "))
    .map(([pid, monitor]) => [Number(pid), monitor === "True"]);
}
// Copies that ran before the test (never touched), and so ours.
let before = new Set();
const ourPids = () => appProcesses().map(([pid]) => pid).filter((p) => !before.has(p));
// Our app itself, without its monitor.
const ourApps = () => appProcesses().filter(([pid, monitor]) => !monitor && !before.has(pid)).map(([pid]) => pid);

async function startApp() {
  if (ourPids().length) return;
  fs.rmSync(path.join(data, "ipc.json"), { force: true });
  spawn(appExe, [], { env: process.env, stdio: "ignore", detached: false });
  await until("the test instance", 20000, () => mdmList().then(() => true, () => false));
}

// Saved and quit through its channel; stopped if it doesn't.
async function stopApp() {
  if (!ourPids().length) return;
  await ipc({ exit: true }).catch(() => {});
  try {
    await until("the test instance to quit", 15000, () => !ourPids().length);
  } catch {
    for (const pid of ourPids()) spawnSync("taskkill", ["/T", "/F", "/PID", String(pid)]);
  }
}

// The download named `file` in MDM's `sub` folder (under the test's Downloads), complete in its
// list and on disk with `size` bytes; its list entry.
async function arrived(file, size, sub = "Compressed", ms = 45000) {
  const want = path.join(downloads, sub, file);
  let last = null;
  try {
    return await until(`${file} in Mister Download Manager`, ms, async () => {
      const item = (await mdmList().catch(() => [])).find((d) => d.file === file);
      last = item || last;
      if (item && item.status === "Error") throw new Error(`${file}: MDM's download failed: ${item.error}`);
      if (!item || item.status !== "Complete") return null;
      if (path.resolve(item.path) !== path.resolve(want)) throw new Error(`${file} saved as ${item.path}, not ${want}`);
      return fs.existsSync(want) && fs.statSync(want).size === size && item;
    });
  } catch (e) {
    if (last && !/MDM's|saved as/.test(e.message)) e.message += ` (MDM: ${last.status} ${last.downloaded}/${last.size} ${last.path})`;
    if (!last) {
      const list = await mdmList().catch(() => []);
      e.message += ` (MDM's latest: ${list.slice(-2).map((d) => `${d.file} ${d.status} ${d.path}`).join("; ") || "none"})`;
    }
    throw e;
  }
}

// The lab server's request log for `p` (a path), oldest first.
async function requests(p) {
  const log = await (await fetch(`${LAB}/_log`)).json();
  return log.filter((e) => e.path === p).sort((a, b) => a.req - b.req);
}
const clearLog = () => fetch(`${LAB}/_log`, { method: "DELETE" });
const brief = (e) => `${e.method} ${e.status} ${e.outcome} ${e.range || "-"}`;
const sizeOf = async (url) => Number((await fetch(url, { method: "HEAD" })).headers.get("content-length"));

// The browser kept nothing: its download folder is empty (no .part, no .crdownload, no
// "Unconfirmed", no placeholder).
async function keptNothing(dl) {
  await sleep(1000);
  const left = fs.readdirSync(dl);
  if (left.length) throw new Error(`the browser kept: ${left.join(", ")}`);
}

// ---- the test instance's windows (Windows) ----------------------------------------------------

// Its windows: titles, closing the one titled `close`, and those that show on a screen. Collected
// in C#: what a PowerShell callback prints is lost.
function appWindows(pid, close = "", onScreen = false) {
  const script = `
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class W {
  public delegate bool P(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
  [DllImport("user32.dll")] static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  public static List<string> Of(uint pid, string close, bool onScreen) {
    var r = new List<string>();
    int sx = GetSystemMetrics(76), sy = GetSystemMetrics(77), sw = GetSystemMetrics(78), sh = GetSystemMetrics(79);
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
        var t = sb.ToString(); R rc; GetWindowRect(h, out rc);
        bool seen = rc.Rt > sx && rc.L < sx + sw && rc.B > sy && rc.T < sy + sh && rc.Rt > rc.L && rc.B > rc.T;
        if (t.Length > 0 && (!onScreen || seen)) { r.Add(t); if (t == close) PostMessage(h, 0x10, IntPtr.Zero, IntPtr.Zero); } }
      return true; }, IntPtr.Zero);
    return r;
  }
}
"@
[W]::Of(${pid}, '${close.replace(/'/g, "''")}', $${onScreen})`;
  return spawnSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).stdout.split(/\r?\n/).filter(Boolean);
}

// The address in the Download video window's "Page address" box, read through UI Automation
// (the app's windows have AccessKit adapters, also off the screens); null while it isn't open.
function videoAddress(pid) {
  const script = `
Add-Type -AssemblyName UIAutomationClient; Add-Type -AssemblyName UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$pid_ = New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, ${pid})
$edit = New-Object System.Windows.Automation.AndCondition(
  (New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)),
  (New-Object System.Windows.Automation.PropertyCondition($A::NameProperty, 'Page address')))
foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $pid_)) {
  $e = $w.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $edit)
  if ($e) { 'ADDRESS ' + $e.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value; break }
}`;
  const out = spawnSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).stdout;
  const m = /^ADDRESS (.*)$/m.exec(out);
  return m ? m[1].trim() : null;
}

const VIDEO_WINDOW = "Download video from a page";
const appPid = () => {
  const [pid] = ourApps();
  if (!pid) throw new Error("the test instance isn't running");
  return pid;
};

// ---- the download button over videos ----------------------------------------------------------

const near = (a, b, what) => {
  if (Math.abs(a - b) > 6) throw new Error(`${what}: ${Math.round(a)} instead of ${Math.round(b)}`);
};

// An element's rectangle (the button's only while shown).
const rect = (session, sel) =>
  session.evaluate(`(() => { const e = document.querySelector(${JSON.stringify(sel)});
    if (!e || (e.localName === "mdm-video-button" && e.style.display !== "block")) return null;
    const r = e.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom }; })()`);

// Point at the video (rectangle `v`) until the button shows; its rectangle, or null.
async function hover(session, v) {
  for (let i = 0; i < 40; i++) {
    await session.mouse((v.l + v.r) / 2 + (i % 2), (v.t + v.b) / 2);
    const b = await rect(session, "mdm-video-button");
    if (b) return b;
    await sleep(200);
  }
  return null;
}

// Click the button's main part: the app's Download video window opens; the address it shows.
async function clickButton(session, b) {
  const pid = appPid();
  appWindows(pid, VIDEO_WINDOW);
  await until("the Download video window closed", 5000, () => !appWindows(pid).includes(VIDEO_WINDOW));
  await session.mouse(b.l + 14, (b.t + b.b) / 2, "click");
  await until("the Download video window", 15000, () => appWindows(pid).includes(VIDEO_WINDOW));
  const address = await until("the Download video window's address", 10000, () => videoAddress(pid));
  appWindows(pid, VIDEO_WINDOW);
  return address;
}

// Point at a video, see the button at its top right, click it (the app's video window opens),
// drag it to the bottom left (it stays there after a reload); `never`: its menu's "Never on
// this site" by keyboard, then it no longer shows and the app lists the site.
async function videoButton(session, never) {
  const page = `${LAB}/page?video=${encodeURIComponent("/gen/1M/clip.mp4")}`;
  await session.navigate(page);
  const v = await rect(session, "#video");
  let b = await hover(session, v);
  if (!b) throw new Error("no button over the video");
  near(b.r, v.r - 10, "button's right edge");
  near(b.t, v.t + 10, "button's top");
  // Click its main part: the app's Download video window opens for the video's own file.
  const address = await clickButton(session, b);
  if (address !== `${LAB}/gen/1M/clip.mp4`) throw new Error(`the video window got ${address}`);
  // Drag it to the bottom left: it snaps to that corner.
  await session.mouse(b.l + 14, (b.t + b.b) / 2, "down");
  await session.mouse(b.l + 30, (b.t + b.b) / 2 + 20);
  await session.mouse(v.l + 40, v.b - 30);
  await session.mouse(v.l + 40, v.b - 30, "up");
  await sleep(300);
  b = await rect(session, "mdm-video-button");
  near(b.l, v.l + 10, "dragged: left edge");
  near(b.b, v.b - 10, "dragged: bottom");
  if (appWindows(appPid()).includes(VIDEO_WINDOW)) throw new Error("a drag opened the video window");
  // Remembered for the site.
  await session.mouse(5, 5);
  await session.navigate(page);
  b = await hover(session, v);
  if (!b) throw new Error("no button after a reload");
  near(b.l, v.l + 10, "after a reload: left edge");
  near(b.b, v.b - 10, "after a reload: bottom");
  if (!never) return "shown, clicked (the video's file), dragged, kept";
  // Its menu by keyboard: open, down to "Never on 127.0.0.1", Enter.
  await session.mouse(b.r - 12, (b.t + b.b) / 2, "click");
  await sleep(200);
  await session.key("ArrowDown");
  await session.key("ArrowDown");
  await session.key("Enter");
  await sleep(500);
  if (await rect(session, "mdm-video-button")) throw new Error("Never on the site: the button is still shown after the menu command");
  await until("the site in the app's list", 15000, () => {
    try {
      const s = JSON.parse(fs.readFileSync(path.join(data, "downloads.json"), "utf8").replace(/^\uFEFF/, "")).settings;
      return (s.video_panel_sites || "").split(" ").includes("127.0.0.1");
    } catch {
      return false;
    }
  });
  await session.mouse(5, 5);
  await session.navigate(page);
  if (await hover(session, v)) throw new Error("the button still shows on a site it should never show on");
  return "shown, clicked (the video's file), dragged, kept, never on the site";
}

// ---- the cases ---------------------------------------------------------------------------------

class Skip extends Error {}

// A link on a page, clicked as a user does.
async function clickLink(t, link) {
  await t.session.navigate(`${LAB}/page?link=${encodeURIComponent(link)}`);
  await t.session.evaluate("document.getElementById('link').click()");
}

// The requests MDM made for `p`: all but the browser's own first one.
async function mdmRequests(p) {
  const all = await requests(p);
  if (all.length < 2) throw new Error(`${p}: only the browser's request reached the server (${all.map(brief).join("; ")})`);
  return all.slice(1);
}

const CASES = {
  // A .zip link, throttled (the browser's download is held while MDM is asked: Firefox pauses it)
  // and at full speed.
  async link(t) {
    const done = [];
    for (const [file, query] of [[`e2e-${t.name}-slow.zip`, "?rate=256K"], [`e2e-${t.name}.zip`, ""]]) {
      const url = `/gen/2M/${file}${query}`;
      const size = await sizeOf(LAB + url);
      await clickLink(t, url);
      await arrived(file, size);
      await keptNothing(t.dl);
      done.push(query ? "throttled" : "full speed");
    }
    return done.join(", ");
  },

  // Cookies and referrer: a file behind a session cookie and hotlink protection.
  async cookie(t) {
    await t.session.navigate(`${LAB}/cookie?set=sid=1`);
    const file = `e2e-c-${t.name}.zip`;
    const prefix = `${LAB}/page`;
    const link = `/gen/1M/${file}?require_cookie=sid%3D1&require_referer=${encodeURIComponent(prefix)}`;
    await clickLink(t, link);
    await arrived(file, 1 << 20);
    const mdm = await mdmRequests(`/gen/1M/${file}`);
    for (const e of mdm) {
      if (!/(^|;\s*)sid=1(;|$)/.test(e.cookie || "")) throw new Error(`MDM's request without the cookie: ${brief(e)} cookie=${e.cookie}`);
      if (!(e.referer || "").startsWith(prefix)) throw new Error(`MDM's request without the referrer: ${brief(e)} referer=${e.referer}`);
      if (e.user_agent !== t.ua) throw new Error(`MDM's User-Agent ${e.user_agent} is not the browser's ${t.ua}`);
      if (e.status >= 400) throw new Error(`MDM's request refused: ${brief(e)}`);
    }
    await keptNothing(t.dl);
    return `${mdm.length} request(s) with sid=1, the page as referrer, the browser's User-Agent`;
  },

  // Names from Content-Disposition: filename= and filename*= (UTF-8).
  async names(t) {
    const plain = `e2e-report-${t.name}.zip`;
    const star = `报告-e2e-${t.name}.zip`;
    await clickLink(t, `/gen/1M/download.php?cd=${encodeURIComponent(plain)}`);
    await arrived(plain, 1 << 20);
    await keptNothing(t.dl);
    await clickLink(t, `/gen/1M/get?cdstar=${encodeURIComponent(star)}`);
    await arrived(star, 1 << 20);
    await keptNothing(t.dl);
    const wrong = fs.readdirSync(path.join(downloads, "Compressed")).filter((f) => /^(download\.php|get|fallback-name\.zip)/.test(f));
    if (wrong.length) throw new Error(`saved under the address's or the fallback name too: ${wrong.join(", ")}`);
    return `${plain}, ${star}`;
  },

  // A redirect: MDM keeps the address clicked (it asks it again to renew the link).
  async redirect(t) {
    const file = `e2e-r-${t.name}.zip`;
    const link = `/redirect?to=${encodeURIComponent(`/gen/2M/${file}`)}`;
    await clickLink(t, link);
    const item = await arrived(file, 2 << 20);
    if (item.url !== LAB + link) throw new Error(`MDM's address is ${item.url}, not the one clicked (${LAB + link})`);
    await mdmRequests(`/gen/2M/${file}`);
    await keptNothing(t.dl);
    return "the address clicked kept, the file from its target";
  },

  // A form's download: MDM posts it again with the form's body; one answered 303 too.
  async form(t) {
    const name = `e2e-form-${t.name}.zip`;
    await t.session.navigate(`${LAB}/formpage?size=1M&name=${encodeURIComponent(name)}`);
    await t.session.evaluate("document.getElementById('submit-form').click()");
    await arrived(name, 1 << 20);
    const posts = await mdmRequests(`/form/1M/${name}`);
    for (const e of posts) {
      if (e.method !== "POST") throw new Error(`MDM didn't post the form again: ${brief(e)}`);
      if (!/(^|&)token=abc(&|$)/.test(e.req_body_head || "")) throw new Error(`MDM's post without token=abc: ${e.req_body_head}`);
      if (e.status >= 400) throw new Error(`MDM's post refused: ${brief(e)}`);
    }
    await keptNothing(t.dl);
    // Posted, answered "303 See Other" with the file's address.
    const other = `see-other-${name}`;
    await t.session.navigate(`${LAB}/formpage?size=1M&name=${encodeURIComponent(name)}`);
    await t.session.evaluate("document.getElementById('submit-see-other').click()");
    await arrived(other, 1 << 20);
    const seen = [...(await requests(`/form/1M/${other}`)), ...(await requests(`/gen/1M/${other}`))].sort((a, b) => a.req - b.req);
    await keptNothing(t.dl);
    return `posted again with token=abc (${posts.length} request(s)); 303: ${seen.map((e) => `${e.method} ${e.path.split("/")[1]} ${e.status}`).join(", ")}`;
  },

  // Alt+click: the browser keeps the download, MDM gets nothing.
  async alt(t) {
    const file = `e2e-alt-${t.name}.zip`;
    const p = `/gen/1M/${file}`;
    await t.session.navigate(`${LAB}/page?link=${encodeURIComponent(p)}`);
    const a = await rect(t.session, "#link");
    await t.session.mouse((a.l + a.r) / 2, (a.t + a.b) / 2, "click", { alt: true });
    const kept = path.join(t.dl, file);
    try {
      await until(`${file} in the browser's folder`, 20000, () => {
        const names = fs.readdirSync(t.dl);
        return names.length === 1 && names[0] === file && fs.statSync(kept).size === 1 << 20;
      });
    } catch (e) {
      const item = (await mdmList()).find((d) => d.file === file);
      const tab = await t.session.evaluate("location.href").catch((x) => x.message);
      e.message += ` (MDM: ${item ? `${item.status} ${item.path}` : "nothing"}; the tab: ${tab}; requests: ${(await requests(p)).map(brief).join("; ") || "none"}; the browser's folder: ${fs.readdirSync(t.dl).join(", ") || "empty"})`;
      throw e;
    }
    await sleep(1500);
    const item = (await mdmList()).find((d) => d.file === file);
    if (item) throw new Error(`MDM got it too: ${item.status} ${item.path}`);
    const all = await requests(p);
    if (all.length !== 1) throw new Error(`${all.length} requests for it (only the browser's expected): ${all.map(brief).join("; ")}`);
    fs.rmSync(kept);
    return "the browser's own download, MDM untouched";
  },

  // Files the browser would show or play, made downloads, MDM's (Chromium 128+, Firefox): typed
  // into the address bar, the tab stays on its page. A PDF sent with "Content-Disposition:
  // inline; filename=..." (as many servers do), one sent without it, an MP3 sent inline.
  async opened(t) {
    if (t.kind === "chromium" && t.session.major < 128) throw new Skip(`${t.name} ${t.session.major}: needs 128 (header conditions)`);
    const failed = [];
    const done = [];
    const left = [];
    for (const [file, query, sub, size] of [
      [`e2e-doc-${t.name}.pdf`, "inline=1", "Documents", "1M"],
      [`e2e-bare-${t.name}.pdf`, "inline=bare", "Documents", "1M"],
      [`e2e-song-${t.name}.mp3`, "inline=1", "Music", "512K"],
    ]) {
      if (idmRunning && file.endsWith(".pdf") && query === "inline=1") {
        left.push(`pdf ${query} (left out: IDM runs and takes it)`);
        continue;
      }
      const p = `/gen/${size}/${file}`;
      await t.session.navigate(`${LAB}/page?link=none`);
      await t.session.navigate(`${LAB}${p}?${query}`, { download: true });
      try {
        await arrived(file, await sizeOf(LAB + p), sub, 20000);
        const [at, type] = await t.session.evaluate("[location.pathname, document.contentType]");
        if (at !== "/page" || type !== "text/html") throw new Error(`the tab shows ${at} (${type}), not the page it was on`);
        await keptNothing(t.dl);
        done.push(`${file.split(".").pop()} ${query}`);
      } catch (e) {
        const tab = await t.session.evaluate("[location.href, document.contentType]").catch((x) => x.message);
        const log = (await requests(p)).map(brief).join("; ");
        failed.push(`${file} (${query}): ${e.message} [the tab: ${JSON.stringify(tab)}; requests: ${log}; the browser's folder: ${fs.readdirSync(t.dl).join(", ") || "empty"}]`);
        for (const f of fs.readdirSync(t.dl)) fs.rmSync(path.join(t.dl, f), { force: true, recursive: true });
      }
    }
    if (failed.length) throw new Error(`${failed.join(" | ")}${done.length ? ` (taken: ${done.join(", ")})` : ""}`);
    return `taken, the tab stayed: ${[...done, ...left].join(", ")}`;
  },

  // The video button on a page whose Content-Security-Policy allows no inline style.
  async csp(t) {
    await t.session.navigate(`${LAB}/page?csp=strict`);
    const v = await rect(t.session, "#video");
    const b = await hover(t.session, v);
    if (!b) throw new Error("no button over the video under csp=strict");
    near(b.b - b.t, 34, "the button's height");
    const pill = await t.session.pill();
    if (!pill) throw new Error("the button's pill wasn't found");
    if (Math.abs(pill.h - 34) > 2 || !/gradient/.test(pill.image)) throw new Error(`the pill isn't styled: ${JSON.stringify(pill)}`);
    const address = await clickButton(t.session, b);
    const want = `${LAB}/gen/1M/clip.mp4?inline=1`;
    if (address !== want) throw new Error(`the video window got ${address}, not ${want}`);
    return `styled (${Math.round(pill.h)}px, ${pill.image.slice(0, 15)}...), sent ${address.replace(LAB, "")}`;
  },

  // A player playing a stream (a blob: in the page): the button sends the manifest it loaded.
  async hls(t) {
    await t.session.navigate(`${LAB}/page?hls=1`);
    const state = await until("the stream player", 15000, () => t.session.evaluate("document.body.dataset.stream || ''"));
    if (state !== "done") throw new Error(`the page's player: ${state}`);
    const v = await rect(t.session, "#player");
    const b = await hover(t.session, v);
    if (!b) throw new Error("no button over the stream's player");
    const address = await clickButton(t.session, b);
    const want = `${LAB}/hls/master.m3u8`;
    if (address !== want) throw new Error(`the video window got ${address}, not ${want}`);
    return `sent ${address.replace(LAB, "")}`;
  },

  async video(t) {
    return videoButton(t.session, t.last);
  },

  // MDM not running: the host starts it (off the screens, with the test's data folder: both
  // reach it through the browser's environment) and hands the download over.
  async start(t) {
    await stopApp();
    const file = `e2e-start-${t.name}.zip`;
    await clickLink(t, `/gen/1M/${file}`);
    await arrived(file, 1 << 20, "Compressed", 60000);
    const pids = ourApps();
    if (pids.length !== 1) throw new Error(`${pids.length} instances run after the download`);
    const shown = pids.flatMap((p) => appWindows(p, "", true));
    await keptNothing(t.dl);
    await stopApp();
    if (shown.length) throw new Error(`the instance the host started shows on the screen: ${shown.join(", ")}`);
    return `started by the host (pid ${pids[0]}), off the screens, stopped`;
  },
};

// Known product bugs: a case listed here still runs, and its failure is reported as BUG (with
// what was found) instead of FAILED. Key: "case" or "browser:case".
// A product failure known and not fixed yet, by case ("browser:case" or "case"): listed as BUG
// (still failing the run) rather than FAILED.
const KNOWN_BUGS = {};

// Is Internet Download Manager running (E2E_IDM_OK=1)? Even with its 127.0.0.1 exception it takes
// a PDF sent "inline; filename=..." at the network level (the browser gets "204 No Content",
// IDM downloads the file itself), so that case is left out while it runs.
let idmRunning = false;

async function run() {
  const args = process.argv.slice(2);
  const only = (args.find((a) => a.startsWith("--only=")) || "").slice(7).split(",").filter(Boolean);
  const wanted = args.filter((a) => !a.startsWith("--"));
  const names = (wanted.length ? wanted : Object.keys(BROWSERS)).filter((n) => {
    if (!BROWSERS[n]) throw new Error(`unknown browser ${n}`);
    return wanted.length || BROWSERS[n][1].some((p) => fs.existsSync(p));
  });
  for (const c of only) if (!CASES[c]) throw new Error(`unknown case ${c} (${Object.keys(CASES).join(", ")})`);
  // IDM's browser integration takes browsers' downloads at the network level (the browser gets
  // "204 No Content"): it would take the test's files and show its own windows. E2E_IDM_OK=1 when
  // IDM leaves 127.0.0.1 alone (an exception for it in its options).
  if (process.platform === "win32") {
    const idm = spawnSync("tasklist", ["/FI", "IMAGENAME eq IDMan.exe", "/NH"], { encoding: "utf8" }).stdout;
    idmRunning = /IDMan\.exe/i.test(idm);
    if (idmRunning && process.env.E2E_IDM_OK !== "1") throw new Error("Internet Download Manager is running: quit it first (it takes the browsers' downloads), or set E2E_IDM_OK=1 if it leaves 127.0.0.1 alone");
  }
  for (const p of [appExe, exe("misterdownloadmanager-nmhost"), exe("labserver")]) {
    if (!fs.existsSync(p)) throw new Error(`build it first: ${p}`);
  }
  const t0 = Date.now();
  fs.rmSync(work, { recursive: true, force: true });
  data = path.join(work, "data");
  downloads = path.join(work, "Downloads");
  fs.mkdirSync(data, { recursive: true });
  // Captured downloads start at once (no Download File Info) into folders under target/e2e; the
  // automation API answers what the list holds.
  const dirs = ["General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents"].map((c) => [
    c,
    c === "General" ? downloads : path.join(downloads, c),
  ]);
  fs.writeFileSync(
    path.join(data, "downloads.json"),
    JSON.stringify({
      next_id: 1,
      items: [],
      version: 2,
      settings: {
        language: "en",
        language_asked: true,
        extension_never_ask: true,
        show_start_dialog: false,
        automation_api: true,
        category_dirs: dirs,
      },
    }),
  );
  // The browsers pass these on to the host, and the host to an app it starts.
  process.env.MDM_DATA_DIR = data;
  process.env.MDM_OFFSCREEN = "1";

  before = new Set(appProcesses().map(([pid]) => pid));
  const server = spawn(exe("labserver"), ["--bind", `127.0.0.1:${PORT}`], { stdio: "ignore", windowsHide: true });
  const nmhost = exe("misterdownloadmanager-nmhost");
  const results = [];
  try {
    await until("the lab server", 10000, () => fetch(`${LAB}/`).then((r) => r.ok, () => false));
    // An installed Mister Download Manager's registration is never replaced or removed.
    const key = "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.misterdownloadmanager.host";
    if (spawnSync("reg", ["query", key], { stdio: "ignore" }).status === 0) {
      throw new Error("Mister Download Manager's host is already registered (an installed copy): not touching it");
    }
    const reg = spawnSync(nmhost, ["--register"], { encoding: "utf8" });
    if (reg.status !== 0) throw new Error(`nmhost --register: ${reg.stderr || reg.stdout}`);
    for (const name of names) {
      const [kind, candidates] = BROWSERS[name];
      const bin = candidates.find((p) => fs.existsSync(p));
      const profile = path.join(work, `profile-${name}`);
      const dl = path.join(work, `browser-${name}`);
      fs.mkdirSync(profile, { recursive: true });
      fs.mkdirSync(dl, { recursive: true });
      const started = Date.now();
      let session;
      try {
        if (!bin) throw new Error("not installed");
        await startApp();
        // The extension, installed, asked the host for the rules: the host notes the browser in
        // extension-seen (by the name Options gives it).
        const seen = path.join(data, "extension-seen");
        fs.rmSync(seen, { force: true });
        let seenAs = "";
        const ready = () =>
          until(`${name}'s extension reaching the host`, 20000, () => {
            try {
              seenAs = fs.readFileSync(seen, "utf8").split("\t")[0];
              return seenAs;
            } catch {
              return false;
            }
          }).then(() => sleep(300)); // its answer stored
        session = await (kind === "firefox" ? firefox : chromium)(bin, profile, dl, `${LAB}/page?link=none`, ready);
        const t = { name, kind, session, dl, last: name === names[names.length - 1], ua: await session.evaluate("navigator.userAgent") };
        results.push([name, "browser", "ok", `${kind} ${session.major} (to MDM: ${seenAs}), extension ${session.extension}`, Date.now() - started]);
        for (const [label, fn] of Object.entries(CASES)) {
          if (only.length && !only.includes(label)) continue;
          const at = Date.now();
          try {
            await startApp();
            await clearLog();
            const detail = await fn(t);
            results.push([name, label, "ok", detail || "", Date.now() - at]);
          } catch (e) {
            const bug = KNOWN_BUGS[`${name}:${label}`] || KNOWN_BUGS[label];
            const status = e instanceof Skip ? "skipped" : bug ? "BUG" : "FAILED";
            results.push([name, label, status, bug ? `${bug} -- ${e.message}` : e.message, Date.now() - at]);
            // What the browser kept, so the next case starts clean.
            for (const f of fs.readdirSync(dl)) fs.rmSync(path.join(dl, f), { force: true, recursive: true });
          }
          console.error(`  ${name} ${label}: ${results[results.length - 1][2]}`);
          // E2E_DEBUG: what the extension did (Chromium).
          if (session.trace && process.env.E2E_DEBUG) console.error(`${name} ${label} trace:`, await session.trace());
        }
      } catch (e) {
        results.push([name, "browser", "FAILED", e.message, Date.now() - started]);
      } finally {
        if (session) session.close();
      }
    }
  } finally {
    spawnSync(nmhost, ["--unregister"]);
    await stopApp().catch(() => {});
    for (const pid of ourPids()) spawnSync("taskkill", ["/T", "/F", "/PID", String(pid)]);
    killTree(server);
  }
  for (const [name, label, status, detail, ms] of results) {
    console.log(`${name.padEnd(8)} ${label.padEnd(9)} ${status.padEnd(8)} ${(ms / 1000).toFixed(1).padStart(5)}s  ${detail}`);
  }
  console.log(`total ${((Date.now() - t0) / 1000).toFixed(0)}s`);
  if (results.some((r) => r[2] === "FAILED" || r[2] === "BUG")) process.exit(1);
}

// Run when started, not when another script imports its helpers (tools/readme/browser-shots.mjs).
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) run().catch((e) => {
  console.error(e.message);
  process.exit(1);
});

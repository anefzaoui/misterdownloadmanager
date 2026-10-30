// The whole browser integration in real browsers, with nothing on the screen: each browser runs
// headless with its own new profile, loads the extension (target/extension, from pack.ps1),
// opens a link to a .zip on the lab server, and passes when the file arrives through the native
// messaging host (target/release) in a separate Mister Download Manager instance (its own data
// folder, MDM_DATA_DIR; off the screens, MDM_OFFSCREEN) and the browser kept none of it.
//
//   cargo build --release -p mdm-app -p mdm-nmhost -p mdm-labserver; tools\extension\pack.ps1
//   node tools/extension/e2e.mjs [chrome] [brave] [edge] [firefox]     (all found by default)
//
// Windows: the host is registered for the current user while it runs (nmhost --register, as the
// installer does) and unregistered at the end. Your own browser profiles are never used.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
export const release = path.join(root, "target", "release");
export const exe = (name) => path.join(release, process.platform === "win32" ? `${name}.exe` : name);
const work = path.join(root, "target", "e2e");
const PORT = 8101;

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

// Each driver opens `page` in the browser with the extension loaded and returns `evaluate`
// (an expression in the page, run as a user's action), `mouse` (move or click at page
// coordinates) and `close`.
async function chromium(bin, profile, dl, page) {
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
    const { id } = await cdp.send("Extensions.loadUnpacked", { path: path.join(root, "target", "extension", "chromium") });
    await sleep(1500); // the service worker starts
    let trace = async () => "";
    if (process.env.E2E_DEBUG) {
      // Every downloads / native call the extension makes, with its result (E2E_DEBUG).
      const { targetInfos } = await cdp.send("Target.getTargets");
      const sw = targetInfos.find((t) => t.type === "service_worker" && t.url.includes(id));
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId: sw.targetId, flatten: true });
      await cdp.send("Runtime.evaluate", { expression: `
        globalThis.trace = []; const t0 = Date.now();
        for (const [obj, names] of [[chrome.downloads, ["pause", "resume", "cancel", "erase"]], [chrome.runtime, ["sendNativeMessage"]]]) {
          for (const n of names) {
            const orig = obj[n].bind(obj);
            obj[n] = (...a) => { trace.push([Date.now() - t0, n, JSON.stringify(a).slice(0, 100)]);
              return orig(...a).then(r => { trace.push([Date.now() - t0, n + " ok", JSON.stringify(r).slice(0, 80)]); return r; },
                                     e => { trace.push([Date.now() - t0, n + " err", e.message]); throw e; }); };
          }
        }
        chrome.downloads.onCreated.addListener(d => trace.push([Date.now() - t0, "created", d.url, d.filename]));
        chrome.downloads.onChanged.addListener(d => trace.push([Date.now() - t0, "changed", JSON.stringify(d).slice(0, 140)]));
        "ok"` }, s);
      trace = async () => (await cdp.send("Runtime.evaluate", { expression: "JSON.stringify(trace)" }, s)).result.value;
    }
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, sessionId);
    await cdp.send("Page.navigate", { url: page }, sessionId);
    const evaluate = async (expression) => {
      const r = await cdp.send("Runtime.evaluate", { expression, userGesture: true, awaitPromise: true, returnByValue: true }, sessionId);
      if (r.exceptionDetails) throw new Error(`evaluate: ${r.exceptionDetails.text} ${expression}`);
      return r.result.value;
    };
    await until("the page", 15000, () => evaluate("document.readyState === 'complete'").catch(() => false));
    // `act`: "move" (the default), "click", or "down" / "up" for a drag.
    let held = false;
    const mouse = async (x, y, act = "move") => {
      const at = { x, y, button: "left", buttons: held ? 1 : 0 };
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
    const navigate = async (url) => {
      await cdp.send("Page.navigate", { url }, sessionId);
      await sleep(300);
      await until("the page", 15000, () => evaluate("document.readyState === 'complete'").catch(() => false));
    };
    const KEYS = { ArrowDown: 40, Enter: 13 };
    const key = async (k) => {
      for (const type of ["keyDown", "keyUp"]) {
        // Enter carries its text, as a real key press does.
        const text = type === "keyDown" && k === "Enter" ? { text: "\r" } : {};
        await cdp.send("Input.dispatchKeyEvent", { type, key: k, code: k, windowsVirtualKeyCode: KEYS[k], ...text }, sessionId);
      }
    };
    return {
      extension: id,
      evaluate,
      mouse,
      navigate,
      key,
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

async function firefox(bin, profile, dl, page) {
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
    await bidi.send("session.new", { capabilities: {} });
    const xpi = path.join(root, "target", "extension", "mdm-firefox.xpi");
    const { extension } = await bidi.send("webExtension.install", { extensionData: { type: "archivePath", path: xpi } });
    await sleep(1500);
    const { contexts } = await bidi.send("browsingContext.getTree", {});
    const context = contexts[0].context;
    await bidi.send("browsingContext.navigate", { context, url: page, wait: "complete" });
    // BiDi returns objects in its own form: the value goes through JSON instead.
    const evaluate = async (expression) => {
      const json = `(async () => JSON.stringify(await (${expression})))()`;
      const r = await bidi.send("script.evaluate", { expression: json, target: { context }, awaitPromise: true, userActivation: true });
      if (r.type === "exception") throw new Error(`evaluate: ${r.exceptionDetails.text} ${expression}`);
      return r.result.value === undefined ? undefined : JSON.parse(r.result.value);
    };
    const mouse = async (x, y, act = "move") => {
      const actions = [{ type: "pointerMove", x: Math.round(x), y: Math.round(y) }];
      if (act === "click" || act === "down") actions.push({ type: "pointerDown", button: 0 });
      if (act === "click" || act === "up") actions.push({ type: "pointerUp", button: 0 });
      await bidi.send("input.performActions", { context, actions: [{ type: "pointer", id: "mouse", actions }] });
    };
    const navigate = (url) => bidi.send("browsingContext.navigate", { context, url, wait: "complete" });
    const KEYS = { ArrowDown: "", Enter: "" };
    const key = (k) =>
      bidi.send("input.performActions", {
        context,
        actions: [{ type: "key", id: "keyboard", actions: [{ type: "keyDown", value: KEYS[k] }, { type: "keyUp", value: KEYS[k] }] }],
      });
    return {
      extension,
      evaluate,
      mouse,
      navigate,
      key,
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

// ---- the download button over videos ----------------------------------------------------------

// The test instance's window titles (Windows), closing the one titled `close`. Collected in C#:
// what a PowerShell callback prints is lost.
function appWindows(pid, close = "") {
  const script = `
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class W {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static List<string> Of(uint pid, string close) {
    var r = new List<string>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
        var t = sb.ToString(); if (t.Length > 0) { r.Add(t); if (t == close) PostMessage(h, 0x10, IntPtr.Zero, IntPtr.Zero); } }
      return true; }, IntPtr.Zero);
    return r;
  }
}
"@
[W]::Of(${pid}, '${close.replace(/'/g, "''")}')`;
  return spawnSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).stdout.split(/\r?\n/).filter(Boolean);
}

const near = (a, b, what) => {
  if (Math.abs(a - b) > 6) throw new Error(`${what}: ${Math.round(a)} instead of ${Math.round(b)}`);
};

// Point at a video, see the button at its top right, click it (the app's video window opens),
// drag it to the bottom left (it stays there after a reload); `never`: its menu's "Never on
// this site" by keyboard, then it no longer shows and the app lists the site.
async function videoButton(session, pid, data, never) {
  const page = `http://127.0.0.1:${PORT}/page?video=${encodeURIComponent("/gen/1M/clip.mp4")}`;
  const title = "Download video from a page";
  const rect = (sel) =>
    session.evaluate(`(() => { const e = document.querySelector(${JSON.stringify(sel)});
      if (!e || (e.localName === "mdm-video-button" && e.style.display !== "block")) return null;
      const r = e.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom }; })()`);
  const hover = async (v) => {
    for (let i = 0; i < 40; i++) {
      await session.mouse((v.l + v.r) / 2 + (i % 2), (v.t + v.b) / 2);
      const b = await rect("mdm-video-button");
      if (b) return b;
      await sleep(200);
    }
    return null;
  };
  await session.navigate(page);
  const v = await rect("#video");
  let b = await hover(v);
  if (!b) throw new Error("no button over the video");
  near(b.r, v.r - 10, "button's right edge");
  near(b.t, v.t + 10, "button's top");
  // Click its main part: the app's Download video window opens for the page.
  appWindows(pid, title);
  await session.mouse(b.l + 14, (b.t + b.b) / 2, "click");
  await until("the Download video window", 15000, async () => appWindows(pid).includes(title));
  appWindows(pid, title);
  // Drag it to the bottom left: it snaps to that corner.
  await session.mouse(b.l + 14, (b.t + b.b) / 2, "down");
  await session.mouse(b.l + 30, (b.t + b.b) / 2 + 20);
  await session.mouse(v.l + 40, v.b - 30);
  await session.mouse(v.l + 40, v.b - 30, "up");
  await sleep(300);
  b = await rect("mdm-video-button");
  near(b.l, v.l + 10, "dragged: left edge");
  near(b.b, v.b - 10, "dragged: bottom");
  if (appWindows(pid).includes(title)) throw new Error("a drag opened the video window");
  // Remembered for the site.
  await session.mouse(5, 5);
  await session.navigate(page);
  b = await hover(v);
  if (!b) throw new Error("no button after a reload");
  near(b.l, v.l + 10, "after a reload: left edge");
  near(b.b, v.b - 10, "after a reload: bottom");
  if (!never) return;
  // Its menu by keyboard: open, down to "Never on 127.0.0.1", Enter.
  await session.mouse(b.r - 12, (b.t + b.b) / 2, "click");
  await sleep(200);
  await session.key("ArrowDown");
  await session.key("ArrowDown");
  await session.key("Enter");
  await sleep(500);
  if (await rect("mdm-video-button")) throw new Error("Never on the site: the button is still shown after the menu command");
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
  if (await hover(v)) throw new Error("the button still shows on a site it should never show on");
}

async function run() {
  const wanted = process.argv.slice(2);
  const names = (wanted.length ? wanted : Object.keys(BROWSERS)).filter((n) => {
    if (!BROWSERS[n]) throw new Error(`unknown browser ${n}`);
    return wanted.length || BROWSERS[n][1].some((p) => fs.existsSync(p));
  });
  // IDM's browser integration takes browsers' downloads at the network level (the browser gets
  // "204 No Content"): it would take the test's files and show its own windows. E2E_IDM_OK=1 when
  // IDM leaves 127.0.0.1 alone (an exception for it in its options).
  if (process.platform === "win32" && process.env.E2E_IDM_OK !== "1") {
    const idm = spawnSync("tasklist", ["/FI", "IMAGENAME eq IDMan.exe", "/NH"], { encoding: "utf8" }).stdout;
    if (/IDMan\.exe/i.test(idm)) throw new Error("Internet Download Manager is running: quit it first (it takes the browsers' downloads), or set E2E_IDM_OK=1 if it leaves 127.0.0.1 alone");
  }
  for (const p of [exe("misterdownloadmanager"), exe("misterdownloadmanager-nmhost"), exe("labserver")]) {
    if (!fs.existsSync(p)) throw new Error(`build it first: ${p}`);
  }
  fs.rmSync(work, { recursive: true, force: true });
  const data = path.join(work, "data");
  const downloads = path.join(work, "Downloads");
  fs.mkdirSync(data, { recursive: true });
  // Captured downloads start at once (no Download File Info) into folders under target/e2e.
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
      settings: { language: "en", language_asked: true, show_start_dialog: false, category_dirs: dirs },
    }),
  );
  process.env.MDM_DATA_DIR = data;
  process.env.MDM_OFFSCREEN = "1";

  const server = spawn(exe("labserver"), ["--bind", `127.0.0.1:${PORT}`], { stdio: "ignore", windowsHide: true });
  const app = spawn(exe("misterdownloadmanager"), [], { env: process.env, stdio: "ignore" });
  const nmhost = exe("misterdownloadmanager-nmhost");
  const results = [];
  try {
    await until("the test instance", 20000, () => fs.existsSync(path.join(data, "ipc.json")));
    await until("the lab server", 10000, () => fetch(`http://127.0.0.1:${PORT}/`).then((r) => r.ok, () => false));
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
      const file = `e2e-${name}.zip`;
      const url = `http://127.0.0.1:${PORT}/gen/2M/${file}`;
      const page = `http://127.0.0.1:${PORT}/page?link=${encodeURIComponent(`/gen/2M/${file}`)}`;
      const profile = path.join(work, `profile-${name}`);
      const dl = path.join(work, `browser-${name}`);
      fs.mkdirSync(profile, { recursive: true });
      fs.mkdirSync(dl, { recursive: true });
      let session;
      try {
        if (!bin) throw new Error("not installed");
        const head = await fetch(url, { method: "HEAD" });
        const size = Number(head.headers.get("content-length"));
        session = await (kind === "firefox" ? firefox : chromium)(bin, profile, dl, page);
        // A link clicked on a page, as a user does.
        await session.evaluate("document.getElementById('link').click()");
        const got = path.join(downloads, "Compressed", file);
        await until(`${file} in Mister Download Manager`, 30000, () => fs.existsSync(got) && fs.statSync(got).size === size);
        await sleep(1000);
        const kept = fs.readdirSync(dl).filter((f) => f.includes("e2e-"));
        if (kept.length) throw new Error(`the browser kept its own copy too: ${kept.join(", ")}`);
        results.push([name, "ok", `extension ${session.extension}`]);
        await videoButton(session, app.pid, data, name === names[names.length - 1]);
        results.push([`${name} video`, "ok", "shown, clicked, dragged, kept" + (name === names[names.length - 1] ? ", never on the site" : "")]);
      } catch (e) {
        results.push([name, "FAILED", e.message]);
        if (session && session.trace) console.error(`${name} trace:`, await session.trace());
      } finally {
        if (session) session.close();
      }
    }
  } finally {
    spawnSync(nmhost, ["--unregister"]);
    killTree(app);
    killTree(server);
  }
  for (const [name, status, detail] of results) console.log(`${name.padEnd(8)} ${status.padEnd(7)} ${detail}`);
  if (results.some((r) => r[1] !== "ok")) process.exit(1);
}

// Run when started, not when another script imports its helpers (tools/readme/browser-shots.mjs).
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) run().catch((e) => {
  console.error(e.message);
  process.exit(1);
});

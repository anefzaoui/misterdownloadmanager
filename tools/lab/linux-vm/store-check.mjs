// A store package checked on Linux before it ships: installed in a throwaway profile (Firefox: the
// .xpi as a temporary add-on over WebDriver BiDi, as about:debugging loads one, since the Snap
// Firefox wants signed add-ons for a normal install; Google Chrome: the unpacked folder over the
// DevTools pipe), its version read back, its popup captured (PNG beside the output), and an
// ordinary download handed to MDM: the browser asked for a lab file, MDM's Download File Info
// window must open and the browser must not keep the download itself. MDM and the lab server must
// be running.
//   node store-check.mjs firefox <file.xpi> [lab] [out folder]
//   node store-check.mjs chrome <unpacked folder> [lab] [out folder]
import fs from "node:fs";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";

const [browser, pkg] = [process.argv[2], path.resolve(process.argv[3])];
const LAB = process.argv[4] || "http://127.0.0.1:8097";
const OUT = path.resolve(process.argv[5] || path.join(process.env.HOME, "lab/store-check"));
const ID = "integration@misterdownloadmanager.com";
const UUID = "5d0c3b8e-7a51-4c1e-9f0a-38e2c4a1d7b6";
const NL = String.fromCharCode(10);
const NUL = String.fromCharCode(0);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(what, ms, f) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await f().catch(() => null);
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out: ${what}`);
    await sleep(250);
  }
}
const run = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: "utf8" });
  } catch (e) {
    return e.stdout || "";
  }
};
const windows = () => run("wmctrl", ["-l"]);
fs.mkdirSync(OUT, { recursive: true });
const say = (s) => console.log(s);

// The ordinary download: a lab file of its own name, MDM's window looked for, then closed.
async function ordinary(navigate, downloadsDir) {
  run("wmctrl", ["-c", "Download File Info"]);
  await sleep(1000);
  const name = `store-${browser}-${Date.now() % 100000}.zip`;
  const t0 = Date.now();
  await navigate(`${LAB}/gen/1M/${name}`).catch(() => {});
  const at = await until("MDM's Download File Info", 30000, async () => windows().includes("Download File Info") && ((Date.now() - t0) / 1000).toFixed(1)).catch(() => null);
  await sleep(2000);
  const kept = downloadsDir && fs.existsSync(downloadsDir) ? fs.readdirSync(downloadsDir).filter((f) => f.startsWith(name.replace(".zip", ""))) : [];
  run("wmctrl", ["-c", "Download File Info"]);
  say(`${at ? "OK  " : "FAIL"} ordinary download ${name}: MDM's Download File Info ${at ? `at ${at} s` : "NOT within 30 s"}; ` + `the browser kept ${kept.length ? kept.join(", ") : "nothing"}`);
}

if (browser === "firefox") {
  class Bidi {
    constructor(ws) {
      this.ws = ws;
      this.next = 1;
      this.waiting = new Map();
      ws.addEventListener("message", (e) => {
        const m = JSON.parse(e.data);
        const w = m.id && this.waiting.get(m.id);
        if (!w) return;
        this.waiting.delete(m.id);
        if (m.type === "error") w.reject(new Error(`${w.method}: ${m.error} ${m.message}`));
        else w.resolve(m.result);
      });
    }
    send(method, params = {}) {
      const id = this.next++;
      this.ws.send(JSON.stringify({ id, method, params }));
      return new Promise((resolve, reject) => this.waiting.set(id, { resolve, reject, method }));
    }
  }
  // The Snap Firefox reads profiles under its own ~/snap folder only.
  const profile = path.join(process.env.HOME, "snap/firefox/common/mdm-store-check");
  try {
    execFileSync("pkill", ["-f", "--", `-profile ${profile}`]);
    execFileSync("sleep", ["2"]);
  } catch {}
  fs.rmSync(profile, { recursive: true, force: true });
  const downloads = path.join(profile, "downloads");
  fs.mkdirSync(downloads, { recursive: true });
  const pref = (k, v) => `user_pref(${JSON.stringify(k)}, ${JSON.stringify(v)});`;
  fs.writeFileSync(
    path.join(profile, "user.js"),
    [
      pref("browser.shell.checkDefaultBrowser", false),
      pref("datareporting.policy.dataSubmissionEnabled", false),
      pref("browser.aboutwelcome.enabled", false),
      pref("extensions.webextensions.uuids", JSON.stringify({ [ID]: UUID })),
      pref("browser.download.dir", downloads),
      pref("browser.download.folderList", 2),
      pref("browser.download.useDownloadDir", true),
    ].join(NL),
  );
  const port = 9300 + Math.floor(Math.random() * 500);
  const ff = spawn("/snap/bin/firefox", ["--headless", "-no-remote", "-profile", profile, "--remote-debugging-port", String(port)], {
    env: { ...process.env, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: "1" },
    stdio: "ignore",
  });
  try {
    const ws = await until("Firefox's remote protocol", 60000, () =>
      new Promise((resolve) => {
        const s = new WebSocket(`ws://127.0.0.1:${port}/session`);
        s.addEventListener("open", () => resolve(s));
        s.addEventListener("error", () => resolve(null));
      }),
    );
    const bidi = new Bidi(ws);
    const { capabilities } = await bidi.send("session.new", { capabilities: {} });
    const { extension } = await bidi.send("webExtension.install", { extensionData: { type: "archivePath", path: pkg } });
    const chrome = async (expression) => {
      const { contexts: w } = await bidi.send("browsingContext.getTree", { "moz:scope": "chrome" });
      const r = await bidi.send("script.evaluate", { expression, target: { context: w[0].context }, awaitPromise: true, "moz:scope": "chrome" });
      if (r.type === "exception") throw new Error(`chrome: ${r.exceptionDetails.text}`);
      return r.result.value;
    };
    const info = await chrome(`(async () => { const { AddonManager } = ChromeUtils.importESModule("resource://gre/modules/AddonManager.sys.mjs");
      const a = await AddonManager.getAddonByID(${JSON.stringify(ID)}); return a ? [a.name, a.version, a.isActive, a.temporarilyInstalled, a.signedState].join(" | ") : "not installed"; })()`);
    say(`Firefox ${capabilities.browserVersion} (Snap, headless): installed ${extension}: ${info} (name | version | active | temporary | signed state)`);
    // A tab of its own (the first one can be a privileged page).
    const { context: ctx } = await bidi.send("browsingContext.create", { type: "tab" });
    const evaluate = async (expression) => {
      const r = await bidi.send("script.evaluate", { expression, target: { context: ctx }, awaitPromise: true });
      if (r.type === "exception") throw new Error(`evaluate: ${r.exceptionDetails.text}`);
      return r.result.value;
    };
    const navigate = async (url) => {
      await bidi.send("browsingContext.navigate", { context: ctx, url, wait: "complete" });
    };
    await bidi.send("browsingContext.setViewport", { context: ctx, viewport: { width: 420, height: 640 } });
    await navigate(`moz-extension://${UUID}/popup.html`);
    await sleep(2500);
    const text = await evaluate(`document.body.innerText.replace(/\\s+/g, " ").trim()`);
    // Firefox captures no extension page over BiDi ("privileged scope"): the text only; the
    // headed proxy-optin.mjs shows the popup on the lab's screen.
    say(`popup: "${text.slice(0, 300)}"`);
    await navigate(`${LAB}/`).catch(() => {});
    await ordinary(navigate, downloads);
  } catch (e) {
    say(`ERROR ${e.message}`);
  } finally {
    ff.kill();
  }
} else {
  class Pipe {
    constructor(proc) {
      this.out = proc.stdio[3];
      this.next = 1;
      this.waiting = new Map();
      let buf = "";
      proc.stdio[4].on("data", (d) => {
        buf += d.toString("utf8");
        let i;
        while ((i = buf.indexOf(NUL)) >= 0) {
          const m = JSON.parse(buf.slice(0, i));
          buf = buf.slice(i + 1);
          const w = m.id && this.waiting.get(m.id);
          if (!w) continue;
          this.waiting.delete(m.id);
          if (m.error) w.reject(new Error(`${w.method}: ${m.error.message}`));
          else w.resolve(m.result);
        }
      });
    }
    send(method, params = {}, sessionId) {
      const id = this.next++;
      this.out.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + NUL);
      return new Promise((resolve, reject) => {
        this.waiting.set(id, { resolve, reject, method });
        setTimeout(() => this.waiting.has(id) && (this.waiting.delete(id), reject(new Error(`${method}: no answer in 60 s`))), 60000);
      });
    }
  }
  const profile = path.join(process.env.HOME, "lab/chrome-store-check");
  fs.rmSync(profile, { recursive: true, force: true });
  const downloads = path.join(profile, "downloads");
  fs.mkdirSync(downloads, { recursive: true });
  // With --user-data-dir, Chrome reads host manifests from <user-data-dir>/NativeMessagingHosts,
  // then from everyone's (/etc/opt/chrome/native-messaging-hosts): the user's is copied there when
  // there is one (with the .deb or .rpm installed there is none, #266, and everyone's is read).
  const hosts = path.join(profile, "NativeMessagingHosts");
  fs.mkdirSync(hosts, { recursive: true });
  const mine = path.join(process.env.HOME, ".config/google-chrome/NativeMessagingHosts/com.misterdownloadmanager.host.json");
  if (fs.existsSync(mine)) fs.copyFileSync(mine, path.join(hosts, "com.misterdownloadmanager.host.json"));
  say(`host manifest: ${fs.existsSync(mine) ? mine : "/etc/opt/chrome/native-messaging-hosts (none in the home folder)"}`);
  const proc = spawn(
    "/usr/bin/google-chrome",
    ["--headless", "--remote-debugging-pipe", "--enable-unsafe-extension-debugging", "--password-store=basic", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "about:blank"],
    { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
  );
  try {
    const cdp = new Pipe(proc);
    const { product } = await cdp.send("Browser.getVersion");
    const { id } = await cdp.send("Extensions.loadUnpacked", { path: pkg });
    const sw = await until("the extension's service worker", 15000, async () =>
      (await cdp.send("Target.getTargets")).targetInfos.find((t) => t.type === "service_worker" && t.url.includes(id)),
    );
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, sessionId);
    // DOWNLOAD_BEHAVIOR=allow: the browser's own downloads go to the profile's folder (CDP's
    // setting); by default Chrome's own behaviour is left alone.
    if (process.env.DOWNLOAD_BEHAVIOR) await cdp.send("Browser.setDownloadBehavior", { behavior: process.env.DOWNLOAD_BEHAVIOR, downloadPath: downloads });
    const evaluate = async (expression) => {
      const r = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
      if (r.exceptionDetails) throw new Error(`evaluate: ${r.exceptionDetails.text}`);
      return r.result.value;
    };
    const navigate = async (url) => {
      await cdp.send("Page.navigate", { url }, sessionId);
      await sleep(1500);
    };
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: 420, height: 640, deviceScaleFactor: 1, mobile: false }, sessionId);
    await navigate(`chrome-extension://${id}/popup.html`);
    await sleep(2500);
    const manifest = await evaluate(`(() => { const m = chrome.runtime.getManifest(); return [m.name, m.version, m.manifest_version].join(" | "); })()`);
    say(`${product} (headless): loaded unpacked ${id} from ${pkg}: ${manifest} (name | version | manifest); service worker ${sw ? "running" : "NOT running"}`);
    const text = await evaluate(`document.body.innerText.replace(/\\s+/g, " ").trim()`);
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, sessionId);
    fs.writeFileSync(path.join(OUT, "chrome-popup.png"), Buffer.from(shot.data, "base64"));
    say(`popup: "${text.slice(0, 300)}"`);
    await navigate(`${LAB}/`);
    await ordinary(navigate, downloads);
  } catch (e) {
    say(`ERROR ${e.message}`);
  } finally {
    proc.kill();
  }
}
process.exit(0);

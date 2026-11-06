// #155 in Google Chrome by the right-click menu, which headless Chrome can't show: a Chrome window
// on the lab's desktop (the DevTools pipe loads the unpacked extension), the lab's HLS player
// page, a real right-click on the player (xdotool), then the menu's own entry by the keyboard
// (Up from the menu's end: Inspect, then the extension's entry above it). Then as streams.mjs:
// MDM's Download video window, the address yt-dlp was given, what the lab saw.
//   node chrome-menu.mjs <unpacked extension folder> [lab] [kind:manifest]   e.g. hls:/hls/master.m3u8
// MENU_KEYS: the keys after the right-click (default "Up Up Return"); MENU_SHOT: a file to write
// "x y" of the click to and wait 4 s on, so a screenshot of the open menu can be taken from outside.
import fs from "node:fs";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";

const ext = path.resolve(process.argv[2]);
const LAB = process.argv[3] || "http://127.0.0.1:8097";
const [kind, manifest] = (process.argv[4] || "hls:/hls/master.m3u8").split(":");
const keys = (process.env.MENU_KEYS || "Up Up Return").split(" ");
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
const ytdlpPids = () => run("pgrep", ["-f", "^python3 /usr/local/bin/yt-dlp"]).split(NL).filter(Boolean);
const argOf = (pid) => {
  try {
    return fs.readFileSync(`/proc/${pid}/cmdline`, "utf8").split(NUL).filter(Boolean).pop();
  } catch {
    return null;
  }
};
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
const profile = path.join(process.env.HOME, "lab/chrome-menu");
fs.rmSync(profile, { recursive: true, force: true });
const hosts = path.join(profile, "NativeMessagingHosts");
fs.mkdirSync(hosts, { recursive: true });
fs.copyFileSync(path.join(process.env.HOME, ".config/google-chrome/NativeMessagingHosts/com.misterdownloadmanager.host.json"), path.join(hosts, "com.misterdownloadmanager.host.json"));
const proc = spawn(
  "/usr/bin/google-chrome",
  ["--remote-debugging-pipe", "--enable-unsafe-extension-debugging", "--password-store=basic", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--window-position=40,60", "--window-size=1100,800", "about:blank"],
  { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
);
const say = (s) => console.log(s);
try {
  const cdp = new Pipe(proc);
  const { product } = await cdp.send("Browser.getVersion");
  const { id } = await cdp.send("Extensions.loadUnpacked", { path: ext });
  say(`${product} (a window on the desktop), extension ${id} from ${ext}`);
  const targets = (await cdp.send("Target.getTargets")).targetInfos;
  const tab = targets.find((t) => t.type === "page");
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: tab.targetId, flatten: true });
  await cdp.send("Page.enable", {}, sessionId);
  const evaluate = async (expression) => {
    const r = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
    if (r.exceptionDetails) throw new Error(`evaluate: ${r.exceptionDetails.text}`);
    return r.result.value;
  };
  const cookie = `sid155=chrome-menu-${kind}`;
  await cdp.send("Page.navigate", { url: `${LAB}/cookie?set=${encodeURIComponent(cookie)}` }, sessionId);
  await sleep(1500);
  const page = `${LAB}/page?${kind}=1`;
  await cdp.send("Page.navigate", { url: page }, sessionId);
  const state = await until("the stream player", 20000, () => evaluate("document.body.dataset.stream || ''"));
  say(`${kind}: the page's player: ${state}`);
  // The player's middle on the screen: the window's place plus the toolbar's height.
  const at = await evaluate(`(() => { const r = document.querySelector("#player").getBoundingClientRect();
    return { x: Math.round(window.screenX + r.left + r.width / 2), y: Math.round(window.screenY + (window.outerHeight - window.innerHeight) + r.top + r.height / 2) }; })()`);
  await sleep(1000);
  await fetch(`${LAB}/_log`, { method: "DELETE" });
  const before = new Set(ytdlpPids());
  run("xdotool", ["mousemove", String(at.x), String(at.y), "click", "1"]);
  await sleep(800);
  run("xdotool", ["click", "3"]);
  await sleep(1500);
  if (process.env.MENU_SHOT) {
    fs.writeFileSync(process.env.MENU_SHOT, `${at.x} ${at.y}${NL}`);
    await sleep(4000);
  }
  for (const k of keys) {
    run("xdotool", ["key", k]);
    await sleep(300);
  }
  const t0 = Date.now();
  const secs = () => ((Date.now() - t0) / 1000).toFixed(1);
  let windowAt = null;
  let given = null;
  let pid = null;
  while (Date.now() - t0 < 60000 && !(windowAt && given)) {
    if (!windowAt && windows().includes("Download video from a page")) windowAt = secs();
    if (!given) {
      pid = ytdlpPids().find((x) => !before.has(x));
      if (pid) given = argOf(pid);
    }
    await sleep(200);
  }
  if (pid) await until("yt-dlp's lookup", 90000, async () => !fs.existsSync(`/proc/${pid}`)).catch(() => {});
  const asked = (await (await fetch(`${LAB}/_log`)).json()).filter((e) => e.referer === page || e.path === manifest);
  const seen = asked.map((e) => `${e.path}${e.query ? "?" + e.query : ""} ${e.status}${(e.cookie || "").includes(cookie) ? " +cookie" : ""}${e.referer === page ? " +referrer" : ""}`);
  const ok = windowAt && given && new URL(given).pathname === manifest && asked.some((e) => e.path === manifest);
  say(
    `${ok ? "OK  " : "FAIL"} ${kind} right-click menu (keys ${keys.join(" ")}): MDM's window ${windowAt ? `at ${windowAt} s` : "NOT within 60 s"}; ` +
      `yt-dlp given ${given || "nothing"}; the lab saw ${seen.join(", ") || "nothing"}`,
  );
  run("wmctrl", ["-c", "Download video from a page"]);
} catch (e) {
  say(`ERROR ${e.message}`);
} finally {
  proc.kill();
}
process.exit(0);

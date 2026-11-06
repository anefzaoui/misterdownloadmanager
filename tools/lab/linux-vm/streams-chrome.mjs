// #155/#167 on Linux with Google Chrome (.deb; Ubuntu's Chromium is a Snap, which can't reach MDM):
// headless over the DevTools pipe (as tools/extension/e2e.mjs drives it), the unpacked extension
// loaded, the lab's HLS and DASH player pages; the button over the player and the popup's media
// list (Chrome's own right-click menu can't be driven). Same checks as streams.mjs.
//   node streams-chrome.mjs <unpacked extension folder> [lab] [kinds]   (kinds: as streams.mjs)
import fs from "node:fs";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";

const ext = path.resolve(process.argv[2]);
const LAB = process.argv[3] || "http://127.0.0.1:8097";
const KINDS = (process.argv[4] || "hls:/hls/master.m3u8,dash:/dash/manifest.mpd").split(",").map((k) => k.split(":"));
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
const run = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: "utf8" });
  } catch (e) {
    return e.stdout || "";
  }
};
const windows = () => run("wmctrl", ["-l"]);
const closeVideo = () => run("wmctrl", ["-c", "Download video from a page"]);
const ytdlpPids = () => run("pgrep", ["-f", "^python3 /usr/local/bin/yt-dlp"]).split(NL).filter(Boolean);
const argOf = (pid) => {
  try {
    return fs.readFileSync(`/proc/${pid}/cmdline`, "utf8").split(NUL).filter(Boolean).pop();
  } catch {
    return null;
  }
};

const profile = path.join(process.env.HOME, "lab/chrome-streams");
fs.rmSync(profile, { recursive: true, force: true });
fs.mkdirSync(path.join(profile, "Default"), { recursive: true });
// With --user-data-dir, Chrome on Linux reads host manifests from <user-data-dir>/NativeMessagingHosts
// (not ~/.config/google-chrome/): the one MDM registered, copied there.
const hosts = path.join(profile, "NativeMessagingHosts");
fs.mkdirSync(hosts, { recursive: true });
const registered = path.join(process.env.HOME, ".config/google-chrome/NativeMessagingHosts/com.misterdownloadmanager.host.json");
fs.copyFileSync(registered, path.join(hosts, "com.misterdownloadmanager.host.json"));
const proc = spawn(
  "/usr/bin/google-chrome",
  ["--headless", "--remote-debugging-pipe", "--enable-unsafe-extension-debugging", "--password-store=basic", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--window-size=1280,900", "about:blank"],
  { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
);
const out = [];
const say = (s) => {
  console.log(s);
  out.push(s);
};
try {
  const cdp = new Pipe(proc);
  const { product } = await cdp.send("Browser.getVersion");
  const { id } = await cdp.send("Extensions.loadUnpacked", { path: ext });
  say(`${product} (headless), extension ${id} from ${ext}`);
  await until("the extension's service worker", 15000, async () =>
    (await cdp.send("Target.getTargets")).targetInfos.find((t) => t.type === "service_worker" && t.url.includes(id)),
  );
  say("service worker running");
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, sessionId);
  const evaluate = async (expression, s = sessionId) => {
    const r = await cdp.send("Runtime.evaluate", { expression, userGesture: true, awaitPromise: true, returnByValue: true }, s);
    if (r.exceptionDetails) throw new Error(`evaluate: ${r.exceptionDetails.text}`);
    return r.result.value;
  };
  const navigate = async (url) => {
    await cdp.send("Page.navigate", { url }, sessionId);
    await until(`the page ${url}`, 15000, () => evaluate("[location.href, document.readyState]").then(([at, st]) => st === "complete" && at === new URL(url).href));
  };
  const mouse = async (x, y, act = "move") => {
    const at = { x, y, button: "left", buttons: 0, modifiers: 0 };
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", ...at }, sessionId);
    if (act === "click") {
      await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", ...at, buttons: 1, clickCount: 1 }, sessionId);
      await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...at, buttons: 0, clickCount: 1 }, sessionId);
    }
  };
  const rect = (sel) =>
    evaluate(`(() => { const e = document.querySelector(${JSON.stringify(sel)});
      if (!e || (e.localName === "mdm-video-button" && e.style.display !== "block")) return null;
      const r = e.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom }; })()`);
  const labLog = async () => await (await fetch(`${LAB}/_log`)).json();
  const check = async (what, page, manifest, cookie, trigger) => {
    closeVideo();
    await until("earlier yt-dlp lookups to end", 90000, async () => ytdlpPids().length === 0).catch(() => {});
    await sleep(1000);
    await fetch(`${LAB}/_log`, { method: "DELETE" });
    const before = new Set(ytdlpPids());
    const t0 = Date.now();
    const r = await trigger();
    const secs = () => ((Date.now() - t0) / 1000).toFixed(1);
    let windowAt = null;
    let given = null;
    let pid = null;
    const end = Date.now() + 60000;
    while (Date.now() < end && !(windowAt && given)) {
      if (!windowAt && windows().includes("Download video from a page")) windowAt = secs();
      if (!given) {
        pid = ytdlpPids().find((x) => !before.has(x));
        if (pid) given = argOf(pid);
      }
      await sleep(200);
    }
    if (pid) await until("yt-dlp's lookup", 90000, async () => !fs.existsSync(`/proc/${pid}`)).catch(() => {});
    const done = secs();
    const asked = (await labLog()).filter((e) => e.referer === page || e.path === "/page" || e.path === manifest);
    const seen = asked.map((e) => `${e.path}${e.query ? "?" + e.query : ""} ${e.status}${(e.cookie || "").includes(cookie) ? " +cookie" : ""}${e.referer === page ? " +referrer" : ""}`);
    const ok = windowAt && given && new URL(given).pathname === manifest && asked.some((e) => e.path === manifest) && !asked.some((e) => e.path === "/page");
    say(
      `${ok ? "OK  " : "FAIL"} ${what}${r ? ` (${r})` : ""}: MDM's window ${windowAt ? `at ${windowAt} s` : "NOT within 60 s"}; ` +
        `yt-dlp given ${given || "nothing"}, done at ${done} s; the lab saw ${seen.join(", ") || "nothing"}`,
    );
  };
  for (const [kind, manifest] of KINDS) {
    const cookie = `sid155=chrome-${kind}`;
    await navigate(`${LAB}/cookie?set=${encodeURIComponent(cookie)}`);
    const page = `${LAB}/page?${kind}=1`;
    await navigate(page);
    const state = await until("the stream player", 20000, () => evaluate("document.body.dataset.stream || ''"));
    say(`${kind}: the page's player: ${state}`);
    const v = await rect("#player");
    await check(`${kind} button`, page, manifest, cookie, async () => {
      let b = null;
      for (let i = 0; i < 40 && !b; i++) {
        await mouse((v.l + v.r) / 2 + (i % 2), (v.t + v.b) / 2);
        b = await rect("mdm-video-button");
        if (!b) await sleep(200);
      }
      if (!b) return "no button over the player";
      await mouse(b.l + 14, (b.t + b.b) / 2, "click");
      return "";
    });
    await check(`${kind} media list`, page, manifest, cookie, async () => {
      const url = `chrome-extension://${id}/popup.html?page=${encodeURIComponent(page)}`;
      const { targetId: tid } = await cdp.send("Target.createTarget", { url });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId: tid, flatten: true });
      const name = manifest.split("/").pop();
      const rows = await until("the media list", 15000, () =>
        evaluate(`[...document.querySelectorAll("#media-list li")].map((li) => li.querySelector(".name").textContent + (li.querySelector("button") ? " [Download]" : ""))`, s).then((r) => r && r.length && r),
      );
      const clicked = await evaluate(
        `(() => { const li = [...document.querySelectorAll("#media-list li")].find((x) => x.querySelector(".name").textContent === ${JSON.stringify(name)}); if (!li || !li.querySelector("button")) return false; li.querySelector("button").click(); return true; })()`,
        s,
      );
      setTimeout(() => cdp.send("Target.closeTarget", { targetId: tid }).catch(() => {}), 20000);
      return `list: ${rows.join("; ")}${clicked ? "" : "; no Download for " + name}`;
    });
  }
} catch (e) {
  say(`ERROR ${e.message}`);
} finally {
  proc.kill();
  fs.writeFileSync(path.join(process.env.HOME, "lab/streams-chrome.txt"), out.join(NL) + NL);
}
process.exit(0);

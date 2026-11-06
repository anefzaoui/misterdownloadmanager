// #155/#167 on Linux: the Snap Firefox (headless, over WebDriver BiDi, as tools/extension/e2e.mjs
// drives it) with an add-on, on the lab server's HLS and DASH player pages. Each hand-over path
// (the button over the player, the right-click menu, the popup's media list) is triggered, then:
// when the host started, when MDM's Download video window opened, the address yt-dlp was given,
// and what yt-dlp asked the lab for once it finished. MDM must be running and not minimized.
//   node streams.mjs <xpi> [lab] [kinds]   lab: http://127.0.0.1:8097
//   kinds: page flag and manifest path, default "hls:/hls/master.m3u8,dash:/dash/manifest.mpd";
//   #171's streams without endings: "hls2:/hls2/playlist,dash2:/dash2/manifest"
import fs from "node:fs";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";

const xpi = path.resolve(process.argv[2]);
const LAB = process.argv[3] || "http://127.0.0.1:8097";
const KINDS = (process.argv[4] || "hls:/hls/master.m3u8,dash:/dash/manifest.mpd").split(",").map((k) => k.split(":"));
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
const hostLog = path.join(process.env.HOME, "lab/hostwrap.log");
const hostLaunches = () => {
  try {
    return fs.readFileSync(hostLog, "utf8").split(NL).filter((l) => l.startsWith("== ") && l.includes("args")).length;
  } catch {
    return 0;
  }
};

// A profile the Snap can read (not /tmp: a Snap has its own).
const profile = path.join(process.env.HOME, "snap/firefox/common/mdm-streams");
fs.rmSync(profile, { recursive: true, force: true });
fs.mkdirSync(profile, { recursive: true });
const pref = (k, v) => `user_pref(${JSON.stringify(k)}, ${JSON.stringify(v)});`;
fs.writeFileSync(
  path.join(profile, "user.js"),
  [
    pref("browser.shell.checkDefaultBrowser", false),
    pref("datareporting.policy.dataSubmissionEnabled", false),
    pref("browser.aboutwelcome.enabled", false),
    pref("extensions.originControls.grantByDefault", true),
    pref("extensions.webextensions.uuids", JSON.stringify({ [ID]: UUID })),
  ].join(NL),
);
const port = 9300 + Math.floor(Math.random() * 500);
const ff = spawn("/snap/bin/firefox", ["-headless", "-no-remote", "-profile", profile, "--remote-debugging-port", String(port), "--width=1280", "--height=900"], {
  env: { ...process.env, MOZ_HEADLESS: "1", MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: "1" },
  stdio: "ignore",
});
const out = [];
const say = (s) => {
  console.log(s);
  out.push(s);
};
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
  say(`Firefox ${capabilities.browserVersion} (Snap, headless), add-on ${path.basename(xpi)}`);
  await bidi.send("webExtension.install", { extensionData: { type: "archivePath", path: xpi } });
  const { contexts } = await bidi.send("browsingContext.getTree", {});
  const context = contexts[0].context;
  const evaluate = async (expression, c = context) => {
    const r = await bidi.send("script.evaluate", {
      expression: `(async () => JSON.stringify(await (${expression})))()`,
      target: { context: c },
      awaitPromise: true,
      userActivation: true,
    });
    if (r.type === "exception") throw new Error(`evaluate: ${r.exceptionDetails.text}`);
    return r.result.value === undefined ? undefined : JSON.parse(r.result.value);
  };
  const navigate = (url) => bidi.send("browsingContext.navigate", { context, url, wait: "complete" });
  const mouse = (x, y, act = "move", button = 0) => {
    const a = [{ type: "pointerMove", x: Math.round(x), y: Math.round(y) }];
    if (act === "click") a.push({ type: "pointerDown", button }, { type: "pointerUp", button });
    return bidi.send("input.performActions", { context, actions: [{ type: "pointer", id: "mouse", actions: a }] });
  };
  const rect = (sel) =>
    evaluate(`(() => { const e = document.querySelector(${JSON.stringify(sel)});
      if (!e || (e.localName === "mdm-video-button" && e.style.display !== "block")) return null;
      const r = e.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom }; })()`);
  const chrome = async (expression) => {
    const { contexts: w } = await bidi.send("browsingContext.getTree", { "moz:scope": "chrome" });
    const r = await bidi.send("script.evaluate", { expression, target: { context: w[0].context }, awaitPromise: true, "moz:scope": "chrome" });
    if (r.type === "exception") throw new Error(`chrome: ${r.exceptionDetails.text}`);
    return r.result.value;
  };
  const labLog = async () => await (await fetch(`${LAB}/_log`)).json();

  // One path: earlier lookups ended, the log cleared, then the trigger; what followed.
  const check = async (what, page, manifest, cookie, trigger) => {
    closeVideo();
    await until("earlier yt-dlp lookups to end", 90000, async () => ytdlpPids().length === 0).catch(() => {});
    await sleep(1000);
    await fetch(`${LAB}/_log`, { method: "DELETE" });
    const before = new Set(ytdlpPids());
    const launches = hostLaunches();
    const t0 = Date.now();
    const r = await trigger();
    const secs = () => ((Date.now() - t0) / 1000).toFixed(1);
    let hostAt = null;
    let windowAt = null;
    let given = null;
    let pid = null;
    const end = Date.now() + 60000;
    while (Date.now() < end && !(windowAt && given)) {
      if (!hostAt && hostLaunches() > launches) hostAt = secs();
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
      `${ok ? "OK  " : "FAIL"} ${what}${r ? ` (${r})` : ""}: host started ${hostAt ? `at ${hostAt} s` : "?"}, ` +
        `MDM's window ${windowAt ? `at ${windowAt} s` : "NOT within 60 s"}; yt-dlp given ${given || "nothing"}, done at ${done} s; ` +
        `the lab saw ${seen.join(", ") || "nothing"}`,
    );
  };

  for (const [kind, manifest] of KINDS) {
    const cookie = `sid155=linux-${kind}`;
    await navigate(`${LAB}/cookie?set=${encodeURIComponent(cookie)}`);
    const page = `${LAB}/page?${kind}=1`;
    await fetch(`${LAB}/_log`, { method: "DELETE" });
    await navigate(page);
    const state = await until("the stream player", 20000, () => evaluate("document.body.dataset.stream || ''"));
    await sleep(3000);
    // The master itself (the first address of that path), not its variants (same path, more query).
    const played = (await labLog()).filter((e) => e.path === manifest);
    const master = played.length ? played[0].path + (played[0].query ? "?" + played[0].query : "") : manifest;
    const times = played.filter((e) => e.path + (e.query ? "?" + e.query : "") === master).length;
    say(`${kind}: the page's player: ${state}; while it played, before anything was asked: ${master} ${times}×, ${played.length - times} variant request(s)`);
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
    await check(`${kind} right-click menu`, page, manifest, cookie, async () => {
      await mouse((v.l + v.r) / 2, (v.t + v.b) / 2, "click", 2);
      return chrome(`(async () => {
        const menu = document.getElementById("contentAreaContextMenu");
        const shown = () => [...menu.querySelectorAll("menuitem")].filter((m) => !m.hidden);
        const L = "Download with Mister Download Manager";
        for (let i = 0; i < 50 && !shown().some((m) => m.label === L); i++) await new Promise((r) => setTimeout(r, 100));
        const item = shown().find((m) => m.label === L);
        if (!item) { const all = shown().map((m) => m.label).join(" | "); menu.hidePopup(); return "not there: " + all; }
        const at = gContextMenu ? { onVideo: gContextMenu.onVideo, mediaURL: gContextMenu.mediaURL } : null;
        menu.activateItem(item);
        return "chosen " + JSON.stringify(at);
      })()`);
    });
    await check(`${kind} media list`, page, manifest, cookie, async () => {
      const url = `moz-extension://${UUID}/popup.html?page=${encodeURIComponent(page)}`;
      await chrome(`gBrowser.addTab(${JSON.stringify(url)}, { triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal() }) && "opened"`);
      const c = await until("the popup's tab", 10000, async () => ((await bidi.send("browsingContext.getTree", {})).contexts.find((x) => x.url === url) || {}).context);
      const name = manifest.split("/").pop();
      const rows = await until("the media list", 15000, () =>
        evaluate(`[...document.querySelectorAll("#media-list li")].map((li) => li.querySelector(".name").textContent + (li.querySelector("button") ? " [Download]" : ""))`, c).then((r) => r && r.length && r),
      );
      const clicked = await evaluate(
        `(() => { const li = [...document.querySelectorAll("#media-list li")].find((x) => x.querySelector(".name").textContent === ${JSON.stringify(name)}); if (!li || !li.querySelector("button")) return false; li.querySelector("button").click(); return true; })()`,
        c,
      );
      setTimeout(() => bidi.send("browsingContext.close", { context: c }).catch(() => {}), 20000);
      return `list: ${rows.join("; ")}${clicked ? "" : "; no Download for " + name}`;
    });
  }
  // Every kind on one page: the popup's media list shows exactly those streams.
  if (KINDS.length > 1) {
    const page = `${LAB}/page?${KINDS.map(([k]) => `${k}=1`).join("&")}`;
    await navigate(page);
    await until("the stream players", 20000, () => evaluate("document.body.dataset.stream || ''"));
    await sleep(3000);
    const url = `moz-extension://${UUID}/popup.html?page=${encodeURIComponent(page)}`;
    await chrome(`gBrowser.addTab(${JSON.stringify(url)}, { triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal() }) && "opened"`);
    const c = await until("the popup's tab", 10000, async () => ((await bidi.send("browsingContext.getTree", {})).contexts.find((x) => x.url === url) || {}).context);
    const rowsNow = () => evaluate(`[...document.querySelectorAll("#media-list li")].map((li) => [li.querySelector(".tag").textContent, li.querySelector(".name").textContent, li.querySelector(".meta").textContent].join(" "))`, c);
    await until("the media list", 20000, () => rowsNow().then((r) => r && r.length >= KINDS.length && r)).catch(() => {});
    await sleep(2000);
    const rows = await rowsNow();
    say(`all on one page (${page.replace(LAB, "")}): the media list: ${rows.join(" | ") || "empty"}`);
  }
} catch (e) {
  say(`ERROR ${e.message}`);
} finally {
  ff.kill();
  fs.writeFileSync(path.join(process.env.HOME, "lab/streams.txt"), out.join(NL) + NL);
}
process.exit(0);

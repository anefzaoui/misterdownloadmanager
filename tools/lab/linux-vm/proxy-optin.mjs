// #42/#191 on Linux: the popup's proxy switch in the Snap Firefox (headless, over WebDriver BiDi).
// Off at first; turning it on brings Firefox's permission prompt, answered through the browser's own
// window (Allow, then Don't allow); the switch and the "proxy" permission are read back after each
// step, and after turning it off again.
//   node proxy-optin.mjs <xpi>
import fs from "node:fs";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";

const xpi = path.resolve(process.argv[2]);
const ID = "integration@misterdownloadmanager.com";
const UUID = "5d0c3b8e-7a51-4c1e-9f0a-38e2c4a1d7b6";
const NL = String.fromCharCode(10);
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

const profile = path.join(process.env.HOME, "snap/firefox/common/mdm-proxy-optin");
// A Firefox left from an earlier run would keep the profile locked.
try {
  execFileSync("pkill", ["-f", "--", `-profile ${profile}`]);
  execFileSync("sleep", ["2"]);
} catch {}
fs.rmSync(profile, { recursive: true, force: true });
fs.mkdirSync(profile, { recursive: true });
const pref = (k, v) => `user_pref(${JSON.stringify(k)}, ${JSON.stringify(v)});`;
fs.writeFileSync(
  path.join(profile, "user.js"),
  [
    pref("browser.shell.checkDefaultBrowser", false),
    pref("datareporting.policy.dataSubmissionEnabled", false),
    pref("browser.aboutwelcome.enabled", false),
    pref("extensions.webextensions.uuids", JSON.stringify({ [ID]: UUID })),
  ].join(NL),
);
const port = 9300 + Math.floor(Math.random() * 500);
// With a window on the lab's desktop: the switch takes a real click (xdotool), which nothing
// else gives an extension's page (permissions.request wants a user input handler).
const ff = spawn("/snap/bin/firefox", ["-no-remote", "-profile", profile, "--remote-debugging-port", String(port), "--width=1100", "--height=760"], {
  env: { ...process.env, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: "1" },
  stdio: "ignore",
});
const say = (s) => console.log(s);
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
  say(`Firefox ${capabilities.browserVersion} (Snap, a window on the desktop), add-on ${xpi}`);
  await bidi.send("webExtension.install", { extensionData: { type: "archivePath", path: xpi } });
  const chrome = async (expression) => {
    const { contexts: w } = await bidi.send("browsingContext.getTree", { "moz:scope": "chrome" });
    const r = await bidi.send("script.evaluate", { expression, target: { context: w[0].context }, awaitPromise: true, "moz:scope": "chrome" });
    if (r.type === "exception") throw new Error(`chrome: ${r.exceptionDetails.text}`);
    return r.result.value;
  };
  const url = `moz-extension://${UUID}/popup.html`;
  await chrome(`gBrowser.selectedTab = gBrowser.addTab(${JSON.stringify(url)}, { triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal() }) && "opened"`);
  const c = await until("the popup's tab", 10000, async () => ((await bidi.send("browsingContext.getTree", {})).contexts.find((x) => x.url === url) || {}).context);
  const ev = async (expression) => {
    const r = await bidi.send("script.evaluate", { expression: `(async () => JSON.stringify(await (${expression})))()`, target: { context: c }, awaitPromise: true, userActivation: true });
    if (r.type === "exception") throw new Error(`evaluate: ${r.exceptionDetails.text}`);
    return r.result.value === undefined ? undefined : JSON.parse(r.result.value);
  };
  const state = async () => {
    const s = await ev(`(async () => ({ shown: !document.getElementById("proxy-row").hidden, on: document.getElementById("proxy").checked,
      label: document.getElementById("proxy-row").textContent.trim(), permission: await browser.permissions.contains({ permissions: ["proxy"] }) }))()`);
    return `switch ${s.shown ? "shown" : "HIDDEN"}, ${s.on ? "on" : "off"} ("${s.label}"); the proxy permission: ${s.permission ? "granted" : "not granted"}`;
  };
  await until("the popup", 10000, () => ev("document.readyState === 'complete'"));
  await sleep(1500);
  say(`at first: ${await state()}`);
  // Turn it on (a click: a user's gesture), then answer Firefox's prompt with `button`
  // ("button": Allow, "secondaryButton": Don't allow); what the prompt said.
  // A real click on the switch: its place on the screen (the window's, the tab's, the switch's).
  const toggle = async () => {
    const b = await ev(`(() => { const r = document.getElementById("proxy").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    const at = JSON.parse(await chrome(`(() => {
      const tab = gBrowser.tabs.find((x) => x.linkedBrowser.currentURI.spec.startsWith(${JSON.stringify(url)}));
      if (tab) gBrowser.selectedTab = tab;
      const r = gBrowser.selectedBrowser.getBoundingClientRect();
      return JSON.stringify({ x: window.mozInnerScreenX + r.left, y: window.mozInnerScreenY + r.top }); })()`));
    const x = Math.round(at.x + b.x), y = Math.round(at.y + b.y);
    await sleep(800);
    // The first click into a window only focuses it: one on the page's empty top left first.
    execFileSync("xdotool", ["mousemove", String(Math.round(at.x + 4)), String(Math.round(at.y + 4)), "click", "1"]);
    await sleep(600);
    execFileSync("xdotool", ["mousemove", String(x), String(y)]);
    await sleep(400);
    execFileSync("xdotool", ["click", "1"]);
    return `clicked at ${x},${y}`;
  };
  const answer = (which) =>
    chrome(`(async () => {
      let n = null;
      for (let i = 0; i < 80 && !n; i++) {
        n = [...document.querySelectorAll("popupnotification")].find((x) => !x.hidden && /permission/i.test(x.id));
        if (!n) await new Promise((r) => setTimeout(r, 100));
      }
      if (!n) return "no prompt";
      const text = (n.getAttribute("label") || "") + " " + [...n.querySelectorAll("li, description")].map((x) => x.textContent).join(" / ");
      n.${which}.click();
      return n.id + ": " + text.replace(/\\s+/g, " ").trim().slice(0, 200);
    })()`);
  if (process.env.PROBE) {
    const asked = ev(`browser.permissions.request({ permissions: ["proxy"] }).then((r) => "resolved " + r, (e) => "rejected " + e)`);
    await sleep(3000);
    say(`notifications: ${await chrome(`JSON.stringify((PopupNotifications._currentNotifications || []).map((n) => n.id)) + " / panel: " + [...document.querySelectorAll("popupnotification")].map((x) => x.id + (x.hidden ? "(hidden)" : "")).join(", ")`)}`);
    say(`request: ${await Promise.race([asked, sleep(5000).then(() => "pending after 8 s")])}`);
  }
  // Each step clicks again when its click was lost (the first one into the page sometimes is).
  const on = async (which) => {
    for (let i = 1; i <= 3; i++) {
      await toggle();
      const prompt = await answer(which);
      if (prompt !== "no prompt") return `${prompt} (click ${i})`;
    }
    return "no prompt after 3 clicks";
  };
  const off = async () => {
    for (let i = 1; i <= 3; i++) {
      await toggle();
      await sleep(1500);
      if (!(await ev(`document.getElementById("proxy").checked`))) return `click ${i}`;
    }
    return "still on after 3 clicks";
  };
  say(`on, the prompt answered Allow: ${await on("button")}`);
  await sleep(1500);
  say(`after Allow: ${await state()}`);
  say(`off: ${await off()}`);
  say(`after turning it off: ${await state()}`);
  say(`on again, the prompt answered Don't allow: ${await on("secondaryButton")}`);
  await sleep(1500);
  say(`after Don't allow: ${await state()}`);
} catch (e) {
  say(`ERROR ${e.message}`);
} finally {
  ff.kill();
}
process.exit(0);

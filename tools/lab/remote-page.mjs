// The remote control's page (#141) in a headless Chromium as a phone sees it: pairs with a code
// MDM shows, adds downloads, pauses and starts one, and screenshots each step in the languages
// and themes asked for. Exits non-zero when a step fails. A test client: it accepts MDM's
// self-signed certificate without asking (its own profile, deleted afterwards).
//
//   node tools/lab/remote-page.mjs --url https://127.0.0.1:18463/ --code 12345678 --out shots \
//     [--add http://127.0.0.1:18090/gen/20M/a.bin ...] [--langs en,de,ar] [--themes light,dark] \
//     [--browser path/to/chrome] [--width 390 --height 844] [--keep-profile dir]
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : def;
};
const many = (name) => args.flatMap((a, i) => (a === `--${name}` ? [args[i + 1]] : []));
const base = opt("url");
const code = opt("code", "");
const out = opt("out", "remote-shots");
const adds = many("add");
const langs = opt("langs", "en").split(",");
const themes = opt("themes", "light").split(",");
const width = Number(opt("width", "390"));
const height = Number(opt("height", "844"));
if (!base) throw new Error("--url is required");
fs.mkdirSync(out, { recursive: true });

const candidates = [
  opt("browser"),
  process.env.CHROME,
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  `${process.env.LOCALAPPDATA}\\Google\\Chrome\\Application\\chrome.exe`,
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/usr/bin/google-chrome",
  "/snap/bin/chromium",
].filter(Boolean);
const browser = candidates.find((p) => fs.existsSync(p));
if (!browser) throw new Error("no Chromium found (--browser)");
const profile = opt("keep-profile") || fs.mkdtempSync(path.join(os.tmpdir(), "mdm-remote-page-"));

const proc = spawn(
  browser,
  [
    "--headless=new",
    "--remote-debugging-pipe",
    `--user-data-dir=${profile}`,
    // A test client of a self-signed certificate on this computer or the lab network.
    "--ignore-certificate-errors",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
);

// Chromium's DevTools protocol over the pipe.
let next = 1;
const waiting = new Map();
const listeners = [];
let buf = "";
proc.stdio[4].on("data", (d) => {
  buf += d.toString("utf8");
  let i;
  while ((i = buf.indexOf("\0")) >= 0) {
    const m = JSON.parse(buf.slice(0, i));
    buf = buf.slice(i + 1);
    const w = m.id && waiting.get(m.id);
    if (w) {
      waiting.delete(m.id);
      m.error ? w.reject(new Error(`${w.method}: ${m.error.message}`)) : w.resolve(m.result);
    } else if (m.method) for (const f of listeners) f(m);
  }
});
const send = (method, params = {}, sessionId) => {
  const id = next++;
  proc.stdio[3].write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + "\0");
  return new Promise((resolve, reject) => waiting.set(id, { resolve, reject, method }));
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failed++;
};

try {
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await send("Target.attachToTarget", { targetId, flatten: true });
  const page = (method, params) => send(method, params, s);
  await page("Page.enable");
  await page("Runtime.enable");
  await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 2, mobile: true });
  await page("Emulation.setTouchEmulationEnabled", { enabled: true });
  const js = async (expression) => {
    const r = await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(`${expression}: ${r.exceptionDetails.text}`);
    return r.result.value;
  };
  const until = async (expression, what, ms = 15000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      if (await js(expression).catch(() => false)) return true;
      await sleep(200);
    }
    check(false, `waited for ${what}`);
    return false;
  };
  const shot = async (name) => {
    const { data } = await page("Page.captureScreenshot", { format: "png" });
    fs.writeFileSync(path.join(out, `${name}.png`), Buffer.from(data, "base64"));
  };
  const look = async (lang, theme) => {
    await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
    const { product } = await send("Browser.getVersion");
    const ua = `Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) ${product.replace("HeadlessChrome", "Chrome")} Mobile Safari/537.36`;
    await page("Emulation.setUserAgentOverride", { userAgent: ua, acceptLanguage: lang, platform: "Android" });
    await page("Emulation.setLocaleOverride", { locale: lang }).catch(() => {});
  };
  const open = async (url) => {
    await page("Page.navigate", { url });
    await until("document.readyState === 'complete' && document.documentElement.lang !== ''", "the page");
    await sleep(600);
  };

  // Pairing, in the first language and theme.
  await look(langs[0], themes[0]);
  await open(code ? `${base}#pair=${code}` : base);
  const paired = await js("!!localStorage.getItem('mdm-token')");
  if (!paired) {
    check(await js("!document.getElementById('pair').hidden"), "the pairing screen shows");
    check((await js("document.getElementById('code').value.replace(/ /g, '')")) === code, "the QR code's code is filled in");
    check((await js("document.getElementById('name').value")) === "Android", "the device is named after the phone");
    await shot(`pair-${langs[0]}-${themes[0]}`);
    await js("document.getElementById('name').value = 'Test phone'; document.querySelector('#pair-form button').click(); true");
    await until("!document.getElementById('main').hidden", "the list after pairing");
    check(await js("!!localStorage.getItem('mdm-token')"), "paired: the token is kept");
  }
  // Adding.
  for (const url of adds) {
    await js(`document.getElementById('url').value = ${JSON.stringify(url)}; document.querySelector('#add button[type=submit]').click(); true`);
    await until(`[...document.querySelectorAll('#list .name')].some((n) => ${JSON.stringify(url)}.endsWith(n.textContent))`, `${url} in the list`);
    await sleep(1200);
  }
  if (adds.length) {
    await until("[...document.querySelectorAll('#list .state')].some((n) => n.classList.contains('s-downloading'))", "a download running");
    // Pause the first running one, then start it again.
    await js("[...document.querySelectorAll('#list .item')].find((li) => li.querySelector('.s-downloading')).querySelector('.toggle').click(); true");
    await until("[...document.querySelectorAll('#list .state')].some((n) => n.classList.contains('s-stopped'))", "paused");
    check(true, "pause from the phone");
    await js("[...document.querySelectorAll('#list .item')].find((li) => li.querySelector('.s-stopped')).querySelector('.toggle').click(); true");
    await until("![...document.querySelectorAll('#list .state')].some((n) => n.classList.contains('s-stopped'))", "started again");
    check(true, "start from the phone");
    await sleep(2500);
  }
  // One waiting in the main queue, then the queue started from the phone.
  const queued = opt("queued");
  if (queued) {
    const name = queued.split("/").pop();
    const row = `[...document.querySelectorAll('#list .item')].find((li) => li.querySelector('.name').textContent === ${JSON.stringify(name)})`;
    await js(`document.getElementById('when').selectedIndex = 1; document.getElementById('url').value = ${JSON.stringify(queued)}; document.querySelector('#add button[type=submit]').click(); true`);
    await until(`${row}?.querySelector('.s-queued')`, `${name} waiting in the main queue`);
    check(true, "added to a queue, not started");
    check(await js("document.querySelector('#queues .chip span').textContent.includes('(1)')"), "the main queue counts it");
    await js("document.querySelector('#queues .chip button').click(); true");
    await until(`${row}?.querySelector('.s-downloading, .s-complete')`, `${name} started by its queue`);
    check(true, "the queue started from the phone");
    await until("document.querySelector('#queues .chip').classList.contains('on')", "the queue shown running");
    await js("document.getElementById('when').selectedIndex = 0; true");
  }
  // Each language and theme: the list, and the remove question.
  for (const lang of langs) {
    for (const theme of themes) {
      await look(lang, theme);
      await open(base);
      await until("!document.getElementById('main').hidden && document.querySelectorAll('#category option').length > 1", "the list");
      const dir = await js("document.documentElement.dir");
      check(dir === (["ar", "he", "fa", "ur", "ug"].includes(lang.split("-")[0]) ? "rtl" : "ltr"), `${lang}: ${dir}`);
      await sleep(1500);
      await shot(`list-${lang}-${theme}`);
      if (await js("document.querySelectorAll('#list .remove').length > 0")) {
        await js("document.querySelector('#list .remove').click(); true");
        await sleep(400);
        await shot(`remove-${lang}-${theme}`);
        await js("document.getElementById('confirm').close('cancel'); true");
      }
    }
  }
} catch (e) {
  check(false, e.message);
} finally {
  proc.kill();
  await new Promise((r) => (proc.exitCode !== null ? r() : proc.once("exit", r)));
  await sleep(500);
  // Chromium's helpers can hold the profile a moment longer: left for the system to clean then.
  if (!opt("keep-profile")) try { fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 }); } catch {}
}
console.log(failed ? `${failed} failed` : "all passed");
process.exit(failed ? 1 : 0);

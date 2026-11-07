// The extension sending downloads to another MDM (#157), in a headless Chromium: its popup's
// "Send downloads to" pairs with a remote MDM (an address and a code), then a download the
// browser starts is caught and goes there. Screenshots the popup in each language and theme.
// A test client: Chromium accepts the MDM's self-signed certificate without asking (in real use,
// the popup's "Open it" lets the person accept it once).
//
//   node tools/lab/extension-remote.mjs --url https://127.0.0.1:18464/ --code 12345678 \
//     --download http://127.0.0.1:18090/gen/5M/dist.iso --out shots [--langs en,de,ar] \
//     [--themes light,dark] [--extension target/extension/chromium] [--browser chrome]
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : def;
};
const base = opt("url");
const code = opt("code");
const download = opt("download");
const out = opt("out", "extension-remote-shots");
const langs = opt("langs", "en").split(",");
const themes = opt("themes", "light").split(",");
const extDir = path.resolve(opt("extension", "target/extension/chromium"));
const browser = [opt("browser"), "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", "/usr/bin/google-chrome"]
  .filter(Boolean)
  .find((p) => fs.existsSync(p));
if (!base || !code || !browser) throw new Error("--url, --code and a Chromium are needed");
fs.mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failed++;
};

// One Chromium in `lang`: the DevTools protocol over a pipe, the extension loaded.
async function open(lang) {
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), "mdm-ext-remote-"));
  // Downloads allowed (headless Chromium refuses them by default), into the profile.
  fs.mkdirSync(path.join(profile, "Default"), { recursive: true });
  fs.writeFileSync(
    path.join(profile, "Default", "Preferences"),
    JSON.stringify({ download: { default_directory: path.join(profile, "dl"), prompt_for_download: false, directory_upgrade: true } }),
  );
  const proc = spawn(
    browser,
    [
      "--headless=new",
      "--remote-debugging-pipe",
      "--enable-unsafe-extension-debugging",
      "--ignore-certificate-errors",
      `--lang=${lang}`,
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"], env: { ...process.env, LANGUAGE: lang } },
  );
  let next = 1;
  const waiting = new Map();
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
      }
    }
  });
  const send = (method, params = {}, sessionId) => {
    const id = next++;
    proc.stdio[3].write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + "\0");
    return new Promise((resolve, reject) => waiting.set(id, { resolve, reject, method }));
  };
  const { id } = await send("Extensions.loadUnpacked", { path: extDir });
  const close = async () => {
    proc.kill();
    await new Promise((r) => (proc.exitCode !== null ? r() : proc.once("exit", r)));
    try {
      fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
    } catch {
      // Left for the system's temporary files.
    }
  };
  // A tab and its session.
  const tab = async (url) => {
    const { targetId } = await send("Target.createTarget", { url });
    const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
    const page = (method, params) => send(method, params, sessionId);
    await page("Runtime.enable");
    const js = async (expression) => {
      const r = await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(`${expression}: ${r.exceptionDetails.text}`);
      return r.result.value;
    };
    return { page, js, targetId };
  };
  return { id, send, tab, close, profile };
}

async function until(js, expression, what, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await js(expression).catch(() => false)) return true;
    await sleep(250);
  }
  check(false, `waited for ${what}`);
  return false;
}

try {
  // Pairing and a caught download, in the first language.
  {
    const b = await open(langs[0]);
    const { js, page } = await b.tab(`chrome-extension://${b.id}/popup.html`);
    await page("Emulation.setDeviceMetricsOverride", { width: 360, height: 640, deviceScaleFactor: 1, mobile: false });
    await until(js, "!!document.getElementById('to-remote')", "the popup", 20000);
    await sleep(500);
    await js("document.getElementById('to-remote').click(); true");
    check(await js("!document.getElementById('remote-form').hidden"), "Another MDM shows the address and code boxes");
    await js(`document.getElementById('remote-url').value = ${JSON.stringify(base)}; document.getElementById('remote-code').value = ${JSON.stringify(code)}; true`);
    await js("document.getElementById('remote-pair').click(); true");
    await until(js, "document.getElementById('remote-form').hidden && document.getElementById('status').classList.contains('ok')", "paired");
    const said = await js("document.getElementById('status-text').textContent");
    check(said.includes(new URL(base).host), `the status names the server: ${said}`);
    if (download) {
      // A tab that downloads (not attached to: it ends as the download starts). The extension
      // hands it to the other MDM, whose list the caller checks.
      const u = new URL(download);
      const t = await b.tab(`${u.origin}/page?link=${encodeURIComponent(u.pathname)}`);
      await until(t.js, "document.readyState === 'complete' && !!document.getElementById('link')", "the lab's page");
      await t.page("Runtime.evaluate", { expression: "document.getElementById('link').click()", userGesture: true });
      await sleep(6000);
      const left = fs.existsSync(path.join(b.profile, "dl")) ? fs.readdirSync(path.join(b.profile, "dl")) : [];
      check(!left.some((f) => f.endsWith(path.basename(new URL(download).pathname))), "the browser didn't keep the file: the other MDM took it");
    }
    await b.close();
  }
  // The popup's look: Send downloads to, each language and theme.
  for (const lang of langs) {
    for (const theme of themes) {
      const b = await open(lang);
      const { js, page } = await b.tab(`chrome-extension://${b.id}/popup.html`);
      await page("Emulation.setDeviceMetricsOverride", { width: 360, height: 640, deviceScaleFactor: 1, mobile: false });
      await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
      await until(js, "!!document.getElementById('to-remote')", "the popup", 20000);
      await sleep(500);
      await js("document.getElementById('to-remote').click(); true");
      await js(`document.getElementById('remote-url').value = ${JSON.stringify(base)}; true`);
      await sleep(300);
      const { data } = await page("Page.captureScreenshot", { format: "png" });
      fs.writeFileSync(path.join(out, `popup-${lang}-${theme}.png`), Buffer.from(data, "base64"));
      await b.close();
    }
  }
} catch (e) {
  check(false, e.message);
}
console.log(failed ? `${failed} failed` : "all passed");
process.exit(failed ? 1 : 0);

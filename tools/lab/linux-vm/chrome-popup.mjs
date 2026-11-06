// The extension's popup in Google Chrome (headless, the DevTools pipe): an expression evaluated in
// it, printed. For checks of what the popup shows there.
//   node chrome-popup.mjs <unpacked extension folder> "<expression>"
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const ext = path.resolve(process.argv[2]);
const expression = process.argv[3];
const NUL = String.fromCharCode(0);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const profile = path.join(process.env.HOME, "lab/chrome-popup");
fs.rmSync(profile, { recursive: true, force: true });
fs.mkdirSync(profile, { recursive: true });
const proc = spawn(
  "/usr/bin/google-chrome",
  ["--headless", "--remote-debugging-pipe", "--enable-unsafe-extension-debugging", "--password-store=basic", `--user-data-dir=${profile}`, "--no-first-run", "about:blank"],
  { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
);
let next = 1;
const waiting = new Map();
let buf = "";
proc.stdio[4].on("data", (d) => {
  buf += d.toString("utf8");
  let i;
  while ((i = buf.indexOf(NUL)) >= 0) {
    const m = JSON.parse(buf.slice(0, i));
    buf = buf.slice(i + 1);
    const w = m.id && waiting.get(m.id);
    if (!w) continue;
    waiting.delete(m.id);
    if (m.error) w.reject(new Error(m.error.message));
    else w.resolve(m.result);
  }
});
const send = (method, params = {}, sessionId) => {
  const id = next++;
  proc.stdio[3].write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + NUL);
  return new Promise((resolve, reject) => {
    waiting.set(id, { resolve, reject });
    setTimeout(() => waiting.has(id) && (waiting.delete(id), reject(new Error(`${method}: no answer in 60 s`))), 60000);
  });
};
try {
  const { product } = await send("Browser.getVersion");
  const { id } = await send("Extensions.loadUnpacked", { path: ext });
  const { targetId } = await send("Target.createTarget", { url: `chrome-extension://${id}/popup.html` });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  await sleep(3000);
  const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
  console.log(`${product}, extension ${id}: ${JSON.stringify(r.result.value)}`);
} catch (e) {
  console.log(`ERROR ${e.message}`);
} finally {
  proc.kill();
}
process.exit(0);

// The README's pictures of the download button over a video, in a real Chrome window (its own
// tabs and address bar): the Blender Foundation's Big Buck Bunny (CC BY 3.0, ATTRIBUTION.md) on
// YouTube, and the same film as a plain video file. Nothing shows on the screen: the window
// opens far off the screens without taking the focus (and keeps painting there), Windows draws
// it into the picture (PrintWindow), and a separate Mister Download Manager runs off the screen
// for the button to ask (MDM_DATA_DIR, MDM_OFFSCREEN). Windows only.
//
//   cargo build --release -p mdm-app -p mdm-nmhost; tools\extension\pack.ps1
//   node tools/readme/browser-shots.mjs        -> docs/images/video-button-*.png
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { Pipe, exe, killProfile, killTree, root, sleep, until } from "../extension/e2e.mjs";

const SHOTS = [
  // Blender's own upload; hl=en for English around it.
  { name: "video-button-youtube", url: "https://www.youtube.com/watch?v=aqz-KE-bpKQ&hl=en&t=236s", at: 236 },
  // Wikimedia Commons' copy of the film, as a plain video file in the browser.
  {
    name: "video-button-page",
    url: "https://upload.wikimedia.org/wikipedia/commons/transcoded/c/c0/Big_Buck_Bunny_4K.webm/Big_Buck_Bunny_4K.webm.720p.vp9.webm",
    at: 230,
  },
];
const W = 1280;
const H = 800;
const OFF = -20000;
const work = path.join(root, "target", "browser-shots");
const chrome = [
  `${process.env.ProgramFiles}\\Google\\Chrome\\Application\\chrome.exe`,
  `${process.env.LOCALAPPDATA}\\Google\\Chrome\\Application\\chrome.exe`,
].find((p) => fs.existsSync(p));

// PrintWindow of the browser's window(s) into `out` (tools/lab/cap-own.ps1).
function capture(pid, out) {
  const r = spawnSync("powershell", ["-NoProfile", "-File", path.join(root, "tools", "lab", "cap-own.ps1"), "-ProcessId", String(pid), "-Out", out], {
    encoding: "utf8",
  });
  if (r.status !== 0) throw new Error(`capture: ${r.stderr || r.stdout}`);
}

// Undoes what main() set up: the host registration, the browser, the app. Run when main ends
// and when the time limit stops it.
let cleanup = () => {};

// A close-up of the button over its video: 16:9, the button near its top right corner, cut from
// the window's picture (the page fills its bottom, centred across).
function crop(whole, at, out) {
  const script = `
import json, sys
from PIL import Image
at = json.loads(sys.argv[2])
im = Image.open(sys.argv[1])
s = at["dpr"]
x0 = (im.width - at["w"] * s) / 2
y0 = im.height - at["h"] * s
w = 560
h = w * 9 / 16
right = min(at["r"] + 26, at["w"])
top = max(at["t"] - 26, 0)
box = [x0 + (right - w) * s, y0 + top * s, x0 + right * s, y0 + (top + h) * s]
im.crop([round(v) for v in box]).save(sys.argv[3], optimize=True)
`;
  const r = spawnSync("python", ["-c", script, whole, JSON.stringify(at), out], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`crop: ${r.stderr}`);
}

async function main() {
  if (!chrome) throw new Error("Google Chrome not found");
  fs.rmSync(work, { recursive: true, force: true });
  const data = path.join(work, "data");
  const profile = path.join(work, "profile");
  fs.mkdirSync(data, { recursive: true });
  fs.mkdirSync(path.join(profile, "Default"), { recursive: true });
  fs.writeFileSync(path.join(data, "downloads.json"), JSON.stringify({ next_id: 1, items: [], version: 2, settings: { language: "en", language_asked: true } }));
  // No translation offers or other bubbles over the page.
  fs.writeFileSync(path.join(profile, "Default", "Preferences"), JSON.stringify({ translate: { enabled: false }, browser: { has_seen_welcome_page: true } }));
  process.env.MDM_DATA_DIR = data;
  process.env.MDM_OFFSCREEN = "1";

  const app = spawn(exe("misterdownloadmanager"), [], { env: process.env, stdio: "ignore" });
  const nmhost = exe("misterdownloadmanager-nmhost");
  let browser = null;
  cleanup = () => {
    cleanup = () => {};
    spawnSync(nmhost, ["--unregister"]);
    killTree(browser);
    killProfile(profile);
    killTree(app);
  };
  try {
    await until("Mister Download Manager", 20000, () => fs.existsSync(path.join(data, "ipc.json")));
    const key = "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.misterdownloadmanager.host";
    if (spawnSync("reg", ["query", key], { stdio: "ignore" }).status === 0) {
      throw new Error("Mister Download Manager's host is already registered (an installed copy): not touching it");
    }
    spawnSync(nmhost, ["--register"]);
    browser = spawn(
      chrome,
      [
        "--remote-debugging-pipe",
        "--enable-unsafe-extension-debugging",
        `--user-data-dir=${profile}`,
        "--no-first-run",
        "--no-default-browser-check",
        "--no-startup-window",
        `--window-position=${OFF},${OFF}`,
        `--window-size=${W},${H}`,
        "--lang=en-US",
        "--hide-crash-restore-bubble",
        // Drawn at twice the size, so the close-up of the button stays sharp.
        "--force-device-scale-factor=2",
        // Off the screens a window counts as hidden: keep it painting.
        "--disable-features=CalculateNativeWinOcclusion",
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
        // Nor suspend its media as a background page's.
        "--disable-background-media-suspend",
        "--autoplay-policy=no-user-gesture-required",
      ],
      { env: process.env, stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"] },
    );
    const cdp = new Pipe(browser);
    await cdp.send("Extensions.loadUnpacked", { path: path.join(root, "target", "extension", "chromium") });
    console.error("extension loaded");
    await sleep(1500);
    for (const shot of SHOTS) {
      // Once more if the page went elsewhere meanwhile (YouTube reloads itself at times).
      for (let attempt = 1; ; attempt++) {
        try {
          await one(cdp, browser, shot);
          break;
        } catch (e) {
          if (attempt >= 2 || !/navigated or closed/.test(e.message)) throw e;
          console.error(`${shot.name}: the page navigated, again`);
        }
      }
    }
  } finally {
    cleanup();
  }
}

// One picture: a new off-screen window on the shot's page, the button over its video.
async function one(cdp, browser, shot) {
  console.error(`${shot.name}: opening ${shot.url}`);
  // A new window, off the screens and not activated.
  const { targetId } = await cdp.send("Target.createTarget", {
    url: shot.url,
    newWindow: true,
    background: true,
    left: OFF,
    top: OFF,
    width: W,
    height: H,
  });
  console.error(`${shot.name}: window made`);
  const { windowId } = await cdp.send("Browser.getWindowForTarget", { targetId });
  await cdp.send("Browser.setWindowBounds", { windowId, bounds: { left: OFF, top: OFF, width: W, height: H, windowState: "normal" } });
  console.error(`${shot.name}: placed off the screens`);
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  // Off the screens and never activated, the page would count as hidden and be frozen:
  // it runs as the active, focused page.
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Page.setWebLifecycleState", { state: "active" }, sessionId).catch(() => {});
  await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true }, sessionId).catch(() => {});
  console.error(`${shot.name}: page active`);
  // A page that doesn't answer within 10 s counts as "not yet".
  const evaluate = async (expression) => {
    const call = cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
    const r = await Promise.race([call, sleep(10000).then(() => ({}))]);
    return r.result && r.result.value;
  };
  // A video file's page never finishes loading: its video is enough.
  await until(`${shot.name}: its video`, 45000, () => evaluate("(document.querySelector('video')?.readyState ?? 0) >= 1"));
  // YouTube: the theatre view, and nothing but the film (no suggested videos of others).
  await evaluate(`(() => {
    if (!location.hostname.endsWith("youtube.com")) return;
    if (!document.querySelector("ytd-watch-flexy[theater]")) document.querySelector(".ytp-size-button")?.click();
    const st = document.createElement("style");
    st.textContent = "#secondary, #related, ytd-watch-next-secondary-results-renderer, #comments { display: none !important } html, body { overflow-x: hidden !important }";
    document.head.appendChild(st);
  })()`);
  // Past any advert, then still on a frame of the film.
  await until(`${shot.name}: the film`, 60000, async () => {
    await evaluate("document.querySelector('.ytp-ad-skip-button, .ytp-skip-ad-button')?.click()");
    return evaluate("!document.querySelector('.ad-showing') && document.querySelector('video').readyState >= 1");
  });
  // The film at `at` seconds, paused there: YouTube through its own player (it manages the
  // stream; the page started at that time), a plain video by seeking.
  await evaluate(`(async () => {
    const v = document.querySelector('video'); v.muted = true;
    const yt = document.getElementById('movie_player');
    if (yt && yt.seekTo) {
      yt.mute(); yt.setPlaybackQualityRange?.('hd720'); yt.seekTo(${shot.at}, true); yt.playVideo();
    } else {
      v.currentTime = ${shot.at};
      await new Promise((r) => v.addEventListener('seeked', r, { once: true }));
      await v.play().catch(() => {});
    }
  })()`);
  await until(`${shot.name}: playing at ${shot.at}s`, 90000, () =>
    evaluate(`(() => { const v = document.querySelector('video'); return !v.paused && v.readyState >= 3 && v.currentTime >= ${shot.at}; })()`),
  );
  // Paused right there: a moment later the film is in motion (blurred frames).
  await evaluate(`(() => { const yt = document.getElementById('movie_player'); if (yt && yt.pauseVideo) yt.pauseVideo(); else document.querySelector('video').pause(); })()`);
  await evaluate(`(async () => { const yt = document.getElementById('movie_player'); if (yt && yt.seekTo) { yt.seekTo(${shot.at}, true); await new Promise((r) => setTimeout(r, 1500)); } })()`);
  console.error(`${shot.name}: on the film`);
  // The frame itself, not a buffering spinner over a dark picture.
  await until(`${shot.name}: the frame`, 60000, () =>
    evaluate(`(() => { const v = document.querySelector('video');
      window.__mdmState = [document.visibilityState, v.readyState, v.networkState, v.seeking, v.paused, v.currentTime.toFixed(1)].join(' ');
      return v.readyState >= 2 && !v.seeking; })()`).then(async (ok) => {
      if (!ok) console.error(`${shot.name}: waiting (visibility readyState network seeking paused time):`, await evaluate("window.__mdmState"));
      return ok;
    }),
  );
  await sleep(3000);
  // The pointer over the video's upper middle: the button shows (and the player's controls).
  const v = await evaluate("(() => { const r = document.querySelector('video').getBoundingClientRect(); return { x: r.left + r.width * 0.55, y: r.top + r.height * 0.35 }; })()");
  const move = (dx) => cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: v.x + dx, y: v.y }, sessionId);
  await until(`${shot.name}: the button`, 15000, async () => {
    await move(Math.random() * 4);
    return evaluate("document.querySelector('mdm-video-button')?.style.display === 'block'");
  });
  await sleep(600); // faded in
  console.error(`${shot.name}: button shown, capturing`);
  // Keep the pointer moving while the window is drawn, so nothing fades.
  const keep = setInterval(() => move(Math.random() * 4).catch(() => {}), 400);
  // Where the button is (page coordinates), and the page's size and scale.
  const at = await evaluate(`(() => { const r = document.querySelector('mdm-video-button').getBoundingClientRect();
    return { l: r.left, t: r.top, r: r.right, b: r.bottom, w: innerWidth, h: innerHeight, dpr: devicePixelRatio }; })()`);
  const whole = path.join(work, `${shot.name}-window.png`);
  try {
    capture(browser.pid, whole);
  } finally {
    clearInterval(keep);
  }
  crop(whole, at, path.join(root, "docs", "images", `${shot.name}.png`));
  console.log(`docs/images/${shot.name}.png`);
  await cdp.send("Target.closeTarget", { targetId });
}

// Its own time limit, so the clean-up (the host registration, the browser) always runs.
let timer;
const limit = new Promise((_, reject) => {
  timer = setTimeout(() => reject(new Error("took over 3 minutes")), 180000);
});
Promise.race([main(), limit])
  .catch((e) => {
    console.error(e.message);
    process.exitCode = 1;
  })
  .finally(() => {
    clearTimeout(timer);
    cleanup();
    process.exit();
  });

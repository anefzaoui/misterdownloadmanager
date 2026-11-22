import { execFile } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createConnection } from "node:net";
import { homedir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

const execute = promisify(execFile);
const MAX_PLAYLIST = 2 * 1024 * 1024;
const MAX_RESOURCES = 10_000;
const SITE = /(^|\.)flixcloud\.cc$/i;
const CDN = /(^|\.)(?:flixcloud\.cc|crimsoncdn\.top)$/i;

function remoteAddress(value) {
  const url = new URL(value);
  if (
    url.protocol !== "https:" ||
    url.username ||
    url.password ||
    (url.port && url.port !== "443") ||
    !CDN.test(url.hostname)
  ) {
    throw new Error("The playlist names an unsupported resource host");
  }
  url.hash = "";
  return url;
}

export function playlistMask(value) {
  if (typeof value !== "string" || !/^[A-Za-z0-9+/]{43}=$/.test(value)) {
    throw new Error("The player did not supply a valid playlist mask");
  }
  const mask = Buffer.from(value, "base64");
  if (mask.length !== 32 || mask.toString("base64") !== value) {
    throw new Error("The player did not supply a valid playlist mask");
  }
  return mask;
}

export function decodePlaylist(bytes, mask) {
  let text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  if (!text.startsWith("#EXTM3U")) {
    text = text.trim();
    if (
      !text ||
      text.length % 4 !== 0 ||
      !/^[A-Za-z0-9+/]*={0,2}$/.test(text)
    ) {
      throw new Error("The server did not return a supported playlist");
    }
    const encoded = Buffer.from(text, "base64");
    if (encoded.toString("base64") !== text) {
      throw new Error("The server returned invalid playlist encoding");
    }
    const decoded = encoded.map((byte, i) => byte ^ mask[i % mask.length]);
    text = new TextDecoder("utf-8", { fatal: true }).decode(decoded);
  }
  if (!text.startsWith("#EXTM3U")) {
    throw new Error("The decoded response is not an HLS playlist");
  }
  if (
    /#EXT-X-(?:SESSION-)?KEY:[^\r\n]*(?:METHOD=SAMPLE-AES|KEYFORMAT="(?!identity"))/i.test(text) ||
    /skd:\/\/|com\.apple\.streamingkeydelivery|com\.microsoft\.playready/i.test(text)
  ) {
    throw new Error("DRM-protected streams are not supported");
  }
  return text;
}

async function boundedBody(response) {
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > MAX_PLAYLIST) {
      throw new Error("The playlist exceeds the 2 MB limit");
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

export async function createBridge({
  url,
  mask,
  userAgent,
  title,
  preferredLanguage,
  fetchImpl = fetch,
  idleMs = 30 * 60 * 1000,
}) {
  const key = playlistMask(mask);
  const initial = remoteAddress(url);
  if (!SITE.test(initial.hostname) || !initial.pathname.endsWith(".m3u8")) {
    throw new Error("Expected a FlixCloud playlist address");
  }
  const resources = new Map();
  const registered = new Map();
  const prefix = randomBytes(24).toString("hex");
  let base;
  let active = 0;
  let idle;

  function register(value, name) {
    const upstream = remoteAddress(value).href;
    if (!registered.has(upstream)) {
      if (resources.size >= MAX_RESOURCES) {
        throw new Error("The stream names too many resources");
      }
      const leaf = name || new URL(upstream).pathname.split("/").at(-1) || "resource";
      const path = `/${prefix}/${randomBytes(16).toString("hex")}/${encodeURIComponent(leaf)}`;
      registered.set(upstream, path);
      resources.set(path, upstream);
    }
    return base + registered.get(upstream);
  }

  function rewrite(text, parent) {
    let lines = text.split(/\r?\n/);
    if (preferredLanguage && text.includes("#EXT-X-STREAM-INF:")) {
      const audio = lines.filter((line) => /^#EXT-X-MEDIA:.*\bTYPE=AUDIO(?:,|$)/.test(line));
      const chosen = audio.filter((line) =>
        line.match(/\bLANGUAGE="([^"]+)"/)?.[1].toLowerCase() === preferredLanguage);
      if (audio.length && !chosen.length) {
        throw new Error(`The requested audio language (${preferredLanguage}) is unavailable`);
      }
      lines = lines.filter((line) => !audio.includes(line) || chosen.includes(line));
      lines = lines.map((line) => chosen.includes(line)
        ? (/,DEFAULT=(?:YES|NO)/.test(line)
          ? line.replace(/,DEFAULT=(?:YES|NO)/, ",DEFAULT=YES")
          : `${line},DEFAULT=YES`)
        : line);
    }
    return lines.map((line) => {
      if (!line || line.trim() === "") return line;
      if (!line.startsWith("#")) {
        return register(new URL(line.trim(), parent));
      }
      return line.replace(/URI="([^"]+)"/g, (_, uri) =>
        `URI="${register(new URL(uri, parent))}"`);
    }).join("\n");
  }

  function resetIdle() {
    clearTimeout(idle);
    if (idleMs > 0) {
      idle = setTimeout(() => {
        if (active === 0) server.close();
        else resetIdle();
      }, idleMs);
      idle.unref();
    }
  }

  const server = createServer(async (request, response) => {
    const upstream = resources.get(request.url);
    if (!upstream || !["GET", "HEAD"].includes(request.method)) {
      response.writeHead(404).end();
      return;
    }
    active++;
    resetIdle();
    const abort = new AbortController();
    const deadline = setTimeout(() => abort.abort(), 120_000);
    const disconnected = () => {
      if (!response.writableFinished) abort.abort();
    };
    response.on("close", disconnected);
    try {
      const headers = {
        Referer: "https://flixcloud.cc/",
        Origin: "https://flixcloud.cc",
        "Accept-Encoding": "identity",
        ...(userAgent ? { "User-Agent": userAgent } : {}),
      };
      if (request.headers.range) headers.Range = request.headers.range;
      const result = await fetchImpl(upstream, {
        method: request.method,
        headers,
        signal: abort.signal,
        redirect: "error",
      });
      if (!result.ok) {
        await result.body?.cancel();
        response.writeHead(result.status, {
          "Content-Type": "text/plain",
          "Cache-Control": "no-store",
        }).end(`Upstream returned HTTP ${result.status}`);
        return;
      }
      if (new URL(upstream).pathname.endsWith(".m3u8") && request.method !== "HEAD") {
        const text = rewrite(decodePlaylist(await boundedBody(result), key), upstream);
        response.writeHead(200, {
          "Content-Type": "application/vnd.apple.mpegurl",
          "Content-Length": Buffer.byteLength(text),
          "Cache-Control": "no-store",
          "X-Content-Type-Options": "nosniff",
        }).end(text);
      } else {
        const outgoing = { "Cache-Control": "no-store" };
        for (const name of ["content-type", "content-range", "accept-ranges"]) {
          const value = result.headers.get(name);
          if (value) outgoing[name] = value;
        }
        if (!result.headers.get("content-encoding")) {
          const length = result.headers.get("content-length");
          if (length) outgoing["content-length"] = length;
        }
        response.writeHead(result.status, outgoing);
        if (request.method === "HEAD" || !result.body) response.end();
        else await pipeline(Readable.fromWeb(result.body), response);
      }
    } catch (error) {
      if (!abort.signal.aborted) {
        console.error(`FlixCloud bridge: ${error.message}`);
      }
      if (!response.headersSent) {
        response.writeHead(502, {
          "Content-Type": "text/plain",
          "Cache-Control": "no-store",
        }).end("FlixCloud compatibility request failed; see the bridge log");
      } else {
        response.destroy(error);
      }
    } finally {
      clearTimeout(deadline);
      response.off("close", disconnected);
      active--;
      resetIdle();
    }
  });
  server.on("close", () => clearTimeout(idle));
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  base = `http://127.0.0.1:${server.address().port}`;
  resetIdle();
  const name = typeof title === "string" && title.trim()
    ? `${title.replace(/\.(?:mkv|mp4|webm|mov)$/i, "").replace(/[^\w .()-]/g, "_").slice(0, 120)}.m3u8`
    : undefined;
  return { server, url: register(initial, name) };
}

async function discover(page) {
  const session = `mdm-flixcloud-${randomBytes(8).toString("hex")}`;
  let executable = process.env.AGENT_BROWSER_EXECUTABLE || "agent-browser";
  if (process.platform === "win32" && !process.env.AGENT_BROWSER_EXECUTABLE) {
    const candidates = (process.env.PATH || "").split(delimiter).flatMap((folder) => [
      join(folder, "agent-browser.exe"),
      join(folder, "node_modules", "agent-browser", "bin", `agent-browser-win32-${process.arch}.exe`),
    ]);
    executable = candidates.find((candidate) => existsSync(candidate)) || executable;
  }
  const browser = async (...args) => {
    const { stdout } = await execute(executable, ["--session", session, ...args, "--json"], {
      windowsHide: true,
      timeout: 60_000,
      maxBuffer: 2 * MAX_PLAYLIST,
    });
    const result = JSON.parse(stdout);
    if (!result.success) throw new Error(result.error || "Browser inspection failed");
    return result.data;
  };
  try {
    const initial = new URL(page);
    if (initial.protocol !== "https:" || initial.username || initial.password) {
      throw new Error("Expected an HTTPS video page");
    }
    if (!SITE.test(initial.hostname) && initial.hostname !== "reanime.cz") {
      throw new Error("Use a Re:ANIME page or a FlixCloud player page");
    }
    await browser("open", initial.href);
    if (!SITE.test(initial.hostname)) {
      await browser("wait", "iframe[src*=\"flixcloud.cc/e/\"]");
      const { result: embedded } = await browser("eval",
        "document.querySelector('iframe[src*=\"flixcloud.cc/e/\"]').src");
      const frame = new URL(embedded);
      if (frame.protocol !== "https:" || !SITE.test(frame.hostname)) {
        throw new Error("The page did not provide a FlixCloud player");
      }
      await browser("open", frame.href);
    }
    await browser("wait", "--fn",
      "window.__pk && window.Artplayer && Artplayer.instances.some(a => a.hls && a.hls.url)");
    const { result } = await browser("eval", `(() => {
      const a = Artplayer.instances.find(a => a.hls && a.hls.url);
      a.pause();
      return {url:a.hls.url,mask:window.__pk,userAgent:navigator.userAgent,title:document.title};
    })()`);
    if (!result || typeof result.url !== "string") {
      throw new Error("The player did not provide stream information");
    }
    playlistMask(result.mask);
    remoteAddress(result.url);
    return {
      ...result,
      ...(initial.hostname === "reanime.cz" && initial.searchParams.get("lang") === "dub"
        ? { preferredLanguage: "eng" }
        : {}),
      ...(initial.hostname === "reanime.cz" && initial.searchParams.get("lang") === "sub"
        ? { preferredLanguage: "jpn" }
        : {}),
    };
  } finally {
    await browser("close");
  }
}

export async function openInMdm(url, title, page, dataDir) {
  const endpoint = JSON.parse(await readFile(join(dataDir, "ipc.json"), "utf8"));
  if (
    !Number.isInteger(endpoint.port) ||
    endpoint.port < 1 ||
    endpoint.port > 65535 ||
    typeof endpoint.token !== "string" ||
    endpoint.token.length < 32
  ) {
    throw new Error("MDM's local command endpoint is invalid");
  }
  await new Promise((resolve, reject) => {
    const socket = createConnection({ host: "127.0.0.1", port: endpoint.port });
    let answer = "";
    socket.setTimeout(5000, () => socket.destroy(new Error("MDM did not answer")));
    socket.on("error", reject);
    socket.on("end", () => reject(new Error("MDM closed its command channel without an answer")));
    socket.on("connect", () => socket.write(`${JSON.stringify({
      token: endpoint.token,
      video_page: url,
      video_title: title,
      page_url: page,
    })}\n`));
    socket.on("data", (chunk) => {
      answer += chunk;
      if (answer.length > 1024) {
        socket.destroy(new Error("MDM's command answer was too large"));
      } else if (answer.includes("\n")) {
        socket.destroy();
        if (answer.trim() === "ok") resolve();
        else reject(new Error("MDM refused the video command"));
      }
    });
  });
}

async function main() {
  const args = process.argv.slice(2);
  const check = args.includes("--check");
  const [page, mdm] = args.filter((arg) => arg !== "--check");
  if (!page) {
    throw new Error("Usage: node tools\\video\\flixcloud.mjs <video-page-url> [MDM executable] [--check]");
  }
  const stream = await discover(page);
  const bridge = await createBridge(stream);
  try {
    const probe = await fetch(bridge.url);
    if (!probe.ok || !(await probe.text()).startsWith("#EXTM3U")) {
      throw new Error("The compatibility playlist could not be verified");
    }
    if (check) {
      console.log("The player's encoded playlist was read successfully.");
      bridge.server.close();
      return;
    }
    console.log(`MDM playlist: ${bridge.url}`);
    console.log("The bridge closes after 30 minutes without requests.");
    if (mdm) {
      const portable = join(dirname(mdm), "data");
      const systemData = process.platform === "win32"
        ? process.env.APPDATA
        : process.platform === "darwin"
          ? join(homedir(), "Library", "Application Support")
          : process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
      const dataDir = process.env.MDM_DATA_DIR ||
        (existsSync(portable) ? portable : join(systemData, "misterdownloadmanager"));
      await openInMdm(bridge.url, stream.title, page, dataDir);
      console.log("Opened the quality chooser in Mister Download Manager.");
    }
  } catch (error) {
    bridge.server.close();
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(`FlixCloud support: ${error.message}`);
    process.exitCode = 1;
  });
}

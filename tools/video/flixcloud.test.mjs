import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createBridge, decodePlaylist, openInMdm, playlistMask } from "./flixcloud.mjs";

const mask = Buffer.alloc(32, 37);
const encoded = (text) => Buffer.from(
  Buffer.from(text).map((byte, i) => byte ^ mask[i % mask.length]),
).toString("base64");
const root = "https://fetch8.flixcloud.cc/_v8/test/master.m3u8";

test("decodes the player's encoding without changing ordinary HLS", () => {
  const text = "#EXTM3U\n#EXT-X-ENDLIST\n";
  assert.equal(decodePlaylist(Buffer.from(encoded(text)), mask), text);
  assert.equal(decodePlaylist(Buffer.from(text), mask), text);
  assert.throws(() => decodePlaylist(Buffer.from(encoded(text)), Buffer.alloc(32, 4)));
  assert.throws(() => decodePlaylist(Buffer.from("<html>refused</html>"), mask));
  assert.throws(() => playlistMask(""));
  assert.throws(() => playlistMask("A".repeat(42) + "B="));
});

test("refuses DRM rather than trying to bypass it", () => {
  assert.throws(() => decodePlaylist(Buffer.from(
    '#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI="key"\n',
  ), mask), /DRM/);
});

test("serves decoded nested playlists, keys and ranged fragments through opaque local URLs", async () => {
  const master = '#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",URI="audio/en.m3u8"\n' +
    '#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=320x180,AUDIO="audio"\nvideo.m3u8\n';
  const video = '#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="key.bin"\n' +
    '#EXTINF:2,\nhttps://vault-91.crimsoncdn.top/_v8/test/seg-0.png\n#EXT-X-ENDLIST\n';
  const calls = [];
  const upstream = new Map([
    [root, encoded(master)],
    [new URL("video.m3u8", root).href, encoded(video)],
    [new URL("audio/en.m3u8", root).href, encoded("#EXTM3U\n#EXT-X-ENDLIST\n")],
    [new URL("key.bin", root).href, Buffer.alloc(16, 9)],
    ["https://vault-91.crimsoncdn.top/_v8/test/seg-0.png", Buffer.from([1, 2, 3])],
  ]);
  const bridge = await createBridge({
    url: root,
    mask: mask.toString("base64"),
    idleMs: 0,
    fetchImpl: async (url, options) => {
      calls.push({ url, options });
      assert.equal(options.headers.Referer, "https://flixcloud.cc/");
      assert.equal(options.headers.Origin, "https://flixcloud.cc");
      assert.equal(options.redirect, "error");
      assert.ok(upstream.has(url));
      return new Response(upstream.get(url), {
        status: options.headers.Range ? 206 : 200,
        headers: options.headers.Range ? { "Content-Range": "bytes 0-2/3" } : {},
      });
    },
  });
  try {
    const text = await (await fetch(bridge.url)).text();
    assert.ok(text.startsWith("#EXTM3U"));
    assert.ok(!text.includes(mask.toString("base64")));
    const audio = text.match(/URI="([^"]+)"/)[1];
    assert.ok((await (await fetch(audio)).text()).startsWith("#EXTM3U"));
    const variant = text.trim().split("\n").at(-1);
    const media = await (await fetch(variant)).text();
    const key = media.match(/URI="([^"]+)"/)[1];
    assert.equal((await (await fetch(key)).arrayBuffer()).byteLength, 16);
    const fragment = media.split("\n").find((line) => line.startsWith("http"));
    const response = await fetch(fragment, { headers: { Range: "bytes=0-2" } });
    assert.equal(response.status, 206);
    assert.equal(response.headers.get("content-range"), "bytes 0-2/3");
    assert.deepEqual([...new Uint8Array(await response.arrayBuffer())], [1, 2, 3]);
    const count = calls.length;
    assert.equal((await fetch(new URL("/https://example.com", bridge.url))).status, 404);
    assert.equal(calls.length, count);
  } finally {
    bridge.server.close();
    await once(bridge.server, "close");
  }
});

test("rejects resources outside the supported hosts", async () => {
  const bridge = await createBridge({
    url: root,
    mask: mask.toString("base64"),
    idleMs: 0,
    fetchImpl: async () => new Response(encoded(
      "#EXTM3U\n#EXTINF:2,\nhttp://127.0.0.1/private\n#EXT-X-ENDLIST\n",
    )),
  });
  try {
    assert.equal((await fetch(bridge.url)).status, 502);
  } finally {
    bridge.server.close();
    await once(bridge.server, "close");
  }
});

test("keeps English audio for a dubbed page rather than silently selecting Japanese", async () => {
  const master = '#EXTM3U\n' +
    '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="Native",LANGUAGE="jpn",DEFAULT=YES,URI="jpn.m3u8"\n' +
    '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",LANGUAGE="eng",DEFAULT=NO,URI="eng.m3u8"\n' +
    '#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=320x180,AUDIO="audio"\nvideo.m3u8\n';
  const bridge = await createBridge({
    url: root,
    mask: mask.toString("base64"),
    preferredLanguage: "eng",
    idleMs: 0,
    fetchImpl: async () => new Response(encoded(master)),
  });
  try {
    const text = await (await fetch(bridge.url)).text();
    assert.ok(text.includes('LANGUAGE="eng",DEFAULT=YES'));
    assert.ok(!text.includes('LANGUAGE="jpn"'));
  } finally {
    bridge.server.close();
    await once(bridge.server, "close");
  }
});

test("refuses a missing requested audio language instead of choosing another", async () => {
  const bridge = await createBridge({
    url: root,
    mask: mask.toString("base64"),
    preferredLanguage: "eng",
    idleMs: 0,
    fetchImpl: async () => new Response(encoded(
      '#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",LANGUAGE="jpn",URI="jpn.m3u8"\n' +
      '#EXT-X-STREAM-INF:BANDWIDTH=200000,AUDIO="audio"\nvideo.m3u8\n',
    )),
  });
  try {
    assert.equal((await fetch(bridge.url)).status, 502);
  } finally {
    bridge.server.close();
    await once(bridge.server, "close");
  }
});

test("opens only the video chooser through MDM's authenticated local command channel", async () => {
  const folder = await mkdtemp(join(tmpdir(), "mdm-flixcloud-test-"));
  const token = "test-token-".repeat(8);
  let received;
  const server = createServer((socket) => {
    let line = "";
    socket.on("data", (chunk) => {
      line += chunk;
      if (!line.includes("\n")) return;
      received = JSON.parse(line);
      socket.end("ok\n");
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    await writeFile(join(folder, "ipc.json"), JSON.stringify({ port: server.address().port, token }));
    await openInMdm("http://127.0.0.1/test.m3u8", "Test clip", "https://reanime.cz/watch/test", folder);
    assert.deepEqual(received, {
      token,
      video_page: "http://127.0.0.1/test.m3u8",
      video_title: "Test clip",
      page_url: "https://reanime.cz/watch/test",
    });
    assert.ok(!("silent" in received));
  } finally {
    server.close();
    await once(server, "close");
    await rm(folder, { recursive: true, force: true });
  }
});

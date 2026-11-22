# Optional FlixCloud compatibility bridge

This experimental source-tree tool is a workaround, not native MDM or browser-extension
support. Some FlixCloud players return base64/XOR-encoded HLS playlists instead of
`#EXTM3U` text, causing "Response data has no m3u header." The bridge uses a separate
browser session to discover the stream and its playlist mask, closes that browser, and
serves ordinary HLS to MDM through a temporary loopback address.

Only download material you are authorised to save. DRM-protected streams are not supported.
Ordinary HLS AES-128 keys and media are forwarded unchanged for the downloader to handle.

## Setup and use

Install Node.js 22 or newer and install agent-browser and its Chromium browser separately:

```powershell
npm install -g agent-browser
agent-browser install
```

Start MDM and install its yt-dlp and ffmpeg helpers. From the repository root on Windows:

```powershell
pwsh -NoProfile -File .\tools\video\flixcloud.ps1
```

Paste a Re:ANIME watch-page URL or a FlixCloud player-page URL when prompted. The launcher
expects MDM's default Program Files installation. Choose a quality in MDM, start downloading,
and **keep the launcher's terminal open until the download finishes**.

For another installation path, run the Node helper directly:

```powershell
node .\tools\video\flixcloud.mjs "<video-page-url>" "C:\path\to\misterdownloadmanager.exe"
```

`MDM_DATA_DIR` can select the running MDM's data folder. Without an executable argument, the
helper prints a local playlist URL to paste into MDM's video lookup. Add `--check` to discover
the stream, validate its root playlist, and exit without opening MDM. If agent-browser is
not found on PATH, set `AGENT_BROWSER_EXECUTABLE` to its executable's full path.

## Limits

- Tested on Windows with MDM 0.5.2. Linux and macOS are not verified; the PowerShell launcher
  is Windows-only.
- Supports the current FlixCloud player layout, `flixcloud.cc` and `crimsoncdn.top` resource
  hosts, and public playback without a signed-in browser profile. Site changes can break it.
- Re:ANIME `lang=dub` selects English audio and `lang=sub` Japanese where separate audio
  renditions exist. A missing requested language is an error, not a fallback to another one.
- This does not change automatic capture in MDM's browser extension or install any app patch.
- The bridge exits after 30 minutes without requests. Every run creates a new local address;
  restarting it does not restore an old download's URLs. Resume after shutdown and automatic
  refresh of expired upstream signatures are not supported.
- Upstream traffic uses Node's networking, not MDM's proxy, DNS or network-adapter settings.
  Do not use this workaround if those settings are required for your connection.
- It exports no browser cookies and does not enable MDM's automation API. It uses MDM's
  existing authenticated local command channel only to request the quality chooser.
- Local playlist URLs grant access to that session's registered resources. Do not share them.

## Focused checks

No npm dependencies or network access are needed for the automated tests:

```powershell
node --test .\tools\video\flixcloud.test.mjs
node --check .\tools\video\flixcloud.mjs
```

Tests use synthetic playlists, mocked upstream responses and an isolated mock IPC endpoint;
they never connect to a real MDM instance or modify a user's downloads.

# Architecture

How Mister Download Manager's pieces fit together.

## Crates

| Path | Crate | Role |
|---|---|---|
| `crates/engine` | `mdm-engine` | Downloads one file over HTTP(S) or FTP(S): probe, segment planner, connection workers, resume state, per-host limits, speed limiter, checksums, proxy scripts |
| `crates/app` | `mdm-app` | The desktop app (`misterdownloadmanager.exe`, egui): list, categories, queues, scheduler, dialogs, tray, settings |
| `crates/cli` | `mdm-cli` | Headless downloads with progress (`get`, `resume`), videos, torrents and the automation API's commands: a library the app's program runs (`misterdownloadmanager --cli …`), and `misterdownloadmanager-cli`, a small launcher that starts it with the same arguments, terminal and exit code |
| `crates/mcp` | `mdm-mcp` | The MCP server (`misterdownloadmanager-mcp`): AI assistants list, add and control the running app's downloads through the automation API |
| `crates/nmhost` | `mdm-nmhost` | The browser's native messaging host: takes captured downloads from the extension and hands them to the app |
| `crates/ipc` | `mdm-ipc` | The single-instance channel (a token-protected local socket), the data folder, and the capture rules shared by the app and the host |
| `crates/tools` | `mdm-tools` | Video pages through yt-dlp and ffmpeg (run as separate programs) |
| `crates/crash` | `mdm-crash` | Crash and freeze reports: the monitor process, out-of-process minidumps, symbolicated stacks, the report's text and the release logs ([crash-reports.md](crash-reports.md)) |
| `crates/torrent` | `mdm-torrent` | BitTorrent and magnet links through librqbit |
| `crates/labserver` | `mdm-labserver` | An HTTP Range server with a request log and fault injection, and an FTP/FTPS server with its own faults, for tests and benchmarks |
| `extension/` | — | The Chromium (MV3) and Firefox extension |
| `docs/` | — | The user guide and tutorials (Help opens them), this file, the benchmark data |
| `website/` | — | A future website's pages (not published yet) |

Dependencies point inward: `app`, `cli`, `mcp` and `nmhost` use `engine` and `ipc`; the engine knows
nothing of the UI. The app sees the engine only through its events (progress, segment table
snapshots, finished, failed).

## Engine

```
Download
 ├─ probe()              size, resumable?, validators (ETag / Last-Modified), file name
 ├─ Planner (pure)       segment table: when a connection is free, the largest remaining
 │                       part is split in half (planner.rs, tested by random simulation)
 ├─ Worker × N           one HTTP/1.1 connection each, reused across segments; slow ones
 │                       are dropped and their part re-split
 │    └─ writes at its offset into <name>.mdm.part (created sparse)
 ├─ HostLimits           learns how many connections each server accepts
 ├─ Limiter (shared)     per-download and global token buckets
 ├─ State                segment table saved atomically (craft-atomic) for crash-safe resume
 └─ events → channel     Progress, Done, Failed
```

When every segment is done, the part file is checked against the expected size and renamed into
place; the file's date can follow the server's.

A download's first request (`get`) follows redirects and the pages that lead on to a file by
themselves (a waiting page's refresh). Sites whose links aren't the file's address have a module
in `sites/` (Google Drive: `sites/gdrive.rs`): it turns the link into the file's address, reads
the site's own pages (a confirmation form, an error page) and names its refusals
(`Error::Refused`), with pure functions tested without a network. The lab server plays such a
site on its real host names for the end-to-end tests (`labserver/src/drive.rs`).

FTP and FTPS (`ftp.rs`) go through the same planner, workers and state: a segment is `REST` +
`RETR` on a control session instead of a ranged `GET`, `SIZE` and `MDTM` stand in for the
headers, and sessions are pooled per download and reused between segments.

## App

One egui window painted to fixed pixel coordinates, plus one immediate viewport per dialog.
`app.rs` holds the state (downloads, queues, settings, the list's selection) and turns menu,
toolbar, key and IPC commands into actions. Each dialog is its own module and returns an
`Outcome` (open, closed, done with a value). Windows-only calls are confined to `win32.rs` and
their Linux and macOS counterparts to `unix.rs` (starting with the session, the browsers' host,
sounds, show in folder, the crash handler); every other file is safe, portable Rust. Fonts come from the system:
Windows' Fonts folder, macOS's system fonts, fontconfig on Linux (`theme.rs`).

Every colour and size a control is drawn with comes from the active theme (#45): `palette.rs`
declares its roles (`"button.hot"`, `"list.row"`…) with the classic look's values, the shared
painters (`winui.rs`, `chrome.rs`, `list.rs`, `scrollbar.rs`) read them through
`theme::colors()` and `theme::metrics()` and decide each control's states from them, and
`themefile.rs` reads theme files (TOML or JSON in the data folder's `themes`) that change any of
them. Where a window puts its controls stays its layout. So do the icons
before file names (`fileicons.rs`): the Windows shell's system image lists, macOS Finder's
(NSWorkspace), the Linux desktop's icon theme (`xdgicons.rs`, shared-mime-info and the Icon
Theme Specification). Each type is looked up once per pixel size, on a background thread. Tango's
icons (bundled) stand in until the system's icon is ready, and wherever the system has none.

The windows are drawn with OpenGL (eframe's glow) or with the system's own graphics API through
eframe's wgpu, with that one backend only: Direct3D 12 on Windows (the default there), Vulkan on
Linux, Metal on macOS (choices there; OpenGL stays their default). View → Draw with picks one. One
code path does it on every system (`crates/app/src/renderer.rs`): when the chosen one can't start,
the other one is tried, so the windows always open; a start whose first frame doesn't come, or a
graphics card lost twice, is remembered with its reason, which the menu shows.

The download list and settings are saved atomically to `%APPDATA%\misterdownloadmanager\`
(`downloads.json`). The browser host reads the same file for the capture rules, so a change in
Options applies to the next captured download.

## Browser integration

```
page ──► extension ──native messaging──► mdm-nmhost ──ipc──► app
```

The extension asks the host which file types to take, checks each download against them, and
cancels the browser's download only after the app has accepted it; otherwise the browser keeps
it.

## Translations

Every text the UI shows goes through `crate::i18n` (craft-i18n catalogs, `i18n/<locale>.tsv`).
`tools/i18n/extract.py` rebuilds `i18n/en.tsv` from the source, with a note for translators on
each text (what it is, where it shows, its placeholders).

## Tests and benchmarks

`cargo test` runs unit tests and end-to-end downloads against `mdm-labserver` with injected
faults (slow, dropped and refused connections, servers without ranges, changing files).
`tools/bench` runs the published speed comparison; its raw data is in `docs/bench/`.

# Architecture

How Mister Download Manager's pieces fit together.

## Crates

| Path | Crate | Role |
|---|---|---|
| `crates/engine` | `mdm-engine` | Downloads one file over HTTP(S) or FTP(S): probe, segment planner, connection workers, resume state, per-host limits, speed limiter, checksums, proxy scripts |
| `crates/app` | `mdm-app` | The desktop app (`misterdownloadmanager.exe`, egui): list, categories, queues, scheduler, dialogs, tray, settings |
| `crates/cli` | `mdm-cli` | Headless downloads with progress (`get`, `resume`) |
| `crates/nmhost` | `mdm-nmhost` | The browser's native messaging host: takes captured downloads from the extension and hands them to the app |
| `crates/ipc` | `mdm-ipc` | The single-instance channel (a token-protected local socket), the data folder, and the capture rules shared by the app and the host |
| `crates/tools` | `mdm-tools` | Video pages through yt-dlp and ffmpeg (run as separate programs) |
| `crates/torrent` | `mdm-torrent` | BitTorrent and magnet links through librqbit |
| `crates/labserver` | `mdm-labserver` | An HTTP Range server with a request log and fault injection, and an FTP/FTPS server with its own faults, for tests and benchmarks |
| `extension/` | — | The Chromium (MV3) and Firefox extension |
| `docs/` | — | The user guide and tutorials (Help opens them), this file, the benchmark data |
| `website/` | — | A future website's pages (not published yet) |

Dependencies point inward: `app`, `cli` and `nmhost` use `engine` and `ipc`; the engine knows
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

FTP and FTPS (`ftp.rs`) go through the same planner, workers and state: a segment is `REST` +
`RETR` on a control session instead of a ranged `GET`, `SIZE` and `MDTM` stand in for the
headers, and sessions are pooled per download and reused between segments.

## App

One egui window painted to fixed pixel coordinates, plus one immediate viewport per dialog.
`app.rs` holds the state (downloads, queues, settings, the list's selection) and turns menu,
toolbar, key and IPC commands into actions. Each dialog is its own module and returns an
`Outcome` (open, closed, done with a value). Windows-only calls are confined to `win32.rs` and
their Linux and macOS counterparts to `unix.rs` (starting with the session, the browsers' host,
sounds, show in folder); every other file is safe, portable Rust. Fonts come from the system:
Windows' Fonts folder, macOS's system fonts, fontconfig on Linux (`theme.rs`).

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

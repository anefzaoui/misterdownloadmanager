# Changelog

Each release's notes on GitHub come from its section here (`## [X.Y.Z]`, see
`.github/workflows/release.yml`).

## [0.1.0] - 2026-10-30

The first release: a free download manager for Windows, laid out the way Internet Download
Manager users expect, with experimental builds for macOS and Linux.

- **The download engine**: up to 32 connections per file, re-splitting the slowest part until
  the last byte, and crash-safe resume after a restart or a dropped connection. HTTP, HTTPS,
  **FTP and FTPS** (`ftp://`, `ftpes://` explicit TLS, `ftps://` implicit TLS), proxies, SOCKS and
  **proxy auto-config (PAC)** scripts.
- **The main window** you know: categories, the download list, the progress window with every
  connection's share, queues and the **Scheduler**, the speed limiter, Sites Logins, and
  **Options** where you expect them, every control working. Day and dark mode.
- **Browser integration** for Chrome, Edge, Brave, Opera and Vivaldi (Firefox in testing):
  downloads hand over to MDM with their cookies and referrer, right-click **Download with Mister
  Download Manager**, and a **Download video** button over videos on any page, which can be
  dragged along the video's edge, hidden on a video or a site, or turned off in Options.
- **Copy a link, get the download window**: copying a link to a file type on the **File types**
  list opens **Download File Info** with it, ready to start, also without the browser extension
  (**Options → General → Offer to download links copied to the clipboard**, on by default).
- **Videos from web pages** through yt-dlp (installed and kept current from MDM, with ffmpeg for
  the qualities that need merging), and **BitTorrent and magnet links** through librqbit: choose
  the files, see peers, trackers and pieces, keep sharing if you like.
- **86 interface languages**, every language Windows 11 offers, chosen by flag on the first start
  or in **View → Language...**; right to left for Arabic, Hebrew, Persian, Urdu and Uyghur; fonts
  for every script from the system itself. The installer speaks the same languages, and the
  browser extension speaks your browser's (50 languages).
- **Options → Regional**: dates and times as your region writes them, or your own way (day, month
  or year first, 12- or 24-hour clock, the week's first day); the list and the Scheduler follow.
- **[Command line and automation](docs/command-line.md)**: IDM's switches (`/d /p /f /a /n /s`),
  `misterdownloadmanager-cli` (files, videos, torrents) and a local, token-protected automation
  API that lists, pauses, starts, removes and deletes downloads.
- **Downloads**: the Windows installer, a portable zip and a plain zip, the browser extension on
  its own (a zip for Chromium browsers, an `.xpi` for Firefox, always the app's version), and
  **macOS and Linux to try**: a universal `.dmg` and an AppImage, `.deb`, `.rpm` and `.tar.gz`,
  marked `-experimental` because they are untested by us. There, MDM connects itself to Chrome,
  Edge, Brave, Vivaldi and Firefox, has its tray icon, follows the system's light or dark look and
  uses the system's fonts and ffmpeg. Tell us how they work for you.
- **Help** opens the [user guide and tutorials](docs/README.md).

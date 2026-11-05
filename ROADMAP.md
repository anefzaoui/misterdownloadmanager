# Roadmap

What's done for the next release, what's being built, and what's planned. Each item links to its
issue, where the details and the discussion are. Released changes are in
[CHANGELOG.md](CHANGELOG.md). Ideas and bug reports are welcome in
[Issues](https://github.com/anefzaoui/misterdownloadmanager/issues).

✅ done, not released yet · 🔄 being built · 📋 planned · 🔍 looking into it

## The next release

Everything below ships together, in one release.

### Done

- ✅ Video downloads: subtitles, beside the video or inside it
  ([#68](https://github.com/anefzaoui/misterdownloadmanager/issues/68))
- ✅ Torrent security: stricter file-name checks, no silent overwrite, files marked as downloaded
  from the internet, network switches (incoming, UPnP, port, DHT, local peers), SOCKS5 proxy and
  IP blocklist ([#40](https://github.com/anefzaoui/misterdownloadmanager/issues/40))
- ✅ Torrents: a speed limit per torrent, remembered limits, and the global cap covering torrents
  too ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
- ✅ Google Drive: public files download by their share links
  ([#34](https://github.com/anefzaoui/misterdownloadmanager/issues/34)); folders come later
- ✅ Video downloads: Stop ends yt-dlp and everything it started
  ([#61](https://github.com/anefzaoui/misterdownloadmanager/issues/61)); no error while yt-dlp
  goes on and finishes ([#79](https://github.com/anefzaoui/misterdownloadmanager/issues/79)); a
  quality the reachable YouTube servers don't serve falls back to the nearest one
  ([#94](https://github.com/anefzaoui/misterdownloadmanager/issues/94))
- ✅ A video's sound as MP3, at the quality you pick
  ([#32](https://github.com/anefzaoui/misterdownloadmanager/issues/32))
- ✅ More than 32 connections per download, if you want them
  ([#12](https://github.com/anefzaoui/misterdownloadmanager/issues/12))
- ✅ Your own yt-dlp and ffmpeg ([#51](https://github.com/anefzaoui/misterdownloadmanager/issues/51))
- ✅ View → Always dark ([#60](https://github.com/anefzaoui/misterdownloadmanager/issues/60))
- ✅ Zen, Helium and other Firefox- and Chromium-based browsers
  ([#41](https://github.com/anefzaoui/misterdownloadmanager/issues/41))
- ✅ Linux: menus open where clicked under Wayland
  ([#50](https://github.com/anefzaoui/misterdownloadmanager/issues/50)), and the window closes on
  desktops without a tray, such as MangoWC ([#59](https://github.com/anefzaoui/misterdownloadmanager/issues/59))
- ✅ Downloads that hand out one-time links no longer fail with 403
  ([#28](https://github.com/anefzaoui/misterdownloadmanager/issues/28))
- ✅ The extension's Download video menu keeps its highlight inside its edge
  ([#81](https://github.com/anefzaoui/misterdownloadmanager/issues/81))
- ✅ No crash when a window opens while MDM is minimized
  ([#22](https://github.com/anefzaoui/misterdownloadmanager/issues/22))
- ✅ Long file names stay inside the progress window: the Save to path is shortened in the
  middle, in every language ([#62](https://github.com/anefzaoui/misterdownloadmanager/issues/62))
- ✅ Every button translated: Download complete's Open with... no longer stays in English
  ([#80](https://github.com/anefzaoui/misterdownloadmanager/issues/80))
- ✅ A smaller download ([#25](https://github.com/anefzaoui/misterdownloadmanager/issues/25)),
  AppImage updates with zsync ([#24](https://github.com/anefzaoui/misterdownloadmanager/issues/24)),
  the Bangladesh flag for Bangla ([#27](https://github.com/anefzaoui/misterdownloadmanager/issues/27))
- ✅ Download complete shows above other windows, Options from the extension comes to the front
  ([#77](https://github.com/anefzaoui/misterdownloadmanager/issues/77)), and dragging the file out
  of Download complete works in every language
  ([#78](https://github.com/anefzaoui/misterdownloadmanager/issues/78))
- ✅ A file-type icon before every file name: the download list, Download File Info, Properties,
  a torrent's files and the Scheduler's (Material Icon Theme, the same on every system), and every
  list's rows named for screen readers ([#95](https://github.com/anefzaoui/misterdownloadmanager/issues/95))
- ✅ Website in European Portuguese, and an [intended use](https://misterdownloadmanager.com/intended-use.html) page
- ✅ Website: the [free IDM alternative](https://misterdownloadmanager.com/idm-alternative.html) page compares
  MDM with IDM and with nine other free download managers, each checked on its own site, with
  their logos, a feature-by-feature form and an MDM-vs-IDM label with the benchmark's times to scale

### Being built

- 🔄 Torrents, the rest: upload limits, a share ratio that survives restarts, seeding limits, more
  torrent actions, and RSS feeds with auto-download rules
  ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))

### Bugs being looked into

- 🔍 A freeze of the window on start ([#57](https://github.com/anefzaoui/misterdownloadmanager/issues/57)):
  most likely a computer busy just after start; freeze reports now measure it, so the next one says
- 🔍 A crash on Linux as Download File Info opens
  ([#48](https://github.com/anefzaoui/misterdownloadmanager/issues/48)), likely fixed with #22
- 🔍 Linux display issues ([#29](https://github.com/anefzaoui/misterdownloadmanager/issues/29))

## Planned

### Video and audio

- 📋 More audio choices: FLAC, Opus, the original track
  ([#70](https://github.com/anefzaoui/misterdownloadmanager/issues/70))
- 📋 Whole playlists and channels, also as MP3
  ([#69](https://github.com/anefzaoui/misterdownloadmanager/issues/69))
- 📋 gallery-dl support ([#58](https://github.com/anefzaoui/misterdownloadmanager/issues/58))

### Downloads and windows

- 📋 Hide a download's progress window to the tray
  ([#63](https://github.com/anefzaoui/misterdownloadmanager/issues/63))
- 📋 A link grabber with packages, and auto-extract of archives
  ([#37](https://github.com/anefzaoui/misterdownloadmanager/issues/37))
- 📋 Debrid services: Real-Debrid, AllDebrid, Premiumize
  ([#44](https://github.com/anefzaoui/misterdownloadmanager/issues/44))

### Network

- 📋 Bind downloads to a network adapter (VPN), with a kill switch
  ([#35](https://github.com/anefzaoui/misterdownloadmanager/issues/35))
- 📋 Custom DNS, and DNS over HTTPS or TLS
  ([#36](https://github.com/anefzaoui/misterdownloadmanager/issues/36))
- 📋 Downloads follow a browser's VPN or proxy extension
  ([#42](https://github.com/anefzaoui/misterdownloadmanager/issues/42))

### Browser extension

- 📋 List the media a tab has loaded, to download stubborn streams
  ([#38](https://github.com/anefzaoui/misterdownloadmanager/issues/38))
- 📋 Chrome Web Store and Firefox Add-ons listings (in review)

### Look and performance

- 📋 Themes: colours, accent, pywal and Base16, with the layout kept
  ([#45](https://github.com/anefzaoui/misterdownloadmanager/issues/45))
- 📋 Less memory while idle ([#72](https://github.com/anefzaoui/misterdownloadmanager/issues/72))
- 📋 Benchmarks against other download managers
  ([#71](https://github.com/anefzaoui/misterdownloadmanager/issues/71))

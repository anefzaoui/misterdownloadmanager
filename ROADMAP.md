# Roadmap

What's done for the next release, what's being built, and what's planned. Each item links to its
issue, where the details and the discussion are. Released changes are in
[CHANGELOG.md](CHANGELOG.md). Ideas and bug reports are welcome in
[Issues](https://github.com/anefzaoui/misterdownloadmanager/issues).

✅ done, not released yet · 🔄 being built · 📋 planned · 🔍 looking into it

## The next release

Everything below ships together, in one release.

### Done

- ✅ HLS and DASH videos from the browser reach yt-dlp as their stream, by the button over the
  video, the right-click menu (it did nothing in Firefox) and the media list
  ([#155](https://github.com/anefzaoui/misterdownloadmanager/issues/155))
- ✅ A download or magnet link sent while the window is minimized opens its window at once
  ([#162](https://github.com/anefzaoui/misterdownloadmanager/issues/162))
- ✅ Linux: Ubuntu's Firefox (the Snap) reaches MDM: browser integration is set up for it, and the
  help says what the desktop asks the first time; Chromium as a Snap, which can't reach MDM, is no
  longer offered (part of [#164](https://github.com/anefzaoui/misterdownloadmanager/issues/164))
- ✅ A smaller download: the installer 15 % and the zips 19 % smaller, nothing removed
  ([#158](https://github.com/anefzaoui/misterdownloadmanager/issues/158))
- ✅ Linux: MDM's first window opens seconds sooner; the system's fonts are looked up all at once
  while it starts, not one by one in the window (on a busy computer that froze it, part of
  [#164](https://github.com/anefzaoui/misterdownloadmanager/issues/164))
- ✅ Firefox's add-on offered from [addons.mozilla.org](https://addons.mozilla.org/firefox/addon/mister-download-manager/)
  first: **Add to Firefox** on the website, in the help, the FAQ and the README, and in MDM's
  **Add Mister Download Manager to your browser** window (for Zen and LibreWolf too); Chrome,
  Edge, Brave, Opera and Vivaldi by hand until the Chrome Web Store listing is approved
- ✅ Support MDM: a way to support its creator on [GitHub Sponsors](https://github.com/sponsors/anefzaoui),
  asked for only (**Help → Support MDM**, a line in About, the website, the README, the
  extension's popup and GitHub's Sponsor button); MDM stays free, nothing changes or unlocks
- ✅ A server that can't resume a download no longer gets the file downloaded twice on a busy
  computer (part of [#136](https://github.com/anefzaoui/misterdownloadmanager/issues/136))
- ✅ Translators: a reviewed translation is never replaced by machine translation, and CI checks it;
  Italian is the first catalog reviewed by a native speaker
  ([#150](https://github.com/anefzaoui/misterdownloadmanager/pull/150))
- ✅ Screen readers reach every row of a list, in view or not (300 around the view in long lists)
  ([#111](https://github.com/anefzaoui/misterdownloadmanager/issues/111))
- ✅ Less memory in the notification area after Minimize too, and again 2 minutes after hiding
  ([#72](https://github.com/anefzaoui/misterdownloadmanager/issues/72))
- ✅ Windows: MDM draws with Direct3D 12 by default; **View → Draw with** goes back to OpenGL, and
  either one falls back to the other when it can't start
  ([#89](https://github.com/anefzaoui/misterdownloadmanager/issues/89))
- ✅ Screen readers see a window's contents the first time they read it, not only its title bar
  ([#131](https://github.com/anefzaoui/misterdownloadmanager/issues/131))
- ✅ Magnet links get their file list in seconds again: the DHT keeps going after a computer it
  asked is gone, and Add torrent shows the wait (peers found, from where, what may be in the
  way) ([#92](https://github.com/anefzaoui/misterdownloadmanager/issues/92))
- ✅ Two engine tests that failed now and then on busy machines are reliable
  ([#123](https://github.com/anefzaoui/misterdownloadmanager/issues/123))
- ✅ Numbers are grouped and pointed the interface language's way (German `10.200`, `1,50 MB`)
  ([#128](https://github.com/anefzaoui/misterdownloadmanager/issues/128))
- ✅ The extension's button lists the media the tab has loaded (HLS and DASH streams, video and
  sound files), to download a stream the page won't give up; copy-protected ones are shown as
  protected, never downloaded ([#38](https://github.com/anefzaoui/misterdownloadmanager/issues/38))
- ✅ Torrents: RSS feeds with download rules (words, episodes, each episode once, a folder per rule)
  ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
- ✅ Torrents: Recheck, Reannounce, Add trackers and Copy magnet link; files chosen while a
  torrent runs; a `.torrent` file's web address opens Add torrent ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
- ✅ Link grabber: links in packages, looked up for name and size; archives (zip and tar
  built in; 7z and RAR need 7-Zip) extracted when they are downloaded
  ([#37](https://github.com/anefzaoui/misterdownloadmanager/issues/37)); collecting copied links
  comes later
- ✅ Image galleries with gallery-dl: Wikimedia Commons, Flickr, DeviantArt and many more,
  verified like yt-dlp, or your own copy ([#58](https://github.com/anefzaoui/misterdownloadmanager/issues/58))
- ✅ Torrents: seeding limits (a share ratio, or a time), off by default, and a finished torrent's
  tabs while it is shared ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
- ✅ A paused or finished download no longer leaves its progress file to be written again a moment
  later ([#106](https://github.com/anefzaoui/misterdownloadmanager/issues/106)), and a download
  finishing during a save no longer fails ([#116](https://github.com/anefzaoui/misterdownloadmanager/issues/116))
- ✅ Tests and test tools stay on 127.0.0.1: no Windows Firewall questions while developing
- ✅ Debrid accounts (experimental): Real-Debrid, AllDebrid and Premiumize; file-host links downloaded through your own account (Options → Accounts)
  ([#44](https://github.com/anefzaoui/misterdownloadmanager/issues/44))
- ✅ Downloads through one network adapter (a VPN's), with a kill switch: nothing goes out
  another way when it disconnects ([#35](https://github.com/anefzaoui/misterdownloadmanager/issues/35))
- ✅ Torrents on Windows through the chosen network adapter, with the same kill switch
  ([#143](https://github.com/anefzaoui/misterdownloadmanager/issues/143))
- ✅ Downloads from a browser behind a VPN or proxy extension go through the same proxy
  ([#42](https://github.com/anefzaoui/misterdownloadmanager/issues/42))
- ✅ Hide to the notification area: a download's progress window (its own button, or Minimize)
  and the main window's Minimize, with the tray's "Downloads in progress"
  ([#63](https://github.com/anefzaoui/misterdownloadmanager/issues/63))
- ✅ Whole playlists and channels, one quality for all: also as the original sound, MP3, Opus or FLAC
  ([#69](https://github.com/anefzaoui/misterdownloadmanager/issues/69))
- ✅ Choose the DNS: Cloudflare, Google, Quad9, AdGuard, NextDNS or your own, over HTTPS, TLS or
  plain, with a Test button and no silent fallback
  ([#36](https://github.com/anefzaoui/misterdownloadmanager/issues/36))
- ✅ Less memory in the notification area: under 10 MB there, and the language list's fonts let
  go when it closes ([#72](https://github.com/anefzaoui/misterdownloadmanager/issues/72))
- ✅ Video downloads: subtitles, beside the video or inside it
  ([#68](https://github.com/anefzaoui/misterdownloadmanager/issues/68)); more audio choices:
  the original track, Opus and FLAC ([#70](https://github.com/anefzaoui/misterdownloadmanager/issues/70))
- ✅ Torrent security: stricter file-name checks, no silent overwrite, files marked as downloaded
  from the internet, network switches (incoming, UPnP, port, DHT, local peers), SOCKS5 proxy and
  IP blocklist ([#40](https://github.com/anefzaoui/misterdownloadmanager/issues/40))
- ✅ Torrents: a speed limit per torrent, remembered limits, and the global cap covering torrents
  too ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
- ✅ Torrents: upload limits (all torrents, and each one), and a share ratio that survives pauses
  and restarts ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
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
- ✅ No crash when a window opens while MDM is minimized or in the notification area, such as a
  download or a magnet link from the browser
  ([#22](https://github.com/anefzaoui/misterdownloadmanager/issues/22),
  [#145](https://github.com/anefzaoui/misterdownloadmanager/issues/145))
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
- ✅ The built-in categories can be renamed, given other file types and deleted, and restored
  ([#93](https://github.com/anefzaoui/misterdownloadmanager/issues/93))
- ✅ A file-type icon before every file name, in every window that names a file (the download
  list, Download File Info, Properties, Download complete, Add torrent, a torrent's files, the
  Scheduler, Batch download, Download video and its playlists), each the system's own icon for
  its type (Windows Explorer's, macOS Finder's, the Linux desktop's icon theme), sharp at every
  display scale
  ([#122](https://github.com/anefzaoui/misterdownloadmanager/issues/122)), and every
  list's rows named for screen readers ([#95](https://github.com/anefzaoui/misterdownloadmanager/issues/95)),
  inside their list, with their place in it ([#102](https://github.com/anefzaoui/misterdownloadmanager/issues/102));
  the Categories pane, the menu bar and every right-click menu too, with Shift+F10 and the Menu key
  ([#113](https://github.com/anefzaoui/misterdownloadmanager/issues/113))
- ✅ The README's, the docs' and the website's screenshots are taken on a stock Windows (GitHub's
  runner), so the file icons in them are Windows' own
- ✅ Website in European Portuguese, and an [intended use](https://misterdownloadmanager.com/intended-use.html) page
- ✅ Website: the [free IDM alternative](https://misterdownloadmanager.com/idm-alternative.html) page compares
  MDM with IDM and with nine other free download managers, each checked on its own site, with
  their logos, a feature-by-feature form and an MDM-vs-IDM label with the benchmark's times to scale

### Being built

- 🔄 Themes ([#45](https://github.com/anefzaoui/misterdownloadmanager/issues/45)): today's look stays the default. Every colour, size and spacing now
  comes from the chosen theme (the classic look pixel for pixel the same); **View → Theme** with
  Classic, Native (follows the system's accent and contrast on Windows, Linux and macOS) and High
  contrast; then style themes with their own control shapes, a drawn title bar on Windows and their
  own icons: Metro first, then Adwaita and Breeze, then Luna, Aero and Aqua with icons drawn for
  MDM. Theme files can be added by anyone, and Base16 and pywal schemes imported
- 🔄 A minimized MDM keeps working: queues, the Scheduler, speed limits, the kill switch and saving
  no longer wait for the window to be restored (after [#162](https://github.com/anefzaoui/misterdownloadmanager/issues/162))
- 🔄 Where a download's server is: its country, flag and round trip, from a table that comes with
  MDM; no address is sent anywhere ([#140](https://github.com/anefzaoui/misterdownloadmanager/issues/140))
- 🔄 Torrents: download in order, to watch a video while it downloads; the other torrents rarest
  first ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39))
- 🔄 A stream whose address doesn't end in `.m3u8` or `.mpd` is still recognised by its contents
  ([#171](https://github.com/anefzaoui/misterdownloadmanager/issues/171))
- 🔄 Chrome Web Store: **Add to Chrome** beside **Add to Firefox** in MDM, on the website and in the
  help, and MDM's browser window opens each browser's store
- 🔄 Firefox: reading the browser's proxy or VPN extension becomes a switch in the add-on, so the
  update asks for no new permission ([#42](https://github.com/anefzaoui/misterdownloadmanager/issues/42))
- 🔄 MDM's browser window: the check mark before **Connected** shows in every language (an empty
  box in Arabic and on Linux), the German subtitle fits, the Arabic layout is mirrored
  ([#185](https://github.com/anefzaoui/misterdownloadmanager/issues/185), [#170](https://github.com/anefzaoui/misterdownloadmanager/issues/170))
- 🔄 Linux supported like Windows, no longer experimental: every change is now checked on Ubuntu
  (GNOME) as well as Windows, then on KDE, Cinnamon and Xfce ([#164](https://github.com/anefzaoui/misterdownloadmanager/issues/164))
- 🔄 Before the release: every screenshot taken again on Windows and Linux (smaller files, WebP),
  and every page of the docs, the README and the website brought up to date
  ([#186](https://github.com/anefzaoui/misterdownloadmanager/issues/186))

### Bugs being looked into

- 🔍 A freeze of the window on start ([#57](https://github.com/anefzaoui/misterdownloadmanager/issues/57)):
  most likely a computer busy just after start; freeze reports now measure it, so the next one says
- 🔍 A crash on Linux as Download File Info opens
  ([#48](https://github.com/anefzaoui/misterdownloadmanager/issues/48)), likely fixed with #22
- 🔍 Linux display issues ([#29](https://github.com/anefzaoui/misterdownloadmanager/issues/29))

## Planned

### Downloads and windows

- 📋 Download video: the **Install ffmpeg** button fits its text in every language
  ([#172](https://github.com/anefzaoui/misterdownloadmanager/issues/172))
- 📋 The Scheduler wakes the computer from sleep to run a queue
  ([#142](https://github.com/anefzaoui/misterdownloadmanager/issues/142))
- 📋 Debrid services, the rest: a per-download "directly instead" choice, "through the service" in
  File Info, the command line using the accounts, torrents through the service, the system
  keychains for the sign-ins
  ([#44](https://github.com/anefzaoui/misterdownloadmanager/issues/44))

### Network

- 📋 Video pages through a chosen network adapter too ([#35](https://github.com/anefzaoui/misterdownloadmanager/issues/35))
- 📋 Control MDM from a phone or another computer on the local network, off by default
  ([#141](https://github.com/anefzaoui/misterdownloadmanager/issues/141))
- 📋 Run MDM headless on a server (Docker, Unraid) and send it downloads from the browser
  ([#157](https://github.com/anefzaoui/misterdownloadmanager/issues/157))

### Browser extension

- ✅ On Firefox Add-ons: [Mister Download Manager](https://addons.mozilla.org/firefox/addon/mister-download-manager/)
- ✅ On the Chrome Web Store: [Mister Download Manager](https://chromewebstore.google.com/detail/mister-download-manager/ejoleppdfgjdndddcjoehopknmlglojp)
- 📋 Chrome: use the browser's proxy or VPN extension for downloads too (it needs a permission
  Chrome can't ask for later, so it waits for a release that can ask everyone once)

### Look and performance

- 📋 Website: the header's buttons fit in long languages ([#168](https://github.com/anefzaoui/misterdownloadmanager/issues/168)), and Arabic pages
  don't scroll sideways on phones ([#169](https://github.com/anefzaoui/misterdownloadmanager/issues/169))
- 📋 Linux: follow the desktop's dark or light setting ([#156](https://github.com/anefzaoui/misterdownloadmanager/issues/156))
- 📋 Benchmarks against other download managers
  ([#71](https://github.com/anefzaoui/misterdownloadmanager/issues/71))

# Mister Download Manager on Android

> [!NOTE]
> The Android app is new and still being tested. It is built from the repository's
> `android/` folder; what is done is in [ROADMAP.md](../ROADMAP.md).
> How it is made: [android-architecture.md](android-architecture.md).

The Android app runs the same download engine as the desktop app (segmented downloads that
resume, torrents, videos with yt-dlp, debrid accounts, the speed limiter, queues and the
Scheduler), with an interface made for a phone.

## Install

Download the APK from this repository's releases (tags named `android-v…`):

| File | For |
| --- | --- |
| `app-arm64-v8a-release.apk` | Almost every phone and tablet from the last ten years. **Not sure? Take this one.** |
| `app-armeabi-v7a-release.apk` | Older 32-bit phones. |
| `app-x86_64-release.apk` | Chromebooks and emulators on PCs. |
| `app-universal-release.apk` | Any of them (larger). |

Open the file on your phone and allow your browser or file manager to install apps when Android
asks. Android 8 or newer is needed.

## The app

Three places, along the bottom (a rail on tablets): **Downloads**, **Queues** and **Settings**.
The blue header shows the total speed, how many downloads run and the free space; the tabs under
it filter by state (Running, Waiting, Done, Failed), the funnel by category, the magnifier finds
a download by name or address. Downloads in progress come first, then what has finished. Tap a
download for its details (its parts as the connections bring them, its fields, its log); touch
and hold to select several and pause, resume or delete them together.

## Add downloads

- **Share** a link from any app (a browser's Share menu) and choose **Download with MDM**.
- Long-press a link in a browser, **Open with**, and choose MDM (MDM never takes links over by
  default).
- Tap the green **Add**: a link on the clipboard is filled in, and the paste button adds more.
  Choose a category (or let MDM choose by the file's type), and Start now or Add to queue.
- **Magnet links** open in MDM, and so do **.torrent files** from a file manager or a browser's
  downloads: you choose which of the torrent's files to download, and whether in order (to watch
  while it downloads).
- A **video page** (YouTube, Vimeo and the many sites yt-dlp knows): MDM looks up its qualities
  and you pick one; a **playlist or channel** lists its videos to tick, in one quality for all.
  The first time, MDM asks to install yt-dlp (about 3 MB, from yt-dlp's own releases, checked
  against its developers' signature). ffmpeg, which joins a video's picture and sound, comes
  with the app.
- An **image gallery** (Wikimedia Commons, Flickr, DeviantArt…): every picture, through
  gallery-dl, which MDM installs the first time the same way.

## While downloads run

After the phone restarts, MDM sets the Scheduler's next start again; on Android 15, which lets no
app restart its downloads by itself then, a notification lets one tap go on where they stopped.

Downloads keep going when you leave the app: a notification shows how many run, how fast, and
has **Pause all**. On Android 14 and newer they run as a "user-initiated transfer", which Android
lets run as long as needed. On older versions they run as a foreground service; Android 15 allows
those 6 hours a day, after which MDM pauses them and says so.

Some phone makers stop apps in the background more than Android does. If downloads stop when the
screen is off, open **Settings → Battery optimization** in MDM and let it run unrestricted
([dontkillmyapp.com](https://dontkillmyapp.com) explains each maker's settings).

## Where files go

**Download/MisterDownloadManager** on the phone, in a folder per category (Video, Music,
Compressed, Documents, Programs). Finished files show up in Files and in your gallery. In
**Settings → Folder** you can choose an SD card or USB drive (MDM's own folder there, which
Android removes with the app), or **Another folder…**: any folder you pick, once you let MDM
manage all files (Android asks once).

## Settings

| Setting | |
| --- | --- |
| Connections per download | How many parts a file downloads in at once (8 by default). |
| Download on Wi-Fi only | On mobile data, running downloads pause and the queue stops; back on Wi-Fi, the same ones go on. |
| Speed limiter | A limit for all downloads, in KB/s. |
| Scheduler | Each queue's downloads at once, a daily start and stop time, days. A queue with a start time starts then even when MDM isn't open. |
| Torrents | Keep sharing finished torrents, DHT, UPnP, local peers, incoming connections. |
| Network | A proxy server or an automatic configuration script; DNS over HTTPS (Cloudflare, Google, Quad9, AdGuard, NextDNS or your own). |
| Archives | Extract zip, 7z, RAR, tar, gz and xz archives when they finish, and delete them after (7-Zip comes with the app for 7z and RAR). |
| Accounts | Debrid services (Real-Debrid, AllDebrid, Premiumize, TorBox, Mega-Debrid): sign in on the service's own page with a code, or paste an API token. |
| Language | Any of MDM's 86 languages, or the phone's. |

## When something goes wrong

If MDM closes unexpectedly, the next time it opens a notification offers to share the crash
report (the phone's model and Android version, what happened, and the end of MDM's log, with no
addresses' passwords or cookies). **Settings → Check for updates** looks for a newer release. **Settings → Open-source licences**
lists the programs by others the app carries (Python, ffmpeg, QuickJS, 7-Zip, its typeface) with
their licences.

## Not on Android (yet)

- The browser extension: Chrome on Android has no extensions. Share or "Open with" instead.
- Link grabber's window, RSS feeds and Sites Logins (many links still go in through Add).
- Music and films from DRM streaming services (Spotify, Apple Music, Netflix…): no download
  manager can save them, and MDM says so when such a link is added.
- Themes: the app has MDM's own look, light and dark with the system.
- Some of the app's texts are still English in other languages until the project's translation
  run covers them.

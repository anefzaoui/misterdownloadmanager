# How the Android app is made

The Android app is not a second download manager: it is MDM's own engine and MDM's own
application logic, the same Rust code the Windows, Linux and macOS builds run, with a phone
interface on top. This page explains how that works, what had to change in the shared code (and
why the desktop is unaffected), and how it is built and tested. What the app does for its users is
in [android.md](android.md).

## The idea in one picture

```
 ┌───────────────────────────── Android app (Kotlin, Jetpack Compose) ─────────────────────────────┐
 │  Downloads · Queues · Settings · Add sheet · Details      notifications, background job/service  │
 │  ui/*.kt                                                   service/*.kt                          │
 │                         core/Mdm.kt  ── JSON requests ──▶   ◀── snapshots (≤ 4 a second) ──       │
 └──────────────────────────────────────────────┬─────────────────────────────────────────────────┘
                                                │ JNI: three calls (start, request, stop) and two
                                                │ callbacks (changed, call)
 ┌──────────────────────────────────────────────┴─────────────────────────────────────────────────┐
 │  crates/android (libmdm.so)            the JNI bridge, about 200 lines                         │
 ├────────────────────────────────────────────────────────────────────────────────────────────────┤
 │  crates/app: android.rs (Core: a thread running App::tick)  android_ui.rs (the JSON interface)  │
 │  the desktop's App, unchanged in what it does: the list, queues, Scheduler, categories,       │
 │  settings, debrid, video/playlist/gallery look-ups, torrents, archive extraction, i18n        │
 ├────────────────────────────────────────────────────────────────────────────────────────────────┤
 │  mdm-engine (HTTP/FTP segmented downloads)  mdm-torrent (librqbit)  mdm-debrid  mdm-extract     │
 │  mdm-tools (yt-dlp, gallery-dl, ffmpeg)     mdm-geo  mdm-ipc  …    — all unchanged in behaviour │
 └────────────────────────────────────────────────────────────────────────────────────────────────┘
        runs as separate programs: Python (yt-dlp, gallery-dl), ffmpeg, QuickJS, 7-Zip
```

The desktop app already had a **headless mode**: `App::tick` runs everything a frame of the main
window would (start queued downloads, apply the speed limiter, run the Scheduler, finish and
extract downloads, save the list) without drawing anything. MDM's command line and its server
mode (`--headless`) use it. The Android core is that same headless `App`, ticked on its own
thread, with a small JSON interface in front of it in place of the egui windows.

## The core on Android

`crates/app/src/android.rs`:

- **`Core::start(config, host)`** sets the data, cache and downloads folders (the app's private
  folder, `Download/MisterDownloadManager`), the interface language (the phone's, or the one
  chosen), the User-Agent of the phone's WebView, and starts one thread that owns the `App`.
- That thread loops: run the jobs the interface sent, run one `App::tick` (a pass of an egui
  context that draws nothing), and, at most 4 times a second and only when something changed,
  push a **snapshot** (the list, speeds, queues, free space, the language and its direction) to
  the host. It sleeps until the next tick the `App` asks for, or until a request wakes it.
- **`Core::request(json)`** runs one request on the core thread and returns its JSON answer.
- The **`Host`** trait is how the core reaches Android: `changed(snapshot)`, the keychain
  (secrets are kept with the Android Keystore, `core/Secrets.kt`, as Windows' Credential Manager
  or the Secret Service keep them on the desktop), `wake_at` (an exact alarm for the Scheduler's
  next start) and `keep_awake` (a wake lock while downloads run).

`crates/app/src/android_ui.rs` is the interface: a `match` on the request's `op`, each answer
built from the `App`'s own state and functions, so the phone sees exactly what the desktop
windows would:

| Area | Operations |
|---|---|
| The list | `snapshot`, `add`, `add_torrent_file`, `classify`, `act` (start, pause, remove, delete), `details` |
| Queues and Scheduler | `queue`, `queues`, `set_queue`, `speed_limit` |
| Videos and playlists (yt-dlp) | `video_lookup`, `video_poll`, `video_pick`, `playlist_start`, `playlist_poll`, `playlist_add` |
| Torrents | `torrent_lookup`, `torrent_poll`, `torrent_add` |
| Galleries (gallery-dl) | `gallery_add` |
| Helpers | `helpers`, `install_ytdlp`, `install_gallery` |
| Debrid accounts | `debrid`, `debrid_add`, `debrid_signin`, `debrid_signin_state`, `debrid_signin_cancel`, `debrid_remove` |
| Settings and language | `settings`, `set_settings`, `downloads_dir`, `languages`, `set_language`, `tr` |
| Life cycle | `dismiss_notice`, `save`, `exit` |

The words the phone shows that come from the core (a download's state, an error, where its
server is, a video's qualities) come in the interface language through MDM's own catalogs.

## The JNI bridge

`crates/android` builds `libmdm.so` (a `cdylib`). Its three exported functions are
`NativeCore.start`, `NativeCore.request` and `NativeCore.stop`; the core calls back into Kotlin's
`NativeHost.changed` (a snapshot) and `NativeHost.call` (the keychain, alarms, wake locks). It
also hands the JVM to `rustls-platform-verifier`, so HTTPS trusts the phone's own certificate
store. This crate is the only Android FFI in the workspace, as
`win32.rs` is Windows'.

## What changed in the shared code

The rule was: nothing the desktop does changes. Each change is a `cfg`, a split or a facade:

| Where | What | Why |
|---|---|---|
| `crates/app` | `main.rs` became `lib.rs` plus a 3-line `main.rs` | Android loads a library, not a program |
| `crates/app/Cargo.toml` | eframe, arboard and egui-winit only off Android; feature `android-core` | no window system on Android; the feature lets the Android core be tested on Linux |
| `crates/filedialog` | `rfd` behind a small facade crate | no native file dialog on Android (the app picks folders itself) |
| `crates/app/src/tray/none.rs` | a tray that cannot exist | no notification-area icon on Android |
| `crates/app/src/secrets.rs` | the keychain through the `Host` | the Android Keystore lives in Kotlin |
| `crates/app/src/unix.rs` | Android's platform calls re-exported from `android.rs` | default browser, file icons, keychain, alarms |
| `crates/engine` (FTP over TLS) | a WebPKI verifier on Android | the platform verifier there has no "extra roots" constructor |
| `crates/engine` | `rquickjs` with its `bindgen` feature on Android | it ships no prebuilt bindings for Android |
| `crates/tools/src/android.rs` | where the helper programs are and how to run them | see below |
| `crates/tools/src/pages.rs` | `drm_service`: Spotify, Apple Music, Netflix… named | said at once instead of a page downloaded or yt-dlp asked in vain |
| `crates/app/src/whereis.rs` | `summary`: the server line in words | the desktop draws it; the phone shows the same words |
| `crates/app/src/app.rs` | `android` flag; `add_without_window` | the quality menu and Add don't open desktop windows |

`cargo test`, `cargo clippy --all-targets` and the desktop's 547 unit tests pass unchanged.

## The helper programs

Android allows an app to run programs only from its native library folder (files in its data
folder can't be executed, "W^X"). So every helper ships in the APK as a `lib*.so`, unpacked by
Android into that folder, and is run by its path:

| Helper | What for | Where it comes from |
|---|---|---|
| Python 3.12 (`libpython.so`, its library in `libpython.zip.so`) | runs yt-dlp and gallery-dl | youtubedl-android 0.18.1's AAR (Termux's build) |
| ffmpeg, ffprobe | joins a video's picture and sound | youtubedl-android 0.18.1's AAR (Termux's GPL-3.0 build) |
| QuickJS (`libqjs.so`) | the JavaScript YouTube's player needs | youtubedl-android 0.18.1's AAR |
| 7-Zip (`lib7zz.so`) | 7z and RAR archives | built from 7-Zip 26.06's source with the NDK by the app's build |
| yt-dlp, gallery-dl | the look-ups and downloads | **not in the APK**: installed on first use from their own releases, checked against their developers' signatures, as on the desktop |

Only the AARs' native files are taken, none of their Java code: MDM's core runs the programs
itself. `mdm-tools`' `android.rs` unpacks the zipped libraries once (per version), makes links
under their usual names (`ffmpeg`, `python`, `7zz`…), and builds each command with the
environment it needs (`PYTHONHOME`, `LD_LIBRARY_PATH`, the certificate file, `HOME`, `TMPDIR`).
yt-dlp then runs as `python yt-dlp`, gallery-dl as `python -m gallery_dl` from its wheel.

7-Zip is built from source because its official Linux builds are linked with glibc, and
Android's app sandbox (seccomp) kills them on their first system call it doesn't allow. The
emulator test caught that.

## The Android side

- **Downloads in the background**: on Android 14 and newer a *user-initiated data transfer job*
  (no daily limit); before, or when Android refuses the job, a `dataSync` foreground service
  (6 hours a day on Android 15, then paused with a notice). One ongoing notification with the
  total progress and **Pause all**, updated at most once a second; one per finished or failed
  download.
- **Scheduler**: the core asks for an exact alarm at the next start; it is set again after the
  phone restarts. Android 15 lets no app restart its downloads after a reboot by itself, so a
  notification offers to.
- **Storage**: `Download/MisterDownloadManager/<category>` by default, an SD card's app folder,
  or any folder once **All files access** is granted. Finished files are added to the media
  library.
- **Links in**: Share, Open with, magnet links, `.torrent` files, the clipboard.
- **Wi-Fi only**, crash reports (shared by the user, nothing sent by itself), update check
  against the repository's `android-v*` releases (the APK checked against `SHA256SUMS.txt`).

## The interface

Jetpack Compose and Material 3, in MDM's own look (DESIGN.md's "waybill on the parcel"): the
logo's blue as the header, white sheets, Archivo condensed for titles and field names, tabular
figures for numbers, green only for Add and for what finished. A bottom bar on phones and a rail on
tablets. Right-to-left languages mirror the layout and keep numbers left to right; the layout holds
at a 130 % font size.

## Languages

The app's own texts (`res/values/strings.xml`) join MDM's catalogs as rows of context
`android` (`tools/i18n/extract.py`), translated with the rest by `tools/i18n/translate.py`, and
are written back as each language's `values-*/strings.xml` by `tools/android/strings.py`,
plurals mapped to Android's by each language's CLDR rules.

## Tests

| Test | What it proves | Where |
|---|---|---|
| The core end to end, on Linux | the JSON interface against MDM's test server: add, run, complete, pause, resume, queues, details (where the server is), settings, languages, debrid token kept in the keychain, DRM services named | `crates/app/tests/android_core.rs` |
| Screenshots on the JVM | every screen, light and dark, right to left, large font, tablet | `android/app/src/test` (Roborazzi) |
| Instrumented, on an Android 14 emulator | the real APK: the core starts through JNI, downloads over HTTP and HTTPS into Download/, answers the interface, 7-Zip makes and extracts an archive | `android/app/src/androidTest` |
| CI | all three, and the APKs for arm64, 32-bit ARM and x86-64 | `.github/workflows/android.yml` |

## Building it

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/28.2.13676358
cd android && ./gradlew assembleDebug            # arm64 by default; -Pmdm.abis=x86_64 for an emulator
cargo test -p mdm-app --features android-core --test android_core   # the core, no phone needed
./gradlew :app:recordRoborazziDebug -Pmdm.skipCargo=true            # the screenshots
```

Build options: `-Pmdm.abis=…`, `-Pmdm.skipCargo=true` (reuse `libmdm.so`),
`-Pmdm.updateRepo=owner/repo` (where updates are looked for), `-Pmdm.sevenZipRar=false` (7-Zip
without its RAR code).

## Licences

MDM stays GPL-3.0. Everything the APK carries by others is a separate program or a file, listed
with its licence and source in **Settings → Open-source licences** and in
[ATTRIBUTION.md](../ATTRIBUTION.md). No code, artwork or text from Internet Download Manager.

## Not yet on Android

Link grabber's window, RSS feeds, Sites Logins, Click'n'Load, the phone's system proxy, and a
TalkBack pass on a real phone. [ROADMAP.md](../ROADMAP.md) keeps the list.

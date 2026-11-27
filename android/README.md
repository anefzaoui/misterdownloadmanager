# Mister Download Manager for Android

The Android app: MDM's own engine and interface (crates/app) in a GameActivity, laid out for a
phone. `crates/android` is the native library the activity loads (`libmdm.so`); this folder is
the Gradle project around it: the activity, the downloads' foreground service, Android's
keystore, alarms, clipboard and intents for the app (`Platform.kt`), and the helper programs the
APK carries.

## What you need

- Rust with the Android targets: `rustup target add aarch64-linux-android` (and
  `armv7-linux-androideabi`, `x86_64-linux-android` for those ABIs), and
  `cargo install cargo-ndk`
- JDK 21, and the Android SDK with NDK 28.2.13676358 (`sdkmanager "ndk;28.2.13676358"`);
  `ANDROID_HOME` (or `local.properties`' `sdk.dir`) set
- Nothing else: 7-Zip is built from its source with the NDK, and Python, ffmpeg and QuickJS come
  from Maven Central at build time

## Build

```sh
cd android
./gradlew assembleDebug                              # arm64-v8a, the phones most people have
./gradlew assembleDebug -Pmdm.abis=x86_64            # the emulator
./gradlew assembleDebug -Pmdm.abis=arm64-v8a,armeabi-v7a,x86_64
```

The APK is `app/build/outputs/apk/debug/app-debug.apk`; install it with
`adb install -r app/build/outputs/apk/debug/app-debug.apk`. Other properties:
`-Pmdm.cargoProfile=dev` (a quicker, larger `libmdm.so`), `-Pmdm.skipCargo=true` (use the
`libmdm.so` already in `src/main/jniLibs`), `-Pmdm.sevenZipRar=false` (7-Zip without its RAR
code). A release build and its signing are the maintainer's.

## What the app does

- The download list in a phone layout (`crates/app/src/phone.rs`): under the top bar a card of
  how the downloads go (their speed, how many run, a ring with how far they are) and chips for
  All, Unfinished and Finished; rows for a finger with the file type's icon, a progress bar, the
  status in its colour and the figures, each with its own Pause or Resume and ⋮ menu; a tap
  opens a download (its page, or a finished one's file); a long press starts selection mode
  (check circles, "N selected", Select all, the picked downloads' menu, Resume / Stop / Delete
  at the bottom); out of it the bottom bar has Add URL, Video, Stop All and the Scheduler; the
  drawer has the categories and queues, then the rest of the commands; the top bar searches
  the list (by file name) and sorts it (the desktop's Arrange files); all in MDM's theme colours
- A download's page: its figures, the whole progress bar and under it each connection's part
  (the desktop's split view), Pause / Cancel, Properties / Open folder, its connections
- Every window (Add URL, Download File Info, the progress window, Options, Add torrent...) as a
  page over the list, its title in a top bar whose × closes it, laid out for the phone's width
  with big buttons at the bottom (a few rarely used ones, such as Torrent settings, are drawn a
  little smaller to fit); pages slide in, menus rise from the bottom; **Back** closes the page,
  menu or selection in front, and on the list sends the app to the background
- Downloads in `Download/MisterDownloadManager`, kept going by a foreground service whose
  notification says how they go and has **Stop All**; each download has its own notification
  (a progress bar, its figures, **Stop**), and one that completes says so, with **Open** (no
  Download complete page over the app)
- Links in: Share (text with links), a file's link or a magnet opened with MDM, a `.torrent`
  opened or shared; files out: **Open**, **Open with...** and **Open folder** through Android's
  apps
- Sign-ins in Android's keystore, the Scheduler's exact alarms, the clipboard
- The **...** buttons through Android's pickers: a folder on the phone's storage (or a card), a
  file to open copied into the app's cache, a file to save in a folder picked there
- Video downloads with the APK's Python, ffmpeg and QuickJS; 7z and RAR with its 7-Zip

## Not yet

- Controls inside the pages keep the desktop's size (21 point fields and check boxes; a touch
  near one reaches it); only the pages' main buttons are a finger's size
- Downloads go on while the app runs in the background; if Android closes the app, they stop
  where they were and go on at its next start
- Resuming after the phone restarts (Android doesn't let an app open its screen by itself then)

See [third_party/README.md](third_party/README.md) for where the files the build fetches come
from.

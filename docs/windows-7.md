# Windows 7

[← Back to docs index](README.md)

MDM has a build of its own for **Windows 7 SP1**, 64-bit and 32-bit, **from a later release**:
version 0.5.2 and the releases before it have no Windows 7 files. It is **experimental**: it is
built and checked on newer Windows, and hasn't been tried on a real Windows 7 PC yet. If you run
Windows 7, you can help: tell us how it went in an
[issue](https://github.com/anefzaoui/misterdownloadmanager/issues).

## Choose a download

From a later release, the [Releases](https://github.com/anefzaoui/misterdownloadmanager/releases)
page has Windows 7 files, with `win7` and the processor in their names:

| Files | For |
|---|---|
| `...-setup-win7-x64.exe`, `...-portable-win7-x64.zip`, `...-win7-x64.zip` | **64-bit Windows 7 SP1** |
| `...-setup-win7-x86.exe`, `...-portable-win7-x86.zip`, `...-win7-x86.zip` | **32-bit Windows 7 SP1** |

Not sure which you have? **Start → right-click Computer → Properties → System type** says it. The
setup, portable zip and plain zip are the same three kinds as for Windows 10 and 11
([Getting started](getting-started.md#choose-a-download)).

**Help → Check for updates** keeps a Windows 7 copy on the Windows 7 files: it never updates it to
the files for Windows 10 and 11, which don't start on Windows 7, and those never update to these.
On Windows 10 or 11, take the usual files instead: they draw faster (Direct3D 12) and have
everything below.

## What Windows 7 needs

- **Service Pack 1** and its updates from Windows Update. Setup refuses to run on Windows 7
  without SP1.
- **A graphics driver with OpenGL 2.1 or later**: the one from your graphics card's maker (NVIDIA,
  AMD, Intel), or from Windows Update. Windows 7 has no Direct3D 12, so MDM draws its windows with
  OpenGL only. Windows' own OpenGL (version 1.1, what a PC without its graphics driver, a virtual
  machine without its guest tools or Remote Desktop gets) can't draw them: MDM then says so in a
  message and ends.
- **Trusted certificates kept current** (Windows Update's automatic root certificate updates, on
  unless a company turned them off): MDM checks each secure (HTTPS) site's certificate with
  Windows, as on Windows 10 and 11.

## What's different on Windows 7

- **Drawing:** OpenGL only. The **View → Draw with** submenu isn't there at all: OpenGL is the
  only way the Windows 7 build draws, so there is nothing to choose.
- **Video pages:** yt-dlp's own Windows builds no longer start on Windows 7 (since its 2024.11.04
  version), so MDM doesn't download one there. **Helpers...** says MDM has no copy for this system:
  choose a yt-dlp that runs on Windows 7 with **Use another program...**, and MDM uses it.
- **ffmpeg** (64-bit Windows 7): MDM downloads the same build as on Windows 10, which says it
  is made for Windows 7 and later. On 32-bit Windows 7, as on 32-bit Windows 10, none is built
  that MDM can check: choose your own in **Helpers...**.
- **gallery-dl:** MDM downloads its 32-bit build on both kinds of Windows 7: it is the one still
  built with a Python that runs on Windows 7.
- **Sites with a bot check:** the browser-like connection, which MDM tries once when a site's bot
  check refused its own, isn't in the Windows 7 build (its encryption library doesn't run on
  Windows 7). Such a download stays with the browser, as when that connection fails.
- **The browser extension** needs Chrome 116 or Firefox 140 or later, and the last versions for
  Windows 7 are Chrome 109 and Firefox ESR 115: the extension can't be added there yet. Add
  downloads with **Add URL**, the clipboard or the drop target
  ([Adding downloads](adding-downloads.md)).
- **Text:** Windows 7's DirectWrite lacks what MDM's classic text drawing uses, so MDM draws its
  text itself, as it does for scripts DirectWrite doesn't draw.

## For developers

The Windows 7 build is Rust's own Windows 7 targets, `x86_64-win7-windows-msvc` and
`i686-win7-windows-msvc` ([their page](https://doc.rust-lang.org/nightly/rustc/platform-support/win7-windows-msvc.html)).
They are tier 3: Rust ships no standard library for them, so it is built from source with the
program (`-Zbuild-std`), which takes a nightly toolchain and its `rust-src`. The nightly is pinned
in `tools/win7/toolchain.txt` and moved on on purpose.

```
tools\win7\cargo.ps1 -Arch x64 build --release --no-default-features -p mdm-app
tools\installer\build.ps1 -Arch x64 -Windows7     # the setup and both zips, then the import check
python tools\win7\imports.py check target\x86_64-win7-windows-msvc\release\misterdownloadmanager.exe
```

`tools/win7/cargo.ps1` installs the pinned toolchain when missing and passes the target and
`-Zbuild-std`. `--no-default-features` leaves the browser-like connection out.

A program that imports a function Windows 7 doesn't have doesn't start there ("The procedure entry
point ... could not be located"). `tools/win7/imports.py check` reads the programs' import tables and
fails on any function `tools/win7/win7-imports.tsv` doesn't list as Windows 7's: each function MDM's
programs import, with the first Windows that has it from Microsoft's documentation ("Minimum
supported client"), or a note for what the documentation doesn't cover. CI runs it on every build.
A new import fails it until it is looked up and added (`imports.py docs` fills the rows from a
checkout of [MicrosoftDocs/sdk-api](https://github.com/MicrosoftDocs/sdk-api)). A function only
newer Windows has is looked up when it is needed instead (`GetProcAddress`), with Windows 7's way
beside it, as `win32.rs`'s `dpi` module does for Windows 10's DPI functions.

Two libraries load a function by name themselves, which no import table shows: BoringSSL (the
browser-like connection's) takes Windows 8's `ProcessPrng` and ends the program without it, so the
Windows 7 build leaves that connection out (`mdm_engine::browserlike::AVAILABLE` is false there
whatever the features say); AWS-LC (rustls's) uses `BCryptGenRandom` instead when built for these
targets. So `imports.py check` also fails on a program that holds the name `ProcessPrng` or
`bcryptprimitives` at all, and on one whose header asks for a newer Windows than 7 (an operating
system or subsystem version above 6.1).

windows-rs links a few COM functions from Windows 8's `combase.dll`. A copy of windows-link (the
crate that names each function's DLL), patched into the workspace, links them from `ole32.dll`
instead, on these targets only. A patch can't be limited to one target, so the copy is compiled
into every build (where it does what windows-link does), and CI checks it is the published crate
with that one change (`tools/win7/check_windows_link.py`).

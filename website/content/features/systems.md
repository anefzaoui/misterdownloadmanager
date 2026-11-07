---
name: Windows, Mac and Linux
title: Download manager for Windows, with macOS and Linux builds
headline: Made for Windows. On a Mac and Linux too.
line: Stable on Windows 10 and 11, installed or portable; experimental builds for macOS and Linux.
lede: MDM is built for 64-bit Windows 10 and 11, as an installer, a portable copy or a plain zip. Experimental builds for macOS and Linux come with every release.
description: Mister Download Manager runs on Windows 10 and 11, installed or portable, with experimental macOS (.dmg) and Linux (AppImage, .deb, .rpm) builds.
group: app
order: 340
since: 0.1.0
shot: /assets/screens/main-light.webp
shot_alt: The main window on Windows 11: the toolbar, the categories on the left and the download list on the right
where: The download page: pick your system's file
help: getting-started.md#choose-a-download
---

## Windows: three ways to have it

On 64-bit Windows 10 and 11, MDM is the stable, recommended build. The [download
page](/download.html) offers three files:

- **The installer** (`setup.exe`), the one to pick unless you have a reason not to. It installs
  for you alone without administrator rights, or for everyone, and can set up browser
  integration and open `.torrent` files and magnet links with MDM.
- **The portable copy** (`portable.zip`), which runs from any folder.
- **The plain zip** (`x64.zip`), the same programs without an installer.

**Help → Check for updates → Update now** downloads a new version, checks its signature, puts it
in place and restarts, for the setup and both zips.

## Portable: everything in one folder

Unzip the portable copy anywhere, a USB stick included. The download list, the settings, the
video helpers and torrent data stay in a `data` folder next to the program, and nothing is
written to the registry. Two things need the installer instead: starting with Windows, and
taking downloads from the browser, since both go through the registry. [Carry MDM on a USB
stick](guide-usb-stick.md) shows the set-up.

## Mac and Linux: experimental, and said so

The files marked `-experimental` are for macOS 11 and later, on Apple Silicon or Intel, and for
Linux on x86-64: an AppImage for most distributions, a `.deb` for Ubuntu, Debian, Mint and
Pop!_OS, an `.rpm` for Fedora, openSUSE and RHEL, and a `.tar.gz` of the plain programs. They
are new, so you'd be among the first: please [tell us how it
went](https://github.com/anefzaoui/misterdownloadmanager/issues/new/choose), good or bad.

There, MDM connects itself to your browsers at each start, has its tray icon, follows the
system's light or dark look and uses the system's fonts and ffmpeg. The Mac app isn't notarized
by Apple yet, so macOS asks before its first start: right-click it and choose **Open** (on
macOS 15 and later, **System Settings → Privacy & Security → Open Anyway**). A `.deb` or `.rpm`
update opens in the system's installer. A few parts work only on Windows, and the help says so
where it matters.

## Smaller Mac downloads, lighter Linux updates

<!--@coming 0.5-->

The Mac gets two smaller downloads, one for Apple Silicon and one for Intel, instead of one
universal `.dmg` of 80 MB. From 0.2.4 or older, update the Mac once by hand: **Download** in
**Check for updates** opens the release, where you pick your Mac's file. The AppImage can update
through AppImageLauncher, Gear Lever and AppImageUpdate, which fetch only the parts that changed.
Linux can draw with Vulkan and macOS with Metal, from **View → Draw with**, while OpenGL stays the
default there. In a Wayland session, MDM runs through XWayland so its menus open where you click.

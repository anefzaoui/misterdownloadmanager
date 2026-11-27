# Third-party assets

| Asset | Author | Licence | Where |
|---|---|---|---|
| Lucide icons v1.63.0 (toolbar, dialog glyphs, the globe for a browser without a logo, the check and cross marks of a result, each download's status icon, the helpers' role icons, gallery-dl's included) | Lucide Icons and Contributors | ISC — `crates/app/assets/icons/lucide/LICENSE` | `crates/app/assets/icons/lucide/` |
| Tango Icon Theme 0.8.90, 25 of its icons (the icon before a file's or folder's name where the system has none of its own, and until the system's is ready: every window that names a file; this computer and the local network where a download's server is) | The Tango Desktop Project (Ulisse Perusin, Steven Garrity, Lapo Calamandrei, Ryan Collier, Rodney Dawes, Andreas Nilsson, Tuomas Kuosmanen, Garrett LeSage, Jakub Steiner), <http://tango.freedesktop.org> | Public domain — crates/app/assets/icons/tango/README.txt | crates/app/assets/icons/tango/ |
| flag-icons 7.6.0 (language flags: picker, View → Language, README) | Panayiotis Lipiridis and contributors, <https://github.com/lipis/flag-icons> | MIT — `crates/app/assets/flags/LICENSE` | `crates/app/assets/flags/` |
| flag-icons 7.6.0, all 250 country flags in one picture (where a download's server is: Download File Info, the progress window, Properties), drawn by `crates/geo/examples/flag_atlas.rs` | Panayiotis Lipiridis and contributors, <https://github.com/lipis/flag-icons> | MIT — `crates/geo/data/LICENSE-flag-icons` | `crates/geo/data/flags.png` |
| DB-IP IP to Country Lite, a monthly edition (which country a download's server is in, looked up on the computer, never sent anywhere), packed by `tools/geo/build.py` | DB-IP, <https://db-ip.com>: IP Geolocation by DB-IP | Creative Commons Attribution 4.0 International, <https://creativecommons.org/licenses/by/4.0/>; the edition used is in `crates/geo/data/SOURCE.txt` | `crates/geo/data/ipcountry.bin.xz` |
| Patrick Hand (the README's hand-drawn pictures, as outlines; not in the app) | Patrick Wagesreiter | SIL Open Font License 1.1 — `tools/readme/node_modules/@fontsource/patrick-hand/LICENSE` | `docs/images/races-*.svg` |
| *Big Buck Bunny* (the film in the README's pictures of the download button over a video; not in the app) | Blender Foundation, <https://peach.blender.org> | Creative Commons Attribution 3.0 | `docs/images/video-button-*.png` |
| Browser logos: Chrome, Chromium, Edge, Edge Legacy, Internet Explorer, Brave, Vivaldi, Opera, Opera GX, Firefox, Safari and Yandex from browser-logos 75.0.1; Zen, LibreWolf, Floorp, Waterfox, Thorium and Helium from each browser's own repository (before each browser's name: the browser extension's window, the first start's setup, Options → General's browser list and Test browser integration), unchanged | browser-logos: Cătălin Mariș and contributors, <https://github.com/alrra/browser-logos>; the others: each browser's makers, the file and commit of each in `crates/app/assets/browsers/SOURCES.txt` | browser-logos' files MIT — `crates/app/assets/browsers/LICENSE-browser-logos.txt`; the others as `SOURCES.txt` lists (GPL-3.0, MPL-2.0, BSD-3-Clause, or none stated). **The logos are trademarks of their respective owners, used only to identify the browsers.** | `crates/app/assets/browsers/` |
| Silk icon set 1.3 (Categories tree) | Mark James, <http://www.famfamfam.com/lab/icons/silk/> | Creative Commons Attribution 2.5 — `crates/app/assets/icons/silk/readme.txt` | `crates/app/assets/icons/silk/` |
| Fluent UI System Icons 1.1.345, 49 of its icons (the Metro theme's toolbar, categories, file types and title-bar buttons), recoloured white to be tinted | Microsoft Corporation, <https://github.com/microsoft/fluentui-system-icons> (commit 9eb184e) | MIT — `crates/app/assets/themes/metro/icons/LICENSE` | `crates/app/assets/themes/metro/icons/` (its magnet, Torrents, torrent-file and Hide to the notification area icons are MDM's own) |
| adwaita-icon-theme 51.0, 50 of its icons (the Adwaita theme's toolbar, categories, file types and title-bar buttons), unchanged; its one-colour icons are drawn in the theme's text colour when read | The GNOME Project, <https://gitlab.gnome.org/GNOME/adwaita-icon-theme> (commit 70ecd52) | LGPL-3.0 (of LGPL-3.0 or CC BY-SA 3.0 US) — `crates/app/assets/themes/adwaita/icons/LICENSE`, `COPYING` | `crates/app/assets/themes/adwaita/icons/` (its PDF, torrent-file and Hide to the notification area icons are MDM's own) |
| breeze-icons 6.31, 51 of its icons (the Breeze theme's toolbar, categories, file types and title-bar buttons), unchanged; its one-colour icons are drawn in the theme's text colour when read | Uri Herrera and the KDE Visual Design Group, <https://invent.kde.org/frameworks/breeze-icons> (commit 60ca517) | LGPL-3.0-or-later — `crates/app/assets/themes/breeze/icons/LICENSE` | `crates/app/assets/themes/breeze/icons/` (its PDF and Hide to the notification area icons are MDM's own) |

The app icon (`art/Mister Download Manager.png`) was made for Mister Download Manager by its
author, Ahmed Nefzaoui, with ChatGPT (OpenAI's image generation); every icon size is built from
it by `tools/icon/build_icon.py`.

The Luna, Aero and Aqua themes' icons (`crates/app/assets/themes/luna/icons/`, `aero/icons/`,
`aqua/icons/`) were drawn for Mister Download Manager, as were the Metro, Adwaita and Breeze
icons named MDM's own above; each set's `SOURCES.txt` lists them.

The README's block icons (`docs/images/icons/`: arrow, globe, gift, magnet) were made for it
with OpenAI's GPT-5 Image Mini by `tools/readme/icons.py`, given the app icon as the style to follow.

## Archive extraction (mdm-extract)

The libraries Link grabber's archive extraction is built on (all compatible with MDM's GPL-3.0):

| Library | Licence | For |
|---|---|---|
| [zip](https://crates.io/crates/zip) 9.1 | MIT | .zip, with [deflate64](https://crates.io/crates/deflate64) (MIT) and WinZip AES ([RustCrypto](https://github.com/RustCrypto) aes, hmac, pbkdf2: MIT OR Apache-2.0) |
| [lzma-rust2](https://crates.io/crates/lzma-rust2) 0.23 | Apache-2.0 | .xz |
| [tar](https://crates.io/crates/tar) 0.4 | MIT OR Apache-2.0 | .tar |

7z and RAR archives are extracted by [7-Zip](https://www.7-zip.org/) (GNU LGPL, with the
unRAR restriction for its RAR code), run as a separate program when the user has it installed.
It is neither bundled nor linked: UnRAR's licence doesn't allow its code in a GPL program, and a
built-in 7z decoder made MDM about 1 MB larger.

## Torrents (mdm-torrent)

Torrents run on [librqbit](https://github.com/ikatson/rqbit) by Igor Katson and its
contributors (Apache-2.0), built from MDM's copy of it,
[anefzaoui/rqbit](https://github.com/anefzaoui/rqbit), which adds MDM's own changes and these
fixes taken from upstream ahead of its next release, each under Apache-2.0 by its author:

| Author | Change (ikatson/rqbit pull request) |
|---|---|
| Damián Búho | the peers-seen metric's name (#717) |
| uzinag (qnfm) | torrent ids handed out one at a time, so torrents added together are all added (#711); UTF-8 name fields, `name.utf-8` and `path.utf-8` (#765) |
| zerx-lab | the fake-peer test harness; a HAVE for every finished piece (#719); one tracker's failure kept to that tracker (#723); a dropped session not kept alive by a silent peer (#752); a pause late in the first check kept (#721); a start during that check keeps its peers (#746) |
| Lucas Clemente Vella | local peer discovery with a torrent registered twice (#713) |
| Poonnawit | HTTP trackers asked at their `interval` (#731) |
| MemoDo-En | the UDP tracker socket falls back to IPv4 without IPv6 (#707) |
| Jake Huneau | the piece map saved on pause (#754) |
| Martin Bruse | a file's progress counted with its piece, past an empty file (#656) |
| Adam Greenwell | a read-only file handle on a Windows sharing violation (#645) |

System resources used at run time and **not** bundled: Windows fonts (Segoe UI, Tahoma,
Arial) and each system's own file-type icons (the Windows shell's, macOS Finder's, the Linux
desktop's icon theme).

MDM contains no artwork, text or code from Internet Download Manager (see the README).

# Third-party assets

| Asset | Author | Licence | Where |
|---|---|---|---|
| Lucide icons v1.63.0 (toolbar, dialog glyphs) | Lucide Icons and Contributors | ISC — `crates/app/assets/icons/lucide/LICENSE` | `crates/app/assets/icons/lucide/` |
| Material Icon Theme v5.40.0, 48 of its file icons (the icon before a file's name: the download list, Download File Info, Properties, a torrent's files, the Scheduler's files) | Material Extensions (Philipp Kief and contributors), <https://github.com/material-extensions/vscode-material-icon-theme> | MIT — `crates/app/assets/icons/material/LICENSE` | `crates/app/assets/icons/material/` (chosen and fetched by `tools/icons/material.py`; release and commit in `VERSION`) |
| flag-icons 7.6.0 (language flags: picker, View → Language, README) | Panayiotis Lipiridis and contributors, <https://github.com/lipis/flag-icons> | MIT — `crates/app/assets/flags/LICENSE` | `crates/app/assets/flags/` |
| Patrick Hand (the README's hand-drawn pictures, as outlines; not in the app) | Patrick Wagesreiter | SIL Open Font License 1.1 — `tools/readme/node_modules/@fontsource/patrick-hand/LICENSE` | `docs/images/races-*.svg` |
| *Big Buck Bunny* (the film in the README's pictures of the download button over a video; not in the app) | Blender Foundation, <https://peach.blender.org> | Creative Commons Attribution 3.0 | `docs/images/video-button-*.png` |
| Silk icon set 1.3 (Categories tree) | Mark James, <http://www.famfamfam.com/lab/icons/silk/> | Creative Commons Attribution 2.5 — `crates/app/assets/icons/silk/readme.txt` | `crates/app/assets/icons/silk/` |

The app icon (`art/Mister Download Manager.png`) was made for Mister Download Manager by its
author, Ahmed Nefzaoui, with ChatGPT (OpenAI's image generation); every icon size is built from
it by `tools/icon/build_icon.py`.

The README's block icons (`docs/images/icons/`: arrow, globe, gift, magnet) were made for it
with OpenAI's GPT-5 Image Mini by `tools/readme/icons.py`, given the app icon as the style to follow.

## Archive extraction (mdm-extract)

The libraries Link grabber's archive extraction is built on (all compatible with MDM's GPL-3.0):

| Library | Licence | For |
|---|---|---|
| [zip](https://crates.io/crates/zip) 9.1 | MIT | .zip, with [deflate64](https://crates.io/crates/deflate64) (MIT) and WinZip AES ([RustCrypto](https://github.com/RustCrypto) aes, hmac, pbkdf2: MIT OR Apache-2.0) |
| [sevenz-rust2](https://crates.io/crates/sevenz-rust2) 0.24 | Apache-2.0 | .7z (decoding only) |
| [lzma-rust2](https://crates.io/crates/lzma-rust2) 0.23 | Apache-2.0 | LZMA, LZMA2 and .xz |
| [tar](https://crates.io/crates/tar) 0.4 | MIT OR Apache-2.0 | .tar |

RAR archives are extracted by [7-Zip](https://www.7-zip.org/) (GNU LGPL, with the unRAR
restriction for its RAR code), run as a separate program when the user has it installed. It is
neither bundled nor linked: UnRAR's licence doesn't allow its code in a GPL program.

System resources used at run time and **not** bundled: Windows fonts (Segoe UI, Tahoma,
Arial) and the Windows shell's file-type icons.

MDM contains no artwork, text or code from Internet Download Manager (see the README).

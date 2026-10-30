# Third-party assets

| Asset | Author | Licence | Where |
|---|---|---|---|
| Lucide icons v1.63.0 (toolbar, dialog glyphs) | Lucide Icons and Contributors | ISC — `crates/app/assets/icons/lucide/LICENSE` | `crates/app/assets/icons/lucide/` |
| flag-icons 7.6.0 (language flags: picker, View → Language, README) | Panayiotis Lipiridis and contributors, <https://github.com/lipis/flag-icons> | MIT — `crates/app/assets/flags/LICENSE` | `crates/app/assets/flags/` |
| Patrick Hand (the README's hand-drawn pictures, as outlines; not in the app) | Patrick Wagesreiter | SIL Open Font License 1.1 — `tools/readme/node_modules/@fontsource/patrick-hand/LICENSE` | `docs/images/races-*.svg` |
| *Big Buck Bunny* (the film in the README's pictures of the download button over a video; not in the app) | Blender Foundation, <https://peach.blender.org> | Creative Commons Attribution 3.0 | `docs/images/video-button-*.png` |
| Silk icon set 1.3 (Categories tree) | Mark James, <http://www.famfamfam.com/lab/icons/silk/> | Creative Commons Attribution 2.5 — `crates/app/assets/icons/silk/readme.txt` | `crates/app/assets/icons/silk/` |

The app icon (`art/Mister Download Manager.png`) was made for Mister Download Manager by its
author, Ahmed Nefzaoui, with ChatGPT (OpenAI's image generation); every icon size is built from
it by `tools/icon/build_icon.py`.

The README's block icons (`docs/images/icons/`: arrow, globe, gift, magnet) were made for it
with OpenAI's GPT-5 Image Mini by `tools/readme/icons.py`, given the app icon as the style to follow.

System resources used at run time and **not** bundled: Windows fonts (Segoe UI, Tahoma,
Arial) and the Windows shell's file-type icons.

MDM contains no artwork, text or code from Internet Download Manager (see the README).

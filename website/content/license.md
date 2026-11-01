---
title: License
description: Mister Download Manager is free software under the GNU General Public License, version 3. What that means for you, and the licences of every component it is built from.
lede: Mister Download Manager is free software under the GNU General Public License, version 3 (GPL-3.0). Here's what that means, and who else's work it is built on.
fine: Applies to version {{VERSION}}
priority: 0.4
---

<div class="summary-box" markdown="1">

## What the GPL means for you

- **Use it** for anything, at home or at work, on as many computers as you like, for free.
- **Share it**: give copies to anyone, or put it on your own website.
- **Study and change it**: the [source code]({{REPO}}) is yours to read and modify.
- **If you distribute a changed version**, you must release your changes under the GPL too, with their source code, so everyone keeps the same freedoms.
- It comes **without warranty**; see the licence's sections 15 and 16.

</div>

This summary is not the licence. The full text is in the [LICENSE file]({{REPO}}/blob/main/LICENSE) and at [gnu.org/licenses/gpl-3.0](https://www.gnu.org/licenses/gpl-3.0.html).

Copyright © 2026 Ahmed Nefzaoui and contributors.

> Mister Download Manager is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
>
> Mister Download Manager is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.

## Artwork and fonts

| Component | By | Licence |
|---|---|---|
| The MDM logo and app icon | Ahmed Nefzaoui, made with ChatGPT image generation | Part of MDM |
| Lucide icons 1.63 (toolbar, dialogs) | Lucide Icons and contributors | ISC |
| flag-icons 7.6 (language flags) | Panayiotis Lipiridis and contributors | MIT |
| Silk icons 1.3 (categories tree) | Mark James | CC BY 2.5 |
| System fonts (Segoe UI and others) | The operating system | Used at run time, not included |

The full list, with the files each covers, is in [ATTRIBUTION.md]({{REPO}}/blob/main/ATTRIBUTION.md).

## Tools MDM downloads when you ask

| Tool | Used for | Licence |
|---|---|---|
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Finding and downloading videos on web pages | Unlicense |
| [FFmpeg](https://ffmpeg.org) (BtbN's builds) | Merging video and sound | LGPL 2.1 or later |

They aren't included in MDM's downloads. MDM fetches them from their official releases on first use and checks them before they run.

## This website

The site's charts use [Apache ECharts](https://echarts.apache.org) 5.6 (Apache License 2.0) and its text is set in [Archivo](https://github.com/Omnibus-Type/Archivo) by Omnibus-Type (SIL Open Font License 1.1), both served from this site. The language flags are the app's own, from [flag-icons](https://github.com/lipis/flag-icons) (MIT). The browser logos come from [browser-logos](https://github.com/alrra/browser-logos) (MIT); the logos themselves are trademarks of their owners and appear only to name the browsers MDM works with. Screenshots show freely available files, including the Blender Foundation's open movies (CC BY).

## Rust libraries built into MDM

MDM is written in Rust and built on the work of hundreds of open-source libraries. These are the ones compiled into the app, the command-line tool and the browser helper, with their licences, all compatible with the GPL. Each library's licence text comes with its source code.

{{THIRD_PARTY}}

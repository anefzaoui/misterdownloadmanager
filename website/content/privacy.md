---
title: Privacy policy
description: What Mister Download Manager, its browser extension and this website do with your data: no accounts, no analytics, no telemetry, no tracking.
lede: Short version: Mister Download Manager doesn't collect anything about you. No accounts, no analytics, no telemetry, no ads, no tracking.
fine: Last updated {{TODAY}} · applies to the app, the browser extension and misterdownloadmanager.com
priority: 0.4
---

<div class="summary-box" markdown="1">

## In short

- The app, the extension and this site **send nothing about you to us**. We don't run servers that could receive it.
- The app talks to **the servers you download from**, to **GitHub** to check for updates once a week (you can turn that off), and to GitHub again for **yt-dlp and ffmpeg** if you download videos.
- **Torrents** are peer to peer: other people in a torrent's swarm see your IP address, as with every torrent client.
- The **browser extension** talks only to Mister Download Manager on your own computer.
- Your download list and settings stay **on your computer**.

</div>

Mister Download Manager ("MDM") is free, open-source software made by Ahmed Nefzaoui and contributors. Because its whole source code is public at [github.com/anefzaoui/misterdownloadmanager]({{REPO}}), you don't have to take this page's word for it: every network connection described here can be checked in the code.

## The app

### What stays on your computer

Your download list, settings, logins you save in **Sites Logins**, the history of each download and the files you download are stored only on your computer: in `%APPDATA%\misterdownloadmanager` on Windows (or the `data` folder of the portable copy), and in the usual application-data folder on macOS and Linux. Nothing is uploaded anywhere. Uninstalling offers to delete them.

### Who the app talks to, and why

- **The servers you download from.** To download a file, MDM connects to the server that holds it and sends what any browser would: the request, and the cookies, referrer and browser identification the extension passed along, so downloads that need you to be signed in keep working. Those servers see your IP address, as they would if your browser downloaded the file.
- **Your proxy**, if you set one in **Options → Proxy / Socks**, and your network's proxy auto-config script, if you point MDM at one.
- **GitHub, to check for updates.** Once a week MDM asks GitHub's public API for the latest release of MDM (`api.github.com/repos/anefzaoui/misterdownloadmanager/releases/latest`). The request carries MDM's version in its User-Agent and nothing else. Turn the weekly check off in **Help → Check for updates**. GitHub's own privacy statement applies to that request.
- **GitHub, for video helpers.** If you use **Download video from a page**, MDM downloads yt-dlp (and ffmpeg, if you want merged qualities) from their official GitHub releases, checks them, and once per session asks GitHub whether yt-dlp has a newer release. yt-dlp then talks to the video site you asked for, as your browser would.
- **Torrent peers and trackers.** When you download a torrent, MDM connects to the trackers listed in the torrent, to the BitTorrent DHT network and to other peers, who see your IP address and which pieces you have. To accept connections it may ask your router to open a port (UPnP). This is how BitTorrent works; it ends when the torrent stops.

MDM has **no analytics, no crash reporting, no telemetry and no advertising**. It never sends us your download list, the addresses you download, or anything else.

### Reporting a problem

**Help → Report a problem** opens a new GitHub issue in your browser with MDM's version, your Windows version and your language filled in. Nothing is sent until you choose to submit it on GitHub, where it becomes public; read it before you post.

### The automation API

The automation API is off by default. When you turn it on, it listens only on `127.0.0.1` (your own computer) and answers only programs that present the secret token from MDM's data folder.

## The browser extension

The **Mister Download Manager Integration Module** extension for Chrome, Edge, Brave, Opera, Vivaldi and Firefox:

- When you start a download that MDM should take, it passes **the download's address, the page it came from, the site's cookies for that address and your browser's User-Agent** to MDM on your computer, through the browser's native-messaging channel. Nothing goes over the internet.
- Its **Download video** button reads the page only to find video elements and place the button. Clicking it passes the page's address to MDM.
- It stores its own settings (on or off, file types, where you dragged the video button) in the browser's extension storage, on your computer.
- It has **no remote code, no analytics and no ads**, and sends nothing to us or anyone else.

Its permissions, and why: *downloads* to see and hand over downloads; *cookies* so signed-in downloads work in MDM; *nativeMessaging* to talk to MDM; *contextMenus* for the right-click item; *storage* for its settings; access to all sites because downloads and videos can come from any site.

## This website

misterdownloadmanager.com is a static site hosted by **GitHub Pages**. We don't use cookies, analytics, ads, fonts or scripts from other companies. Your choice of light or dark mode is remembered in your browser's local storage and never sent anywhere. GitHub, as the host, receives your IP address and browser details when you visit, and may keep them in its server logs for security; see the [GitHub Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement). Download links lead to GitHub Releases.

## Children

MDM collects no personal data from anyone, children included.

## Your rights

Since we hold no personal data about you, there is nothing for us to show, correct or delete. Data that you send to GitHub (an issue, for example) is handled by GitHub and can be edited or deleted there.

## Changes

If MDM ever needs to contact a new service, this page will say so before the release that does it, and the change will be listed in [What's new](/news.html). Past versions of this page are in the project's history on GitHub.

## Questions

Open an [issue on GitHub]({{REPO}}/issues). For anything sensitive, use GitHub's private [security advisory form]({{REPO}}/security/advisories/new).

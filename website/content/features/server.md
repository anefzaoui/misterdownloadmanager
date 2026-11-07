---
name: Server mode
title: Run MDM on a home server, NAS, Docker or Unraid
headline: Let the home server do the downloading.
line: MDM without a window on a NAS, in Docker or on Unraid, fed from your phone or browser.
lede: Run MDM without a window on a home server or a NAS, and send it downloads from your phone or your browser. The files land in the server's own folders, such as a network share.
description: Run Mister Download Manager without a window on a home server, NAS, Docker or Unraid, and send it downloads from your phone or your browser.
group: network
order: 240
since: 0.5
where: The command line: `misterdownloadmanager --headless`
help: server.md
---

## The same MDM, without a window

`misterdownloadmanager --headless` runs MDM with its list, categories, queues, Scheduler, speed
limits and torrents, but no window. Its folders and port come from flags or the environment. It
stops cleanly, saving its list, when the system stops it. It runs on any Linux machine with MDM
installed, and on Windows and macOS the same way.

## Docker and Unraid

A `Dockerfile` and a Compose file come with MDM's source. For now you build the image yourself; a
published image comes later. The first build is long, up to an hour on a small server, and the
image is small, about 175 MB, with no desktop libraries.

Give it a folder for its settings and one for the downloads, where each category gets its own
folder. Mount a network share there and the files land straight on it. On Unraid, a template
maps the folders and runs MDM as `nobody:users`, as Unraid's shares expect.

## Control it from your phone or your browser

The remote control is on in this mode. The log prints the address to open and a pairing code
with a QR code: pair your phone as with the [remote control](/features/remote.html), and
`misterdownloadmanager-cli pair` makes a new code at any time.

In the browser extension, **Send downloads to → Another Mister Download Manager (a server)** sends the downloads it catches, and
**Download with Mister Download Manager** on a link, to the server instead of your computer. The
browser's cookies stay behind, so a download that needs you signed in to a site stays with your
computer.

## What a server doesn't do

Video pages and image galleries need helpers that the image doesn't include. Nothing opens a
window, so a download that would ask a question, such as a duplicate, takes the choice set in the
settings, or isn't added. Don't publish the port on the internet: reach your home network through
a VPN instead.

Server mode is new, so please [tell us how it goes](https://github.com/anefzaoui/misterdownloadmanager/issues).

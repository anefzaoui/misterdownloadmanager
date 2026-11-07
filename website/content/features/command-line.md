---
name: Command line
title: Download manager command line and automation API
headline: Scripts welcome. IDM's switches included.
line: IDM's command-line switches, a terminal downloader with JSON output, and a local API for other programs.
lede: MDM takes IDM's command-line switches, so scripts written for IDM can hand it their downloads. A separate tool downloads in a terminal, and a local API lets other programs see and control the running app.
description: Mister Download Manager's command line: IDM's /d /p /f /a /n /s switches, a terminal downloader with --json, and a local automation API.
group: app
order: 360
since: 0.1.0
where: `misterdownloadmanager-cli`, in MDM's folder
help: command-line.md
---

## IDM's switches, unchanged

The app understands the same switches as IDM: `/d` for the address, `/p` for the folder, `/f`
for the file name, `/a` to queue it without starting, `/n` to skip the **Download File Info**
window, and `/s` to start the main download queue. A batch file or another program that drives
IDM this way can drive MDM by changing the program's name.

```
misterdownloadmanager.exe /d https://example.com/files/report.pdf /p D:\Reports /n
```

If MDM is running, the command goes to it; if not, MDM starts and carries it out. On macOS and
Linux the program has the same name, without `.exe`.

## A downloader for the terminal

`misterdownloadmanager-cli` comes with every download of MDM and works without the app running.
`get` downloads a file over several connections with a progress line, **Ctrl+C** pauses it and
`resume` carries on later. `video-get` downloads a video, and `torrent-get` a torrent or only some
of its files. `video-info` and `torrent-info` say what there is to choose before you download.
`misterdownloadmanager-cli help <command>` lists every option, and a command that fails exits
with code 1.

## JSON for scripts and programs

With `--json`, every command prints JSON, one object per line: a progress line every half second
while a file downloads, then the path, size and time when it's done. Sizes are in bytes, speeds
in bytes per second, and a failure prints an error object, so a script or an AI agent reads the
result without parsing text. The [formats](command-line.md#json-for-scripts-and-programs) are
listed command by command.

## An API for the running app

Other programs can ask the running app for its downloads, as JSON, and pause, start, remove or
delete them. They connect to a port on this computer only, with a secret token that changes at
every start and that only your own programs can read. The API is off until you turn it on with
`misterdownloadmanager-cli api on`, and `status`, `pause`, `start`, `remove` and `delete` use it
from the command line.

Every request and answer is described field by field in a [JSON
Schema](/automation-api.schema.json), made from the code the app answers with, so programs can
check what they send or generate their types from it. AI assistants use the same API through
MDM's [MCP server](/features/ai-assistants.html).

## More to script

<!--@coming 0.5-->

`playlist-get` downloads a playlist's or channel's videos, and `gallery-get` a picture gallery.
`get` takes up to 128 connections with `-c`, `--dns` to choose who turns names into addresses,
and `--adapter` to send a download through one network adapter or not at all. The app's
`/startup` starts it hidden in the notification area. The API's list also gives the queues, the
categories and the free space, and it starts and stops a queue by name.

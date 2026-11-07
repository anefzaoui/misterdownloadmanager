---
name: AI assistants
title: Control your downloads from an AI assistant (MCP server)
headline: Ask your assistant. MDM does the downloading.
line: Claude, Cursor, VS Code and other MCP assistants can list, add, pause and resume your downloads.
lede: MDM comes with an MCP server, so an AI assistant that supports MCP can work with the downloads in your running MDM. You ask in your own words, and it lists, adds, pauses or resumes them.
description: Mister Download Manager's MCP server lets Claude, Cursor, VS Code and other AI assistants list, add, pause and resume downloads, on this PC only.
group: app
order: 370
since: 0.2.0
where: `misterdownloadmanager-mcp`, beside the app
help: ai-assistants.md
---

## Ask in plain words

With the server set up, you can ask your assistant things like "What is MDM downloading right
now, and how long is left?", "Download this ISO to my ISOs folder tonight, don't start it yet",
or "Remove the finished downloads from the list but keep the files". The assistant turns that
into requests to MDM and tells you what happened.

It works with assistants that support MCP, the Model Context Protocol: Claude Desktop, Claude
Code, Cursor, VS Code and others.

## What it can do, and what it can't

The assistant can list your downloads with their status, size, progress and speed, and filter
them by status or by name. It can add an http, https, ftp or ftps address and start it or only
queue it, in the folder and under the name you say. A magnet link opens **Add torrent** in MDM,
for you to choose the files. It can pause, resume and remove downloads, deleting the files only
when asked, and start the main download queue.

It can't change MDM's options, see your browser, or turn on MDM's automation API by itself. MDM
has to be running: the server can't start it for you.

## Set up once

The server, `misterdownloadmanager-mcp`, sits next to the app on Windows, macOS and Linux (on
Linux use the `.deb`, `.rpm` or `.tar.gz`: the AppImage doesn't expose it). You turn on the
automation API once with `misterdownloadmanager-cli api on`, then tell your assistant where the
server is. The assistant starts it by itself from then on. The [set-up
guide](ai-assistants.md#set-it-up) has the path for each kind of install and the exact entry for
Claude Desktop, Claude Code, Cursor and VS Code.

Without the API, the assistant can still add downloads, as MDM's [command
line](/features/command-line.html) can.

## Know where your data goes

MDM itself still sends nothing anywhere: the server talks only to the MDM on this computer, over
a token-protected local channel. But what the assistant reads, it passes on to its own AI
service. When it lists your downloads, their file names, addresses and folders go to that
service, under that service's privacy terms. Turn the API off with `misterdownloadmanager-cli
api off`, or remove the server from the assistant, when you don't want that.

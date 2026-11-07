---
name: Speed limiter
title: Download speed limiter
headline: Fast downloads that leave room for everyone else.
line: One cap for all of MDM, a limit per file, and full speed when you're away.
lede: A download manager can fill your whole connection. The Speed Limiter caps MDM so that browsing, video calls and games stay smooth, and can give it full speed again while you're away from the computer.
description: The speed limiter in Mister Download Manager: one cap for all downloads, a limit per file or per download, and full speed when you're away.
group: organising
order: 120
since: 0.1.0
shot: images/help/speed-limiter.webp
shot_alt: Speed Limiter settings: Maximum download speed for one file, Always turn on Speed Limiter on startup, and Limit all downloads together to, with Only while I'm using the computer
where: Downloads → Speed Limiter → Settings...
help: connections-and-speed.md#speed-limiter
---

## One cap for all of MDM

In **Downloads → Speed Limiter → Settings...**, tick **Limit all downloads together to** and type
a speed in KBytes/sec. However many files are downloading, together they stay under it. It works
on its own: there is nothing else to turn on. About two thirds of your connection's speed is a
good start.

Tick **Only while I'm using the computer** and the cap applies only while you use the mouse or
keyboard. After a minute without input, MDM goes back to full speed.

## A limit per file, or for one download

**Maximum download speed for one file** holds each download to the same speed. Set it first,
since it starts at a very slow 10 KBytes/sec, then choose **Downloads → Speed Limiter → Turn
on**. **Turn off** gives full speed back, and **Always turn on Speed Limiter on Mister Download
Manager startup** keeps it on from the start.

To slow down a single download, open its progress window and the **Speed Limiter** tab. The new
limit applies at once, and the other downloads keep their speed. With **Remember Speed Limiter
settings for this file on download stop/resume**, the limit comes back when you resume the file,
even after restarting MDM.

Some servers drop the connection when the speed is very low. Avoid a tight limit for a server
that can't resume: the progress window shows **Resume capability: No** for those.

## Torrents share the cap fairly

<!--@coming 0.5-->

Each torrent gets a limit of its own, like any other download, and a remembered limit applies
again when it resumes. Under **Limit all downloads together to**, torrents and other downloads
share the cap: each side gets at least half, and more when the other side needs less. A torrent
can't be held under 16 KBytes/sec, because a lower limit made torrents drop their peers and
stall.

## Capped by data, not by speed

On an internet plan with a data cap, the amount matters more than the speed. The Scheduler's
**Download limits** stops MDM after a set number of megabytes every few hours, and lets it carry
on when the period is over: see [Scheduler and queues](/features/scheduler.html).

[Keep some speed for everything else](guide-keep-speed-for-everything-else.md) walks through the
settings step by step.

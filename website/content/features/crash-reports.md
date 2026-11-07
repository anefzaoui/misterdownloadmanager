---
name: Crash reports
title: Crash and freeze reports you read before you send
headline: When something breaks, you see why.
line: A crash or freeze opens a report you read first, then send on GitHub yourself if you want.
lede: If MDM closes unexpectedly or stops responding, a window opens with a report of what happened. You can read all of it, and send it to the developers on GitHub if you want to.
description: Mister Download Manager's crash and freeze reports: readable, free of your addresses and file names, and sent only by you, through your browser and GitHub.
group: app
order: 320
since: 0.2.3
shot: images/crash-report.webp
shot_alt: The crash report window after MDM stopped responding
where: Opens by itself when MDM crashes or stops responding
help: crash-reports.md
---

## A crash or a freeze doesn't go unnoticed

A second copy of MDM watches the first. When MDM crashes, the report window says **Mister
Download Manager closed unexpectedly**. When its window hasn't answered for 8 seconds, it says
**Mister Download Manager stopped responding**, and **End and restart MDM** closes the frozen
copy and starts a fresh one. If MDM comes back by itself, the window says so.

The report names what happened and where in MDM's code, so the problem can be found and fixed.
It works on Windows, Linux and macOS.

## You read it first, and you send it

Everything that would be sent is in the window, under **The report**. You can add a few words
under **What were you doing when it happened?**, such as "I opened Add URL".

**Send on GitHub** opens GitHub's crash report form in your browser, already filled in, with the
folder that holds the report. You check it and submit it. That needs a GitHub account. **Nothing
is sent by itself**: there is no background upload and no reporting service, only your browser
and GitHub.

If someone already reported the same problem, the window says so and **Open the reported issue**
takes you there, with your report on the clipboard to add as a comment.

## What it leaves out

A report holds the error, where it happened, MDM's recent log, and your system: MDM's version,
how it was installed, your operating system, the processor, the language and theme, and how many
downloads and queues you have, as counts only.

It does not hold your download addresses or file names, and your user name shows as `~` in
folder paths. If an error message quotes something you typed, that text is in it, which is why
you read it first.

The crash data file (`.dmp`) helps the developers most, but it can hold pieces of what MDM had in
memory, such as addresses, cookies and saved passwords. It is attached only when you tick **Also
attach the crash data**.

## Freeze reports that tell the kind of freeze

<!--@coming 0.5-->

A freeze report will say whether MDM's window ran all along (a loop), waited (on a lock, the disk
or the network), or hardly ran at all. That last kind usually means a computer too busy to run
MDM, most likely not a bug in MDM, and those reports share one signature so they find one issue
instead of many. macOS doesn't measure this yet. The window's title will be in MDM's language
too, and the report will keep its own lines in every language, left to right in Arabic and
Persian.

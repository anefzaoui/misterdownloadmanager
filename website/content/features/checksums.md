---
name: Verify checksum
title: Verify a download's checksum (MD5, SHA-256)
headline: Make sure it's the file they published.
line: Paste the checksum a site publishes, and MDM says plainly whether your file matches it.
lede: Many sites publish a checksum next to a download. Paste it into MDM and it tells you in plain words whether the file you got is the one they meant.
description: Check a download against its published MD5, SHA-1, SHA-256, SHA-512 or CRC32 checksum in Mister Download Manager, from the list or the command line.
group: organising
order: 150
since: 0.1.0
where: Right-click a download → Verify checksum...
help: download-list.md#verify-checksum
---

## Paste it, get a plain answer

A checksum is a short code worked out from a file's contents, published so you can tell that
the copy you downloaded is the right one. Right-click a download in the list, choose **Verify
checksum...** and paste the code from the site. MDM answers **The file matches** or **The file
does NOT match**. Nothing to install, and no command to type.

## The common kinds, read as they are

MDM reads MD5, SHA-1, SHA-256, SHA-512 and CRC32 checksums. Paste the code as the site shows it:
there is no list to pick the kind from, as MDM tells them apart by their length.

## Ask before it's done

A finished file is checked at once. Ask while it is still downloading, and MDM checks it when the
download ends. You can paste the code as soon as the download starts, and come back to the
answer.

## From the command line too

The command line's `get` takes `--checksum` and checks the finished file: the code in hex, on its
own or named, as in `sha256:…`. One command downloads the file and checks it:

```
misterdownloadmanager-cli get https://example.com/big.iso -o D:\Downloads --checksum sha256:9f86d0…
```

The [command line guide](command-line.md) lists everything `get` takes.

## MDM checks its own downloads too

The same care goes into what MDM fetches for itself. The helpers it can install for videos are
verified before they run: yt-dlp against its developers' signing key, ffmpeg against its
published checksums. MDM's own updates install only files listed in a checksum list signed with
MDM's release key, which every release carries.

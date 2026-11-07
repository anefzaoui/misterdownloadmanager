---
name: Image galleries
title: Download image galleries with gallery-dl
headline: A whole gallery at once. Only new pictures next time.
line: Paste a gallery's address and MDM saves all its pictures into a folder of their own.
lede: MDM downloads the pictures of a gallery, a Flickr album, a DeviantArt gallery, a Wikimedia Commons category and many more, with gallery-dl, a free program that reads several hundred sites.
description: Download whole image galleries from Flickr, DeviantArt, Imgur, Wikimedia Commons and more with Mister Download Manager and gallery-dl.
group: downloading
order: 40
since: 0.5
shot: images/help/site-gallery.webp
shot_alt: Download a gallery: a gallery's address, gallery-dl offered for install, the folder its files go to and how many at most
where: Tasks → Add new download, with a gallery's address
help: galleries.md
---

## Paste the address, get the pictures

Click **Add URL** and paste a gallery's address. Instead of saving the page as a file, MDM opens
**Download a gallery**. When its line reads **gallery-dl can download this gallery's files**,
check **Save to** and click **Download Now**, or **Later** to add it without starting.

The pictures go into a folder of their own, named after the gallery. Want only a few? Type a
number in **Files at most** to take the first ones the site lists, on most sites the newest.

The gallery gets one row in your download list. Its size grows as the pictures arrive, and its
log says how many were saved.

## Next time, only what is new

Start a finished gallery again with **Resume** and MDM fetches only the pictures that aren't in
its folder yet. The ones you already have are skipped, so following an artist or an album costs
nothing but the new pictures.

## Which sites

Add URL sends these to the gallery window: Wikimedia Commons categories, Flickr photographers and
albums, DeviantArt and ArtStation galleries, Imgur albums, Pixiv artists and works, Pinterest
boards and pins, Danbooru and Gelbooru searches, Tumblr blogs and Bluesky profiles. A picture's
own address, one ending in `.jpg` or `.png`, still downloads as a single file.

## A helper you can check

gallery-dl is an optional helper, like yt-dlp for [videos](/features/video.html). On Windows and
Linux, MDM installs it the first time you need it, from its releases on Codeberg, and only when
the release is signed by its author's key. On macOS, install it with Homebrew or pip and choose
it in **Helpers...**, which also takes your own copy on any system, such as the gallery-dl your
Linux distribution ships (version 1.20 or later).

It asks sites for one picture a second at most, and names itself as MDM, because some servers,
Wikimedia's among them, refuse tools that ask faster or without a name. It uses no
configuration file of yours.

Two things to know: **Stop** keeps the pictures saved so far, and **Delete** takes the gallery
off the list but leaves its folder and pictures on disk. Delete the folder yourself if you want
it gone.

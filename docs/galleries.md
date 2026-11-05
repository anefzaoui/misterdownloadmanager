# Image galleries

MDM downloads the pictures of a gallery (a Wikimedia Commons category, a Flickr album, a DeviantArt gallery, an Imgur album and many more) with [gallery-dl](https://codeberg.org/mikf/gallery-dl), a free program that reads several hundred sites. Like yt-dlp for videos, it's an optional helper: MDM installs it the first time you need it, or uses your own copy.

## Download a gallery

1. Click **Add URL** and paste the gallery's address, for example `https://commons.wikimedia.org/wiki/Category:Featured_pictures_of_birds`.
2. MDM opens **Download a gallery** instead of downloading the page as a file. The first time, click **Install gallery-dl**.
3. gallery-dl checks the address on your computer, without going online. When the line reads **gallery-dl can download this gallery's files**, you're ready.
4. Check **Save to** (the General category's folder by default). The pictures go into a folder of their own there, named after the gallery.
5. Optionally, type a number in **Files at most** to download only the first ones the site lists. On most sites those are the newest. Leave it empty for all of them.
6. Click **Download Now**, or **Later** to add it to the list without starting.

The gallery gets one row in the list. Its size grows as the pictures arrive, and its log says how many were saved. Each picture is marked as downloaded from the internet, as any download is.

## Get the new pictures later

Start a finished gallery again (**Resume**) to fetch only the pictures that aren't in its folder yet. gallery-dl skips the ones already there.

## Stop, delete

- **Stop** ends gallery-dl and everything it started. The pictures saved so far stay.
- **Delete** takes the gallery off the list. Its folder and pictures stay, even if you tick the box to delete files: delete the folder yourself if you want it gone.

## Which addresses open the gallery window

Add URL sends these to the gallery window:
- Wikimedia Commons categories;
- Flickr photographers and albums;
- DeviantArt and ArtStation galleries;
- Imgur albums;
- Pixiv artists and works;
- Pinterest boards and pins;
- Danbooru and Gelbooru searches;
- Tumblr blogs;
- Bluesky profiles.

A picture's own address (one ending in `.jpg` or `.png`) downloads as a file, as before.

## gallery-dl itself

- **Where it comes from:** MDM downloads gallery-dl from its releases on Codeberg. It accepts it only when the release's checksum list is signed by gallery-dl's author's key, pinned in MDM, and the program matches that list. It keeps the previous version and never goes back to an older one. On Windows and on Linux (x86-64) MDM installs it; on macOS, install it with Homebrew (`brew install gallery-dl`) or pip and choose it in **Helpers...**.
- **Your own copy:** **Helpers...** (in the gallery window or the video window) lists yt-dlp, ffmpeg and gallery-dl, and **Use another program...** takes your own copy of each instead of MDM's.
- **How it asks:** gallery-dl asks sites as MDM, one picture a second at most. Wikimedia's servers refuse tools that ask faster or without a name. It uses no configuration file of yours.
- **Network adapter:** like yt-dlp, gallery-dl can't be held to one network adapter. While one is chosen (**Options → Proxy / Socks → Network adapter**), galleries don't start.

## On the command line

- `install-gallery-dl` installs or updates gallery-dl, verified in the same way.
- `gallery-get URL [-o folder] [--limit N]` downloads a gallery into a folder named after it. See [Command line and automation](command-line.md).

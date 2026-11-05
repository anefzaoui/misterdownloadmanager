# Changelog

Each release's notes on GitHub come from its section here (`## [X.Y.Z]`, see
`.github/workflows/release.yml`).

## [Unreleased]

### Security

- **Torrents: file names are checked more strictly** ([#40](https://github.com/anefzaoui/misterdownloadmanager/issues/40)):
  a torrent with names the system can't write safely is refused, saying which name, and so are
  torrents kept from an earlier run.
- The torrent library is updated (librqbit 9.1.0).
- **No silent overwrite**: when some of a torrent's files are already in the folder, MDM asks
  before writing into them (**No** is the default). The command line's `torrent-get` stops and
  names them, unless given `--use-existing`.
- **Files from torrents are marked as downloaded from the internet**, as web downloads are:
  SmartScreen checks a program before it first runs, Office opens documents in Protected View,
  and Gatekeeper checks apps on a Mac.
- Torrents' trackers on this computer or the local network aren't contacted, and a `.torrent`
  file over 10 MB isn't read (it isn't one). Peers see MDM's major version only.

### Added

- **The extension lists the media a page has loaded**
  ([#38](https://github.com/anefzaoui/misterdownloadmanager/issues/38)). The toolbar button's
  window has **Media on this page**: the HLS and DASH streams the page's players loaded (each
  once, with its qualities and length) and the video and sound files it played, with their size.
  **Download** sends a stream to the Download video window and a file to the list, with the
  page's cookies, the page as the referrer and the browser's User-Agent, so a stream a page won't
  give up can still be saved. Copy-protected (DRM) streams are listed as **Protected** and never
  sent. Nothing new to allow: the extension already watched the tab's responses.
- **Link grabber, and archives extracted when they are downloaded**
  ([#37](https://github.com/anefzaoui/misterdownloadmanager/issues/37)). The window that took
  the browser's "Download all links", Tasks → Import and dropped lists grew into Link grabber
  (also Tasks → Link grabber..., where pasted links go). Links come in packages: a split
  archive's volumes together, the other links of a page together, the rest in "Various files".
  Each link is looked up for its name, size and whether the server has it (Offline links come
  unticked), and each package has its folder, an archive password, "Extract when done" and
  "Delete archives after extracting". **Start** or **Add to queue** (IDM's import, unchanged:
  an `.ef2` file still brings each download's referrer and cookies).
  MDM extracts ZIP (AES and Deflate64 too), TAR, TAR.GZ, TAR.XZ, GZ and XZ itself; 7z and RAR
  need 7-Zip (split `.7z.001` and `.partN.rar` too), and a window says so when it isn't
  installed. It extracts when a package says so,
  for any download with **Options → Archives → Extract archives when their download completes**
  (off by default), or with right-click → **Extract**. A split archive is extracted once its last
  volume is in. Extracted files are marked as downloaded from the internet, as the archive is;
  names that would leave the folder, links, and archives that would fill the disk are refused;
  an existing file is never overwritten. The archive goes to the Recycle Bin (the Trash) only
  when you ask. Options has a new **Archives** page.
- **Real-Debrid accounts, experimental** ([#44](https://github.com/anefzaoui/misterdownloadmanager/issues/44)):
  Options → Accounts keeps your debrid service account. Sign in by code in the browser, or with
  your API token. A link from a file host your service supports is unlocked through your account,
  then downloaded with all of MDM's connections, pause and resume, from Add URL, the browser's
  link menu and batch downloads. When the direct link expires, MDM asks the
  service for a new one. The direct link is never shown or copied, and the sign-in is sealed for
  your Windows account. AllDebrid and Premiumize come next. See
  [Debrid service accounts](docs/debrid.md). Experimental: built from Real-Debrid's public API
  documentation, not yet confirmed with a real account.
- The proxy's password in Options → Proxy / Socks is now sealed on disk too on Windows, like
  Sites Logins' passwords.
- **Image galleries** ([#58](https://github.com/anefzaoui/misterdownloadmanager/issues/58)): a
  gallery's address in Add URL (a Wikimedia Commons category, a Flickr album, a DeviantArt or
  Imgur gallery and more) opens **Download a gallery**, which downloads its pictures with
  gallery-dl into a folder of their own, all of them or the first N. Started again, a gallery
  fetches only its new pictures. gallery-dl is an optional helper like yt-dlp: installed from its
  releases only when signed by its author's key, or your own copy (Helpers). On the command line:
  `install-gallery-dl` and `gallery-get`.
- **RSS feeds** ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39)): Tasks → RSS
  feeds... watches RSS and Atom feeds of torrents, lists their articles (download one by hand), and
  its download rules add the new ones whose titles they match: words or regular expressions, words
  to skip, an episode filter (`1x2;1x5-8;2x1-`) that takes each episode once, a folder per rule.
  A feed's backlog is never downloaded by itself.
- **Torrent commands** ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39)): a
  torrent's right-click menu has **Torrent → Recheck files**, **Reannounce**, **Add trackers...**
  and **Copy magnet link**. Its Files tab can tick and untick files while it runs. A `.torrent`
  file's web address, in Add URL or from the browser, opens Add torrent with what the file holds.
  TORRENT is in Options → File types' defaults, so the browser hands `.torrent` files over; a
  list saved before keeps what it had.
- **Torrents: seeding limits** ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39)):
  under "Keep sharing torrents after they finish", Torrent settings can stop sharing a finished
  torrent at a share ratio, or after it has been shared for some minutes; both off by default. A
  torrent stopped so says it in its event log and isn't shared again at the next start.
- **The built-in categories are yours to change** ([#93](https://github.com/anefzaoui/misterdownloadmanager/issues/93)):
  Compressed, Documents, Music, Programs and Video can be renamed, given other file types and
  deleted, like your own categories (right-click → Properties or Delete). A renamed category keeps
  its folder. Deleting one asks what happens to its downloads: move them to General, or take them
  off the list; their files stay either way. **Options → Save to → Restore the built-in
  categories** brings them back as they came. General and Torrents still change only their folder.
- **Downloads from a browser behind a VPN or proxy extension go through it too**
  ([#42](https://github.com/anefzaoui/misterdownloadmanager/issues/42)): the browser extension
  passes the proxy the browser uses for a download (an extension's, or its settings' manual proxy
  or auto-config script) with it. MDM downloads through the same proxy while Options → Proxy /
  Socks → "Use proxy/socks from a browser's request" is on, which is the default. A proxy that
  needs the VPN extension's own login can't be shared: the download says so rather than going
  without it. The extension asks for the browser's *proxy* permission for this, to read the
  browser's proxy, never to change it.
- **Hide to the notification area** ([#63](https://github.com/anefzaoui/misterdownloadmanager/issues/63)):
  a download's progress window has a button that hides it in the tray while the download goes
  on: on Windows in its title bar, left of Minimize (the progress window draws its own Windows 11
  title bar now, keeping the system's snapping, shaking, menu and shadow), and in its Alt+Space
  menu; on Linux and macOS at the end of its tab row. The tray menu's new **Downloads in
  progress** lists the running downloads to bring one's window back (double-clicking it in the
  list does too). With
  **Options → General → Minimize to the notification area** (on by default), Minimize hides the
  main window and progress windows there as well. Only where a tray shows MDM's icon.
- **Playlists and channels** ([#69](https://github.com/anefzaoui/misterdownloadmanager/issues/69)):
  Download video from a page lists a playlist's or a channel's videos, with their lengths, all
  ticked, with Select all and Select none. One quality applies to all of them (the best, up to a
  height, the original sound, MP3, Opus or FLAC). Each video gets its own download, in a folder named after
  the list, and Download Now starts the main queue, which runs as many at once as its settings
  say. A playlist is listed as its videos come in; the listing can be stopped and taken up again
  (List the rest), and one the site breaks off says after how many videos and why. A channel is
  listed 50 videos at a time, with Load more. Lists of thousands stay quick, and adding more than
  500 downloads asks first. On the command line: `playlist-info` and `playlist-get`.
- **Choose the DNS** ([#36](https://github.com/anefzaoui/misterdownloadmanager/issues/36)):
  Options → Connection → DNS picks who turns names into addresses for downloads, FTP, MDM's own
  downloads and the update check. The system's (the default), Cloudflare, Google, Quad9, AdGuard,
  NextDNS (with your configuration ID) or your own, asked over HTTPS (the default), TLS, or
  plain over TCP. **Test** shows how long it takes to answer. When it can't be reached, the
  download stops with a message naming it, unless you allow falling back to the system's DNS;
  your hosts file and local names stay with the system's. The command line's `get` takes
  `--dns`. Video pages and torrent trackers still use the system's DNS.
- **Downloads through one network adapter, with a kill switch**
  ([#35](https://github.com/anefzaoui/misterdownloadmanager/issues/35)): Options → Proxy / Socks
  → Network adapter. Pick one, usually your VPN's, and every connection MDM opens goes out
  through it or not at all: downloads, FTP, the DNS you chose, MDM's own downloads and the update
  check. When the adapter disconnects, nothing goes out through your normal connection. Running
  downloads stop and start again when it's back. Video pages, torrents on Windows and `ftps://`
  can't be held to an adapter yet, so they don't start while one is chosen. The command line's `get` takes
  `--adapter`. PAC scripts' `myIpAddress` no longer opens a network socket on Windows.
- **Drop-down lists for screen readers**: a screen reader or UI Automation can now set a
  drop-down list's choice by its text, and open or close the list.
- **Torrents: upload limits, and a share ratio that lasts**
  ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39)): Torrent settings can
  limit what all torrents together send, and a torrent's progress window can limit its own, kept
  with it. Its General tab now counts what it sent and how long it shared over all its runs, so
  the share ratio no longer starts again at 0 after a pause or a restart.
- **More audio choices for a video** ([#70](https://github.com/anefzaoui/misterdownloadmanager/issues/70)):
  under the MP3 ones, **Audio only OPUS** (YouTube's Opus is only moved into an `.opus` file, not
  converted) and **Audio only FLAC** (lossless, with an honest note in the window: it can't add
  quality the site's lossy sound doesn't have, the file is only bigger). The site's own sound is
  now named **Audio only, original**. The command line's `video-get` takes `-f opus:` and
  `-f flac:`.
- **A file-type icon before every file name or folder**
  ([#122](https://github.com/anefzaoui/misterdownloadmanager/issues/122)): the download list,
  Download File Info, Properties, Download complete (its open folder), Add torrent (the torrent and
  its tree), a torrent's Files tab, the progress window's Save To, the Scheduler's files, Batch
  download's preview, and Download video's qualities and a playlist's videos (the file each one
  gives). Each is your
  system's own icon for that type, the one its file manager shows: Windows Explorer's, macOS
  Finder's, or your Linux desktop's icon theme (KDE's, GNOME's or GTK's choice, then the themes it
  builds on). It is sharp at every display scale (125, 150, 200 %), taken at the size the screen
  needs. Each type is looked up once, in the background, so a long list never waits for it. Where
  the system has no icon for a type (a Linux desktop without an icon theme), MDM draws one of its
  own, from the Tango icons (public domain).
- **Video subtitles** ([#68](https://github.com/anefzaoui/misterdownloadmanager/issues/68)):
  Download video from a page lists the video's subtitle languages, the uploader's first, then the
  site's automatic captions (in the language spoken, English and yours). They're saved beside the
  video as `<name>.<language>.srt`, or put into the video with ffmpeg (MP4, MKV), and the choice is
  offered again for the next video. Subtitles a site refuses (YouTube limits how often they may
  be fetched) never cost the video: MDM asks once more after a pause, then says which are missing.
  On the command line: `video-get --subs`, `--auto-subs`, `--embed-subs`; `video-info` lists them. A finished video and its subtitles are now marked as
  downloaded from the internet, as other downloads are.
- **View → Always dark** ([#60](https://github.com/anefzaoui/misterdownloadmanager/issues/60)):
  MDM stays dark whatever the system says, for a desktop that reports no light or dark
  preference (MDM also reads it on Linux now, see Fixed).
- **Torrent settings: the network is yours to choose**
  ([#40](https://github.com/anefzaoui/misterdownloadmanager/issues/40)): accept incoming connections
  or not, map the port with UPnP or not, a fixed port, the DHT, finding peers on the local network,
  a SOCKS5 proxy from Proxy / Socks (which turns off what would go around it), an IP blocklist
  (P2P or eMule format, fetched again every 7 days), and stopping the torrent engine when no
  torrent runs. A new help page, [Torrents and your security](docs/torrent-security.md), says what
  others can see and what each switch does.
- **Your own yt-dlp and ffmpeg** ([#51](https://github.com/anefzaoui/misterdownloadmanager/issues/51)):
  **Download video from a page → Helpers...** shows where each comes from, and **Use another
  program...** takes a copy you already have instead of MDM's own (checked once, then used by
  the window and the command line alike; MDM leaves its updates to you). **Use MDM's own copy**
  goes back.
- **Zen, Helium and other browsers built on Firefox or Chromium** ([#41](https://github.com/anefzaoui/misterdownloadmanager/issues/41)):
  the extension reaches MDM in them, also when one is the only browser on Linux or macOS (Zen
  reads Firefox's folder, which MDM now creates; Helium its own). **Add Mister Download Manager
  to your browser** lists them, Zen, LibreWolf, Floorp and Waterfox by their own names (Options
  too), and sends Firefox's family to the right page. On Windows MDM lists the browsers Windows
  itself lists, so Helium is no longer taken for Chrome. Flatpak browsers can't start MDM's
  helper, which the help page now says.
- **A video's sound as MP3, at the quality you pick** ([#32](https://github.com/anefzaoui/misterdownloadmanager/issues/32)):
  **Download video** lists **Audio only MP3** at its best variable bitrate (VBR) and at 320, 256,
  192 and 128 kbps, after the sound in the site's own format. ffmpeg converts it (on Windows MDM
  offers to install it, as for the best video qualities; on Linux and macOS it uses the system's),
  and the browser's **Download video** button offers the same. The command line takes
  `video-get --format mp3:192` (`mp3:0` for VBR).
- **The Linux AppImage updates through AppImage tools too** ([#24](https://github.com/anefzaoui/misterdownloadmanager/issues/24)):
  it carries the address of its updates, and each release has its `.AppImage.zsync`, so
  AppImageLauncher, Gear Lever and AppImageUpdate download only the parts that changed.
  **Update now** works as before.
- **More than 32 connections per download, if you want them** ([#12](https://github.com/anefzaoui/misterdownloadmanager/issues/12)):
  **Options → Connection** now takes any number from 1 to 32, typed or set with the arrows,
  and **Allow more than 32 connections** raises that to 128, for server exceptions too. Servers
  that allow fewer are still noticed and asked for fewer. The command line's `-c` takes 1 to 128.
- **MDM offers the browser extension**: until a browser's extension has reached MDM, each start
  shows **Add Mister Download Manager to your browser**, with a button for every browser
  installed (the tutorial until the extension is listed in the Chrome Web Store, its store page
  after). The window says when the extension is connected; **Don't ask again** stops it.
- The store's copy shows no Download video button on YouTube, whose downloads the store's
  policy forbids; **Tasks → Download video from a page** still takes any page.
- **The extension's toolbar button** says whether the browser reaches MDM. When it doesn't, a
  red **!** and **How to fix it** lead to the new help page, [The browser and MDM aren't
  connected](https://misterdownloadmanager.com/help/browser-help.html), at the section for the
  reason. Its popup also pauses taking downloads, leaves the current site to the browser for
  good, and opens MDM or its Options.
- **Options → General → Test browser integration** checks MDM's side in order (integration on,
  the installed copy, the browsers can find MDM, which browsers' extension connected) and
  offers **Set it up** when the browsers can't find MDM.
- **Options → File types → Don't take files smaller than** leaves small files (a receipt, an
  icon) to the browser, and **Edit list ...** for the addresses left to the browser works.
- **Download selected links** and **Download all links** on the browser's right-click menu: the
  page's links go to Import, to tick there.
- **PDFs, music and videos the browser would open itself are taken too**, when their type is in
  **Options → File types** (**Options → General → Keys...** turns it off). Hold **Alt** while
  clicking a link to let the browser have it.
- **Downloads from forms** (a button that posts and gets a file back) are posted again by MDM,
  with the form's data, instead of being lost. A form that sends a file (multipart) stays with
  the browser.
- **Google Drive links** ([#34](https://github.com/anefzaoui/misterdownloadmanager/issues/34)):
  paste a Drive file's link (`drive.google.com/file/d/…/view`, `…/uc?id=…`, `…/open?id=…`) and
  MDM downloads the file itself, as it was uploaded, over several connections and resumable,
  with its real name and size in Download File Info. Large files get past Drive's "can't scan
  this file for viruses" page as **Download anyway** does. Google Docs, Sheets and Slides come
  as Word, Excel and PowerPoint files. A private file downloads with the sign-in the extension
  passes from the browser. When Drive won't give a file (too many downloads lately, sign-in
  needed, no access), MDM says so in your language instead of saving Drive's page. The video
  button over a Drive video downloads the original file. Folders aren't downloaded yet: MDM
  says the link is a folder. See [Google Drive links](docs/adding-downloads.md#google-drive-links).

### Changed

- **Developers: two engine tests no longer fail now and then**
  ([#123](https://github.com/anefzaoui/misterdownloadmanager/issues/123)): the pause-and-resume
  and event-log tests no longer depend on how fast the machine is. The lab server can hold a
  byte back until a test lets it go (`stall_at`), and a test can wait until every request it
  made is in the server's log.
- **Torrents behind a proxy open no UDP socket toward the network**: UDP trackers were already
  left out there; now their client stays on this computer too.
- **Developers: tests no longer make Windows Firewall ask about them.** The torrent tests and
  test tools listen on 127.0.0.1 only (a new setting of the torrent engine, its listen address),
  so each new build of a test no longer opens sockets on every address.
- **Less memory in the notification area** ([#72](https://github.com/anefzaoui/misterdownloadmanager/issues/72)):
  closed to the tray, MDM gives the memory it isn't using back to Windows. Task Manager's
  **Memory** drops from about 50 MB to under 10 MB (with 500 downloads in the list, from 58 to
  about 14 MB), and is given back again every 10 minutes while MDM stays there. After the
  language list (which needs a font for every script) closes, those fonts are let go: about 70 MB
  less than before. On Linux the heap's free memory goes back to the system the same way.
  [MDM uses a lot of memory](docs/troubleshooting.md#mdm-uses-a-lot-of-memory) explains
  Task Manager's figures.
- **Freeze reports say what kind of freeze it was** ([#57](https://github.com/anefzaoui/misterdownloadmanager/issues/57)):
  MDM measures how much processor time the window got while it didn't answer. The report then
  says whether it ran all along (a loop), waited (a deadlock, or the disk or the network), or
  hardly ran at all, which means a computer too busy to run MDM or slow to load it from disk,
  most likely not a bug in MDM. Those last ones share one signature, so they find one issue
  instead of many.
- **MDM's programs are a third of their size, its download about half** ([#25](https://github.com/anefzaoui/misterdownloadmanager/issues/25)):
  on Windows, the programs went from 93 MB to 33 MB (`misterdownloadmanager.exe` from 61 MB to
  29 MB, the command line from 28 MB to a 0.2 MB launcher), and setup.exe from 23 MB to 12 MB.
  On Linux the AppImage went from 36 MB to 16 MB, the .deb from 27 MB to 13 MB.
  - The command line runs in the app's own program, so the download engine ships once. Every
    command, `--json`, Ctrl+C and exit code work as before.
  - The translations (85 languages besides English) are packed, 12 MB of text in under 1 MB
    without the translators' notes, and unpacked once at start.
  - Release signatures are checked by MDM's own small verifier instead of a full OpenPGP
    library.
  - The code that draws the windows is optimised for size (the engine stays optimised for
    speed), and the programs are optimised as a whole.
  - Crash reports name functions from the release's symbol files only, without the debug-info
    reader a build from source can add.
  - The installer packs tighter, and MDM's own code no longer carries the build machine's
    folder paths (a bundled C library still keeps a few of its own).
- **Two smaller macOS downloads** ([#25](https://github.com/anefzaoui/misterdownloadmanager/issues/25)):
  a .dmg for Apple Silicon (`-macos-arm64`, about 10 MB) and one for Intel (`-macos-x86_64`,
  about 13 MB) instead of one universal .dmg of 80 MB. From 0.2.4 or older, update once by hand:
  **Download** in Check for updates opens the release; pick your Mac's .dmg. **Update now**
  works again after that.
- **Proxy auto-config scripts (PAC) run in QuickJS**, a much smaller JavaScript engine, with
  limits on memory and running time: a script that loops or grows forever now fails, as a broken
  script does (the download goes on with the default proxy settings), instead of holding it.
- The extension is called **Mister Download Manager**.
- A download from the browser keeps the browser's name, size and type: Download File Info no
  longer asks the server again, so a link's own name (`download="…"`), a Unicode name and a
  download behind a sign-in show right away, and correctly. When it must ask (an address typed
  in), it asks with the cookies, referrer and User-Agent the download will use.
- The address the browser was given is kept along with where its redirects led, so an expired
  signed link (a GitHub release, a cloud drive) is renewed when resumed, and a site excluded in
  Options is left alone even when it redirects to another.
- The download button over videos passes the page's referrer, cookies and the browser's
  User-Agent to yt-dlp, so videos that need a sign-in or play only where they're embedded work.
- Files downloaded from the web are marked as downloaded from the internet, as the browser marks
  its own: Windows' Mark of the Web (SmartScreen, Office's Protected View), macOS' quarantine.
- Downloads from a private window are kept out of the saved list.
- **MDM follows every kind of redirect a browser does** ([#28](https://github.com/anefzaoui/misterdownloadmanager/issues/28)):
  any HTTP redirect (301, 302, 303, 307, 308), up to 20 in a row instead of 10, and the pages
  download sites put in between ("your download starts in a moment"), which lead on with a
  `Refresh` header or a `<meta http-equiv="refresh">`: their wait is kept (up to 10 seconds), and
  the cookies they set go along while the download runs (not into a resume later). A page that
  leads on only through its own script still stops there.

### Fixed

- **Numbers are written the interface language's way** ([#128](https://github.com/anefzaoui/misterdownloadmanager/issues/128)):
  counts, sizes, speeds, percentages and the list's columns group and point their digits as the
  chosen language does (German `10.200` and `1,50 MB`, French `10 200`, Hindi `1,00,000`),
  whatever the Windows region; a region in the same language keeps its own way (Swiss German
  `10’200`). English is unchanged.
- **A finished torrent's progress window lost its tabs while it was still shared**: Files, Peers,
  Trackers, Pieces and General (with the share ratio) said "Shown while the torrent is
  downloading." They now stay for as long as the torrent is shared.
- **"Hide tab" was cut in longer languages** (German "Registerkarte au..."): the button grows to
  fit its label where there is room, through a shared helper other buttons can use.
- **Screen readers can reach the Categories pane and the menus** ([#113](https://github.com/anefzaoui/misterdownloadmanager/issues/113)):
  the pane reads as a tree (each category named with its count, open or shut, selected), the menu
  bar and every right-click menu as menus whose items can be run, and **Shift+F10** or the
  **Menu** key opens the selected category's menu. A menu opened from the keyboard no longer stays
  invisible until the mouse moves.
- **A download's progress file could be written again just after it stopped**
  ([#106](https://github.com/anefzaoui/misterdownloadmanager/issues/106)). MDM saves a
  download's progress every few seconds, in a file beside it. A download paused or finished
  during one of those saves could stop before the save was done. That save then wrote an old
  progress file a moment later. A download now stops only once its save is done.
- **A download could fail as it finished, with "part file still in use"**
  ([#116](https://github.com/anefzaoui/misterdownloadmanager/issues/116)). It happened when the
  last bytes arrived during a progress save, which still held the file. It has the same cause
  and fix as the one above.
- **A video download could fail in MDM while yt-dlp went on and finished it**
  ([#79](https://github.com/anefzaoui/misterdownloadmanager/issues/79)). A line yt-dlp printed in
  the system's code page rather than UTF-8 (a title in Turkish or Russian on some Windows) ended
  MDM's reading there, and the finished file was then "not said". yt-dlp now always writes
  UTF-8, every line is read to the end, and the saved file is also found by its name. Whenever
  MDM gives a video download up, it ends yt-dlp and everything yt-dlp started, so nothing goes
  on downloading unseen.
- **YouTube: "Requested format is not available" after MDM moved a video to another server**
  ([#94](https://github.com/anefzaoui/misterdownloadmanager/issues/94)). When the servers this
  computer can reach don't serve the chosen quality, MDM downloads the nearest quality below it
  (always a video, never the sound alone) and says so in the download's log. With no video left
  at all, it tries the page's own servers. A server yt-dlp couldn't resolve or reach is avoided on
  the next look-up. The log also names the qualities a move left out.
- **Lists read as blank items to screen readers** ([#95](https://github.com/anefzaoui/misterdownloadmanager/issues/95)):
  the rows of Download video's qualities, Add torrent's files, the language picker, Options
  (browsers, sounds, the file-type and site lists), the Scheduler and Import now have their names
  (a format and its size, a file's name and size), their check boxes theirs, ticked or not, and
  their selected row; Download video's list is called Quality, Add torrent's Files and the
  sounds' Sounds. Choosing a row from a screen reader (UI Automation's Select) now shows the new
  choice at once: it used to wait for the next mouse move.
- **List rows sit inside their list for screen readers** ([#102](https://github.com/anefzaoui/misterdownloadmanager/issues/102)):
  they used to be listed beside it. A screen reader now says which row of how many ("4 of 12"),
  choosing from the list itself works (UI Automation's Selection), and a tree's check box belongs
  to its file's row. The language picker's list and the Options browser list and the Scheduler's
  two lists have names now too.
- **Torrents: speed limits now work as they do for other downloads**
  ([#39](https://github.com/anefzaoui/misterdownloadmanager/issues/39)): a limit set in a
  torrent's progress window changed every torrent's limit, a remembered limit was ignored, the
  Speed Limiter's "for one file" was shared by all torrents together, and "Limit all downloads
  together" left torrents out. Each torrent now has a limit of its own, a remembered one applies
  again when it resumes, and torrents and other downloads share the global cap evenly: each
  side gets at least half of it, and the rest when the other side needs less. A limit under
  16 KB/s, which made a torrent drop its peers and stall, gives it 16 KB/s. A
  torrent started again while its files were still being checked no longer stays paused, and
  of two torrents started at the same moment, the second no longer fails with "the torrent is
  no longer there".
- **Stopping a download under a low speed limit took seconds**: each connection first waited
  for its turn under the limit. Stop now ends those waits at once.
- **Screen readers: the progress window's Speed Limiter** took a speed set with a screen reader
  or UI Automation only after the focus moved on, and its "Remember Speed Limiter settings for
  this file" check box had no name. A speed set that way applies at once, and the box is named.
- **The browser extension's Download video menu: an item's highlight ran past the menu's
  edge** ([#81](https://github.com/anefzaoui/misterdownloadmanager/issues/81)).
- **A long file name ran out of the progress window** ([#62](https://github.com/anefzaoui/misterdownloadmanager/issues/62)):
  on the **Options on completion** tab, a longer translation of "Save To:" (Turkish, German…)
  ran into the path, and the path ran past the window's edge. The label now takes the room its
  language needs, and the path is shortened in the middle, keeping the file's name in sight,
  and shown whole on hover; right-to-left languages included.
- **Download complete's Open with... stayed in English** in every other language
  ([#80](https://github.com/anefzaoui/misterdownloadmanager/issues/80)). The tool that collects
  MDM's texts for translation now also finds labels given through a window's own helpers, and
  checks in CI that it keeps finding them.
- **Download complete shows above other programs' windows**, as IDM's does, so the end of a
  download isn't missed ([#77](https://github.com/anefzaoui/misterdownloadmanager/issues/77)):
  it doesn't take the keyboard from the program being typed in, and once clicked it is a window
  like any other.
- **Options opened from the extension's Button settings... comes to the front**
  ([#77](https://github.com/anefzaoui/misterdownloadmanager/issues/77)), instead of opening behind
  the browser on Windows.
- **Dragging the file out of Download complete works in every language**
  ([#78](https://github.com/anefzaoui/misterdownloadmanager/issues/78)): the handle looked for the
  English window title, so outside English the drag did nothing. The handle now says what it does
  on hover. It is Windows only, and no longer shown on Linux and macOS, where it did nothing.
- **A Google Drive link saved Drive's web page** (a file named `view` or `download`) instead of
  the file ([#34](https://github.com/anefzaoui/misterdownloadmanager/issues/34)).
- **Download File Info's errors are worded in the interface language**, as the list's are.
- **Resuming after a waiting page**: when a download's address had to be asked again (an expired
  link), MDM now goes through the same waiting pages and redirects as the first time, instead
  of giving up on them.
- **Linux: the window couldn't be closed** on desktops without a tray, such as MangoWC
  ([#59](https://github.com/anefzaoui/misterdownloadmanager/issues/59)): closing hid it to a tray
  that wasn't there. Closing now hides to the tray only when a tray shows MDM's icon, and quits
  otherwise. (The menus and dark mode there: see #50 and #60.)
- **Stopping a video download left yt-dlp downloading in the background** on Windows
  ([#61](https://github.com/anefzaoui/misterdownloadmanager/issues/61)): `yt-dlp.exe` starts a
  second process that does the work, and only the first was ended. Resuming then failed ("yt-dlp
  did not provide information") and a new try crawled, sharing the line with the old one. Each
  helper now runs in a job of its own that Stop ends whole.
- **Linux: menus opened in the middle of the screen** in a Wayland session
  ([#50](https://github.com/anefzaoui/misterdownloadmanager/issues/50)): Wayland lets no program
  place its windows, and MDM's menus and drop-down lists are windows it places. MDM now runs
  through XWayland when it is there (`MDM_WAYLAND=1` keeps Wayland).
- **ffmpeg could fail to install** with "the release has no an LGPL Windows build": BtbN, whose
  builds MDM installs, publishes a dated build each day and then remakes its `latest` release,
  and meanwhile GitHub's "latest" is the dated one, whose files are named differently. MDM now
  reads BtbN's newest releases, either naming, and takes the newest complete build. Errors while
  installing yt-dlp or ffmpeg are also worded in the interface language now.
- **Links that work only once** (or hosts that allow one download at a time) failed with "403
  Forbidden" after Download File Info had looked them up
  ([#28](https://github.com/anefzaoui/misterdownloadmanager/issues/28)): the window's look and the
  download each asked the server. The download now continues from the answer the window got,
  and asks again only if that connection has closed meanwhile.
- A download whose address ends in a long token (file hosts such as AnkerGames' mirrors) failed
  with "the path is too long for Windows" before it started
  ([#28](https://github.com/anefzaoui/misterdownloadmanager/issues/28)). Every new download's name
  is now shortened to fit its folder (keeping the extension), and when nobody named the file
  (no name from the browser, no answer to the look-up, not edited), it gets the name the server
  gives, as in a browser: `Call-of-Duty-Infinite-Warfare-AnkerGames.zip` rather than the
  address's token.
- Bangla shows Bangladesh's flag instead of India's, in the language picker, on the website and
  in the README ([#27](https://github.com/anefzaoui/misterdownloadmanager/issues/27)).
- MDM closed with an internal error when a window opened while the main window was minimized
  or hidden, for example Download File Info for a browser download
  ([#22](https://github.com/anefzaoui/misterdownloadmanager/issues/22)). The cause was in egui,
  the toolkit MDM is drawn with ([emilk/egui#8470](https://github.com/emilk/egui/issues/8470)),
  and is fixed in MDM's copy of it.
- **View → Dark Mode support** did nothing on Linux: MDM asked the window system for the
  desktop's light or dark setting, and X11 never says. It now asks the desktop itself (the
  freedesktop portal, which KDE, GNOME and most others answer) and follows it as it changes.
- A second browser download arriving while Download File Info was open replaced the first, which
  was lost (the browser had already given it up). They now wait their turn, and those still
  waiting when MDM quits are kept in the main queue (Download Later).
- When Firefox started MDM for a download, closing its helper could close MDM too.
- A download with very large cookies made the browser's helper start a second MDM and wait 5
  seconds; MDM only starts now when it isn't running, and takes what the browser sends.
- MDM listens for the browser before its window is up, and the browser's helper waits 20 seconds
  for a slow first start instead of 5.
- File names can no longer hide a right-to-left mark (`invoice‮fdp.exe`), and a server's
  Unicode `filename*` name wins over its plain fallback.
- Browser downloads are no longer taken when the extension may not read the site (its access set
  to **On click**): without the site's cookies they would fail.
- The extension's right-click menu, and its translation, are set up again cleanly after an
  update.

## [0.2.4] - 2026-11-01

A fix for 0.2.3, whose toolbar buttons did nothing (only their drop-down arrows worked), and for
dark mode, which drew everything a little bigger and in other places than day mode.

### Added

- **The video helpers are offered on the first start.** A window explains what yt-dlp and
  ffmpeg do and downloads both on **Yes, download them (recommended)**; **Not now** leaves them
  for the video window. Upgrading? It asks once too, if one of them is missing.

### Changed

- **MDM's own downloads use its own downloader**: yt-dlp, ffmpeg, MDM's updates and a crash
  report's symbols now come down with several connections, resume after a cut connection and go
  through your proxy settings. Each file is still checked against its publisher's signed
  checksums before use.

### Fixed

- **The toolbar's buttons work again**: Add URL, Add Torrent, Delete Completed, Options,
  Scheduler, Start Queue and the others did nothing when clicked in 0.2.3; their drop-down arrows
  did.
- **Dark mode has day mode's sizes and places**: the menu bar, the toolbar, the panes, the list's
  lines and selection, buttons and the progress window's table measure the same in both modes.
  Only the colours change.
- **Texts that were always in English** are translated in every language: the empty list's
  message, buttons of the crash report window, and why OK or Verify is greyed in some windows.
- **Menus fit their texts**: they were wider than needed, so short commands looked lost in them.
- **The language list scrolls with the mouse wheel.**
- **A cut file name's full text shows at the pointer**, not under the list.
- **Toolbar buttons** have as much room under their label as above their icon when the pointer is
  over them.

## [0.2.3] - 2026-11-01

A fix for 0.2.2, which froze on every system as soon as Add URL opened, and crash reports, so the
next problem like it is caught and fixed sooner.

### Added

- **Crash reports.** If MDM closes unexpectedly or stops responding, a report window opens. It
  shows what happened and where in MDM's code, lets you read the whole report, and **Send on
  GitHub** opens a crash report form in your browser, filled in. Nothing is sent by itself. When
  the same problem was already reported, it takes you to that issue instead. A frozen MDM can be
  ended and restarted from there. Reports leave out your download addresses and file names. See
  [Crash reports](docs/crash-reports.md).
- **A log**, kept in MDM's data folder (`logs`), so a report can show what led to a problem.
- **MDM checks for updates every time it starts** (it was once a week), and once a day while it
  stays open; a new version's window opens in front. **Check for updates at every start** in
  that window turns it off.

### Changed

- **One look for every kind of control**, in both themes: text boxes, read-only fields, check
  boxes, buttons, drop-downs, tabs, lists and their headers, popups and progress bars are each
  drawn one way everywhere (dark mode drew some windows' controls in an older style, with bright
  borders). A number and its arrows are one box now, as Windows 11's are; text is centred in
  controls of any height; one blue for selection and focus.
- **A tour of the features** on the website instead of a wall of screenshots.

### Fixed

- **The window froze for seconds while a big download was writing**: MDM saved its list on the
  window's thread, and the save waited for the disk. It saves on a thread of its own now (the
  new crash reports caught it).
- **Add URL froze MDM** (Tasks → Add new download, the toolbar's Add URL, Ctrl+N), and so did
  Download File Info: their address and Save As boxes took the keyboard focus and MDM waited on
  itself. Everything clicked after that seemed to do nothing. A test now opens Add URL the way
  you do, and a stricter check looks for this kind of hang in the whole app.

## [0.2.2] - 2026-11-01

A sweep through every window for the small things that make a program feel finished: how each
control looks and answers the mouse and the keyboard, dark mode everywhere, screen readers,
and a long list of fixes found by reviewing all of it (issue #7 and more).

### Added

- **A status strip** under the list: how many downloads it shows and how many are selected,
  how many are running and how fast in all. The tray icon's tip says the same.
- **Drag and drop on the main window**: drop a link to add it, several links or a list
  (.txt, .ef2) to choose them in Import, a .torrent file to add it.
- **MDM comes back as you left it**: the window's place and size, the Categories pane, its
  open branches and the node shown.
- **Download counts** in the Categories tree, **an empty list that says why** (with Clear
  filters or Add new download), **a right-click menu on the list's blank space**, a
  double-click on a column border to fit it, and **Reset columns**.
- **Screen readers** (Narrator, NVDA): every window, dialogs and menus included, now tells them
  what each control is, its name and its state; the download list reads each download.
- **Keyboard everywhere**: arrow keys in radio groups and tab strips, Ctrl+Tab between tabs,
  type a name to jump to it in any list or drop-down, Up/Down through Add URL's and Save As'
  history, Home/End in menus, Tab between the Categories tree and the list (the tree answers
  the arrows), F2 to rename, held arrows repeat, the date picker's calendar works from the
  keyboard and has Today.

### Changed

- **Windows 11's look in light mode**: rounded text boxes and drop-downs (no more black corner
  dots), pressed states on every button (issue #7), focus rings only when you use the
  keyboard, tooltips with Windows 11's corners and padding, and dark mode in every window.
- **Every list in the dialogs** scrolls, highlights and takes the keys the same way, and the
  Scheduler's list of a queue's files now scrolls at all.
- **Numbers** are typed in one kind of box everywhere: arrows step them, a value out of range is
  outlined in red and set right when you leave the box (some were changed silently before).
- **Things that fail say why**: opening a file that was moved, a file that can't be deleted or
  moved, the list that can't be saved, a computer that won't shut down, a check closed early.
- **System file dialogs no longer freeze MDM's other windows** while they are open.
- **The mouse wheel scrolls as many lines a notch as Windows' mouse settings say**, in every
  list, tree and text box; Page Up / Page Down in the download list go to the edge of what's
  shown first, as in Explorer; text boxes of several lines have the lists' scroll bar.
- The toolbar's right-click menu is gone: it offered button sizes and styles that don't exist.

### Fixed

- **Options' OK undid changes made meanwhile** (the toolbar's Speed Limiter, Torrent
  settings…), and the Scheduler's Close undid downloads added to or removed from a queue.
- **Shift+Delete's answer was remembered**: after one Shift+Delete, a plain Delete with "Don't
  show again" erased files from disk. Shift+Delete now works on macOS and Linux too.
- **Sleep when done hibernated** the computer on Windows.
- **Enter did two things**: with Cancel or a check box focused, Enter also pressed OK.
- **Download File Info** ignored an edited address and accepted an empty Save As.
- **Find** could select a download hidden by the column filters; the selection vanished when
  the list lost focus; Delete now selects the next download.
- **The keyboard focus got lost** when a button greyed itself or Escape closed a drop-down:
  Tab started over at the top of the window. It now moves on, or comes back, as in Windows.
- **Help → Report a problem** filled the form's System and Installed with only for a moment;
  they stay filled now.
- A torrent's progress window: its tabs touched the buttons above them, and Resume
  capability's Yes / No stood apart from the other values. A long list of file types in
  Options ran out under its box.
- Deleting a queue asks first; a running queue stops when deleted; the toolbar's Start / Stop
  Queue arrows always work.
- Many more: the checksum dialog, the progress window's completion options and speed limit,
  the torrent and video windows (Stop a look-up, stale video choices), Batch's letter ranges,
  addresses without http://, duplicate links arriving together, imports that doubled
  downloads, menu titles that shared access letters in French and other languages, and more.

## [0.2.1] - 2026-10-31

### Added

- **Update now** in Help → Check for updates: MDM downloads the new version, checks it, puts it
  in place and restarts, on Windows (setup, portable and plain zip), Mac and Linux (AppImage,
  .tar.gz; a .deb or .rpm opens in the system's installer). It installs only files listed in a
  checksum list signed by MDM's release key, which every release now carries
  (`SHA256SUMS.txt.sig`). The first update this way will be from 0.2.1 to the next version.

### Fixed

- **Ghost windows while a menu is open** (issue #4): faded copies of the main window titled
  "egui window" opened and closed with every redraw of an open menu, and the toolbar's hover
  flickered under them. On a Mac in full screen they opened a space of their own and froze MDM
  (issue #5). They came with 0.2.0's menu keyboard support, which asked about a submenu's window
  that wasn't open, and that made one.
- **Check for updates**: the release notes scroll (the wheel, a trackpad, their scroll bar, and
  the arrow, Page and Home/End keys); their lines no longer break mid-sentence, and links show
  as their text (issue #5).
- **Controls that didn't react to the mouse**: the ▾ of Address and Save As (Add URL, Download
  File Info), the drop-down lists, check boxes, radio buttons, the Categories pane's ×, the
  Scheduler's date button and calendar, scroll bars and the list's column headers now show
  when the pointer is over them and when they're pressed, as Windows' own controls do; an
  arrow stays pressed while its list is open.

### Changed

- **Help → Report a problem** opens GitHub's new bug report form with your MDM version, your
  system and its version, how MDM was installed and its language filled in. Every report now
  says which system and version it comes from.

## [0.2.0] - 2026-10-31

Mac fixes from the first round of real use, found on every system and fixed there too; the
menus and text boxes work as in any desktop program; and MDM works with AI assistants.

### Fixed

- **Menus on macOS and Linux**: clicking a menu item did nothing (the click moved the focus to
  the menu's own window, which closed it first). The list's right-click menu and the toolbar's
  drop-downs had the same fault.
- **Paste** from other programs into any text box (Ctrl+V, ⌘V): MDM kept a clipboard of its own.
- **The app icon on macOS**: the Dock showed a small, blurry icon while MDM ran; it keeps the
  app's own now. Elsewhere windows take their icon from a large size (sharp at any scaling).
- **The download list's file icons** were drawn over the Categories pane when the list was
  scrolled sideways (the Categories tree and the Scheduler had the same fault).
- **The download progress window** could be stretched on macOS and Linux without its contents
  following; it keeps its size, as on Windows.
- **Questions** ("Create this folder?", delete, a duplicate download, the turn-off countdown)
  could open behind the window that asked them; they stay in front now.
- **Video downloads' progress** went back and forth (a stream in fragments re-estimates its size,
  and picture and sound arrive one after the other); it only moves forward now, from the size
  the video window showed.

### Added

- The menus show each command's keys (**Ctrl+N**, on a Mac **⌘N**) and work from the keyboard:
  **F10** or **Alt** (on a Mac **Ctrl+F2**), the underlined letters, the arrows, **Enter**, **Esc**.
- Right-click any text box for **Cut**, **Copy**, **Paste**, **Delete** and **Select all**.
- **Address** (Add URL) and **Save As** (Download File Info) list the last 20 addresses and folders
  from their arrows; **...** beside Save As chooses a folder, **+** beside Category makes one.
- Help → Keyboard shortcuts shows the Mac's keys on a Mac.
- **AI assistants**: `misterdownloadmanager-mcp`, an MCP server that comes with MDM, lets Claude,
  Cursor, VS Code and other MCP assistants list, add, pause, resume and remove the running
  app's downloads and start its queue, through the automation API
  ([how to set it up](docs/ai-assistants.md)).
- **`--json`** for `misterdownloadmanager-cli`: every command prints JSON for scripts and AI
  agents, downloads as one `progress` line per half second and then `done` or `paused`
  ([the formats](docs/command-line.md#json-for-scripts-and-programs)).
- **The automation API's JSON Schema**, [automation-api.schema.json](docs/automation-api.schema.json),
  made from the code the app answers with. Each download in its list now says why it failed
  (`error`), and the "it is off" answer carries `"code": "api-off"` for programs to check.
- The website has `llms.txt` and `llms-full.txt`, and every help page as Markdown, for AI tools.

### Changed

- `misterdownloadmanager-cli status`, `pause`, `start`, `remove`, `delete` and `api` exit with
  code 1 when the app answers with an error (the API is off, an unknown action), as they do when
  the app isn't running.

## [0.1.0] - 2026-10-30

The first release: a free download manager for Windows, laid out the way Internet Download
Manager users expect, with experimental builds for macOS and Linux.

- **The download engine**: up to 32 connections per file, re-splitting the slowest part until
  the last byte, and crash-safe resume after a restart or a dropped connection. HTTP, HTTPS,
  **FTP and FTPS** (`ftp://`, `ftpes://` explicit TLS, `ftps://` implicit TLS), proxies, SOCKS and
  **proxy auto-config (PAC)** scripts.
- **The main window** you know: categories, the download list, the progress window with every
  connection's share, queues and the **Scheduler**, the speed limiter, Sites Logins, and
  **Options** where you expect them, every control working. Day and dark mode.
- **Browser integration** for Chrome, Edge, Brave, Opera and Vivaldi (Firefox in testing):
  downloads hand over to MDM with their cookies and referrer, right-click **Download with Mister
  Download Manager**, and a **Download video** button over videos on any page, which can be
  dragged along the video's edge, hidden on a video or a site, or turned off in Options.
- **Copy a link, get the download window**: copying a link to a file type on the **File types**
  list opens **Download File Info** with it, ready to start, also without the browser extension
  (**Options → General → Offer to download links copied to the clipboard**, on by default).
- **Videos from web pages** through yt-dlp (installed and kept current from MDM, with ffmpeg for
  the qualities that need merging), and **BitTorrent and magnet links** through librqbit: choose
  the files, see peers, trackers and pieces, keep sharing if you like.
- **86 interface languages**, every language Windows 11 offers, chosen by flag on the first start
  or in **View → Language...**; right to left for Arabic, Hebrew, Persian, Urdu and Uyghur; fonts
  for every script from the system itself. The installer speaks the same languages, and the
  browser extension speaks your browser's (50 languages).
- **Options → Regional**: dates and times as your region writes them, or your own way (day, month
  or year first, 12- or 24-hour clock, the week's first day); the list and the Scheduler follow.
- **[Command line and automation](docs/command-line.md)**: IDM's switches (`/d /p /f /a /n /s`),
  `misterdownloadmanager-cli` (files, videos, torrents) and a local, token-protected automation
  API that lists, pauses, starts, removes and deletes downloads.
- **Downloads**: the Windows installer, a portable zip and a plain zip, the browser extension on
  its own (a zip for Chromium browsers, an `.xpi` for Firefox, always the app's version), and
  **macOS and Linux to try**: a universal `.dmg` and an AppImage, `.deb`, `.rpm` and `.tar.gz`,
  marked `-experimental` because they are untested by us. There, MDM connects itself to Chrome,
  Edge, Brave, Vivaldi and Firefox, has its tray icon, follows the system's light or dark look and
  uses the system's fonts and ffmpeg. Tell us how they work for you.
- **Help** opens the [user guide and tutorials](docs/README.md).

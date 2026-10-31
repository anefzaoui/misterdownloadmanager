# Changelog

Each release's notes on GitHub come from its section here (`## [X.Y.Z]`, see
`.github/workflows/release.yml`).

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

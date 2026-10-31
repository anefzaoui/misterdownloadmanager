# Command line and automation

[← Back to docs index](README.md)

MDM can be driven without its window in three ways:

- **The app's own command line**, the same switches as IDM's: hand a download to the running app from a script, a batch file or another program.
- **`misterdownloadmanager-cli`**, a separate command-line tool: download files, videos and torrents in a terminal, with progress, without the app.
- **The automation API**: other programs list the running app's downloads (as JSON) and pause, start, remove or delete them, over a local, token-protected channel. AI assistants use it through MDM's MCP server: see [AI assistants](ai-assistants.md).

The examples use Windows' paths; on macOS and Linux the programs have the same names without `.exe`.

## The app's command line

```
misterdownloadmanager.exe /d URL [/p folder] [/f name] [/a] [/n] [/s]
```

| Switch | What it does |
|---|---|
| `/d URL` | The address to download. |
| `/p folder` | Save it in this folder (else the category's folder from **Options → Save to**). |
| `/f name` | Save it under this name. |
| `/a` | Add it to the main download queue without starting it. |
| `/n` | Silent: no **Download File Info** window, the download starts (or is queued, with `/a`) at once. |
| `/s` | Start the main download queue. |
| `/exit` | Save and close the running app (the installer uses it before an upgrade). |
| *a `.torrent` file or `magnet:` link* | Open it in **Add torrent** (what Windows passes when MDM opens torrents). |

Switches can start with `/` or `-`, in any case. If MDM is running, the command goes to it; if not, MDM starts and carries it out. For example:

```
misterdownloadmanager.exe /d https://example.com/files/report.pdf /p D:\Reports /n
misterdownloadmanager.exe /d https://example.com/big.iso /a
misterdownloadmanager.exe /s
```

## misterdownloadmanager-cli

It comes with every download of MDM, next to the app (the installer puts it in MDM's folder). It works on its own: the app needn't run, except for `status` and `api`.

| Command | What it does |
|---|---|
| `get URL [-o folder\|file] [-c connections] [-H "Header: value"]... [--checksum hash]` | Download a file with up to 8 connections (`-c`), showing progress. `Ctrl+C` pauses it; `resume` continues. `--checksum` checks the finished file: hex MD5, SHA-1, SHA-256, SHA-512 or CRC32, optionally named (`sha256:…`). |
| `resume file.mdm.json [-c connections] [-H ...]` | Continue a paused or broken download from the `.mdm.json` file next to it. |
| `video-info URL` | What video a page holds (YouTube, Vimeo, ...), and the qualities to choose from. |
| `video-get URL [-f selector] [-o folder]` | Download a video; `-f` takes a quality's selector from `video-info` (`b` = the best single file). |
| `install-ytdlp`, `install-ffmpeg` | Install or update the helpers videos need: yt-dlp (checked against its signing key) and ffmpeg (Windows; on macOS and Linux MDM uses the system's). |
| `torrent-info SOURCE` | A torrent's name, files (numbered) and size; `SOURCE` is a magnet link or a `.torrent` file. |
| `torrent-get SOURCE [-o folder] [--files 0,2]` | Download a torrent, or only some of its files. |
| `torrent-create PATH -o file.torrent` | Make a `.torrent` of a file or folder (peers find it through the DHT). |
| `status` | The running app's downloads, as JSON, with their ids (the automation API must be on). |
| `pause ID...`, `start ID...` | Pause running downloads, or start (resume) stopped, queued or failed ones, in the running app. |
| `remove ID...`, `delete ID...` | Take downloads off the running app's list: `remove` keeps their files, `delete` deletes them too. A running download is paused first (`pause`). |
| `api on`, `api off` | Turn the running app's automation API on or off. |

`misterdownloadmanager-cli help <command>` shows every option. For example:

```
misterdownloadmanager-cli get https://example.com/big.iso -o D:\Downloads -c 16 --checksum sha256:9f86d0…
misterdownloadmanager-cli video-get "https://www.youtube.com/watch?v=aqz-KE-bpKQ" -f "b[height=720]"
misterdownloadmanager-cli torrent-get "magnet:?xt=urn:btih:…" --files 0
```

## The automation API

Other programs can ask the running app what it is downloading, and pause, start, remove or delete downloads. The API is **off by default**; turn it on with `misterdownloadmanager-cli api on` (and off with `api off`).

### How a program talks to MDM

1. While it runs, MDM listens on a port of `127.0.0.1` (this computer only) and writes the port and a secret token to `ipc.json` in its data folder:
   - Windows: `%APPDATA%\misterdownloadmanager\ipc.json`
   - macOS: `~/Library/Application Support/misterdownloadmanager/ipc.json`
   - Linux: `~/.local/share/misterdownloadmanager/ipc.json`
   - A [portable copy](portable.md): `data\ipc.json` next to the program.

   The token changes at every start; only programs that can read your data folder (yours) can use it.
2. Connect to that port and send one line of JSON: the token and the command's fields, e.g. `{"token": "<token>", "query": "list"}`.
3. MDM answers with one line: `ok` for a command, or a JSON object for a question.

### What it understands

| Fields (besides `token`) | Answer |
|---|---|
| `{"query": "list"}` | Every download and the total speed: `{"downloads": [{"id", "file", "url", "path", "status", "size", "downloaded", "speed", "added"}], "speed": bytes per second}`. Only while the API is on. |
| `{"action": "pause", "ids": [3, 4]}` | Acts on downloads by id: `"pause"`, `"start"`, `"remove"` (file kept) or `"delete"` (file deleted too). Answers `{"done": [3], "skipped": [{"id": 4, "why": "not running"}]}`; a running download must be paused before it is removed or deleted. Only while the API is on. |
| `{"api": true}` / `{"api": false}` | Turns the API on or off: `{"api": true}`. |
| `{"url": "https://…", "dir": "D:\\Files", "name": "a.zip", "silent": true}` | Adds a download, as the command line's `/d /p /f /n` do (`"later": true` is `/a`); `ok`. Also with the API off, like the command line. |
| `{"start_queue": true}` | Starts the main download queue; `ok`. |

An unknown question or action is answered `{"error": "…"}`, as is a question while the API is off (then with `"code": "api-off"`, for programs to check: the error text may change). A wrong token is answered `denied`.

### Examples

PowerShell (Windows):

```powershell
$ep = Get-Content "$env:APPDATA\misterdownloadmanager\ipc.json" | ConvertFrom-Json
$client = New-Object Net.Sockets.TcpClient("127.0.0.1", $ep.port)
$stream = $client.GetStream()
$writer = New-Object IO.StreamWriter($stream); $reader = New-Object IO.StreamReader($stream)
$writer.WriteLine((@{ token = $ep.token; query = "list" } | ConvertTo-Json -Compress))
$writer.Flush()
$reader.ReadLine() | ConvertFrom-Json | Select-Object -ExpandProperty downloads | Format-Table file, status, downloaded, size
$client.Close()
```

Python:

```python
import json, os, socket

ipc = os.path.join(os.environ["APPDATA"], "misterdownloadmanager", "ipc.json")  # see above elsewhere
ep = json.load(open(ipc))
with socket.create_connection(("127.0.0.1", ep["port"])) as s:
    s.sendall((json.dumps({"token": ep["token"], "query": "list"}) + "\n").encode())
    answer = json.loads(s.makefile().readline())
for d in answer["downloads"]:
    print(d["file"], d["status"], d["downloaded"], d["size"])
```

To add a download instead, send `{"token": "…", "url": "https://example.com/a.zip", "silent": true}`.

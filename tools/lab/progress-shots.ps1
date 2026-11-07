# The progress window of a video and of a torrent, for the docs (progress-video.png,
# progress-torrent.png: docs/images/*.webp through `tools/shots/make.py docs`), at 1x and off the screens, as docs-shots.ps1 takes the others. Each
# runs in a Mister Download Manager instance of its own (MDM_DATA_DIR, MDM_OFFSCREEN) whose list
# holds that one download; the automation API starts it and /show progress opens its window,
# so nothing is clicked or typed. The video comes from the labserver through yt-dlp (copied
# from your own MDM's tools folder: Tasks → Download video from a page installs it); the
# torrent from the swarm example (seeders and a tracker on this computer):
#
#   cargo build --release -p mdm-app -p mdm-cli -p mdm-labserver
#   cargo build --release -p mdm-torrent --example swarm
#   tools\lab\progress-shots.ps1 -Bin target\release
#
# On a machine without MDM (GitHub's runner, docs-shots.yml), yt-dlp comes from -YtDlp:
#
#   tools\lab\progress-shots.ps1 -Bin target\debug -YtDlp <yt-dlp.exe> -Work C:\Demo\p -At 40,40
param(
  [string]$Bin = "target\release",
  [string]$OutDir = "docs\images",
  [int]$Port = 8099,
  [int]$TrackerPort = 6969,
  # Only these shots ("video", "torrent"); both when empty.
  [string[]]$Only = @(),
  # The torrent's other details tabs too ("peers", "trackers", "pieces", "general"), each
  # as progress-torrent-<tab>.png (/show progress:<tab>).
  [string[]]$TorrentTabs = @(),
  # Dark mode (View → Always dark, whatever Windows' own mode); the same file names, or with
  # -Suffix ("-dark") after each.
  [switch]$Dark,
  [string]$Suffix = "",
  # A yt-dlp.exe to use instead of your own MDM's (its tools folder): on a machine without MDM
  # (GitHub's runner, docs-shots.yml).
  [string]$YtDlp = "",
  # The run's temporary folder (its path shows in the torrent's window); default
  # target\progress-shots in this checkout.
  [string]$Work = "",
  # Where the windows open instead of far off the screens (MDM_OFFSCREEN_AT, as docs-shots.ps1).
  [string]$At = ""
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$bin = (Resolve-Path (Join-Path $root $Bin)).Path
$exe = Join-Path $bin "misterdownloadmanager.exe"
$cli = Join-Path $bin "misterdownloadmanager-cli.exe"
$out = if ([IO.Path]::IsPathRooted($OutDir)) { $OutDir } else { Join-Path $root $OutDir }
# Under the repository (ignored target/): the temp folder's path names the user.
$work = if ($Work) { $Work } else { Join-Path $root "target\progress-shots" }
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class PS2 {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  public static List<string> Of(uint pid) {
    var r = new List<string>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); if (sb.Length > 0) r.Add(sb.ToString()); }
      return true; }, IntPtr.Zero);
    return r;
  }
}
"@
function Want([string]$name) { $Only.Count -eq 0 -or $Only -contains $name }
# Is the shot's client area (below the caption) one colour, not yet painted? As docs-shots.ps1.
function Blank([string]$file) {
  Add-Type -AssemblyName System.Drawing
  $bmp = [System.Drawing.Bitmap]::FromFile($file)
  try {
    $first = $bmp.GetPixel(4, 40).ToArgb()
    for ($y = 40; $y -lt $bmp.Height - 4; $y += 4) {
      for ($x = 4; $x -lt $bmp.Width - 4; $x += 4) {
        if ($bmp.GetPixel($x, $y).ToArgb() -ne $first) { return $false }
      }
    }
    return $true
  } finally { $bmp.Dispose() }
}

# One instance with one download: start it, let it run for $wait seconds, shoot its progress
# window. $item is the list row (its id is 1).
function Scenario([string]$name, [hashtable]$item, [int]$wait, [scriptblock]$prepare, [string[]]$tabs = @()) {
  $dir = Join-Path $work $name
  New-Item -ItemType Directory -Force "$dir\data" | Out-Null
  $dirs = @("General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents") |
    ForEach-Object { , @($_, $(if ($_ -eq "General") { "$dir\Downloads" } else { "$dir\Downloads\$_" })) }
  $item.id = 1
  $list = [ordered]@{
    next_id = 2; version = 2; items = @($item)
    settings = [ordered]@{
      language = "en"; language_asked = $true; helpers_asked = $true; extension_never_ask = $true
      dark_mode_support = [bool]$Dark; always_dark = [bool]$Dark; update_check = $false; category_dirs = $dirs
      # The torrent engine reaches the swarm on 127.0.0.1 and listens nowhere else (no firewall
      # prompt), as docs-shots.ps1 sets it.
      torrent_incoming = $false; torrent_upnp = $false; torrent_dht = $false; torrent_lsd = $false
    }
  }
  if ($prepare) { & $prepare $dir }
  [IO.File]::WriteAllText("$dir\data\downloads.json", ($list | ConvertTo-Json -Depth 8))
  $env:MDM_DATA_DIR = "$dir\data"
  $env:MDM_OFFSCREEN = "1"
  if ($At) { $env:MDM_OFFSCREEN_AT = $At }
  $app = Start-Process -PassThru $exe
  try {
    Start-Sleep 3
    & $cli api on | Out-Null
    & $cli start 1 | Out-Null
    Start-Sleep $wait
    & $exe /show progress
    $title = $null
    for ($i = 0; $i -lt 50 -and -not $title; $i++) {
      Start-Sleep -Milliseconds 200
      $title = @([PS2]::Of([uint32]$app.Id) | Where-Object { $_ -match '^\d+(\.\d+)?% ' })[0]
    }
    if (-not $title) { throw "${name}: no progress window (" + (([PS2]::Of([uint32]$app.Id)) -join " | ") + ")" }
    # Its title starts with the percentage, which changes: match what follows it.
    $pattern = [WildcardPattern]::Escape($title) -replace '^\d+(\.\d+)?% ', '*'
    $file = Join-Path $out "progress-$name$Suffix.png"
    for ($try = 0; $try -lt 20; $try++) {
      Start-Sleep -Milliseconds 1500
      & "$PSScriptRoot\cap-own.ps1" -ProcessId $app.Id -Title $pattern -Out $file | Out-Null
      if (-not (Blank $file)) { break }
    }
    "  progress-$name$Suffix.png  ($title)"
    foreach ($tab in $tabs) {
      & $exe /show "progress:$tab"
      $file = Join-Path $out "progress-$name-$tab$Suffix.png"
      for ($try = 0; $try -lt 20; $try++) {
        Start-Sleep -Milliseconds 1500
        & "$PSScriptRoot\cap-own.ps1" -ProcessId $app.Id -Title $pattern -Out $file | Out-Null
        if (-not (Blank $file)) { break }
      }
      "  progress-$name-$tab$Suffix.png"
    }
  } finally {
    Stop-Process -Id $app.Id -ErrorAction SilentlyContinue
    Remove-Item Env:MDM_DATA_DIR, Env:MDM_OFFSCREEN, Env:MDM_OFFSCREEN_AT, Env:MDM_LAB_PEERS -ErrorAction SilentlyContinue
  }
}

$srv = $null; $swarm = $null
try {
  if (Test-Path $work) { Remove-Item $work -Recurse -Force }
  New-Item -ItemType Directory -Force $work, $out | Out-Null

  if (Want "video") {
    $tools = Join-Path ([Environment]::GetFolderPath("ApplicationData")) "misterdownloadmanager\tools"
    if ($YtDlp) {
      # A tools folder of the run's own, holding the given yt-dlp as MDM installs it.
      $tools = Join-Path $work "tools"
      New-Item -ItemType Directory -Force "$tools\yt-dlp\given" | Out-Null
      Copy-Item (Resolve-Path $YtDlp).Path "$tools\yt-dlp\given\yt-dlp.exe"
      [IO.File]::WriteAllText("$tools\yt-dlp\current.json", (@{ version = "given"; exe = "$tools\yt-dlp\given\yt-dlp.exe" } | ConvertTo-Json))
    }
    if (-not (Test-Path "$tools\yt-dlp\current.json")) { throw "no yt-dlp in ${tools}: install it from MDM first, or pass -YtDlp" }
    $srv = Start-Process -PassThru -WindowStyle Hidden (Join-Path $bin "labserver.exe") -ArgumentList "--bind 127.0.0.1:$Port"
    $video = "open-movie-trailer.mp4"
    $item = @{
      url = "http://127.0.0.1:$Port/gen/400M/$video" + "?rate=1500K"; file_name = $video
      dest = "$work\video\Downloads\Video\$video"; size = 419430400; downloaded = 0
      status = "Queued"; category = "Video"; description = ""; last_try = $null; error = $null
      resumable = $null; video = @{ selector = "b" }
    }
    # yt-dlp (and ffmpeg) from your own MDM, each tool's current.json pointing at its copy.
    $copyTools = {
      param($dir)
      foreach ($t in "yt-dlp", "ffmpeg") {
        if (-not (Test-Path "$tools\$t\current.json")) { continue }
        $cur = Get-Content "$tools\$t\current.json" -Raw | ConvertFrom-Json
        $to = "$dir\data\tools\$t\$($cur.version)"
        New-Item -ItemType Directory -Force $to | Out-Null
        Copy-Item "$(Split-Path -Parent $cur.exe)\*" $to -Recurse
        $cur.exe = Join-Path $to (Split-Path -Leaf $cur.exe)
        [IO.File]::WriteAllText("$dir\data\tools\$t\current.json", ($cur | ConvertTo-Json))
      }
    }
    Scenario "video" $item 12 $copyTools
  }

  if (Want "torrent") {
    $log = Join-Path $work "swarm.txt"
    $swarm = Start-Process -PassThru -WindowStyle Hidden (Join-Path $bin "examples\swarm.exe") `
      -ArgumentList "`"$work\swarm`" 600 4 $TrackerPort" -RedirectStandardOutput $log
    # Ready when it prints the torrent's path, its info hash and its seeders' addresses (after
    # making the files and checking them in every seeder).
    for ($i = 0; $i -lt 300; $i++) {
      Start-Sleep 1
      $lines = @(Get-Content $log -ErrorAction SilentlyContinue)
      if ($lines.Count -ge 3) { break }
      if ($swarm.HasExited) { throw "the swarm stopped" }
    }
    if ($lines.Count -lt 3) { throw "the swarm never got ready" }
    $torrent = $lines[0]
    $hash = ([regex]::Match($lines[1], '[0-9a-f]{40}')).Value
    # MDM leaves out trackers on this computer (#40): an off-screen copy is handed the seeders
    # instead (MDM_LAB_PEERS, torrents.rs).
    $env:MDM_LAB_PEERS = $lines[2] -replace '^peers ', ''
    $name = "Open Movie Night"
    $folder = "$work\torrent\Downloads\Torrents\$name"
    $item = @{
      url = "magnet:?xt=urn:btih:$hash&dn=Open+Movie+Night"; file_name = $name
      dest = $folder; size = 629145600 + 50 * 1024; downloaded = 0
      status = "Queued"; category = "Torrents"; description = ""; last_try = $null; error = $null
      resumable = $null; torrent = @{ hash = $hash; files = $null; folder = $folder }
    }
    # The app keeps a copy of each .torrent in its data folder.
    $saveTorrent = {
      param($dir)
      New-Item -ItemType Directory -Force "$dir\data\torrents\files" | Out-Null
      Copy-Item $torrent "$dir\data\torrents\files\$hash.torrent"
    }
    Scenario "torrent" $item 25 $saveTorrent $TorrentTabs
  }
} finally {
  if ($srv) { Stop-Process -Id $srv.Id -ErrorAction SilentlyContinue }
  if ($swarm) { Stop-Process -Id $swarm.Id -ErrorAction SilentlyContinue }
  # The downloads and the swarm's files (600 MB) don't stay behind.
  Start-Sleep 1
  Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

# The progress window of a video and of a torrent, for the docs (docs/images/progress-video.png,
# progress-torrent.png), at 1x and off the screens, as docs-shots.ps1 takes the others. Each
# runs in a Mister Download Manager instance of its own (MDM_DATA_DIR, MDM_OFFSCREEN) whose list
# holds that one download; the automation API starts it and /show progress opens its window,
# so nothing is clicked or typed. The video comes from the labserver through yt-dlp (copied
# from your own MDM's tools folder: Tasks → Download video from a page installs it); the
# torrent from the swarm example (seeders and a tracker on this computer):
#
#   cargo build --release -p mdm-app -p mdm-cli -p mdm-labserver
#   cargo build --release -p mdm-torrent --example swarm
#   tools\lab\progress-shots.ps1 -Bin target\release
param(
  [string]$Bin = "target\release",
  [string]$OutDir = "docs\images",
  [int]$Port = 8099,
  [int]$TrackerPort = 6969,
  # Only these shots ("video", "torrent"); both when empty.
  [string[]]$Only = @(),
  # The torrent's other details tabs too ("peers", "trackers", "pieces", "general"), each
  # as progress-torrent-<tab>.png (/show progress:<tab>).
  [string[]]$TorrentTabs = @()
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$bin = (Resolve-Path (Join-Path $root $Bin)).Path
$exe = Join-Path $bin "misterdownloadmanager.exe"
$cli = Join-Path $bin "misterdownloadmanager-cli.exe"
$out = if ([IO.Path]::IsPathRooted($OutDir)) { $OutDir } else { Join-Path $root $OutDir }
# Under the repository (ignored target/): the temp folder's path names the user.
$work = Join-Path $root "target\progress-shots"
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
      language = "en"; language_asked = $true; dark_mode_support = $false; category_dirs = $dirs
    }
  }
  if ($prepare) { & $prepare $dir }
  [IO.File]::WriteAllText("$dir\data\downloads.json", ($list | ConvertTo-Json -Depth 8))
  $env:MDM_DATA_DIR = "$dir\data"
  $env:MDM_OFFSCREEN = "1"
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
    $file = Join-Path $out "progress-$name.png"
    for ($try = 0; $try -lt 20; $try++) {
      Start-Sleep -Milliseconds 1500
      & "$PSScriptRoot\cap-own.ps1" -ProcessId $app.Id -Title $pattern -Out $file | Out-Null
      if (-not (Blank $file)) { break }
    }
    "  progress-$name.png  ($title)"
    foreach ($tab in $tabs) {
      & $exe /show "progress:$tab"
      $file = Join-Path $out "progress-$name-$tab.png"
      for ($try = 0; $try -lt 20; $try++) {
        Start-Sleep -Milliseconds 1500
        & "$PSScriptRoot\cap-own.ps1" -ProcessId $app.Id -Title $pattern -Out $file | Out-Null
        if (-not (Blank $file)) { break }
      }
      "  progress-$name-$tab.png"
    }
  } finally {
    Stop-Process -Id $app.Id -ErrorAction SilentlyContinue
    Remove-Item Env:MDM_DATA_DIR, Env:MDM_OFFSCREEN -ErrorAction SilentlyContinue
  }
}

$srv = $null; $swarm = $null
try {
  if (Test-Path $work) { Remove-Item $work -Recurse -Force }
  New-Item -ItemType Directory -Force $work, $out | Out-Null

  if (Want "video") {
    $tools = Join-Path ([Environment]::GetFolderPath("ApplicationData")) "misterdownloadmanager\tools"
    if (-not (Test-Path "$tools\yt-dlp\current.json")) { throw "no yt-dlp in ${tools}: install it from MDM first" }
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
    # Ready when it prints the torrent's path and its info hash (after making the files and
    # checking them in every seeder).
    for ($i = 0; $i -lt 300; $i++) {
      Start-Sleep 1
      $lines = @(Get-Content $log -ErrorAction SilentlyContinue)
      if ($lines.Count -ge 2) { break }
      if ($swarm.HasExited) { throw "the swarm stopped" }
    }
    if ($lines.Count -lt 2) { throw "the swarm never got ready" }
    $torrent = $lines[0]
    $hash = ([regex]::Match($lines[1], '[0-9a-f]{40}')).Value
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

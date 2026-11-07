# Screenshots for the README and docs/ (raw PNGs; `python tools/shots/make.py docs <run>` makes
# docs/images/<name>.webp of them, tools/website/screens.py the site's). The published ones are taken on
# GitHub's Windows runner (.github/workflows/docs-shots.yml), a stock Windows whose file icons are
# Windows' own; a run here shows this PC's. At 1x: MDM_ZOOM=2 draws the content
# twice as large but Windows keeps the title bar at the display's scale, out of proportion. Runs its
# own Mister Download Manager instance (its own data folder, MDM_DATA_DIR) and its own labserver,
# both addressed by process id: the user's own instance and anything else running are never
# touched. Nothing shows on the screen: with MDM_OFFSCREEN=1 the app opens every window far off
# the screens, without focus, taskbar button, tray icon or sound, and PrintWindow captures them
# there (cap-own.ps1). Dialogs open through the app's
# own command line (/show <window>), so nothing is clicked or typed on the screen.
#
#   tools\lab\docs-shots.ps1 -Exe target\release\misterdownloadmanager.exe -Server target\release\labserver.exe
#   tools\lab\docs-shots.ps1 ... -Lang ar -Only arabic-main
param(
  [Parameter(Mandatory)][string]$Exe,
  [Parameter(Mandatory)][string]$Server,
  [string]$OutDir = "docs\images",
  [string]$Lang = "en",
  [int]$Port = 8099,
  # The name the downloads' addresses use for the lab server (it listens on 127.0.0.1): one the
  # hosts file points there gives the shots a neutral address (docs-shots.yml: downloads.example).
  [string]$UrlHost = "127.0.0.1",
  [int]$Zoom = 1,
  # A .torrent file: its Add torrent window becomes add-torrent.png.
  [string]$Torrent = "",
  # Only these shots (names without .png); all when empty.
  [string[]]$Only = @(),
  # Dark mode for every window (View → Always dark: Windows' own mode doesn't matter).
  [switch]$Dark,
  # Where the main window opens instead of far off the screens (MDM_OFFSCREEN_AT, "x,y"): on a
  # machine nobody watches whose OpenGL is software (GitHub's runner, docs-shots.yml), which
  # draws nothing off the screens.
  [string]$At = "",
  # The run's temporary folder, whose path shows in some windows (Add torrent's); default
  # target\docs-shots in this checkout. From a worktree, the main checkout's target keeps it short.
  [string]$Work = "",
  # Other shots than the built-in list, each "<name>=<what>" (the help pages' shot list,
  # tools/shots/make.py): <what> is a /show argument (options:connection), "@main" (the main
  # window), "@menu:<show argument>" (a menu: every window of ours, composed), or
  # "@download:<path>" (Download File Info for the lab server's <path>, as a browser hands it),
  # or "@complete:<path>" (the Download complete window once that small file is in).
  # With -Torrent, "@torrent" is its Add torrent window. "a+b" runs /show a first, then shoots b
  # ("theme:metro+@main").
  [string[]]$Show = @(),
  # Beside each picture, <name>.ui.json: its controls' names and places (ui-rects.ps1).
  [switch]$Ui,
  # More settings for the run, as JSON members added to downloads.json's settings
  # ('"video_cookies":"firefox"': Options, Sites Logins, with a browser chosen).
  [string]$Settings = ""
)
$ErrorActionPreference = "Stop"
# With -Show, its list instead of the built-in one (unless -Only names some of those too). From
# `powershell -File`, a list comes as one string: split at its commas.
$Show = @($Show | ForEach-Object { $_ -split "," } | Where-Object { $_ })
if ($Show.Count -and $Only.Count -eq 0) { $Only = @("-") }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Exe = (Resolve-Path $Exe).Path
$Server = (Resolve-Path $Server).Path
$out = if ([IO.Path]::IsPathRooted($OutDir)) { $OutDir } else { Join-Path $root $OutDir }
New-Item -ItemType Directory -Force $out | Out-Null
# Under the repository (ignored target/): the temp folder's path names the user.
$work = if ($Work) { $Work } else { Join-Path $root "target\docs-shots" }
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class DS {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static List<KeyValuePair<IntPtr,string>> Of(uint pid) {
    var r = new List<KeyValuePair<IntPtr,string>>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) {
        var sb = new StringBuilder(512); GetWindowText(h, sb, 512);
        if (sb.Length > 0) r.Add(new KeyValuePair<IntPtr,string>(h, sb.ToString()));
      }
      return true; }, IntPtr.Zero);
    return r;
  }
}
"@
function Want([string]$name) { $Only.Count -eq 0 -or $Only -contains $name }
function Titles([int]$procId) { [DS]::Of([uint32]$procId) | ForEach-Object { $_.Value } }
# Close our windows whose title matches (WM_CLOSE), never another process's.
function Close-Ours([int]$procId, [string]$like) {
  [DS]::Of([uint32]$procId) | Where-Object { $_.Value -like $like } |
    ForEach-Object { [DS]::PostMessage($_.Key, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
}
# The window that appeared since `$before` (a dialog), by title; waits up to 10 s (a first
# window in Chinese, Japanese or Korean loads their large fonts).
function New-Window([int]$procId, [string[]]$before) {
  for ($i = 0; $i -lt 50; $i++) {
    $new = @(Titles $procId | Where-Object { $before -notcontains $_ })
    if ($new.Count) { return $new[0] }
    Start-Sleep -Milliseconds 200
  }
  throw "no new window"
}
# Is the shot's client area (below the 31 px caption, inside the border) one colour, not yet
# painted? Sampled every 4 px.
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
function Shot([int]$procId, [string]$title, [string]$name, [switch]$All) {
  # A progress window's title starts with its percentage, which changes as it downloads: match
  # what follows it. Wildcard characters in a title are taken literally.
  $title = [WildcardPattern]::Escape($title) -replace '^\d+(\.\d+)?% ', '*'
  # -All: every window of ours composed (a menu over the main window), not one by its title.
  $by = if ($All) { @{} } else { @{ Title = $title } }
  # A window off the screens can take a moment to paint: shot again until its client area isn't
  # a single colour (up to 10 s).
  $file = Join-Path $out "$name.png"
  for ($try = 0; $try -lt 20; $try++) {
    Start-Sleep -Milliseconds 500
    & "$PSScriptRoot\cap-own.ps1" -ProcessId $procId @by -Out $file | Out-Null
    if (-not (Blank $file)) { break }
  }
  if ($Ui) {
    try { & "$PSScriptRoot\ui-rects.ps1" -ProcessId $procId @by -Out (Join-Path $out "$name.ui.json") | Out-Null }
    catch { Write-Warning "${name}: no controls ($_)" }
  }
  "  $name.png  ($title)"
}

# MDM starts itself again when its renderer fails (a lost Direct3D device, renderer.rs): the
# shots go on in the new copy, ours too (our program, started after this run with /updated).
$started = Get-Date
function Ours { Get-CimInstance Win32_Process -Filter "Name='$(Split-Path -Leaf $Exe)'" |
  Where-Object { $_.ExecutablePath -eq $Exe -and $_.CreationDate -ge $started } }
function Follow {
  if (-not $script:app.HasExited) { return }
  $n = @(Ours | Where-Object { $_.CommandLine -like "*/updated*" })[0]
  if (-not $n) { throw "MDM ended" }
  Write-Warning "MDM started again (pid $($n.ProcessId)); the shots go on there"
  $script:app = Get-Process -Id $n.ProcessId
  Start-Sleep 4
  $script:main = @(Titles $script:app.Id | Where-Object { $_ -like "Mister Download Manager*" })[0]
}
$srv = $null; $app = $null
try {
  if (Test-Path $work) { Remove-Item $work -Recurse -Force }
  New-Item -ItemType Directory -Force "$work\data", "$work\Downloads" | Out-Null
  # The language is chosen (no first-start picker); light mode unless asked, dark with View →
  # Always dark, whatever Windows' own mode. Every category's folder is under $work, so no shot
  # shows the user's own folders (or name). Nothing reaches out: no update check, and the torrent
  # engine neither listens nor uses the DHT, UPnP or local discovery (no firewall prompt).
  $dark = $Dark -or ($Only -contains "main-dark")
  $q = { param($s) ($s | ConvertTo-Json) }
  $dirs = @("General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents") |
    ForEach-Object {
      $d = if ($_ -eq "General") { "$work\Downloads" } else { "$work\Downloads\$_" }
      "[" + (& $q $_) + "," + (& $q $d) + "]"
    }
  Set-Content -Encoding utf8 "$work\data\downloads.json" (
    '{"next_id":1,"items":[],"version":2,"settings":{"language":' + (& $q $Lang) +
    ',"language_asked":true,"helpers_asked":true,"extension_never_ask":true,"dark_mode_support":' + $dark.ToString().ToLower() +
    ',"always_dark":' + $dark.ToString().ToLower() +
    ',"update_check":false,"torrent_incoming":false,"torrent_upnp":false,"torrent_dht":false,"torrent_lsd":false,"torrent_stop_idle":true' +
    ',"category_dirs":[' + ($dirs -join ",") + ']' + $(if ($Settings) { ",$Settings" } else { "" }) + '}}')

  $srv = Start-Process -PassThru -WindowStyle Hidden $Server -ArgumentList "--bind 127.0.0.1:$Port"
  $base = if ($Port -eq 80) { "http://$UrlHost" } else { "http://${UrlHost}:$Port" }
  # /show crash-test:freeze for the crash report window's shot.
  $env:MDM_CRASH_TEST = "1"
  $env:MDM_DATA_DIR = "$work\data"
  # Only when asked: MDM_ZOOM overrides View → Interface size (-Settings '"interface_size":150').
  if ($Zoom -ne 1) { $env:MDM_ZOOM = "$Zoom" } else { Remove-Item Env:MDM_ZOOM -ErrorAction SilentlyContinue }
  $env:MDM_OFFSCREEN = "1"
  if ($At) { $env:MDM_OFFSCREEN_AT = $At }
  $app = Start-Process -PassThru $Exe
  Start-Sleep 3
  $main = @(Titles $app.Id | Where-Object { $_ -like "Mister Download Manager*" })[0]

  # A list that looks used: finished files, one still coming in (rate-limited: the rate is per
  # connection, so 256K over its 8 connections is about 2 MB/sec).
  $files = @(
    @("big-buck-bunny-1080p.mp4", "180M", ""), @("project-report-2026.pdf", "4M", ""),
    @("holiday-photos.zip", "64M", ""), @("podcast-episode-42.mp3", "38M", ""),
    @("rust-1.99.1-x86_64-pc-windows-msvc.msi", "92M", ""), @("open-data-2026.csv", "12M", ""),
    @("ubuntu-26.04-desktop-amd64.iso", "6G", "?rate=256K")
  )
  foreach ($f in $files) {
    & $Exe /d "$base/gen/$($f[1])/$($f[0])$($f[2])" /p "$work\Downloads" /n
    Start-Sleep -Milliseconds 400
  }
  # Let the small ones finish; close the dialogs they open (Download complete, progress), never
  # the main window.
  for ($i = 0; $i -lt 25; $i++) {
    Start-Sleep 1
    [DS]::Of([uint32]$app.Id) | Where-Object { $_.Value -notlike "Mister Download Manager*" } |
      ForEach-Object { [DS]::PostMessage($_.Key, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  }
  Start-Sleep 2
  $main = @(Titles $app.Id | Where-Object { $_ -like "Mister Download Manager*" })[0]

  # Each window opened by name through the app's own command line (/show, mdm_ipc), so no
  # click or key ever goes to the screen. name = /show argument. These are the README's and the
  # site's pictures and the few docs pages not yet in the help's shot list; the help pages'
  # own come from tools/shots/shots.toml (-Show).
  $windows = [ordered]@{
    "add-url" = "addurl"; "speed-limiter" = "speedlimit"; "scheduler" = "scheduler"
    "options-connection" = "options:connection"; "options-remote" = "options:remote"; "grabber" = "grabber"
    "language-picker" = "language"; "progress" = "progress"
  }
  if ($Torrent -and (Want "add-torrent")) {
    $before = @(Titles $app.Id)
    $copy = Join-Path $work (Split-Path -Leaf $Torrent)
    Copy-Item (Resolve-Path $Torrent).Path $copy
    & $Exe $copy
    $t = New-Window $app.Id $before
    Start-Sleep -Milliseconds 1500
    Shot $app.Id $t "add-torrent"
    Close-Ours $app.Id $t
  }
  foreach ($name in $windows.Keys) {
    if (-not (Want $name)) { continue }
    $before = @(Titles $app.Id)
    & $Exe /show $windows[$name]
    # A window that doesn't come up is reported; the run goes on with the others.
    try { $t = New-Window $app.Id $before } catch { Write-Warning "${name}: no window"; continue }
    # Options with a dialog over it: the dialog is the shot (the second new window).
    $also = @()
    if ($name -eq "options-panels") {
      for ($i = 0; $i -lt 50; $i++) {
        $new = @(Titles $app.Id | Where-Object { $before -notcontains $_ })
        if ($new.Count -ge 2) { break }
        Start-Sleep -Milliseconds 200
      }
      $also = @($t)
      $t = @($new | Where-Object { $_ -ne $also[0] })[0]
      if (-not $t) { Write-Warning "${name}: no dialog"; Close-Ours $app.Id $also[0]; continue }
    }
    Start-Sleep -Milliseconds 1200
    Shot $app.Id $t $name
    Close-Ours $app.Id $t
    foreach ($w in $also) { Close-Ours $app.Id $w }
    Start-Sleep -Milliseconds 600
  }
  # -Show's list (the help pages' shots), in its order.
  foreach ($entry in $Show) {
    Follow
    $name, $what = $entry -split "=", 2
    # "theme:metro+@main": /show theme:metro first (no window of its own), then the shot.
    $parts = @($what -split "\+")
    $what = $parts[-1]
    foreach ($p in $parts[0..($parts.Count - 2)]) {
      if ($parts.Count -lt 2) { break }
      & $Exe /show $p
      Start-Sleep -Milliseconds 1500
    }
    $before = @(Titles $app.Id)
    if ($what -eq "@main") {
      if ($Show.IndexOf($entry) -eq 0) { Start-Sleep 10 }
      Shot $app.Id $main $name
      continue
    }
    if ($what -like "@menu:*") {
      & $Exe /show $what.Substring(6)
      for ($i = 0; $i -lt 50 -and -not (@(Titles $app.Id) -like "misterdownloadmanager menu*"); $i++) { Start-Sleep -Milliseconds 200 }
      Start-Sleep -Milliseconds 1200
      Shot $app.Id "" $name -All
      # The menu's windows are ours: closed like any other.
      Close-Ours $app.Id "misterdownloadmanager menu*"
      Start-Sleep -Milliseconds 800
      continue
    }
    if ($what -eq "@torrent") {
      if (-not $Torrent) { Write-Warning "${name}: no -Torrent"; continue }
      $copy = Join-Path $work (Split-Path -Leaf $Torrent)
      if (-not (Test-Path $copy)) { Copy-Item (Resolve-Path $Torrent).Path $copy }
      & $Exe $copy
    } elseif ($what -like "@complete:*") {
      # A small download, silent: the shot is its Download complete window once it's done.
      & $Exe /d "$base/$($what.Substring(10))" /p "$work\Downloads" /n
      $t = $null
      for ($i = 0; $i -lt 75 -and -not $t; $i++) {
        Start-Sleep -Milliseconds 200
        $t = @(Titles $app.Id | Where-Object { $before -notcontains $_ -and $_ -like "Download complete*" })[0]
      }
      if (-not $t) { Write-Warning "${name}: no Download complete window"; continue }
      Start-Sleep -Milliseconds 1500
      Shot $app.Id $t $name
      @(Titles $app.Id | Where-Object { $before -notcontains $_ }) | ForEach-Object { Close-Ours $app.Id ([WildcardPattern]::Escape($_)) }
      Start-Sleep -Milliseconds 600
      continue
    } elseif ($what -like "@download:*") {
      # As a browser hands it over: no /n, so Download File Info asks.
      & $Exe /d "$base/$($what.Substring(10))"
    } else {
      & $Exe /show $what
    }
    try { $t = New-Window $app.Id $before } catch { Write-Warning "${name}: no window"; continue }
    # Options → Download panels opens a dialog over Options: that dialog is the shot.
    $also = @()
    if ($what -eq "options:panels") {
      for ($i = 0; $i -lt 50; $i++) {
        $new = @(Titles $app.Id | Where-Object { $before -notcontains $_ })
        if ($new.Count -ge 2) { break }
        Start-Sleep -Milliseconds 200
      }
      $also = @($t)
      $t = @($new | Where-Object { $_ -ne $also[0] })[0]
      if (-not $t) { Write-Warning "${name}: no dialog"; Close-Ours $app.Id $also[0]; continue }
    }
    Start-Sleep -Milliseconds 1500
    Shot $app.Id $t $name
    Close-Ours $app.Id $t
    foreach ($w in $also) { Close-Ours $app.Id $w }
    Start-Sleep -Milliseconds 600
  }
  # The main window last: by now every small file has finished (a fresh .zip can sit at 100%
  # for a moment while an antivirus scan holds it).
  if ((Want "main-light") -or (Want "main-dark") -or (Want "arabic-main")) {
    $name = if ($Lang -eq "ar") { "arabic-main" } elseif ($dark) { "main-dark" } else { "main-light" }
    # Shot alone (no windows before it): give the last small files their moment too.
    if ($Only.Count -eq 1) { Start-Sleep 10 }
    if (Want $name) { Shot $app.Id $main $name }
  }
  # The crash report window, from a freeze on purpose (MDM_CRASH_TEST): last, as it leaves MDM
  # frozen. The window is the crash monitor's, a process of its own, found by its title.
  $crashShot = if ($dark) { "crash-report-dark" } else { "crash-report" }
  if (Want $crashShot) {
    & $Exe /show "crash-test:freeze" | Out-Null
    $title = "Mister Download Manager crash report"
    $mon = 0
    for ($i = 0; $i -lt 60 -and -not $mon; $i++) {
      Start-Sleep -Milliseconds 500
      # Our app's own monitor (its child): never another copy's report window, which this run
      # would shoot and then close.
      $mon = (Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id)" -ErrorAction SilentlyContinue |
        Where-Object { (Titles $_.ProcessId) -contains $title } | Select-Object -First 1).ProcessId
    }
    if (-not $mon) { throw "no crash report window" }
    # The dump read and GitHub searched.
    Start-Sleep 8
    Shot $mon $title $crashShot
    Stop-Process -Id $mon -ErrorAction SilentlyContinue
  }
} finally {
  if ($app) { Stop-Process -Id $app.Id -ErrorAction SilentlyContinue }
  # A copy started again by itself, and its crash monitor: ours as well.
  Ours | ForEach-Object { Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }
  if ($srv) { Stop-Process -Id $srv.Id -ErrorAction SilentlyContinue }
  Remove-Item Env:MDM_DATA_DIR, Env:MDM_ZOOM, Env:MDM_OFFSCREEN, Env:MDM_OFFSCREEN_AT, Env:MDM_CRASH_TEST -ErrorAction SilentlyContinue
  # The downloads (a 6 GB file partly fetched) don't stay behind.
  Start-Sleep 1
  Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

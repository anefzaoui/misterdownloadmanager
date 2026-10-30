# Screenshots for the README and docs/ (docs/images/<name>.png), at 1x: MDM_ZOOM=2 draws the content
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
  [int]$Zoom = 1,
  # A .torrent file: its Add torrent window becomes add-torrent.png.
  [string]$Torrent = "",
  # Only these shots (names without .png); all when empty.
  [string[]]$Only = @()
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Exe = (Resolve-Path $Exe).Path
$Server = (Resolve-Path $Server).Path
$out = if ([IO.Path]::IsPathRooted($OutDir)) { $OutDir } else { Join-Path $root $OutDir }
New-Item -ItemType Directory -Force $out | Out-Null
# Under the repository (ignored target/): the temp folder's path names the user.
$work = Join-Path $root "target\docs-shots"
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
function Shot([int]$procId, [string]$title, [string]$name) {
  # A progress window's title starts with its percentage, which changes as it downloads: match
  # what follows it. Wildcard characters in a title are taken literally.
  $title = [WildcardPattern]::Escape($title) -replace '^\d+(\.\d+)?% ', '*'
  # A window off the screens can take a moment to paint: shot again until its client area isn't
  # a single colour (up to 10 s).
  $file = Join-Path $out "$name.png"
  for ($try = 0; $try -lt 20; $try++) {
    Start-Sleep -Milliseconds 500
    & "$PSScriptRoot\cap-own.ps1" -ProcessId $procId -Title $title -Out $file | Out-Null
    if (-not (Blank $file)) { break }
  }
  "  $name.png  ($title)"
}

$srv = $null; $app = $null
try {
  if (Test-Path $work) { Remove-Item $work -Recurse -Force }
  New-Item -ItemType Directory -Force "$work\data", "$work\dl" | Out-Null
  # The language is chosen (no first-start picker); light mode unless asked. Every category's
  # folder is under $work, so no shot shows the user's own folders (or name).
  $dark = $Only -contains "main-dark"
  $q = { param($s) ($s | ConvertTo-Json) }
  $dirs = @("General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents") |
    ForEach-Object {
      $d = if ($_ -eq "General") { "$work\Downloads" } else { "$work\Downloads\$_" }
      "[" + (& $q $_) + "," + (& $q $d) + "]"
    }
  Set-Content -Encoding utf8 "$work\data\downloads.json" (
    '{"next_id":1,"items":[],"version":2,"settings":{"language":' + (& $q $Lang) +
    ',"language_asked":true,"dark_mode_support":' + $dark.ToString().ToLower() +
    ',"category_dirs":[' + ($dirs -join ",") + ']}}')

  $srv = Start-Process -PassThru -WindowStyle Hidden $Server -ArgumentList "--bind 127.0.0.1:$Port"
  $env:MDM_DATA_DIR = "$work\data"
  $env:MDM_ZOOM = "$Zoom"
  $env:MDM_OFFSCREEN = "1"
  $app = Start-Process -PassThru $Exe
  Start-Sleep 3
  $main = @(Titles $app.Id | Where-Object { $_ -like "Mister Download Manager*" })[0]

  # A list that looks used: finished files, one still coming in (rate-limited).
  $files = @(
    @("big-buck-bunny-1080p.mp4", "180M", ""), @("project-report-2026.pdf", "4M", ""),
    @("holiday-photos.zip", "64M", ""), @("podcast-episode-42.mp3", "38M", ""),
    @("rust-1.99.1-x86_64-pc-windows-msvc.msi", "92M", ""), @("open-data-2026.csv", "12M", ""),
    @("ubuntu-26.04-desktop-amd64.iso", "6G", "?rate=3M")
  )
  foreach ($f in $files) {
    & $Exe /d "http://127.0.0.1:$Port/gen/$($f[1])/$($f[0])$($f[2])" /p "$work\dl" /n
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
  # click or key ever goes to the screen. name = /show argument.
  $windows = [ordered]@{
    "add-url" = "addurl"; "batch" = "batch"; "find" = "find"; "speed-limiter" = "speedlimit"
    "properties" = "properties"; "scheduler" = "scheduler"; "about" = "about"
    "options-general" = "options:general"; "options-connection" = "options:connection"
    "options-save-to" = "options:saveto"; "options-panels" = "options:panels"; "options-regional" = "options:regional"
    "language-picker" = "language"
    "delete-confirm" = "delete"; "progress" = "progress"
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
  # The main window last: by now every small file has finished (a fresh .zip can sit at 100%
  # for a moment while an antivirus scan holds it).
  if ((Want "main-light") -or (Want "main-dark") -or (Want "arabic-main")) {
    $name = if ($Lang -eq "ar") { "arabic-main" } elseif ($dark) { "main-dark" } else { "main-light" }
    # Shot alone (no windows before it): give the last small files their moment too.
    if ($Only.Count -eq 1) { Start-Sleep 10 }
    if (Want $name) { Shot $app.Id $main $name }
  }
} finally {
  if ($app) { Stop-Process -Id $app.Id -ErrorAction SilentlyContinue }
  if ($srv) { Stop-Process -Id $srv.Id -ErrorAction SilentlyContinue }
  Remove-Item Env:MDM_DATA_DIR, Env:MDM_ZOOM, Env:MDM_OFFSCREEN -ErrorAction SilentlyContinue
  # The downloads (a 6 GB file partly fetched) don't stay behind.
  Start-Sleep 1
  Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

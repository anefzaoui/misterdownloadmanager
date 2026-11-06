# Issue #145: a magnet link or a download handed over from the browser while MDM's main window was
# hidden (closed to the notification area) crashed 0.2.4 ("egui backend is implemented incorrectly
# - the user callback was never called"). This script plays it on a test copy, off the screens:
#
#   powershell -File tools/lab/magnet-hidden.ps1 -Exe <dir>\misterdownloadmanager.exe [-State hidden] [-Send magnet]
#
# -Send: what the browser hands over.
#   magnet    the browser gives a magnet link to Windows, which starts the program with it (the file
#             association); that second launch passes it to the running copy, which opens Add torrent.
#   download  the extension gives a download to the native host (misterdownloadmanager-nmhost.exe,
#             beside -Exe), started as Firefox starts it (the manifest's path, then the add-on's
#             ID) with one native message; the host passes it on and the copy opens Download File
#             Info. The address is on 127.0.0.1 port 9, where nothing answers.
# -State: visible (the window as it opens), hidden (ShowWindow hides it, as the notification area
#   does), tray (MDM hides it there itself: `/show tray`, after 0.2.4), minimized,
#   dialog (Options open, the main window minimized), twice (two in a row, hidden), restoring (a
#   plain second launch, which shows the window again, right before), all (each in turn).
# -Renderer: glow or direct3d (MDM_RENDERER; after 0.2.4).
#
# Each run has its own data folder (MDM_DATA_DIR, deleted after unless -Keep), so nothing touches
# an installed MDM, its registrations or the browsers. Nothing goes to the internet: the magnet
# names no tracker, the update check is off, and the torrent engine's DHT, local discovery, UPnP
# and incoming connections are off. A release that can't turn them off (0.2.4 and older) needs
# -NoTorrentEngine: a file named `torrents` in its data folder, so its torrent engine can't start.
# No mouse, keyboard or focus is used: the test copy's own main window is only minimized (without
# activating anything) or hidden with ShowWindow. After a crash, the crash monitor's report window
# (a process of the test copy) is closed.
#
# Prints, per state: whether the copy survived, which dialog opened, and the panic line of its log.
param(
  [Parameter(Mandatory)][string]$Exe,
  [ValidateSet("visible", "hidden", "tray", "minimized", "dialog", "twice", "restoring", "all")]
  [string]$State = "all",
  [ValidateSet("magnet", "download")]
  [string]$Send = "magnet",
  [string]$Renderer = "",
  [string]$Magnet = "magnet:?xt=urn:btih:5b4f0c3a9e1d27f86c0b3a4e9d1f2a7c6e8b0d14&dn=MDM%20test%20145",
  [string]$Url = "http://127.0.0.1:9/mdm-test-145.zip",
  [int]$WaitSec = 6,
  [switch]$NoTorrentEngine,
  [switch]$Keep,
  # One line of JSON per state, for scripts.
  [switch]$Json
)
$ErrorActionPreference = "Stop"
$Exe = (Resolve-Path $Exe).Path
$Host_ = Join-Path (Split-Path $Exe) "misterdownloadmanager-nmhost.exe"
if ($Send -eq "download" -and -not (Test-Path $Host_)) { throw "no native host beside -Exe: $Host_" }
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class MagWin {
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  public static string Title(IntPtr h) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
  // Titled top-level windows of one process, shown or not.
  public static List<IntPtr> Of(uint pid) {
    var o = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && Title(h).Length > 0) o.Add(h); return true; }, IntPtr.Zero);
    return o;
  }
}
"@
$SW_HIDE = 0; $SW_SHOWMINNOACTIVE = 7; $WM_CLOSE = 0x10
# Windows of the test copy that aren't MDM's own (OpenGL's and the input method's helpers).
$Helpers = "__wglDummyWindowFodder", "NVOGLDC invisible", "Default IME", "MSCTFIME UI"

function Windows-Of([int]$ProcId) {
  [MagWin]::Of([uint32]$ProcId) | Where-Object { $Helpers -notcontains [MagWin]::Title($_) } | ForEach-Object {
    $how = if (-not [MagWin]::IsWindowVisible($_)) { "hidden" } elseif ([MagWin]::IsIconic($_)) { "minimized" } else { "shown" }
    [pscustomobject]@{ Handle = $_; Title = [MagWin]::Title($_); How = $how }
  }
}

# The first shown window of the test copy whose title is like -Title and that isn't one of -Except.
function Wait-Window([int]$ProcId, [string]$Title, [int]$Sec = 20, [IntPtr[]]$Except = @()) {
  $until = (Get-Date).AddSeconds($Sec)
  while ((Get-Date) -lt $until) {
    $w = @(Windows-Of $ProcId | Where-Object { $_.Title -like $Title -and $_.How -eq "shown" -and $Except -notcontains $_.Handle })
    if ($w.Count -ge 1) { return $w[0] }
    Start-Sleep -Milliseconds 200
  }
  throw "no window '$Title' of process $ProcId after $Sec s"
}

# A second launch of the test copy (what Windows starts for a magnet link): it hands its arguments
# to the running copy and quits.
function Second-Launch([string[]]$ArgList) {
  $p = Start-Process -FilePath $Exe -ArgumentList $ArgList -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(15000)) {
    # No copy took it (it had crashed): this launch became one. It goes, with its crash monitor.
    Get-CimInstance Win32_Process -Filter "ParentProcessId = $($p.Id)" | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Stop-Process -Id $p.Id -Force
    throw "the second launch didn't quit"
  }
}

# The native host as Firefox starts it (manifest path, add-on ID), with one native message (a
# 32-bit length, then the JSON) on its input; its reply is returned. Through files and cmd's
# redirections: a .NET input stream would put a byte-order mark before the length.
function Native-Message([hashtable]$Message) {
  $json = [Text.Encoding]::UTF8.GetBytes(($Message | ConvertTo-Json -Compress))
  $msg = [IO.Path]::GetTempFileName(); $out = [IO.Path]::GetTempFileName()
  try {
    [IO.File]::WriteAllBytes($msg, [BitConverter]::GetBytes([uint32]$json.Length) + $json)
    $manifest = Join-Path (Split-Path $Exe) "firefox-host.json"
    $p = Start-Process -FilePath "cmd.exe" -PassThru -WindowStyle Hidden -ArgumentList "/d /c `"`"$Host_`" `"$manifest`" integration@misterdownloadmanager.com < `"$msg`" > `"$out`"`""
    if (-not $p.WaitForExit(15000)) { throw "the native host didn't quit" }
    $bytes = [IO.File]::ReadAllBytes($out)
    if ($bytes.Length -lt 4) { return "(no reply)" }
    [Text.Encoding]::UTF8.GetString($bytes, 4, $bytes.Length - 4)
  } finally { Remove-Item $msg, $out -ErrorAction SilentlyContinue }
}

function Hand-Over {
  if ($Send -eq "magnet") {
    Second-Launch @("`"$Magnet`"")
    return "second launch"
  }
  # What the extension sends for a download it takes over from Firefox (background.js).
  Native-Message @{
    url = $Url; filename = "mdm-test-145.zip"; mime = "application/zip"; size = 10485760
    referrer = "http://127.0.0.1:9/"; user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:144.0) Gecko/20100101 Firefox/144.0"
    brands = @(); v = "0.2.4"; explicit = $true
  }
}

function Run-State([string]$St) {
  $data = Join-Path ([IO.Path]::GetTempPath()) ("mdm-145-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
  New-Item -ItemType Directory -Force $data | Out-Null
  # The list's settings: no update check, no first-start questions, the torrent engine kept local.
  $settings = @{
    update_check = $false; helpers_asked = $true; language_asked = $true; extension_never_ask = $true
    torrent_dht = $false; torrent_lsd = $false; torrent_upnp = $false; torrent_incoming = $false
  }
  @{ next_id = 1; items = @(); version = 2; settings = $settings } | ConvertTo-Json -Depth 4 |
    Set-Content -Encoding Ascii (Join-Path $data "downloads.json")
  if ($NoTorrentEngine) {
    Set-Content -Encoding Ascii (Join-Path $data "torrents") "not a folder (tools/lab/magnet-hidden.ps1)"
  }
  $env:MDM_DATA_DIR = $data
  $env:MDM_OFFSCREEN = "1"
  if ($Renderer) { $env:MDM_RENDERER = $Renderer } else { Remove-Item Env:MDM_RENDERER -ErrorAction SilentlyContinue }
  $app = Start-Process -FilePath $Exe -PassThru
  $reply = ""
  try {
    # The main window: the copy's first (its title has the version, Options' starts the same).
    $main = Wait-Window $app.Id "Mister Download Manager*"
    Start-Sleep -Milliseconds 800
    switch ($St) {
      "visible" { }
      "hidden" { [MagWin]::ShowWindow($main.Handle, $SW_HIDE) | Out-Null }
      "tray" { Second-Launch @("/show", "tray") }
      "minimized" { [MagWin]::ShowWindow($main.Handle, $SW_SHOWMINNOACTIVE) | Out-Null }
      "dialog" {
        $had = @(Windows-Of $app.Id | ForEach-Object { $_.Handle })
        Second-Launch @("/show", "options")
        Wait-Window $app.Id "*" -Except $had | Out-Null
        Start-Sleep -Milliseconds 500
        [MagWin]::ShowWindow($main.Handle, $SW_SHOWMINNOACTIVE) | Out-Null
      }
      "twice" { [MagWin]::ShowWindow($main.Handle, $SW_HIDE) | Out-Null }
      "restoring" { [MagWin]::ShowWindow($main.Handle, $SW_HIDE) | Out-Null }
    }
    Start-Sleep -Milliseconds 1500
    $before = (Windows-Of $app.Id | Where-Object { $_.Handle -eq $main.Handle }).How
    if ($St -eq "restoring") {
      # A plain second launch shows the window again; the hand-over follows before it has.
      Start-Process -FilePath $Exe -WindowStyle Hidden | Out-Null
    }
    $reply = Hand-Over
    if ($St -eq "twice") {
      Start-Sleep -Milliseconds 300
      $app.Refresh()
      $reply += " / " + $(if ($app.HasExited) { "(none: the copy had closed)" } else { Hand-Over })
    }
    Start-Sleep -Seconds $WaitSec
    $app.Refresh()
    $alive = -not $app.HasExited
    $wins = if ($alive) { (Windows-Of $app.Id | ForEach-Object { "$($_.Title) [$($_.How)]" }) -join "; " } else { "" }
    $log = Get-ChildItem (Join-Path $data "logs") -Filter *.log -ErrorAction SilentlyContinue |
      Get-Content -ErrorAction SilentlyContinue
    $panic = @($log | Select-String "panic: " | ForEach-Object { $_.Line -replace '^.*panic: ', '' })
    $opened = @($log | Select-String "window opened: " | ForEach-Object { $_.Line -replace '^.*window opened: ', '' } |
      Where-Object { $_ -ne "main" })
    $reports = @(Get-ChildItem (Join-Path $data "crashes") -Filter *.txt -ErrorAction SilentlyContinue).Name
    $row = [pscustomobject]@{
      State = $St; Send = $Send; Exe = $Exe; Renderer = $(if ($Renderer) { $Renderer } else { "default" })
      MainBefore = $before; HandOver = $reply; Survived = $alive; Opened = ($opened -join ", "); Windows = $wins
      Panic = ($panic -join " | "); CrashReports = ($reports -join ", "); Data = $data
    }
    if ($Json) { $row | ConvertTo-Json -Compress } else { $row }
  } finally {
    $app.Refresh()
    if (-not $app.HasExited) {
      # The test copy quits, and its crash monitor with it.
      Second-Launch @("/exit")
      if (-not $app.WaitForExit(15000)) { Stop-Process -Id $app.Id -Force }
    }
    # Its crash monitor: gone with it, or showing a crash report, closed here.
    Start-Sleep -Milliseconds 1500
    Get-CimInstance Win32_Process -Filter "Name = '$([IO.Path]::GetFileName($Exe))'" | Where-Object {
      $_.ExecutablePath -eq $Exe -and [int]$_.ParentProcessId -eq $app.Id
    } | ForEach-Object {
      $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
      if ($p) {
        Windows-Of $p.Id | ForEach-Object { [MagWin]::PostMessage($_.Handle, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
        if (-not $p.WaitForExit(10000)) { Stop-Process -Id $p.Id -Force }
      }
    }
    if (-not $Keep) { Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue }
    Remove-Item Env:MDM_DATA_DIR, Env:MDM_OFFSCREEN, Env:MDM_RENDERER -ErrorAction SilentlyContinue
  }
}

$states = if ($State -eq "all") { "visible", "hidden", "tray", "minimized", "dialog", "twice", "restoring" } else { @($State) }
foreach ($s in $states) { Run-State $s }

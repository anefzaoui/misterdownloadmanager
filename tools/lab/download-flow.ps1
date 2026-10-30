# Captures IDM's download-flow windows (Download File Info, progress dialog, Download complete)
# for one throttled labserver download. Every new IDM window is moved to the VS Code monitor
# the moment it appears (IDM centres them on the primary monitor, where the user works), then
# captured with PrintWindow; buttons are pressed with WM_COMMAND — no mouse or keyboard.
param([Parameter(Mandatory)][string]$OutDir, [int]$Port = 8095, [string]$Size = "200M", [string]$Rate = "1M")
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class DF {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string c, string t);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
  public static List<IntPtr> Visible(uint pid) { var o = new List<IntPtr>(); EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) o.Add(h); return true; }, IntPtr.Zero); return o; }
  public static string Title(IntPtr h) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256); return sb.ToString(); }
}
"@
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
New-Item -ItemType Directory -Force $OutDir | Out-Null
$dl = Join-Path $root ".study\lab\dl"; New-Item -ItemType Directory -Force $dl | Out-Null
$file = "flow-$Size.bin"; Remove-Item "$dl\$file" -ErrorAction SilentlyContinue
$area = (& "$PSScriptRoot\vscode-monitor.ps1") -split ' ' | ForEach-Object { [int]$_ }
$idmPid = [uint32](Get-Process IDMan).Id
$known = New-Object System.Collections.Generic.HashSet[IntPtr]
foreach ($h in [DF]::Visible($idmPid)) { [void]$known.Add($h) }

function Wait-New([string]$like, [int]$sec) {
  $end = (Get-Date).AddSeconds($sec)
  while ((Get-Date) -lt $end) {
    foreach ($h in [DF]::Visible($idmPid)) {
      if (-not $known.Contains($h) -and [DF]::Title($h) -like $like) {
        [void]$known.Add($h)
        [DF]::SetWindowPos($h, [IntPtr]::Zero, $area[0] + 960, $area[1] + 100, 0, 0, 0x15) | Out-Null
        return $h
      }
    }
    Start-Sleep -Milliseconds 100
  }
  throw "no window like '$like' within $sec s"
}
function Shot($h, $name) { Start-Sleep -Milliseconds 700; & "$PSScriptRoot\capwin.ps1" -Title ([DF]::Title($h)) -Out (Join-Path $OutDir $name) }
function Press($h, [string]$caption) {
  foreach ($c in @($caption, "&$caption")) {
    $b = [DF]::FindWindowEx($h, [IntPtr]::Zero, "Button", $c)
    if ($b -ne [IntPtr]::Zero) { [DF]::PostMessage($h, 0x111, [IntPtr][DF]::GetDlgCtrlID($b), $b) | Out-Null; return }
  }
  throw "button '$caption' not found"
}

Get-Process labserver -ErrorAction SilentlyContinue | Stop-Process
$srv = Start-Process -PassThru -WindowStyle Hidden "$root\target\debug\labserver.exe" `
  -ArgumentList "--bind 127.0.0.1:$Port --faults `"rate=$Rate`" --log-file `"$OutDir\flow.jsonl`""
try {
  Start-Sleep -Milliseconds 800
  & "C:\Program Files (x86)\Internet Download Manager\IDMan.exe" /d "http://127.0.0.1:$Port/gen/$Size/$file" /p $dl /f $file
  $info = Wait-New "Download File Info*" 20
  Shot $info "file-info.png"
  Press $info "Start Download"
  $prog = Wait-New "*$file*" 20
  Start-Sleep -Seconds 3
  Shot $prog "progress.png"
  $done = Wait-New "Download complete*" 600
  Shot $done "complete.png"
  [DF]::PostMessage($done, 0x111, [IntPtr]2, [IntPtr]::Zero) | Out-Null   # IDCANCEL = Close
} finally {
  Stop-Process $srv.Id -ErrorAction SilentlyContinue
}

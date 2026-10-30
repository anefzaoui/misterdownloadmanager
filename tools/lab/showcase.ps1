# Website screenshots: runs a separate Mister Download Manager instance (its own data folder via
# MDM_DATA_DIR, so the real download list is untouched), fills it from labserver with realistic
# file names (some complete, one still downloading), and captures the main window in dark and
# light mode plus the progress dialog. The user's own instance is stopped for the run (same
# window title) and restarted afterwards.
param([string]$OutDir = "website\assets\screens", [int]$Port = 8097)
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $root
$work = Join-Path $root ".study\showcase"
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$work\data", "$work\dl" | Out-Null
$exe = Join-Path $root "target\release\misterdownloadmanager.exe"
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class SC {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
"@
function Close-Titled([string]$like) {
  [SC]::EnumWindows({ param($h, $l)
    $sb = New-Object System.Text.StringBuilder 256; [SC]::GetWindowText($h, $sb, 256) | Out-Null
    if ([SC]::IsWindowVisible($h) -and $sb.ToString() -like $like) { [SC]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
    return $true }, [IntPtr]::Zero) | Out-Null
}

$hadUserInstance = [bool](Get-Process misterdownloadmanager -ErrorAction SilentlyContinue)
Get-Process misterdownloadmanager -ErrorAction SilentlyContinue | Stop-Process
Get-Process labserver -ErrorAction SilentlyContinue | Stop-Process
$srv = Start-Process -PassThru -WindowStyle Hidden "$root\target\release\labserver.exe" -ArgumentList "--bind 127.0.0.1:$Port"
$env:MDM_DATA_DIR = "$work\data"
try {
  Start-Sleep -Milliseconds 800
  Start-Process $exe
  Start-Sleep 3
  & "$PSScriptRoot\to-vscode-monitor.ps1" -Title "Mister Download Manager" -DX 185 -DY 570 -Width 929 -Height 465 | Out-Null
  $files = @(
    @("big-buck-bunny-1080p.mp4", "180M", ""), @("project-report-2026.pdf", "4M", ""),
    @("holiday-photos.zip", "64M", ""), @("podcast-episode-42.mp3", "38M", ""),
    @("rust-1.99.1-x86_64-pc-windows-msvc.msi", "92M", ""), @("open-data-2026.csv", "12M", ""),
    @("ubuntu-26.04-desktop-amd64.iso", "6G", "?rate=2M")
  )
  foreach ($f in $files) {
    & $exe /d "http://127.0.0.1:$Port/gen/$($f[1])/$($f[0])$($f[2])" /p "$work\dl" /n
    Start-Sleep -Milliseconds 400
  }
  # Let the small ones finish; close their "Download complete" dialogs.
  for ($i = 0; $i -lt 30; $i++) { Start-Sleep 1; Close-Titled "Download complete" }
  Close-Titled "*ubuntu-26.04*"   # its progress window, for a clean main-window shot
  Start-Sleep 2
  & "$PSScriptRoot\cap-own.ps1" -Out "$root\$OutDir\main-dark.png"
  # The progress dialog of the ISO: double-click its row (last row).
  & "$PSScriptRoot\probe-own.ps1" -X 300 -Y 259 -Double -NoEscape
  Start-Sleep 2
  & "$PSScriptRoot\capwin.ps1" -Title "*ubuntu-26.04*" -Out "$root\$OutDir\progress-dark.png"
  Close-Titled "*ubuntu-26.04*"
  # Light mode: View → Dark Mode support.
  & "$PSScriptRoot\probe-own.ps1" -X 190 -Y 40 -HoverX 210 -HoverY 174 -HoverClick -NoEscape
  Start-Sleep 3
  & "$PSScriptRoot\cap-own.ps1" -Out "$root\$OutDir\main-light.png"
  & "$PSScriptRoot\probe-own.ps1" -X 300 -Y 259 -Double -NoEscape
  Start-Sleep 2
  & "$PSScriptRoot\capwin.ps1" -Title "*ubuntu-26.04*" -Out "$root\$OutDir\progress-light.png"
} finally {
  Get-Process misterdownloadmanager -ErrorAction SilentlyContinue | Stop-Process
  Stop-Process $srv.Id -ErrorAction SilentlyContinue
  Remove-Item Env:MDM_DATA_DIR
  if ($hadUserInstance) { Start-Process $exe; Start-Sleep 2; & "$PSScriptRoot\to-vscode-monitor.ps1" -Title "Mister Download Manager" -DX 185 -DY 570 -Width 929 -Height 465 | Out-Null }
}

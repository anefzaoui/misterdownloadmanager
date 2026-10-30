# Clicks in our own window and captures our windows, politely: waits until the user has been
# idle for -IdleMs, then clicks, captures, presses Escape (unless -NoEscape), and gives the
# user's foreground window and cursor back. X/Y are relative to the window's visible top-left.
param(
  [string]$Title = "Mister Download Manager 0",
  [Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y,
  [switch]$Right, [switch]$Double, [switch]$NoEscape,
  # Hover this point (visible-relative) after the click, before capturing (submenus).
  [int]$HoverX = -1, [int]$HoverY = -1,
  # Also left-click at the hover point (e.g. an item of the menu the first click opened).
  [switch]$HoverClick,
  [string]$Out = "",
  # SendKeys text typed after the click (our window is verified to be in front), e.g. "{DELETE}".
  [string]$Key = "",
  # Drag from (X, Y) to (ToX, ToY) with the left button instead of clicking.
  [int]$ToX = -1, [int]$ToY = -1,
  # Only move the pointer there (hover states); no click.
  [switch]$NoClick,
  # Whose windows to capture (cap-own -ProcName), e.g. IDMan.
  [string]$CapProc = "misterdownloadmanager",
  [int]$IdleMs = 2000,
  # Only this process's windows (a test instance next to the user's own, same titles).
  [int]$ProcessId = 0
)
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class PO {
  [StructLayout(LayoutKind.Sequential)] public struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LASTINPUTINFO i);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  public static bool Activate(IntPtr h) {
    var fg = GetForegroundWindow(); if (fg == h) return true;
    uint me = GetCurrentThreadId(), them = GetWindowThreadProcessId(fg, IntPtr.Zero);
    AttachThreadInput(me, them, true); bool ok = SetForegroundWindow(h); AttachThreadInput(me, them, false);
    return ok && GetForegroundWindow() == h;
  }
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  public static uint IdleMs() { var i = new LASTINPUTINFO(); i.cbSize = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.dwTime; }
}
"@
$script:h = [IntPtr]::Zero
[PO]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [PO]::GetWindowText($w, $sb, 256) | Out-Null
  $owner = 0; [PO]::GetWindowThreadProcessId($w, [ref]$owner) | Out-Null
  $mine = ($ProcessId -eq 0) -or ($owner -eq $ProcessId)
  if ($mine -and [PO]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:h = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:h -eq [IntPtr]::Zero) { throw "no visible window titled '$Title*'" }
$waited = 0
while ([PO]::IdleMs() -lt $IdleMs) { Start-Sleep -Milliseconds 250; $waited += 250; if ($waited -gt 120000) { throw "user busy for 2 minutes" } }
$prev = [PO]::GetForegroundWindow()
$cur = New-Object PO+POINT; [PO]::GetCursorPos([ref]$cur) | Out-Null
try {
  # Safety: click only when our window is in front AND is the window under the point —
  # otherwise the click would land in the user's program.
  if (-not [PO]::Activate($script:h)) { throw "could not bring our window to the front; not clicking" }
  Start-Sleep -Milliseconds 150
  $r = New-Object PO+RECT; [PO]::DwmGetWindowAttribute($script:h, 9, [ref]$r, 16) | Out-Null
  $pt = New-Object PO+POINT; $pt.X = $r.L + $X; $pt.Y = $r.T + $Y
  # Arrive via a neighbouring pixel: winit reports no move when the cursor re-enters at the
  # very pixel it left from, and egui then ignores the click (no pointer position).
  [PO]::SetCursorPos($pt.X + 1, $pt.Y + 1) | Out-Null
  Start-Sleep -Milliseconds 40
  [PO]::SetCursorPos($pt.X, $pt.Y) | Out-Null
  Start-Sleep -Milliseconds 60
  $under = [PO]::GetAncestor([PO]::WindowFromPoint($pt), 2)  # GA_ROOT
  $pid1 = 0; [PO]::GetWindowThreadProcessId($under, [ref]$pid1) | Out-Null
  $pid2 = 0; [PO]::GetWindowThreadProcessId($script:h, [ref]$pid2) | Out-Null
  if ($pid1 -ne $pid2 -or [PO]::GetForegroundWindow() -ne $script:h) { throw "target point is not on our window; not clicking" }
  $down, $up = if ($Right) { 0x8, 0x10 } else { 0x2, 0x4 }
  $n = if ($Double) { 2 } else { 1 }
  if ($NoClick) {
    # Hover: a second 1 px move, then time for a repaint (egui draws hover on the next move).
    $n = 0; Start-Sleep -Milliseconds 200
    $c = New-Object PO+POINT; [PO]::GetCursorPos([ref]$c) | Out-Null
    [PO]::SetCursorPos($c.X + 1, $c.Y + 1) | Out-Null; Start-Sleep -Milliseconds 800
  }
  if ($ToX -ge 0) {
    $n = 0
    [PO]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero)
    for ($s = 1; $s -le 12; $s++) {
      [PO]::SetCursorPos([int]($pt.X + ($r.L + $ToX - $pt.X) * $s / 12), [int]($pt.Y + ($r.T + $ToY - $pt.Y) * $s / 12)) | Out-Null
      Start-Sleep -Milliseconds 30
    }
    [PO]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero)
  }
  for ($i = 0; $i -lt $n; $i++) {
    [PO]::mouse_event($down, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 30
    [PO]::mouse_event($up, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60
  }
  Start-Sleep -Milliseconds 300
  if ($Key) {
    if ([PO]::GetForegroundWindow() -ne $script:h) { throw "our window lost the front; not typing" }
    [System.Windows.Forms.SendKeys]::SendWait($Key); Start-Sleep -Milliseconds 700
  }
  if ($HoverX -ge 0) {
    [PO]::SetCursorPos($r.L + $HoverX, $r.T + $HoverY) | Out-Null; Start-Sleep -Milliseconds 400
    if ($HoverClick) { [PO]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 30; [PO]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 500 }
  }
  if ($Out) { & "$PSScriptRoot\cap-own.ps1" -ProcName $CapProc -Out $Out -ProcessId $ProcessId }
  & "$PSScriptRoot\windows.ps1" -ProcName misterdownloadmanager | Select-String "menu"
  if (-not $NoEscape) { [System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 100 }
} finally {
  [PO]::SetCursorPos($cur.X, $cur.Y) | Out-Null
  if ($prev -ne [IntPtr]::Zero) { [PO]::Activate($prev) | Out-Null }
}

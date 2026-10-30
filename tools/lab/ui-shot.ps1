# Opens an IDM dialog through its own menu command (as a click would), screenshots it — and
# optionally each of its tabs (Ctrl+Tab, only while the user is idle) — then closes it with
# IDCANCEL (Cancel / Escape semantics, nothing saved).
# -Top is the top-level menu index (0 Tasks, 1 File, 2 Downloads, 3 View, 4 Help);
# -Item is the item text without '&' (submenus: "Speed Limiter/Settings...").
param(
  [Parameter(Mandatory)][int]$Top,
  [Parameter(Mandatory)][string]$Item,
  [Parameter(Mandatory)][string]$Out,
  [int]$Tabs = 1,
  [switch]$KeepOpen
)
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class U {
  [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int i);
  [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr m, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint id, StringBuilder s, int n, uint f);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LASTINPUTINFO i);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint c);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
  [DllImport("user32.dll", EntryPoint="GetWindowThreadProcessId")] public static extern uint GetWindowThreadProcessId2(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  public static uint IdleMs() { var i = new LASTINPUTINFO(); i.cbSize = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.dwTime; }
  public static bool Activate(IntPtr h) {
    var fg = GetForegroundWindow(); if (fg == h) return true;
    uint me = GetCurrentThreadId(), them = GetWindowThreadProcessId(fg, IntPtr.Zero);
    AttachThreadInput(me, them, true); bool ok = SetForegroundWindow(h); AttachThreadInput(me, them, false);
    return ok && GetForegroundWindow() == h;
  }
}
"@
function Text($m, $i) { $sb = New-Object System.Text.StringBuilder 256; [U]::GetMenuString($m, $i, $sb, 256, 0x400) | Out-Null; $sb.ToString().Replace("&", "") }
function Shot($h, $path) {
  $r = New-Object U+RECT; [U]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
  [U]::PrintWindow($h, $hdc, 2) | Out-Null
  $g.ReleaseHdc($hdc); $g.Dispose(); $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}

$script:main = [IntPtr]::Zero
[U]::EnumWindows({ param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256; [U]::GetWindowText($h, $sb, 256) | Out-Null
  if ($sb.ToString() -like "Internet Download Manager*" -and [U]::GetMenu($h) -ne [IntPtr]::Zero) { $script:main = $h; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:main -eq [IntPtr]::Zero) { throw "IDM main window not found" }

$menu = [U]::GetSubMenu([U]::GetMenu($script:main), $Top)
foreach ($part in $Item.Split("/")) {
  $found = $false
  for ($i = 0; $i -lt [U]::GetMenuItemCount($menu); $i++) {
    if ((Text $menu $i) -eq $part) {
      $sub = [U]::GetSubMenu($menu, $i)
      if ($sub -ne [IntPtr]::Zero) { $menu = $sub } else { $id = [U]::GetMenuItemID($menu, $i) }
      $found = $true; break
    }
  }
  if (-not $found) { throw "menu item '$part' not found" }
}
# Polite (the user works on this machine): no focus is taken except briefly for tab switching,
# and keys are only typed while the dialog is verified to be in front.
$prevFg = [U]::GetForegroundWindow()
$idmPid = (Get-Process IDMan).Id
function Visible() {
  $list = New-Object System.Collections.ArrayList
  [U]::EnumWindows({ param($h, $l)
    $p = 0; [U]::GetWindowThreadProcessId2($h, [ref]$p) | Out-Null
    if ($p -eq $idmPid -and [U]::IsWindowVisible($h)) { [void]$list.Add($h) }
    return $true }, [IntPtr]::Zero) | Out-Null
  , $list
}
$before = Visible
[U]::PostMessage($script:main, 0x111, [IntPtr]$id, [IntPtr]::Zero) | Out-Null   # WM_COMMAND
# The new dialog: an IDM window that was not visible before the command.
$dlg = [IntPtr]::Zero
for ($w = 0; $w -lt 30 -and $dlg -eq [IntPtr]::Zero; $w++) {
  Start-Sleep -Milliseconds 200
  foreach ($h in (Visible)) { if (-not $before.Contains($h)) { $dlg = $h; break } }
}
if ($dlg -eq [IntPtr]::Zero) { throw "no dialog appeared for '$Item'" }
# IDM centres dialogs on the primary monitor, where the user works: move it to the VS Code one.
$area = (& "$PSScriptRoot\vscode-monitor.ps1") -split ' ' | ForEach-Object { [int]$_ }
[U]::SetWindowPos($dlg, [IntPtr]::Zero, $area[0] + 1000, $area[1] + 120, 0, 0, 0x15) | Out-Null  # NOSIZE|NOZORDER|NOACTIVATE
Start-Sleep -Milliseconds 600
try {
  for ($t = 0; $t -lt $Tabs; $t++) {
    $path = if ($Tabs -gt 1) { $Out -replace "\.png$", "-$t.png" } else { $Out }
    Shot $dlg $path
    $sb = New-Object System.Text.StringBuilder 256; [U]::GetWindowText($dlg, $sb, 256) | Out-Null
    "$($sb.ToString()) -> $path"
    if ($t -lt $Tabs - 1) {
      $waited = 0
      while ([U]::IdleMs() -lt 2000) { Start-Sleep -Milliseconds 250; $waited += 250; if ($waited -gt 120000) { throw "user busy" } }
      if (-not [U]::Activate($dlg)) { throw "could not bring the dialog to the front; not typing" }
      [System.Windows.Forms.SendKeys]::SendWait("^{TAB}"); Start-Sleep -Milliseconds 500
    }
  }
} finally {
  if (-not $KeepOpen) { [U]::PostMessage($dlg, 0x111, [IntPtr]2, [IntPtr]::Zero) | Out-Null }  # IDCANCEL
  if ($prevFg -ne [IntPtr]::Zero) { [U]::Activate($prevFg) | Out-Null }
}

# Right-clicks a point in a window (window-relative), then dumps the context menu that opens
# (all levels, via MN_GETHMENU on the #32768 popup) and screenshots only the popup's own rect.
# Closes the menu with Escape, so no command runs. Black-box UI observation (BEHAVIOR-LAB.md).
param(
  [string]$Title = "Internet Download Manager",
  [Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y,
  [string]$Out = "",
  # Left click first (select the item under the point), as a user would.
  [switch]$Select,
  # Left click instead of right (dropdown buttons).
  [switch]$Left,
  # Hover this item (label prefix) to open its submenu; dump and screenshot that too.
  [string]$Sub = ""
)
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class K {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint id, StringBuilder s, int n, uint f);
  [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr m, uint id, uint f);
  [DllImport("user32.dll")] public static extern uint GetMenuDefaultItem(IntPtr m, uint byPos, uint flags);
  [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr h, IntPtr m, uint i, out RECT r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string c, string t);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LASTINPUTINFO i);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  public static uint IdleMs() { var i = new LASTINPUTINFO(); i.cbSize = 8; GetLastInputInfo(ref i); return (uint)Environment.TickCount - i.dwTime; }
  public static bool Activate(IntPtr h) {
    var fg = GetForegroundWindow(); if (fg == h) return true;
    uint me = GetCurrentThreadId(), them = GetWindowThreadProcessId(fg, IntPtr.Zero);
    AttachThreadInput(me, them, true); bool ok = SetForegroundWindow(h); AttachThreadInput(me, them, false);
    return ok && GetForegroundWindow() == h;
  }
}
"@
function Dump($menu, $depth) {
  $n = [K]::GetMenuItemCount($menu)
  $def = [K]::GetMenuDefaultItem($menu, 1, 0)
  for ($i = 0; $i -lt $n; $i++) {
    $sb = New-Object System.Text.StringBuilder 256
    [K]::GetMenuString($menu, $i, $sb, 256, 0x400) | Out-Null
    $state = [K]::GetMenuState($menu, $i, 0x400)
    $text = $sb.ToString()
    if ($state -band 0x800) { $text = "----" }
    $flags = @(); if ($state -band 0x3) { $flags += "disabled" }; if ($state -band 0x8) { $flags += "checked" }
    if ($def -eq $i) { $flags += "default" }
    $suffix = if ($flags) { "  [" + ($flags -join ",") + "]" } else { "" }
    ("  " * $depth) + $text + $suffix
    $sub = [K]::GetSubMenu($menu, $i)
    if ($sub -ne [IntPtr]::Zero) { Dump $sub ($depth + 1) }
  }
}
$script:h = [IntPtr]::Zero
[K]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [K]::GetWindowText($w, $sb, 256) | Out-Null
  if ([K]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:h = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:h -eq [IntPtr]::Zero) { throw "no visible window titled '$Title*'" }
# Politeness and safety (the user works on this machine): wait for 2 s of idle, bring the
# window forward properly, and click only if it is in front and under the point.
$waited = 0
while ([K]::IdleMs() -lt 2000) { Start-Sleep -Milliseconds 250; $waited += 250; if ($waited -gt 120000) { throw "user busy" } }
$prevFg = [K]::GetForegroundWindow()
$prevCur = New-Object K+POINT; [K]::GetCursorPos([ref]$prevCur) | Out-Null
if (-not [K]::Activate($script:h)) { throw "could not bring '$Title' to the front; not clicking" }
Start-Sleep -Milliseconds 300
$r = New-Object K+RECT; [K]::GetWindowRect($script:h, [ref]$r) | Out-Null
$pt = New-Object K+POINT; $pt.X = $r.L + $X; $pt.Y = $r.T + $Y
[K]::SetCursorPos($pt.X + 1, $pt.Y + 1) | Out-Null; Start-Sleep -Milliseconds 40
[K]::SetCursorPos($pt.X, $pt.Y) | Out-Null; Start-Sleep -Milliseconds 60
$under = [K]::GetAncestor([K]::WindowFromPoint($pt), 2)
if ($under -ne $script:h -or [K]::GetForegroundWindow() -ne $script:h) {
  [K]::SetCursorPos($prevCur.X, $prevCur.Y) | Out-Null
  throw "point is not on '$Title'; not clicking"
}
if ($Select) { [K]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero); [K]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 250 }
if ($Left) { [K]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero); [K]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero) }
else { [K]::mouse_event(0x8, 0, 0, 0, [IntPtr]::Zero); [K]::mouse_event(0x10, 0, 0, 0, [IntPtr]::Zero) }
$popup = [IntPtr]::Zero
for ($i = 0; $i -lt 20 -and $popup -eq [IntPtr]::Zero; $i++) {
  Start-Sleep -Milliseconds 100
  $p = [K]::FindWindow("#32768", $null)
  if ($p -ne [IntPtr]::Zero -and [K]::IsWindowVisible($p)) { $popup = $p }
}
if ($popup -eq [IntPtr]::Zero) {
  [K]::SetCursorPos($prevCur.X, $prevCur.Y) | Out-Null
  if ($prevFg -ne [IntPtr]::Zero) { [K]::Activate($prevFg) | Out-Null }
  "(no context menu at $X,$Y)"; return
}
Start-Sleep -Milliseconds 250  # let the fade-in finish
$pr = New-Object K+RECT; [K]::GetWindowRect($popup, [ref]$pr) | Out-Null
"menu at ({0},{1}) window-relative, {2}x{3}" -f ($pr.L - $r.L), ($pr.T - $r.T), ($pr.R - $pr.L), ($pr.B - $pr.T)
$hmenu = [K]::SendMessage($popup, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)  # MN_GETHMENU
if ($hmenu -ne [IntPtr]::Zero) { Dump $hmenu 0 }
if ($Out) {
  # Only the popup's own pixels (it is topmost while open).
  $bmp = New-Object System.Drawing.Bitmap ($pr.R - $pr.L), ($pr.B - $pr.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($pr.L, $pr.T, 0, 0, $bmp.Size); $g.Dispose()
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  "-> $Out"
}
if ($Sub -and $hmenu -ne [IntPtr]::Zero) {
  $n = [K]::GetMenuItemCount($hmenu)
  for ($i = 0; $i -lt $n; $i++) {
    $sb = New-Object System.Text.StringBuilder 256
    [K]::GetMenuString($hmenu, $i, $sb, 256, 0x400) | Out-Null
    if ($sb.ToString() -like "$Sub*") {
      $ir = New-Object K+RECT; [K]::GetMenuItemRect([IntPtr]::Zero, $hmenu, $i, [ref]$ir) | Out-Null
      [K]::SetCursorPos($ir.L + 30, [int](($ir.T + $ir.B) / 2)) | Out-Null
      Start-Sleep -Milliseconds 900
      $sp = [IntPtr]::Zero; $w = [IntPtr]::Zero
      while (($w = [K]::FindWindowEx([IntPtr]::Zero, $w, "#32768", $null)) -ne [IntPtr]::Zero) {
        if ($w -ne $popup -and [K]::IsWindowVisible($w)) { $sp = $w }
      }
      if ($sp -eq [IntPtr]::Zero) { "(no submenu for '$Sub')"; break }
      $sr = New-Object K+RECT; [K]::GetWindowRect($sp, [ref]$sr) | Out-Null
      "submenu '$Sub' item rect ({0},{1})-({2},{3}); submenu at ({4},{5}) {6}x{7} (relative to the menu)" -f ($ir.L - $pr.L), ($ir.T - $pr.T), ($ir.R - $pr.L), ($ir.B - $pr.T), ($sr.L - $pr.L), ($sr.T - $pr.T), ($sr.R - $sr.L), ($sr.B - $sr.T)
      $sm = [K]::SendMessage($sp, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)
      if ($sm -ne [IntPtr]::Zero) { Dump $sm 1 }
      if ($Out) {
        # Menu plus submenu, side by side as on screen.
        $L = [Math]::Min($pr.L, $sr.L); $T = [Math]::Min($pr.T, $sr.T)
        $R = [Math]::Max($pr.R, $sr.R); $B = [Math]::Max($pr.B, $sr.B)
        $bmp = New-Object System.Drawing.Bitmap ($R - $L), ($B - $T)
        $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::Black)
        $g.CopyFromScreen($pr.L, $pr.T, $pr.L - $L, $pr.T - $T, (New-Object System.Drawing.Size ($pr.R - $pr.L), ($pr.B - $pr.T)))
        $g.CopyFromScreen($sr.L, $sr.T, $sr.L - $L, $sr.T - $T, (New-Object System.Drawing.Size ($sr.R - $sr.L), ($sr.B - $sr.T)))
        $g.Dispose(); $o2 = $Out -replace '\.png$', '-sub.png'; $bmp.Save($o2); $bmp.Dispose(); "-> $o2"
      }
      [System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 150
      break
    }
  }
}
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 150
if ([K]::IsWindowVisible($popup)) { [System.Windows.Forms.SendKeys]::SendWait("{ESC}") }
[K]::SetCursorPos($prevCur.X, $prevCur.Y) | Out-Null
if ($prevFg -ne [IntPtr]::Zero) { [K]::Activate($prevFg) | Out-Null }

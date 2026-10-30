# Dumps a window's menu bar (all levels) as indented text, via the standard Win32 menu API.
# Black-box UI observation for the behaviour lab (notes/BEHAVIOR-LAB.md).
param([string]$Title = "Internet Download Manager*")
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class M {
  [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint id, StringBuilder s, int n, uint f);
  [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr m, uint id, uint f);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
}
"@
function Dump($menu, $depth) {
  $n = [M]::GetMenuItemCount($menu)
  for ($i = 0; $i -lt $n; $i++) {
    $sb = New-Object System.Text.StringBuilder 256
    [M]::GetMenuString($menu, $i, $sb, 256, 0x400) | Out-Null
    $state = [M]::GetMenuState($menu, $i, 0x400)
    $text = $sb.ToString()
    if ($state -band 0x800) { $text = "----" }
    $flags = @(); if ($state -band 0x3) { $flags += "disabled" }; if ($state -band 0x8) { $flags += "checked" }
    $suffix = if ($flags) { "  [" + ($flags -join ",") + "]" } else { "" }
    ("  " * $depth) + $text + $suffix
    $sub = [M]::GetSubMenu($menu, $i)
    if ($sub -ne [IntPtr]::Zero) { Dump $sub ($depth + 1) }
  }
}
# The process's "main window" can be a progress dialog, so find the top-level window by title.
$script:hwnd = [IntPtr]::Zero
[M]::EnumWindows({ param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [M]::GetWindowText($h, $sb, 256) | Out-Null
  if ($sb.ToString() -like $Title -and [M]::GetMenu($h) -ne [IntPtr]::Zero) { $script:hwnd = $h; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:hwnd -eq [IntPtr]::Zero) { throw "no window titled '$Title' with a menu bar" }
$menu = [M]::GetMenu($script:hwnd)
if ($menu -eq [IntPtr]::Zero) { throw "window has no Win32 menu bar" }
Dump $menu 0

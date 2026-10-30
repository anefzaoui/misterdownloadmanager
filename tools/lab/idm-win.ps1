# Reads or sets the outer rect of IDM's main window (for captures that need another size).
# Without -W it prints "x y w h"; with -X -Y -W -H it moves/resizes without activating.
param([int]$X = 0, [int]$Y = 0, [int]$W = 0, [int]$H = 0, [string]$Title = "Internet Download Manager*")
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class IW {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$script:hwnd = [IntPtr]::Zero
[IW]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [IW]::GetWindowText($w, $sb, 256) | Out-Null
  if ([IW]::IsWindowVisible($w) -and $sb.ToString() -like $Title -and [IW]::GetMenu($w) -ne [IntPtr]::Zero) { $script:hwnd = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:hwnd -eq [IntPtr]::Zero) { throw "IDM main window not found" }
if ($W -gt 0) {
  [IW]::SetWindowPos($script:hwnd, [IntPtr]::Zero, $X, $Y, $W, $H, 0x14) | Out-Null  # NOZORDER|NOACTIVATE
}
$r = New-Object IW+RECT; [IW]::GetWindowRect($script:hwnd, [ref]$r) | Out-Null
"{0} {1} {2} {3}" -f $r.L, $r.T, ($r.R - $r.L), ($r.B - $r.T)

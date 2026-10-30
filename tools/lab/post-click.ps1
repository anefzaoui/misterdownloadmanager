# Clicks inside one of OUR windows by posting mouse messages to it — the real cursor never
# moves and focus is never taken, so the user can keep working. X/Y are relative to the
# window's visible top-left (as in our captures). -Move only hovers.
param(
  [Parameter(Mandatory)][string]$Title, [Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y,
  [switch]$Right, [switch]$Move, [switch]$Double
)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class PC {
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
$script:h = [IntPtr]::Zero
[PC]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [PC]::GetWindowText($w, $sb, 256) | Out-Null
  if ([PC]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:h = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:h -eq [IntPtr]::Zero) { throw "no visible window titled '$Title*'" }
$r = New-Object PC+RECT; [PC]::DwmGetWindowAttribute($script:h, 9, [ref]$r, 16) | Out-Null
$p = New-Object PC+POINT; $p.X = $r.L + $X; $p.Y = $r.T + $Y
[PC]::ScreenToClient($script:h, [ref]$p) | Out-Null
$lp = [IntPtr](($p.Y -shl 16) -bor ($p.X -band 0xffff))
# Everything is queued back to back: the window's mouse tracking sees the real cursor
# elsewhere and posts WM_MOUSELEAVE, which must arrive after our button messages.
[PC]::PostMessage($script:h, 0x200, [IntPtr]::Zero, $lp) | Out-Null   # WM_MOUSEMOVE
if ($Move) { return }
$down, $up, $btn = if ($Right) { 0x204, 0x205, 2 } else { 0x201, 0x202, 1 }
$n = if ($Double) { 2 } else { 1 }
for ($i = 0; $i -lt $n; $i++) {
  [PC]::PostMessage($script:h, $down, [IntPtr]$btn, $lp) | Out-Null
  [PC]::PostMessage($script:h, $up, [IntPtr]::Zero, $lp) | Out-Null
}

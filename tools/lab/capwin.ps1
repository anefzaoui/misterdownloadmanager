# Screenshots one window (never the whole screen) of one process: the visible top-level window of
# -Pid whose title starts with -Title (wildcards allowed), or its only one when -Title is left out.
# Several copies of an app share a window title, so the process ID is required; no window, or more
# than one, is an error that lists what the process shows. PrintWindow lets the window draw itself,
# so it works off-screen and behind other windows: nothing is shown, restored, activated or brought
# forward (a minimized window can't be drawn and is an error).
param(
  [Parameter(Mandatory)][Alias("Pid")][int]$ProcessId,
  [string]$Title = "",
  [Parameter(Mandatory)][string]$Out
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class CapWin {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public static string Title(IntPtr h) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
  // Visible, titled top-level windows of one process.
  public static List<IntPtr> Of(uint pid) {
    var o = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h) && Title(h).Length > 0) o.Add(h); return true; }, IntPtr.Zero);
    return o;
  }
}
"@
if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) { throw "no process $ProcessId" }
$all = [CapWin]::Of([uint32]$ProcessId)
$found = @($all | Where-Object { [CapWin]::Title($_) -like "$Title*" })
if ($found.Count -ne 1) {
  $titles = ($all | ForEach-Object { "  '$([CapWin]::Title($_))'" }) -join "`n"
  $what = if ($found.Count -eq 0) { "no window" } else { "$($found.Count) windows" }
  throw "$what of process $ProcessId titled '$Title*'; it shows:`n$titles"
}
$h = $found[0]
if ([CapWin]::IsIconic($h)) { throw "'$([CapWin]::Title($h))' is minimized; PrintWindow can't draw it" }
$r = New-Object CapWin+RECT; [CapWin]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
$g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
[CapWin]::PrintWindow($h, $hdc, 2) | Out-Null   # PW_RENDERFULLCONTENT
$g.ReleaseHdc($hdc); $g.Dispose()
# Keep the visible bounds (DWMWA_EXTENDED_FRAME_BOUNDS: no invisible resize border), less the 1 px
# window border Windows 11 draws round them, which PrintWindow paints black.
$v = New-Object CapWin+RECT; [CapWin]::DwmGetWindowAttribute($h, 9, [ref]$v, 16) | Out-Null
$crop = New-Object System.Drawing.Rectangle ($v.L - $r.L + 1), ($v.T - $r.T + 1), ($v.R - $v.L - 2), ($v.B - $v.T - 2)
$shot = $bmp.Clone($crop, $bmp.PixelFormat); $bmp.Dispose()
$shot.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $shot.Dispose()
"$([CapWin]::Title($h)) (process $ProcessId) -> $Out"

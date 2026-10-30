# Captures the screen area of a window (found by title prefix) plus padding — so popup menus
# and drop-downs over it are included. The window is brought to the front first; only its own
# rectangle (+ pad) is captured, never the whole desktop.
param([Parameter(Mandatory)][string]$Title, [Parameter(Mandatory)][string]$Out, [int]$PadRight = 0, [int]$PadBottom = 0, [switch]$NoFront)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class R {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$script:w = [IntPtr]::Zero
[R]::EnumWindows({ param($x, $l)
  $sb = New-Object System.Text.StringBuilder 256; [R]::GetWindowText($x, $sb, 256) | Out-Null
  if ([R]::IsWindowVisible($x) -and $sb.ToString() -like "$Title*") { $script:w = $x; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:w -eq [IntPtr]::Zero) { throw "no visible window '$Title*'" }
if (-not $NoFront) { [R]::SetForegroundWindow($script:w) | Out-Null; Start-Sleep -Milliseconds 400 }
$r = New-Object R+RECT; [R]::GetWindowRect($script:w, [ref]$r) | Out-Null
$wd = $r.R - $r.L + $PadRight; $ht = $r.B - $r.T + $PadBottom
$bmp = New-Object System.Drawing.Bitmap $wd, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
$g.Dispose(); $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
"captured $wd x $ht at ($($r.L),$($r.T)) -> $Out"

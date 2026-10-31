# Screenshots a program's visible top-level windows (main window plus any menu / dialog
# windows), composited over black. Each window renders itself (PrintWindow), so pixels of other
# programs never appear — even when a window is covered by the user's editor.
# -Title: only windows whose title starts with it (one window for a screenshot).
param([string]$ProcName = "misterdownloadmanager", [Parameter(Mandatory)][string]$Out, [int]$ProcessId = 0,
  [string]$Title = "")
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class CO {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
# -ProcessId: one instance only (a test instance next to the user's own).
$ids = if ($ProcessId) { @($ProcessId) } else { @(Get-Process $ProcName -ErrorAction Stop | ForEach-Object { $_.Id }) }
$wins = New-Object System.Collections.ArrayList
[CO]::EnumWindows({ param($h, $l)
  $p = 0; [CO]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
  $sb = New-Object System.Text.StringBuilder 256; [CO]::GetWindowText($h, $sb, 256) | Out-Null
  $titled = (-not $Title) -or ($sb.ToString() -like "$Title*")
  if ($ids -contains $p -and $titled -and [CO]::IsWindowVisible($h)) {
    $v = New-Object CO+RECT
    # DWMWA_EXTENDED_FRAME_BOUNDS: the visible bounds, without the invisible resize border;
    # less the 1 px window border Windows 11 draws round them, which PrintWindow paints black.
    [CO]::DwmGetWindowAttribute($h, 9, [ref]$v, 16) | Out-Null
    $v.L += 1; $v.T += 1; $v.R -= 1; $v.B -= 1
    if ($v.R - $v.L -gt 8) { [void]$wins.Add(@($h, $v)) }
  }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($wins.Count -eq 0) { throw "no windows" }
$L = [int]($wins | ForEach-Object { $_[1].L } | Measure-Object -Minimum).Minimum
$T = [int]($wins | ForEach-Object { $_[1].T } | Measure-Object -Minimum).Minimum
$R = [int]($wins | ForEach-Object { $_[1].R } | Measure-Object -Maximum).Maximum
$B = [int]($wins | ForEach-Object { $_[1].B } | Measure-Object -Maximum).Maximum
$bmp = New-Object System.Drawing.Bitmap ($R - $L), ($B - $T)
$g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::Black)
# Back to front (EnumWindows is front to back).
for ($i = $wins.Count - 1; $i -ge 0; $i--) {
  $h = $wins[$i][0]; $v = $wins[$i][1]
  $w = New-Object CO+RECT; [CO]::GetWindowRect($h, [ref]$w) | Out-Null
  $one = New-Object System.Drawing.Bitmap ($w.R - $w.L), ($w.B - $w.T)
  $og = [System.Drawing.Graphics]::FromImage($one); $hdc = $og.GetHdc()
  [CO]::PrintWindow($h, $hdc, 2) | Out-Null   # PW_RENDERFULLCONTENT
  $og.ReleaseHdc($hdc); $og.Dispose()
  # Keep only the visible bounds (drop the invisible resize border and the 1 px border).
  $src = New-Object System.Drawing.Rectangle ($v.L - $w.L), ($v.T - $w.T), ($v.R - $v.L), ($v.B - $v.T)
  $g.DrawImage($one, ($v.L - $L), ($v.T - $T), $src, [System.Drawing.GraphicsUnit]::Pixel)
  $one.Dispose()
}
$g.Dispose(); $bmp.Save($Out); $bmp.Dispose()
"$($wins.Count) windows, origin ($L,$T) -> $Out"

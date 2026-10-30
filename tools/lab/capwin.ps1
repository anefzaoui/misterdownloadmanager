# Screenshots one window (never the whole screen): by -Title prefix (any visible top-level
# window), else the main window of -ProcName.
param([string]$ProcName = "IDMan", [string]$Title = "", [Parameter(Mandatory)][string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$script:h = [IntPtr]::Zero
if ($Title) {
  [W]::EnumWindows({ param($w, $l)
    $sb = New-Object System.Text.StringBuilder 256; [W]::GetWindowText($w, $sb, 256) | Out-Null
    if ([W]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:h = $w; return $false }
    return $true }, [IntPtr]::Zero) | Out-Null
} else {
  $p = Get-Process $ProcName -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($p) { $script:h = $p.MainWindowHandle }
}
if ($script:h -eq [IntPtr]::Zero) { throw "window not found" }
$h = $script:h
[W]::ShowWindow($h, 5) | Out-Null; [W]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500
$r = New-Object W+RECT; [W]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
$g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
[W]::PrintWindow($h, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc); $g.Dispose(); $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
$sb = New-Object System.Text.StringBuilder 256; [W]::GetWindowText($h, $sb, 256) | Out-Null
"$($sb.ToString()) -> $Out"

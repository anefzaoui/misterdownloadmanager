# Lists the visible top-level windows of a process (title and rect).
param([string]$ProcName = "IDMan")
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class WL {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$ids = @(Get-Process $ProcName -ErrorAction Stop | ForEach-Object { $_.Id })
$out = New-Object System.Collections.ArrayList
[WL]::EnumWindows({ param($h, $l)
  $p = 0; [WL]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
  if ($ids -contains $p -and [WL]::IsWindowVisible($h)) {
    $sb = New-Object System.Text.StringBuilder 256; [WL]::GetWindowText($h, $sb, 256) | Out-Null
    $r = New-Object WL+RECT; [WL]::GetWindowRect($h, [ref]$r) | Out-Null
    [void]$out.Add(("[{0}] {1}x{2} at ({3},{4})" -f $sb.ToString(), ($r.R - $r.L), ($r.B - $r.T), $r.L, $r.T))
  }
  return $true }, [IntPtr]::Zero) | Out-Null
$out

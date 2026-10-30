# Prints GWL_STYLE / GWL_EXSTYLE flags of visible windows whose title starts with -Title.
param([Parameter(Mandatory)][string]$Title)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class WS {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$styles = @{ 0x00800000="BORDER"; 0x00400000="DLGFRAME"; 0x00040000="THICKFRAME"; 0x00020000="MINIMIZEBOX"; 0x00010000="MAXIMIZEBOX"; 0x00080000="SYSMENU"; 0x80000000="POPUP"; 0x04000000="CLIPSIBLINGS"; 0x02000000="CLIPCHILDREN" }
$ex = @{ 0x1="DLGMODALFRAME"; 0x8="TOPMOST"; 0x100="WINDOWEDGE"; 0x200="CLIENTEDGE"; 0x80="TOOLWINDOW"; 0x40000="APPWINDOW"; 0x10000="CONTROLPARENT" }
$out = New-Object System.Collections.ArrayList
[WS]::EnumWindows({ param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256; [WS]::GetWindowText($h, $sb, 256) | Out-Null
  if ([WS]::IsWindowVisible($h) -and $sb.ToString() -like "$Title*") {
    $s = [WS]::GetWindowLong($h, -16); $e = [WS]::GetWindowLong($h, -20)
    $sf = ($styles.Keys | Where-Object { ($s -band $_) -eq $_ } | ForEach-Object { $styles[$_] }) -join "|"
    $ef = ($ex.Keys | Where-Object { ($e -band $_) -eq $_ } | ForEach-Object { $ex[$_] }) -join "|"
    $w = New-Object WS+RECT; [WS]::GetWindowRect($h, [ref]$w) | Out-Null
    $c = New-Object WS+RECT; [WS]::GetClientRect($h, [ref]$c) | Out-Null
    [void]$out.Add(("[{0}] style=0x{1:X8} {2} ex=0x{3:X8} {4} outer={5}x{6} client={7}x{8}" -f $sb.ToString(), $s, $sf, $e, $ef, ($w.R-$w.L), ($w.B-$w.T), $c.R, $c.B))
  }
  return $true }, [IntPtr]::Zero) | Out-Null
$out

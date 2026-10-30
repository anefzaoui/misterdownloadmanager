# Moves/resizes a top-level window (found by exact title prefix and a menu-bar-or-not filter)
# so two apps can be screenshotted at identical sizes.
param([Parameter(Mandatory)][string]$Title, [int]$Left = 40, [int]$Top = 40, [int]$Width = 929, [int]$Height = 465)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class S {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
}
"@
$script:h = [IntPtr]::Zero
[S]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [S]::GetWindowText($w, $sb, 256) | Out-Null
  if ([S]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:h = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:h -eq [IntPtr]::Zero) { throw "no visible window '$Title*'" }
[S]::ShowWindow($script:h, 9) | Out-Null   # restore if maximised
[S]::SetWindowPos($script:h, [IntPtr]::Zero, $Left, $Top, $Width, $Height, 0x4) | Out-Null   # SWP_NOZORDER
"sized '$Title' to ${Width}x${Height}"

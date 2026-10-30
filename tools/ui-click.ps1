# Clicks at a point relative to a window's top-left corner (window found by title prefix),
# optionally types text and presses Enter. For driving the app's UI in manual checks.
param(
  [Parameter(Mandatory)][string]$Title,
  [int]$X = -1, [int]$Y = -1,
  [string]$Type = "",
  [switch]$Enter,
  [switch]$Right,
  [switch]$Double,
  # Drag from (X, Y) to (ToX, ToY), window-relative.
  [int]$ToX = -1, [int]$ToY = -1
)
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class C {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$script:h = [IntPtr]::Zero
[C]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [C]::GetWindowText($w, $sb, 256) | Out-Null
  if ([C]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:h = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:h -eq [IntPtr]::Zero) { throw "no visible window titled '$Title*'" }
[C]::SetForegroundWindow($script:h) | Out-Null
Start-Sleep -Milliseconds 300
if ($X -ge 0) {
  $r = New-Object C+RECT; [C]::GetWindowRect($script:h, [ref]$r) | Out-Null
  [C]::SetCursorPos($r.L + $X, $r.T + $Y) | Out-Null
  if ($ToX -ge 0) {
    [C]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero)
    for ($i = 1; $i -le 12; $i++) {
      $px = $r.L + $X + ($ToX - $X) * $i / 12; $py = $r.T + $Y + ($ToY - $Y) * $i / 12
      [C]::SetCursorPos([int]$px, [int]$py) | Out-Null; Start-Sleep -Milliseconds 30
    }
    [C]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero)
  }
  elseif ($Right) { [C]::mouse_event(0x8, 0, 0, 0, [IntPtr]::Zero); [C]::mouse_event(0x10, 0, 0, 0, [IntPtr]::Zero) }
  else {
    [C]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero); [C]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero)
    if ($Double) { Start-Sleep -Milliseconds 80; [C]::mouse_event(0x2, 0, 0, 0, [IntPtr]::Zero); [C]::mouse_event(0x4, 0, 0, 0, [IntPtr]::Zero) }
  }
  Start-Sleep -Milliseconds 300
}
if ($Type) { [System.Windows.Forms.SendKeys]::SendWait(($Type -replace '([+^%~(){}\[\]])', '{$1}')) }
if ($Enter) { [System.Windows.Forms.SendKeys]::SendWait("{ENTER}") }

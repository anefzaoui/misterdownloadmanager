# Inspects an open IDM dialog without the mouse or keyboard focus (black-box observation, D-001):
# lists its child controls (class, id, text, rect relative to the window), reads every combo
# box's items, and can post arrow keys to a child list (-KeysTo <class> -Keys Down,Down).
# -Page N switches a property sheet (like IDM's Options) to page N first (PSM_SETCURSEL).
param(
  [Parameter(Mandatory)][string]$Title,
  [string]$KeysTo = "",
  [string[]]$Keys = @(),
  [int]$Page = -1
)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class DL {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, StringBuilder l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$script:dlg = [IntPtr]::Zero
[DL]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [DL]::GetWindowText($w, $sb, 256) | Out-Null
  if ([DL]::IsWindowVisible($w) -and $sb.ToString() -eq $Title) { $script:dlg = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:dlg -eq [IntPtr]::Zero) { throw "no visible window titled '$Title'" }
if ($Page -ge 0) { [DL]::SendMessage($script:dlg, 0x465, [IntPtr]$Page, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 300 }
$wr = New-Object DL+RECT; [DL]::GetWindowRect($script:dlg, [ref]$wr) | Out-Null
$children = New-Object System.Collections.ArrayList
[DL]::EnumChildWindows($script:dlg, { param($c, $l) [void]$children.Add($c); return $true }, [IntPtr]::Zero) | Out-Null
$vk = @{ Down = 0x28; Up = 0x26; Home = 0x24; End = 0x23; Left = 0x25; Right = 0x27 }
foreach ($c in $children) {
  $cls = New-Object System.Text.StringBuilder 64; [DL]::GetClassName($c, $cls, 64) | Out-Null
  $txt = New-Object System.Text.StringBuilder 256; [DL]::GetWindowText($c, $txt, 256) | Out-Null
  $r = New-Object DL+RECT; [DL]::GetWindowRect($c, [ref]$r) | Out-Null
  $vis = if ([DL]::IsWindowVisible($c)) { "" } else { " (hidden)" }
  $en = if ([DL]::IsWindowEnabled($c)) { "" } else { " (disabled)" }
  "{0,-18} id {1,5}  {2},{3} {4}x{5}  '{6}'{7}{8}" -f $cls, [DL]::GetDlgCtrlID($c), ($r.L - $wr.L), ($r.T - $wr.T), ($r.R - $r.L), ($r.B - $r.T), $txt, $vis, $en
  if ($cls.ToString() -eq "ComboBox") {
    $n = [int][DL]::SendMessage($c, 0x146, [IntPtr]::Zero, [IntPtr]::Zero)   # CB_GETCOUNT
    for ($i = 0; $i -lt $n; $i++) {
      $item = New-Object System.Text.StringBuilder 256
      [DL]::SendMessage($c, 0x148, [IntPtr]$i, $item) | Out-Null           # CB_GETLBTEXT
      "    item $i : '$item'"
    }
  }
  if ($cls.ToString() -eq "ListBox") {
    $n = [int][DL]::SendMessage($c, 0x18B, [IntPtr]::Zero, [IntPtr]::Zero)   # LB_GETCOUNT
    for ($i = 0; $i -lt $n; $i++) {
      $item = New-Object System.Text.StringBuilder 256
      [DL]::SendMessage($c, 0x189, [IntPtr]$i, $item) | Out-Null           # LB_GETTEXT
      $sel = [int][DL]::SendMessage($c, 0x187, [IntPtr]$i, [IntPtr]::Zero)  # LB_GETSEL
      "    item $i : '$item' sel=$sel"
    }
  }
  if ($KeysTo -and $cls.ToString() -eq $KeysTo -and [DL]::IsWindowVisible($c)) {
    foreach ($k in $Keys) {
      [DL]::PostMessage($c, 0x100, [IntPtr]$vk[$k], [IntPtr]1) | Out-Null       # WM_KEYDOWN
      [DL]::PostMessage($c, 0x101, [IntPtr]$vk[$k], [IntPtr]0xC0000001) | Out-Null # WM_KEYUP
    }
    "    posted keys: $($Keys -join ',')"
  }
}

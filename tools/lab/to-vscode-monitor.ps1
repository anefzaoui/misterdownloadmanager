# Moves a window (title prefix) onto the VS Code monitor at offset (DX, DY), keeping its size
# unless -Width/-Height are given. Used right after launching anything for testing.
# -ProcessId: only that process's windows (a test instance next to the user's own, same titles).
param([Parameter(Mandatory)][string]$Title, [int]$DX = 60, [int]$DY = 60, [int]$Width = 0, [int]$Height = 0,
  [int]$ProcessId = 0)
$area = (& "$PSScriptRoot\vscode-monitor.ps1") -split ' ' | ForEach-Object { [int]$_ }
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class MV {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$script:found = @()
[MV]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [MV]::GetWindowText($w, $sb, 256) | Out-Null
  $owner = 0; [MV]::GetWindowThreadProcessId($w, [ref]$owner) | Out-Null
  $mine = ($ProcessId -eq 0) -or ($owner -eq $ProcessId)
  if ($mine -and [MV]::IsWindowVisible($w) -and $sb.ToString() -like "$Title*") { $script:found += $w }
  return $true }, [IntPtr]::Zero) | Out-Null
foreach ($h in $script:found) {
  $r = New-Object MV+RECT; [MV]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = if ($Width) { $Width } else { $r.R - $r.L }; $ht = if ($Height) { $Height } else { $r.B - $r.T }
  $flags = if ($Width -or $Height) { 0x4 } else { 0x5 }  # NOZORDER (+ NOSIZE)
  [MV]::SetWindowPos($h, [IntPtr]::Zero, $area[0] + $DX, $area[1] + $DY, $w, $ht, $flags) | Out-Null
  "moved '$Title' to ($($area[0] + $DX), $($area[1] + $DY))"
}
if (-not $script:found) { "no window '$Title*'" }

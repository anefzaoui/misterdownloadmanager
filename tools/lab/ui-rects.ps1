# The controls of one of our windows, as UI Automation sees them (the app's AccessKit tree):
# each one's name, type and rectangle in the window's picture (cap-own.ps1's, which starts 1 px
# inside the visible bounds), as JSON. The help pictures' shot list (tools/shots/) rings and
# numbers controls by name with it, so a moved button needs no new coordinates. Reads only:
# nothing is clicked, focused or typed, and only our process's windows are looked at.
#
# Without -Title, every visible window of the process (a menu over the main window), placed as
# cap-own.ps1 composes them.
#
#   tools\lab\ui-rects.ps1 -ProcessId 1234 -Title "Add URL" -Out target\shots\add-url.ui.json
param([Parameter(Mandatory)][int]$ProcessId, [string]$Title = "",
  [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class UR {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$wins = New-Object System.Collections.ArrayList
[UR]::EnumWindows({ param($h, $l)
  $p = 0; [UR]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
  $sb = New-Object System.Text.StringBuilder 512; [UR]::GetWindowText($h, $sb, 512) | Out-Null
  $titled = (-not $Title) -or ($sb.ToString() -like "$Title*")
  if ($p -eq $ProcessId -and [UR]::IsWindowVisible($h) -and $titled) {
    $v = New-Object UR+RECT
    [UR]::DwmGetWindowAttribute($h, 9, [ref]$v, 16) | Out-Null
    if ($v.R - $v.L -gt 10) { [void]$wins.Add(@($h, $v)) }
  }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($wins.Count -eq 0) { throw "no window '$Title'" }
# With -Title, the first (frontmost) match only, as cap-own.ps1 shoots one window by its title.
if ($Title) { $wins = @(,$wins[0]) }
# The picture's origin: the visible bounds less Windows 11's 1 px border (as cap-own.ps1 cuts).
$ox = [int]($wins | ForEach-Object { $_[1].L } | Measure-Object -Minimum).Minimum + 1
$oy = [int]($wins | ForEach-Object { $_[1].T } | Measure-Object -Minimum).Minimum + 1
$list = foreach ($w in $wins) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($w[0])
  # AccessKit builds its tree on the first request: ask until it has more than the window itself.
  $all = $null
  for ($i = 0; $i -lt 10; $i++) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition)
    if ($all.Count -gt 0) { break }
    Start-Sleep -Milliseconds 300
  }
  foreach ($e in $all) {
    try {
      $c = $e.Current
      $r = $c.BoundingRectangle
      if ($r.IsEmpty -or $r.Width -lt 1) { continue }
      [pscustomobject]@{
        name = $c.Name
        type = $c.ControlType.ProgrammaticName -replace '^ControlType\.', ''
        rect = @([int][math]::Round($r.X - $ox), [int][math]::Round($r.Y - $oy), [int][math]::Round($r.Width), [int][math]::Round($r.Height))
      }
    } catch { }
  }
}
$json = ConvertTo-Json -InputObject @($list) -Depth 4
[IO.File]::WriteAllText($Out, $json, (New-Object System.Text.UTF8Encoding $false))
"$(@($list).Count) controls -> $Out"
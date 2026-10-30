# Runs one of IDM's own menu commands by its path (e.g. "View/Dark Mode support"), exactly as
# choosing it would (WM_COMMAND with the item's ID) — no mouse, no keyboard. Prints the item's
# checked state before and after. -Query only prints the state.
param([Parameter(Mandatory)][string]$Path, [switch]$Query, [string]$Title = "Internet Download Manager*")
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class MC {
  [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int i);
  [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr m, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint id, StringBuilder s, int n, uint f);
  [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr m, uint id, uint f);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
}
"@
$script:hwnd = [IntPtr]::Zero
[MC]::EnumWindows({ param($w, $l)
  $sb = New-Object System.Text.StringBuilder 256; [MC]::GetWindowText($w, $sb, 256) | Out-Null
  if ([MC]::IsWindowVisible($w) -and $sb.ToString() -like $Title -and [MC]::GetMenu($w) -ne [IntPtr]::Zero) { $script:hwnd = $w; return $false }
  return $true }, [IntPtr]::Zero) | Out-Null
if ($script:hwnd -eq [IntPtr]::Zero) { throw "IDM main window not found" }
function Find($menu, [string[]]$parts) {
  $n = [MC]::GetMenuItemCount($menu)
  for ($i = 0; $i -lt $n; $i++) {
    $sb = New-Object System.Text.StringBuilder 256
    [MC]::GetMenuString($menu, $i, $sb, 256, 0x400) | Out-Null
    $text = $sb.ToString().Replace("&", "")
    if ($text -like "$($parts[0])*") {
      if ($parts.Count -eq 1) { return @($menu, $i) }
      return Find ([MC]::GetSubMenu($menu, $i)) $parts[1..($parts.Count - 1)]
    }
  }
  throw "menu item '$($parts[0])' not found"
}
$m, $i = Find ([MC]::GetMenu($script:hwnd)) ($Path -split "/")
$state = { if ([MC]::GetMenuState($m, $i, 0x400) -band 0x8) { "checked" } else { "unchecked" } }
"before: $(& $state)"
if ($Query) { return }
$id = [MC]::GetMenuItemID($m, $i)
[MC]::PostMessage($script:hwnd, 0x111, [IntPtr]$id, [IntPtr]::Zero) | Out-Null  # WM_COMMAND
Start-Sleep -Milliseconds 1500
"after: $(& $state)"

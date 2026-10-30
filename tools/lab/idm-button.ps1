# Presses a button in one of IDM's dialogs by its caption (BM_CLICK) — no mouse or keyboard.
# The dialog is found by exact title and the standard dialog class (#32770).
param([Parameter(Mandatory)][string]$Dialog, [Parameter(Mandatory)][string]$Button)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class BC {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr after, string c, string t);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
}
"@
$dlg = [BC]::FindWindow("#32770", $Dialog)
if ($dlg -eq [IntPtr]::Zero) { throw "dialog '$Dialog' not found" }
$btn = [IntPtr]::Zero
foreach ($cap in @($Button, "&$Button")) {
  $btn = [BC]::FindWindowEx($dlg, [IntPtr]::Zero, "Button", $cap)
  if ($btn -ne [IntPtr]::Zero) { break }
}
if ($btn -eq [IntPtr]::Zero) { throw "button '$Button' not found in '$Dialog'" }
# The notification a click sends (WM_COMMAND, BN_CLICKED = 0 in the high word); BM_CLICK can
# fail when the dialog is not the active window.
$id = [BC]::GetDlgCtrlID($btn)
[BC]::PostMessage($dlg, 0x111, [IntPtr]$id, $btn) | Out-Null
"pressed '$Button' (id $id) in '$Dialog'"

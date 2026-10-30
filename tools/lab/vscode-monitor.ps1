# Prints the working area of the monitor that shows VS Code, as "left top width height".
# Test windows are opened there so they never cover the user's main screen.
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class VM {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
$code = Get-Process Code -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$screen = if ($code) {
  $r = New-Object VM+RECT; [VM]::GetWindowRect($code.MainWindowHandle, [ref]$r) | Out-Null
  [System.Windows.Forms.Screen]::FromRectangle([System.Drawing.Rectangle]::FromLTRB($r.L, $r.T, $r.R, $r.B))
} else { [System.Windows.Forms.Screen]::PrimaryScreen }
$a = $screen.WorkingArea
"{0} {1} {2} {3}" -f $a.Left, $a.Top, $a.Width, $a.Height

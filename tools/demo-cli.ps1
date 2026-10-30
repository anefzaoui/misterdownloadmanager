# Visible demo: one window runs labserver (request log scrolling by), another downloads a
# file from it with misterdownloadmanager-cli. Press Ctrl+C in the download window to pause; it prints the
# `resume` command to continue.
param([string]$Size = "1G", [string]$Rate = "4M")
$root = Split-Path -Parent $PSScriptRoot
$dl = Join-Path $root ".study\demo"
New-Item -ItemType Directory -Force $dl | Out-Null
Get-Process labserver -ErrorAction SilentlyContinue | Stop-Process
Start-Process powershell -ArgumentList '-NoExit', '-Command',
  "`$host.UI.RawUI.WindowTitle = 'labserver (test server, rate $Rate per connection)'; & '$root\target\debug\labserver.exe' --bind 127.0.0.1:8092 --faults 'rate=$Rate'"
Start-Sleep -Seconds 1
Start-Process powershell -ArgumentList '-NoExit', '-Command',
  "`$host.UI.RawUI.WindowTitle = 'misterdownloadmanager-cli download'; Set-Location '$dl'; & '$root\target\debug\misterdownloadmanager-cli.exe' get http://127.0.0.1:8092/gen/$Size/demo-$Size.bin -o '$dl'"

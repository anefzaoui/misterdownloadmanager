# Runs one behaviour-lab scenario: labserver with faults, IDM downloads via its documented
# command line, wait for completion (or timeout), then summarise the request log.
param(
  [Parameter(Mandatory)][string]$Name,
  [string]$Faults = "",
  [string]$Size = "100M",
  [int]$TimeoutSec = 120,
  [int]$Port = 8090
)
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$lab = Join-Path $root ".study\lab"
New-Item -ItemType Directory -Force "$lab\dl" | Out-Null
$log = "$lab\$Name.jsonl"
Remove-Item $log -ErrorAction SilentlyContinue
$file = "$Name.bin"
Remove-Item "$lab\dl\$file" -ErrorAction SilentlyContinue

Get-Process labserver -ErrorAction SilentlyContinue | Stop-Process
$srv = Start-Process -PassThru -WindowStyle Hidden "$root\target\debug\labserver.exe" `
  -ArgumentList "--bind 127.0.0.1:$Port --faults `"$Faults`" --log-file `"$log`""
Start-Sleep -Milliseconds 800

$started = Get-Date
& "C:\Program Files (x86)\Internet Download Manager\IDMan.exe" /d "http://127.0.0.1:$Port/gen/$Size/$file" /p "$lab\dl" /f $file /n
while (((Get-Date) - $started).TotalSeconds -lt $TimeoutSec -and -not (Test-Path "$lab\dl\$file")) {
  Start-Sleep -Milliseconds 500
}
$done = Test-Path "$lab\dl\$file"
Start-Sleep -Seconds 2   # let the last log lines land
Stop-Process $srv.Id
"scenario $Name  faults='$Faults'  size=$Size  completed=$done  wall=$([int]((Get-Date) - $started).TotalSeconds)s"
& "$PSScriptRoot\analyze.ps1" -Log $log

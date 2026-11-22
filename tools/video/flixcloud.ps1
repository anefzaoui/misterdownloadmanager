param([string]$Page)

$ErrorActionPreference = 'Stop'
if (-not $Page) {
    $Page = Read-Host 'Paste the Re:ANIME or FlixCloud video page URL'
}
$node = (Get-Command node -ErrorAction Stop).Source
$mdm = Join-Path $env:ProgramFiles 'Mister Download Manager\misterdownloadmanager.exe'
if (-not (Test-Path -LiteralPath $mdm)) {
    throw 'Mister Download Manager was not found in Program Files.'
}
if (-not (Get-Process misterdownloadmanager -ErrorAction SilentlyContinue)) {
    throw 'Start Mister Download Manager first, then run this launcher again.'
}
Write-Host 'Preparing the video for MDM. Keep this window open while downloading.'
& $node (Join-Path $PSScriptRoot 'flixcloud.mjs') $Page $mdm
if ($LASTEXITCODE) {
    throw "FlixCloud support exited with code $LASTEXITCODE."
}

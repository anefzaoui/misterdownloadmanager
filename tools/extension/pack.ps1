# Builds the browser extension for each browser family from extension/ (one background.js):
#   target/extension/chromium/  - manifest.json (Chrome, Edge, Brave, Opera, Vivaldi; load unpacked)
#   target/extension/firefox/   - manifest.firefox.json as manifest.json
#   target/extension/mdm-chromium.zip - the chromium folder, zipped (a release download: unzip it
#                                       and load the folder unpacked)
#   target/extension/mdm-firefox.xpi  - the firefox folder, zipped (Firefox needs it signed by AMO
#                                       to install for good; about:debugging loads it unsigned)
# Both manifests get the app's version (Cargo.toml), so an extension and its app always match.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$src = Join-Path $root "extension"
# Joined part by part: .NET (the zips below) takes a backslash literally on Linux and macOS.
$out = Join-Path (Join-Path $root "target") "extension"
$cargo = Get-Content (Join-Path $root "Cargo.toml") -Raw
$version = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version) { throw "no version in Cargo.toml" }
foreach ($kind in "chromium", "firefox") {
  $dir = Join-Path $out $kind
  if (Test-Path $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
  New-Item -ItemType Directory -Force $dir | Out-Null
  Copy-Item (Join-Path $src "background.js") $dir
  # The download button over videos (a content script in every page).
  Copy-Item (Join-Path $src "video.js") $dir
  Copy-Item (Join-Path $src "icons") (Join-Path $dir "icons") -Recurse
  # Its texts in the browser's language (tools/extension/locales.py).
  Copy-Item (Join-Path $src "_locales") (Join-Path $dir "_locales") -Recurse
  $manifest = if ($kind -eq "firefox") { "manifest.firefox.json" } else { "manifest.json" }
  $json = Get-Content (Join-Path $src $manifest) -Raw
  # The first "version" only (the manifest's own).
  $json = ([regex]'("version"\s*:\s*)"[^"]*"').Replace($json, "`${1}`"$version`"", 1)
  [IO.File]::WriteAllText((Join-Path $dir "manifest.json"), $json, (New-Object Text.UTF8Encoding $false))
}
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
# The zips have manifest.json at their root. Entries are named with "/" as zip requires
# (Windows PowerShell's CreateFromDirectory writes "\", which Firefox rejects).
foreach ($pair in @(@("chromium", "mdm-chromium.zip"), @("firefox", "mdm-firefox.xpi"))) {
  $dir = (Resolve-Path (Join-Path $out $pair[0])).Path
  $zip = Join-Path $out $pair[1]
  if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
  $archive = [System.IO.Compression.ZipFile]::Open($zip, [System.IO.Compression.ZipArchiveMode]::Create)
  try {
    foreach ($f in Get-ChildItem -LiteralPath $dir -Recurse -File | Sort-Object FullName) {
      $name = $f.FullName.Substring($dir.Length).TrimStart('\', '/').Replace('\', '/')
      [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $f.FullName, $name)
    }
  } finally { $archive.Dispose() }
}
"built $out (chromium, firefox, mdm-chromium.zip, mdm-firefox.xpi; version $version)"

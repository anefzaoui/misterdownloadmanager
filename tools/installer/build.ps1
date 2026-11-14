# Builds the Windows release into target/dist/:
#   MisterDownloadManager-<version>-setup.exe     the installer (tools/installer/mdm.iss)
#   MisterDownloadManager-<version>-portable.zip  the same programs with a `data` folder (portable
#                                                 mode: settings beside the exe, no registry)
#   MisterDownloadManager-<version>-x64.zip       the same programs, data in %APPDATA% as installed
#   SHA256SUMS.txt
# Needs Inno Setup 6 (ISCC.exe on PATH, in $env:ISCC, or in its default folders).
# Code signing: set $env:MDM_SIGN_COMMAND to a signtool command line with $f for the file, e.g.
#   signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /a $f
# Inno Setup then signs the programs (in place, so the zip carries them signed too), the
# installer and its uninstaller.
# Or signed elsewhere (SignPath, in release.yml), in three runs:
#   -ToSign <dir>            builds (unless -NoBuild), puts what is to be signed in <dir> and stops:
#                            the programs and Inno Setup's uninstaller (named uninst-*.e32.exe)
#   -Signed <dir>            no build: the installer and the zips from those files, once signed
#   -SignedInstaller <file>  the installer, once signed, replaces the one in target/dist
param([switch]$NoBuild, [string]$SignCommand = $env:MDM_SIGN_COMMAND, [string]$ToSign,
  [string]$Signed, [string]$SignedInstaller)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$dist = Join-Path $root "target\dist"
$bin = Join-Path $root "target\release"
$programs = "misterdownloadmanager.exe", "misterdownloadmanager-nmhost.exe",
  "misterdownloadmanager-cli.exe", "misterdownloadmanager-mcp.exe"

$cargo = Get-Content (Join-Path $root "Cargo.toml") -Raw
$version = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version) { throw "no version in Cargo.toml" }

function Write-Sums {
  $files = Get-ChildItem $dist -File | Where-Object { $_.Name -like "MisterDownloadManager-$version-*" }
  $files | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name } |
    Set-Content -Encoding ascii (Join-Path $dist "SHA256SUMS.txt")
  $files | ForEach-Object { "{0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB) }
}

# A file signed elsewhere has to come back with a signature. Whether Windows trusts its
# certificate is the signer's matter, not checked here.
function Assert-Signed([string]$path) {
  if (-not (Get-AuthenticodeSignature -LiteralPath $path).SignerCertificate) { throw "not signed: $path" }
}

if ($SignedInstaller) {
  $setup = Join-Path $dist "MisterDownloadManager-$version-setup.exe"
  if (-not (Test-Path -LiteralPath $setup)) { throw "no installer to replace: $setup" }
  Assert-Signed $SignedInstaller
  Copy-Item -LiteralPath $SignedInstaller $setup -Force
  Write-Sums
  return
}
if ($Signed) {
  if ($SignCommand) { throw "-Signed packages programs that are signed already: no MDM_SIGN_COMMAND" }
  $NoBuild = $true
  $bin = (Resolve-Path -LiteralPath $Signed).Path
  foreach ($exe in $programs) { Assert-Signed (Join-Path $bin $exe) }
}

if (-not $NoBuild) {
  # The build machine's paths out of the programs (#25): panic locations and debug info say
  # "cargo\..." for cargo's folder and "mdm\..." for this checkout (shorter, and no user name).
  # Encoded flags, as those paths may hold spaces. They replace .cargo/config.toml's, so its
  # C runtime built into the programs (#785: no VCRUNTIME140.dll from Windows) is repeated here.
  $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
  $env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$cargoHome=cargo" + [char]0x1f + "--remap-path-prefix=$root=mdm" + [char]0x1f + "-Ctarget-feature=+crt-static"
  try {
    & cargo build --release -p mdm-app -p mdm-nmhost -p mdm-cli -p mdm-mcp
    if ($LASTEXITCODE) { throw "cargo build failed" }
  } finally {
    Remove-Item Env:CARGO_ENCODED_RUSTFLAGS
  }
}
& (Join-Path $root "tools\extension\pack.ps1") | Out-Null

# The installer's languages (tools/installer/languages.py writes them; committed, so no key needed).
if (-not (Test-Path (Join-Path $PSScriptRoot "languages.iss"))) { throw "run python tools/installer/languages.py first" }

$iscc = @($env:ISCC, (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
  "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") |
  Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) not found" }
$isccArgs = @("/Qp", "/DAppVersion=$version")

if ($ToSign) {
  # With /DUninstallerDir, Inno Setup writes the uninstaller there unsigned and stops ("attach
  # your digital signature ... and compile again"). Named .exe for the signer; -Signed names it
  # back.
  if (Test-Path -LiteralPath $ToSign) { Remove-Item -LiteralPath $ToSign -Recurse -Force }
  New-Item -ItemType Directory -Force $ToSign | Out-Null
  $ToSign = (Resolve-Path -LiteralPath $ToSign).Path
  foreach ($exe in $programs) { Copy-Item (Join-Path $bin $exe) $ToSign }
  $ErrorActionPreference = "Continue" # ISCC's expected error on stderr, checked below
  $log = & $iscc @isccArgs "/DUninstallerDir=$ToSign" (Join-Path $PSScriptRoot "mdm.iss") 2>&1
  $ErrorActionPreference = "Stop"
  $global:LASTEXITCODE = 0 # else a CI step ends with ISCC's exit code
  $uninst = @(Get-ChildItem -LiteralPath $ToSign -Filter "uninst-*.e32")
  if ($uninst.Count -ne 1) { $log | Write-Output; throw "ISCC wrote no uninstaller to sign" }
  Rename-Item -LiteralPath $uninst[0].FullName "$($uninst[0].Name).exe"
  Get-ChildItem -LiteralPath $ToSign -File | ForEach-Object { "to sign: $($_.Name)" }
  return
}

New-Item -ItemType Directory -Force $dist | Out-Null
if ($Signed) {
  # The signed uninstaller, under the name Inno Setup looks for. The installer itself is signed
  # afterwards (-SignedInstaller).
  $uninst = @(Get-ChildItem -LiteralPath $bin -Filter "uninst-*.e32.exe")
  if ($uninst.Count -ne 1) { throw "no signed uninstaller (uninst-*.e32.exe) in $bin" }
  Assert-Signed $uninst[0].FullName
  $uninstDir = Join-Path $dist "stage\uninstaller"
  if (Test-Path -LiteralPath $uninstDir) { Remove-Item -LiteralPath $uninstDir -Recurse -Force }
  New-Item -ItemType Directory -Force $uninstDir | Out-Null
  Copy-Item -LiteralPath $uninst[0].FullName (Join-Path $uninstDir $uninst[0].BaseName)
  $isccArgs += @("/DBin=$bin", "/DUninstallerDir=$uninstDir")
} elseif ($SignCommand) {
  $isccArgs += @("/DSign", "/Smdmsign=$SignCommand")
}
& $iscc @isccArgs (Join-Path $PSScriptRoot "mdm.iss")
if ($LASTEXITCODE) { throw "ISCC failed" }

# The zips: the same programs; the portable one adds a `data` folder next to the exe, which
# switches the app to portable mode. The plain zip keeps its data in %APPDATA% as installed.
function New-Zip([string]$name, [bool]$portable) {
  $stage = Join-Path $dist "stage\$name"
  if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
  New-Item -ItemType Directory -Force $stage | Out-Null
  foreach ($exe in $programs) { Copy-Item (Join-Path $bin $exe) $stage }
  New-Item -ItemType Directory -Force (Join-Path $stage "extension") | Out-Null
  Copy-Item (Join-Path $root "target\extension\chromium") (Join-Path $stage "extension\chromium") -Recurse
  Copy-Item (Join-Path $root "target\extension\mdm-firefox.xpi") (Join-Path $stage "extension")
  Copy-Item (Join-Path $root "LICENSE") (Join-Path $stage "LICENSE.txt")
  Copy-Item (Join-Path $root "ATTRIBUTION.md") (Join-Path $stage "ATTRIBUTION.txt")
  if ($portable) {
    New-Item -ItemType Directory -Force (Join-Path $stage "data") | Out-Null
    Set-Content -Encoding utf8 (Join-Path $stage "data\README.txt") @"
This folder makes Mister Download Manager portable: the download list and settings are kept
here instead of in your profile, and nothing is written to the registry. Delete the folder to
use the profile instead. Browser integration needs the installer.
"@
  }
  $zip = Join-Path $dist "$name.zip"
  if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
  # Freshly copied programs can be held for a moment by an antivirus scan: retry.
  foreach ($try in 1..10) {
    try {
      # Entries under the folder's name, with "/" as zip requires (Windows PowerShell's
      # CreateFromDirectory writes "\", which other systems' tools take as part of the name).
      $base = Split-Path -Parent $stage
      $archive = [System.IO.Compression.ZipFile]::Open($zip, [System.IO.Compression.ZipArchiveMode]::Create)
      try {
        foreach ($f in Get-ChildItem -LiteralPath $stage -Recurse -File | Sort-Object FullName) {
          $entry = $f.FullName.Substring($base.Length).TrimStart('\', '/').Replace('\', '/')
          [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $f.FullName,
            $entry, [System.IO.Compression.CompressionLevel]::Optimal)
        }
      } finally { $archive.Dispose() }
      break
    } catch {
      if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
      if ($try -eq 10) { throw }
      Start-Sleep -Seconds 2
    }
  }
}
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
New-Zip "MisterDownloadManager-$version-portable" $true
New-Zip "MisterDownloadManager-$version-x64" $false
Remove-Item -LiteralPath (Join-Path $dist "stage") -Recurse -Force

Write-Sums

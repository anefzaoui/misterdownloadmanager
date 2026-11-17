# Cargo for Windows 7 SP1's build (docs/windows-7.md): Rust's win7 targets, x86_64-win7-windows-msvc
# (-Arch x64) or i686-win7-windows-msvc (-Arch x86), are tier 3: Rust ships no standard library for
# them, so it is built from source with the program (-Zbuild-std), which takes a nightly toolchain
# and its rust-src. The nightly is pinned (toolchain.txt) and moved on deliberately: a nightly has
# broken these targets before (rust-lang/rust#164340, fixed two days later). The rest of the
# arguments are cargo's, e.g.
#   tools/win7/cargo.ps1 -Arch x64 build --release --no-default-features -p mdm-app
#   tools/win7/cargo.ps1 -Arch x86 test --workspace --no-default-features --features mdm-crash/native-symbols
# The toolchain is installed when missing (rustup, with rust-src only).
# (A plain script, not an advanced one: cargo's -p would bind to PowerShell's own -PipelineVariable.)
param([ValidateSet("x64", "x86")][string]$Arch = "x64")
$Cargo = @($args)
$ErrorActionPreference = "Stop"
$toolchain = (Get-Content (Join-Path $PSScriptRoot "toolchain.txt") -TotalCount 1).Trim()
$target = @{ x64 = "x86_64-win7-windows-msvc"; x86 = "i686-win7-windows-msvc" }[$Arch]
if (-not $Cargo) { throw "no cargo command (build, test, clippy...)" }

# Installed (again) when missing; rustup adds rust-src to a toolchain installed without it.
$have = & rustup toolchain list | Where-Object { $_ -like "$toolchain-*" }
if (-not $have) {
  & rustup toolchain install $toolchain --profile minimal --component rust-src
} else {
  & rustup component add rust-src --toolchain $toolchain
}
if ($LASTEXITCODE) { throw "rustup couldn't install $toolchain with rust-src" }
# windows-sys 0.48 (still in the tree: mio 0.8, os_pipe, winapi-util) links Windows' functions
# through windows_x86_64_msvc's / windows_i686_msvc's import library, whose build script points
# the linker at it only for the -pc- targets' names: pointed at here. The flags replace
# .cargo/config.toml's, so its C runtime built in is repeated (as build.ps1 does) unless the
# caller set its own (build.ps1's, which repeat it).
& cargo "+$toolchain" fetch --quiet
if ($LASTEXITCODE) { throw "cargo fetch failed" }
$crate = @{ x64 = "windows_x86_64_msvc-0.48.5"; x86 = "windows_i686_msvc-0.48.5" }[$Arch]
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
$lib = Get-ChildItem (Join-Path $cargoHome "registry\src\*\$crate\lib") -Directory -ErrorAction SilentlyContinue |
  Select-Object -First 1
if (-not $lib) { throw "$crate's import library not found in cargo's registry (run cargo fetch)" }
$flags = if ($env:CARGO_ENCODED_RUSTFLAGS) { $env:CARGO_ENCODED_RUSTFLAGS } else { "-Ctarget-feature=+crt-static" }
$before = $env:CARGO_ENCODED_RUSTFLAGS
# windows_slim_errors: windows-rs's errors without their WinRT error info, which it reports
# through Windows 8's RoOriginateErrorW (api-ms-win-core-winrt-error-l1-1-0.dll, an import that
# keeps a program from starting on Windows 7); the error codes and messages stay.
$sep = [char]0x1f
$env:CARGO_ENCODED_RUSTFLAGS = $flags + $sep + "-Lnative=$($lib.FullName)" + $sep + "--cfg" + $sep + "windows_slim_errors"
try {
  # The command, then the target and build-std, then the rest of cargo's arguments.
  $all = @("+$toolchain", $Cargo[0], "--target", $target, "-Zbuild-std=std,panic_abort") + @($Cargo | Select-Object -Skip 1)
  & cargo @all
  $code = $LASTEXITCODE
} finally {
  $env:CARGO_ENCODED_RUSTFLAGS = $before
}
exit $code

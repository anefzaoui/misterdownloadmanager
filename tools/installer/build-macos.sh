#!/usr/bin/env bash
# Builds the macOS release into target/dist/ (experimental: built by CI, not yet tested by us):
#   MisterDownloadManager-<version>-macos-universal-experimental.dmg
# One app for Apple Silicon and Intel (lipo), ad-hoc signed: without an Apple Developer ID, macOS
# asks before the first start (right-click the app -> Open).
# Needs: macOS with Xcode's command line tools, rustup, pwsh (the extension).
#   tools/installer/build-macos.sh [--no-build]
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
dist="$root/target/dist"
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1)"
[ -n "$version" ] || { echo "no version in Cargo.toml" >&2; exit 1; }
name="MisterDownloadManager-$version-macos-universal-experimental"
programs=(misterdownloadmanager misterdownloadmanager-nmhost misterdownloadmanager-cli misterdownloadmanager-mcp)
targets=(aarch64-apple-darwin x86_64-apple-darwin)

if [ "${1:-}" != "--no-build" ]; then
  for t in "${targets[@]}"; do
    rustup target add "$t"
    cargo build --release --target "$t" -p mdm-app -p mdm-nmhost -p mdm-cli -p mdm-mcp
  done
fi
pwsh -NoProfile -File "$root/tools/extension/pack.ps1"

mkdir -p "$dist"
stage="$dist/stage-macos"
rm -rf "$stage"
app="$stage/dmg/Mister Download Manager.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for p in "${programs[@]}"; do
  lipo -create -output "$app/Contents/MacOS/$p" \
    "$root/target/aarch64-apple-darwin/release/$p" "$root/target/x86_64-apple-darwin/release/$p"
done
# Crash reports: the release's symbols (the -symbols zip) from each half's debug info, which stays
# in the build's object files until dsymutil gathers it (the app ships without it).
syms=()
for t in "${targets[@]}"; do
  for p in "${programs[@]}"; do
    dsymutil "$root/target/$t/release/$p" -o "$stage/dsym/$t/$p.dSYM"
    syms+=("$stage/dsym/$t/$p.dSYM")
  done
done
python3 "$root/tools/installer/symbols.py" macos-universal "$version" "$dist" "${syms[@]}"
cp -r "$root/target/extension/chromium" "$app/Contents/Resources/extension-chromium"
cp "$root/target/extension/mdm-firefox.xpi" "$root/LICENSE" "$root/ATTRIBUTION.md" "$app/Contents/Resources/"

# The icon: every size macOS asks for, from the 1254 px artwork.
set_dir="$stage/AppIcon.iconset"
mkdir -p "$set_dir"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$root/art/Mister Download Manager.png" --out "$set_dir/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$root/art/Mister Download Manager.png" --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/AppIcon.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Mister Download Manager</string>
  <key>CFBundleDisplayName</key><string>Mister Download Manager</string>
  <key>CFBundleIdentifier</key><string>io.github.anefzaoui.misterdownloadmanager</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>misterdownloadmanager</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
codesign --force --deep --sign - "$app"

# The disk image: the app next to a link to Applications, to drag it there.
ln -s /Applications "$stage/dmg/Applications"
rm -f "$dist/$name.dmg"
hdiutil create -volname "Mister Download Manager" -srcfolder "$stage/dmg" -ov -format UDZO \
  "$dist/$name.dmg"

rm -rf "$stage"
(cd "$dist" && shasum -a 256 "$name".dmg > SHA256SUMS-macos.txt)
ls -l "$dist"

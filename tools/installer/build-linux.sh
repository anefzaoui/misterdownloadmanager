#!/usr/bin/env bash
# Builds the Linux release into target/dist/ (experimental: built by CI, not yet tested by us):
#   MisterDownloadManager-<version>-linux-x86_64-experimental.AppImage  runs on most distros
#   MisterDownloadManager-<version>-linux-x86_64-experimental.tar.gz    the plain programs
#   MisterDownloadManager-<version>-linux-x86_64-experimental.deb       Debian, Ubuntu, Mint
#   MisterDownloadManager-<version>-linux-x86_64-experimental.rpm       Fedora, RHEL, openSUSE
# Needs: cargo, pwsh (the extension), python3 with Pillow (the icon), dpkg-deb, rpmbuild, curl (appimagetool is fetched).
# Build on the oldest Ubuntu you want to support: the programs need its glibc or newer.
#   tools/installer/build-linux.sh [--no-build]
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
dist="$root/target/dist"
bin="$root/target/release"
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$root/Cargo.toml" | head -1)"
[ -n "$version" ] || { echo "no version in Cargo.toml" >&2; exit 1; }
name="MisterDownloadManager-$version-linux-x86_64-experimental"
programs=(misterdownloadmanager misterdownloadmanager-nmhost misterdownloadmanager-cli misterdownloadmanager-mcp)

if [ "${1:-}" != "--no-build" ]; then
  cargo build --release -p mdm-app -p mdm-nmhost -p mdm-cli -p mdm-mcp
fi
pwsh -NoProfile -File "$root/tools/extension/pack.ps1"

mkdir -p "$dist"
stage="$dist/stage-linux"
rm -rf "$stage"
mkdir -p "$stage"

# One tree in the usual Linux layout; every package is made from it.
tree="$stage/tree"
mkdir -p "$tree/usr/bin" "$tree/usr/share/applications" \
  "$tree/usr/share/icons/hicolor/256x256/apps" "$tree/usr/share/misterdownloadmanager" \
  "$tree/usr/share/doc/misterdownloadmanager"
for p in "${programs[@]}"; do install -m 755 "$bin/$p" "$tree/usr/bin/$p"; done
cp -r "$root/target/extension/chromium" "$tree/usr/share/misterdownloadmanager/extension-chromium"
cp "$root/target/extension/mdm-firefox.xpi" "$tree/usr/share/misterdownloadmanager/"
cp "$root/LICENSE" "$root/ATTRIBUTION.md" "$tree/usr/share/doc/misterdownloadmanager/"
python3 - "$root/art/Mister Download Manager.png" \
  "$tree/usr/share/icons/hicolor/256x256/apps/misterdownloadmanager.png" <<'EOF'
import sys
from PIL import Image
Image.open(sys.argv[1]).convert("RGBA").resize((256, 256), Image.LANCZOS).save(sys.argv[2])
EOF
cat > "$tree/usr/share/applications/misterdownloadmanager.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Mister Download Manager
Comment=Download manager
Exec=misterdownloadmanager %U
Icon=misterdownloadmanager
Terminal=false
Categories=Network;FileTransfer;
MimeType=x-scheme-handler/magnet;application/x-bittorrent;
EOF

# tar.gz: the programs, the extension and the licence in one folder.
plain="$stage/$name"
mkdir -p "$plain"
for p in "${programs[@]}"; do cp "$tree/usr/bin/$p" "$plain/"; done
cp -r "$tree/usr/share/misterdownloadmanager/extension-chromium" "$plain/extension-chromium"
cp "$tree/usr/share/misterdownloadmanager/mdm-firefox.xpi" "$plain/"
cp "$root/LICENSE" "$root/ATTRIBUTION.md" "$plain/"
tar -C "$stage" -czf "$dist/$name.tar.gz" "$name"

# .deb
deb="$stage/deb"
cp -r "$tree" "$deb"
mkdir -p "$deb/DEBIAN"
cat > "$deb/DEBIAN/control" <<EOF
Package: misterdownloadmanager
Version: $version
Architecture: amd64
Maintainer: Mister Download Manager <https://github.com/anefzaoui/misterdownloadmanager>
Depends: libc6 (>= 2.35), libgtk-3-0, libgl1, libxkbcommon0
Section: net
Priority: optional
Homepage: https://github.com/anefzaoui/misterdownloadmanager
Description: Download manager (experimental Linux build)
 Speeds up downloads with several connections at once, resumes them, catches them from the
 browser, and downloads videos and torrents.
EOF
dpkg-deb --root-owner-group --build "$deb" "$dist/$name.deb"

# .rpm, from the same tree (nothing is compiled by rpmbuild).
rpm="$stage/rpm"
mkdir -p "$rpm"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
cat > "$rpm/SPECS/mdm.spec" <<EOF
Name: misterdownloadmanager
Version: $version
Release: 1
Summary: Download manager (experimental Linux build)
License: GPL-3.0-or-later
URL: https://github.com/anefzaoui/misterdownloadmanager
Requires: gtk3, mesa-libGL, libxkbcommon
AutoReqProv: no

%description
Speeds up downloads with several connections at once, resumes them, catches them from the
browser, and downloads videos and torrents.

%install
cp -a "$tree/." %{buildroot}/

%files
/usr/bin/misterdownloadmanager
/usr/bin/misterdownloadmanager-nmhost
/usr/bin/misterdownloadmanager-cli
/usr/bin/misterdownloadmanager-mcp
/usr/share/applications/misterdownloadmanager.desktop
/usr/share/icons/hicolor/256x256/apps/misterdownloadmanager.png
/usr/share/misterdownloadmanager
/usr/share/doc/misterdownloadmanager
EOF
rpmbuild --define "_topdir $rpm" --define "_build_id_links none" -bb "$rpm/SPECS/mdm.spec"
cp "$rpm"/RPMS/x86_64/*.rpm "$dist/$name.rpm"

# AppImage: the same tree as an AppDir.
appdir="$stage/AppDir"
cp -r "$tree" "$appdir"
ln -s usr/bin/misterdownloadmanager "$appdir/AppRun"
cp "$tree/usr/share/applications/misterdownloadmanager.desktop" "$appdir/"
cp "$tree/usr/share/icons/hicolor/256x256/apps/misterdownloadmanager.png" "$appdir/"
tool="$stage/appimagetool"
curl -fsSL -o "$tool" \
  https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x "$tool"
# No FUSE on CI runners: the tool runs from its own extracted copy.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" "$appdir" "$dist/$name.AppImage"

rm -rf "$stage"
(cd "$dist" && sha256sum "$name".* > SHA256SUMS-linux.txt)
ls -l "$dist"

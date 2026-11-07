#!/usr/bin/env bash
# Moves the AUR package (packaging/aur/misterdownloadmanager-bin) to a published release: its
# version, the .deb's name and checksum (from the release's SHA256SUMS.txt), pkgrel 1, and
# .SRCINFO again (needs makepkg, so Arch).
#   packaging/aur/update.sh 0.3.0
# The package-managers workflow runs it in an Arch container.
set -euo pipefail
version="${1:?usage: update.sh <version, e.g. 0.3.0>}"
version="${version#v}"
dir="$(cd "$(dirname "$0")" && pwd)/misterdownloadmanager-bin"
repo="${MDM_REPO:-anefzaoui/misterdownloadmanager}"

sums="$(curl -fsSL "https://github.com/$repo/releases/download/v$version/SHA256SUMS.txt")"
# MisterDownloadManager-<version>-linux-x86_64[-experimental].deb
line="$(grep -E "  MisterDownloadManager-$version-linux-x86_64(-[a-z]+)?\.deb$" <<<"$sums" | head -1)"
[ -n "$line" ] || { echo "no Linux .deb in v$version's SHA256SUMS.txt" >&2; exit 1; }
sum="${line%%  *}"
file="${line#*  }"
suffix="${file#MisterDownloadManager-$version-linux-x86_64}"
suffix="${suffix%.deb}"

sed -i \
  -e "s/^pkgver=.*/pkgver=$version/" \
  -e "s/^pkgrel=.*/pkgrel=1/" \
  -e "s/^_suffix=.*/_suffix=$suffix/" \
  -e "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$sum')/" \
  "$dir/PKGBUILD"
echo "PKGBUILD: $version, $file, $sum"

if command -v makepkg >/dev/null; then
  (cd "$dir" && makepkg --printsrcinfo > .SRCINFO)
  echo ".SRCINFO written"
else
  echo "makepkg isn't here (not Arch): .SRCINFO not written" >&2
fi

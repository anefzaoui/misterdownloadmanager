#!/usr/bin/env bash
set -euo pipefail

# 1. Determine target version
VER="${1:-${VER:-}}"
if [ -z "$VER" ]; then
  echo "No version specified, detecting latest release..."
  if command -v gh >/dev/null 2>&1; then
    VER=$(gh release view --repo anefzaoui/misterdownloadmanager --json tagName --jq .tagName 2>/dev/null || true)
  fi
  if [ -z "$VER" ] || [ "$VER" = "null" ]; then
    AUTH_HEADER=()
    if [ -n "${GH_TOKEN:-}" ]; then
      AUTH_HEADER=(-H "Authorization: Bearer $GH_TOKEN")
    elif [ -n "${GITHUB_TOKEN:-}" ]; then
      AUTH_HEADER=(-H "Authorization: Bearer $GITHUB_TOKEN")
    fi
    VER=$(curl -fsSL "${AUTH_HEADER[@]}" "https://api.github.com/repos/anefzaoui/misterdownloadmanager/releases/latest" 2>/dev/null | jq -r .tag_name 2>/dev/null || true)
  fi
fi

VER="${VER#v}"
if [ -z "$VER" ] || [ "$VER" = "null" ]; then
  echo "Error: Could not determine valid MDM version" >&2
  exit 1
fi

echo "Target MDM version: $VER"

DEST_BASE="${3:-bin}"

download_arch() {
  local target_arch="$1"   # "x86_64" or "arm64"
  local folder_name="$2"   # "amd64" or "arm64"
  local dest_dir="${DEST_BASE}/${folder_name}"

  local tar_url="https://github.com/anefzaoui/misterdownloadmanager/releases/download/v${VER}/MisterDownloadManager-${VER}-linux-${target_arch}.tar.gz"
  echo "Downloading $tar_url ..."

  local tmp_dir
  tmp_dir=$(mktemp -d)

  curl -fsSL "$tar_url" -o "$tmp_dir/mdm.tar.gz"
  tar -xzf "$tmp_dir/mdm.tar.gz" -C "$tmp_dir"

  mkdir -p "$dest_dir"
  cp "$tmp_dir"/MisterDownloadManager-*/misterdownloadmanager "$dest_dir/"
  cp "$tmp_dir"/MisterDownloadManager-*/misterdownloadmanager-cli "$dest_dir/"
  chmod +x "$dest_dir/misterdownloadmanager" "$dest_dir/misterdownloadmanager-cli"

  rm -rf "$tmp_dir"
  echo "Successfully extracted misterdownloadmanager and misterdownloadmanager-cli (${folder_name}) to ${dest_dir}"
}

# 2. Check architecture
ARCH_RAW="${2:-all}"
case "$ARCH_RAW" in
  all)
    echo "Downloading binaries for both amd64 (x86_64) and arm64..."
    download_arch "x86_64" "amd64"
    download_arch "arm64" "arm64"
    ;;
  x86_64|amd64|x86-64|x64)
    download_arch "x86_64" "amd64"
    ;;
  aarch64|arm64|armv8*|arm64v8)
    download_arch "arm64" "arm64"
    ;;
  *)
    echo "Error: Unsupported architecture '$ARCH_RAW'." >&2
    echo "Mister Download Manager releases support x86_64 and arm64 (or 'all')." >&2
    exit 1
    ;;
esac

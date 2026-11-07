#!/usr/bin/env bash
# Update now, end to end, on Linux: the .tar.gz and AppImage paths (README.md). Off-screen test
# copies in folders of their own, on a virtual screen of this script's own (Xvfb :96, a session
# bus, Metacity), fake 9.9.9 releases signed with a throwaway key made for this run (mkrel.py),
# served by the lab server on 127.0.0.1. Needs a release-mode build with the `test-update`
# feature (vmtest or release profile: debug builds are "built from source" and never update).
#
#   tools/lab/update/upd-lin.sh <folder with misterdownloadmanager and labserver> [scenario ...]
#   e.g. upd-lin.sh ~/src/mdm/target/vmtest     (every scenario)
#
# Scenarios and what each must show (the run fails otherwise, exit status 1):
#   targz-good, appimage-good   put in place, the new copy started with /updated, the old ended
#   targz-bad-hash              refused: nothing replaced, nothing started
#   appimage-other-key          the checksum list signed by another key: refused
#   targz-close-download        the window closed while it downloads (1 MB/s): nothing replaced or started
#   appimage-close-install      the window closed during "Installing the update...": replaced, nothing started
#
# Work folder: $MDM_UPDATE_E2E_WORK, else /tmp/mdm-update-e2e (emptied first).
set -uo pipefail
bin=$(cd "${1:?usage: upd-lin.sh <build folder> [scenario ...]}" && pwd); shift
here=$(cd "$(dirname "$0")" && pwd)
all="targz-good appimage-good targz-bad-hash appimage-other-key targz-close-download appimage-close-install"
want=${*:-$all}
work=${MDM_UPDATE_E2E_WORK:-/tmp/mdm-update-e2e}
rm -rf "$work"; mkdir -p "$work"
for t in Xvfb dbus-daemon metacity wmctrl python3 gpg; do
  command -v $t >/dev/null || { echo "needs $t"; exit 2; }
done

# Our own screen, never the desktop's (nor WSLg's on WSL).
unset WAYLAND_DISPLAY WAYLAND_SOCKET PULSE_SERVER
export DISPLAY=:96 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1
export XDG_RUNTIME_DIR=$work/xdg-run
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
now() { cut -d' ' -f1 /proc/uptime; }
T0=$(now)
log() { echo "[$(awk -v a="$T0" -v b="$(now)" 'BEGIN {printf "%6.1f", b - a}')s] $*" | tee -a "$work/log.txt"; }
xvfb=""; dbus=""; wm=""
cleanup() {
  local rc=$?
  pkill -f "$work/run-" 2>/dev/null
  [ -n "$wm" ] && kill "$wm" 2>/dev/null
  [ -n "$dbus" ] && kill "$dbus" 2>/dev/null
  [ -n "$xvfb" ] && kill "$xvfb" 2>/dev/null
  exit $rc
}
trap cleanup EXIT
Xvfb :96 -screen 0 1280x800x24 -nolisten tcp -dpi 96 >"$work/xvfb.log" 2>&1 </dev/null & xvfb=$!
for _ in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done
dbus=$(dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --fork --print-pid)
metacity --replace --no-composite >"$work/metacity.log" 2>&1 </dev/null & wm=$!
sleep 1

port=8299; slowport=8296; base=http://127.0.0.1:$port/files
keys=$work/keys
release() { # release <name> <kind targz|file> <asset> [mkrel args]
  local name=$1 kind=$2 asset=$3; shift 3
  python3 "$here/mkrel.py" --keys "$keys" --out "$work/rel-$name" --base "$base" --asset "$asset" \
    --kind "$kind" --program "$bin/misterdownloadmanager" --program-name misterdownloadmanager "$@" |
    sed 's/^/  /' | tee -a "$work/log.txt"
}
TGZ=MisterDownloadManager-9.9.9-linux-x86_64.tar.gz
APP=MisterDownloadManager-9.9.9-linux-x86_64.AppImage
log "making the fake releases (throwaway keys in $keys)"
release targz-good targz $TGZ
release targz-bad-hash targz $TGZ --sums bad
release targz-slow targz $TGZ --pad 30 --asset-base "http://127.0.0.1:$slowport"
release appimage-good file $APP
release appimage-other-key file $APP --signer other

mine() { wmctrl -lp 2>/dev/null | awk -v p="$1" '$3 == p {print $1}'; }
run() { # run <name> <targz|appimage> <release> <hold> <close: -|download|install>; sets STARTED ENDED REPLACED
  local name=$1 kind=$2 rel=$3 hold=$4 close=$5
  local w=$work/run-$1 prog srv slow="" pid upd before new
  mkdir -p "$w/copy" "$w/data" "$w/Downloads"
  if [ "$kind" = appimage ]; then prog=$w/copy/MisterDownloadManager.AppImage; else prog=$w/copy/misterdownloadmanager; fi
  cp "$bin/misterdownloadmanager" "$prog"
  local start_hash; start_hash=$(sha256sum "$prog" | cut -c1-16)
  python3 - "$w/data/downloads.json" "$w/Downloads" <<'PY'
import json, sys
path, dl = sys.argv[1:3]
s = dict(language="en", language_asked=True, helpers_asked=True, extension_never_ask=True,
         update_check=False, torrent_incoming=False, torrent_upnp=False, torrent_dht=False,
         torrent_lsd=False, category_dirs=[["General", dl]])
json.dump({"next_id": 1, "items": [], "version": 2, "settings": s}, open(path, "w"))
PY
  "$bin/labserver" --bind "127.0.0.1:$port" --files "$work/rel-$rel" >"$w/labserver.log" 2>&1 & srv=$!
  [ "$close" = download ] && { python3 "$here/slowserve.py" "$work/rel-$rel" $slowport 1000000 2>"$w/slow.log" & slow=$!; }
  (
    export MDM_DATA_DIR=$w/data MDM_OFFSCREEN=1 MDM_UPDATE_FEED=$base/latest.json MDM_TEST_UPDATE_KEY=$keys/test-key.asc
    [ "$hold" != 0 ] && export MDM_TEST_UPDATE_HOLD=$hold
    if [ "$kind" = appimage ]; then export APPIMAGE=$prog; else unset APPIMAGE; fi
    cd "$w" && setsid -f "$prog" >"$w/stdout.log" 2>&1 </dev/null
  )
  pid=""
  for _ in $(seq 300); do pid=$(pgrep -f "^$prog$" | head -1); [ -n "$pid" ] && [ -n "$(mine "$pid")" ] && break; sleep 0.1; done
  STARTED=no; ENDED=no; REPLACED=no
  if [ -z "$pid" ]; then log "  MDM showed no window (see $w/stdout.log)"; kill $srv $slow 2>/dev/null; return; fi
  sleep 2
  before=$(mine "$pid")
  ( export MDM_DATA_DIR=$w/data; [ "$kind" = appimage ] && export APPIMAGE=$prog; "$prog" /show updates >/dev/null 2>&1 )
  upd=""
  for _ in $(seq 100); do upd=$(mine "$pid" | grep -vxF -f <(echo "$before") | head -1); [ -n "$upd" ] && break; sleep 0.1; done
  sleep 2
  ( export MDM_DATA_DIR=$w/data; [ "$kind" = appimage ] && export APPIMAGE=$prog; "$prog" /show update-now >/dev/null 2>&1 )
  log "  Update now pressed (window $upd)"
  [ "$close" = download ] && { sleep 1.5; wmctrl -i -c "$upd"; log "  the update window closed while it downloads"; }
  [ "$close" = install ] && { sleep 3; wmctrl -i -c "$upd"; log "  the update window closed during 'Installing the update...'"; }
  local deadline=$(( $(cut -d. -f1 /proc/uptime) + hold + 30 ))
  while [ "$(cut -d. -f1 /proc/uptime)" -lt "$deadline" ]; do
    [ $ENDED = no ] && ! kill -0 "$pid" 2>/dev/null && { ENDED=yes; log "  the old copy ended"; }
    if [ $STARTED = no ]; then
      new=$(pgrep -af "/updated" | grep -F "$w/copy/" | head -1)
      [ -n "$new" ] && { STARTED=yes; log "  a new copy started: $new"; }
    fi
    [ $ENDED = yes ] && [ $STARTED = yes ] && break
    sleep 0.25
  done
  # Replaced: the tar.gz's marker file, or the AppImage changed (the release's has one appended).
  if [ -f "$w/copy/NEW-COPY.txt" ] || [ "$(sha256sum "$prog" | cut -c1-16)" != "$start_hash" ]; then REPLACED=yes; fi
  log "  folder: $(ls "$w/copy" | tr '\n' ' ')"
  pkill -f "$w/copy/" 2>/dev/null
  kill $srv $slow 2>/dev/null
  sleep 1
}

failed=()
check() { # check <name> <started> <ended> <replaced>
  local line="started=$STARTED old-ended=$ENDED replaced=$REPLACED"
  if [ "$STARTED $ENDED $REPLACED" = "$2 $3 $4" ]; then log "PASS $1: $line"
  else log "FAIL $1: $line (expected started=$2 old-ended=$3 replaced=$4)"; failed+=("$1"); fi
}
for s in $want; do
  log "=== $s"
  case $s in
    targz-good)             run $s targz targz-good 4 -;                 check $s yes yes yes ;;
    appimage-good)          run $s appimage appimage-good 4 -;           check $s yes yes yes ;;
    targz-bad-hash)         run $s targz targz-bad-hash 0 -;             check $s no no no ;;
    appimage-other-key)     run $s appimage appimage-other-key 0 -;      check $s no no no ;;
    targz-close-download)   run $s targz targz-slow 0 download;          check $s no no no ;;
    appimage-close-install) run $s appimage appimage-good 8 install;     check $s no no yes ;;
    *) log "unknown scenario $s"; failed+=("$s") ;;
  esac
done
[ ${#failed[@]} -gt 0 ] && { log "failed: ${failed[*]}"; exit 1; }
log "all passed"

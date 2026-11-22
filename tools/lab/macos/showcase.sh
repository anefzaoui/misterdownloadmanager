#!/bin/bash
# MDM on macOS, shown on GitHub's macOS runner (.github/workflows/macos-showcase.yml): an MDM of
# its own (its own data folder and download folder) with a list that looks used, from its own lab
# server at a neutral address; every window opened through MDM's own command line (`/show`),
# light and dark (the system's appearance, which MDM follows), the style themes, the menu bar
# item, and a short screen recording. Nothing personal is on a runner; nothing of another
# program is shown but macOS's own menu bar and wallpaper.
#
#   showcase.sh <Mister Download Manager.app> <labserver program> <out folder>
#
# <out>/runner.txt says what the runner had (macOS, processor, displays and modes, what this
# process may do); <out>/light, <out>/dark, <out>/themes: the pictures, at the display's own
# pixels (screencapture -l: one window, without its shadow; *-shadow.png with it, on
# transparency); <out>/recording: the recording and what it measured.
set -u
APP=$1; LABSRV=$2; OUT=$3
WORK="$HOME/Demo"
rm -rf "$WORK"; mkdir -p "$WORK" "$OUT"
MDM_EXE="$APP/Contents/MacOS/misterdownloadmanager"
. "$(dirname "$0")/lib.sh"
SHADOW=1

mactool_build || { log "mactool didn't build"; exit 1; }

# ---- What the runner has
{
  sw_vers; uname -m; sysctl -n machdep.cpu.brand_string hw.memsize hw.ncpu
  echo; mt access
  echo; mt displays
  echo; system_profiler SPDisplaysDataType 2>/dev/null | sed -n '1,40p'
  echo; echo "ffmpeg: $(command -v ffmpeg || echo none)"
  echo "wallpaper: $(osascript -e 'tell application "System Events" to get picture of current desktop' 2>&1)"
} >"$OUT/runner.txt" 2>&1
cat "$OUT/runner.txt"

# ---- The sharpest picture the display offers: a HiDPI mode (2 pixels a point) of the largest
# size that has one, if the display offers any.
best=$(mt displays | awk '/^  mode/ {split($2, p, "x"); split(substr($3, 8), q, "x");
         if (q[1] == 2 * p[1] && p[1] >= 1280 && $5 == "gui=true") print p[1], p[2], q[1]}' | sort -n -r | head -1)
if [ -n "$best" ]; then
  log "HiDPI mode offered: $best"
  # shellcheck disable=SC2086
  mt mode $best | tee -a "$OUT/runner.txt"
  sleep 3
  mt displays | head -1 | tee -a "$OUT/runner.txt"
else
  log "no HiDPI mode offered: pictures at 1 pixel a point" | tee -a "$OUT/runner.txt"
fi
read -r SW SH < <(mt displays | awk '/^screen/ {for (i = 1; i <= NF; i++) if ($i ~ /^frame=/) {split(substr($i, 7), f, "x"); print f[1], f[2]; exit}}')
PARK_X=$((SW - 2)); PARK_Y=$((SH / 2))
log "screen ${SW}x${SH} points"

desktop_quiet
lab_start "$LABSRV" || { log "no lab server"; exit 1; }

# A list that looks used: finished files, one still coming in (rate-limited per connection: 256K
# over 8 connections is about 2 MB/s, so the progress window shows its 8 connections and the
# segment map for minutes).
fill_list() {
  local f name size q
  for f in big-buck-bunny-1080p.mp4:180M project-report-2026.pdf:4M holiday-photos.zip:64M \
           podcast-episode-42.mp3:38M rust-1.99.1-aarch64-apple-darwin.tar.xz:92M \
           open-data-2026.csv:12M "ubuntu-26.04-desktop-arm64.iso:6G:?rate=256K"; do
    IFS=: read -r name size q <<< "$f"
    mdm /d "$BASE/gen/$size/$name${q:-}" /p "$WORK/Downloads" /n
    sleep 0.4
  done
}

# The menu bar item (the status item, MDM's notification-area icon): the right of the menu bar,
# then its menu opened with a click (when this runner lets a click through).
menu_bar_item() { # <dir>
  local item x y w h
  item=$(mt windows "$PID" | awk -F'\t' '$3 == 25 {print $4, $5, $6, $7; exit}')
  if [ -z "$item" ]; then log "no menu bar item"; mt windows "$PID" >"$1/windows.txt"; return; fi
  read -r x y w h <<< "$item"
  log "menu bar item at $x,$y ${w}x$h"
  screencapture -x -R "$((SW - 520)),0,520,$((h + 4))" "$1/menu-bar.png"
  mt click $((x + w / 2)) $((y + h / 2)); sleep 1.5
  mt windows "$PID" >"$1/windows-after-click.txt"
  screencapture -x -R "$((x - 260)),0,520,420" "$1/menu-bar-menu.png"
  mt key 53; sleep 1
  # The same through System Events, for a runner whose clicks don't reach.
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $PID) to click menu bar item 1 of menu bar 2" >"$1/menu-bar-osascript.txt" 2>&1
  sleep 1.5
  screencapture -x -R "$((x - 260)),0,520,420" "$1/menu-bar-menu-osascript.png"
  mt key 53; sleep 1; park
}

for mode in light dark; do
  dir="$OUT/$mode"; mkdir -p "$dir"
  log "---- $mode"
  appearance "$mode"
  DATA="$WORK/data-$mode"; rm -rf "$DATA" "$WORK/Downloads"
  mdm_settings "$DATA" "$WORK/Downloads" follow '"show_complete":false,"show_progress":false'
  mdm_start "$dir/mdm.log" || { mt windows >"$dir/windows.txt"; screen "$dir/screen-no-window.png"; continue; }
  mt windows "$PID" >"$dir/windows-start.txt"
  fill_list
  sleep 20
  park; sleep 1
  screen "$dir/screen-main.png"
  shoot "$MAIN" "$dir/main.png"
  # View → Theme open (a menu: MDM's own windows, taken from the screen).
  read -r x y w h < <(bounds "$MAIN")
  mdm /show menu:3:theme; sleep 2
  mt windows "$PID" >"$dir/windows-menu.txt"
  screencapture -x -R "$x,$y,$((w + 280)),$h" "$dir/menu-theme.png"
  mt key 53; sleep 0.5; mt key 53; sleep 1
  # The windows, by name.
  for pair in progress=progress options=options:connection add-url=addurl scheduler=scheduler \
              safari=safari-steps about=about speed-limiter=speedlimit; do
    name=${pair%%=*}; show=${pair#*=}
    if w=$(opened mdm /show "$show"); then sleep 2; park; shoot "$w" "$dir/$name.png"; else log "$name: no window"; fi
  done
  screen "$dir/screen-windows.png"
  [ "$mode" = light ] && menu_bar_item "$dir"
  # The style themes, the main window and the progress window in each.
  if [ "$mode" = light ] || [ "$mode" = dark ]; then
    tdir="$OUT/themes-$mode"; mkdir -p "$tdir"
    prog=$(mine | while read -r id; do [ "$id" != "$MAIN" ] && echo "$id"; done | head -1)
    for t in aqua metro emerald luna aero adwaita breeze native high-contrast classic; do
      mdm /show "theme:$t"; sleep 2.5; park
      shoot "$MAIN" "$tdir/main-$t.png"
    done
  fi
  mt windows "$PID" >"$dir/windows-end.txt"
  mdm_exit; echo "exit after /exit: $EXIT_SECONDS s ($mode, windows open)" >>"$OUT/runner.txt"
done

# ---- The recording: the list filling, the progress window opening on a running download, a
# theme switched, Options opened. screencapture -v first (macOS's own), then ffmpeg's
# avfoundation at 60 frames a second when ffmpeg is there.
rdir="$OUT/recording"; mkdir -p "$rdir"
appearance light
DATA="$WORK/data-rec"; rm -rf "$DATA" "$WORK/Downloads"
mdm_settings "$DATA" "$WORK/Downloads" follow '"show_complete":false,"show_progress":false'
if mdm_start "$rdir/mdm.log"; then
  fill_list; sleep 12; park
  sequence() {
    sleep 1.5; mdm /show progress; sleep 5
    mdm /show theme:aqua; sleep 3.5
    mdm /show theme:metro; sleep 3.5
    mdm /show theme:classic; sleep 2
    mdm /show options:connection; sleep 4
  }
  log "recording: screencapture -v"
  screencapture -x -v -V 24 "$rdir/screencapture.mov" >"$rdir/screencapture.log" 2>&1 &
  rec=$!
  sequence
  for _ in $(seq 40); do kill -0 "$rec" 2>/dev/null || break; sleep 1; done; sleep 3
  ls -l "$rdir"
  if command -v ffmpeg >/dev/null; then
    ffmpeg -hide_banner -f avfoundation -list_devices true -i "" >"$rdir/avfoundation-devices.txt" 2>&1
    mdm_exit; mdm_start "$rdir/mdm2.log" && { fill_list; sleep 8; park; }
    log "recording: ffmpeg avfoundation, 60 fps"
    to 40 ffmpeg -hide_banner -y -f avfoundation -capture_cursor 0 -framerate 60 -i "Capture screen 0:none" \
      -t 24 -c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p "$rdir/ffmpeg-60fps.mp4" >"$rdir/ffmpeg.log" 2>&1 &
    rec=$!
    sequence
    wait "$rec" 2>/dev/null
  fi
  for f in "$rdir"/*.mov "$rdir"/*.mp4; do
    [ -f "$f" ] || continue
    if command -v ffprobe >/dev/null; then
      ffprobe -v error -select_streams v:0 -count_frames -show_entries stream=width,height,r_frame_rate,avg_frame_rate,nb_read_frames,duration -of default=nw=1 "$f" >"$f.txt" 2>&1
    else
      mdls -name kMDItemPixelWidth -name kMDItemPixelHeight -name kMDItemDurationSeconds -name kMDItemCodecs "$f" >"$f.txt" 2>&1
    fi
    log "$(basename "$f"): $(tr '\n' ' ' <"$f.txt")"
  done
  mdm_exit
fi

lab_stop
appearance light
rm -rf "$WORK/Downloads"
log "pictures in $OUT"

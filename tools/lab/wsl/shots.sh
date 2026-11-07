#!/usr/bin/env bash
# Screenshots of MDM's windows on the lab's virtual screen: ONE MDM per language and mode, each
# window opened by MDM's own command line (`/show <window>`, as tools/lab/docs-shots.ps1 does
# on Windows), captured once it has painted, closed, then the next. Nothing shows on Windows
# (env.sh), nothing is clicked: the pointer only moves at the screen's edge.
#
#   shots.sh <label> [--langs en,de,ar] [--modes light,dark] [--windows <name>[=<show>],...]
#            [--theme <id>] [--out <dir>] [--finished-only] [--keep-list]
#   e.g. shots.sh main --langs en,de,ar,ja,hi
#        shots.sh pr-199 --modes dark --theme adwaita --windows main,options-general=options:general
#   From Windows: wsl -d mdm-lab -- bash ~/lab/tools/shots.sh main --langs en,de
# (Lists take commas or spaces; commas pass through wsl.exe, which re-parses its command line.)
#
# <label>: a build in ~/lab/bin (build.sh). Shots go to ~/lab/shots/<label>/<lang>-<mode>/<name>.png
# (from Windows: \\wsl$\mdm-lab\home\lab\lab\shots\...), with timing.txt beside them. A window
# name is the file's; `=<show>` is MDM's `/show` argument when it differs (default list: the
# README's 21 windows). `main` is the main window; a `menu:...` show is captured with the main
# window, grown by MDM_GROW pixels (default 300) for menus past its edge.
#
# Each run has its own data folder (MDM_DATA_DIR) and download folder under ~/lab/run/<label>,
# and its own lab server (port 8197): the list looks used (the README's files), downloaded once
# for the first language and given to the others. --finished-only: no download still running (shots
# that must not change between runs: progress and the main window's speeds otherwise do);
# --keep-list: the list of the label's last run again (the same names and dates: two runs
# compared pixel by pixel, e.g. two renderers). MDM_SET: more settings, a JSON object; MDM_* in
# the environment (MDM_RENDERER, ...) reach MDM.
# MDM_OFFSCREEN passes through when set (Xvfb is off the user's screen already, so it's off by
# default: the shots show MDM as a person sees it).
set -uo pipefail
. "$HOME/lab/tools/env.sh"
label=${1:?usage: shots.sh <label> [--langs ...] [--modes ...] [--windows ...] [--theme ...] [--out ...]}
shift
langs="en"; modes="light"; theme=classic; out=""; finished_only=0; keep_list=0
windows="main add-url=addurl batch find speed-limiter=speedlimit properties scheduler about
  options-general=options:general options-connection=options:connection options-save-to=options:saveto
  options-panels=options:panels options-regional=options:regional options-file-types=options:types
  options-archives=options:archives grabber language-picker=language helpers extension
  delete-confirm=delete progress"
while [ $# -gt 0 ]; do
  case "$1" in
    --langs) langs=${2//,/ }; shift ;;
    --modes) modes=${2//,/ }; shift ;;
    --windows) windows=${2//,/ }; shift ;;
    --theme) theme=$2; shift ;;
    --out) out=$2; shift ;;
    --finished-only) finished_only=1 ;;
    --keep-list) keep_list=1 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
bin=$HOME/lab/bin/$label
app=$bin/misterdownloadmanager
[ -x "$app" ] || { echo "no build $label in ~/lab/bin (build.sh)"; exit 1; }
out=${out:-$HOME/lab/shots/$label}
work=$HOME/lab/run/$label
port=8197
base=http://127.0.0.1:$port
grow=${MDM_GROW:-300}
mkdir -p "$out"
# One run at a time on the screen (windows raised and closed by another run would land in its
# shots): a second one waits.
exec 9>"$HOME/lab/screen.lock"
flock -n 9 || { echo "another shots.sh is running; waiting for it"; flock 9; }
bash "$HOME/lab/tools/screen.sh" start >/dev/null || exit 1
read -r SW SH < <(xdotool getdisplaygeometry)
# Times from the kernel's uptime: WSL's wall clock jumps (back, too) when it syncs with Windows.
now() { cut -d' ' -f1 /proc/uptime; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN {printf "%.1f", b - a}'; }

# --- The lab server, ours alone (stopped at the end).
srv_pid=""
cleanup() {
  [ -n "${PID:-}" ] && { kill "$PID" 2>/dev/null; sleep 0.5; kill -9 "$PID" 2>/dev/null; }
  [ -n "$srv_pid" ] && kill "$srv_pid" 2>/dev/null
}
trap cleanup EXIT
if [ "$keep_list" = 1 ] && [ -f "$work/items.json" ]; then
  find "$work" -mindepth 1 -maxdepth 1 ! -name items.json ! -name Downloads -exec rm -rf {} +
else
  rm -rf "$work"
fi
mkdir -p "$work/Downloads"
"$bin/labserver" --bind "127.0.0.1:$port" >"$work/labserver.log" 2>&1 9>&- &
srv_pid=$!

# --- Windows of our MDM (the process id wmctrl reads from _NET_WM_PID), newest last.
mine() { wmctrl -lp 2>/dev/null | awk -v p="$PID" '$3 == p {print $1}'; }
# Capture `win` into `png` once it has painted (capture.py: two captures in a row the same, not
# one colour; at most MDM_SHOT_WAIT seconds, default 2), raised and focused first as a newly
# opened window is (an off-screen run, MDM_OFFSCREEN, leaves a dialog behind its window).
shoot() { # shoot <win> <png> [grow px]
  wmctrl -i -a "$1" 2>/dev/null; sleep 0.1
  python3 "$HOME/lab/tools/capture.py" "$1" "$2" "${3:-0}" "${MDM_SHOT_WAIT:-2}"
}
close_new() { # every window not in $1 (the ones before) closed as a person would (title bar ✕)
  local w
  for w in $(mine); do grep -qx "$w" <<<"$1" || wmctrl -i -c "$w"; done
  for _ in $(seq 30); do [ "$(mine | grep -cvxF -f <(echo "$1"))" = 0 ] && return; sleep 0.1; done
  xdotool key Escape 2>/dev/null
}

# --- The settings every run starts from (docs-shots.ps1's): the language chosen, nothing asked,
# nothing reaching out, every category's folder in the run's download folder.
write_state() { # write_state <data dir> <lang> <mode>
  python3 - "$1/downloads.json" "$2" "$3" "$theme" "$work/Downloads" "$work/items.json" <<'PY'
import json, os, sys
path, lang, mode, theme, dl, items = sys.argv[1:7]
cats = ["General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents"]
s = dict(language=lang, language_asked=True, helpers_asked=True, extension_never_ask=True,
         dark_mode_support=(mode != "light"), always_dark=(mode == "dark"), theme=theme,
         update_check=False, torrent_incoming=False, torrent_upnp=False, torrent_dht=False,
         torrent_lsd=False, torrent_stop_idle=True,
         category_dirs=[[c, dl if c == "General" else os.path.join(dl, c)] for c in cats])
s.update(json.loads(os.environ.get("MDM_SET") or "{}"))
d = {"next_id": 1, "items": [], "version": 2}
if os.path.exists(items):
    d.update(json.load(open(items)))
d["settings"] = s
json.dump(d, open(path, "w"))
PY
}
# The README's list: finished files and one still coming in (rate-limited per connection).
seed() {
  local f
  for f in big-buck-bunny-1080p.mp4:180M project-report-2026.pdf:4M holiday-photos.zip:64M \
           podcast-episode-42.mp3:38M rust-1.99.1-x86_64-unknown-linux-gnu.tar.xz:92M \
           open-data-2026.csv:12M "ubuntu-26.04-desktop-amd64.iso:6G:?rate=64K"; do
    IFS=: read -r name size q <<<"$f"
    [ "$finished_only" = 1 ] && [ -n "$q" ] && continue
    timeout 20 "$app" /d "$base/gen/$size/$name${q:-}" /p "$work/Downloads" /n >/dev/null 2>&1
  done
  # The finished ones' dialogs (Download complete) closed as they come, never the main window.
  for _ in $(seq 40); do
    for w in $(mine); do [ "$w" != "$MAIN" ] && wmctrl -i -c "$w" 2>/dev/null; done
    sleep 0.25
  done
  python3 - "$DATA/downloads.json" "$work/items.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
json.dump({"items": d.get("items", []), "next_id": d.get("next_id", 1)}, open(sys.argv[2], "w"))
print(f"  list: {len(d.get('items', []))} downloads")
PY
}

total_start=$(now); shots=0; shot_time=0
timing="$out/timing.txt"
echo "label $label ($(cat "$bin/COMMIT" 2>/dev/null)), langs: $langs, modes: $modes, theme $theme" | tee "$timing"
for lang in $langs; do
  for mode in $modes; do
    combo_start=$(now)
    DATA=$work/data-$lang-$mode
    rm -rf "$DATA"; mkdir -p "$DATA"
    write_state "$DATA" "$lang" "$mode"
    export MDM_DATA_DIR=$DATA
    dir="$out/$lang-$mode"; rm -rf "$dir"; mkdir -p "$dir"
    xdotool mousemove $((SW - 2)) $((SH / 2))
    (cd "$work" && setsid -f "$app" >"$DATA/stdout.log" 2>&1 </dev/null 9>&-)
    PID=""
    for _ in $(seq 300); do
      PID=$(pgrep -u "$(id -u)" -f "^$app$" | head -1)
      [ -n "$PID" ] && [ -n "$(mine)" ] && break
      sleep 0.1
    done
    MAIN=$(mine | head -1)
    [ -z "$MAIN" ] && { echo "$lang-$mode: MDM showed no window (see $DATA/stdout.log)"; continue; }
    [ -f "$work/items.json" ] || seed
    started=$(since "$combo_start")
    for entry in $windows; do
      name=${entry%%=*}; show=${entry#*=}
      t0=$(now)
      before=$(mine)
      win=$MAIN; g=0
      case "$show" in
        main) ;;
        menu:*|theme:*) timeout 20 "$app" /show "$show" >/dev/null 2>&1; g=$grow ;;
        *)
          timeout 20 "$app" /show "$show" >/dev/null 2>&1
          # Options with a dialog over it (options:panels): the dialog is the shot. The newest
          # window is the one created last (the highest id: the window manager's list doesn't
          # keep that order).
          want=1; [ "$show" = options:panels ] && want=2
          win=""
          for _ in $(seq 100); do
            new=$(mine | grep -vxF -f <(echo "$before"))
            [ "$(grep -c . <<<"$new")" -ge "$want" ] && {
              win=$(while read -r w; do printf '%d %s\n' "$w" "$w"; done <<<"$new" | sort -n | tail -1 | cut -d' ' -f2)
              break; }
            sleep 0.1
          done
          [ -z "$win" ] && { echo "  $name: no window"; continue; }
          ;;
      esac
      shoot "$win" "$dir/$name.png" "$g"
      [ "$show" != main ] && close_new "$before"
      dt=$(since "$t0"); shots=$((shots + 1)); shot_time=$(awk -v a="$shot_time" -v b="$dt" 'BEGIN {print a + b}')
      echo "  $lang-$mode/$name.png ${dt}s" >>"$timing"
    done
    kill "$PID" 2>/dev/null; for _ in $(seq 20); do kill -0 "$PID" 2>/dev/null || break; sleep 0.1; done
    kill -9 "$PID" 2>/dev/null; PID=""
    echo "$lang-$mode: start ${started}s, $(ls "$dir" | wc -l) shots, $(since "$combo_start")s in all" | tee -a "$timing"
  done
done
total=$(since "$total_start")
echo "== $shots shots in ${total}s: $(awk -v t="$total" -v n="$shots" 'BEGIN {printf "%.2f", t / (n ? n : 1)}') s per shot in all, $(awk -v t="$shot_time" -v n="$shots" 'BEGIN {printf "%.2f", t / (n ? n : 1)}') s per window shot -> $out" | tee -a "$timing"

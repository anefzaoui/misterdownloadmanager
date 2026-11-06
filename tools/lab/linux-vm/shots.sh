#!/bin/bash
# One MDM window ready for a screenshot: MDM restarted with the language and mode set in its
# settings, the window opened with `/show <name>` and raised; prints its frame on the screen as
# "X Y W H" for the host to crop VirtualBox's screenshot to (vm.ps1 shot; capturing an OpenGL
# window from inside the guest gives an empty image).
#   shots.sh <label> <main|fileinfo|show name> <lang> <light|dark>   e.g. shots.sh base extension de dark
. ~/lab/env.sh
label=$1; show=$2; lang=$3; mode=$4
app=$HOME/lab/bin/$label/misterdownloadmanager
data=$HOME/.local/share/misterdownloadmanager
pkill -x misterdownloadm; sleep 2
# MDM_STATE: a saved list and settings put back first, so builds compared see the same list.
[ -n "${MDM_STATE:-}" ] && cp "$MDM_STATE" "$data/downloads.json"
# MDM_SEEN: the browsers whose extension reached MDM, "Name<TAB>version" lines with "\n"
# between (printf's %b turns them into real tabs and newlines).
[ -n "${MDM_SEEN:-}" ] && printf '%b\n' "$MDM_SEEN" > "$data/extension-seen"
python3 - "$data/downloads.json" "$lang" "$mode" <<'PY'
import json, sys
p, lang, mode = sys.argv[1:4]
d = json.load(open(p))
s = d.setdefault("settings", {})
s.update(language=lang, language_asked=True, always_dark=(mode == "dark"), dark_mode_support=(mode == "dark"))
json.dump(d, open(p, "w"))
PY
cd ~; setsid -f "$app" >/dev/null 2>&1 </dev/null
pid=""
for _ in $(seq 60); do
  pid=$(pgrep -f "^$app$" | head -1)
  [ -n "$pid" ] && wmctrl -lp | awk -v p="$pid" '$3 == p' | grep -q . && break
  sleep 1
done
main=$(wmctrl -lp | awk -v p="$pid" '$3 == p {print $1}' | head -1)
sleep 4
# `main`: the main window itself; `fileinfo`: Download File Info for a lab file (`/d`); any other
# name: `/show <name>`.
case "$show" in
  main) ;;
  fileinfo) timeout 20 "$app" /d "http://127.0.0.1:8097/gen/1M/shot.zip" ;;
  *) timeout 20 "$app" /show "$show" ;;
esac
win=$main
for _ in $(seq 40); do
  [ "$show" = main ] && break
  win=$(wmctrl -lp | awk -v p="$pid" -v m="$main" '$3 == p && $1 != m {print $1}' | tail -1)
  [ -n "$win" ] && break
  sleep 0.5
done
sleep 2
timeout 5 wmctrl -i -a "$win"; sleep 1
# The client area, widened by the window manager's frame (title bar and borders).
read -r l r top b < <(timeout 5 xprop -id "$win" _NET_FRAME_EXTENTS | grep -o '[0-9]\+' | tr '\n' ' ')
timeout 5 xwininfo -id "$win" | awk -v l="${l:-0}" -v r="${r:-0}" -v t="${top:-0}" -v b="${b:-0}" '/Absolute upper-left X/ {x=$4} /Absolute upper-left Y/ {y=$4} /Width:/ {w=$2} /Height:/ {h=$2}
   END {print x - l, y - t, w + l + r, h + t + b}'

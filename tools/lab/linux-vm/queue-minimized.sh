#!/bin/bash
# #176 on X11: a minimized MDM keeps working. Minimize to the notification area off, the main
# queue set to one file at a time and to start 75 s from now; three lab files (throttled, a few
# seconds each) added to it, then the window minimized. Prints when the queue started, each
# file's completion (order, time) and each "Download complete" window, and the main window's state.
#   queue-minimized.sh <label>     (a build in ~/lab/bin/<label>; the lab server on :8097)
. ~/lab/env.sh
app=$HOME/lab/bin/$1/misterdownloadmanager
data=$HOME/.local/share/misterdownloadmanager
dir=$HOME/Downloads/Compressed
tag=q$(date +%H%M%S)
pkill -x misterdownloadm; sleep 2
start=$(date -d '+75 seconds' '+%H %M %S')
python3 - "$data/downloads.json" $start <<'PY'
import json, sys
p, h, m, s = sys.argv[1], *map(int, sys.argv[2:5])
d = json.load(open(p))
st = d.setdefault("settings", {})
st.update(minimize_to_tray=False, language="en", language_asked=True, extension_never_ask=True,
          always_dark=False, dark_mode_support=False)
for q in d.get("queues", []):
    if not q.get("sync"):
        q["files_at_once"] = 1
        q["schedule"].update(start_at=[h, m, s], days=[True] * 7, once=None, start_on_startup=False)
json.dump(d, open(p, "w"))
PY
echo "== $1: the main queue starts at $(echo $start | tr ' ' ':'), one file at a time"
cd ~; setsid -f "$app" >/dev/null 2>&1 </dev/null
main=""
for _ in $(seq 60); do
  pid=$(pgrep -f "^$app$" | head -1)
  [ -n "$pid" ] && main=$(wmctrl -lp | awk -v p="$pid" '$3 == p && /Mister Download Manager 0\./ {print $1}' | head -1)
  [ -n "$main" ] && break; sleep 1
done
sleep 4
for n in 1 2 3; do
  "$app" /d "http://127.0.0.1:8097/gen/2M/$tag-$n.zip?rate=400K" /a /n
  sleep 1
done
sleep 3
xdotool windowminimize "$(printf %d "$main")"; sleep 2
state() { xprop -id "$main" WM_STATE 2>/dev/null | grep -o 'Normal\|Iconic'; }
echo "$(date +%T) files added, main window: $(state)"
seen=""; done_files=""
end=$(( $(date +%s) + 180 ))
while [ "$(date +%s)" -lt $end ]; do
  for n in 1 2 3; do
    f="$dir/$tag-$n.zip"
    if [ -f "$f" ] && [ "$(stat -c %s "$f")" = 2097152 ] && ! echo "$done_files" | grep -q " $n "; then
      done_files="$done_files $n "; echo "$(date +%T) file $n complete; main window: $(state)"
    fi
  done
  for w in $(wmctrl -lp | awk -v p="$pid" '$3 == p && /Download complete/ {print $1}'); do
    echo "$seen" | grep -q "$w" && continue
    seen="$seen $w"; echo "$(date +%T) a Download complete window opened ($w); main window: $(state)"
  done
  [ "$(echo $done_files | wc -w)" = 3 ] && [ "$(echo $seen | wc -w)" -ge 3 ] && break
  sleep 1
done
echo "$(date +%T) finished: files$done_files; Download complete windows: $(echo $seen | wc -w); main window: $(state)"

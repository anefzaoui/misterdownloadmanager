#!/bin/bash
# #162/#173 on X11: with MDM's window minimized (Minimize to the notification area off), a magnet
# from a second launch and a download through the browser's host; how long until each one's
# window opens. Torrents' network is off (no DHT, UPnP, local discovery or incoming connections):
# the magnet only opens its window, nothing reaches the internet.
#   handoff-minimized.sh <label>     (a build in ~/lab/bin/<label>)
. ~/lab/env.sh
bin=$HOME/lab/bin/$1
app=$bin/misterdownloadmanager
data=$HOME/.local/share/misterdownloadmanager
pkill -x misterdownloadm; sleep 2
python3 - "$data/downloads.json" <<'PY'
import json, sys
p = sys.argv[1]
d = json.load(open(p))
s = d.setdefault("settings", {})
s.update(minimize_to_tray=False, torrent_dht=False, torrent_upnp=False, torrent_lsd=False, torrent_incoming=False,
         language="en", language_asked=True, extension_never_ask=True)
json.dump(d, open(p, "w"))
PY
cd ~; setsid -f "$app" >/dev/null 2>&1 </dev/null
# This instance's own main window (by its process), not one left from an earlier run.
main=""
for _ in $(seq 60); do
  pid=$(pgrep -f "^$app$" | head -1)
  [ -n "$pid" ] && main=$(wmctrl -lp | awk -v p="$pid" '$3 == p && /Mister Download Manager 0\./ {print $1}' | head -1)
  [ -n "$main" ] && break; sleep 1
done
[ -z "$main" ] && { echo "MDM's main window didn't appear"; exit 1; }
sleep 5
state() { xprop -id "$main" WM_STATE 2>/dev/null | grep -o 'Normal\|Iconic\|Withdrawn'; }
minimize() { xdotool windowminimize "$(printf %d "$main")"; sleep 2; }
# Wait for a window whose title contains $1: the seconds it took, or "not in 30 s".
opened() {
  local t0; t0=$(date +%s.%N)
  for _ in $(seq 150); do
    if wmctrl -l | grep -q -F -- "$1"; then echo "$(echo "$(date +%s.%N) - $t0" | bc | cut -c1-4) s"; return; fi
    sleep 0.2
  done
  echo "not in 30 s"
}
echo "== $1 (pid $pid), main window $main, load $(cut -d" " -f1-3 /proc/loadavg)"
minimize; echo "main window: $(state)"
"$app" 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=lab-minimized' &
echo "magnet (second launch) -> Add torrent: $(opened 'Add torrent')    main window then: $(state)"
wmctrl -c 'Add torrent'; sleep 2
minimize; echo "main window: $(state)"
python3 ~/lab/nmping.py "$bin/misterdownloadmanager-nmhost" '{"url":"http://127.0.0.1:8097/gen/1M/minimized.zip","referrer":"http://127.0.0.1:8097/page","size":1048576}' | tail -1 &
echo "download (browser host) -> Download File Info: $(opened 'Download File Info')    main window then: $(state)"
wmctrl -c 'Download File Info'; sleep 1
wmctrl -l | cut -c1-90 | grep -v -E 'Desktop Icons|mdm@|MDM build'

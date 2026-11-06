#!/bin/bash
# A torrent in MDM's list without any network: a .torrent made here for a local file (no trackers),
# the torrent network off (no DHT, UPnP, local discovery, incoming connections), added through a
# second launch; Add torrent shows, then is confirmed. For the torrent windows' and menus' checks.
#   torrent-menu.sh <label> add       start MDM, open Add torrent for the lab torrent (prints its frame)
#   torrent-menu.sh <label> confirm   press Add torrent's default button
. ~/lab/env.sh
app=$HOME/lab/bin/$1/misterdownloadmanager
data=$HOME/.local/share/misterdownloadmanager
dir=$HOME/lab/t177
frame() {
  local w=$1
  read -r l r top b < <(xprop -id "$w" _NET_FRAME_EXTENTS | grep -o '[0-9]\+' | tr '\n' ' ')
  xwininfo -id "$w" | awk -v l="${l:-0}" -v r="${r:-0}" -v t="${top:-0}" -v b="${b:-0}" '/Absolute upper-left X/ {x=$4} /Absolute upper-left Y/ {y=$4} /Width:/ {w=$2} /Height:/ {h=$2}
    END {print x - l, y - t, w + l + r, h + t + b}'
}
case "$2" in
  add)
    mkdir -p "$dir"
    [ -f "$dir/lab-order.bin" ] || head -c 1048576 /dev/urandom > "$dir/lab-order.bin"
    python3 - "$dir/lab-order.bin" "$dir/lab-order.torrent" <<'PY'
import hashlib, sys
src, out = sys.argv[1:3]
data = open(src, "rb").read()
plen = 65536
pieces = b"".join(hashlib.sha1(data[i:i + plen]).digest() for i in range(0, len(data), plen))
def be(x):
    if isinstance(x, int): return b"i%de" % x
    if isinstance(x, str): x = x.encode()
    if isinstance(x, bytes): return b"%d:%s" % (len(x), x)
    if isinstance(x, dict): return b"d" + b"".join(be(k) + be(x[k]) for k in sorted(x)) + b"e"
info = {"name": "lab-order.bin", "length": len(data), "piece length": plen, "pieces": pieces}
open(out, "wb").write(be({"info": info, "created by": "mdm lab"}))
PY
    pkill -x misterdownloadm; sleep 2
    python3 - "$data/downloads.json" <<'PY'
import json, sys
p = sys.argv[1]
d = json.load(open(p))
d["settings"].update(torrent_dht=False, torrent_upnp=False, torrent_lsd=False, torrent_incoming=False,
                     language="en", language_asked=True, always_dark=False, dark_mode_support=False)
json.dump(d, open(p, "w"))
PY
    cd ~; setsid -f "$app" >/dev/null 2>&1 </dev/null; sleep 12
    "$app" "$dir/lab-order.torrent"
    for _ in $(seq 40); do w=$(wmctrl -l | grep -F 'Add torrent' | awk '{print $1}' | head -1); [ -n "$w" ] && break; sleep 0.5; done
    sleep 2; wmctrl -i -a "$w"; sleep 1; frame "$w" ;;
  confirm)
    w=$(wmctrl -l | grep -F 'Add torrent' | awk '{print $1}' | head -1)
    wmctrl -i -a "$w"; sleep 0.5; xdotool key Return; sleep 3
    wmctrl -l | grep -c -F 'Add torrent' ;;
esac

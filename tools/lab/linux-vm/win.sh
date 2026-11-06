#!/bin/bash
# One MDM kept running while the host screenshots several of its windows in turn (faster than
# shots.sh, which restarts MDM for each): start it once with a language, mode and theme, then
# open, shoot and close windows one by one. Each `open` prints the window's frame as "X Y W H"
# for the host to crop VirtualBox's screenshot to (shot-host.sh's way).
#
#   win.sh start <label> <lang> <light|dark|system> [theme]   MDM restarted with these settings
#                                       (system: Dark Mode support, following the desktop; theme:
#                                       classic, native, high-contrast, metro, adwaita, breeze…)
#   win.sh open <main|fileinfo|show name>              e.g. options, scheduler, addurl, progress,
#                                       properties, about, menu:3:theme
#   win.sh close                         every MDM window but the main one, closed as a person would
#   win.sh theme <id>                    View → Theme → <id>, live (/show theme:<id>)
#   win.sh click|hover|press|release <x> <y>   real input at a point of the main window's frame
#                                        (MDM_ON=last: of the window last opened)
#   win.sh key <key>...                  keys to the focused window (xdotool names)
#   win.sh stop                          MDM ended
#
# MDM_STATE: a saved downloads.json put back first (as shots.sh); MDM_SET: more settings, a JSON
# object (e.g. '{"net_adapter":"none0"}'). MDM_NATIVE_ACCENT and
# MDM_NATIVE_CONTRAST pass through to MDM (the Native theme's stand-ins for the system's).
. ~/lab/env.sh
data=$HOME/.local/share/misterdownloadmanager
state=/tmp/mdm-win.state
cmd=$1; shift

frame() { # the window's frame, widened by the window manager's title bar and borders
  local win=$1 l r t b
  read -r l r t b < <(timeout 5 xprop -id "$win" _NET_FRAME_EXTENTS | grep -o '[0-9]\+' | tr '\n' ' ')
  timeout 5 xwininfo -id "$win" | awk -v l="${l:-0}" -v r="${r:-0}" -v t="${t:-0}" -v b="${b:-0}" '/Absolute upper-left X/ {x=$4} /Absolute upper-left Y/ {y=$4} /Width:/ {w=$2} /Height:/ {h=$2}
     END {print x - l, y - t, w + l + r, h + t + b}'
}
mine() { wmctrl -lp | awk -v p="$PID" '$3 == p {print $1}'; }

case "$cmd" in
  start)
    label=$1; lang=$2; mode=$3; theme=${4:-classic}
    app=$HOME/lab/bin/$label/misterdownloadmanager
    # The last MDM gone for good first: a new one started while it still ends hands its command
    # line to it and leaves, and nothing is left running.
    pkill -x misterdownloadm
    for _ in $(seq 20); do pgrep -x misterdownloadm >/dev/null || break; sleep 0.5; done
    pkill -9 -x misterdownloadm; sleep 1
    [ -n "${MDM_STATE:-}" ] && cp "$MDM_STATE" "$data/downloads.json"
    python3 - "$data/downloads.json" "$lang" "$mode" "$theme" <<'PY'
import json, os, sys
p, lang, mode, theme = sys.argv[1:5]
d = json.load(open(p))
s = d.setdefault("settings", {})
s.update(language=lang, language_asked=True, always_dark=(mode == "dark"),
         dark_mode_support=(mode != "light"), theme=theme)
s.update(json.loads(os.environ.get("MDM_SET") or "{}"))
json.dump(d, open(p, "w"))
PY
    # The pointer parked at the screen's right edge, so no hover or tooltip shows in a shot.
    read -r sw sh < <(xdotool getdisplaygeometry); xdotool mousemove $((sw - 2)) $((sh / 2))
    cd ~; setsid -f "$app" >/tmp/mdm-win.log 2>&1 </dev/null
    for _ in $(seq 60); do
      PID=$(pgrep -f "^$app$" | head -1)
      [ -n "$PID" ] && [ -n "$(mine)" ] && break
      sleep 1
    done
    main=$(mine | head -1)
    echo "PID=$PID MAIN=$main APP=$app" > "$state"
    sleep 4
    timeout 5 wmctrl -i -a "$main"; sleep 1
    frame "$main"
    ;;
  open)
    . "$state"
    show=$1
    before=$(mine | tr '\n' ' ')
    case "$show" in
      main) ;;
      fileinfo) timeout 20 "$APP" /d "http://127.0.0.1:8097/gen/1M/shot.zip" ;;
      *) timeout 20 "$APP" /show "$show" ;;
    esac
    win=$MAIN
    for _ in $(seq 40); do
      case "$show" in main|menu:*|theme:*) break ;; esac
      new=$(mine | grep -v -F -x -f <(echo "$before" | tr ' ' '\n' | grep .) | tail -1)
      [ -n "$new" ] && win=$new && break
      sleep 0.5
    done
    sleep 2
    timeout 5 wmctrl -i -a "$win"; sleep 1
    sed -i '/^LAST=/d' "$state"; echo "LAST=$win" >> "$state"
    frame "$win"
    ;;
  close)
    . "$state"
    read -r sw sh < <(xdotool getdisplaygeometry); xdotool mousemove $((sw - 2)) $((sh / 2))
    for w in $(mine); do [ "$w" != "$MAIN" ] && timeout 5 wmctrl -i -c "$w"; done
    sleep 1.5
    # A menu or popup inside the main window: Escape closes it.
    timeout 5 xdotool key --window "$MAIN" Escape 2>/dev/null; sleep 0.5
    ;;
  theme)
    . "$state"
    timeout 20 "$APP" /show "theme:$1"; sleep 2
    frame "$MAIN"
    ;;
  click|hover|press|release)
    # Real input inside the VM: the pointer moved to <x> <y> in the main window's frame (a
    # negative x counts from its right edge, for right-to-left languages), then a click, a button
    # held down or let go, or nothing. Prints the main window's frame. (`/show menu:…` opens the
    # wrong popup on X11, so menus are opened as a person would.)
    . "$state"
    # MDM_ON=last: the point is in the window `open` showed last instead.
    target=$MAIN; [ "${MDM_ON:-}" = last ] && target=${LAST:-$MAIN}
    read -r fx fy fw fh < <(frame "$target")
    x=$1; y=$2; [ "$x" -lt 0 ] && x=$((fw + x))
    xdotool mousemove $((fx + x)) $((fy + y)); sleep 0.4
    case "$cmd" in
      click) xdotool click 1 ;;
      press) xdotool mousedown 1 ;;
      release) xdotool mouseup 1 ;;
    esac
    sleep "${MDM_SETTLE:-1.2}"
    echo "$fx $fy $fw $fh"
    ;;
  key)
    # Keys to the focused window, one by one: e.g. `key Tab Tab space`.
    for k in "$@"; do xdotool key "$k"; sleep 0.3; done
    sleep 0.8
    . "$state"; frame "$MAIN"
    ;;
  stop)
    pkill -x misterdownloadm
    ;;
  *) echo "win.sh start|open|close|theme|click|hover|press|release|key|stop"; exit 2 ;;
esac

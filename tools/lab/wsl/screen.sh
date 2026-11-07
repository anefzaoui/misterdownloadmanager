#!/usr/bin/env bash
# The lab's virtual screen: Xvfb :99 (1600x1000, 24-bit), a session bus and Metacity, GNOME's
# window manager without a compositor (title bars and borders as on a desktop; a capture of
# the screen reads what was drawn). Nothing of it shows on Windows (env.sh).
#
#   screen.sh start     started if it isn't (shots.sh starts it itself)
#   screen.sh stop      the screen ended, with every MDM, lab server and window on it (all of the
#                       lab user's: not while someone else's shots.sh runs, it holds ~/lab/screen.lock)
#   screen.sh status
. "$HOME/lab/tools/env.sh"
run=$XDG_RUNTIME_DIR
# (Programs that stay are started with `9>&-`: shots.sh's lock, fd 9, must not stay with them.)
up() { xdpyinfo >/dev/null 2>&1; }
case "${1:-status}" in
  start)
    if ! up; then
      # WSLg mounts /tmp/.X11-unix read-only: Xvfb can't put its socket file there and listens
      # on the abstract socket alone, which every X client tries first.
      setsid -f Xvfb :99 -screen 0 1600x1000x24 -nolisten tcp -dpi 96 >"$run/xvfb.log" 2>&1 </dev/null 9>&-
      for _ in $(seq 50); do up && break; sleep 0.1; done
      up || { echo "Xvfb did not start:"; cat "$run/xvfb.log"; exit 1; }
    fi
    if [ ! -S "$run/bus" ]; then
      dbus-daemon --session --address="unix:path=$run/bus" --fork --print-pid >"$run/dbus.pid" 9>&-
    fi
    if ! pgrep -u "$(id -u)" -x metacity >/dev/null; then
      setsid -f metacity --replace --no-composite >"$run/metacity.log" 2>&1 </dev/null 9>&-
      for _ in $(seq 50); do xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' && break; sleep 0.1; done
    fi
    # The pointer parked at the right edge: no hover or tooltip in a shot.
    xdotool mousemove 1598 500
    echo "screen :99 up ($(xdpyinfo | awk '/dimensions:/ {print $2}'), $(wmctrl -m | awk '/Name:/ {print $2}'))"
    ;;
  stop)
    exec 9>"$HOME/lab/screen.lock"
    flock -n 9 || { echo "shots.sh is running (~/lab/screen.lock): not stopped"; exit 1; }
    pkill -u "$(id -u)" -x misterdownloadm
    pkill -u "$(id -u)" -x labserver
    sleep 1
    pkill -9 -u "$(id -u)" -x misterdownloadm
    pkill -u "$(id -u)" -x metacity
    [ -f "$run/dbus.pid" ] && kill "$(cat "$run/dbus.pid")" 2>/dev/null; rm -f "$run/dbus.pid" "$run/bus"
    pkill -u "$(id -u)" -x Xvfb
    echo "screen :99 stopped"
    ;;
  status)
    if up; then echo "screen :99 up"; wmctrl -lp 2>/dev/null; else echo "screen :99 down"; fi
    ;;
  *) echo "screen.sh start|stop|status"; exit 2 ;;
esac

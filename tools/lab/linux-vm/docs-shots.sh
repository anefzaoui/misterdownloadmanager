#!/bin/bash
# The README's, docs' and site's Linux screenshots, on a real desktop: GNOME, KDE Plasma,
# Cinnamon or Xfce in a lab VM (docs-shots-host.ps1 runs this from the Windows host and makes the
# WebP files). The Linux counterpart of tools/lab/docs-shots.ps1: an MDM of its own (its own data
# folder, MDM_DATA_DIR, and download folders), its own lab server with neutral files, every window
# opened through MDM's own command line (`/show <window>`), at 1x.
#
#   docs-shots.sh <label> [out folder] [light,dark] [main,add-url,...]
#
# <label>: a build in ~/lab/bin/<label>/ (tools/lab/wsl/deploy.ps1). Shots land in
# <out folder>/<mode>/<name>.png (default ~/lab/shots/docs-<label>). The list (default: all):
#   main  add-url  progress  options  scheduler  add-torrent  browser-offer  tray
# `tray` (light only) is the top right of the screen with MDM's icon in the desktop's tray and its
# menu open, clicked as a person would: real input inside the VM, never on the host.
#
# Captured inside the guest (ImageMagick's `import -window root`, cropped to the window's frame,
# title bar included, from _NET_FRAME_EXTENTS): VirtualBox's own screenshots come out black since
# the guests' kernel update. A shot is kept once two reads in a row are the same (a progress
# window, which never settles, after a few tries).
#
# MDM_SUDO_PASSWORD (set by the host side): the lab server answers as http://downloads.example on
# port 80 (a reserved name added to /etc/hosts, unprivileged ports from 80 for this boot only), so
# the addresses in the shots are neutral; without it, http://127.0.0.1:8199.
set -u
. ~/lab/env.sh
label=$1
out=${2:-$HOME/lab/shots/docs-$label}
modes=${3:-light,dark}
only=${4:-}
bin=$HOME/lab/bin/$label
app=$bin/misterdownloadmanager
# The run's folder shows in some windows (Add torrent's): short and neutral, as docs-shots.ps1's
# C:\Demo.
run=$HOME/Demo
torrent=$HOME/lab/docs-shots.torrent
want() { [ -z "$only" ] || [[ ",$only," == *",$1,"* ]]; }
sudo_() { echo "$MDM_SUDO_PASSWORD" | sudo -S -p '' "$@"; }

# ---- The lab server, at a neutral address when it can be
port=8199; host=127.0.0.1
if [ -n "${MDM_SUDO_PASSWORD:-}" ] && ! ss -ltn | grep -q '127.0.0.1:80 '; then
  grep -q 'downloads\.example' /etc/hosts || sudo_ sh -c "echo '127.0.0.1 downloads.example' >> /etc/hosts"
  sudo_ sysctl -q net.ipv4.ip_unprivileged_port_start=80
  port=80; host=downloads.example
fi
base="http://$host"; [ "$port" = 80 ] || base="http://$host:$port"
pkill -f "$bin/labserver" 2>/dev/null
setsid -f "$bin/labserver" --bind "127.0.0.1:$port" >/dev/null 2>&1 </dev/null
sleep 1

# ---- The desktop: no notification banners during the shots (put back at the end), the overview
# closed, the pointer at the screen's right edge (no hover or tooltip).
banners=$(gsettings get org.gnome.desktop.notifications show-banners 2>/dev/null)
gsettings set org.gnome.desktop.notifications show-banners false 2>/dev/null
# The dark shots on a dark desktop, as someone in dark mode sees MDM: the window manager's title
# bar follows the desktop's theme, not MDM's (GNOME; put back at the end).
gtk_theme=$(gsettings get org.gnome.desktop.interface gtk-theme 2>/dev/null | tr -d "'")
scheme=$(gsettings get org.gnome.desktop.interface color-scheme 2>/dev/null | tr -d "'")
desktop_mode() {
  [ -n "$gtk_theme" ] || return 0
  if [ "$1" = dark ]; then
    gsettings set org.gnome.desktop.interface color-scheme prefer-dark
    case "$gtk_theme" in *-dark) ;; *) gsettings set org.gnome.desktop.interface gtk-theme "$gtk_theme-dark" ;; esac
  else
    gsettings set org.gnome.desktop.interface color-scheme "$scheme"
    gsettings set org.gnome.desktop.interface gtk-theme "$gtk_theme"
  fi
  sleep 2
}
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
  --method org.freedesktop.DBus.Properties.Set org.gnome.Shell OverviewActive '<false>' >/dev/null 2>&1
read -r SW SH < <(xdotool getdisplaygeometry)
park() { xdotool mousemove $((SW - 2)) $((SH / 2)); }

mine() { wmctrl -lp | awk -v p="$PID" '$3 == p {print $1}'; }
# The window's frame on the screen, "X Y W H": the client grown by the window manager's title bar
# and borders, clipped to the screen.
frame() {
  local l r t b
  read -r l r t b < <(timeout 5 xprop -id "$1" _NET_FRAME_EXTENTS | grep -o '[0-9]\+' | tr '\n' ' ')
  timeout 5 xwininfo -id "$1" | awk -v l="${l:-0}" -v r="${r:-0}" -v t="${t:-0}" -v b="${b:-0}" -v sw="$SW" -v sh="$SH" '
    /Absolute upper-left X/ {x=$4} /Absolute upper-left Y/ {y=$4} /Width:/ {w=$2} /Height:/ {h=$2}
    END { x -= l; y -= t; w += l + r; h += t + b
          if (x < 0) { w += x; x = 0 } if (y < 0) { h += y; y = 0 }
          if (x + w > sw) w = sw - x; if (y + h > sh) h = sh - y
          print x, y, w, h }'
}
# A rectangle of the screen into <file>, once two reads in a row are the same (at most 8 reads).
grab() {
  local x=$1 y=$2 w=$3 h=$4 file=$5 last="" sum
  for _ in $(seq 8); do
    import -silent -window root -crop "${w}x${h}+${x}+${y}" +repage "$file"
    sum=$(convert "$file" rgb:- 2>/dev/null | md5sum)
    [ "$sum" = "$last" ] && break
    last=$sum; sleep 0.8
  done
  echo "  $(basename "$file") ${w}x${h}"
}
shoot() { # <window id> <name>
  timeout 5 wmctrl -i -a "$1"; park; sleep 1.5
  read -r x y w h < <(frame "$1")
  grab "$x" "$y" "$w" "$h" "$dir/$2.png"
}
# The window that appears after <command>, by id (up to 20 s).
opened() {
  local before new
  before=$(mine | sort)
  "$@" >/dev/null 2>&1
  for _ in $(seq 40); do
    new=$(comm -13 <(echo "$before") <(mine | sort) | tail -1)
    # Not the main window under a new id (its title: the program's name and version).
    [ -n "$new" ] && ! wmctrl -l | grep -i "^$new " | grep -q "Mister Download Manager [0-9]" && { echo "$new"; return 0; }
    sleep 0.5
  done
  return 1
}
close_dialogs() { for w in $(mine); do [ "$w" != "$MAIN" ] && timeout 5 wmctrl -i -c "$w"; done; sleep 1; }

IFS=, read -r -a mode_list <<< "$modes"
for mode in "${mode_list[@]}"; do
  dir=$out/$mode; mkdir -p "$dir"
  work=$run
  pkill -f "^$app$" 2>/dev/null; sleep 1
  desktop_mode "$mode"
  rm -rf "$work"; mkdir -p "$work/data" "$work/Downloads"
  export MDM_DATA_DIR=$work/data
  # Its own list and settings: the language chosen, light or dark whatever the desktop's mode,
  # every category's folder under the run's own, nothing reaching out (no update check; the
  # torrent engine neither listens nor uses the DHT, UPnP or local discovery).
  python3 - "$work" "$mode" <<'PY'
import json, sys
work, mode = sys.argv[1:3]
dark = mode == "dark"
cats = ["General", "Compressed", "Documents", "Music", "Programs", "Video", "Torrents"]
dirs = [[c, f"{work}/Downloads" + ("" if c == "General" else f"/{c}")] for c in cats]
settings = dict(language="en", language_asked=True, helpers_asked=True, extension_never_ask=True,
                dark_mode_support=dark, always_dark=dark, update_check=False, torrent_incoming=False,
                torrent_upnp=False, torrent_dht=False, torrent_lsd=False, torrent_stop_idle=True,
                category_dirs=dirs)
json.dump({"next_id": 1, "items": [], "version": 2, "settings": settings},
          open(f"{work}/data/downloads.json", "w"))
PY
  park
  cd ~; setsid -f "$app" >"$work/mdm.log" 2>&1 </dev/null
  PID=""; MAIN=""
  for _ in $(seq 60); do
    PID=$(pgrep -f "^$app$" | head -1)
    [ -n "$PID" ] && MAIN=$(mine | head -1) && [ -n "$MAIN" ] && break
    sleep 1
  done
  [ -n "$MAIN" ] || { echo "no main window ($mode)"; tail -5 "$work/mdm.log"; continue; }
  # MDM_MAIN_AT="x,y": the main window moved there (clear of a desktop's lasting notification).
  [ -n "${MDM_MAIN_AT:-}" ] && wmctrl -i -r "$MAIN" -e "0,$MDM_MAIN_AT,-1,-1"
  sleep 3
  # A list that looks used: finished files, one still coming in (rate-limited per connection).
  for f in big-buck-bunny-1080p.mp4:180M project-report-2026.pdf:4M holiday-photos.zip:64M \
           podcast-episode-42.mp3:38M rust-1.99.1-x86_64-unknown-linux-gnu.tar.xz:92M \
           open-data-2026.csv:12M "ubuntu-26.04-desktop-amd64.iso:6G:?rate=256K"; do
    IFS=: read -r name size q <<< "$f"
    "$app" /d "$base/gen/$size/$name${q:-}" /p "$work/Downloads" /n >/dev/null 2>&1
    sleep 0.5
  done
  # The small ones finish; their windows (Download complete, progress) close, never the main one.
  for _ in $(seq 20); do sleep 1; close_dialogs; done
  sleep 3
  echo "$mode:"
  want main && shoot "$MAIN" main
  for pair in add-url=addurl progress=progress options=options:connection scheduler=scheduler browser-offer=extension; do
    name=${pair%%=*}; show=${pair#*=}
    want "$name" || continue
    if w=$(opened "$app" /show "$show"); then sleep 2; shoot "$w" "$name"; else echo "  $name: no window"; fi
    close_dialogs
  done
  if want add-torrent && [ -f "$torrent" ]; then
    cp "$torrent" "$work/Open Movie Night.torrent"
    if w=$(opened "$app" "$work/Open Movie Night.torrent"); then sleep 2; shoot "$w" add-torrent; else echo "  add-torrent: no window"; fi
    close_dialogs
  fi
  # The tray: MDM's icon is the leftmost of the top bar's status icons that appeared with it. Its
  # menu opens with a click; the shot is the screen's top right (MDM_TRAY_W x MDM_TRAY_H), with
  # MDM's window hidden to the tray first, so only the desktop is behind the menu.
  if want tray && [ "$mode" = light ]; then
    tray=${MDM_TRAY_AT:-}
    if [ -n "$tray" ]; then
      # Banners back (GNOME shows a "do not disturb" bell while they're off), then a moment.
      [ -n "$banners" ] && gsettings set org.gnome.desktop.notifications show-banners "$banners" 2>/dev/null
      "$app" /show tray >/dev/null 2>&1
      sleep 3
      xdotool mousemove ${tray% *} ${tray#* } click 1; sleep 2
      grab $((SW - ${MDM_TRAY_W:-320})) 0 ${MDM_TRAY_W:-320} ${MDM_TRAY_H:-490} "$dir/tray.png"
      xdotool key Escape; park
    else
      echo "  tray: MDM_TRAY_AT=\"x y\" (the icon's place in the top bar) not set"
      import -silent -window root -crop "${SW}x40+0+0" +repage "$dir/topbar.png"
    fi
  fi
  # Ended as a person would (/exit, IDM's switch): a signal reads as a crash to its monitor.
  timeout 20 "$app" /exit >/dev/null 2>&1
  for _ in $(seq 20); do pgrep -f "^$app$" >/dev/null || break; sleep 0.5; done
  pkill -f "^$app$"
done
pkill -f "$bin/labserver"
[ -n "$banners" ] && gsettings set org.gnome.desktop.notifications show-banners "$banners" 2>/dev/null
desktop_mode light
# The downloads go; the last run's log stays (mdm.log) until the next run.
rm -rf "$run/Downloads"
echo "shots in $out"

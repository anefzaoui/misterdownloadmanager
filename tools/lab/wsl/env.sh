# Sourced by every mdm-lab script that starts a program with a window: it opens on the lab's
# own virtual screen (Xvfb :99, screen.sh), never on Windows.
#
# WSLg would show a Linux window on the Windows desktop (Wayland at WAYLAND_DISPLAY, X11 at :0)
# and play its sound there (PULSE_SERVER), and it can't be turned off for one distribution
# (guiApplications is a global .wslconfig key, left alone). So its variables go, the display is
# :99, and the runtime folder (where Wayland and PulseAudio sockets are looked for) is the
# lab's own, never WSLg's /mnt/wslg/runtime-dir.
unset WAYLAND_DISPLAY WAYLAND_SOCKET PULSE_SERVER PULSE_RUNTIME_PATH
export DISPLAY=:99
export XDG_RUNTIME_DIR=$HOME/lab/xdg-run
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
export XDG_SESSION_TYPE=x11
# The session bus screen.sh starts for the virtual screen (GTK's dialogs want one).
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
# Software OpenGL (Mesa's llvmpipe), as on a machine without a GPU: the same pixels every run,
# and nothing goes through WSL's GPU bridge to Windows.
export LIBGL_ALWAYS_SOFTWARE=1 GALLIUM_DRIVER=llvmpipe
# A browser or file manager MDM asks for (xdg-open) has nowhere to go: interop is off and none
# is installed; BROWSER says so plainly.
export BROWSER=/bin/false
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# The desktop session's display, bus and runtime folder, for scripts started from outside it
# (VBoxManage guestcontrol): GUI programs open on the lab's screen, gsettings and D-Bus reach it.
export DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
export XAUTHORITY=/run/user/$(id -u)/gdm/Xauthority
[ -f ~/.cargo/env ] && . ~/.cargo/env

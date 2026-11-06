#!/bin/bash
# Another desktop beside GNOME in a lab clone (vm.ps1 clone), GDM kept as the login manager and the
# auto-login session switched to the new desktop, for MDM's checks on KDE Plasma, Cinnamon, XFCE.
# Reboot the clone afterwards. The sudo password comes on stdin (vm.ps1 run passes MDM_SUDO_PASSWORD):
#   echo "$MDM_SUDO_PASSWORD" | bash desktop.sh "<packages>" <session>
#   e.g. "kde-plasma-desktop xdg-desktop-portal-kde" plasma · cinnamon-core cinnamon ·
#        "xfce4 xfce4-goodies xdg-desktop-portal-gtk" xfce
# Plasma: its screen locker is turned off afterwards (kwriteconfig5 --file kscreenlockerrc --group
# Daemon --key Autolock false), as GNOME's is in setup.sh.
read -r PW
S() { echo "$PW" | sudo -S -p '' "$@"; }
pkgs=$1; session=$2
S true || exit 1
# GDM stays the display manager (a desktop's own, sddm or lightdm, would ask).
for dm in gdm3 lightdm sddm; do echo "$dm shared/default-x-display-manager select gdm3" | S debconf-set-selections; done
S env DEBIAN_FRONTEND=noninteractive apt-get update -q >/dev/null
S env DEBIAN_FRONTEND=noninteractive apt-get install -y -q $pkgs 2>&1 | tail -3
ls /usr/share/xsessions/
# AccountsService names the session GDM logs into; XSession for an X11 one.
S sed -i -e "s/^Session=.*/Session=$session/" -e "/^XSession=/d" /var/lib/AccountsService/users/"$USER"
S sed -i "s/^Session=$session/Session=$session\nXSession=$session/" /var/lib/AccountsService/users/"$USER"
S head -3 /var/lib/AccountsService/users/"$USER"

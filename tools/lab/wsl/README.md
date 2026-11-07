# mdm-lab: Linux builds and window shots on WSL2

`mdm-lab` is a WSL2 distribution (Ubuntu 24.04) of its own. It builds MDM for Linux at the PC's
full speed and screenshots its windows on a virtual screen (Xvfb) that never shows on Windows.
The VirtualBox machines in [`../linux-vm/`](../linux-vm/) run on top of Hyper-V here (VirtualBox's
NEM mode), about three times slower: builds there took 15 to 60 minutes and a shot about 40 s.
Here a build of main takes under 5 minutes from scratch and about 35 s after a one-line change,
and a window shot about 1.2 s. This lab takes over those two jobs, and `deploy.ps1` hands its
builds to the VMs for the checks only a real desktop can do.

## Setting it up

From PowerShell on the Windows host, once:

```powershell
wsl --install -d Ubuntu-24.04 --name mdm-lab --location C:\mdm-wsl --no-launch
wsl -d mdm-lab -u root -- bash /mnt/d/Projects/misterdownloadmanager/tools/lab/wsl/setup.sh
wsl --terminate mdm-lab     # its wsl.conf applies at the next start
```

`setup.sh` (as root) writes the distribution's `/etc/wsl.conf` (systemd, the `lab` user, and no
interop: nothing in mdm-lab can start a Windows program), installs the build's libraries (those of
`ci.yml` and the VMs), Xvfb, Metacity, ImageMagick, xwd, xdotool, wmctrl, Mesa's software OpenGL
and Vulkan (llvmpipe, lavapipe), ffmpeg, 7-Zip and an Ubuntu desktop's fonts with every script
MDM's languages need, then, as `lab`: Rust, the VMs' `vmtest`
cargo profile, the checkout in `~/src/mdm` (the distribution's own ext4 disk: building from
`/mnt/c` or `/mnt/d` goes through 9P and is many times slower) and the lab scripts in
`~/lab/tools/`. Running it again only adds what's missing and refreshes the scripts.

After a change to these scripts, the same `setup.sh` line copies them in again (a minute: the
packages are there already).

From Git Bash rather than PowerShell, prefix `MSYS_NO_PATHCONV=1` (or Git Bash rewrites `/mnt/...`
and `~/...` paths) and use `wsl -d mdm-lab --exec bash -c '...'` when the command has `$VARIABLES`:
without `--exec`, `wsl.exe` hands the line to a shell that expands them first.

It changes nothing outside mdm-lab: never `%USERPROFILE%\.wslconfig` and never another
distribution. Never run `wsl --shutdown` for it (that stops every distribution): `wsl --terminate
mdm-lab` stops this one.

## Building

```powershell
wsl -d mdm-lab -- bash ~/lab/tools/build.sh main                 # ~/lab/bin/main/
wsl -d mdm-lab -- bash ~/lab/tools/build.sh pr/167 pr-167        # a pull request, labelled
wsl -d mdm-lab -- bash ~/lab/tools/build.sh --release main       # fat LTO, what ships: main-release
```

A branch, `pr/<number>` or a commit is fetched and checked out (detached), built with the
`vmtest` profile (release without LTO, incremental) on every core, and its four programs
(`misterdownloadmanager`, `-nmhost`, `-cli`, `labserver`) copied to `~/lab/bin/<label>/` with a
`COMMIT` file. The target folder stays, so the next build recompiles only what changed. The log
is `~/lab/build.log`. Builds take turns (a lock): every caller shares the one checkout. From Windows the builds are at `\\wsl$\mdm-lab\home\lab\lab\bin\`.

## Window shots

```powershell
wsl -d mdm-lab -- bash ~/lab/tools/shots.sh main --langs en,de,ar,ja,hi
wsl -d mdm-lab -- bash ~/lab/tools/shots.sh pr-199 --modes light,dark --theme adwaita `
    --windows main,options-general=options:general,about
```

One MDM per language and mode: each window opens through MDM's own `/show <window>` command line,
is shot once it has painted (`capture.py`: two reads of it in a row the same, at most 2 s for a
window that never settles, such as progress), with its title bar and borders, is closed from its
title bar, then the next. No restart per shot. The default list is 21 windows: the main window and the 20
dialogs of `tools/lab/docs-shots.ps1` (`<file name>=<show argument>` when they differ). Lists
take commas: `wsl.exe` hands its command line to a shell again, which splits quoted spaces. A
`menu:...` show is shot with the main window, grown by `MDM_GROW` pixels (default 300).

Each run has its own data folder (`MDM_DATA_DIR`), download folder and lab server (port 8197),
under `~/lab/run/<label>`; the list is downloaded once, for the first language, and given to the
others. `--finished-only` leaves out the download still running (shots that must not change
between runs); `--keep-list` reuses the label's last list (the same names and dates: two runs
compared pixel by pixel). Shots land in `~/lab/shots/<label>/<lang>-<mode>/` with `timing.txt`.
`MDM_SET` adds settings (a JSON object), as in `../linux-vm/win.sh`; other `MDM_*` variables
(`MDM_RENDERER`, `MDM_OFFSCREEN`, ...) reach MDM.

`renderers.sh <label>` compares OpenGL and Vulkan (lavapipe) on nine windows, pixel by pixel
(`pixdiff.py` from `../linux-vm/`), for builds with View → Draw with → Vulkan: forced, as the
saved choice (OpenGL, saying why, with no hardware driver) and with `MDM_TEST_SOFTWARE_CARD=1`.

`screen.sh start|stop|status` runs the virtual screen by hand (`shots.sh` starts it itself):
Xvfb `:99` at 1600x1000, a session bus, and Metacity (GNOME's window manager, without a
compositor) for title bars and borders. For anything else on that screen:

```bash
. ~/lab/tools/env.sh && bash ~/lab/tools/screen.sh start
~/lab/bin/main/misterdownloadmanager &        # on :99, never on Windows
import -window root /tmp/screen.png        # or capture.py <window id> <out.png>
bash ~/lab/tools/screen.sh stop               # MDM, the lab server and the screen ended
```

### Why nothing shows on Windows

WSLg shows Linux windows on the Windows desktop and plays their sound there, and it can only be
turned off for every distribution at once (`guiApplications` in `.wslconfig`), which the lab leaves
alone. So `env.sh`, which every lab script sources, removes `WAYLAND_DISPLAY` and `PULSE_SERVER`,
points `DISPLAY` at `:99` and `XDG_RUNTIME_DIR` at the lab's own folder (WSLg's Wayland and
PulseAudio sockets are in its own), and draws with software OpenGL (llvmpipe). With interop off,
`xdg-open` can't reach a Windows browser either.

## Into a VM

```powershell
tools\lab\wsl\deploy.ps1 main mdm-ubuntu            # mdm-lab's ~/lab/bin/main -> the VM's
tools\lab\wsl\deploy.ps1 pr-167 mdm-kde -DryRun     # what it would copy (no VBoxManage call)
```

The programs are read from `\\wsl$\mdm-lab\...` and copied with `VBoxManage guestcontrol copyto`
into a folder beside `~/lab/bin/<label>/`, then swapped in whole. The VM must be running. Its
account comes from `MDM_VM_USER` / `MDM_VM_PASSWORD` or the git-ignored `k\vm-ubuntu.txt`. The VM's
own harnesses (`win.sh`, `shots.sh`, `streams.mjs`, ...) then run that build as one of theirs.

## What mdm-lab can't replace

Builds and window shots, yes; a desktop, no. These still need the VMs in `../linux-vm/`:

- **The desktop's portals and settings**: the GNOME and KDE file choosers (xdg-desktop-portal),
  the system's dark mode, accent and contrast as MDM reads them, notifications.
- **The tray**: there is no StatusNotifier host or AppIndicator on Xvfb, so the tray icon, its
  menu and Hide to the notification area can't be checked here.
- **Real browsers and native messaging**: the Snap Firefox, Chromium and Google Chrome with the
  extension handing downloads and streams to MDM (`streams.mjs`, `store-check.mjs`, ...).
- **Desktop-specific behaviour**: GNOME Shell's, KWin's, Cinnamon's or XFCE's own window
  placement, focus, frames and Wayland sessions; AT-SPI and screen readers; real hardware OpenGL.
  Metacity's frames look like GNOME's but are not GNOME Shell's.
- **Sound** and anything else that leaves the machine through the desktop.

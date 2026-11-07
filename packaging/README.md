# Packages for package managers

MDM in WinGet, the AUR and the Microsoft Store, **coming with 0.3.0**: not listed yet.

| Folder | What |
| --- | --- |
| [`winget/`](winget/) | WinGet manifests, in winget-pkgs' layout |
| [`aur/`](aur/) | The AUR package `misterdownloadmanager-bin`, and `update.sh` that moves it to a release |
| [`msstore/`](msstore/) | The Microsoft Store listing's texts |

Every pull request that changes this folder is checked by
[`package-managers.yml`](../.github/workflows/package-managers.yml): `winget validate` on the
manifests, and the AUR package built from its release on Arch and checked with `namcap`.

## Once listed

| Where | How |
| --- | --- |
| WinGet | `winget install AhmedNefzaoui.MisterDownloadManager` (for everyone: `--scope machine`) |
| AUR | `yay -S misterdownloadmanager-bin`, or any AUR helper |

## WinGet

- The package is the release's Inno Setup installer, for one user (`%LOCALAPPDATA%\Programs`, no
  administrator) or for everyone (Program Files). It installs with the installer's default
  tasks: browser integration, `.torrent` files and magnet links, start with Windows.
- Upgrades find the installed copy by the installer's uninstall entry,
  `{667DB4D8-32C2-4487-A0E5-5825BF29778C}_is1`, so a copy installed from the download page is
  upgraded in place too.
- It depends on the Visual C++ runtime (`Microsoft.VCRedist.2015+.x64`), which winget installs
  first when it's missing.
- No portable package: winget deletes a portable package's folder on uninstall and upgrade, which
  would take the portable copy's download list and settings with it.

## AUR

- `misterdownloadmanager-bin` is the release's `.deb`, unpacked: the same programs, desktop entry,
  icon and browser extension as the `.deb` and `.rpm`. Nothing is compiled.
- The programs link only `glibc` and `libgcc`; the window loads the rest at run time (OpenGL,
  Wayland, X11, the keyboard, D-Bus), so those are its dependencies even though `namcap` can't see
  them. Optional: a portal or zenity for file pickers, `xdg-utils`, a Secret Service keyring,
  Vulkan, and the system's `yt-dlp` and `ffmpeg`.
- Browser integration works as from the `.deb`: MDM registers its browser host for the browsers
  it finds, in your home folder, each time it starts.

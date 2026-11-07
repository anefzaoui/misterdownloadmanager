# Update now, end to end

Checks **Help → Check for updates → Update now** the way a user's update goes. The scripts:

1. find a newer release;
2. download it and check its signed checksum list;
3. show **Installing the update...**;
4. put it in place;
5. start the new copy (`/updated`) while the old one closes.

They also check that MDM refuses a release whose checksum is wrong or whose list is signed by another key, and that closing the window half-way starts nothing.

Nothing here touches the installed MDM or the real releases:

- **The test copies.** Each run starts a copy of the given build in a folder of its own, off-screen (`MDM_OFFSCREEN=1`).
- **The releases.** They are fake 9.9.9 ones made by `mkrel.py`, served by the lab server on 127.0.0.1 (`--files`). Their `latest.json` is the release feed (`MDM_UPDATE_FEED`).
- **The keys.** The checksum lists are signed with two throwaway Ed25519 keys made for each run in its work folder: "test" and "other". MDM's release key is never read or needed.

## The build

These checks need the **`test-update`** cargo feature. It is off by default, and no release build turns it on. It adds:

- `MDM_TEST_UPDATE_KEY`: a public key the release's checksum list may be signed with, besides MDM's own.
- `/show update-now`: Update now pressed, with no clicks, in the window `/show updates` opens (that one every build has).
- `MDM_TEST_UPDATE_HOLD=N`: holds "Installing the update..." for N seconds.

The build must be release mode. A debug build is "built from source", and those never update.

```powershell
# Windows
cargo build --release -p mdm-app --features test-update -p mdm-labserver
```
```bash
# Linux (mdm-lab: the vmtest profile)
cargo build --profile vmtest -p mdm-app --features test-update -p mdm-labserver
```

## Windows: the portable zip

```powershell
tools\lab\update\upd-win.ps1 -Exe target\release\misterdownloadmanager.exe -Server target\release\labserver.exe
tools\lab\update\upd-win.ps1 ... -Only good,bad-hash        # some scenarios
tools\lab\update\upd-win.ps1 ... -Shots -ShotDir <folder>   # plus screenshots, below
```

Needs Python 3 and gpg (Git for Windows has one). The work folder is `target\update-e2e`, emptied first: the copies, releases, keys and `log.txt` are there.

Each copy is portable (`data\` beside its program), so `Install::PortableZip`. The windows open off the screens and nothing is clicked: closing a window is its title bar's `WM_CLOSE`.

`-Shots` also captures Download complete and the "Installing the update..." state in English, German and Arabic, light and dark, with PrintWindow (`tools\lab\cap-own.ps1`).

## Linux: the .tar.gz and the AppImage

```bash
tools/lab/update/upd-lin.sh ~/src/mdm/target/vmtest                     # every scenario
tools/lab/update/upd-lin.sh ~/src/mdm/target/vmtest targz-good          # one
```

Needs Xvfb, dbus-daemon, Metacity (wmctrl needs a window manager), wmctrl, Python 3 and gpg. mdm-lab has them all.

The script runs on a virtual screen of its own (`:96`), never the desktop's or WSLg's. Its work folder is `$MDM_UPDATE_E2E_WORK`, else `/tmp/mdm-update-e2e`.

- A copy run from its folder is `Install::TarGz`.
- With `APPIMAGE` pointing at the copy, it is `Install::AppImage`. The fake AppImage is the same program with a marker appended, so it can be told apart once replaced.

macOS (the .dmg: `hdiutil`, `ditto`) has no runner yet: build and clippy only.

## Scenarios and what each must show

Each runner prints `PASS` or `FAIL` per scenario and exits with 1 when any fails.

| Windows | Linux | Must show |
|---|---|---|
| `good` | `targz-good`, `appimage-good` | downloaded, checked, put in place; the new copy started with `/updated`; the old one ended |
| `bad-hash` | `targz-bad-hash` | refused ("the downloaded program doesn't match its signed checksum"); nothing replaced, nothing started |
| `other-key` | `appimage-other-key` | refused ("the release's checksum list is not signed by its publisher's key"); nothing replaced or started |
| `close-download` | `targz-close-download` | the update window closed while it downloads (the asset served at 1 MB/s by `slowserve.py`); nothing replaced or started |
| `close-install` | `appimage-close-install` | the window closed during "Installing the update..." (held 8 s): the files are put in place, since that can't stop half-way, but nothing is started; the running copy stays and the next start is the new version |

## The files

- `mkrel.py`: makes a fake release (the asset, `SHA256SUMS.txt` and its signature, `latest.json`). On its first use in a work folder it also makes the throwaway keys there.
- `slowserve.py`: serves a folder slowly (the cancel-while-downloading scenario).
- `upd-win.ps1`, `upd-lin.sh`: the runners.

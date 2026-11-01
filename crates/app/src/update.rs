//! Updating in place (Help → Check for updates → Update now): this copy's release file, fetched
//! and checked in the background (`mdm_tools::update`: the release's signed checksum list), then
//! put where this copy is. MDM closes for it; the new copy starts with `/updated`, waits for the
//! old one to be gone, and removes what the update set aside.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdm_tools::update::{Install, Progress};

/// The switch the updated copy starts with.
pub const UPDATED: &str = "/updated";

/// What follows once the file is in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// MDM closes; the new copy is starting (or the installer is running and starts it).
    Restart,
    /// Handed to the system's package installer (a .deb or .rpm): MDM stays open.
    Opened,
}

/// The release file this copy updates from, when `assets` (name, address) has it and the
/// signature of the release's checksum list.
pub fn asset_for(install: Install, version: &str, assets: &[(String, String)]) -> Option<String> {
    let name = install.asset(version)?;
    let has = |n: &str| assets.iter().any(|(a, _)| a == n);
    (has(&name) && has("SHA256SUMS.txt") && has("SHA256SUMS.txt.sig")).then_some(name)
}

/// Where update files are kept until the next start.
fn folder() -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join("updates"))
}

/// A release file on its way: its progress, then the checked file or why not. Dropped
/// (Cancel, the window closed), it stops.
pub struct Job {
    pub progress: Arc<Progress>,
    slot: Arc<Mutex<Option<Result<PathBuf, String>>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Job {
    pub fn start(
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
        assets: Vec<(String, String)>,
        name: String,
    ) -> Job {
        let progress = Arc::new(Progress::default());
        let slot = Arc::new(Mutex::new(None));
        let (p, out, ctx) = (progress.clone(), slot.clone(), ctx.clone());
        // Repaint while it runs, for the progress bar.
        let ticker = ctx.clone();
        let task = rt.spawn(async move {
            let res = match folder() {
                Some(dir) => mdm_tools::update::fetch(&assets, &name, &dir, &p)
                    .await
                    .map_err(|e| e.to_string()),
                None => Err("no data folder".into()),
            };
            *out.lock().unwrap() = Some(res);
            ctx.request_repaint();
        });
        ticker.request_repaint_after(Duration::from_millis(200));
        Job {
            progress,
            slot,
            task,
        }
    }

    /// The checked file, or why not, once the download ended.
    pub fn result(&self) -> Option<Result<PathBuf, String>> {
        self.slot.lock().unwrap().take()
    }
}

/// Put `file` (checked) in place of this `install`.
pub fn apply(install: Install, file: &Path) -> Result<Next, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no program folder")?.to_owned();
    let unpack_dir = || -> Result<PathBuf, String> {
        let d = folder().ok_or("no data folder")?.join("unpacked");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        Ok(d)
    };
    let restart = |program: &Path| -> Result<Next, String> {
        Command::new(program)
            .arg(UPDATED)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Next::Restart)
    };
    match install {
        // The installer closes MDM itself (/exit), installs where it was (the same install
        // mode), and starts it again (/relaunch, tools/installer/mdm.iss).
        Install::Installer => {
            Command::new(file)
                .args([
                    "/VERYSILENT",
                    "/SUPPRESSMSGBOXES",
                    "/NORESTART",
                    "/relaunch=1",
                ])
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(Next::Restart)
        }
        Install::PortableZip | Install::PlainZip => {
            let out = unpack_dir()?;
            mdm_tools::update::unzip(file, &out).map_err(|e| e.to_string())?;
            let new = mdm_tools::update::single_folder(&out);
            // A portable copy's list and settings stay where they are.
            mdm_tools::update::replace_files(&new, &dir, &["data"]).map_err(|e| e.to_string())?;
            restart(&exe)
        }
        Install::TarGz => {
            let out = unpack_dir()?;
            let ok = Command::new("tar")
                .arg("-xzf")
                .arg(file)
                .arg("-C")
                .arg(&out)
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                return Err("couldn't unpack the update".into());
            }
            let new = mdm_tools::update::single_folder(&out);
            mdm_tools::update::replace_files(&new, &dir, &[]).map_err(|e| e.to_string())?;
            restart(&exe)
        }
        Install::AppImage => {
            let target = std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .ok_or("not running as an AppImage")?;
            let staged = target.with_extension("AppImage.new");
            std::fs::copy(file, &staged).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            std::fs::rename(&staged, &target).map_err(|e| e.to_string())?;
            restart(&target)
        }
        Install::Dmg => {
            let bundle = mdm_tools::update::app_bundle(&exe).ok_or("not in an app bundle")?;
            let mount = folder().ok_or("no data folder")?.join("mount");
            std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
            let run = |c: &mut Command| c.status().is_ok_and(|s| s.success());
            if !run(Command::new("hdiutil")
                .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
                .arg(&mount)
                .arg(file))
            {
                return Err("couldn't open the update's disk image".into());
            }
            let name = bundle.file_name().ok_or("no app name")?.to_owned();
            let staged = bundle.with_extension("app.new");
            let _ = std::fs::remove_dir_all(&staged);
            let copied = run(Command::new("ditto").arg(mount.join(&name)).arg(&staged));
            run(Command::new("hdiutil").arg("detach").arg(&mount));
            if !copied {
                return Err("couldn't copy the new app".into());
            }
            // The running app is renamed aside (removed on the next start), the new one takes
            // its place.
            let old = bundle.with_extension("app.old");
            let _ = std::fs::remove_dir_all(&old);
            std::fs::rename(&bundle, &old).map_err(|e| e.to_string())?;
            std::fs::rename(&staged, &bundle).map_err(|e| e.to_string())?;
            Command::new("open")
                .arg("-n")
                .arg(&bundle)
                .args(["--args", UPDATED])
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(Next::Restart)
        }
        // Installing a package needs the administrator: the system's installer asks.
        Install::Deb | Install::Rpm => {
            open::that_detached(file).map_err(|e| e.to_string())?;
            Ok(Next::Opened)
        }
        Install::Source => Err("built from source: update it from the repository".into()),
    }
}

/// The single-instance lock for a copy started by an update (`/updated`): the old copy may
/// still be saving and closing, so it is waited for (up to 20 s).
pub fn lock_after_update() -> Option<std::fs::File> {
    let start = Instant::now();
    loop {
        if let Some(lock) = mdm_ipc::instance_lock() {
            return Some(lock);
        }
        if start.elapsed() > Duration::from_secs(20) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// At start, once this is the running copy: what an update set aside (the old programs, a
/// Mac's old app) and its downloaded files are removed.
pub fn tidy() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if let Some(dir) = exe.parent() {
        mdm_tools::update::remove_old(dir);
    }
    if let Some(bundle) = mdm_tools::update::app_bundle(&exe) {
        let _ = std::fs::remove_dir_all(bundle.with_extension("app.old"));
    }
    if let Some(dir) = folder() {
        // A running installer (it starts the new copy before it ends) keeps its file: the next
        // start removes it.
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_signed_release_with_this_copys_file_updates_it() {
        let a = |names: &[&str]| -> Vec<(String, String)> {
            names
                .iter()
                .map(|n| (n.to_string(), format!("https://x/{n}")))
                .collect()
        };
        let full = a(&[
            "MisterDownloadManager-0.2.2-setup.exe",
            "SHA256SUMS.txt",
            "SHA256SUMS.txt.sig",
        ]);
        assert_eq!(
            asset_for(Install::Installer, "0.2.2", &full).as_deref(),
            Some("MisterDownloadManager-0.2.2-setup.exe")
        );
        // An unsigned release, another kind's file, a build from source: no.
        let unsigned = a(&["MisterDownloadManager-0.2.2-setup.exe", "SHA256SUMS.txt"]);
        assert_eq!(asset_for(Install::Installer, "0.2.2", &unsigned), None);
        assert_eq!(asset_for(Install::PortableZip, "0.2.2", &full), None);
        assert_eq!(asset_for(Install::Source, "0.2.2", &full), None);
    }
}

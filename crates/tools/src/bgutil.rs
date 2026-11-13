//! Detection, installation and status for the bgutil YouTube Proof-of-Origin Token (POT) provider
//! ([Brainicism/bgutil-ytdlp-pot-provider] and its Rust fork [jim60105/bgutil-ytdlp-pot-provider-rs]).
//!
//! YouTube uses proof-of-origin tokens to protect videos against automated / bot traffic
//! ("Sign in to confirm you're not a bot"). yt-dlp delegates token generation to a POT provider
//! plugin, which either calls a local HTTP server on `127.0.0.1:4416` or a standalone CLI binary
//! (`bgutil-pot` or `bgutil`).
//!
//! If the user already has a provider (system executable or HTTP server), MDM detects and uses it.
//! If not, MDM can install the lightweight, standalone Rust release (`bgutil-pot` binary + plugin zip)
//! from `jim60105/bgutil-ytdlp-pot-provider-rs`.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::{Error, Installed, make_current, mark_downloaded};

/// The default port for the bgutil POT HTTP provider server.
pub const DEFAULT_PORT: u16 = 4416;

/// GitHub releases endpoint for the standalone Rust implementation of the POT provider.
const RELEASES: &str =
    "https://api.github.com/repos/jim60105/bgutil-ytdlp-pot-provider-rs/releases/latest";

/// Overall detection and readiness status for the bgutil POT provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// Whether a POT HTTP server is currently running and answering on `127.0.0.1:4416`.
    pub server_reachable: bool,
    /// An executable found on the system or installed by MDM (`bgutil-pot` or `bgutil`).
    pub provider_exe: Option<Installed>,
    /// Location of the yt-dlp plugin for bgutil, if found.
    pub plugin_path: Option<PathBuf>,
}

impl Status {
    /// Whether any bgutil POT provider component is present (server running, CLI executable,
    /// or installed yt-dlp plugin).
    pub fn is_detected(&self) -> bool {
        self.server_reachable || self.provider_exe.is_some() || self.plugin_path.is_some()
    }

    /// Whether bgutil POT provider is ready to supply tokens to yt-dlp.
    pub fn is_ready(&self) -> bool {
        self.server_reachable || self.provider_exe.is_some()
    }
}

/// The standalone binary name for this target architecture in `jim60105` releases.
pub fn asset() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("bgutil-pot-windows-x86_64.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("bgutil-pot-linux-x86_64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("bgutil-pot-linux-aarch64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("bgutil-pot-macos-aarch64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("bgutil-pot-macos-x86_64")
    } else {
        None
    }
}

/// The plugin zip archive name in releases.
pub const PLUGIN_ZIP_ASSET: &str = "bgutil-ytdlp-pot-provider-rs.zip";

/// Check if a POT HTTP provider server is listening on `127.0.0.1:4416`.
pub fn server_reachable() -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));
    TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

/// The bgutil provider executable to use: MDM's managed install in `dir`, else the system's.
pub fn installed(dir: &Path) -> Option<Installed> {
    own(dir).or_else(find_system_executable)
}

/// MDM's own managed install in `dir`.
fn own(dir: &Path) -> Option<Installed> {
    let text = std::fs::read(dir.join("bgutil").join("current.json")).ok()?;
    let i: Installed = serde_json::from_slice(&text).ok()?;
    i.exe.is_file().then_some(i)
}

/// Find a bgutil POT provider executable on the system (`bgutil-pot`, `bgutil-pot-server`, or `bgutil`).
pub fn find_system_executable() -> Option<Installed> {
    for name in ["bgutil-pot", "bgutil-pot-server", "bgutil"] {
        if let Some(inst) = crate::system::installed(name) {
            return Some(inst);
        }
    }
    None
}

/// Check candidate plugin directories for the bgutil plugin.
pub fn find_plugin(dir: Option<&Path>) -> Option<PathBuf> {
    for path in plugin_dirs(dir) {
        if let Some(found) = check_dir_for_bgutil(&path) {
            return Some(found);
        }
    }
    None
}

/// Return all detection status at once.
pub fn status(dir: Option<&Path>) -> Status {
    Status {
        server_reachable: server_reachable(),
        provider_exe: dir.and_then(installed).or_else(find_system_executable),
        plugin_path: find_plugin(dir),
    }
}

/// Standard directories where yt-dlp searches for plugins, plus MDM's managed plugins folder.
pub fn plugin_dirs(dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(d) = dir {
        dirs.push(d.join("plugins"));
        dirs.push(d.join("yt-dlp-plugins"));
    }

    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(appdata.join(r"yt-dlp\plugins"));
            dirs.push(appdata.join(r"yt-dlp-plugins"));
        }
        if let Some(user) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            dirs.push(user.join(r"yt-dlp-plugins"));
            dirs.push(user.join(r".yt-dlp\plugins"));
        }
    }

    #[cfg(unix)]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            dirs.push(home.join(".config/yt-dlp/plugins"));
            dirs.push(home.join(".config/yt-dlp-plugins"));
            dirs.push(home.join(".yt-dlp/plugins"));
            dirs.push(home.join("yt-dlp-plugins"));
        }
        dirs.push(PathBuf::from("/etc/yt-dlp/plugins"));
        dirs.push(PathBuf::from("/etc/yt-dlp-plugins"));
    }

    dirs
}

fn check_dir_for_bgutil(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if name.contains("bgutil") || name.contains("pot-provider") {
            return Some(path);
        }
    }
    None
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Install the latest standalone Rust POT provider (`bgutil-pot`) and plugin into `dir`
/// unless already installed, downloading via MDM's engine (`net`).
pub async fn install_latest(
    dir: &Path,
    net: &mdm_engine::Options,
    _progress: Option<&crate::update::Progress>,
) -> Result<Installed, Error> {
    let Some(bin_asset) = asset() else {
        return Err(Error::Missing("Rust bgutil build for this target".into()));
    };

    let client = crate::client(net, RELEASES)?;
    let body = crate::get_bytes(&client, RELEASES).await?;
    let release: Release =
        serde_json::from_slice(&body).map_err(|e| Error::BadRelease(e.to_string()))?;
    let version = release.tag_name.trim_start_matches('v').to_owned();

    let find_asset_url = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
            .ok_or_else(|| Error::Missing(name.to_owned()))
    };

    let bin_url = find_asset_url(bin_asset)?;
    let plugin_zip_url = find_asset_url(PLUGIN_ZIP_ASSET).ok();

    let root = dir.join("bgutil");
    std::fs::create_dir_all(&root)?;

    // Download the binary executable
    let exe_name = crate::system::program_name("bgutil-pot");
    let target_dir = root.join(&version);
    std::fs::create_dir_all(&target_dir)?;
    let target_exe = target_dir.join(&exe_name);

    let temp_exe = root.join(format!("bgutil-{}.download", version));
    // Fetch binary
    let bytes = crate::get_bytes(&client, &bin_url).await?;
    std::fs::write(&target_exe, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&target_exe, std::fs::Permissions::from_mode(0o755));
    }
    mark_downloaded(&target_exe, &bin_url);
    let _ = std::fs::remove_file(&temp_exe);

    // Download and extract plugin zip if available
    if let Some(zip_url) = plugin_zip_url
        && let Ok(zip_bytes) = crate::get_bytes(&client, &zip_url).await
    {
        let plugins_target = dir.join("plugins");
        let _ = unpack_plugin_zip(&zip_bytes, &plugins_target);
    }

    make_current(
        &root,
        Installed {
            version,
            exe: target_exe,
        },
    )
}

/// Unpack the bgutil plugin zip into `dest_dir`.
fn unpack_plugin_zip(bytes: &[u8], dest_dir: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest_dir)?;
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| Error::BadRelease(e.to_string()))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| Error::BadRelease(e.to_string()))?;
        let Some(enclosed) = file.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let outpath = dest_dir.join(enclosed);
        if file.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plugin_in_directory() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = temp.path().join("bgutil-ytdlp-pot-provider");
        std::fs::create_dir(&plugin).unwrap();
        assert_eq!(check_dir_for_bgutil(temp.path()), Some(plugin));
    }

    #[test]
    fn detects_plugin_zip_in_directory() {
        let temp = tempfile::tempdir().unwrap();
        let zip = temp.path().join("bgutil-ytdlp-pot-provider-rs.zip");
        std::fs::write(&zip, b"PK").unwrap();
        assert_eq!(check_dir_for_bgutil(temp.path()), Some(zip));
    }

    #[test]
    fn ignores_unrelated_directories() {
        let temp = tempfile::tempdir().unwrap();
        let other = temp.path().join("some-other-plugin");
        std::fs::create_dir(&other).unwrap();
        assert_eq!(check_dir_for_bgutil(temp.path()), None);
    }

    #[test]
    fn asset_name_for_supported_platforms() {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(asset(), Some("bgutil-pot-windows-x86_64.exe"));
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(asset(), Some("bgutil-pot-linux-x86_64"));
    }

    #[test]
    fn unzips_plugin_archive() {
        let temp = tempfile::tempdir().unwrap();
        let zip_path = temp.path().join("test.zip");
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("bgutil-plugin/manifest.json", opts).unwrap();
            w.write_all(b"{\"name\": \"bgutil\"}").unwrap();
            w.finish().unwrap();
        }
        let bytes = std::fs::read(&zip_path).unwrap();
        let out_dir = temp.path().join("extracted");
        unpack_plugin_zip(&bytes, &out_dir).unwrap();
        assert!(out_dir.join("bgutil-plugin/manifest.json").is_file());
    }
}

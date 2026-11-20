//! Deno, the JavaScript runtime yt-dlp solves YouTube's formats with (yt-dlp's EJS guide,
//! <https://github.com/yt-dlp/yt-dlp/wiki/EJS>): it is the only runtime yt-dlp enables by itself,
//! and YouTube lists only the sound without one. MDM downloads it from its official releases, as
//! it does yt-dlp and ffmpeg, so nothing has to be installed by hand.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Error, Installed, make_current, mark_downloaded, newer};

/// Deno's newest release, read as yt-dlp's and ffmpeg's release lists are.
const RELEASES: &str = "https://api.github.com/repos/denoland/deno/releases/latest";

/// Deno's file for a system and a processor in its releases, as Rust names them
/// (`std::env::consts`); `None` where it publishes none.
fn asset_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("deno-x86_64-pc-windows-msvc.zip"),
        ("windows", "aarch64") => Some("deno-aarch64-pc-windows-msvc.zip"),
        ("macos", "x86_64") => Some("deno-x86_64-apple-darwin.zip"),
        ("macos", "aarch64") => Some("deno-aarch64-apple-darwin.zip"),
        ("linux", "x86_64") => Some("deno-x86_64-unknown-linux-gnu.zip"),
        ("linux", "aarch64") => Some("deno-aarch64-unknown-linux-gnu.zip"),
        _ => None,
    }
}

/// The program's name in the archive and in MDM's install.
fn exe_name() -> &'static str {
    if cfg!(windows) { "deno.exe" } else { "deno" }
}

/// A version that can go into a folder's name (`2.6.3`), and nowhere but there.
fn version_ok(version: &str) -> bool {
    !version.is_empty() && !version.contains(['/', '\\', ':']) && !version.starts_with('.')
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// GitHub's digest of the file, held against Deno's own line for it ([`crate::sum_for`]);
    /// absent on older releases, whose checksum line still stands.
    #[serde(default)]
    digest: Option<String>,
}

/// The Deno to use: the person's own ([`crate::chosen`], #51), else MDM's install in `dir`
/// (`dir/deno/current.json`) if any and still there, else the system's own (`deno` on the PATH
/// or in a folder its own installer uses, #852).
pub fn installed(dir: &Path) -> Option<Installed> {
    crate::chosen::chosen(dir, crate::chosen::Helper::Deno)
        .or_else(|| own(dir))
        .or_else(|| crate::system::installed("deno"))
}

/// Whether MDM downloads a Deno of its own on this system and processor.
pub fn available() -> bool {
    asset_for(std::env::consts::OS, std::env::consts::ARCH).is_some()
}

/// MDM's own install in `dir`.
pub(crate) fn own(dir: &Path) -> Option<Installed> {
    std::fs::read(dir.join("deno").join("current.json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<Installed>(&text).ok())
        .filter(|i| i.exe.is_file())
}

/// Install Deno's newest release into `dir` unless the installed one is as new, the zip
/// downloaded with MDM's engine (`net`), counted into `progress`.
pub async fn install_latest(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
) -> Result<Installed, Error> {
    // The person's own copy: theirs to update (#51).
    if let Some(mine) = crate::chosen::chosen(dir, crate::chosen::Helper::Deno) {
        return Ok(mine);
    }
    let Some(name) = asset_for(std::env::consts::OS, std::env::consts::ARCH) else {
        return Err(Error::Missing(format!(
            "Deno build for {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    };
    let client = crate::client(net, RELEASES)?;
    let body = client
        .get(RELEASES)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let release: Release =
        serde_json::from_slice(&body).map_err(|e| Error::BadRelease(e.to_string()))?;
    // `v2.6.3` in the tag, `2.6.3` in the install (`newer` compares the numbers).
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_owned();
    if !version_ok(&version) {
        return Err(Error::BadRelease(format!("version {version:?}")));
    }
    if let Some(current) = own(dir)
        && !newer(&version, &current.version)
    {
        return Ok(current);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| Error::Missing(format!("{name} in Deno {version}")))?;
    // Deno publishes a `hash  name` line beside each zip, as ffmpeg's releases publish theirs.
    let sums_name = format!("{name}.sha256sum");
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == sums_name)
        .ok_or_else(|| Error::Missing(sums_name.clone()))?;
    let sums = client
        .get(&sums.browser_download_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected = crate::sum_for(&sums, name)
        .ok_or_else(|| Error::Missing(format!("checksum for {name}")))?;
    if let Some(d) = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        && hex::decode(d).ok().as_deref() != Some(&expected[..])
    {
        return Err(Error::BadChecksum);
    }

    // Download to a file beside the install, checked against the checksum.
    let root = dir.join("deno");
    std::fs::create_dir_all(&root)?;
    // Named by its checksum: one release's cut-off download resumes, never another's.
    let zip_path = root.join(format!("deno-{}.zip", hex::encode(&expected[..8])));
    crate::fetch_verified(
        &asset.browser_download_url,
        &zip_path,
        expected,
        net,
        progress,
    )
    .await?;
    let res = unpack(&zip_path, &root.join(&version), &asset.browser_download_url);
    let _ = std::fs::remove_file(&zip_path);
    let exe = res?;
    make_current(&root, Installed { version, exe })
}

/// Unpack the program from the release's zip into `folder`, marked as the download it was.
fn unpack(zip_path: &Path, folder: &Path, from: &str) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(folder)?;
    let exe = folder.join(exe_name());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)
        .map_err(|e| Error::BadRelease(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::BadRelease(e.to_string()))?;
        // Only the program itself, by its own name (nothing climbs out of `folder`).
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        if !entry.is_file() || path.file_name() != Some(std::ffi::OsStr::new(exe_name())) {
            continue;
        }
        let mut out = std::fs::File::create(&exe)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    if !exe.is_file() {
        return Err(Error::Missing(format!("{} in Deno's archive", exe_name())));
    }
    mark_downloaded(&exe, from);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_file_for_this_system() {
        assert_eq!(
            asset_for("windows", "x86_64"),
            Some("deno-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(
            asset_for("windows", "aarch64"),
            Some("deno-aarch64-pc-windows-msvc.zip")
        );
        assert_eq!(
            asset_for("macos", "aarch64"),
            Some("deno-aarch64-apple-darwin.zip")
        );
        assert_eq!(
            asset_for("linux", "x86_64"),
            Some("deno-x86_64-unknown-linux-gnu.zip")
        );
        assert_eq!(asset_for("linux", "riscv64"), None);
        assert_eq!(asset_for("freebsd", "x86_64"), None);
    }

    #[test]
    fn keeps_a_version_out_of_the_path() {
        assert!(version_ok("2.6.3"));
        assert!(!version_ok(""));
        assert!(!version_ok("../evil"));
        assert!(!version_ok("2.6.3/../x"));
        assert!(!version_ok(r"2.6.3\x"));
        assert!(!version_ok("C:2.6.3"));
        assert!(!version_ok(".hidden"));
    }
}

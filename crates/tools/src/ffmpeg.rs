//! ffmpeg for merging video and audio (D-011, user 2026-10-30): BtbN's LGPL "shared" Windows
//! build of the newest stable line (`ffmpeg-nX.Y-latest-win64-lgpl-shared-X.Y.zip`), downloaded
//! on first need, checked against the release's `checksums.sha256` and GitHub's own digest of the
//! asset, its `bin/` unpacked into `tools/ffmpeg/<version>/`, switched to atomically, the previous
//! version kept.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Error, Installed, make_current, mark_downloaded, newer, sum_for};

const RELEASES: &str = "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest";

#[derive(Deserialize)]
struct Release {
    published_at: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>` (GitHub's digest of the upload).
    digest: Option<String>,
}

/// The stable line of an LGPL shared win64 asset: `ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip`
/// → `9.0`.
fn stable_line(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("ffmpeg-n")?;
    let (line, tail) = rest.split_once("-latest-win64-lgpl-shared-")?;
    (tail == format!("{line}.zip") && line.split('.').all(|p| p.parse::<u32>().is_ok()))
        .then_some(line)
}

/// The installed ffmpeg in `dir` (`dir/ffmpeg/current.json`), if any and still there; on Linux
/// and macOS, else the system's own. Its `exe` is `ffmpeg(.exe)`; yt-dlp is given its folder.
pub fn installed(dir: &Path) -> Option<Installed> {
    let own = std::fs::read(dir.join("ffmpeg").join("current.json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<Installed>(&text).ok())
        .filter(|i| i.exe.is_file());
    #[cfg(not(windows))]
    return own.or_else(system);
    #[cfg(windows)]
    own
}

/// Linux and macOS: the ffmpeg the package manager or Homebrew installed (BtbN builds only for
/// Windows here). Homebrew's folders are looked in too: an app started from the Dock doesn't
/// get the shell's PATH.
#[cfg(not(windows))]
fn system() -> Option<Installed> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from))
        .map(|d| d.join("ffmpeg"))
        .find(|f| f.is_file())
        .map(|exe| Installed {
            version: "system".into(),
            exe,
        })
}

/// Install the newest stable LGPL build into `dir` unless the installed one is as new.
pub async fn install_latest(dir: &Path) -> Result<Installed, Error> {
    if !cfg!(windows) {
        return Err(Error::Missing(
            "an ffmpeg build for this system (install ffmpeg)".into(),
        ));
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("MisterDownloadManager/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let body = client
        .get(RELEASES)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let release: Release =
        serde_json::from_slice(&body).map_err(|e| Error::BadRelease(e.to_string()))?;
    let asset = release
        .assets
        .iter()
        .filter_map(|a| stable_line(&a.name).map(|l| (l, a)))
        .max_by(|(a, _), (b, _)| {
            let parts =
                |v: &str| -> Vec<u32> { v.split('.').filter_map(|p| p.parse().ok()).collect() };
            parts(a).cmp(&parts(b))
        })
        .map(|(_, a)| a)
        .ok_or_else(|| Error::Missing("an LGPL Windows build".into()))?;
    let line = stable_line(&asset.name).unwrap_or_default();
    // `9.0.20261028`: the line, then the build date (the release is rebuilt daily).
    let date: String = release
        .published_at
        .chars()
        .take(10)
        .filter(char::is_ascii_digit)
        .collect();
    let version = format!("{line}.{date}");
    if let Some(current) = installed(dir)
        && !newer(&version, &current.version)
    {
        return Ok(current);
    }
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == "checksums.sha256")
        .ok_or_else(|| Error::Missing("checksums.sha256".into()))?;
    let sums = client
        .get(&sums.browser_download_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected = sum_for(&sums, &asset.name)
        .ok_or_else(|| Error::Missing(format!("checksum for {}", asset.name)))?;
    if let Some(d) = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        && hex::decode(d).ok().as_deref() != Some(&expected[..])
    {
        return Err(Error::BadChecksum);
    }

    // Download to a file beside the install (about 80 MB), hashing as it arrives.
    let root = dir.join("ffmpeg");
    std::fs::create_dir_all(&root)?;
    let zip_path = root.join(format!("{version}.zip.part"));
    let hash =
        crate::download_hashed(&client, &asset.browser_download_url, &zip_path, None).await?;
    if hash != expected {
        let _ = std::fs::remove_file(&zip_path);
        return Err(Error::BadChecksum);
    }
    let res = unpack(&zip_path, &root, &version, &asset.browser_download_url);
    let _ = std::fs::remove_file(&zip_path);
    let exe = res?;
    make_current(&root, Installed { version, exe })
}

/// Unpack the zip's `bin/` (ffmpeg.exe, ffprobe.exe and their DLLs) into `root/<version>/`.
fn unpack(zip_path: &Path, root: &Path, version: &str, from: &str) -> Result<PathBuf, Error> {
    if version.is_empty() || version.contains(['/', '\\', ':']) || version.starts_with('.') {
        return Err(Error::BadRelease(format!("version {version:?}")));
    }
    let folder = root.join(version);
    std::fs::create_dir_all(&folder)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)
        .map_err(|e| Error::BadRelease(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::BadRelease(e.to_string()))?;
        // Only plain files directly in a `bin` folder, by their own name (nothing can climb out).
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let in_bin = path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|p| p == "bin");
        let Some(name) = path.file_name().map(|n| n.to_owned()) else {
            continue;
        };
        if !entry.is_file() || !in_bin {
            continue;
        }
        let dest = folder.join(&name);
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
        if Path::new(&name).extension().is_some_and(|e| e == "exe") {
            mark_downloaded(&dest, from);
        }
    }
    let exe = folder.join("ffmpeg.exe");
    if !exe.is_file() {
        return Err(Error::Missing("ffmpeg.exe in the build".into()));
    }
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_stable_lgpl_shared_builds_only() {
        assert_eq!(
            stable_line("ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip"),
            Some("9.0")
        );
        assert_eq!(stable_line("ffmpeg-n9.0-latest-win64-lgpl-9.0.zip"), None);
        assert_eq!(
            stable_line("ffmpeg-master-latest-win64-lgpl-shared.zip"),
            None
        );
        assert_eq!(
            stable_line("ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"),
            None
        );
        assert_eq!(
            stable_line("ffmpeg-n8.1-latest-win64-lgpl-shared-9.0.zip"),
            None
        );
    }

    #[test]
    fn unpacks_only_bin_files() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("b.zip");
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (name, data) in [
                ("ffmpeg-n9.0/bin/ffmpeg.exe", "exe"),
                ("ffmpeg-n9.0/bin/avcodec-62.dll", "dll"),
                ("ffmpeg-n9.0/doc/readme.txt", "doc"),
                ("ffmpeg-n9.0/LICENSE.txt", "lic"),
            ] {
                w.start_file(name, opts).unwrap();
                w.write_all(data.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let exe = unpack(&zip_path, dir.path(), "9.0.20261028", "https://x/").unwrap();
        let folder = exe.parent().unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "exe");
        assert!(folder.join("avcodec-62.dll").is_file());
        assert!(!folder.join("readme.txt").exists());
        assert!(!folder.join("LICENSE.txt").exists());
    }
}

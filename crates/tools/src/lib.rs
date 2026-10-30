//! Verified, pinned installs of helper tools (D-011). yt-dlp is never updated by itself
//! (`yt-dlp -U`): we fetch a release from its GitHub page, verify its `SHA2-256SUMS` against
//! yt-dlp's OpenPGP signing key (pinned in `keys/yt-dlp.asc`), verify the program against that
//! list, install it next to earlier versions (`tools/yt-dlp/<version>/`), switch to it
//! atomically, never downgrade, and keep the previous version for rollback.

pub mod ffmpeg;
pub mod pages;
pub mod proc;
pub mod resolve;
pub mod servers;
pub mod video;

use std::io::Cursor;
use std::path::{Path, PathBuf};

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// yt-dlp's release signing key, from its repository (`public.key`).
const YTDLP_KEY: &str = include_str!("../keys/yt-dlp.asc");
/// Where releases come from.
const RELEASES: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("disk: {0}")]
    Io(#[from] std::io::Error),
    #[error("the release's checksum list is not signed by yt-dlp's key")]
    BadSignature,
    #[error("the downloaded program doesn't match its signed checksum")]
    BadChecksum,
    #[error("the release has no {0}")]
    Missing(String),
    #[error("unexpected release data: {0}")]
    BadRelease(String),
}

/// An installed tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub exe: PathBuf,
}

/// `<data dir>/tools`.
pub fn tools_dir() -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join("tools"))
}

/// yt-dlp's file name for this platform in its releases.
pub fn ytdlp_asset() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else {
        "yt-dlp_linux"
    }
}

/// Whether `sums` carries a valid signature `sig` (binary OpenPGP) by yt-dlp's key or one of its
/// subkeys.
pub fn signed_by_ytdlp(sums: &[u8], sig: &[u8]) -> bool {
    let Ok((key, _)) = SignedPublicKey::from_armor_single(Cursor::new(YTDLP_KEY)) else {
        return false;
    };
    let Ok(sig) = DetachedSignature::from_bytes(Cursor::new(sig)) else {
        return false;
    };
    sig.verify(&key, sums).is_ok()
        || key
            .public_subkeys
            .iter()
            .any(|sub| sig.verify(&sub.key, sums).is_ok())
}

/// The SHA-256 a `hash  name` list gives for `name`.
pub fn sum_for(sums: &str, name: &str) -> Option<[u8; 32]> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        if file.trim().trim_start_matches('*') != name {
            return None;
        }
        hex::decode(hash).ok()?.try_into().ok()
    })
}

/// The yt-dlp install in `dir` (`dir/yt-dlp/current.json`), if any and still there.
pub fn installed(dir: &Path) -> Option<Installed> {
    let text = std::fs::read(dir.join("yt-dlp").join("current.json")).ok()?;
    let i: Installed = serde_json::from_slice(&text).ok()?;
    i.exe.is_file().then_some(i)
}

/// Versions are release dates (`2026.10.26`, maybe `.1`): numeric parts compare in order.
pub(crate) fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split('.').filter_map(|p| p.parse().ok()).collect() };
    parts(a) > parts(b)
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
}

/// Install the latest yt-dlp release into `dir` unless the installed one is as new. Returns
/// what is installed afterwards.
pub async fn install_latest(dir: &Path) -> Result<Installed, Error> {
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
    if let Some(current) = installed(dir)
        && !newer(&release.tag_name, &current.version)
    {
        return Ok(current); // never the same again, never older
    }
    let url = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
            .ok_or_else(|| Error::Missing(name.to_owned()))
    };
    let fetch = |u: String| {
        let client = client.clone();
        async move {
            Ok::<_, Error>(
                client
                    .get(u)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?,
            )
        }
    };
    let sums = fetch(url("SHA2-256SUMS")?).await?;
    let sig = fetch(url("SHA2-256SUMS.sig")?).await?;
    if !signed_by_ytdlp(&sums, &sig) {
        return Err(Error::BadSignature);
    }
    let sums = String::from_utf8_lossy(&sums);
    let asset = ytdlp_asset();
    let expected =
        sum_for(&sums, asset).ok_or_else(|| Error::Missing(format!("checksum for {asset}")))?;
    let exe_url = url(asset)?;
    let program = fetch(exe_url.clone()).await?;
    if Sha256::digest(&program).as_slice() != expected {
        return Err(Error::BadChecksum);
    }
    put(dir, &release.tag_name, asset, &program, &exe_url)
}

/// Install verified `program` as `version` and make it current (atomically), keeping the
/// previous version for rollback and removing older ones.
fn put(
    dir: &Path,
    version: &str,
    asset: &str,
    program: &[u8],
    from: &str,
) -> Result<Installed, Error> {
    if version.is_empty() || version.contains(['/', '\\', ':']) || version.starts_with('.') {
        return Err(Error::BadRelease(format!("version {version:?}")));
    }
    let root = dir.join("yt-dlp");
    let folder = root.join(version);
    std::fs::create_dir_all(&folder)?;
    let exe = folder.join(asset);
    craft_atomic::atomic_write(&exe, program)?;
    mark_downloaded(&exe, from);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }
    make_current(
        &root,
        Installed {
            version: version.to_owned(),
            exe,
        },
    )
}

/// Make `now` (installed in `root/<version>/`) the current version of a tool: `current.json`
/// switched atomically, the previous version kept for rollback, older ones removed.
pub(crate) fn make_current(root: &Path, now: Installed) -> Result<Installed, Error> {
    let previous = std::fs::read(root.join("current.json"))
        .ok()
        .and_then(|t| serde_json::from_slice::<Installed>(&t).ok());
    let json = serde_json::to_vec_pretty(&now).map_err(std::io::Error::other)?;
    craft_atomic::atomic_write(&root.join("current.json"), &json)?;
    // Keep the new and the previous version; older ones go.
    let keep: Vec<String> = [Some(now.version.clone()), previous.map(|p| p.version)]
        .into_iter()
        .flatten()
        .collect();
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !keep.contains(&name) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(now)
}

/// Mark-of-the-Web: the program came from the internet (Windows checks it like a browser
/// download).
pub(crate) fn mark_downloaded(path: &Path, from: &str) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(
            stream,
            format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={from}\r\n"),
        );
    }
    #[cfg(not(windows))]
    let _ = (path, from);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMS: &[u8] = include_bytes!("../tests/fixtures/SHA2-256SUMS");
    const SIG: &[u8] = include_bytes!("../tests/fixtures/SHA2-256SUMS.sig");

    #[test]
    fn a_real_release_checksum_list_is_signed_by_the_pinned_key() {
        assert!(signed_by_ytdlp(SUMS, SIG));
    }

    #[test]
    fn a_changed_list_fails_the_signature() {
        let mut forged = SUMS.to_vec();
        forged[0] = if forged[0] == b'0' { b'1' } else { b'0' };
        assert!(!signed_by_ytdlp(&forged, SIG));
        assert!(!signed_by_ytdlp(SUMS, b"not a signature"));
    }

    #[test]
    fn checksums_by_name() {
        let sums = String::from_utf8_lossy(SUMS);
        let exe = sum_for(&sums, "yt-dlp.exe").unwrap();
        assert_eq!(
            hex::encode(exe),
            "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d"
        );
        assert!(sum_for(&sums, "yt-dlp.ex").is_none());
    }

    #[test]
    fn versions_compare_as_dates() {
        assert!(newer("2026.10.26", "2026.9.30"));
        assert!(newer("2026.10.26.1", "2026.10.26"));
        assert!(!newer("2026.10.26", "2026.10.26"));
    }

    #[test]
    fn installs_switch_and_keep_one_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let name = ytdlp_asset();
        put(dir.path(), "2026.01.01", name, b"one", "https://x/").unwrap();
        put(dir.path(), "2026.02.01", name, b"two", "https://x/").unwrap();
        let now = put(dir.path(), "2026.03.01", name, b"three", "https://x/").unwrap();
        assert_eq!(installed(dir.path()).unwrap(), now);
        assert_eq!(std::fs::read(&now.exe).unwrap(), b"three");
        let root = dir.path().join("yt-dlp");
        assert!(
            root.join("2026.02.01").is_dir(),
            "the previous version stays"
        );
        assert!(!root.join("2026.01.01").exists(), "older ones go");
        assert!(put(dir.path(), "../evil", name, b"x", "https://x/").is_err());
    }
}

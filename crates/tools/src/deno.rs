//! Deno, the JavaScript runtime yt-dlp solves YouTube's challenges with (#1448, #1470): without
//! one, a YouTube look-up is slow, lists few qualities, or fails ("Requested format is not
//! available"). Where the computer has none (Node.js, or a Deno of its own, found as before:
//! `proc::system_js_runtime`), Helpers downloads Deno's official build for this system and
//! processor from its GitHub releases (`denoland/deno`), checks the zip against the release's own
//! SHA-256 (`<zip>.sha256sum`, and GitHub's digest of the upload when its API gives one), unpacks
//! the one program into `tools/deno/<version>/`, switches to it atomically and keeps the previous
//! version, as ffmpeg's is kept. yt-dlp is then told of it (`--js-runtimes deno:<path>`,
//! `proc`). Deno signs nothing with a key of its own: its published checksums are what there is
//! to check against, as with ffmpeg.

use std::path::{Path, PathBuf};

use crate::{Error, Installed, github, mark_downloaded, newer, sum_for};

/// Deno's latest release on GitHub's API.
const RELEASES: &str = "https://api.github.com/repos/denoland/deno/releases/latest";
/// Deno's repository on GitHub.
const REPO: &str = "denoland/deno";

/// Deno's zip for this system and processor in its releases; None where it publishes none
/// (32-bit Windows, other processors): a runtime of the person's own works there.
pub fn asset() -> Option<&'static str> {
    asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// Whether MDM downloads a Deno of its own on this system ([`asset`]).
pub fn available() -> bool {
    asset().is_some()
}

/// [`asset`] for a system and a processor, as Rust names them (`std::env::consts`).
fn asset_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("deno-x86_64-pc-windows-msvc.zip"),
        ("windows", "aarch64") => Some("deno-aarch64-pc-windows-msvc.zip"),
        ("linux", "x86_64") => Some("deno-x86_64-unknown-linux-gnu.zip"),
        ("linux", "aarch64") => Some("deno-aarch64-unknown-linux-gnu.zip"),
        ("macos", "x86_64") => Some("deno-x86_64-apple-darwin.zip"),
        ("macos", "aarch64") => Some("deno-aarch64-apple-darwin.zip"),
        _ => None,
    }
}

/// The checksum file Deno publishes beside each zip.
fn sums_name(asset: &str) -> String {
    format!("{asset}.sha256sum")
}

/// The SHA-256 a Deno `.sha256sum` file gives for `name`. Its Linux and macOS ones are
/// `sha256sum`'s `hash  name`; its Windows ones are PowerShell's `Get-FileHash | Format-List`:
///
/// ```text
/// Algorithm : SHA256
/// Hash      : CBA84B26…
/// Path      : C:\a\deno\deno\target\release\deno-x86_64-pc-windows-msvc.zip
/// ```
///
/// That one counts only when it is SHA-256 and its path ends in `name`.
pub(crate) fn sum_in(text: &str, name: &str) -> Option<[u8; 32]> {
    if let Some(sum) = sum_for(text, name) {
        return Some(sum);
    }
    let field = |key: &str| {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == key).then(|| v.trim())
        })
    };
    let file = field("Path")?.rsplit(['\\', '/']).next()?;
    if !field("Algorithm")?.eq_ignore_ascii_case("SHA256") || file != name {
        return None;
    }
    hex::decode(field("Hash")?).ok()?.try_into().ok()
}

/// `v2.9.8` → `2.9.8`: a release's tag as the version kept (compared by [`newer`]).
fn version_of(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// `deno.exe` on Windows, `deno` elsewhere.
fn program() -> String {
    format!("deno{}", std::env::consts::EXE_SUFFIX)
}

/// MDM's own Deno in `dir` (`dir/deno/current.json`), if any and still there.
pub fn installed(dir: &Path) -> Option<Installed> {
    std::fs::read(dir.join("deno").join("current.json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<Installed>(&text).ok())
        .filter(|i| i.exe.is_file())
}

/// Install Deno's latest release into `dir` unless the installed one is as new, the zip
/// downloaded with MDM's engine (`net`), counted into `progress`. Returns what is installed
/// afterwards.
pub async fn install_latest(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
) -> Result<Installed, Error> {
    install_with(dir, net, progress, None).await?;
    installed(dir).ok_or_else(|| Error::Missing("Deno after its install".into()))
}

/// [`install_latest`] for `autoupdate`: the newest release, installed when it is newer than the
/// version in use and than `floor` (a version the person went back from), switched to now or once
/// the runs using Deno end.
pub(crate) async fn install_with(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
    floor: Option<&str>,
) -> Result<crate::autoupdate::Got, Error> {
    let asset = asset().ok_or_else(|| Error::Missing("Deno build for this system".into()))?;
    let _installing = crate::autoupdate::installing(&dir.join("deno"), "Deno")?;
    // From GitHub's API, else (its rate limit) github.com's pages (#1337): the same files,
    // checked the same way.
    let names = [sums_name(asset), asset.to_owned()];
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let release = github::latest_release(net, RELEASES, github::WEB, REPO, &names).await?;
    install_release(dir, net, progress, asset, &release, floor).await
}

/// [`install_latest`] from `release`, its zip `asset`.
async fn install_release(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
    asset: &str,
    release: &github::Latest,
    floor: Option<&str>,
) -> Result<crate::autoupdate::Got, Error> {
    let sums = sums_name(asset);
    let version = version_of(&release.tag).to_owned();
    let root = dir.join("deno");
    if installed(dir).is_some_and(|current| !newer(&version, &current.version))
        || floor.is_some_and(|f| !newer(&version, f))
    {
        // never the same again, never older
        return Ok(crate::autoupdate::Got {
            latest: version,
            settled: None,
        });
    }
    // Already here (staged, or kept after going back): switched to, not downloaded again.
    if let Some(here) = crate::autoupdate::kept(&root, &version) {
        let settled = Some(crate::autoupdate::settle(&root, here)?);
        return Ok(crate::autoupdate::Got {
            latest: version,
            settled,
        });
    }
    let url = |name: &str| {
        release
            .files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, u)| u.clone())
            .ok_or_else(|| Error::Missing(name.to_owned()))
    };
    let sums_url = url(&sums)?;
    let client = crate::client(net, &sums_url)?;
    let text = String::from_utf8_lossy(&crate::get_bytes(&client, &sums_url).await?).into_owned();
    let expected =
        sum_in(&text, asset).ok_or_else(|| Error::Missing(format!("checksum for {asset}")))?;
    // GitHub's own digest of the upload, when its API gave one, must agree.
    if let Some((_, d)) = release.digests.iter().find(|(n, _)| n == asset)
        && let Some(hex_sum) = d.strip_prefix("sha256:")
        && hex::decode(hex_sum).ok().as_deref() != Some(&expected[..])
    {
        return Err(Error::BadChecksum);
    }
    let zip_url = url(asset)?;
    std::fs::create_dir_all(&root)?;
    // Named by its checksum: one release's cut-off download resumes, never another's.
    let zip_path = root.join(format!("deno-{}.zip", hex::encode(&expected[..8])));
    crate::fetch_verified(&zip_url, &zip_path, expected, net, progress).await?;
    let res = unpack(&zip_path, &root, &version, &zip_url);
    let _ = std::fs::remove_file(&zip_path);
    let exe = res?;
    tracing::info!("Deno {version} verified and installed: {}", exe.display());
    let settled = Some(crate::autoupdate::settle(
        &root,
        Installed {
            version: version.clone(),
            exe,
        },
    )?);
    Ok(crate::autoupdate::Got {
        latest: version,
        settled,
    })
}

/// Unpack the zip's one program (`deno.exe`, `deno`, at its top) into `root/<version>/`.
fn unpack(zip_path: &Path, root: &Path, version: &str, from: &str) -> Result<PathBuf, Error> {
    if version.is_empty() || version.contains(['/', '\\', ':']) || version.starts_with('.') {
        return Err(Error::BadRelease(format!("version {version:?}")));
    }
    let name = program();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path)?)
        .map_err(|e| Error::BadRelease(e.to_string()))?;
    let mut entry = zip
        .by_name(&name)
        .map_err(|_| Error::Missing(format!("{name} in the release")))?;
    if !entry.is_file() {
        return Err(Error::Missing(format!("{name} in the release")));
    }
    let folder = root.join(version);
    std::fs::create_dir_all(&folder)?;
    let exe = folder.join(&name);
    // Written beside, then put in place: a cut-off unpack never leaves half a program.
    let part = folder.join(format!("{name}.part"));
    {
        let mut out = std::fs::File::create(&part)?;
        std::io::copy(&mut entry, &mut out)?;
        out.sync_all()?;
    }
    std::fs::rename(&part, &exe)?;
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

    const WIN_X64: &str =
        include_str!("../tests/fixtures/deno/deno-x86_64-pc-windows-msvc.zip.sha256sum");
    const WIN_ARM: &str =
        include_str!("../tests/fixtures/deno/deno-aarch64-pc-windows-msvc.zip.sha256sum");
    const LINUX_X64: &str =
        include_str!("../tests/fixtures/deno/deno-x86_64-unknown-linux-gnu.zip.sha256sum");
    const MAC_ARM: &str =
        include_str!("../tests/fixtures/deno/deno-aarch64-apple-darwin.zip.sha256sum");

    /// Each system and processor takes Deno's own build for it (its v2.9.8 names); 32-bit
    /// Windows and other processors none.
    #[test]
    fn each_system_takes_its_own_build() {
        for (os, arch, zip) in [
            ("windows", "x86_64", "deno-x86_64-pc-windows-msvc.zip"),
            ("windows", "aarch64", "deno-aarch64-pc-windows-msvc.zip"),
            ("linux", "x86_64", "deno-x86_64-unknown-linux-gnu.zip"),
            ("linux", "aarch64", "deno-aarch64-unknown-linux-gnu.zip"),
            ("macos", "x86_64", "deno-x86_64-apple-darwin.zip"),
            ("macos", "aarch64", "deno-aarch64-apple-darwin.zip"),
        ] {
            assert_eq!(asset_for(os, arch), Some(zip), "{os} {arch}");
            assert_eq!(sums_name(zip), format!("{zip}.sha256sum"));
        }
        assert_eq!(asset_for("windows", "x86"), None);
        assert_eq!(asset_for("linux", "riscv64"), None);
        assert_eq!(asset_for("freebsd", "x86_64"), None);
        assert_eq!(available(), asset().is_some());
    }

    /// Deno's real checksum files (v2.9.8): PowerShell's list on Windows, `sha256sum`'s line
    /// elsewhere, each giving the digest GitHub has for the zip.
    #[test]
    fn the_release_checksum_files_are_read_in_both_forms() {
        for (text, name, digest) in [
            (
                WIN_X64,
                "deno-x86_64-pc-windows-msvc.zip",
                "cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf",
            ),
            (
                WIN_ARM,
                "deno-aarch64-pc-windows-msvc.zip",
                "b413b92b735b50c198e8fe3b85af1aa50e29d5f72e229aab701d132286e14cef",
            ),
            (
                LINUX_X64,
                "deno-x86_64-unknown-linux-gnu.zip",
                "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609",
            ),
            (
                MAC_ARM,
                "deno-aarch64-apple-darwin.zip",
                "9f0bf660393da2eaff044cdc7444cb08c73f235849b61d224d825a194ce7c98e",
            ),
        ] {
            assert_eq!(sum_in(text, name).map(hex::encode).as_deref(), Some(digest));
        }
        // Another file's checksum is not this one's.
        assert_eq!(sum_in(WIN_X64, "deno-aarch64-pc-windows-msvc.zip"), None);
        assert_eq!(
            sum_in(LINUX_X64, "deno-aarch64-unknown-linux-gnu.zip"),
            None
        );
        assert_eq!(sum_in(WIN_X64, "msvc.zip"), None);
        // Not SHA-256, not a whole hash, or nothing at all.
        let md5 = WIN_X64.replace("SHA256", "MD5");
        assert_eq!(sum_in(&md5, "deno-x86_64-pc-windows-msvc.zip"), None);
        let short = WIN_X64.replace("CBA84B26", "");
        assert_eq!(sum_in(&short, "deno-x86_64-pc-windows-msvc.zip"), None);
        assert_eq!(sum_in("", "deno-x86_64-pc-windows-msvc.zip"), None);
    }

    #[test]
    fn tags_are_versions() {
        assert_eq!(version_of("v2.9.8"), "2.9.8");
        assert_eq!(version_of("2.9.8"), "2.9.8");
        assert!(newer(version_of("v2.10.0"), "2.9.8"));
        assert!(!newer(version_of("v2.9.8"), "2.9.8"));
    }

    #[test]
    fn unpacks_the_program_alone() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("d.zip");
        let name = program();
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (n, data) in [(name.as_str(), "deno"), ("README.md", "doc")] {
                w.start_file(n, opts).unwrap();
                w.write_all(data.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let exe = unpack(&zip_path, dir.path(), "2.9.8", "https://x/").unwrap();
        assert_eq!(exe, dir.path().join("2.9.8").join(&name));
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "deno");
        assert!(!exe.with_file_name("README.md").exists());
        assert!(!exe.with_file_name(format!("{name}.part")).exists());
        assert!(unpack(&zip_path, dir.path(), "../x", "https://x/").is_err());
        // A zip without the program is refused.
        let empty = dir.path().join("e.zip");
        zip::ZipWriter::new(std::fs::File::create(&empty).unwrap())
            .finish()
            .unwrap();
        assert!(matches!(
            unpack(&empty, dir.path(), "2.9.9", "https://x/"),
            Err(Error::Missing(_))
        ));
    }

    /// Installed as `current.json` names it, only while the program is there.
    #[test]
    fn the_installed_copy_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed(dir.path()), None);
        let root = dir.path().join("deno");
        let exe = root.join("2.9.8").join(program());
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"deno").unwrap();
        let now = crate::make_current(
            &root,
            Installed {
                version: "2.9.8".into(),
                exe: exe.clone(),
            },
        )
        .unwrap();
        assert_eq!(installed(dir.path()), Some(now));
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(installed(dir.path()), None);
    }

    /// The whole install against a release served on a lab server, as GitHub's API lists it:
    /// the zip checked against its checksum file and GitHub's digest, unpacked, made current;
    /// the same release again is not downloaded twice; a zip that isn't the one the checksum
    /// names, or a digest that disagrees, is refused and nothing is installed.
    #[tokio::test]
    async fn installs_from_a_release_checked_against_its_sums() {
        let Some(asset) = asset() else {
            return; // no Deno build for this system
        };
        let name = program();
        let mut zip = Vec::new();
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut zip));
            w.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"a Deno, as far as this test goes").unwrap();
            w.finish().unwrap();
        }
        let sum = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&zip));
        let files = tempfile::tempdir().unwrap();
        std::fs::write(files.path().join(asset), &zip).unwrap();
        std::fs::write(
            files.path().join(sums_name(asset)),
            format!("{sum}  {asset}\n"),
        )
        .unwrap();
        let lab = mdm_labserver::LabServer::start(mdm_labserver::Config {
            files: Some(files.path().to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
        let net = crate::net(mdm_engine::Proxy::None);
        let release = |digest: &str| github::Latest {
            tag: "v2.9.8".into(),
            files: [asset.to_owned(), sums_name(asset)]
                .into_iter()
                .map(|n| (n.clone(), lab.url(&format!("/files/{n}"))))
                .collect(),
            digests: vec![(asset.to_owned(), format!("sha256:{digest}"))],
        };
        let dir = tempfile::tempdir().unwrap();
        // A digest that disagrees with the checksum file: refused before downloading.
        let wrong = install_release(
            dir.path(),
            &net,
            None,
            asset,
            &release(&"0".repeat(64)),
            None,
        )
        .await;
        assert!(matches!(wrong, Err(Error::BadChecksum)), "{wrong:?}");
        assert_eq!(installed(dir.path()), None);
        let got = match install_release(dir.path(), &net, None, asset, &release(&sum), None)
            .await
            .unwrap()
            .settled
        {
            Some(crate::autoupdate::Settled::Switched(i)) => i,
            other => panic!("{other:?}"),
        };
        assert_eq!(got.version, "2.9.8");
        assert_eq!(got.exe, dir.path().join("deno").join("2.9.8").join(&name));
        assert_eq!(
            std::fs::read(&got.exe).unwrap(),
            b"a Deno, as far as this test goes"
        );
        assert_eq!(installed(dir.path()), Some(got.clone()));
        // The zip itself is not kept.
        let left: Vec<_> = std::fs::read_dir(dir.path().join("deno"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(
            left.iter().all(|n| !n.to_string_lossy().ends_with(".zip")),
            "{left:?}"
        );
        // A newer release whose zip isn't the one its checksum names: refused, the installed
        // one stays.
        std::fs::write(files.path().join(asset), b"not the listed zip").unwrap();
        let mut newer_one = release(&sum);
        newer_one.tag = "v2.9.9".into();
        let bad = install_release(dir.path(), &net, None, asset, &newer_one, None).await;
        assert!(matches!(bad, Err(Error::BadChecksum)), "{bad:?}");
        assert_eq!(installed(dir.path()), Some(got));
    }
}

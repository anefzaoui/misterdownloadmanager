//! ffmpeg for merging video and audio (D-011, user 2026-10-30): BtbN's LGPL "shared" Windows
//! build of a stable line for this processor (`…-win64-lgpl-shared-X.Y.zip`, `…-winarm64-…` on
//! Windows on ARM; BtbN builds none for 32-bit Windows), downloaded on first need, its `bin/`
//! unpacked into `tools/ffmpeg/<version>/`, switched to atomically, the previous version kept.
//!
//! Which build, and the SHA-256 it must have, come from MDM's own list of the builds its
//! maintainers checked (`ffmpeg-builds.txt`, made by `tools/release/ffmpeg_list.py`), signed with
//! MDM's release key ([`crate::signed_by_mdm`]) and published with each release of MDM
//! (release.yml), as its `SHA256SUMS.txt` is. The list must carry that signature, must be no older
//! than the newest one this copy of MDM has seen (its sequence number, kept in
//! `tools/ffmpeg/builds-list.json`), and the zip is installed only when its SHA-256 is the one the
//! list gives. Nothing BtbN's release says is trusted on its own (its checksum file, GitHub's
//! digest): a list that can't be fetched or checked means no ffmpeg is installed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::autoupdate::Got;
use crate::{Error, Installed, mark_downloaded, newer};

/// MDM's signed list of the ffmpeg builds it installs, a file of its latest release; its
/// signature is the same address with `.sig`.
const LIST: &str =
    "https://github.com/anefzaoui/misterdownloadmanager/releases/latest/download/ffmpeg-builds.txt";
/// The list's first line: what it is, and its format.
const FORMAT: &str = "mdm-ffmpeg-builds 1";
/// The newest list's sequence number this copy of MDM has seen, in `tools/ffmpeg/`.
const SEEN: &str = "builds-list.json";

/// Why MDM's signed list of ffmpeg builds let no build in.
#[derive(Debug, thiserror::Error)]
pub enum ListError {
    /// It couldn't be fetched (why).
    #[error("it couldn't be downloaded ({0})")]
    Unavailable(Box<Error>),
    /// Its signature isn't by MDM's release key, or it has none.
    #[error("it isn't signed by MDM's release key")]
    Unsigned,
    /// An older list than one already seen: never gone back to.
    #[error("it is older than one already seen (number {got}, {seen} before)")]
    Older { got: u64, seen: u64 },
    /// Not a list this MDM can read (what is wrong).
    #[error("it can't be read: {0}")]
    Unreadable(String),
    /// It names no build for this system and processor.
    #[error("it names no build for {0}")]
    NoBuild(String),
    /// The downloaded build's SHA-256 isn't the one the list gives.
    #[error("the downloaded build isn't the one it names")]
    NotListed,
}

/// BtbN's name for its Windows builds for this processor (`win64`, `winarm64`); None where it
/// publishes none (32-bit Windows), and off Windows, where the system's ffmpeg is used.
pub fn build() -> Option<&'static str> {
    build_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// Whether MDM downloads an ffmpeg of its own on this system ([`build`]). Where it doesn't, the
/// person's own (chosen in Helpers) or the system's is used.
pub fn available() -> bool {
    build().is_some()
}

/// [`build`] for a system and a processor, as Rust names them (`std::env::consts`).
fn build_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("win64"),
        ("windows", "aarch64") => Some("winarm64"),
        _ => None,
    }
}

/// The stable line of an LGPL shared asset for `build` (`win64`, `winarm64`), named as BtbN's
/// `latest` release names it (`ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip`) or as a dated build
/// does (`ffmpeg-n9.0.2-47-gb204e94e90-win64-lgpl-shared-9.0.zip`) → `9.0`. Never the development
/// (`master`, `ffmpeg-N-…`) builds, nor another processor's.
fn stable_line<'a>(name: &'a str, build: &str) -> Option<&'a str> {
    let rest = name.strip_prefix("ffmpeg-n")?.strip_suffix(".zip")?;
    let (head, line) = rest.rsplit_once(&format!("-{build}-lgpl-shared-"))?;
    let numeric = |v: &str| !v.is_empty() && v.split('.').all(|p| p.parse::<u32>().is_ok());
    let (version, build) = head.split_once('-')?;
    let of_line = version == line || version.starts_with(&format!("{line}."));
    let build_ok = build == "latest"
        || build.split_once("-g").is_some_and(|(n, hash)| {
            n.parse::<u32>().is_ok() && hash.chars().all(|c| c.is_ascii_hexdigit())
        });
    (numeric(line) && numeric(version) && of_line && build_ok).then_some(line)
}

/// One build the list names.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Build {
    /// The system and the processor, as Rust names them (`windows`, `x86_64`).
    os: String,
    arch: String,
    /// MDM's version for it: its stable line, then its build's date (`9.0.20261031`).
    version: String,
    sha256: [u8; 32],
    /// Where it is downloaded from, and its file name (the address's last part).
    url: String,
    name: String,
}

/// MDM's list of the ffmpeg builds it installs.
#[derive(Debug, PartialEq, Eq)]
struct List {
    /// Each new list's is higher than the one before.
    sequence: u64,
    builds: Vec<Build>,
}

/// The list in `text`: [`FORMAT`] first, then `sequence <number>` and a line
/// `build <os> <arch> <version> <sha256> <address>` for each system. Blank lines and `#` comments
/// are skipped, and lines of another kind left for a later MDM; a line of these kinds that isn't
/// as it should be makes the whole list unreadable.
fn parse(text: &str) -> Result<List, ListError> {
    let bad = |what: String| ListError::Unreadable(what);
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let head = lines.next().unwrap_or_default();
    if head.split_whitespace().ne(FORMAT.split_whitespace()) {
        return Err(bad("not a list of ffmpeg builds this MDM reads".into()));
    }
    let mut sequence = None;
    let mut builds: Vec<Build> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words[..] {
            ["sequence", n] => {
                let n = n
                    .parse::<u64>()
                    .map_err(|_| bad(format!("sequence number {n:?}")))?;
                if sequence.replace(n).is_some() {
                    return Err(bad("two sequence numbers".into()));
                }
            }
            ["build", os, arch, version, sum, url] => {
                let numeric = version
                    .split('.')
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
                if !numeric {
                    return Err(bad(format!("version {version:?}")));
                }
                let sha256: [u8; 32] = hex::decode(sum)
                    .ok()
                    .and_then(|v| v.try_into().ok())
                    .ok_or_else(|| bad(format!("SHA-256 {sum:?}")))?;
                let name = url::Url::parse(url)
                    .ok()
                    .filter(|u| matches!(u.scheme(), "https" | "http"))
                    .and_then(|u| Some(u.path_segments()?.next_back()?.to_owned()))
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| bad(format!("address {url:?}")))?;
                if builds.iter().any(|b| b.os == os && b.arch == arch) {
                    return Err(bad(format!("two builds for {os} {arch}")));
                }
                builds.push(Build {
                    os: os.into(),
                    arch: arch.into(),
                    version: version.into(),
                    sha256,
                    url: url.into(),
                    name,
                });
            }
            ["sequence", ..] | ["build", ..] => return Err(bad(format!("line {line:?}"))),
            _ => {}
        }
    }
    let sequence = sequence.ok_or_else(|| bad("no sequence number".into()))?;
    Ok(List { sequence, builds })
}

impl List {
    /// Its build for `os` and `arch` (as Rust names them), which must be BtbN's LGPL shared
    /// build for that processor, its version of that build's stable line.
    fn build_for(&self, os: &str, arch: &str) -> Result<&Build, ListError> {
        let none = || ListError::NoBuild(format!("{os} {arch}"));
        let btbn = build_for(os, arch).ok_or_else(none)?;
        let build = self
            .builds
            .iter()
            .find(|b| b.os == os && b.arch == arch)
            .ok_or_else(none)?;
        let line = stable_line(&build.name, btbn).ok_or_else(|| {
            ListError::Unreadable(format!("{} isn't an LGPL build for {btbn}", build.name))
        })?;
        if !build.version.starts_with(&format!("{line}.")) {
            return Err(ListError::Unreadable(format!(
                "version {} for ffmpeg {line}",
                build.version
            )));
        }
        Ok(build)
    }
}

/// Where the list comes from, and whose signature it must carry.
pub(crate) struct Source<'a> {
    /// The list's address; its signature's is the same with `.sig`.
    pub list: &'a str,
    pub signed: &'a (dyn Fn(&[u8], &[u8]) -> bool + Sync),
}

/// What `tools/ffmpeg/builds-list.json` keeps.
#[derive(Default, Serialize, Deserialize)]
struct Seen {
    sequence: u64,
}

/// The newest list's sequence number seen in `root` (0 for none).
fn seen(root: &Path) -> u64 {
    std::fs::read(root.join(SEEN))
        .ok()
        .and_then(|t| serde_json::from_slice::<Seen>(&t).ok())
        .unwrap_or_default()
        .sequence
}

/// The list from `source`, accepted only with its signature and when it is no older than the
/// newest one seen in `root`, which it then becomes.
async fn checked_list(
    root: &Path,
    net: &mdm_engine::Options,
    source: &Source<'_>,
) -> Result<List, ListError> {
    let fetched = async {
        let client = crate::client(net, source.list)?;
        let text = crate::get_bytes(&client, source.list).await?;
        let sig = crate::get_bytes(&client, &format!("{}.sig", source.list)).await?;
        Ok::<_, Error>((text, sig))
    };
    let (text, sig) = fetched
        .await
        .map_err(|e| ListError::Unavailable(Box::new(e)))?;
    if !(source.signed)(&text, &sig) {
        return Err(ListError::Unsigned);
    }
    let list =
        parse(std::str::from_utf8(&text).map_err(|_| ListError::Unreadable("not text".into()))?)?;
    let seen = seen(root);
    if list.sequence < seen {
        return Err(ListError::Older {
            got: list.sequence,
            seen,
        });
    }
    if list.sequence > seen {
        let json = serde_json::to_vec_pretty(&Seen {
            sequence: list.sequence,
        })
        .map_err(|e| ListError::Unreadable(e.to_string()))?;
        let kept = std::fs::create_dir_all(root)
            .and_then(|()| craft_atomic::atomic_write(&root.join(SEEN), &json));
        if let Err(e) = kept {
            return Err(ListError::Unavailable(Box::new(Error::Io(e))));
        }
    }
    Ok(list)
}

/// The list's address: MDM's latest release's.
fn list_url() -> String {
    // The real app's tests only (feature `test-update`, never in a release): a list on a lab
    // server, signed with the test key MDM_TEST_UPDATE_KEY names (`signed_by_mdm`).
    #[cfg(feature = "test-update")]
    if let Ok(url) = std::env::var("MDM_TEST_FFMPEG_LIST") {
        return url;
    }
    LIST.to_owned()
}

/// The ffmpeg to use: the person's own (`chosen`, #51), else MDM's install in `dir`
/// (`dir/ffmpeg/current.json`), if any and still there; else the system's own. Its `exe` is
/// `ffmpeg(.exe)`; yt-dlp is given its folder.
pub fn installed(dir: &Path) -> Option<Installed> {
    let own = crate::chosen::chosen(dir, crate::chosen::Helper::Ffmpeg).or_else(|| own(dir));
    // Android: the APK's own (android.rs); none can be downloaded and run there.
    #[cfg(target_os = "android")]
    return own.or_else(crate::android::ffmpeg);
    #[cfg(not(target_os = "android"))]
    own.or_else(|| crate::system::installed("ffmpeg"))
}

/// MDM's own install in `dir`.
fn own(dir: &Path) -> Option<Installed> {
    std::fs::read(dir.join("ffmpeg").join("current.json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<Installed>(&text).ok())
        .filter(|i| i.exe.is_file())
}

/// Install the build MDM's signed list names into `dir` unless the installed one is as new, the
/// zip downloaded with MDM's engine (`net`), counted into `progress`.
pub async fn install_latest(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
) -> Result<Installed, Error> {
    // The person's own copy: theirs to update (#51).
    if let Some(mine) = crate::chosen::chosen(dir, crate::chosen::Helper::Ffmpeg) {
        return Ok(mine);
    }
    install_with(dir, net, progress, None).await?;
    own(dir).ok_or_else(|| Error::Missing("ffmpeg after its install".into()))
}

/// [`install_latest`] for `autoupdate`: the build the list names, installed when it is newer than
/// the version in use and than `floor` (a version the person went back from), switched to now or
/// once the runs using ffmpeg end.
pub(crate) async fn install_with(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
    floor: Option<&str>,
) -> Result<Got, Error> {
    let list = list_url();
    let source = Source {
        list: &list,
        signed: &crate::signed_by_mdm,
    };
    let system = (std::env::consts::OS, std::env::consts::ARCH);
    install_from(dir, net, progress, floor, &source, system).await
}

/// [`install_with`] from the list at `source`, for `system` (its OS and processor, as Rust names
/// them).
async fn install_from(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
    floor: Option<&str>,
    source: &Source<'_>,
    (os, arch): (&str, &str),
) -> Result<Got, Error> {
    if build_for(os, arch).is_none() {
        return Err(Error::Missing(if cfg!(windows) {
            "ffmpeg build for this processor (choose your own ffmpeg in Helpers)".into()
        } else {
            "ffmpeg build for this system (install ffmpeg)".into()
        }));
    }
    let root = dir.join("ffmpeg");
    let _installing = crate::autoupdate::installing(&root, "ffmpeg")?;
    let list = checked_list(&root, net, source)
        .await
        .map_err(Error::FfmpegList)?;
    let build = list.build_for(os, arch).map_err(Error::FfmpegList)?;
    let version = build.version.clone();
    // Never the same again, never older, never at or below `floor`.
    if own(dir).is_some_and(|c| !newer(&version, &c.version))
        || floor.is_some_and(|f| !newer(&version, f))
    {
        return Ok(Got {
            latest: version,
            settled: None,
        });
    }
    // Already here (staged, or kept after going back): switched to, not downloaded again.
    if let Some(here) = crate::autoupdate::kept(&root, &version) {
        let settled = Some(crate::autoupdate::settle(&root, here)?);
        return Ok(Got {
            latest: version,
            settled,
        });
    }
    // Download to a file beside the install (about 80 MB), checked against the list's SHA-256.
    std::fs::create_dir_all(&root)?;
    // Named by its checksum: one build's cut-off download resumes, never another's.
    let zip_path = root.join(format!("ffmpeg-{}.zip", hex::encode(&build.sha256[..8])));
    match crate::fetch_verified(&build.url, &zip_path, build.sha256, net, progress).await {
        Err(Error::BadChecksum) => return Err(Error::FfmpegList(ListError::NotListed)),
        got => got?,
    }
    let res = unpack(&zip_path, &root, &version, &build.url);
    let _ = std::fs::remove_file(&zip_path);
    let exe = res?;
    tracing::info!("ffmpeg {version} verified against MDM's signed list and installed");
    let settled = Some(crate::autoupdate::settle(
        &root,
        Installed {
            version: version.clone(),
            exe,
        },
    )?);
    Ok(Got {
        latest: version,
        settled,
    })
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

    fn line(name: &str) -> Option<&str> {
        stable_line(name, "win64")
    }

    #[test]
    fn picks_stable_lgpl_shared_builds_only() {
        assert_eq!(
            line("ffmpeg-n9.0-latest-win64-lgpl-shared-9.0.zip"),
            Some("9.0")
        );
        assert_eq!(line("ffmpeg-n9.0-latest-win64-lgpl-9.0.zip"), None);
        assert_eq!(line("ffmpeg-master-latest-win64-lgpl-shared.zip"), None);
        assert_eq!(line("ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"), None);
        assert_eq!(line("ffmpeg-n8.1-latest-win64-lgpl-shared-9.0.zip"), None);
        // A dated build's names (what GitHub calls "latest" while BtbN remakes its own).
        assert_eq!(
            line("ffmpeg-n9.0.2-47-gb204e94e90-win64-lgpl-shared-9.0.zip"),
            Some("9.0")
        );
        assert_eq!(
            line("ffmpeg-n8.1.3-34-ga35157abb9-win64-lgpl-shared-8.1.zip"),
            Some("8.1")
        );
        for other in [
            "ffmpeg-N-128406-g1fe6854fd9-win64-lgpl-shared.zip",
            "ffmpeg-n9.0.2-47-gb204e94e90-win64-lgpl-9.0.zip",
            "ffmpeg-n9.0.2-47-gb204e94e90-win64-gpl-shared-9.0.zip",
            "ffmpeg-n9.0.2-47-gb204e94e90-linux64-lgpl-shared-9.0.tar.xz",
            "ffmpeg-n8.1.3-34-ga35157abb9-win64-lgpl-shared-9.0.zip",
            "ffmpeg-n9.0.2-x-gzz-win64-lgpl-shared-9.0.zip",
            // Windows on ARM's: not x64's.
            "ffmpeg-n9.0-latest-winarm64-lgpl-shared-9.0.zip",
        ] {
            assert_eq!(line(other), None, "{other}");
        }
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

    /// Each processor takes BtbN's build for it, never another's: x64's on x64, ARM64's on
    /// Windows on ARM; 32-bit Windows (BtbN builds none for it) and other systems none.
    #[test]
    fn each_processor_takes_its_own_build() {
        assert_eq!(build_for("windows", "x86_64"), Some("win64"));
        assert_eq!(build_for("windows", "aarch64"), Some("winarm64"));
        assert_eq!(build_for("windows", "x86"), None);
        assert_eq!(build_for("linux", "x86_64"), None);
        assert_eq!(build_for("macos", "aarch64"), None);
        assert_eq!(available(), build().is_some());
        assert_eq!(
            stable_line(
                "ffmpeg-n9.0.2-57-gd838f9f9ae-winarm64-lgpl-shared-9.0.zip",
                "winarm64"
            ),
            Some("9.0")
        );
        // The list's build for each processor is that processor's.
        let list = parse(&list_text(
            1,
            &[
                (
                    "x86_64",
                    &[1; 32],
                    "https://x/ffmpeg-n9.0.2-1-gabc-win64-lgpl-shared-9.0.zip",
                ),
                (
                    "aarch64",
                    &[2; 32],
                    "https://x/ffmpeg-n9.0.2-1-gabc-win64-lgpl-shared-9.0.zip",
                ),
            ],
        ))
        .unwrap();
        assert_eq!(list.build_for("windows", "x86_64").unwrap().sha256, [1; 32]);
        // x64's file named as ARM64's build: refused.
        assert!(matches!(
            list.build_for("windows", "aarch64"),
            Err(ListError::Unreadable(_))
        ));
        assert!(matches!(
            list.build_for("windows", "x86"),
            Err(ListError::NoBuild(_))
        ));
    }

    /// A list naming a build for each of `builds` (processor, SHA-256, address), Windows', version
    /// 9.0.20261031, with this sequence number.
    fn list_text(sequence: u64, builds: &[(&str, &[u8; 32], &str)]) -> String {
        let mut text = format!("# A test list\n{FORMAT}\nsequence {sequence}\n");
        for (arch, sum, url) in builds {
            text += &format!(
                "build windows {arch} 9.0.20261031 {} {url}\n",
                hex::encode(sum)
            );
        }
        text
    }

    /// The list release.yml signs and publishes with each release reads, and names an LGPL
    /// shared build for each processor MDM installs ffmpeg on, from a dated release of BtbN's
    /// (whose files stay; its `latest` release's change each day), at an address of GitHub's.
    #[test]
    fn the_list_a_release_publishes_reads() {
        let list = parse(include_str!("../../../tools/release/ffmpeg-builds.txt")).unwrap();
        assert!(list.sequence > 0);
        for arch in ["x86_64", "aarch64"] {
            let build = list.build_for("windows", arch).unwrap();
            let (repo, tag) = build
                .url
                .strip_prefix("https://github.com/")
                .and_then(|rest| rest.strip_suffix(&format!("/{}", build.name)))
                .and_then(|rest| rest.split_once("/releases/download/"))
                .unwrap();
            assert!(repo.ends_with("/FFmpeg-Builds"), "{repo}");
            assert!(tag.starts_with("autobuild-"), "{tag}");
        }
    }

    /// What isn't a list of this format, or a line of it that isn't as it should be, is
    /// refused whole; comments, blank lines and lines of a kind a later MDM adds are passed over.
    #[test]
    fn lists_that_cant_be_read() {
        let good = list_text(7, &[("x86_64", &[1; 32], "https://x/a.zip")]);
        let list = parse(&format!("{good}\n# note\nsigned-by someone\n")).unwrap();
        assert_eq!(list.sequence, 7);
        assert_eq!(list.builds[0].name, "a.zip");
        let sum = hex::encode([1; 32]);
        for bad in [
            String::new(),
            "mdm-ffmpeg-builds 2\nsequence 1\n".into(),
            format!("sequence 1\n{FORMAT}\n"),
            format!("{FORMAT}\n"),
            format!("{FORMAT}\nsequence x\n"),
            format!("{FORMAT}\nsequence -1\n"),
            format!("{FORMAT}\nsequence 1\nsequence 2\n"),
            format!("{FORMAT}\nsequence 1\nbuild windows x86_64 9.0.1 {sum}\n"),
            format!("{FORMAT}\nsequence 1\nbuild windows x86_64 9.0.x {sum} https://x/a.zip\n"),
            format!("{FORMAT}\nsequence 1\nbuild windows x86_64 ../9 {sum} https://x/a.zip\n"),
            format!(
                "{FORMAT}\nsequence 1\nbuild windows x86_64 9.0.1 {} https://x/a.zip\n",
                &sum[2..]
            ),
            format!("{FORMAT}\nsequence 1\nbuild windows x86_64 9.0.1 {sum} file:///C:/a.zip\n"),
            format!("{FORMAT}\nsequence 1\nbuild windows x86_64 9.0.1 {sum} https://x/\n"),
            format!(
                "{FORMAT}\nsequence 1\nbuild windows x86_64 9.0.1 {sum} https://x/a.zip\nbuild windows x86_64 9.0.1 {sum} https://x/b.zip\n"
            ),
        ] {
            assert!(
                matches!(parse(&bad), Err(ListError::Unreadable(_))),
                "{bad:?}"
            );
        }
    }

    /// A zip of BtbN's layout holding `exe` as its ffmpeg.exe.
    fn zip_of(path: &Path, exe: &str) {
        use std::io::Write;
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("ffmpeg-n9.0/bin/ffmpeg.exe", exe),
            ("ffmpeg-n9.0/bin/avcodec-62.dll", "dll"),
            ("ffmpeg-n9.0/LICENSE.txt", "lic"),
        ] {
            w.start_file(name, opts).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    /// [`install_from`] for Windows on `arch`, from the list at `list` checked with `signed`.
    async fn install(
        dir: &Path,
        list: &str,
        arch: &str,
        signed: &(dyn Fn(&[u8], &[u8]) -> bool + Sync),
    ) -> Result<Got, Error> {
        let net = crate::net(mdm_engine::Proxy::None);
        let source = Source { list, signed };
        install_from(dir, &net, None, None, &source, ("windows", arch)).await
    }

    /// End to end against a lab server: a list signed by a key made here (as MDM's release key
    /// signs the real one) and a zip it names are installed; a zip that isn't the one the list
    /// names, a list signed by another key, an older list than one already seen and no list at
    /// all are refused, and nothing of them is installed.
    #[tokio::test]
    async fn installs_only_a_build_a_signed_list_names() {
        let sum = |data: &[u8]| -> [u8; 32] { <sha2::Sha256 as sha2::Digest>::digest(data).into() };
        let files = tempfile::tempdir().unwrap();
        let f = files.path();
        let x64 = "ffmpeg-n9.0.2-39-g7adfbe40dc-win64-lgpl-shared-9.0.zip";
        let arm = "ffmpeg-n9.0.2-39-g7adfbe40dc-winarm64-lgpl-shared-9.0.zip";
        zip_of(&f.join(x64), "the checked build");
        zip_of(&f.join(arm), "a build nobody checked");
        let lab = mdm_labserver::LabServer::start(mdm_labserver::Config {
            files: Some(f.to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
        let key = crate::openpgp::TestKey::new();
        let other = crate::openpgp::TestKey::new();
        let public = key.public_armored();
        let signed = move |data: &[u8], sig: &[u8]| crate::signed_by(&public, data, sig);
        // `name` and its signature by `by`.
        let publish = |name: &str, text: &str, by: &crate::openpgp::TestKey| {
            std::fs::write(f.join(name), text).unwrap();
            std::fs::write(f.join(format!("{name}.sig")), by.sign(text.as_bytes())).unwrap();
            lab.url(&format!("/files/{name}"))
        };
        let x64_sum = sum(&std::fs::read(f.join(x64)).unwrap());
        // The ARM64 line names another zip's SHA-256 than the one served.
        let good = list_text(
            5,
            &[
                ("x86_64", &x64_sum, &lab.url(&format!("/files/{x64}"))),
                ("aarch64", &[7; 32], &lab.url(&format!("/files/{arm}"))),
            ],
        );

        // Accepted: the list's signature is the key's, and the zip is the one it names.
        let tools = tempfile::tempdir().unwrap();
        let d = tools.path();
        let list = publish("good.txt", &good, &key);
        let got = install(d, &list, "x86_64", &signed).await.unwrap();
        assert_eq!(got.latest, "9.0.20261031");
        assert!(got.settled.is_some());
        let now = own(d).unwrap();
        assert_eq!(now.version, "9.0.20261031");
        assert_eq!(
            std::fs::read_to_string(&now.exe).unwrap(),
            "the checked build"
        );
        assert_eq!(seen(&d.join("ffmpeg")), 5);
        // The same list again: nothing new to install.
        let again = install(d, &list, "x86_64", &signed).await.unwrap();
        assert!(again.settled.is_none());

        // A zip that isn't the one the list names: refused, and not kept.
        let arm_tools = tempfile::tempdir().unwrap();
        let err = install(arm_tools.path(), &list, "aarch64", &signed).await;
        assert!(
            matches!(err, Err(Error::FfmpegList(ListError::NotListed))),
            "{err:?}"
        );
        assert!(own(arm_tools.path()).is_none());
        let zips: Vec<_> = std::fs::read_dir(arm_tools.path().join("ffmpeg"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".zip"))
            .collect();
        assert!(zips.is_empty(), "{zips:?}");

        // Signed by another key: refused, wherever it came from.
        let fresh = tempfile::tempdir().unwrap();
        let forged = publish("forged.txt", &good, &other);
        let err = install(fresh.path(), &forged, "x86_64", &signed).await;
        assert!(
            matches!(err, Err(Error::FfmpegList(ListError::Unsigned))),
            "{err:?}"
        );
        // A signature of another text: refused too.
        std::fs::write(
            f.join("forged.txt"),
            good.replace("sequence 5", "sequence 9"),
        )
        .unwrap();
        std::fs::write(f.join("forged.txt.sig"), key.sign(good.as_bytes())).unwrap();
        let err = install(fresh.path(), &forged, "x86_64", &signed).await;
        assert!(
            matches!(err, Err(Error::FfmpegList(ListError::Unsigned))),
            "{err:?}"
        );
        assert!(own(fresh.path()).is_none());

        // Older than the list seen before (5): refused, though signed.
        let old = publish("old.txt", &good.replace("sequence 5", "sequence 4"), &key);
        let err = install(d, &old, "x86_64", &signed).await;
        assert!(
            matches!(
                err,
                Err(Error::FfmpegList(ListError::Older { got: 4, seen: 5 }))
            ),
            "{err:?}"
        );
        assert_eq!(seen(&d.join("ffmpeg")), 5);

        // No list (or no signature): nothing installed.
        let err = install(fresh.path(), &lab.url("/files/none.txt"), "x86_64", &signed).await;
        assert!(
            matches!(err, Err(Error::FfmpegList(ListError::Unavailable(_)))),
            "{err:?}"
        );
        std::fs::write(f.join("unsigned.txt"), &good).unwrap();
        let err = install(
            fresh.path(),
            &lab.url("/files/unsigned.txt"),
            "x86_64",
            &signed,
        )
        .await;
        assert!(
            matches!(err, Err(Error::FfmpegList(ListError::Unavailable(_)))),
            "{err:?}"
        );
        assert!(own(fresh.path()).is_none());
    }
}

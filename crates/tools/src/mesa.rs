//! Mesa's software OpenGL, llvmpipe (#476), for a Windows computer with no graphics card's driver
//! (Windows Server, a virtual machine, Remote Desktop): there MDM would draw with Direct3D 12 on
//! WARP, Windows' own software renderer, which keeps the processor far busier for the same
//! windows. MDM doesn't ship Mesa: it offers it, and downloads it only when the person says yes.
//!
//! Which build, its size and the SHA-256 of the archive and of each file MDM takes from it come
//! from MDM's signed list of the helper builds its maintainers checked (`ffmpeg-builds.txt`, its
//! `mesa` lines: `tools/release/ffmpeg_list.py mesa`), as ffmpeg's do ([`crate::ffmpeg`]): the list
//! must carry MDM's release key's signature and be no older than the newest one seen here (kept in
//! `tools/mesa/builds-list.json`). The archive is pal1000/mesa-dist-win's release (MIT, built from
//! Mesa's own MIT code and LLVM's); its files are unpacked into `tools/mesa/<version>/` only when
//! each one's SHA-256 is the list's, then made current (`tools/mesa/current.json`). Nothing about
//! the build is in MDM itself: a new Mesa is a new list.
//!
//! The app loads `opengl32.dll` from there before OpenGL starts (renderer.rs, win32.rs).

use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use crate::ffmpeg::{ListError, Source};
use crate::{Error, Installed};

/// Its folder in the tools folder.
pub const FOLDER: &str = "mesa";
/// The file the app loads: Mesa's OpenGL, named as Windows' own is. The others in the list sit
/// beside it (pal1000's `libgallium_wgl.dll`, which it loads from its own folder).
pub const OPENGL: &str = "opengl32.dll";

/// One build the list names (a `mesa` line). Its archive's checksum and address, and its files,
/// are read by the install, Windows' only; the list reads the same on every system.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Build {
    /// The system and the processor, as Rust names them (`windows`, `x86_64`).
    pub(crate) os: String,
    pub(crate) arch: String,
    /// Mesa's version (`26.3.1`).
    pub version: String,
    /// The archive's SHA-256, its size in bytes (the offer says it before anything is
    /// downloaded), and where it is downloaded from.
    sha256: [u8; 32],
    pub size: u64,
    url: String,
    /// The files taken from it: their path in the archive and their SHA-256, `opengl32.dll`'s
    /// first.
    files: Vec<(String, [u8; 32])>,
}

impl Build {
    /// A `mesa` line's words after its kind:
    /// `<os> <arch> <version> <sha256> <size> <address> <path>=<sha256>…`. Each path is one in
    /// the archive (`x64/opengl32.dll`) whose last part is a plain DLL name, the first
    /// `opengl32.dll`; no two of them end in the same name (they are unpacked side by side).
    pub(crate) fn parse(words: &[&str]) -> Result<Build, String> {
        let [os, arch, version, sum, size, url, files @ ..] = words else {
            return Err(format!("Mesa line {:?}", words.join(" ")));
        };
        let numeric = version
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if !numeric {
            return Err(format!("Mesa version {version:?}"));
        }
        let sha = |s: &str| -> Result<[u8; 32], String> {
            hex::decode(s)
                .ok()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| format!("SHA-256 {s:?}"))
        };
        let size = size
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
            .ok_or_else(|| format!("size {size:?}"))?;
        let named = url::Url::parse(url)
            .ok()
            .filter(|u| matches!(u.scheme(), "https" | "http"))
            .and_then(|u| Some(u.path_segments()?.next_back()?.to_owned()))
            .is_some_and(|n| !n.is_empty());
        if !named {
            return Err(format!("address {url:?}"));
        }
        let mut taken: Vec<(String, [u8; 32])> = Vec::new();
        for f in files {
            let (path, sum) = f
                .split_once('=')
                .ok_or_else(|| format!("Mesa file {f:?}"))?;
            let name = file_name(path).ok_or_else(|| format!("Mesa file {path:?}"))?;
            if taken
                .iter()
                .any(|(p, _)| file_name(p).is_some_and(|n| n.eq_ignore_ascii_case(name)))
            {
                return Err(format!("two Mesa files named {name}"));
            }
            taken.push((path.to_owned(), sha(sum)?));
        }
        let first = taken.first().and_then(|(p, _)| file_name(p));
        if !first.is_some_and(|n| n.eq_ignore_ascii_case(OPENGL)) {
            return Err(format!("the Mesa line's first file isn't {OPENGL}"));
        }
        Ok(Build {
            os: (*os).into(),
            arch: (*arch).into(),
            version: (*version).into(),
            sha256: sha(sum)?,
            size,
            url: (*url).into(),
            files: taken,
        })
    }
}

/// The plain file name a path in the archive ends in, when every part of it is a plain name
/// (no `..`, no drive, no `\`) and the name is a DLL's of letters, digits, `.`, `_` and `-`:
/// what can be written in the version's folder and nowhere else.
fn file_name(path: &str) -> Option<&str> {
    let plain = |p: &str| !p.is_empty() && p != "." && p != "..";
    let parts: Vec<&str> = path.split('/').collect();
    if !parts.iter().all(|p| plain(p) && !p.contains(['\\', ':'])) {
        return None;
    }
    let name = *parts.last()?;
    let chars_ok = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let dll = name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".dll");
    (chars_ok && dll && !name.starts_with('.')).then_some(name)
}

/// The installed Mesa in the tools folder `dir`, if any and still there: its version, and its
/// `opengl32.dll` as [`Installed::exe`].
pub fn installed(dir: &Path) -> Option<Installed> {
    std::fs::read(dir.join(FOLDER).join("current.json"))
        .ok()
        .and_then(|text| serde_json::from_slice::<Installed>(&text).ok())
        .filter(|i| i.exe.is_file())
}

/// What MDM's signed list offers for this system (its version and download size), fetched and
/// checked as the install would; [`ListError::NoBuild`] where it offers none.
pub async fn offer(dir: &Path, net: &mdm_engine::Options) -> Result<Build, Error> {
    let list = crate::ffmpeg::list_url();
    let source = Source {
        list: &list,
        signed: &crate::signed_by_mdm,
    };
    let system = (std::env::consts::OS, std::env::consts::ARCH);
    offer_from(dir, net, &source, system).await
}

async fn offer_from(
    dir: &Path,
    net: &mdm_engine::Options,
    source: &Source<'_>,
    (os, arch): (&str, &str),
) -> Result<Build, Error> {
    let list = crate::ffmpeg::checked_list(&dir.join(FOLDER), net, source)
        .await
        .map_err(Error::MesaList)?;
    list.mesa
        .into_iter()
        .find(|b| b.os == os && b.arch == arch)
        .ok_or_else(|| Error::MesaList(ListError::NoBuild(format!("{os} {arch}"))))
}

/// Download, check and install the build MDM's signed list names for this system into the tools
/// folder `dir` (a click's: never by itself), counted into `progress`. The one installed already
/// when it is that version. Windows only: Linux and macOS have their own software OpenGL.
#[cfg(windows)]
pub async fn install(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
) -> Result<Installed, Error> {
    let list = crate::ffmpeg::list_url();
    let source = Source {
        list: &list,
        signed: &crate::signed_by_mdm,
    };
    let system = (std::env::consts::OS, std::env::consts::ARCH);
    install_from(dir, net, progress, &source, system).await
}

#[cfg(windows)]
async fn install_from(
    dir: &Path,
    net: &mdm_engine::Options,
    progress: Option<&crate::update::Progress>,
    source: &Source<'_>,
    system: (&str, &str),
) -> Result<Installed, Error> {
    let root = dir.join(FOLDER);
    let _installing = crate::autoupdate::installing(&root, "Mesa")?;
    let build = offer_from(dir, net, source, system).await?;
    if let Some(now) = installed(dir).filter(|i| i.version == build.version) {
        return Ok(now);
    }
    std::fs::create_dir_all(&root)?;
    // Named by its checksum: one build's cut-off download resumes, never another's.
    let archive = root.join(format!("mesa-{}.7z", hex::encode(&build.sha256[..8])));
    match crate::fetch_verified(&build.url, &archive, build.sha256, net, progress).await {
        Err(Error::BadChecksum) => return Err(Error::MesaList(ListError::NotListed)),
        got => got?,
    }
    // Unpacking is seconds of the processor's work: off the runtime's threads.
    let unpacked = {
        let (archive, root, build) = (archive.clone(), root.clone(), build.clone());
        tokio::task::spawn_blocking(move || unpack(&archive, &root, &build)).await
    };
    let _ = std::fs::remove_file(&archive);
    let dll = unpacked.map_err(|e| Error::Io(std::io::Error::other(e)))??;
    tracing::info!(
        "Mesa {} verified against MDM's signed list and installed",
        build.version
    );
    let new = Installed {
        version: build.version.clone(),
        exe: dll,
    };
    match crate::autoupdate::settle(&root, new)? {
        crate::autoupdate::Settled::Switched(i) | crate::autoupdate::Settled::Staged(i) => Ok(i),
    }
}

/// The build's files, from the 7z archive at `archive`, into `root/<version>/`: each one checked
/// against its SHA-256 in the list before it takes its name (a file that isn't the list's is
/// removed, and nothing is installed). Its `opengl32.dll`.
#[cfg(windows)]
fn unpack(archive: &Path, root: &Path, build: &Build) -> Result<PathBuf, Error> {
    use sha2::Digest as _;
    use std::io::Write as _;
    let bad = |e: sevenz_rust2::Error| Error::BadRelease(format!("Mesa's archive: {e}"));
    let folder = root.join(&build.version);
    std::fs::create_dir_all(&folder)?;
    let mut reader =
        sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty()).map_err(bad)?;
    // Only the blocks holding the files wanted are decoded (pal1000's archive keeps its 32-bit
    // files in a block of their own).
    let path_of = |p: &str| p.replace('\\', "/");
    let wanted = |name: &str| build.files.iter().position(|(p, _)| *p == path_of(name));
    let mut found: Vec<Option<PathBuf>> = vec![None; build.files.len()];
    let mut failed: Option<Error> = None;
    reader
        .for_each_entries_with_decision(|entry, data| {
            use sevenz_rust2::EntryDecision;
            let Some(i) = wanted(entry.name()).filter(|_| !entry.is_directory()) else {
                return Ok(EntryDecision::Continue);
            };
            let (path, sum) = &build.files[i];
            let name = file_name(path).unwrap_or(OPENGL);
            let dest = folder.join(name);
            let part = folder.join(format!("{name}.part"));
            let mut out = std::fs::File::create(&part)?;
            let mut sha = sha2::Sha256::new();
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = data.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                sha.update(&buf[..n]);
                out.write_all(&buf[..n])?;
            }
            drop(out);
            if sha.finalize().as_slice() != sum {
                let _ = std::fs::remove_file(&part);
                failed = Some(Error::MesaList(ListError::NotListed));
                return Ok(EntryDecision::Stop);
            }
            std::fs::rename(&part, &dest)?;
            found[i] = Some(dest);
            Ok(if found.iter().all(Option::is_some) {
                EntryDecision::Stop
            } else {
                EntryDecision::Continue
            })
        })
        .map_err(bad)?;
    if let Some(e) = failed {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(e);
    }
    if let Some(i) = found.iter().position(Option::is_none) {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(Error::Missing(format!(
            "{} in Mesa's archive",
            build.files[i].0
        )));
    }
    Ok(found.swap_remove(0).unwrap_or_else(|| folder.join(OPENGL)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn sum(data: &[u8]) -> [u8; 32] {
        <sha2::Sha256 as sha2::Digest>::digest(data).into()
    }

    /// A `mesa` line's words for `files` (path, SHA-256).
    fn line(url: &str, archive: &[u8; 32], files: &[(&str, [u8; 32])]) -> String {
        let mut s = format!(
            "mesa windows x86_64 26.3.1 {} 66270373 {url}",
            hex::encode(archive)
        );
        for (p, h) in files {
            s += &format!(" {p}={}", hex::encode(h));
        }
        s
    }

    fn parse_line(text: &str) -> Result<Build, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        Build::parse(&words[1..])
    }

    /// A line reads with its files, `opengl32.dll` first; anything else in it is refused.
    #[test]
    fn mesa_lines_that_read_and_that_dont() {
        let s = [1u8; 32];
        let good = line(
            "https://github.com/pal1000/mesa-dist-win/releases/download/26.3.1/mesa3d-26.3.1-release-msvc.7z",
            &s,
            &[("x64/opengl32.dll", s), ("x64/libgallium_wgl.dll", s)],
        );
        let b = parse_line(&good).unwrap();
        assert_eq!((b.version.as_str(), b.size), ("26.3.1", 66_270_373));
        assert_eq!(b.files.len(), 2);
        let hex = hex::encode(s);
        for bad in [
            // No file, or opengl32.dll not first.
            line("https://x/a.7z", &s, &[]),
            line("https://x/a.7z", &s, &[("x64/libgallium_wgl.dll", s)]),
            // A path that climbs out, a drive, a backslash, a name that isn't a DLL's.
            line("https://x/a.7z", &s, &[("../opengl32.dll", s)]),
            line("https://x/a.7z", &s, &[("x64/../../opengl32.dll", s)]),
            line("https://x/a.7z", &s, &[("C:/opengl32.dll", s)]),
            line("https://x/a.7z", &s, &[("x64\\opengl32.dll", s)]),
            line("https://x/a.7z", &s, &[("x64/opengl32.exe", s)]),
            line("https://x/a.7z", &s, &[("x64/con .dll", s)]),
            // Two files of the same name.
            line(
                "https://x/a.7z",
                &s,
                &[("x64/opengl32.dll", s), ("x86/OPENGL32.dll", s)],
            ),
            // Not an address of the web's.
            line("file:///C:/a.7z", &s, &[("x64/opengl32.dll", s)]),
            line("https://x/", &s, &[("x64/opengl32.dll", s)]),
            format!("mesa windows x86_64 26.3.x {hex} 1 https://x/a.7z x64/opengl32.dll={hex}"),
            format!("mesa windows x86_64 26.3.1 {hex} 0 https://x/a.7z x64/opengl32.dll={hex}"),
            format!("mesa windows x86_64 26.3.1 {hex} big https://x/a.7z x64/opengl32.dll={hex}"),
            format!(
                "mesa windows x86_64 26.3.1 {hex} 1 https://x/a.7z x64/opengl32.dll={}",
                &hex[2..]
            ),
            format!("mesa windows x86_64 26.3.1 {hex} 1 https://x/a.7z x64/opengl32.dll"),
            "mesa windows x86_64".into(),
        ] {
            assert!(parse_line(&bad).is_err(), "{bad}");
        }
    }

    /// The list release.yml signs reads with its Mesa line: x64's, from pal1000's releases on
    /// GitHub, opengl32.dll and the gallium driver beside it.
    #[test]
    fn the_list_a_release_publishes_offers_mesa_on_x64() {
        let list =
            crate::ffmpeg::parse(include_str!("../../../tools/release/ffmpeg-builds.txt")).unwrap();
        let b = list
            .mesa
            .iter()
            .find(|b| b.os == "windows" && b.arch == "x86_64")
            .unwrap();
        assert!(
            b.url
                .starts_with("https://github.com/pal1000/mesa-dist-win/releases/download/"),
            "{}",
            b.url
        );
        let names: Vec<&str> = b.files.iter().filter_map(|(p, _)| file_name(p)).collect();
        assert_eq!(names, ["opengl32.dll", "libgallium_wgl.dll"]);
    }

    /// A list with a `mesa` line: an MDM that reads only ffmpeg's lines passes over it, and a
    /// second line for the same system makes it unreadable.
    #[test]
    fn mesa_lines_beside_ffmpegs() {
        let s = [3u8; 32];
        let m = line("https://x/a.7z", &s, &[("x64/opengl32.dll", s)]);
        let text = format!("mdm-ffmpeg-builds 1\nsequence 3\n{m}\n");
        assert_eq!(crate::ffmpeg::parse(&text).unwrap().mesa.len(), 1);
        let twice = format!("{text}{m}\n");
        assert!(crate::ffmpeg::parse(&twice).is_err());
    }

    /// End to end against a lab server, from a real 7z archive (BCJ2 and LZMA, solid, two blocks:
    /// as pal1000's is, made by 7-Zip): a list signed by a key made here and an archive it names
    /// are installed, only the files it names, each checked; a file whose SHA-256 isn't the
    /// list's, an archive that isn't the list's, and another key's signature are refused, and
    /// nothing of them is installed.
    #[cfg(windows)]
    #[tokio::test]
    async fn installs_only_what_a_signed_list_names() {
        const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/mesa-fixture.7z");
        let files = tempfile::tempdir().unwrap();
        let f = files.path();
        std::fs::write(f.join("mesa.7z"), FIXTURE).unwrap();
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
        let publish = |name: &str, text: &str, by: &crate::openpgp::TestKey| {
            std::fs::write(f.join(name), text).unwrap();
            std::fs::write(f.join(format!("{name}.sig")), by.sign(text.as_bytes())).unwrap();
            lab.url(&format!("/files/{name}"))
        };
        let gl = sum(b"fake opengl32 for MDM's tests\n");
        let gallium = sum(b"fake gallium for MDM's tests\n");
        let archive = sum(FIXTURE);
        let url = lab.url("/files/mesa.7z");
        let list_of = |seq: u64, archive: &[u8; 32], gl: [u8; 32]| {
            let m = line(
                &url,
                archive,
                &[
                    ("x64/opengl32.dll", gl),
                    ("x64/libgallium_wgl.dll", gallium),
                ],
            );
            format!("mdm-ffmpeg-builds 1\nsequence {seq}\n{m}\n")
        };
        let net = crate::net(mdm_engine::Proxy::None);
        let install = |dir: PathBuf, list: String| {
            let signed = signed.clone();
            let net = net.clone();
            async move {
                let source = Source {
                    list: &list,
                    signed: &signed,
                };
                install_from(&dir, &net, None, &source, ("windows", "x86_64")).await
            }
        };

        // Accepted: both files, and only them (the archive's 32-bit and other files stay out).
        let tools = tempfile::tempdir().unwrap();
        let d = tools.path().to_owned();
        let list = publish("good.txt", &list_of(5, &archive, gl), &key);
        let got = install(d.clone(), list.clone()).await.unwrap();
        assert_eq!(got.version, "26.3.1");
        assert_eq!(installed(&d).unwrap(), got);
        let folder = got.exe.parent().unwrap();
        assert_eq!(
            std::fs::read(folder.join("opengl32.dll")).unwrap(),
            b"fake opengl32 for MDM's tests\n"
        );
        assert!(folder.join("libgallium_wgl.dll").is_file());
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["libgallium_wgl.dll", "opengl32.dll"]);
        // No archive left behind.
        assert!(
            !std::fs::read_dir(d.join(FOLDER))
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(".7z"))
        );
        // Offered from the same list: its version and size.
        let source = Source {
            list: &list,
            signed: &signed,
        };
        let offered = offer_from(&d, &net, &source, ("windows", "x86_64"))
            .await
            .unwrap();
        assert_eq!(offered.size, 66_270_373);
        let none = offer_from(&d, &net, &source, ("windows", "aarch64")).await;
        assert!(
            matches!(none, Err(Error::MesaList(ListError::NoBuild(_)))),
            "{none:?}"
        );

        // A file in the archive that isn't the one the list names: refused, nothing installed.
        let fresh = tempfile::tempdir().unwrap();
        let wrong = publish("wrong-file.txt", &list_of(5, &archive, [9; 32]), &key);
        let err = install(fresh.path().to_owned(), wrong).await;
        assert!(
            matches!(err, Err(Error::MesaList(ListError::NotListed))),
            "{err:?}"
        );
        assert!(installed(fresh.path()).is_none());
        assert!(!fresh.path().join(FOLDER).join("26.3.1").exists());

        // An archive that isn't the list's (its SHA-256): refused, and not kept.
        let wrong = publish("wrong-archive.txt", &list_of(5, &[7; 32], gl), &key);
        let err = install(fresh.path().to_owned(), wrong).await;
        assert!(
            matches!(err, Err(Error::MesaList(ListError::NotListed))),
            "{err:?}"
        );
        assert!(installed(fresh.path()).is_none());
        assert!(
            !std::fs::read_dir(fresh.path().join(FOLDER))
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(".7z"))
        );

        // Signed by another key: refused.
        let forged = publish("forged.txt", &list_of(5, &archive, gl), &other);
        let err = install(fresh.path().to_owned(), forged).await;
        assert!(
            matches!(err, Err(Error::MesaList(ListError::Unsigned))),
            "{err:?}"
        );
        assert!(installed(fresh.path()).is_none());
    }
}

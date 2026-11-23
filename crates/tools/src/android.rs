//! The helpers on Android. Android 10 and later run no program from a file the app wrote itself, only
//! those its APK installs (`nativeLibraryDir`), so the programs ship in the APK as `lib*.so`
//! files (the Android app's build takes them from youtubedl-android, GPL-3.0):
//!
//! - `libpython.so`, Python 3.12, with its standard library and libraries in `libpython.zip.so`;
//! - `libffmpeg.so` and `libffprobe.so`, their libraries in `libffmpeg.zip.so`;
//! - `libqjs.so`, QuickJS, the JavaScript runtime YouTube's pages need (#852).
//!
//! The zips are unpacked once per APK into `<data>/helpers` (libraries are loaded from there; only
//! running a program from there is refused). yt-dlp itself is a Python zipapp, data rather than a
//! program: MDM installs and updates it as on Linux, checked against yt-dlp's signature, and runs
//! it with the bundled Python ([`command`]). `<data>/helpers/bin` holds `ffmpeg`, `ffprobe` and
//! `qjs`: links to the APK's programs under the names yt-dlp looks for.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Where the helpers are, once unpacked.
#[derive(Debug)]
pub struct Helpers {
    /// `nativeLibraryDir`: the APK's programs.
    pub lib: PathBuf,
    /// `<data>/helpers`.
    pub root: PathBuf,
    /// Temporary files.
    pub cache: PathBuf,
    /// What failed while unpacking, if anything (logged; the helpers then aren't usable).
    pub error: Option<String>,
}

static HELPERS: OnceLock<Helpers> = OnceLock::new();

impl Helpers {
    pub fn python(&self) -> PathBuf {
        self.lib.join("libpython.so")
    }
    fn python_home(&self) -> PathBuf {
        self.root.join("python/usr")
    }
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    /// `ffmpeg` under its own name (a link to the APK's `libffmpeg.so`).
    pub fn ffmpeg(&self) -> PathBuf {
        self.bin().join("ffmpeg")
    }
    pub fn qjs(&self) -> PathBuf {
        self.bin().join("qjs")
    }
    fn library_path(&self) -> String {
        let mut dirs = vec![
            self.python_home().join("lib"),
            self.root.join("ffmpeg/usr/lib"),
            self.lib.clone(),
        ];
        if let Some(old) = std::env::var_os("LD_LIBRARY_PATH") {
            dirs.extend(std::env::split_paths(&old));
        }
        std::env::join_paths(dirs)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Unpack the helpers (a thread of its own: tens of megabytes the first time after an install
/// or an update). `lib` is `nativeLibraryDir`, `data` the app's data folder, `cache` its cache.
/// [`helpers`] waits for it.
pub fn start(lib: &Path, data: &Path, cache: &Path) {
    let (lib, root, cache) = (lib.to_owned(), data.join("helpers"), cache.to_owned());
    let unpacking = {
        let (lib, root, cache) = (lib.clone(), root.clone(), cache.clone());
        std::thread::Builder::new()
            .name("helpers-unpack".into())
            .spawn(move || {
                let error = unpack(&lib, &root).err();
                if let Some(e) = &error {
                    tracing::warn!("helpers: {e}");
                } else {
                    tracing::info!("helpers ready in {}", root.display());
                }
                let _ = HELPERS.set(Helpers {
                    lib,
                    root,
                    cache,
                    error,
                });
            })
    };
    // No thread for it: the helpers are known unusable now, so [`helpers`] never waits.
    if let Err(e) = unpacking {
        let error = format!("no thread to unpack them: {e}");
        tracing::warn!("helpers: {error}");
        let _ = HELPERS.set(Helpers {
            lib,
            root,
            cache,
            error: Some(error),
        });
    }
}

/// The helpers, once unpacked (waits for [`start`]'s thread); None when never started.
pub fn helpers() -> Option<&'static Helpers> {
    if STARTED.get().is_none() && HELPERS.get().is_none() {
        return None;
    }
    Some(HELPERS.wait())
}

static STARTED: OnceLock<()> = OnceLock::new();

/// [`start`], remembered so [`helpers`] knows to wait.
pub fn init(lib: &Path, data: &Path, cache: &Path) {
    if STARTED.set(()).is_ok() {
        start(lib, data, cache);
    }
}

/// The stamp of what is unpacked: the zips' sizes and dates, so an updated APK unpacks again.
fn stamp(lib: &Path) -> String {
    ["libpython.zip.so", "libffmpeg.zip.so"]
        .iter()
        .map(|n| {
            let m = std::fs::metadata(lib.join(n)).ok();
            let len = m.as_ref().map_or(0, std::fs::Metadata::len);
            let at = m
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            format!("{n}:{len}:{at}")
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn unpack(lib: &Path, root: &Path) -> Result<(), String> {
    let want = stamp(lib);
    let stamp_file = root.join("unpacked");
    let fresh = std::fs::read_to_string(&stamp_file).is_ok_and(|s| s == want)
        && root.join("python/usr/lib").is_dir();
    if !fresh {
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        for (zip, into) in [
            ("libpython.zip.so", "python"),
            ("libffmpeg.zip.so", "ffmpeg"),
        ] {
            let path = lib.join(zip);
            if !path.is_file() {
                continue;
            }
            let file = std::fs::File::open(&path).map_err(|e| format!("{zip}: {e}"))?;
            let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{zip}: {e}"))?;
            archive
                .extract(root.join(into))
                .map_err(|e| format!("{zip}: {e}"))?;
        }
        std::fs::write(&stamp_file, &want).map_err(|e| e.to_string())?;
    }
    // The programs under the names yt-dlp asks for; made again each start (an update moves
    // nativeLibraryDir).
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    for (name, target) in [
        ("ffmpeg", "libffmpeg.so"),
        ("ffprobe", "libffprobe.so"),
        ("qjs", "libqjs.so"),
        ("python", "libpython.so"),
        // 7-Zip for 7z and RAR (crates/extract finds `7zz` on the PATH, which android.rs sets).
        ("7zz", "lib7zz.so"),
    ] {
        let link = bin.join(name);
        let _ = std::fs::remove_file(&link);
        if lib.join(target).is_file() {
            std::os::unix::fs::symlink(lib.join(target), &link).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Whether `exe` is a program the system runs itself (the APK's, or a link to one), rather than
/// a Python program (a zipapp, a script) the bundled Python runs.
fn native(h: &Helpers, exe: &Path) -> bool {
    let real = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_owned());
    real.starts_with(&h.lib)
        || real.starts_with("/system")
        || (!real.starts_with(&h.root) && elf(&real))
}

/// Whether the file at `path` is a program (an ELF file): its first 4 bytes.
fn elf(path: &Path) -> bool {
    use std::io::Read as _;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok_and(|()| &magic == b"\x7fELF")
}

/// The command that runs `exe` here: itself when it is one of the APK's programs, else the
/// bundled Python with `exe` (yt-dlp's zipapp, gallery-dl). Either way with the environment the
/// helpers need: their libraries, Python's home and certificates, `bin` first on the PATH.
pub fn command(exe: &Path) -> Command {
    let Some(h) = helpers() else {
        return Command::new(exe);
    };
    let mut cmd = if native(h, exe) {
        Command::new(exe)
    } else if exe.extension().is_some_and(|e| e == "whl") {
        // A wheel (gallery-dl's): imported from the archive itself, run as its module.
        let module = exe
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.split('-').next())
            .unwrap_or("gallery_dl")
            .to_owned();
        let mut c = Command::new(h.python());
        c.env("PYTHONPATH", exe).args(["-m", &module]);
        c
    } else {
        let mut c = Command::new(h.python());
        c.arg(exe);
        c
    };
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/system/bin".into());
    let mut dirs = vec![h.bin()];
    dirs.extend(std::env::split_paths(&path));
    cmd.env("LD_LIBRARY_PATH", h.library_path())
        .env("PYTHONHOME", h.python_home())
        .env("SSL_CERT_FILE", h.python_home().join("etc/tls/cert.pem"))
        .env("HOME", &h.root)
        .env("TMPDIR", &h.cache)
        .env("PATH", std::env::join_paths(dirs).unwrap_or(path));
    cmd
}

/// The bundled ffmpeg, as an installed helper (yt-dlp's `--ffmpeg-location`).
pub fn ffmpeg() -> Option<crate::Installed> {
    let h = helpers()?;
    (h.error.is_none() && h.ffmpeg().exists()).then(|| crate::Installed {
        version: "bundled".into(),
        exe: h.ffmpeg(),
    })
}

/// `--js-runtimes quickjs:<qjs>`: YouTube's pages need a JavaScript runtime (#852).
pub fn js_runtime_args() -> Option<Vec<String>> {
    let h = helpers()?;
    h.qjs().exists().then(|| {
        vec![
            "--js-runtimes".to_owned(),
            format!("quickjs:{}", h.qjs().display()),
        ]
    })
}

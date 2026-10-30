//! Choosing the local file name for a download.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use percent_encoding::percent_decode_str;
use url::Url;

/// File name from `Content-Disposition`, else the last URL path segment, else `download`.
/// Always safe to use as a single path component on Windows and Unix.
pub fn file_name(content_disposition: Option<&str>, url: &Url) -> String {
    let from_header = content_disposition
        .map(content_disposition::parse_content_disposition)
        .and_then(|cd| cd.filename_full());
    let from_url = || {
        url.path_segments()
            .and_then(|mut s| s.next_back())
            .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
    };
    let name = from_header.or_else(from_url).unwrap_or_default();
    // A header may carry a path; keep only its last component.
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    sanitize(name)
}

/// Make `name` a valid single file name on Windows (the strictest target).
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = clamp(cleaned.trim().trim_end_matches(['.', ' ']));
    if cleaned.is_empty() {
        return "download".to_owned();
    }
    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    // Windows' reserved device names, with or without an extension (COM¹ … use superscripts).
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.chars().count() == 4
        && stem
            .chars()
            .nth(3)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³')));
    if reserved {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

/// Longest file name we create, in UTF-16 units: NTFS allows 255, and the part file adds
/// ".mdm.part" (9) and a duplicate may add " (NNN)" (6). The extension is kept.
const MAX_NAME: usize = 230;

/// Longest full path we create, in UTF-16 units. On Windows: `MAX_PATH` (260) without the
/// terminating NUL, so Explorer and other programs can still open the file.
#[cfg(windows)]
const MAX_PATH_LEN: usize = 259;
#[cfg(not(windows))]
const MAX_PATH_LEN: usize = 4095;
/// What our files add to a destination: ".mdm.part" / ".mdm.json".
const SIDECAR: usize = 9;
/// What a duplicate's number adds: " (NNN)".
const DUPLICATE: usize = 6;
/// Shortest name worth making when a deep folder leaves little room.
const MIN_NAME: usize = 20;

fn utf16_len(s: &std::ffi::OsStr) -> usize {
    s.to_string_lossy().encode_utf16().count()
}

/// Whether `dest` and the part and state files beside it stay within the path limit.
pub fn path_fits(dest: &Path) -> bool {
    utf16_len(dest.as_os_str()) + SIDECAR <= MAX_PATH_LEN
}

/// `name`, shortened (keeping the extension) so that `dir/name`, its part and state files and a
/// duplicate's number stay within the path limit. None when `dir` alone leaves too little room.
pub fn fit_name(dir: &Path, name: &str) -> Option<String> {
    let room = MAX_PATH_LEN.checked_sub(utf16_len(dir.as_os_str()) + 1 + SIDECAR + DUPLICATE)?;
    (room >= MIN_NAME).then(|| clamp_to(name, room.min(MAX_NAME)))
}

fn clamp(name: &str) -> String {
    clamp_to(name, MAX_NAME)
}

fn clamp_to(name: &str, max: usize) -> String {
    let len = |s: &str| s.encode_utf16().count();
    if len(name) <= max {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && len(&name[i..]) <= 16 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut out = String::new();
    for ch in stem.chars() {
        if len(&out) + ch.len_utf16() + len(ext) > max {
            break;
        }
        out.push(ch);
    }
    out.trim_end_matches(['.', ' ']).to_owned() + ext
}

/// The part file a download is written to before it is complete.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".mdm.part");
    PathBuf::from(s)
}

/// `dir/name`, or `dir/stem (N).ext` with the lowest N that is free — IDM's default for a
/// duplicate link is "add the duplicate with a numbered file name" (notes/ENGINE.md §6).
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let taken = |p: &Path| p.exists() || part_path(p).exists() || is_reserved(p);
    let first = dir.join(name);
    if !taken(&first) {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1u32..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !taken(p))
        .expect("unbounded search finds a free name")
}

/// Destinations of downloads running in this process: one destination, one download.
fn active() -> &'static Mutex<HashSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(Default::default)
}

fn is_reserved(p: &Path) -> bool {
    active().lock().unwrap().contains(p)
}

/// Holds a destination for one download; released when dropped.
#[derive(Debug)]
pub struct Reservation(PathBuf);

impl Drop for Reservation {
    fn drop(&mut self) {
        active().lock().unwrap().remove(&self.0);
    }
}

/// Reserve `dest` for one download. None when another download in this process has it.
pub fn reserve(dest: &Path) -> Option<Reservation> {
    active()
        .lock()
        .unwrap()
        .insert(dest.to_owned())
        .then(|| Reservation(dest.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_shrink_to_fit_a_deep_folder() {
        let deep = PathBuf::from(format!(r"C:\{}", "d".repeat(200)));
        let name = fit_name(&deep, &format!("{}.zip", "n".repeat(100))).unwrap();
        assert!(name.ends_with(".zip"), "{name}");
        let dest = deep.join(&name);
        assert!(path_fits(&dest));
        // Room for the duplicate's number too.
        assert!(utf16_len(dest.as_os_str()) + SIDECAR + DUPLICATE <= MAX_PATH_LEN);
        assert_eq!(
            fit_name(Path::new(r"C:\dl"), "a.zip").as_deref(),
            Some("a.zip")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_too_deep_for_any_name_is_refused() {
        let deeper = PathBuf::from(format!(r"C:\{}", "d".repeat(240)));
        assert_eq!(fit_name(&deeper, "a.zip"), None);
        assert!(!path_fits(&deeper.join("a-longer-file-name.zip")));
    }

    #[test]
    fn long_names_are_clamped_keeping_the_extension() {
        let long = format!("{}.mp4", "a".repeat(400));
        let s = sanitize(&long);
        assert!(s.encode_utf16().count() <= MAX_NAME);
        assert!(s.ends_with(".mp4"));
    }

    #[test]
    fn all_reserved_device_names() {
        for n in [
            "CONIN$.txt",
            "conout$",
            "COM¹.log",
            "LPT³",
            "com9.txt",
            "nul",
        ] {
            assert!(sanitize(n).starts_with('_'), "{n}");
        }
        assert_eq!(sanitize("COMPUTER.txt"), "COMPUTER.txt");
    }

    #[test]
    fn one_destination_one_download() {
        let p = std::env::temp_dir().join("mdm-reserve-test.bin");
        let a = reserve(&p).expect("free");
        assert!(reserve(&p).is_none(), "second download refused");
        let other = unique_path(&std::env::temp_dir(), "mdm-reserve-test.bin");
        assert_ne!(other, p, "unique_path skips reserved names");
        drop(a);
        assert!(reserve(&p).is_some(), "released when dropped");
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn header_wins_over_url() {
        let u = url("http://x/a/b.zip");
        assert_eq!(
            file_name(Some("attachment; filename=\"movie.mp4\""), &u),
            "movie.mp4"
        );
        assert_eq!(
            file_name(
                Some("attachment; filename*=UTF-8''na%C3%AFve%20file.txt"),
                &u
            ),
            "naïve file.txt"
        );
        assert_eq!(file_name(None, &u), "b.zip");
        assert_eq!(file_name(Some("inline"), &u), "b.zip");
    }

    #[test]
    fn url_fallbacks() {
        assert_eq!(
            file_name(None, &url("http://x/dir/my%20file.iso?x=1")),
            "my file.iso"
        );
        assert_eq!(file_name(None, &url("http://x/")), "download");
    }

    #[test]
    fn sanitizes() {
        assert_eq!(
            file_name(
                Some("attachment; filename=\"../../evil.exe\""),
                &url("http://x/")
            ),
            "evil.exe"
        );
        assert_eq!(sanitize("a<b>c:d?.txt"), "a_b_c_d_.txt");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("COM1"), "_COM1");
        assert_eq!(sanitize("compact.txt"), "compact.txt");
        assert_eq!(sanitize("name. . "), "name");
        assert_eq!(sanitize("..."), "download");
    }

    #[test]
    fn numbered_duplicates() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a.zip"));
        std::fs::write(d.path().join("a.zip"), b"").unwrap();
        assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a (1).zip"));
        std::fs::write(part_path(&d.path().join("a (1).zip")), b"").unwrap();
        assert_eq!(unique_path(d.path(), "a.zip"), d.path().join("a (2).zip"));
        std::fs::write(d.path().join("README"), b"").unwrap();
        assert_eq!(unique_path(d.path(), "README"), d.path().join("README (1)"));
    }
}

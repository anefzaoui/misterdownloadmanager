//! The mark a downloaded file carries to say it came from the internet (#813): Windows' Mark of
//! the Web (the `Zone.Identifier` stream SmartScreen and Office's Protected View read), macOS'
//! quarantine attribute (Gatekeeper's), Linux's origin attributes (shown by file managers,
//! enforced by nothing). Every download MDM makes is marked here (the engine's files as they are
//! created, extracted files, videos, galleries, torrents), as Options → Downloads → "Mark
//! downloaded files as coming from the internet" says and, on Windows, as Windows' own policy
//! for attachments says: browsers save their downloads through Windows' attachment services,
//! which leave the mark out under "Do not preserve zone information in file attachments".
//!
//! The mark is written by whatever writes the file, on its thread, before the download is said to
//! be done (#1103): never on the window's thread, where a disk slow to take it froze the window
//! (#510), and before the file can be opened (Windows checks the mark then) or renamed (a rename
//! keeps it).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(true);

/// Options → Downloads → "Mark downloaded files as coming from the internet", as it is now
/// (MDM's start, Options → OK).
pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the system keeps the mark on downloads, from Windows' Group Policy "Do not preserve
/// zone information in file attachments" (`SaveZoneInformation`, `win32::zone_policy`): the
/// computer's value, where it is set, wins over the user's; 1 keeps none, 2 or none set keeps
/// it.
// Asked on Windows only (its tests run everywhere).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn keeps_zones(machine: Option<u32>, user: Option<u32>) -> bool {
    machine.or(user) != Some(1)
}

/// Whether a downloaded file is marked: MDM's option, and the system's policy.
pub fn wanted(option: bool, system_keeps: bool) -> bool {
    option && system_keeps
}

/// Whether the system keeps the mark on downloads, asked now (a registry read on Windows: for a
/// thread other than the window's).
fn system_keeps() -> bool {
    #[cfg(windows)]
    {
        let (machine, user) = crate::win32::zone_policy();
        keeps_zones(machine, user)
    }
    // macOS has no such policy for its quarantine attribute (Gatekeeper's, which only
    // MDM's option leaves out), nor Linux for its origin attributes, which nothing enforces.
    #[cfg(not(windows))]
    {
        true
    }
}

/// What a mark names as the source of a file that names none of its own (a torrent's, a magnet
/// link's): Windows' own word for the internet zone.
pub const INTERNET: &str = "about:internet";

/// The address a mark names as a file's source: a web or FTP address without its login (user
/// name and password: a mark never carries one), as browsers wrote theirs; any other (a magnet
/// link sent to a debrid account) [`INTERNET`], as a torrent's files are.
pub fn source(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https" | "ftp" | "ftps" | "ftpes") => {
            without_login(u)
        }
        _ => INTERNET.to_owned(),
    }
}

/// The page a mark names as a file's referrer: a web page's address without its login; none
/// for anything else.
pub fn page(referrer: &str) -> Option<String> {
    url::Url::parse(referrer)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(without_login)
}

fn without_login(mut u: url::Url) -> String {
    // Only an address that can't have a login refuses these (a magnet link): none here.
    let _ = u.set_username("");
    let _ = u.set_password(None);
    u.to_string()
}

/// Mark `path`, a file being downloaded (or just downloaded) from `url` (`referrer`'s page), as
/// coming from the internet, unless MDM's option or the system's policy says not to. Every
/// download is marked, whatever its address; the mark names it as [`source`] and [`page`] say.
pub fn mark(path: &Path, url: &str, referrer: Option<&str>) {
    if !wanted(ON.load(Ordering::Relaxed), system_keeps()) {
        return;
    }
    let (url, referrer) = (source(url), referrer.and_then(page));
    #[cfg(windows)]
    crate::win32::mark_downloaded(path, &url, referrer.as_deref());
    #[cfg(unix)]
    crate::unix::mark_downloaded(path, &url, referrer.as_deref());
    // Any other system has no mark to give.
    #[cfg(not(any(windows, unix)))]
    let _ = (path, url, referrer);
}

/// Tests: that `path` carries the mark of a download from `source` (`referrer`'s page): Windows'
/// zone stream with both addresses, Linux's origin attributes, macOS' quarantine attribute
/// (which names no address). A Linux file system without extended attributes keeps none, and
/// is said and passed.
#[cfg(test)]
pub fn assert_marked(path: &Path, source: &str, referrer: Option<&str>) {
    #[cfg(windows)]
    {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let zone = std::fs::read_to_string(&stream)
            .unwrap_or_else(|e| panic!("{}: no mark ({e})", path.display()));
        let lines: Vec<&str> = zone.lines().collect();
        assert!(lines.contains(&"ZoneId=3"), "{zone}");
        assert!(
            lines.contains(&format!("HostUrl={source}").as_str()),
            "{zone}"
        );
        match referrer {
            Some(r) => assert!(
                lines.contains(&format!("ReferrerUrl={r}").as_str()),
                "{zone}"
            ),
            None => assert!(!zone.contains("ReferrerUrl="), "{zone}"),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let read = |name| crate::unix::xattr(path, name);
        match read("user.xdg.origin.url") {
            Err(e) if e.raw_os_error() == Some(libc::ENOTSUP) => {
                eprintln!(
                    "{}: no extended attributes here: not checked",
                    path.display()
                );
            }
            origin => {
                assert_eq!(
                    origin.unwrap().as_deref(),
                    Some(source),
                    "{}",
                    path.display()
                );
                assert_eq!(read("user.xdg.referrer.url").unwrap().as_deref(), referrer);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (source, referrer);
        let q = crate::unix::xattr(path, "com.apple.quarantine").unwrap();
        assert!(
            q.is_some_and(|q| q.starts_with("0081;")),
            "{}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_computers_policy_wins_and_only_1_drops_the_mark() {
        // Not set anywhere: kept, as Windows keeps it.
        assert!(keeps_zones(None, None));
        // The user's own policy.
        assert!(!keeps_zones(None, Some(1)));
        assert!(keeps_zones(None, Some(2)));
        // The computer's, over the user's either way.
        assert!(!keeps_zones(Some(1), None));
        assert!(!keeps_zones(Some(1), Some(2)));
        assert!(keeps_zones(Some(2), Some(1)));
        // A value Windows doesn't define keeps it.
        assert!(keeps_zones(Some(0), None));
    }

    /// A mark names a web or FTP address without its login, anything else (a magnet link, a
    /// torrent's own) as the internet; a referrer only when it is a web page's, without its
    /// login.
    #[test]
    fn a_mark_names_an_address_without_its_login() {
        for (url, named) in [
            ("https://videos.test/v?x=1", "https://videos.test/v?x=1"),
            ("https://user:pw@videos.test/v", "https://videos.test/v"),
            ("http://user@files.test/a.zip", "http://files.test/a.zip"),
            (
                "ftp://user:pw@files.test/a/b.txt",
                "ftp://files.test/a/b.txt",
            ),
            (
                "ftp://files.test/listing.txt",
                "ftp://files.test/listing.txt",
            ),
            ("ftps://u:p@files.test/x", "ftps://files.test/x"),
            ("ftpes://u:p@files.test/x", "ftpes://files.test/x"),
            (
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=setup.exe",
                INTERNET,
            ),
            (INTERNET, INTERNET),
            ("file:///C:/x.exe", INTERNET),
            ("not an address", INTERNET),
        ] {
            assert_eq!(source(url), named, "{url}");
        }
        assert_eq!(
            page("https://me:pw@pages.test/p").as_deref(),
            Some("https://pages.test/p")
        );
        assert_eq!(page("ftp://pages.test/"), None);
        assert_eq!(page("magnet:?xt=urn:btih:00"), None);
        assert_eq!(page("nope"), None);
    }

    /// Every download is marked, with its address as [`source`] says, written so on this
    /// system (Windows' zone stream, Linux's origin attributes, macOS' quarantine).
    #[test]
    fn every_download_is_marked_without_a_login() {
        let dir = tempfile::tempdir().unwrap();
        for (k, (url, referrer, named, page)) in [
            (
                "https://user:pw@videos.test/v",
                Some("https://me:pw@pages.test/p"),
                "https://videos.test/v",
                Some("https://pages.test/p"),
            ),
            (
                "ftp://user:pw@files.test/a/b.txt",
                None,
                "ftp://files.test/a/b.txt",
                None,
            ),
            (
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=setup.exe",
                Some("magnet:?xt=urn:btih:00"),
                INTERNET,
                None,
            ),
            (INTERNET, None, INTERNET, None),
        ]
        .into_iter()
        .enumerate()
        {
            let path = dir.path().join(format!("file{k}.bin"));
            std::fs::write(&path, b"x").unwrap();
            mark(&path, url, referrer);
            assert_marked(&path, named, page);
        }
    }

    #[test]
    fn a_file_is_marked_only_with_the_option_on_and_the_policy_keeping_it() {
        for (option, policy, marked) in [
            (true, None, true),
            (true, Some(2), true),
            (true, Some(1), false),
            (false, None, false),
            (false, Some(2), false),
            (false, Some(1), false),
        ] {
            assert_eq!(
                wanted(option, keeps_zones(None, policy)),
                marked,
                "option {option}, policy {policy:?}"
            );
        }
    }
}

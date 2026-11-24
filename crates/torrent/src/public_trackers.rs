//! A public tracker list (#1776): an option that adds the trackers of a well-known list (or one
//! at an address the user gives) to public torrents, so they find peers sooner. The list is
//! fetched with MDM's own downloader ([`download`]) once a day, again after an hour when a fetch
//! failed; this is what is done with it: read it ([`parse`]), keep it with the time it was tried
//! and fetched ([`Cache`], a file in the torrent data folder, the last good list kept when a
//! fetch fails), and add it to a torrent ([`add_to`]), never to a private one (BEP 27). The app
//! and the command line ([`refresh_file`]) share all of it.
//!
//! Every tracker of a list goes through [`trackers::allowed`], as it is read and again as it is
//! added, and is added with [`trackers::add`]: the info dictionary, and so the hash, stays byte
//! for byte.

use std::path::Path;
use std::time::Duration;

use crate::trackers;

/// The list used unless another address is given: ngosang/trackerslist's best trackers.
pub const DEFAULT_LIST: &str =
    "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt";

/// The most a list may weigh: a list of trackers is a few kilobytes; more isn't one.
pub const MAX_LIST_BYTES: u64 = 256 * 1024;

/// The most trackers taken from a list.
pub const MAX_TRACKERS: usize = 200;

/// The longest tracker address read from a list.
const MAX_LINE: usize = 512;

/// How often the list is fetched: at most once a day.
pub const EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// How soon a fetch that failed (or never ended: MDM was closed meanwhile) is tried again.
pub const RETRY: Duration = Duration::from_secs(60 * 60);

/// The cache's file, in the torrent data folder.
pub const CACHE_FILE: &str = "public-trackers.json";

/// A list's trackers: one per line, blank lines and `#` comments skipped, each one MDM may
/// contact ([`trackers::allowed`], as without a proxy: the session drops the UDP ones itself when
/// it goes through one), each once, at most [`MAX_TRACKERS`].
pub fn parse(body: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(body);
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.len() > MAX_LINE {
            continue;
        }
        if trackers::allowed(line, false) && !out.iter().any(|t| t == line) {
            out.push(line.to_owned());
            if out.len() == MAX_TRACKERS {
                break;
            }
        }
    }
    out
}

/// `torrent` with the list's trackers after its own (each a tier, those it has skipped), its info
/// dictionary byte for byte. None, to leave it as it is: it is private ([`trackers::private`]),
/// it can't be read, or the list adds nothing to it.
pub fn add_to(torrent: &[u8], list: &[String]) -> Option<Vec<u8>> {
    if trackers::private(torrent) {
        return None;
    }
    let kept: Vec<String> = list
        .iter()
        .map(|t| t.trim())
        .filter(|t| t.len() <= MAX_LINE && trackers::allowed(t, false))
        .map(str::to_owned)
        .collect();
    if kept.is_empty() {
        return None;
    }
    trackers::add(torrent, &kept).filter(|out| out != torrent)
}

/// The list as last fetched, and when: what the app keeps in the torrent data folder
/// ([`CACHE_FILE`]). Times are Unix seconds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cache {
    /// The address the list was fetched from (another one now: its trackers aren't used).
    source: String,
    /// When a fetch was last started, whatever came of it (0: never).
    tried: u64,
    /// When the trackers kept were fetched (0: never).
    fetched: u64,
    /// The last good list.
    trackers: Vec<String>,
}

impl Cache {
    /// The cache in `dir` (none there, or one that can't be read: an empty one). Its trackers
    /// are checked again ([`trackers::allowed`]): a file changed by hand is read as a list.
    pub fn load(dir: &Path) -> Cache {
        let Ok(raw) = std::fs::read(dir.join(CACHE_FILE)) else {
            return Cache::default();
        };
        let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            return Cache::default();
        };
        let secs = |k: &str| doc.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        let list: String = doc
            .get("trackers")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Cache {
            source: doc
                .get("source")
                .and_then(|s| s.as_str())
                .unwrap_or_default()
                .to_owned(),
            tried: secs("tried"),
            fetched: secs("fetched"),
            trackers: parse(list.as_bytes()),
        }
    }

    /// Write it to `dir`, through a temporary file: a half-written cache would lose the list.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let doc = serde_json::json!({
            "source": self.source,
            "tried": self.tried,
            "fetched": self.fetched,
            "trackers": self.trackers,
        });
        let tmp = dir.join(format!("{CACHE_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(&doc)?)?;
        std::fs::rename(&tmp, dir.join(CACHE_FILE))
    }

    /// Whether the list at `source` is to be fetched now: never tried from there, last fetched
    /// a day ago or more, or last tried an hour ago or more without a list coming of it (the try
    /// is recorded before it starts, so one that never ended counts as failed: a crash at each
    /// start still asks at most once an hour). A last try more than a day ahead of `now` (the
    /// clock was put back) doesn't hold it off for good.
    pub fn due(&self, source: &str, now: u64) -> bool {
        let wait = if self.fetched >= self.tried {
            EVERY
        } else {
            RETRY
        }
        .as_secs();
        self.source != source.trim()
            || now >= self.tried.saturating_add(wait)
            || self.tried > now.saturating_add(EVERY.as_secs())
    }

    /// A fetch from `source` starts at `now`. Another address than before: the old list's
    /// trackers go (they aren't that address's).
    pub fn tried(&mut self, source: &str, now: u64) {
        let source = source.trim();
        if self.source != source {
            *self = Cache {
                source: source.to_owned(),
                ..Cache::default()
            };
        }
        self.tried = now;
    }

    /// What a fetch from `source` sent, at `now`: its trackers, how many. None when it holds
    /// none MDM may contact (an error page, a list of something else) or `source` isn't the
    /// address tried last: the last good list stays.
    pub fn fetched(&mut self, source: &str, body: &[u8], now: u64) -> Option<usize> {
        if self.source != source.trim() {
            return None;
        }
        let list = parse(body);
        if list.is_empty() {
            return None;
        }
        self.trackers = list;
        self.fetched = now;
        Some(self.trackers.len())
    }

    /// The trackers fetched from `source` (none yet, or from another address: none).
    pub fn trackers(&self, source: &str) -> &[String] {
        if self.source == source.trim() {
            &self.trackers
        } else {
            &[]
        }
    }

    /// When the trackers kept were fetched (None: never).
    pub fn fetched_at(&self) -> Option<u64> {
        (self.fetched != 0).then_some(self.fetched)
    }

    /// What a torrent records once the list from `source` was added to it (a stamp of the
    /// address and its trackers; None while there is no list): another list (a newer fetch that
    /// changed it, another address) has another stamp, and is added again (only its new trackers
    /// are new to the torrent). FNV-1a, the same in every build.
    pub fn stamp(&self, source: &str) -> Option<u64> {
        let list = self.trackers(source);
        if list.is_empty() {
            return None;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for part in std::iter::once(self.source.as_str()).chain(list.iter().map(String::as_str)) {
            for b in part.bytes().chain(std::iter::once(b'\n')) {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        // 0 is what a torrent that never got a list records.
        Some(h.max(1))
    }
}

/// Fetch the list at `url` with MDM's engine (`opts`: the proxy, DNS and network adapter the
/// caller uses for its own downloads), into memory, through a temporary file in `dir` that is
/// removed after (with its resume state). Stopped, and an error, past [`MAX_LIST_BYTES`].
pub async fn download(url: &str, dir: &Path, opts: mdm_engine::Options) -> Result<Vec<u8>, String> {
    use mdm_engine::download::State;
    let url = url::Url::parse(url.trim()).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{url}: not a web address"));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        "public-trackers-{}.part-of-mdm",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let dl = mdm_engine::Download::start(
        mdm_engine::Request {
            url,
            headers: Default::default(),
            dest: mdm_engine::Destination::File(path.clone()),
        },
        opts,
    );
    let too_big = loop {
        let s = dl.snapshot();
        if s.size.is_some_and(|z| z > MAX_LIST_BYTES) || s.downloaded > MAX_LIST_BYTES {
            dl.cancel();
            break true;
        }
        if matches!(s.state, State::Done | State::Failed | State::Cancelled) {
            break false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let res = dl.wait().await;
    let body = if too_big {
        Err(format!("larger than {MAX_LIST_BYTES} bytes: not a list"))
    } else {
        match res {
            Ok(_) => std::fs::read(&path).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    };
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(mdm_engine::state::state_path(&path));
    let _ = std::fs::remove_file(mdm_engine::naming::part_path(&path));
    body.and_then(|b| {
        if b.len() as u64 > MAX_LIST_BYTES {
            Err(format!("larger than {MAX_LIST_BYTES} bytes: not a list"))
        } else {
            Ok(b)
        }
    })
}

/// The cache in `dir`, fetched again first when it is due ([`Cache::due`]), for a process that
/// keeps no copy of its own (the command line): the try recorded before the fetch, what came
/// kept, both saved. `fetch` fetches the list (the engine's [`download`], or a test's fixture).
pub async fn refresh_file<F, Fut>(dir: &Path, url: &str, now: impl Fn() -> u64, fetch: F) -> Cache
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    let mut cache = Cache::load(dir);
    if !cache.due(url, now()) {
        return cache;
    }
    cache.tried(url, now());
    if let Err(e) = cache.save(dir) {
        tracing::warn!("public trackers: the list's cache wasn't saved: {e}");
    }
    match fetch().await {
        Ok(body) => match cache.fetched(url, &body, now()) {
            Some(n) => {
                tracing::info!("public trackers: {n} from {url}");
                if let Err(e) = cache.save(dir) {
                    tracing::warn!("public trackers: the list wasn't saved: {e}");
                }
            }
            None => tracing::warn!("public trackers: {url} sent no tracker; the last list stays"),
        },
        Err(e) => tracing::warn!("public trackers: {url}: {e}; the last list stays"),
    }
    cache
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &[u8] =
        b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
    const PRIVATE_INFO: &[u8] =
        b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaa7:privatei1ee";

    fn s(v: &str) -> String {
        format!("{}:{v}", v.len())
    }

    fn torrent(head: &str, info: &[u8]) -> Vec<u8> {
        let mut t = format!("d{head}4:info").into_bytes();
        t.extend(info);
        t.push(b'e');
        t
    }

    /// A list as ngosang/trackerslist writes it: one tracker per line, a blank line between.
    const FIXTURE: &str = "udp://tracker.opentrackr.org:1337/announce\n\n\
        udp://open.stealth.si:80/announce\r\n\r\n\
        # a comment\n\
        https://tracker.example.org:443/announce\n\n\
        http://127.0.0.1:6969/announce\n\
        udp://192.168.1.1:6969/announce\n\
        udp://localhost:1337/announce\n\
        wss://tracker.example.org/announce\n\
        file:///etc/hosts\n\
        not a tracker\n\
        udp://tracker.opentrackr.org:1337/announce\n";

    const KEPT: [&str; 3] = [
        "udp://tracker.opentrackr.org:1337/announce",
        "udp://open.stealth.si:80/announce",
        "https://tracker.example.org:443/announce",
    ];

    /// Only trackers MDM may contact are read from a list, each once.
    #[test]
    fn a_list_keeps_only_trackers_mdm_may_contact() {
        assert_eq!(parse(FIXTURE.as_bytes()), KEPT);
        // An error page or a captive portal's is no list.
        assert!(parse(b"<html><body>Sign in to the Wi-Fi</body></html>").is_empty());
        assert!(parse(b"").is_empty());
        assert!(parse(&[0xff, 0xfe, 0x00, 0x41]).is_empty());
        // A huge line isn't read; a huge list stops at its cap.
        let long = format!("udp://{}.example:1337/announce", "a".repeat(600));
        assert!(parse(long.as_bytes()).is_empty());
        let many: String = (0..500)
            .map(|i| format!("udp://t{i}.example.org:1337/announce\n"))
            .collect();
        assert_eq!(parse(many.as_bytes()).len(), MAX_TRACKERS);
    }

    /// A public torrent gets the list after its own trackers, its info byte for byte (the same
    /// hash); a list's tracker that slipped in unchecked still isn't added.
    #[test]
    fn a_public_torrent_gets_the_list() {
        let own = "http://own.example/announce";
        let t = torrent(&format!("8:announce{}", s(own)), INFO);
        let mut list: Vec<String> = KEPT.iter().map(|k| k.to_string()).collect();
        list.push("http://10.0.0.1/announce".into());
        list.push(own.into());
        let out = add_to(&t, &list).unwrap();
        let mut want = vec![own];
        want.extend(KEPT);
        assert_eq!(trackers::listed(&out), want);
        let hash = |b: &[u8]| librqbit::torrent_from_bytes(b).unwrap().info_hash;
        assert_eq!(hash(&out), hash(&t));
        assert!(out.windows(INFO.len()).any(|w| w == INFO));
        // What librqbit reads: the torrent's own, then the list's.
        let read: Vec<String> = librqbit::torrent_from_bytes(&out)
            .unwrap()
            .iter_announce()
            .map(|u| String::from_utf8_lossy(u.as_ref()).into_owned())
            .collect();
        assert_eq!(read, want);
        // Nothing new, an empty list or one of refused trackers: left as it is.
        assert_eq!(add_to(&out, &list), None);
        assert_eq!(add_to(&t, &[]), None);
        assert_eq!(add_to(&t, &["http://192.168.0.1/a".into()]), None);
        // One without trackers (a magnet's) gets them all, the refused one still left out.
        let bare = torrent("", INFO);
        let mut all = KEPT.to_vec();
        all.push(own);
        assert_eq!(trackers::listed(&add_to(&bare, &list).unwrap()), all);
    }

    /// A private torrent (BEP 27) never gets another tracker; nor one that can't be read.
    #[test]
    fn a_private_torrent_never_gets_the_list() {
        let list: Vec<String> = KEPT.iter().map(|k| k.to_string()).collect();
        let own = format!(
            "8:announce{}",
            s("http://private.example/announce?passkey=x")
        );
        let private = torrent(&own, PRIVATE_INFO);
        assert!(
            librqbit::torrent_from_bytes(&private)
                .unwrap()
                .info
                .data
                .private
        );
        assert!(trackers::private(&private));
        assert_eq!(add_to(&private, &list), None);
        assert_eq!(add_to(&torrent("", PRIVATE_INFO), &list), None);
        // `private` 0 is public; any other value, or a second `info` saying so, is private.
        let zero = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaa7:privatei0ee";
        assert!(!trackers::private(&torrent(&own, zero)));
        assert!(add_to(&torrent(&own, zero), &list).is_some());
        let two = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaa7:private1:1e";
        assert!(trackers::private(&torrent(&own, two)));
        let mut twice = format!("d{own}4:info").into_bytes();
        twice.extend(INFO);
        twice.extend(b"4:info");
        twice.extend(PRIVATE_INFO);
        twice.push(b'e');
        assert!(trackers::private(&twice));
        assert_eq!(add_to(&twice, &list), None);
        // No info, or no bencode: nothing added.
        assert!(trackers::private(b"de"));
        assert!(trackers::private(b"not a torrent"));
        assert!(trackers::private(&format!("d{own}4:infoi1ee").into_bytes()));
        assert_eq!(add_to(b"not a torrent", &list), None);
        assert!(!trackers::private(&torrent(&own, INFO)));
    }

    /// Fetched at most once a day; another address at once; a clock put back doesn't stop it
    /// for good.
    #[test]
    fn the_list_is_fetched_at_most_once_a_day() {
        let (day, hour) = (EVERY.as_secs(), RETRY.as_secs());
        let t0 = 1_800_000_000;
        let mut c = Cache::default();
        assert!(c.due(DEFAULT_LIST, t0), "never fetched");
        // A good fetch: the next a day later.
        c.tried(DEFAULT_LIST, t0);
        c.fetched(DEFAULT_LIST, FIXTURE.as_bytes(), t0 + 2).unwrap();
        assert!(!c.due(DEFAULT_LIST, t0 + 60));
        assert!(!c.due(DEFAULT_LIST, t0 + hour + 60));
        assert!(!c.due(&format!(" {DEFAULT_LIST} "), t0 + day - 1));
        assert!(c.due(DEFAULT_LIST, t0 + day));
        // Another address: at once.
        assert!(c.due("https://lists.example.org/trackers.txt", t0 + 60));
        // A clock put back by an hour still waits; by more than a day, it doesn't.
        assert!(!c.due(DEFAULT_LIST, t0 - 3600));
        assert!(c.due(DEFAULT_LIST, t0 - day - 1));
    }

    /// A fetch that failed, brought no list, or never ended (the try is recorded before it
    /// starts) is tried again an hour later, not sooner, and the last list stays meanwhile.
    #[test]
    fn a_failed_fetch_is_tried_again_after_an_hour() {
        let (day, hour) = (EVERY.as_secs(), RETRY.as_secs());
        let t0 = 1_800_000_000;
        let mut c = Cache::default();
        // Never ended (MDM closed, or crashed, during it): an hour.
        c.tried(DEFAULT_LIST, t0);
        assert!(!c.due(DEFAULT_LIST, t0 + 60));
        assert!(!c.due(DEFAULT_LIST, t0 + hour - 1));
        assert!(c.due(DEFAULT_LIST, t0 + hour));
        // Then a good one: a day again.
        let t1 = t0 + hour;
        c.tried(DEFAULT_LIST, t1);
        c.fetched(DEFAULT_LIST, FIXTURE.as_bytes(), t1 + 1).unwrap();
        assert!(!c.due(DEFAULT_LIST, t1 + hour + 1));
        // A day later an error page: the list stays, the next try an hour after that one.
        let t2 = t1 + day;
        assert!(c.due(DEFAULT_LIST, t2));
        c.tried(DEFAULT_LIST, t2);
        assert_eq!(c.fetched(DEFAULT_LIST, b"<html>502</html>", t2 + 1), None);
        assert_eq!(c.trackers(DEFAULT_LIST), KEPT);
        assert!(!c.due(DEFAULT_LIST, t2 + hour - 1));
        assert!(c.due(DEFAULT_LIST, t2 + hour));
    }

    /// A torrent records the list it got by a stamp: the same list, the same stamp; a list that
    /// changed, or another address's, another one; no list, none.
    #[test]
    fn a_list_is_stamped_for_the_torrents_that_got_it() {
        let mut c = Cache::default();
        assert_eq!(c.stamp(DEFAULT_LIST), None);
        c.tried(DEFAULT_LIST, 10);
        assert_eq!(c.stamp(DEFAULT_LIST), None, "tried, nothing yet");
        c.fetched(DEFAULT_LIST, FIXTURE.as_bytes(), 11).unwrap();
        let first = c.stamp(DEFAULT_LIST).unwrap();
        assert_ne!(first, 0);
        assert_eq!(c.stamp(&format!(" {DEFAULT_LIST}")), Some(first));
        // The same list fetched again: the same stamp (nothing is added again).
        c.tried(DEFAULT_LIST, 20);
        c.fetched(DEFAULT_LIST, FIXTURE.as_bytes(), 21).unwrap();
        assert_eq!(c.stamp(DEFAULT_LIST), Some(first));
        // A list that changed: another stamp.
        let more = format!("{FIXTURE}udp://tracker.new.example.org:6969/announce\n");
        c.fetched(DEFAULT_LIST, more.as_bytes(), 22).unwrap();
        assert_ne!(c.stamp(DEFAULT_LIST), Some(first));
        // Another address: none until its list comes, then its own.
        assert_eq!(c.stamp("https://lists.example.org/t.txt"), None);
        let mut other = Cache::default();
        other.tried("https://lists.example.org/t.txt", 30);
        other
            .fetched("https://lists.example.org/t.txt", FIXTURE.as_bytes(), 31)
            .unwrap();
        assert_ne!(other.stamp("https://lists.example.org/t.txt"), Some(first));
    }

    /// The command line's refresh (`refresh_file`): fetched only when due, the try saved before
    /// the fetch, the last good list kept when it fails. The fetch is a fixture: no network.
    #[tokio::test]
    async fn the_command_line_refreshes_the_same_cache() {
        use std::cell::Cell;
        let dir = tempfile::tempdir().unwrap();
        let t = Cell::new(1_800_000_000u64);
        let fetches = Cell::new(0);
        let ok = || {
            fetches.set(fetches.get() + 1);
            async { Ok(FIXTURE.as_bytes().to_vec()) }
        };
        let c = refresh_file(dir.path(), DEFAULT_LIST, || t.get(), ok).await;
        assert_eq!(c.trackers(DEFAULT_LIST), KEPT);
        assert_eq!(fetches.get(), 1);
        assert_eq!(Cache::load(dir.path()), c, "saved");
        // An hour later: not due, the cache as it is, no fetch.
        t.set(t.get() + RETRY.as_secs());
        let again = refresh_file(dir.path(), DEFAULT_LIST, || t.get(), ok).await;
        assert_eq!(again, c);
        assert_eq!(fetches.get(), 1);
        // A day later the fetch fails: the list stays, the try is saved, the next is an hour on.
        t.set(t.get() + EVERY.as_secs());
        let failed = refresh_file(
            dir.path(),
            DEFAULT_LIST,
            || t.get(),
            || async { Err("connection refused".to_owned()) },
        )
        .await;
        assert_eq!(failed.trackers(DEFAULT_LIST), KEPT);
        assert!(!Cache::load(dir.path()).due(DEFAULT_LIST, t.get() + 60));
        assert!(Cache::load(dir.path()).due(DEFAULT_LIST, t.get() + RETRY.as_secs()));
        // The try is on disk before the fetch ends: a fetch that never returns still counts.
        let seen = Cell::new(0u64);
        t.set(t.get() + RETRY.as_secs());
        refresh_file(
            dir.path(),
            DEFAULT_LIST,
            || t.get(),
            || {
                seen.set(Cache::load(dir.path()).tried);
                async { Err("stopped".to_owned()) }
            },
        )
        .await;
        assert_eq!(seen.get(), t.get());
    }

    /// A fetch that fails, or brings no list, keeps the last good one; another address drops it.
    #[test]
    fn the_last_good_list_stays() {
        let t0 = 1_800_000_000;
        let mut c = Cache::default();
        c.tried(DEFAULT_LIST, t0);
        assert_eq!(c.fetched(DEFAULT_LIST, FIXTURE.as_bytes(), t0 + 2), Some(3));
        assert_eq!(c.trackers(DEFAULT_LIST), KEPT);
        assert_eq!(c.fetched_at(), Some(t0 + 2));
        // A day later the fetch brings an error page: the list stays, the try counts.
        let later = t0 + EVERY.as_secs();
        c.tried(DEFAULT_LIST, later);
        assert_eq!(
            c.fetched(DEFAULT_LIST, b"<html>502</html>", later + 1),
            None
        );
        assert_eq!(c.trackers(DEFAULT_LIST), KEPT);
        assert_eq!(c.fetched_at(), Some(t0 + 2));
        assert!(!c.due(DEFAULT_LIST, later + 60));
        // An answer for an address no longer tried is ignored.
        assert_eq!(
            c.fetched("https://old.example/list.txt", FIXTURE.as_bytes(), later),
            None
        );
        // Another address: the old list isn't used for it, and goes once it is tried.
        let other = "https://lists.example.org/trackers.txt";
        assert!(c.trackers(other).is_empty());
        c.tried(other, later + 120);
        assert!(c.trackers(DEFAULT_LIST).is_empty());
        assert_eq!(c.fetched_at(), None);
    }

    /// The cache in the data folder: saved and read back as it was; a missing or broken file is
    /// an empty cache, and trackers written into it by hand are checked again.
    #[test]
    fn the_cache_is_kept_in_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Cache::load(dir.path()), Cache::default());
        let mut c = Cache::default();
        c.tried(DEFAULT_LIST, 100);
        c.fetched(DEFAULT_LIST, FIXTURE.as_bytes(), 101).unwrap();
        c.save(dir.path()).unwrap();
        assert_eq!(Cache::load(dir.path()), c);
        assert!(!dir.path().join(format!("{CACHE_FILE}.tmp")).exists());
        // Changed by hand: a local tracker added is dropped as it is read.
        let path = dir.path().join(CACHE_FILE);
        let mut doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["trackers"]
            .as_array_mut()
            .unwrap()
            .push("http://127.0.0.1:80/announce".into());
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert_eq!(Cache::load(dir.path()).trackers(DEFAULT_LIST), KEPT);
        // Broken: empty, and due.
        std::fs::write(&path, b"{ not json").unwrap();
        let broken = Cache::load(dir.path());
        assert_eq!(broken, Cache::default());
        assert!(broken.due(DEFAULT_LIST, 200));
    }
}

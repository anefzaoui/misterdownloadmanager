//! Torrents in the app (M15, D-014). One librqbit session for the whole app, started the first
//! time a torrent needs it (no listening port or DHT for people who never use torrents). Each
//! torrent download is a [`Run`]: it starts the session if needed, adds or resumes its torrent,
//! and reads its progress for the list and the progress window, as the engine's downloads do.
//!
//! The download list is the truth: on start, torrents the session restored but the list no
//! longer has are forgotten (their files stay), and the rest wait paused until started.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mdm_engine::Snapshot;
use mdm_engine::download::State;
use mdm_torrent::{Config, Phase, Torrents};
use serde::{Deserialize, Serialize};

/// What a torrent download keeps in the list (downloads.json).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentSpec {
    /// The info hash, 40 hex digits; the .torrent itself is in [`torrent_file`].
    pub hash: String,
    /// The chosen files (indexes into the torrent's list); None: all of them.
    pub files: Option<Vec<usize>>,
    /// Where its files go (a multi-file torrent's own folder included).
    pub folder: PathBuf,
    /// Bytes sent to peers, in all its runs (librqbit counts only the current one: from 0
    /// again at each resume and each start of the app). Kept by [`Accountant`].
    #[serde(default)]
    pub uploaded: u64,
    /// Seconds it has spent sharing once finished, in all its runs.
    #[serde(default)]
    pub seeded_secs: u64,
    /// Its own upload limit, bytes/s (None: only Torrent settings' limit for all torrents).
    #[serde(default)]
    pub upload_limit: Option<u64>,
}

/// Adds up what each torrent sent, run after run: librqbit's count starts at 0 again each time
/// a torrent resumes (and is 0 while it is paused), so a smaller count than the last one seen
/// is all new.
#[derive(Default)]
pub struct Accountant {
    seen: std::collections::HashMap<String, u64>,
}

impl Accountant {
    /// The bytes `hash` sent since it was last seen; `live` is librqbit's count now.
    pub fn sent(&mut self, hash: &str, live: u64) -> u64 {
        let last = self.seen.insert(hash.to_owned(), live);
        match last {
            Some(last) if live >= last => live - last,
            // First seen in this run of the app, or a new count since: all of it is new.
            _ => live,
        }
    }

    /// Forget torrents no longer in the list.
    pub fn keep(&mut self, hashes: &HashSet<String>) {
        self.seen.retain(|h, _| hashes.contains(h));
    }
}

/// Our torrent data: librqbit's resume data and DHT nodes, and a copy of each .torrent.
pub fn state_dir() -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join("torrents"))
}

pub fn torrent_file(hash: &str) -> Option<PathBuf> {
    state_dir().map(|d| d.join("files").join(format!("{hash}.torrent")))
}

pub fn save_torrent(hash: &str, bytes: &[u8]) -> std::io::Result<()> {
    let path = torrent_file(hash)
        // i18n: an error: the app has no folder for its own data
        .ok_or_else(|| std::io::Error::other(crate::i18n::t("no data folder").to_owned()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// The blocklist as MDM converted it (mdm_torrent::blocklist), in the torrent data folder.
pub const BLOCKLIST_FILE: &str = "blocklist.p2p";
/// The address it was fetched from (another address: fetched again).
const BLOCKLIST_SOURCE: &str = "blocklist.source";
/// How often a blocklist is fetched again.
pub const BLOCKLIST_DAYS: u64 = 7;

/// When the blocklist at `url` was last converted (None: never, or from another address).
pub fn blocklist_updated(url: &str) -> Option<std::time::SystemTime> {
    let dir = state_dir()?;
    let source = std::fs::read_to_string(dir.join(BLOCKLIST_SOURCE)).ok()?;
    (source.trim() == url.trim())
        .then(|| {
            std::fs::metadata(dir.join(BLOCKLIST_FILE))
                .ok()?
                .modified()
                .ok()
        })
        .flatten()
}

/// Fetch and convert the blocklist at `url` (#40) when it was never converted, comes from
/// another address now, or is older than `BLOCKLIST_DAYS`; in the background, one at a time
/// (`busy`), with MDM's own downloader (Proxy / Socks applies). A list that isn't one keeps the
/// last good one. The torrent session reads it when it starts.
pub fn refresh_blocklist(
    rt: &tokio::runtime::Handle,
    url: &str,
    busy: &Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let url = url.trim().to_owned();
    let Some(dir) = state_dir() else { return };
    let Ok(parsed) = url::Url::parse(&url) else {
        return;
    };
    let fresh = blocklist_updated(&url)
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(BLOCKLIST_DAYS * 86_400));
    if url.is_empty() || fresh || busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let busy = busy.clone();
    rt.spawn(async move {
        let _ = std::fs::create_dir_all(&dir);
        let download = dir.join("blocklist.download");
        let fetched =
            mdm_engine::fetch(parsed, &download, crate::settings::own_net(), |_, _| {}).await;
        match fetched {
            Ok(_) => match std::fs::read(&download)
                .ok()
                .as_deref()
                .and_then(mdm_torrent::blocklist::convert)
            {
                Some(list) => {
                    let tmp = dir.join(format!("{BLOCKLIST_FILE}.tmp"));
                    if std::fs::write(&tmp, &list.text).is_ok()
                        && std::fs::rename(&tmp, dir.join(BLOCKLIST_FILE)).is_ok()
                    {
                        let _ = std::fs::write(dir.join(BLOCKLIST_SOURCE), &url);
                        tracing::info!(
                            "blocklist: {} ranges ({} lines skipped)",
                            list.ranges,
                            list.skipped
                        );
                    }
                }
                None => tracing::warn!("blocklist: {url} is not a list MDM can read"),
            },
            Err(e) => tracing::warn!("blocklist: {e}"),
        }
        let _ = std::fs::remove_file(&download);
        let _ = std::fs::remove_file(mdm_engine::state::state_path(&download));
        busy.store(false, Ordering::SeqCst);
    });
}

/// Why torrents can't start with the network adapter as it is (#35), if they can't.
pub fn route_refusal(route: &mdm_engine::Route) -> Option<String> {
    match route {
        mdm_engine::Route::Any => None,
        mdm_engine::Route::Down { label } => Some(crate::i18n::fmt(
            // i18n: an error: the network adapter chosen in Options (a VPN's, usually) is off or gone; {adapter} its name
            crate::i18n::t("the network adapter {adapter} is not connected"),
            &[("adapter", label)],
        )),
        // librqbit can't hold its sockets to an adapter on Windows (BindDevice isn't supported
        // there): refused rather than sent around it.
        #[cfg(windows)]
        mdm_engine::Route::Bound(_) => Some(
            // i18n: an error: torrents are refused while Options → Proxy / Socks → Network adapter names one (Windows only)
            crate::i18n::t("torrents can't go through a chosen network adapter on Windows yet")
                .to_owned(),
        ),
        #[cfg(not(windows))]
        mdm_engine::Route::Bound(_) => None,
    }
}

/// The app's one session, started on first use, as Torrent settings say (`set_config`); it
/// stops when no torrent needs it (`stop_if_idle`), and a new one takes the settings of then.
#[derive(Clone)]
pub struct Host {
    cell: Arc<tokio::sync::Mutex<Option<Arc<Torrents>>>>,
    /// The settings the next session starts with (None: they can't be met, see
    /// Settings::torrent_config) and those of the one running.
    wanted: Arc<Mutex<Option<Config>>>,
    running: Arc<Mutex<Option<Config>>>,
    /// The global cap (D-012), bytes/s: every torrent together stays under it (`set_cap`).
    cap: Arc<Mutex<Option<u64>>>,
    /// Torrent settings' upload limit for all torrents together, bytes/s (`set_upload`).
    upload: Arc<Mutex<Option<u64>>>,
}

impl Host {
    pub fn new() -> Self {
        Host {
            cell: Arc::new(tokio::sync::Mutex::new(None)),
            wanted: Arc::new(Mutex::new(state_dir().map(Config::new))),
            running: Arc::new(Mutex::new(None)),
            cap: Arc::new(Mutex::new(None)),
            upload: Arc::new(Mutex::new(None)),
        }
    }

    /// Torrent settings' upload limit for all torrents (None: unlimited), for the session.
    /// Cheap: called each frame, as `set_cap`; a session that starts takes it at once.
    pub fn set_upload(&self, rate: Option<u64>) {
        *self.upload.lock().unwrap() = rate;
        if let Some(t) = self.get() {
            t.set_upload_limit(rate);
        }
    }

    /// The torrents' share of the global cap now (None: off), for the session's download limit:
    /// torrents together stay under it (`App::apply_global_limit` splits the cap with file
    /// downloads). Cheap: called each frame; the session's limit changes only when it
    /// differs (`Torrents::set_download_limit`), and a session that starts takes it at once.
    pub fn set_cap(&self, rate: Option<u64>) {
        *self.cap.lock().unwrap() = rate;
        if let Some(t) = self.get() {
            t.set_download_limit(rate);
        }
    }

    /// The settings for the next session (Torrent settings changed, or at start).
    pub fn set_config(&self, cfg: Option<Config>) {
        *self.wanted.lock().unwrap() = cfg;
    }

    /// The session if it already runs.
    pub fn get(&self) -> Option<Arc<Torrents>> {
        self.cell.try_lock().ok().and_then(|s| s.clone())
    }

    /// Whether the running session was started with other settings than those wanted now.
    pub fn settings_changed(&self) -> bool {
        self.get().is_some() && *self.running.lock().unwrap() != *self.wanted.lock().unwrap()
    }

    /// The session, started if needed. `known`: the torrents the list has (the rest of what
    /// the session restores is forgotten).
    pub async fn session(&self, known: HashSet<String>) -> Result<Arc<Torrents>, String> {
        let mut cell = self.cell.lock().await;
        if let Some(t) = cell.as_ref() {
            return Ok(t.clone());
        }
        // i18n: an error: the app has no folder for its torrent data
        state_dir().ok_or(crate::i18n::t("no data folder for torrents"))?;
        if let Some(why) = route_refusal(&crate::settings::own_route()) {
            return Err(why);
        }
        let cfg = self.wanted.lock().unwrap().clone().ok_or_else(|| {
            // i18n: an error: Torrent settings ask for the proxy, and Options → Proxy / Socks has no SOCKS5 one
            crate::i18n::t(
                "Torrent settings say to use the proxy, but Proxy / Socks has no SOCKS5 proxy",
            )
            .to_owned()
        })?;
        let t = Torrents::start(cfg.clone()).await.map_err(|e| {
            crate::i18n::fmt(
                // i18n: an error, with the reason
                crate::i18n::t("torrents could not start: {error}"),
                &[("error", &e.to_string())],
            )
        })?;
        for hash in t.hashes() {
            if known.contains(&hash) {
                let _ = t.pause(&hash).await;
            } else {
                let _ = t.remove(&hash, false).await;
            }
        }
        t.set_download_limit(*self.cap.lock().unwrap());
        t.set_upload_limit(*self.upload.lock().unwrap());
        let t = Arc::new(t);
        *cell = Some(t.clone());
        *self.running.lock().unwrap() = Some(cfg);
        Ok(t)
    }

    /// Stop the session now, whatever it is doing (the network adapter changed, #35: its sockets
    /// were bound to the one before, or to none). The next one starts as things are then.
    pub fn stop_now(&self, rt: &tokio::runtime::Handle) {
        let Some(t) = self.cell.try_lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        *self.running.lock().unwrap() = None;
        rt.spawn(async move { t.stop().await });
    }

    /// Stop the session when no torrent needs it (`idle`) and Torrent settings say so, or its
    /// settings changed: it stops listening, and the next one starts with the new ones.
    pub fn stop_if_idle(&self, rt: &tokio::runtime::Handle, idle: bool, stop_idle: bool) {
        if !idle || !(stop_idle || self.settings_changed()) {
            return;
        }
        let Some(t) = self.cell.try_lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        *self.running.lock().unwrap() = None;
        rt.spawn(async move { t.stop().await });
    }

    /// Keep sharing (true) or stop sharing (false) finished torrents: `hashes` are the list's
    /// complete ones. Sharing starts the session if needed.
    pub fn set_sharing(
        &self,
        rt: &tokio::runtime::Handle,
        known: HashSet<String>,
        hashes: Vec<String>,
        on: bool,
    ) {
        if hashes.is_empty() || (!on && self.get().is_none()) {
            return;
        }
        let host = self.clone();
        rt.spawn(async move {
            let Ok(t) = host.session(known).await else {
                return;
            };
            for h in hashes {
                let _ = if on {
                    t.resume(&h).await
                } else {
                    t.pause(&h).await
                };
            }
        });
    }

    /// Forget a torrent the list no longer has; its files stay unless `files` (and the session
    /// runs: it knows which files are the torrent's).
    pub fn forget(&self, rt: &tokio::runtime::Handle, hash: &str, files: bool) {
        if let Some(t) = self.get() {
            let hash = hash.to_owned();
            rt.spawn(async move {
                let _ = t.remove(&hash, files).await;
            });
        }
        if let Some(f) = torrent_file(hash) {
            let _ = std::fs::remove_file(f);
        }
    }
}

enum RunState {
    Starting,
    Ready(Arc<Torrents>),
    Failed(String),
}

/// A running torrent download.
pub struct Run {
    state: Arc<Mutex<RunState>>,
    rt: tokio::runtime::Handle,
    hash: String,
    url: url::Url,
    stopped: Arc<Mutex<bool>>,
    /// Keep sharing once finished (D-018; off by default).
    keep_sharing: bool,
    /// Where its files are (`TorrentSpec::folder`).
    folder: PathBuf,
    /// Its files' Mark-of-the-Web (#40): 0 not yet, 1 once librqbit created them, 2 finished.
    marked: Arc<std::sync::atomic::AtomicU8>,
    /// Its own limits, bytes/s, (download, upload): the download one from the Speed Limiter's
    /// "for one file" or the progress window, the upload one from the progress window
    /// (`TorrentSpec::upload_limit`). Set on the torrent once it is in the session, and again
    /// on each change.
    limit: Arc<Mutex<(Option<u64>, Option<u64>)>>,
}

/// The origin a torrent's files are marked with: not one website, and never the magnet link
/// (its trackers), as Windows' guidance for an unknown source says (and qBittorrent does).
const TORRENT_ORIGIN: &str = "about:internet";

/// Mark `files` (padding left out) under `folder` as downloaded from the internet, as web
/// downloads are (Windows' zone, so SmartScreen and Office's Protected View check them; macOS'
/// quarantine; Linux's origin attribute, which nothing enforces). Every file of the torrent,
/// ticked or not: librqbit creates them all.
fn mark_files(folder: &Path, files: &[mdm_torrent::FileEntry]) {
    for f in files.iter().filter(|f| !f.padding) {
        let path = folder.join(&f.path);
        if !path.is_file() {
            continue;
        }
        #[cfg(windows)]
        crate::win32::mark_downloaded(&path, TORRENT_ORIGIN, None);
        #[cfg(unix)]
        crate::unix::mark_downloaded(&path, TORRENT_ORIGIN, None);
    }
}

/// What the progress window's torrent tabs show.
pub struct Details {
    /// Each file (padding left out): its entry, chosen?, bytes there.
    pub files: Vec<(mdm_torrent::FileEntry, bool, u64)>,
    pub piece_length: u32,
    pub peers: Vec<mdm_torrent::Peer>,
    pub trackers: Vec<String>,
    pub pieces: Vec<bool>,
    pub stats: Option<mdm_torrent::Stats>,
}

impl Run {
    /// Start (or resume) `spec`'s torrent, at most `limit` bytes/s down for this torrent (and
    /// its own upload limit, `spec.upload_limit`).
    pub fn start(
        host: &Host,
        rt: &tokio::runtime::Handle,
        spec: &TorrentSpec,
        url: url::Url,
        known: HashSet<String>,
        limit: Option<u64>,
        keep_sharing: bool,
    ) -> Run {
        let state = Arc::new(Mutex::new(RunState::Starting));
        let own = Arc::new(Mutex::new((limit, spec.upload_limit)));
        let (out, host, spec2, own2) = (state.clone(), host.clone(), spec.clone(), own.clone());
        rt.spawn(async move {
            let res = async {
                let t = host.session(known).await?;
                if t.has(&spec2.hash) {
                    t.resume(&spec2.hash)
                        .await
                        .map_err(|e| crate::errors::torrent(&e))?;
                } else {
                    let file = torrent_file(&spec2.hash).ok_or(crate::i18n::t("no data folder"))?;
                    let bytes = std::fs::read(&file).map_err(|_| {
                        // i18n: an error: the copy of the .torrent file the app keeps is missing
                        crate::i18n::t("the .torrent file kept for it is gone").to_owned()
                    })?;
                    t.add(
                        &bytes,
                        &spec2.folder,
                        spec2.files.clone(),
                        Vec::new(),
                        false,
                    )
                    .await
                    .map_err(|e| crate::errors::torrent(&e))?;
                }
                Ok::<_, String>(t)
            }
            .await;
            match res {
                Ok(t) => {
                    // Under the limit's lock, so a change made meanwhile (set_speed_limit) is
                    // either seen here or applied there once it is Ready. librqbit keeps a
                    // torrent's limits only while the session runs: set them each start.
                    let own = own2.lock().unwrap();
                    *out.lock().unwrap() = RunState::Ready(t.clone());
                    let _ = t.set_torrent_limits(&spec2.hash, own.0, own.1);
                }
                Err(e) => *out.lock().unwrap() = RunState::Failed(e),
            }
        });
        Run {
            state,
            rt: rt.clone(),
            hash: spec.hash.clone(),
            url,
            stopped: Arc::new(Mutex::new(false)),
            keep_sharing,
            folder: spec.folder.clone(),
            marked: Arc::new(std::sync::atomic::AtomicU8::new(0)),
            limit: own,
        }
    }

    /// Mark its files once librqbit has created them (it leaves Checking) and again once it
    /// finishes, off the window's thread.
    fn mark_when(&self, t: &Arc<Torrents>, phase: Phase) {
        use std::sync::atomic::Ordering;
        // Paused: perhaps never started, its files not created yet.
        let step = match phase {
            Phase::Downloading => 1,
            Phase::Finished => 2,
            Phase::Checking | Phase::Paused | Phase::Error => return,
        };
        if self.marked.fetch_max(step, Ordering::SeqCst) >= step {
            return;
        }
        let Some((files, _, _)) = t.files(&self.hash) else {
            return;
        };
        let folder = self.folder.clone();
        self.rt.spawn_blocking(move || mark_files(&folder, &files));
    }

    fn session(&self) -> Option<Arc<Torrents>> {
        match &*self.state.lock().unwrap() {
            RunState::Ready(t) => Some(t.clone()),
            _ => None,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut s = Snapshot {
            state: State::Probing,
            url: self.url.clone(),
            file: None,
            size: None,
            downloaded: 0,
            resumable: Some(true),
            segments: Vec::new(),
            connections: Vec::new(),
            error: None,
            failure: None,
            events: Vec::new(),
        };
        if *self.stopped.lock().unwrap() {
            s.state = State::Cancelled;
            if let Some(st) = self.session().and_then(|t| t.stats(&self.hash).ok()) {
                s.downloaded = st.done;
                s.size = Some(st.total);
            }
            return s;
        }
        let t = match &*self.state.lock().unwrap() {
            RunState::Starting => return s,
            RunState::Failed(e) => {
                s.state = State::Failed;
                s.error = Some(e.clone());
                return s;
            }
            RunState::Ready(t) => t.clone(),
        };
        let Ok(st) = t.stats(&self.hash) else {
            s.state = State::Failed;
            // i18n: an error: the torrent is gone from the torrent session
            s.error = Some(crate::i18n::t("the torrent is no longer there").into());
            return s;
        };
        s.downloaded = st.done;
        s.size = Some(st.total);
        s.state = match st.phase {
            Phase::Checking => State::Verifying,
            Phase::Downloading => State::Downloading,
            Phase::Finished => State::Done,
            Phase::Paused => State::Cancelled,
            Phase::Error => State::Failed,
        };
        s.error = st.error;
        self.mark_when(&t, st.phase);
        // Done: it stops sharing (a download manager finishes downloads; D-018), unless the
        // person chose to keep sharing.
        if s.state == State::Done && !self.keep_sharing {
            self.pause_in_background(&t);
        }
        s
    }

    fn pause_in_background(&self, t: &Arc<Torrents>) {
        let (t, hash) = (t.clone(), self.hash.clone());
        self.rt.spawn(async move {
            let _ = t.pause(&hash).await;
        });
    }

    /// Stop: pause it (the pieces stay; starting again continues).
    pub fn cancel(&self) {
        *self.stopped.lock().unwrap() = true;
        if let Some(t) = self.session() {
            self.pause_in_background(&t);
        }
    }

    /// This torrent's own download limit (#39: not every torrent's): now if it is in the
    /// session, else once it is.
    pub fn set_speed_limit(&self, rate: Option<u64>) {
        let mut own = self.limit.lock().unwrap();
        own.0 = rate;
        if let Some(t) = self.session() {
            let _ = t.set_torrent_limits(&self.hash, own.0, own.1);
        }
    }

    pub fn speed_limit(&self) -> Option<u64> {
        self.limit.lock().unwrap().0
    }

    /// This torrent's own upload limit: now if it is in the session, else once it is.
    pub fn set_upload_limit(&self, rate: Option<u64>) {
        let mut own = self.limit.lock().unwrap();
        own.1 = rate;
        if let Some(t) = self.session() {
            let _ = t.set_torrent_limits(&self.hash, own.0, own.1);
        }
    }

    /// Bytes received from peers since it last started (for the global cap, which HTTP
    /// downloads share with torrents); None until it is in the session.
    pub fn fetched(&self) -> Option<u64> {
        let t = self.session()?;
        t.stats(&self.hash).ok().map(|s| s.fetched)
    }

    pub fn details(&self) -> Details {
        let Some(t) = self.session() else {
            return Details {
                files: Vec::new(),
                piece_length: 0,
                peers: Vec::new(),
                trackers: Vec::new(),
                pieces: Vec::new(),
                stats: None,
            };
        };
        let stats = t.stats(&self.hash).ok();
        let done = stats
            .as_ref()
            .map(|s| s.file_done.clone())
            .unwrap_or_default();
        let (entries, chosen, piece_length) = t.files(&self.hash).unwrap_or_default();
        Details {
            files: entries
                .into_iter()
                .enumerate()
                .filter(|(_, f)| !f.padding)
                .map(|(i, f)| {
                    let on = chosen.as_ref().is_none_or(|c| c.contains(&i));
                    (f, on, done.get(i).copied().unwrap_or(0))
                })
                .collect(),
            piece_length,
            peers: t.peers(&self.hash),
            trackers: t.trackers(&self.hash),
            pieces: t.pieces(&self.hash),
            stats,
        }
    }
}

/// A magnet link for a torrent (what the list shows as its address).
pub fn magnet_url(hash: &str, name: &str) -> url::Url {
    let mut u = url::Url::parse(&format!("magnet:?xt=urn:btih:{hash}")).expect("valid magnet");
    u.query_pairs_mut().append_pair("dn", name);
    u
}

/// The file the list row stands for: the torrent's folder, or its one file.
pub fn dest_for(meta: &mdm_torrent::Meta, dir: &Path, files: Option<&[usize]>) -> PathBuf {
    let folder = meta.folder_in(dir);
    let chosen: Vec<&mdm_torrent::FileEntry> = meta
        .files
        .iter()
        .enumerate()
        .filter(|(i, f)| !f.padding && files.is_none_or(|c| c.contains(i)))
        .map(|(_, f)| f)
        .collect();
    match chosen.as_slice() {
        [one] if meta.files.iter().filter(|f| !f.padding).count() == 1 => folder.join(&one.path),
        _ => folder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #39: what a torrent sent adds up across its runs: librqbit's count is 0 while paused
    /// and starts again at 0 on each resume.
    #[test]
    fn sent_bytes_add_up_across_resumes() {
        let mut a = Accountant::default();
        let mut total = 0;
        for live in [100, 250, 250, 0, 0, 40, 90] {
            total += a.sent("h", live);
        }
        assert_eq!(total, 250 + 90, "first run 250, then 90 after the pause");
        // Another torrent counts on its own; a forgotten one starts afresh.
        assert_eq!(a.sent("other", 30), 30);
        a.keep(&HashSet::from(["other".to_owned()]));
        assert_eq!(a.sent("h", 90), 90);
    }

    #[test]
    fn magnet_links_carry_the_name() {
        let u = magnet_url("0123456789abcdef0123456789abcdef01234567", "A & B");
        assert_eq!(u.scheme(), "magnet");
        assert!(u.as_str().contains("dn=A+%26+B"), "{u}");
    }

    /// #40: a torrent's files get the internet zone, with no address but `about:internet`
    /// (never the magnet link); padding files and files not there are left alone.
    #[cfg(windows)]
    #[test]
    fn torrent_files_are_marked_from_the_internet() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a.exe"), b"x").unwrap();
        std::fs::write(dir.path().join(".pad0"), b"").unwrap();
        let entry = |p: &str, padding| mdm_torrent::FileEntry {
            path: PathBuf::from(p),
            size: 1,
            padding,
        };
        mark_files(
            dir.path(),
            &[
                entry("sub/a.exe", false),
                entry(".pad0", true),
                entry("gone.bin", false),
            ],
        );
        let zone = |p: &Path| {
            let mut s = p.as_os_str().to_owned();
            s.push(":Zone.Identifier");
            std::fs::read_to_string(s).ok()
        };
        assert_eq!(
            zone(&dir.path().join("sub/a.exe")).as_deref(),
            Some("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=about:internet\r\n")
        );
        assert_eq!(
            zone(&dir.path().join(".pad0")),
            None,
            "padding isn't marked"
        );
        assert!(!dir.path().join("gone.bin").exists(), "nothing created");
    }
}

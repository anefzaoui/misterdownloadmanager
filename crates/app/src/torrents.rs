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
use tokio::sync::OnceCell;

/// What a torrent download keeps in the list (downloads.json).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentSpec {
    /// The info hash, 40 hex digits; the .torrent itself is in [`torrent_file`].
    pub hash: String,
    /// The chosen files (indexes into the torrent's list); None: all of them.
    pub files: Option<Vec<usize>>,
    /// Where its files go (a multi-file torrent's own folder included).
    pub folder: PathBuf,
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

/// The app's one session, started on first use.
#[derive(Clone)]
pub struct Host {
    cell: Arc<OnceCell<Arc<Torrents>>>,
}

impl Host {
    pub fn new() -> Self {
        Host {
            cell: Arc::new(OnceCell::new()),
        }
    }

    /// The session if it already runs.
    pub fn get(&self) -> Option<Arc<Torrents>> {
        self.cell.get().cloned()
    }

    /// The session, started if needed. `known`: the torrents the list has (the rest of what
    /// the session restores is forgotten).
    pub async fn session(&self, known: HashSet<String>) -> Result<Arc<Torrents>, String> {
        self.cell
            .get_or_try_init(|| async move {
                // i18n: an error: the app has no folder for its torrent data
                let dir = state_dir().ok_or(crate::i18n::t("no data folder for torrents"))?;
                let t = Torrents::start(Config {
                    state_dir: dir,
                    listen: true,
                    dht: true,
                    offline: false,
                })
                .await
                .map_err(|e| {
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
                Ok(Arc::new(t))
            })
            .await
            .cloned()
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
    /// Start (or resume) `spec`'s torrent, at most `limit` bytes/s for the whole session.
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
        let (out, host, spec2) = (state.clone(), host.clone(), spec.clone());
        rt.spawn(async move {
            let res = async {
                let t = host.session(known).await?;
                t.set_download_limit(limit);
                if t.has(&spec2.hash) {
                    t.resume(&spec2.hash).await.map_err(|e| e.to_string())?;
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
                    .map_err(|e| e.to_string())?;
                }
                Ok::<_, String>(t)
            }
            .await;
            *out.lock().unwrap() = match res {
                Ok(t) => RunState::Ready(t),
                Err(e) => RunState::Failed(e),
            };
        });
        Run {
            state,
            rt: rt.clone(),
            hash: spec.hash.clone(),
            url,
            stopped: Arc::new(Mutex::new(false)),
            keep_sharing,
        }
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

    pub fn set_speed_limit(&self, rate: Option<u64>) {
        if let Some(t) = self.session() {
            t.set_download_limit(rate);
        }
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

    #[test]
    fn magnet_links_carry_the_name() {
        let u = magnet_url("0123456789abcdef0123456789abcdef01234567", "A & B");
        assert_eq!(u.scheme(), "magnet");
        assert!(u.as_str().contains("dn=A+%26+B"), "{u}");
    }
}

//! BitTorrent and magnet downloads (D-014): librqbit, pure Rust, behind the few calls the app
//! needs. Read what a torrent holds without starting it, add it with the files and folder the
//! person chose, then watch it (stats, peers, trackers, pieces), pause, resume and remove it.
//! Torrents are named by their info hash (40 hex digits), which the app keeps in its own list.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The library itself, for what this wrapper doesn't cover (making torrents, tests).
pub use librqbit;
use librqbit::api::{Api, TorrentIdOrHash};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, ListenerOptions, Session,
    SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};

pub mod blocklist;
mod names;
pub mod trackers;
pub use names::UnsafeName;

#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("this is not a magnet link or a .torrent file")]
    NotATorrent,
    /// A file name this system can't write safely (`names`, #40): the torrent is refused, nothing
    /// is written.
    #[error("this torrent has a file name that isn't safe to write here: {} ({})", .0.path, .0.why)]
    UnsafeName(UnsafeName),
    #[error("no peer sent the torrent's file list in time")]
    MetadataTimeout,
    #[error("this torrent is not in the list")]
    Unknown,
    #[error("{0}")]
    Engine(String),
}

fn engine(e: impl std::fmt::Display) -> TorrentError {
    TorrentError::Engine(e.to_string())
}

/// The most a .torrent file may be (#40): the largest real ones (a great many files) are a few
/// MB; anything bigger isn't one, and would be read whole into memory.
pub const MAX_TORRENT_FILE: u64 = 10 << 20;

/// A .torrent file from disk, at most `MAX_TORRENT_FILE` (a bigger one: `InvalidData`).
pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_TORRENT_FILE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TORRENT_FILE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "larger than a .torrent file can be",
        ));
    }
    Ok(bytes)
}

/// A .torrent file's bytes: a torrent, whose names are safe for this system (`names`).
fn check_bytes(torrent: &[u8]) -> Result<(), TorrentError> {
    let meta = librqbit::torrent_from_bytes(torrent).map_err(|_| TorrentError::NotATorrent)?;
    safe_names(
        meta.info
            .data
            .validate()
            .map_err(engine)?
            .iter_file_details(),
    )
}

/// Before librqbit restores its saved torrents (`session.json` and a `<hash>.torrent` for each,
/// in `dir`), take out the ones the name check refuses, and those saved without their .torrent
/// (whose names it can't check): the app adds those again itself, through `add` and its own
/// copy. The rest keep only the trackers MDM may contact (`trackers`, `proxied`), in the list and
/// in their .torrent. Rewritten through a temporary file; a list that can't be read is left to
/// librqbit.
fn prepare_saved(dir: &Path, proxied: bool) {
    let db = dir.join("session.json");
    let Ok(text) = std::fs::read_to_string(&db) else {
        return;
    };
    let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(saved) = doc.get_mut("torrents").and_then(|t| t.as_object_mut()) else {
        return;
    };
    let mut changed = false;
    saved.retain(|_, entry| {
        let Some(hash) = entry.get("info_hash").and_then(|h| h.as_str()) else {
            changed = true;
            return false;
        };
        let hash = hash.to_ascii_lowercase();
        let file = dir.join(format!("{hash}.torrent"));
        let bytes = std::fs::read(&file).ok().filter(|b| check_bytes(b).is_ok());
        let Some(bytes) = bytes else {
            for ext in ["torrent", "bitv"] {
                let _ = std::fs::remove_file(dir.join(format!("{hash}.{ext}")));
            }
            changed = true;
            return false;
        };
        if let Some(filtered) = trackers::filter_torrent(&bytes, proxied).filter(|f| *f != bytes) {
            let _ = std::fs::write(&file, filtered);
        }
        if let Some(list) = entry.get_mut("trackers").and_then(|t| t.as_array_mut()) {
            let before = list.len();
            list.retain(|t| t.as_str().is_some_and(|t| trackers::allowed(t, proxied)));
            changed |= list.len() != before;
        }
        true
    });
    if !changed {
        return;
    }
    let Ok(out) = serde_json::to_string(&doc) else {
        return;
    };
    let tmp = dir.join("session.json.mdm-tmp");
    if std::fs::write(&tmp, &out).is_ok()
        && std::fs::read_to_string(&tmp).is_ok_and(|back| back == out)
    {
        let _ = std::fs::rename(&tmp, &db);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The torrent's file names checked for this system (`names`); padding files are never written.
fn safe_names<'a, B: AsRef<[u8]> + 'a>(
    files: impl Iterator<Item = librqbit::FileDetails<'a, B>>,
) -> Result<(), TorrentError> {
    names::check(files.filter(|f| !f.attrs().padding).map(|f| {
        f.filename
            .iter_components()
            .map(|part| part.into_owned())
            .collect::<Vec<String>>()
    }))
    .map_err(TorrentError::UnsafeName)
}

/// Where a torrent comes from.
#[derive(Clone, Debug)]
pub enum Source {
    Magnet(String),
    /// The bytes of a .torrent file.
    File(Vec<u8>),
}

impl Source {
    /// A magnet link, or nothing.
    pub fn magnet(text: &str) -> Option<Source> {
        let t = text.trim();
        t.get(..8)
            .is_some_and(|s| s.eq_ignore_ascii_case("magnet:?"))
            .then(|| Source::Magnet(t.to_owned()))
    }
}

/// One file inside a torrent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative to the torrent's folder.
    pub path: PathBuf,
    pub size: u64,
    /// BEP 47 padding files: never shown, never chosen.
    pub padding: bool,
}

/// What a torrent holds, before anything is downloaded.
#[derive(Clone, Debug)]
pub struct Meta {
    pub name: String,
    pub info_hash: String,
    pub files: Vec<FileEntry>,
    pub piece_length: u32,
    /// A .torrent file for it (for a magnet: built from the metadata the peers sent), kept by the
    /// app so the torrent can be added again without asking the swarm.
    pub torrent: Vec<u8>,
    /// Peers already met while fetching the metadata.
    pub peers: Vec<SocketAddr>,
}

impl Meta {
    pub fn total(&self) -> u64 {
        self.files
            .iter()
            .filter(|f| !f.padding)
            .map(|f| f.size)
            .sum()
    }

    /// Where its files go when saved in `dir`: a torrent of several files gets its own folder
    /// (named after it), a single file goes straight in.
    pub fn folder_in(&self, dir: &Path) -> PathBuf {
        if self.files.iter().filter(|f| !f.padding).count() > 1 {
            let name: String = self
                .name
                .chars()
                .map(|c| {
                    if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            let name = name.trim().trim_end_matches('.');
            dir.join(if name.is_empty() {
                &self.info_hash
            } else {
                name
            })
        } else {
            dir.to_owned()
        }
    }

    /// Its files (padding never counts) that already exist where it would write them in `dir`
    /// (`folder_in`). Unticked ones count too: librqbit opens every file of the torrent, and
    /// writes into an unticked one the parts of a piece it shares with a ticked one.
    pub fn existing_in(&self, dir: &Path) -> Vec<PathBuf> {
        let base = self.folder_in(dir);
        self.files
            .iter()
            .filter(|f| !f.padding)
            .map(|f| base.join(&f.path))
            .filter(|p| p.exists())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Checking the pieces already on disk.
    Checking,
    Downloading,
    /// Everything chosen is there (it may still share).
    Finished,
    Paused,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub phase: Phase,
    /// Bytes of the chosen files that are there and checked.
    pub done: u64,
    /// Bytes of the chosen files.
    pub total: u64,
    pub uploaded: u64,
    /// Bytes received from peers since it last started (0 while paused; it starts again at 0
    /// each time it resumes).
    pub fetched: u64,
    /// Bytes/s.
    pub down_speed: f64,
    pub up_speed: f64,
    /// Per file, bytes there (same order as `Meta::files`).
    pub file_done: Vec<u64>,
    pub peers_live: usize,
    pub peers_seen: usize,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub addr: String,
    pub client: Option<String>,
    /// Bytes received from it so far.
    pub fetched: u64,
    pub uploaded: u64,
    /// "tcp" or "utp".
    pub kind: Option<String>,
}

/// How the session runs (#40: each of these is the person's to choose, Torrent settings).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Where librqbit keeps its resume data and DHT nodes (in our data folder, portable-aware).
    pub state_dir: PathBuf,
    /// Accept connections from peers (better speed; Windows may ask about the firewall once).
    pub listen: bool,
    /// Ask the router (UPnP) to forward the listening port.
    pub upnp: bool,
    /// The port to listen on; None: one picked at each start.
    pub port: Option<u16>,
    /// The DHT: finding peers without trackers.
    pub dht: bool,
    /// Local peer discovery (LSD): it tells the local network which torrents this computer has.
    pub lsd: bool,
    /// A `socks5://` (or `socks5h://`) proxy for peers and HTTP trackers. librqbit sends DHT,
    /// LSD, UDP trackers, UPnP and incoming connections around it, so with a proxy those are off
    /// and UDP trackers dropped (`trackers`).
    pub proxy: Option<String>,
    /// A blocklist MDM converted (`blocklist::convert`, P2P lines): peers in it are refused.
    pub blocklist: Option<PathBuf>,
    /// Tests: no trackers, no DHT, local peers only.
    pub offline: bool,
}

impl Config {
    /// Everything on, as BitTorrent clients usually are; no proxy, no blocklist.
    pub fn new(state_dir: PathBuf) -> Config {
        Config {
            state_dir,
            listen: true,
            upnp: true,
            port: None,
            dht: true,
            lsd: true,
            proxy: None,
            blocklist: None,
            offline: false,
        }
    }
}

pub struct Torrents {
    session: Arc<Session>,
    api: Api,
    /// Through a proxy: UDP trackers are dropped (`trackers::allowed`).
    proxied: bool,
    /// One add at a time: librqbit gives a new torrent the saved ids' highest plus one, so two
    /// adds at once both took id 0, and the second was answered "already managed" with the
    /// first one's handle: it was never added (two torrents started together, #39).
    adding: tokio::sync::Mutex<()>,
}

/// The lowest limit a torrent can have, bytes/s: librqbit's limiter holds one second's budget
/// and asks it for a whole 16 KiB block at once; under that, every request fails and each peer
/// is dropped, so the torrent would stall instead of going slowly.
pub const MIN_RATE: u64 = 16 * 1024;

/// A rate for librqbit's limiter: 0 and None are unlimited; at least `MIN_RATE`, and above
/// u32::MAX bytes/s is clamped.
fn rate(bps: Option<u64>) -> Option<NonZeroU32> {
    bps.filter(|b| *b > 0)
        .and_then(|b| NonZeroU32::new(b.clamp(MIN_RATE, u32::MAX as u64) as u32))
}

fn id(hash: &str) -> Result<TorrentIdOrHash, TorrentError> {
    TorrentIdOrHash::parse(hash).map_err(|_| TorrentError::Unknown)
}

impl Torrents {
    pub async fn start(cfg: Config) -> Result<Torrents, TorrentError> {
        std::fs::create_dir_all(&cfg.state_dir).map_err(engine)?;
        let proxied = cfg.proxy.is_some();
        let dht = (cfg.dht && !cfg.offline && !proxied).then(|| librqbit::DhtSessionConfig {
            persistence: Some(librqbit::dht::DhtPersistenceConfig {
                dump_interval: None,
                config_filename: Some(cfg.state_dir.join("dht.json")),
            }),
            ..Default::default()
        });
        let listen = (cfg.listen && !proxied).then(|| ListenerOptions {
            listen_addr: if cfg.offline {
                (Ipv4Addr::LOCALHOST, 0).into()
            } else {
                let mut any = ListenerOptions::default().listen_addr;
                any.set_port(cfg.port.unwrap_or(0));
                any
            },
            enable_upnp_port_forwarding: cfg.upnp && !cfg.offline,
            ipv4_only: cfg.offline,
            ..Default::default()
        });
        // A list librqbit can't read would stop the session from starting: only a file there.
        let blocklist_url = cfg
            .blocklist
            .as_deref()
            .filter(|p| p.is_file())
            .and_then(|p| url::Url::from_file_path(p).ok())
            .map(|u| u.to_string());
        let opts = SessionOptions {
            dht,
            listen,
            connect: cfg.proxy.clone().map(|proxy| ConnectionOptions {
                proxy_url: Some(proxy),
                ..Default::default()
            }),
            blocklist_url,
            disable_trackers: cfg.offline,
            disable_local_service_discovery: cfg.offline || !cfg.lsd || proxied,
            ipv4_only: cfg.offline,
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(cfg.state_dir.join("session")),
            }),
            // Major and minor only: the exact build would help someone aiming at one (#40).
            client_name_and_version: Some(format!(
                "MisterDownloadManager/{}.{}",
                env!("CARGO_PKG_VERSION_MAJOR"),
                env!("CARGO_PKG_VERSION_MINOR")
            )),
            ..Default::default()
        };
        prepare_saved(&cfg.state_dir.join("session"), proxied);
        let session = Session::new_with_opts(cfg.state_dir.join("unsorted"), opts)
            .await
            .map_err(engine)?;
        let api = Api::new(session.clone(), None);
        Ok(Torrents {
            session,
            api,
            proxied,
            adding: tokio::sync::Mutex::new(()),
        })
    }

    /// The file list, without downloading. A magnet asks the swarm, for at most `wait`.
    pub async fn metadata(&self, src: &Source, wait: Duration) -> Result<Meta, TorrentError> {
        let add = match src {
            // Only the trackers MDM may contact (#40, `trackers`).
            Source::Magnet(m) => AddTorrent::from_url(trackers::filter_magnet(m, self.proxied)),
            Source::File(b) => AddTorrent::from_bytes(
                trackers::filter_torrent(b, self.proxied).unwrap_or_else(|| b.clone()),
            ),
        };
        let opts = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        let res = tokio::time::timeout(wait, self.session.add_torrent(add, Some(opts)))
            .await
            .map_err(|_| TorrentError::MetadataTimeout)?
            .map_err(|e| {
                let text = format!("{e:#}");
                if text.contains("decoding torrent") || text.contains("valid magnet") {
                    TorrentError::NotATorrent
                } else {
                    engine(text)
                }
            })?;
        let AddTorrentResponse::ListOnly(r) = res else {
            return Err(engine("the torrent was started instead of listed"));
        };
        // Refused before anyone picks files from it (#40).
        safe_names(r.info.iter_file_details())?;
        let files = r
            .info
            .iter_file_details()
            .map(|f| FileEntry {
                path: f.filename.to_pathbuf(),
                size: f.len,
                padding: f.attrs().padding,
            })
            .collect();
        Ok(Meta {
            name: r
                .info
                .name()
                .map(|n| n.into_owned())
                .unwrap_or_else(|| r.info_hash.as_string()),
            info_hash: r.info_hash.as_string(),
            files,
            piece_length: r.info.lengths().default_piece_length(),
            torrent: r.torrent_bytes.to_vec(),
            peers: r.seen_peers,
        })
    }

    /// Start (or keep) a torrent: `files` are indexes into `Meta::files` (None: all), each saved
    /// at `folder` + its path (see `Meta::folder_in`).
    pub async fn add(
        &self,
        torrent: &[u8],
        folder: &Path,
        files: Option<Vec<usize>>,
        peers: Vec<SocketAddr>,
        paused: bool,
    ) -> Result<String, TorrentError> {
        // Checked here too, whatever the caller (the app, the command line, a saved torrent).
        check_bytes(torrent)?;
        let wanted = librqbit::torrent_from_bytes(torrent)
            .map_err(|_| TorrentError::NotATorrent)?
            .info_hash
            .as_string();
        let torrent =
            trackers::filter_torrent(torrent, self.proxied).unwrap_or_else(|| torrent.to_vec());
        let opts = AddTorrentOptions {
            paused,
            only_files: files,
            output_folder: Some(folder.to_string_lossy().into_owned()),
            // Resuming after a restart finds our own partial files there.
            overwrite: true,
            initial_peers: (!peers.is_empty()).then_some(peers),
            ..Default::default()
        };
        let res = {
            let _one = self.adding.lock().await;
            self.session
                .add_torrent(AddTorrent::from_bytes(torrent), Some(opts))
                .await
                .map_err(engine)?
        };
        let handle = res
            .into_handle()
            .ok_or_else(|| engine("the torrent was listed instead of started"))?;
        let got = handle.info_hash().as_string();
        if got != wanted {
            return Err(engine(format!(
                "the session answered with another torrent ({got}) for {wanted}"
            )));
        }
        Ok(got)
    }

    /// Whether the session still has it (resume data from an earlier run).
    pub fn has(&self, hash: &str) -> bool {
        id(hash).is_ok_and(|i| self.session.get(i).is_some())
    }

    pub fn stats(&self, hash: &str) -> Result<Stats, TorrentError> {
        let h = self.session.get(id(hash)?).ok_or(TorrentError::Unknown)?;
        let s = h.stats();
        let live = s.live.as_ref();
        let mib = |m: f64| m * 1024.0 * 1024.0;
        let phase = match s.state {
            TorrentStatsState::Initializing { .. } => Phase::Checking,
            TorrentStatsState::Paused => Phase::Paused,
            TorrentStatsState::Error => Phase::Error,
            TorrentStatsState::Live if s.finished => Phase::Finished,
            TorrentStatsState::Live => Phase::Downloading,
        };
        Ok(Stats {
            phase,
            done: s.progress_bytes,
            total: s.total_bytes,
            uploaded: s.uploaded_bytes,
            fetched: live.map_or(0, |l| l.snapshot.fetched_bytes),
            down_speed: live.map_or(0.0, |l| mib(l.download_speed.mbps)),
            up_speed: live.map_or(0.0, |l| mib(l.upload_speed.mbps)),
            file_done: s.file_progress.clone(),
            peers_live: live.map_or(0, |l| l.snapshot.peer_stats.live as usize),
            peers_seen: live.map_or(0, |l| l.snapshot.peer_stats.seen as usize),
            error: s.error.clone(),
        })
    }

    /// The peers it is talking to.
    pub fn peers(&self, hash: &str) -> Vec<Peer> {
        let Ok(i) = id(hash) else { return Vec::new() };
        let Ok(snap) = self.api.api_peer_stats(i, Default::default()) else {
            return Vec::new();
        };
        let mut out: Vec<Peer> = snap
            .peers
            .into_iter()
            .map(|(addr, p)| Peer {
                addr,
                client: p.client_name,
                fetched: p.counters.fetched_bytes,
                uploaded: p.counters.uploaded_bytes,
                kind: p.conn_kind.map(|k| format!("{k:?}").to_lowercase()),
            })
            .collect();
        out.sort_by(|a, b| b.fetched.cmp(&a.fetched).then(a.addr.cmp(&b.addr)));
        out
    }

    /// Its files (as in `Meta::files`), which are chosen (None: all), and the piece size.
    pub fn files(&self, hash: &str) -> Option<(Vec<FileEntry>, Option<Vec<usize>>, u32)> {
        let h = self.session.get(id(hash).ok()?)?;
        let (files, piece) = h
            .with_metadata(|m| {
                let files = m
                    .info
                    .iter_file_details()
                    .map(|f| FileEntry {
                        path: f.filename.to_pathbuf(),
                        size: f.len,
                        padding: f.attrs().padding,
                    })
                    .collect::<Vec<_>>();
                (files, m.info.lengths().default_piece_length())
            })
            .ok()?;
        Some((files, h.only_files(), piece))
    }

    pub fn trackers(&self, hash: &str) -> Vec<String> {
        let Some(h) = id(hash).ok().and_then(|i| self.session.get(i)) else {
            return Vec::new();
        };
        let mut t: Vec<String> = h.shared().trackers.iter().map(|u| u.to_string()).collect();
        t.sort();
        t
    }

    /// Which pieces are there (empty while it isn't checked yet).
    pub fn pieces(&self, hash: &str) -> Vec<bool> {
        let Ok(i) = id(hash) else { return Vec::new() };
        match self.api.api_dump_haves(i) {
            Ok((bits, len)) => bits.iter().by_vals().take(len as usize).collect(),
            Err(_) => Vec::new(),
        }
    }

    pub async fn pause(&self, hash: &str) -> Result<(), TorrentError> {
        let h = self.session.get(id(hash)?).ok_or(TorrentError::Unknown)?;
        if matches!(h.stats().state, TorrentStatsState::Paused) {
            return Ok(());
        }
        self.session.pause(&h).await.map_err(engine)
    }

    /// Start it again. One still checking its files (added paused, or restored at start) is
    /// waited for: it is Paused only once checked, and unpausing it before did nothing, so it
    /// stayed paused (seen on Linux, where the check took longer than the call).
    pub async fn resume(&self, hash: &str) -> Result<(), TorrentError> {
        let h = self.session.get(id(hash)?).ok_or(TorrentError::Unknown)?;
        if matches!(h.stats().state, TorrentStatsState::Initializing { .. }) {
            h.wait_until_initialized().await.map_err(engine)?;
        }
        if !matches!(h.stats().state, TorrentStatsState::Paused) {
            return Ok(());
        }
        self.session.unpause(&h).await.map_err(engine)
    }

    /// Change which files are downloaded.
    pub async fn choose(&self, hash: &str, files: &HashSet<usize>) -> Result<(), TorrentError> {
        let h = self.session.get(id(hash)?).ok_or(TorrentError::Unknown)?;
        self.session
            .update_only_files(&h, files)
            .await
            .map_err(engine)
    }

    /// Take it out of the session; the files stay unless `delete_files`.
    pub async fn remove(&self, hash: &str, delete_files: bool) -> Result<(), TorrentError> {
        self.session
            .delete(id(hash)?, delete_files)
            .await
            .map_err(engine)
    }

    /// Every torrent the session holds (to forget ones the app no longer lists).
    pub fn hashes(&self) -> Vec<String> {
        self.session
            .with_torrents(|all| all.map(|(_, h)| h.info_hash().as_string()).collect())
    }

    /// The whole session's download limit, bytes/s (None: unlimited): every torrent together.
    /// Setting the same rate again changes nothing (a new limiter would start a new burst), so
    /// it may be called often.
    pub fn set_download_limit(&self, bps: Option<u64>) {
        let want = rate(bps);
        if self.session.ratelimits.get_config().download_bps != want {
            self.session.ratelimits.set_download_bps(want);
        }
    }

    /// The whole session's download limit now, bytes/s (None: unlimited).
    pub fn download_limit(&self) -> Option<u64> {
        let c = self.session.ratelimits.get_config();
        c.download_bps.map(|b| u64::from(b.get()))
    }

    /// The whole session's upload limit, bytes/s (None: unlimited).
    pub fn set_upload_limit(&self, bps: Option<u64>) {
        self.session.ratelimits.set_upload_bps(rate(bps));
    }

    /// One torrent's own limits, bytes/s (None: unlimited), on top of the session's. They apply
    /// at once, running or paused, and last while the session runs (librqbit doesn't save them).
    pub fn set_torrent_limits(
        &self,
        hash: &str,
        down: Option<u64>,
        up: Option<u64>,
    ) -> Result<(), TorrentError> {
        let h = self.session.get(id(hash)?).ok_or(TorrentError::Unknown)?;
        h.set_ratelimits(librqbit::limits::LimitsConfig {
            download_bps: rate(down),
            upload_bps: rate(up),
        });
        Ok(())
    }

    /// One torrent's own limits (down, up), as `set_torrent_limits` stored them.
    pub fn torrent_limits(&self, hash: &str) -> Option<(Option<u64>, Option<u64>)> {
        let h = self.session.get(id(hash).ok()?)?;
        let c = h.ratelimits();
        let get = |v: Option<NonZeroU32>| v.map(|b| u64::from(b.get()));
        Some((get(c.download_bps), get(c.upload_bps)))
    }

    /// Where peers can reach us (None when not listening).
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.session.listen_addr()
    }

    pub async fn stop(&self) {
        self.session.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnets_are_recognised() {
        assert!(matches!(
            Source::magnet("  MAGNET:?xt=urn:btih:abc"),
            Some(Source::Magnet(_))
        ));
        assert!(Source::magnet("https://x/y.torrent").is_none());
        assert!(Source::magnet("magnet").is_none());
    }

    /// A torrent of `dir`'s files, made by librqbit.
    async fn make_torrent(dir: &Path) -> Vec<u8> {
        let spawner = librqbit::spawn_utils::BlockingSpawner::new(1);
        let made = librqbit::create_torrent(
            dir,
            librqbit::CreateTorrentOptions {
                name: Some("set"),
                trackers: Vec::new(),
                piece_length: Some(16 * 1024),
            },
            &spawner,
        )
        .await
        .unwrap();
        made.as_bytes().unwrap().to_vec()
    }

    fn offline(state: &Path) -> Config {
        Config {
            dht: false,
            offline: true,
            ..Config::new(state.to_owned())
        }
    }

    /// A .torrent with one 5-byte file at `folder/<name>`, written by hand (librqbit makes only
    /// valid ones).
    fn torrent_naming(name: &str) -> Vec<u8> {
        let mut t = b"d4:infod5:filesld6:lengthi5e4:pathl6:folder".to_vec();
        t.extend(format!("{}:{name}", name.len()).bytes());
        t.extend(b"eee4:name4:test12:piece lengthi16384e6:pieces20:");
        t.extend([0u8; 20]);
        t.extend(b"ee");
        t
    }

    /// #40: a name the system can't write safely refuses the torrent, listed or added, and
    /// nothing is written.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unsafe_names_refuse_the_torrent() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Torrents::start(offline(&tmp.path().join("state")))
            .await
            .unwrap();
        let out = tmp.path().join("out");
        let bad = if cfg!(windows) {
            ["Q:mdm-test.bin", "x.bin:hidden", "NUL.txt"].as_slice()
        } else {
            ["tab\there.bin"].as_slice()
        };
        for name in bad {
            let bytes = torrent_naming(name);
            let listed = t
                .metadata(&Source::File(bytes.clone()), Duration::from_secs(5))
                .await;
            assert!(
                matches!(listed, Err(TorrentError::UnsafeName(ref u)) if u.path == format!("folder/{name}")),
                "{name}: {listed:?}"
            );
            let added = t.add(&bytes, &out, None, Vec::new(), false).await;
            assert!(
                matches!(added, Err(TorrentError::UnsafeName(_))),
                "{name}: {added:?}"
            );
        }
        assert!(!out.exists(), "nothing was written");
        // A plain name still lists.
        let fine = t
            .metadata(
                &Source::File(torrent_naming("fine.bin")),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(fine.files.len(), 1);
        t.stop().await;
    }

    /// #40: the torrents librqbit saved go through the name check when the session starts again;
    /// one saved without its .torrent is taken out too (the app adds it again from its own copy).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saved_torrents_are_checked_at_start() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let out = tmp.path().join("out");
        let t = Torrents::start(offline(&state)).await.unwrap();
        let good = t
            .add(&torrent_naming("fine.bin"), &out, None, Vec::new(), true)
            .await
            .unwrap();
        t.stop().await;

        // Beside it, as librqbit saves them: an unsafe torrent, and one without its .torrent.
        let dir = state.join("session");
        let db = dir.join("session.json");
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&db).unwrap()).unwrap();
        let saved = doc["torrents"].as_object_mut().unwrap();
        let entry = saved.values().next().unwrap().clone();
        assert_eq!(entry["info_hash"], good.as_str(), "librqbit's format");
        // Names librqbit itself would write (inside `out`), so the check is what refuses them.
        let bad = torrent_naming(if cfg!(windows) {
            "x.bin:hidden"
        } else {
            "tab\there.bin"
        });
        let bad_hash = librqbit::torrent_from_bytes(&bad)
            .unwrap()
            .info_hash
            .as_string();
        std::fs::write(dir.join(format!("{bad_hash}.torrent")), &bad).unwrap();
        let lost = "ab".repeat(20);
        for (n, hash) in [(1000, &bad_hash), (1001, &lost)] {
            let mut e = entry.clone();
            e["info_hash"] = hash.as_str().into();
            saved.insert(n.to_string(), e);
        }
        std::fs::write(&db, serde_json::to_string(&doc).unwrap()).unwrap();

        // A magnet can't resolve offline: without the check this start would wait for ever.
        let t = tokio::time::timeout(Duration::from_secs(30), Torrents::start(offline(&state)))
            .await
            .expect("the session started")
            .unwrap();
        assert!(t.has(&good), "the safe one is kept");
        assert!(!t.has(&bad_hash), "the unsafe one is taken out");
        assert!(!t.has(&lost), "the one without its .torrent is taken out");
        assert!(!dir.join(format!("{bad_hash}.torrent")).exists());
        t.stop().await;
    }

    /// #40: a kept torrent keeps only the trackers MDM may contact, in session.json and in its
    /// .torrent (whose info, so its hash, stays the same); with a proxy, UDP ones go too.
    /// #40: a fixed port is the one listened on; with a proxy nothing listens (incoming
    /// connections would go around it). Nothing else of the network is on (no DHT, UPnP, LSD).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_port_and_the_proxy_are_honoured() {
        let tmp = tempfile::tempdir().unwrap();
        let free = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let quiet = |dir: &str| Config {
            upnp: false,
            dht: false,
            lsd: false,
            ..Config::new(tmp.path().join(dir))
        };
        let t = Torrents::start(Config {
            port: Some(free),
            ..quiet("fixed")
        })
        .await
        .unwrap();
        assert_eq!(t.listen_addr().map(|a| a.port()), Some(free));
        t.stop().await;
        let t = Torrents::start(Config {
            proxy: Some("socks5://127.0.0.1:9".into()),
            ..quiet("proxied")
        })
        .await
        .unwrap();
        assert_eq!(t.listen_addr(), None, "nothing listens behind a proxy");
        t.stop().await;
    }

    #[test]
    fn torrent_files_are_read_up_to_their_cap() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small.torrent");
        std::fs::write(&small, torrent_naming("a.bin")).unwrap();
        assert_eq!(read_file(&small).unwrap(), torrent_naming("a.bin"));
        let big = dir.path().join("big.torrent");
        std::fs::write(&big, vec![b'x'; MAX_TORRENT_FILE as usize + 1]).unwrap();
        assert_eq!(
            read_file(&big).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert!(read_file(&dir.path().join("none.torrent")).is_err());
    }

    #[test]
    fn kept_torrents_lose_the_trackers_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let plain = torrent_naming("fine.bin");
        let hash = librqbit::torrent_from_bytes(&plain)
            .unwrap()
            .info_hash
            .as_string();
        let local = "http://127.0.0.1:9/announce";
        let mut with_tracker = format!("d8:announce{}:{local}", local.len()).into_bytes();
        with_tracker.extend(&plain[1..]);
        assert_eq!(
            librqbit::torrent_from_bytes(&with_tracker)
                .unwrap()
                .info_hash
                .as_string(),
            hash
        );
        std::fs::write(dir.path().join(format!("{hash}.torrent")), &with_tracker).unwrap();
        let doc = serde_json::json!({ "torrents": { "0": {
            "info_hash": hash,
            "trackers": ["udp://t.example:1337/a", local, "https://t.example/a"],
            "output_folder": "x", "only_files": null, "is_paused": true,
        }}});
        std::fs::write(dir.path().join("session.json"), doc.to_string()).unwrap();
        prepare_saved(dir.path(), true);
        let back: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("session.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            back["torrents"]["0"]["trackers"],
            serde_json::json!(["https://t.example/a"])
        );
        let rewritten = std::fs::read(dir.path().join(format!("{hash}.torrent"))).unwrap();
        assert_eq!(
            rewritten, plain,
            "the announce key is gone, the rest byte for byte"
        );
        assert!(check_bytes(&rewritten).is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn seed_to_a_second_session_with_only_the_chosen_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("seed").join("set");
        std::fs::create_dir_all(&src).unwrap();
        let a: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let b: Vec<u8> = (0..120_000u32).map(|i| (i % 13) as u8).collect();
        std::fs::write(src.join("a.bin"), &a).unwrap();
        std::fs::write(src.join("b.bin"), &b).unwrap();
        let torrent = make_torrent(&src).await;

        // The seeder: its files are already there.
        let seeder = Torrents::start(offline(&tmp.path().join("s1")))
            .await
            .unwrap();
        let meta = seeder
            .metadata(&Source::File(torrent.clone()), Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(meta.name, "set");
        assert_eq!(meta.total(), 420_000);
        let names: Vec<String> = meta
            .files
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.ends_with("a.bin")), "{names:?}");
        let hash = seeder
            .add(&torrent, &src, None, Vec::new(), false)
            .await
            .unwrap();
        assert_eq!(hash, meta.info_hash);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while seeder.stats(&hash).unwrap().phase != Phase::Finished {
            assert!(std::time::Instant::now() < deadline, "seeder never checked");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(seeder.pieces(&hash).iter().all(|p| *p));
        let addr = seeder.listen_addr().expect("seeder listens");

        // The leecher wants only b.bin.
        let only_b = meta
            .files
            .iter()
            .position(|f| f.path.ends_with("b.bin"))
            .unwrap();
        let leecher = Torrents::start(offline(&tmp.path().join("s2")))
            .await
            .unwrap();
        let out = meta.folder_in(&tmp.path().join("got"));
        assert!(out.ends_with("set"), "several files: their own folder");
        let h2 = leecher
            .add(&torrent, &out, Some(vec![only_b]), vec![addr], false)
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let s = leecher.stats(&h2).unwrap();
            if s.phase == Phase::Finished {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no transfer: {s:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let got = std::fs::read(out.join(&meta.files[only_b].path)).unwrap();
        assert_eq!(got, b);
        assert!(
            leecher.trackers(&h2).is_empty(),
            "a torrent without trackers"
        );

        leecher.pause(&h2).await.unwrap();
        assert_eq!(leecher.stats(&h2).unwrap().phase, Phase::Paused);
        leecher.remove(&h2, false).await.unwrap();
        assert!(!leecher.has(&h2));
        assert!(
            out.join(&meta.files[only_b].path).exists(),
            "files stay unless asked"
        );
        seeder.stop().await;
        leecher.stop().await;
    }

    /// #39: two torrents added at the same moment are both in the session. (librqbit gave both
    /// the id 0 and answered the second with the first's handle: it was never added.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_torrents_added_at_once_are_both_there() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Torrents::start(offline(&tmp.path().join("state")))
            .await
            .unwrap();
        let (a, b) = (torrent_naming("a.bin"), torrent_naming("b.bin"));
        let (oa, ob) = (tmp.path().join("out-a"), tmp.path().join("out-b"));
        let (ha, hb) = tokio::join!(
            t.add(&a, &oa, None, Vec::new(), true),
            t.add(&b, &ob, None, Vec::new(), true),
        );
        let (ha, hb) = (ha.unwrap(), hb.unwrap());
        assert_ne!(ha, hb);
        assert!(t.has(&ha) && t.has(&hb), "both in the session");
        let mut all = t.hashes();
        all.sort();
        let mut want = vec![ha, hb];
        want.sort();
        assert_eq!(all, want);
        t.stop().await;
    }

    /// #39: a torrent's own limit holds it back, the session's stays as it was, and the bytes
    /// it fetched are counted (the app leaves file downloads the global cap's rest by them).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_torrents_own_limit_holds_it_back() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("seed").join("one");
        std::fs::create_dir_all(&src).unwrap();
        let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 241) as u8).collect();
        std::fs::write(src.join("data.bin"), &data).unwrap();
        let torrent = make_torrent(&src).await;

        let seeder = Torrents::start(offline(&tmp.path().join("s1")))
            .await
            .unwrap();
        let meta = seeder
            .metadata(&Source::File(torrent.clone()), Duration::from_secs(10))
            .await
            .unwrap();
        let hash = seeder
            .add(&torrent, &src, None, Vec::new(), false)
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while seeder.stats(&hash).unwrap().phase != Phase::Finished {
            assert!(std::time::Instant::now() < deadline, "seeder never checked");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let addr = seeder.listen_addr().expect("seeder listens");

        let leecher = Torrents::start(offline(&tmp.path().join("s2")))
            .await
            .unwrap();
        let out = meta.folder_in(&tmp.path().join("got"));
        let h2 = leecher
            .add(&torrent, &out, None, vec![addr], true)
            .await
            .unwrap();
        assert_eq!(leecher.torrent_limits(&h2), Some((None, None)));
        // 1 MiB at 256 KiB/s: about 3 s after the first second's burst.
        leecher
            .set_torrent_limits(&h2, Some(256 * 1024), None)
            .unwrap();
        assert_eq!(leecher.torrent_limits(&h2), Some((Some(256 * 1024), None)));
        leecher.set_torrent_limits(&h2, Some(1), None).unwrap();
        assert_eq!(
            leecher.torrent_limits(&h2),
            Some((Some(MIN_RATE), None)),
            "never under one block a second"
        );
        leecher
            .set_torrent_limits(&h2, Some(256 * 1024), None)
            .unwrap();
        assert_eq!(leecher.download_limit(), None, "the session's untouched");
        assert!(matches!(
            leecher.set_torrent_limits(&"0".repeat(40), None, None),
            Err(TorrentError::Unknown)
        ));

        let t = std::time::Instant::now();
        leecher.resume(&h2).await.unwrap();
        let deadline = t + Duration::from_secs(40);
        let fetched = loop {
            let s = leecher.stats(&h2).unwrap();
            if s.phase == Phase::Finished {
                break s.fetched;
            }
            assert!(std::time::Instant::now() < deadline, "no transfer: {s:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let secs = t.elapsed().as_secs_f64();
        assert!(secs >= 2.0, "1 MiB at 256 KiB/s took {secs:.2}s");
        assert!(fetched >= data.len() as u64, "fetched {fetched}");
        let got = std::fs::read(out.join(&meta.files[0].path)).unwrap();
        assert!(got == data, "the file arrived whole");
        seeder.stop().await;
        leecher.stop().await;
    }

    #[tokio::test]
    async fn junk_is_not_a_torrent() {
        let tmp = tempfile::tempdir().unwrap();
        let t = Torrents::start(Config {
            listen: false,
            ..offline(tmp.path())
        })
        .await
        .unwrap();
        let err = t
            .metadata(
                &Source::File(b"not a torrent".to_vec()),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, TorrentError::NotATorrent), "{err}");
    }
}

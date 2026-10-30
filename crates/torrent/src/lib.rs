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
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerOptions, Session, SessionOptions,
    SessionPersistenceConfig, TorrentStatsState,
};

#[derive(Debug, thiserror::Error)]
pub enum TorrentError {
    #[error("this is not a magnet link or a .torrent file")]
    NotATorrent,
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

/// How the session runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// Where librqbit keeps its resume data and DHT nodes (in our data folder, portable-aware).
    pub state_dir: PathBuf,
    /// Accept connections from peers (better speed; Windows may ask about the firewall once).
    pub listen: bool,
    pub dht: bool,
    /// Tests: no trackers, no DHT, local peers only.
    pub offline: bool,
}

pub struct Torrents {
    session: Arc<Session>,
    api: Api,
}

fn id(hash: &str) -> Result<TorrentIdOrHash, TorrentError> {
    TorrentIdOrHash::parse(hash).map_err(|_| TorrentError::Unknown)
}

impl Torrents {
    pub async fn start(cfg: Config) -> Result<Torrents, TorrentError> {
        std::fs::create_dir_all(&cfg.state_dir).map_err(engine)?;
        let dht = (cfg.dht && !cfg.offline).then(|| librqbit::DhtSessionConfig {
            persistence: Some(librqbit::dht::DhtPersistenceConfig {
                dump_interval: None,
                config_filename: Some(cfg.state_dir.join("dht.json")),
            }),
            ..Default::default()
        });
        let listen = cfg.listen.then(|| ListenerOptions {
            listen_addr: if cfg.offline {
                (Ipv4Addr::LOCALHOST, 0).into()
            } else {
                ListenerOptions::default().listen_addr
            },
            enable_upnp_port_forwarding: !cfg.offline,
            ipv4_only: cfg.offline,
            ..Default::default()
        });
        let opts = SessionOptions {
            dht,
            listen,
            disable_trackers: cfg.offline,
            disable_local_service_discovery: cfg.offline,
            ipv4_only: cfg.offline,
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(cfg.state_dir.join("session")),
            }),
            client_name_and_version: Some(format!(
                "MisterDownloadManager/{}",
                env!("CARGO_PKG_VERSION")
            )),
            ..Default::default()
        };
        let session = Session::new_with_opts(cfg.state_dir.join("unsorted"), opts)
            .await
            .map_err(engine)?;
        let api = Api::new(session.clone(), None);
        Ok(Torrents { session, api })
    }

    /// The file list, without downloading. A magnet asks the swarm, for at most `wait`.
    pub async fn metadata(&self, src: &Source, wait: Duration) -> Result<Meta, TorrentError> {
        let add = match src {
            Source::Magnet(m) => AddTorrent::from_url(m.clone()),
            Source::File(b) => AddTorrent::from_bytes(b.clone()),
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
        let opts = AddTorrentOptions {
            paused,
            only_files: files,
            output_folder: Some(folder.to_string_lossy().into_owned()),
            // Resuming after a restart finds our own partial files there.
            overwrite: true,
            initial_peers: (!peers.is_empty()).then_some(peers),
            ..Default::default()
        };
        let res = self
            .session
            .add_torrent(AddTorrent::from_bytes(torrent.to_vec()), Some(opts))
            .await
            .map_err(engine)?;
        let handle = res
            .into_handle()
            .ok_or_else(|| engine("the torrent was listed instead of started"))?;
        Ok(handle.info_hash().as_string())
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

    pub async fn resume(&self, hash: &str) -> Result<(), TorrentError> {
        let h = self.session.get(id(hash)?).ok_or(TorrentError::Unknown)?;
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

    /// The global download limit, bytes/s (None: unlimited).
    pub fn set_download_limit(&self, bps: Option<u64>) {
        let v = bps.and_then(|b| NonZeroU32::new(b.min(u32::MAX as u64) as u32));
        self.session.ratelimits.set_download_bps(v);
    }

    /// The whole session's upload limit, bytes/s (None: unlimited).
    pub fn set_upload_limit(&self, bps: Option<u64>) {
        let v = bps.and_then(|b| NonZeroU32::new(b.min(u32::MAX as u64) as u32));
        self.session.ratelimits.set_upload_bps(v);
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
            state_dir: state.to_owned(),
            listen: true,
            dht: false,
            offline: true,
        }
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

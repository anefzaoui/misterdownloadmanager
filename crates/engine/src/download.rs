//! One download: probe, parallel connections driven by the planner, positional writes, progress.
//!
//! The first connection is also the probe: a plain `GET` (no Range, as IDM does — D-007) that
//! keeps streaming from byte 0 while further connections split the file (notes/ENGINE.md §1).
//! Range support is learned from the first range answer: 206 confirms it; 200 means the server
//! ignores ranges, so the file goes back to the probe connection alone.
//! One HTTP/1.1 client per download, shared by its connections: one pool (keep-alive sockets
//! reused between segments, TLS sessions resumed) with at most `max_connections` requests at a
//! time, since each worker has one request in flight.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::disk;
use crate::dns;
use crate::limiter::Limiter;
use crate::naming;
use crate::planner::{Claim, Planner, WorkerId};
use crate::sparse;
use crate::state::{ResumeState, Validators, state_path};

/// Bytes buffered per connection before claiming and writing them.
const BATCH: usize = 256 * 1024;
/// Received bytes are written at least this often, so a slow connection shows progress and a
/// pause keeps what arrived.
const FLUSH_EVERY: Duration = Duration::from_secs(1);

/// Shedding a slow connection (`Options::shed_slow`): after `SHED_GRACE` on a response, a
/// connection under 1/`SHED_RATIO` of the best speed one connection reached, with more than
/// `SHED_LEFT` of work left at its own pace, reconnects. A slow TCP path (or a slow server
/// slot) rarely survives a new request, and otherwise every free connection can end up stuck
/// on one (measured: 14 s instead of ~2 s for 32 MiB when every third request was slow).
const SHED_GRACE: Duration = Duration::from_secs(2);
const SHED_RATIO: u64 = 4;
const SHED_LEFT: Duration = Duration::from_secs(2);
/// A download starts with one connection per this many bytes (at most `max_connections`).
const INITIAL_PIECE: u64 = 64 * 1024;
/// A pending connection is taken over after this many first-byte times of the probe (and never
/// sooner than PREEMPT_MIN).
const PREEMPT_TTFB: u32 = 2;
const PREEMPT_MIN: Duration = Duration::from_millis(20);

#[derive(Clone, Debug)]
pub struct Options {
    /// Max simultaneous connections. IDM's default is 8 (notes/ENGINE.md §4).
    pub max_connections: u32,
    /// Smallest half a split may leave. IDM split pieces down to ≤ 12.6 KB (lab S4, D-007).
    pub min_split: u64,
    pub connect_timeout: Duration,
    /// Max silence on a connection before it is considered dead.
    pub read_timeout: Duration,
    /// Consecutive failures after which a connection gives up (it retires if others are
    /// still working, otherwise the download fails).
    pub max_retries: u32,
    /// The first retry is immediate (as IDM, lab S5); then this delay, doubling, capped at 30 s.
    pub retry_delay: Duration,
    pub user_agent: String,
    /// How often resume state is saved (IDM: "several times per minute").
    pub save_interval: Duration,
    /// Speed limit for this download, bytes/s (IDM's limiter is per file). None = unlimited.
    pub speed_limit: Option<u64>,
    /// Before continuing a segment that already has data on disk, re-fetch up to this many of
    /// its last written bytes and compare them (IDM's resume overlap check; catches changed
    /// files on servers without validators). 0 turns it off.
    pub verify_overlap: u64,
    /// Fixed addresses for host names, like hosts-file entries (tests; a DNS option later).
    pub hosts: Vec<(String, Vec<std::net::IpAddr>)>,
    /// Options → Proxy / Socks.
    pub proxy: Proxy,
    /// A limit shared by every download using the same limiter (a global speed cap), on top
    /// of the download's own.
    pub global: Option<Arc<Limiter>>,
    /// The finished file must have this hash (else the server's announced one, if any).
    pub checksum: Option<crate::verify::Hash>,
    /// Reconnect a connection far slower than the others (see `SHED_*`). Beyond IDM's rule.
    pub shed_slow: bool,
    /// Connection caps learned per host, shared by the downloads that use the same table.
    pub host_limits: Option<Arc<crate::hosts::HostLimits>>,
    /// Resume across a changed modification time (IDM: "Ignore file modification time changes
    /// when resuming a download"): servers that stamp every answer with a new date would
    /// otherwise make each resume start over. Only an ETag then tells a changed file.
    pub ignore_mtime: bool,
    /// Give the finished file the server's date (IDM: "Set file creation date as provided by
    /// the server"): created and modified times from `Last-Modified`.
    pub server_time: bool,
    /// FTP data connections: passive (the client connects; IDM's "Use passive mode for FTP",
    /// on by default), else active (the server connects back).
    pub ftp_passive: bool,
    /// Certificates (DER) trusted besides the system's for FTPS (tests' own CA).
    pub extra_roots: Vec<Vec<u8>>,
}

/// Which proxy downloads go through (Options → Proxy / Socks).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Proxy {
    /// "No proxy/socks" (IDM's default): direct connections.
    #[default]
    None,
    /// "Use system settings": Windows' proxy (and the environment's).
    System,
    /// "Manual proxy/socks configuration": `http://host:port` (or `socks5://…`), with an
    /// optional login, for the listed schemes.
    Manual {
        url: String,
        login: Option<(String, String)>,
        http: bool,
        https: bool,
    },
    /// "Use automatic configuration script": the script decides per address (`pac`); one
    /// that doesn't run leaves it to the system's settings.
    Pac { script: Arc<str> },
}

/// Whether the engine can download `url`: http, https, ftp, ftps (implicit TLS) and ftpes
/// (explicit TLS). Magnet links are the torrent engine's.
pub fn supported(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") || crate::ftp::is_ftp(url)
}

/// IDM 6.42 sends this browser-style User-Agent (lab S1, D-007).
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 6.1; Trident/7.0; rv:11.0) like Gecko";

impl Default for Options {
    fn default() -> Self {
        Options {
            max_connections: 8,
            min_split: 12 * 1024,
            connect_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            max_retries: 5,
            retry_delay: Duration::from_secs(1),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            save_interval: Duration::from_secs(2),
            speed_limit: None,
            verify_overlap: 64,
            hosts: Vec::new(),
            proxy: Proxy::None,
            global: None,
            checksum: None,
            shed_slow: true,
            host_limits: None,
            ignore_mtime: false,
            server_time: false,
            ftp_passive: true,
            extra_roots: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Destination {
    /// Save into this folder under the name the server/URL suggests (numbered if taken).
    Dir(PathBuf),
    /// Save exactly here.
    File(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Request {
    pub url: Url,
    /// Extra headers sent on every request (Referer, Cookie, Authorization …).
    pub headers: HeaderMap,
    pub dest: Destination,
}

/// Cloneable (the network and disk errors it wraps are shared), so a snapshot can carry the
/// failure itself for an interface to word in its own language (`Snapshot::failure`).
#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", describe_http(.0))]
    Http(Arc<reqwest::Error>),
    #[error("server answered {0}")]
    Status(StatusCode),
    /// 429 / 503: the server is busy; retry after its `Retry-After` when it gives one.
    #[error("server busy ({0})")]
    Throttled(StatusCode, Option<Duration>),
    #[error("server sent an unexpected range: {0}")]
    BadRange(String),
    #[error("connection closed before the segment was complete")]
    ShortBody,
    #[error("the file changed on the server")]
    FileChanged,
    /// The server ignores ranges and the probe connection can't take the file back alone.
    #[error("the server does not support ranges")]
    NoRanges,
    #[error("cannot resume: {0}")]
    BadState(String),
    #[error("disk: {0}")]
    Io(Arc<io::Error>),
    /// Checked before starting: the file and a safety margin don't fit on the drive.
    #[error("not enough disk space: {} needed, {} free", disk::human(*needed), disk::human(*free))]
    NoSpace { needed: u64, free: u64 },
    /// The drive filled up while downloading (another program took the space).
    #[error("the disk is full")]
    DiskFull,
    #[error("{} is too big for this {file_system} drive (files up to 4 GB)", disk::human(*size))]
    TooBigForDrive { size: u64, file_system: String },
    #[error("cancelled")]
    Cancelled,
    /// Another download in this process is writing to the same file.
    #[error("already being downloaded: {0}")]
    InUse(PathBuf),
    /// The save folder can't be reached or created (a removed drive, an offline share).
    #[error("the folder {} is not available (drive removed or network share offline?)", .0.display())]
    DestinationUnavailable(PathBuf),
    /// The drive or network share went away while downloading.
    #[error("the drive or network share is no longer available")]
    DriveGone,
    /// Longer than Windows programs can open (MAX_PATH).
    #[error("the path is too long for Windows: {}", .0.display())]
    PathTooLong(PathBuf),
    /// A web page (a sign-in wall, an expired link's notice) came instead of the file.
    #[error("the server sent a web page instead of the file (sign-in needed or the link expired?)")]
    GotWebPage,
    /// A full answer to a range request for the same, unchanged file: the server (or one CDN
    /// edge) didn't serve the range this time. Retried; the file did not change.
    #[error("the server did not send the requested part")]
    RangesUnavailable,
    /// The redirected address stopped working and the original address no longer leads to this
    /// file: the user has to give a fresh address (Refresh download address).
    #[error("the download link has expired (refresh the download address)")]
    LinkExpired,
    /// This connection was far slower than the others: it reconnects for the rest of its
    /// segment (not a failure).
    #[error("much slower than the other connections; reconnecting")]
    SlowConnection,
    /// The finished file doesn't have the expected hash; it stays as the part file.
    #[error("the file's {0} checksum doesn't match (the data is kept; redownload it)")]
    ChecksumMismatch(&'static str),
    /// An FTP server's refusal: its reply code and text.
    #[error("the FTP server answered {code} {text}")]
    FtpReply { code: u32, text: String },
    /// A network failure outside HTTP (FTP's sessions): `cause` as the system words it.
    #[error("{}", describe_net(*.kind, .cause))]
    Net { kind: NetKind, cause: String },
}

/// What failed on the network (`Error::Net`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetKind {
    Connect,
    /// The server went quiet.
    Timeout,
    /// The connection broke while receiving.
    Lost,
    /// The secure connection failed (an untrusted certificate, a handshake the server refused).
    Tls,
}

impl Error {
    /// The address no longer serves this file (or refuses us): retrying won't help.
    fn link_dead(&self) -> bool {
        matches!(self, Error::Status(s) if matches!(s.as_u16(), 401 | 403 | 404 | 410 | 416))
            || matches!(self, Error::GotWebPage | Error::LinkExpired)
            // Login refused; no such file (or no permission); a name the server won't take.
            || matches!(self, Error::FtpReply { code: 530 | 532 | 550 | 553, .. })
    }

    /// Errors that stop the whole download instead of being retried by a connection.
    fn is_fatal(&self) -> bool {
        matches!(
            self,
            Error::FileChanged
                | Error::NoRanges
                | Error::BadState(_)
                | Error::Io(_)
                | Error::NoSpace { .. }
                | Error::DiskFull
                | Error::TooBigForDrive { .. }
                | Error::DestinationUnavailable(_)
                | Error::DriveGone
                | Error::PathTooLong(_)
                | Error::ChecksumMismatch(_)
                | Error::Cancelled
                | Error::InUse(_)
        )
    }

    /// A failure of the first request that a later try may not have: a busy server (429, 503),
    /// a server error (500, 502, 504), a network that failed to connect or went quiet.
    fn transient(&self) -> bool {
        match self {
            Error::Throttled(..) => true,
            Error::Status(s) => matches!(s.as_u16(), 500 | 502 | 504),
            Error::Http(h) => h.is_connect() || h.is_timeout() || h.is_request() || h.is_body(),
            Error::ShortBody => true,
            // Too many sessions; no data connection; transfer aborted; file busy; local error.
            Error::FtpReply { code, .. } => matches!(code, 421 | 425 | 426 | 450 | 451),
            Error::Net { kind, .. } => *kind != NetKind::Tls,
            _ => false,
        }
    }

    /// A server turning this connection away while others work: its connection limit. FTP
    /// servers say it at login, with 421 or (vsftpd, ProFTPD) 530.
    fn refused(&self) -> bool {
        match self {
            Error::Throttled(..) => true,
            Error::Status(s) => s.as_u16() == 403,
            Error::Http(h) => h.is_connect(),
            Error::FtpReply { code, .. } => matches!(code, 421 | 530),
            Error::Net { kind, .. } => *kind == NetKind::Connect,
            _ => false,
        }
    }
}

/// How long to wait before try number `failures + 1` after `e`: the first retry at once (IDM
/// reconnects within ~12 ms, lab S5), then `retry_delay` doubling up to 30 s; a busy server's
/// Retry-After wins (capped at 5 minutes). One policy for the first request and every connection.
fn retry_wait(opts: &Options, failures: u32, e: &Error) -> Option<Duration> {
    match e {
        Error::Throttled(_, Some(after)) => Some((*after).min(Duration::from_secs(300))),
        _ if failures > 1 => {
            Some((opts.retry_delay * 2u32.pow((failures - 2).min(10))).min(Duration::from_secs(30)))
        }
        _ => None,
    }
}

/// A network error in words a user can act on, with its deepest cause ("connection refused",
/// "dns error: …") but not the address, which the dialog already shows.
fn describe_http(e: &reqwest::Error) -> String {
    let what = if e.is_timeout() {
        "the server did not answer in time"
    } else if e.is_connect() {
        "cannot connect to the server"
    } else if e.is_body() || e.is_decode() {
        "the connection was lost while receiving data"
    } else if e.is_redirect() {
        "too many redirects"
    } else {
        "network error"
    };
    let mut cause = None;
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        cause = Some(s.to_string());
        source = s.source();
    }
    match cause.map(|c| strip_os_error_code(&c)) {
        Some(c) if !c.is_empty() && !c.contains("://") => format!("{what} ({c})"),
        _ => what.to_owned(),
    }
}

/// A network failure outside HTTP in words a user can act on, with its cause.
fn describe_net(kind: NetKind, cause: &str) -> String {
    let what = match kind {
        NetKind::Connect => "cannot connect to the server",
        NetKind::Timeout => "the server did not answer in time",
        NetKind::Lost => "the connection was lost while receiving data",
        NetKind::Tls => "the secure connection failed",
    };
    if cause.is_empty() {
        what.to_owned()
    } else {
        format!("{what} ({cause})")
    }
}

/// "… refused it. (os error 10061)" → "… refused it".
pub(crate) fn strip_os_error_code(s: &str) -> String {
    let s = match s.rfind(" (os error ") {
        Some(i) if s.ends_with(')') => &s[..i],
        _ => s,
    };
    s.trim().trim_end_matches('.').to_owned()
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        // NOT_READY, BAD_NETPATH, DEV_NOT_EXIST, NETNAME_DELETED, BAD_NET_NAME,
        // DEVICE_NOT_CONNECTED: the drive or share is gone, not a disk fault.
        #[cfg(windows)]
        if matches!(e.raw_os_error(), Some(21 | 53 | 55 | 64 | 67 | 1167)) {
            return Error::DriveGone;
        }
        // ENXIO, ENODEV (both systems); ENOMEDIUM (Linux): the device is gone.
        #[cfg(unix)]
        if matches!(e.raw_os_error(), Some(6 | 19))
            || (cfg!(target_os = "linux") && e.raw_os_error() == Some(123))
        {
            return Error::DriveGone;
        }
        match e.kind() {
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Error::DiskFull,
            _ => Error::Io(Arc::new(e)),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(Arc::new(e))
    }
}

impl From<disk::Refusal> for Error {
    fn from(r: disk::Refusal) -> Self {
        match r {
            disk::Refusal::NoSpace { needed, free } => Error::NoSpace { needed, free },
            disk::Refusal::TooBigForFileSystem { size, file_system } => {
                Error::TooBigForDrive { size, file_system }
            }
        }
    }
}

/// The request headers to send to `to` when they were given for `from`: credentials
/// (cookies, authorization) never follow a redirect to another host or from https down to
/// http — as reqwest does while it follows the redirect itself.
fn scoped_headers(headers: &HeaderMap, from: &Url, to: &Url) -> HeaderMap {
    let mut h = headers.clone();
    let same_host = from.host_str() == to.host_str()
        && from.port_or_known_default() == to.port_or_known_default();
    let downgrade = from.scheme() == "https" && to.scheme() != "https";
    if !same_host || downgrade {
        for name in [
            header::COOKIE,
            header::AUTHORIZATION,
            header::PROXY_AUTHORIZATION,
        ] {
            h.remove(name);
        }
    }
    h
}

/// What the server's range answers have shown so far.
const RANGES_UNKNOWN: u8 = 0;
const RANGES_YES: u8 = 1;
const RANGES_NO: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Probing,
    Downloading,
    /// Checking the finished file against its checksum.
    Verifying,
    Completing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub enum ConnStatus {
    Connecting,
    Receiving,
    /// Waiting before a retry, with the last error.
    Retrying(Error),
    Finished,
    Failed(Error),
    /// Closed for good: `host` allows `cap` connections and the others fill them (learned).
    Retired {
        host: String,
        cap: u32,
    },
}

#[derive(Clone, Debug)]
pub struct ConnView {
    pub id: WorkerId,
    pub downloaded: u64,
    pub status: ConnStatus,
    /// Bytes/s over the current response (0 while not receiving).
    pub speed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentView {
    pub start: u64,
    pub cursor: u64,
    pub end: u64,
}

/// A point-in-time view for progress displays.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub state: State,
    pub url: Url,
    pub file: Option<PathBuf>,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub resumable: Option<bool>,
    pub segments: Vec<SegmentView>,
    pub connections: Vec<ConnView>,
    /// Why the download failed (state `Failed`), in English (the command line, logs).
    pub error: Option<String>,
    /// The same failure as data, for an interface that words it in its own language.
    pub failure: Option<Error>,
    /// What happened, oldest first (the last `EVENTS`): the download's event log. Never
    /// cookies or credentials.
    pub events: Vec<Event>,
}

/// One entry of a download's event log; `seq` counts up within one `Download`.
#[derive(Clone, Debug)]
pub struct Event {
    pub seq: u64,
    pub at: std::time::SystemTime,
    pub text: String,
}

/// Events kept per download.
pub const EVENTS: usize = 50;

/// What a quick look at a URL tells before downloading (for "Download File Info").
#[derive(Clone, Debug)]
pub struct ProbeInfo {
    pub final_url: Url,
    pub size: Option<u64>,
    /// Suggested local name (Content-Disposition, else URL).
    pub file_name: String,
}

/// One plain `GET` to learn the final URL, size and name; the body is not read.
pub async fn probe(url: Url, headers: HeaderMap, opts: &Options) -> Result<ProbeInfo, Error> {
    if crate::ftp::is_ftp(&url) {
        let site = crate::ftp::Site::new(&url, &headers, opts)?;
        let facts = site.check().await?;
        return Ok(ProbeInfo {
            file_name: naming::file_name(None, &url),
            final_url: url,
            size: facts.size,
        });
    }
    let resp = get(
        &client(opts, &url)?,
        url,
        headers,
        &CancellationToken::new(),
    )
    .await?;
    let final_url = resp.url().clone();
    let size = match resp.status() {
        StatusCode::PARTIAL_CONTENT => parse_content_range(resp.headers())?.2,
        _ => resp.content_length(),
    };
    let cd = resp
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok());
    let file_name = naming::file_name(cd, &final_url);
    Ok(ProbeInfo {
        final_url,
        size,
        file_name,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub path: PathBuf,
    pub size: u64,
}

/// A running download.
pub struct Download {
    shared: Arc<Shared>,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<Result<Outcome, Error>>,
}

impl Download {
    /// Start downloading in the background. Must be called inside a Tokio runtime.
    pub fn start(req: Request, opts: Options) -> Download {
        Download::spawn(req.url.clone(), move |shared, cancel| {
            let sh = shared.clone();
            let body = async move {
                if crate::ftp::is_ftp(&req.url) {
                    run_fresh_ftp(req, opts, sh, cancel).await
                } else {
                    run_fresh(req, opts, sh, cancel).await
                }
            };
            run(body, shared)
        })
    }

    /// Continue a paused, cancelled or crashed download from its state file
    /// (`state::state_path(dest)`). `headers` are sent on every request (cookies etc. are not
    /// stored in the state file).
    pub fn resume(
        state_file: PathBuf,
        headers: HeaderMap,
        opts: Options,
    ) -> Result<Download, Error> {
        // A removed drive or offline share: say so (the data is still there for later).
        if let Some(dir) = state_file.parent().filter(|d| !d.as_os_str().is_empty())
            && !dir.is_dir()
        {
            return Err(Error::DestinationUnavailable(dir.to_owned()));
        }
        let saved = ResumeState::load(&state_file).map_err(Error::BadState)?;
        Ok(Download::spawn(saved.url.clone(), move |shared, cancel| {
            run(
                run_resume(saved, state_file, headers, opts, shared.clone(), cancel),
                shared,
            )
        }))
    }

    fn spawn<F, Fut>(url: Url, f: F) -> Download
    where
        F: FnOnce(Arc<Shared>, CancellationToken) -> Fut,
        Fut: Future<Output = Result<Outcome, Error>> + Send + 'static,
    {
        let cancel = CancellationToken::new();
        let shared = Arc::new(Shared {
            view: Mutex::new(Snapshot {
                state: State::Probing,
                url,
                file: None,
                size: None,
                downloaded: 0,
                resumable: None,
                segments: Vec::new(),
                connections: Vec::new(),
                error: None,
                failure: None,
                events: Vec::new(),
            }),
            planner: Mutex::new(None),
            limiter: Limiter::new(None),
            best_speed: Default::default(),
        });
        let task = tokio::spawn(f(shared.clone(), cancel.clone()));
        Download {
            shared,
            cancel,
            task,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot()
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Change this download's speed limit (bytes/s, None = unlimited) while it runs.
    pub fn set_speed_limit(&self, rate: Option<u64>) {
        self.shared.limiter.set(rate);
    }

    pub fn speed_limit(&self) -> Option<u64> {
        self.shared.limiter.get()
    }

    pub async fn wait(self) -> Result<Outcome, Error> {
        match self.task.await {
            Ok(r) => r,
            Err(e) if e.is_cancelled() => Err(Error::Cancelled),
            Err(e) => std::panic::resume_unwind(e.into_panic()),
        }
    }
}

struct Shared {
    view: Mutex<Snapshot>,
    planner: Mutex<Option<Planner>>,
    limiter: Limiter,
    /// The best speed one connection of this download sustained (≥ 1 s), bytes/s: what a good
    /// connection to this server does, the yardstick for shedding.
    best_speed: std::sync::atomic::AtomicU64,
}

impl Shared {
    fn snapshot(&self) -> Snapshot {
        let mut s = self.view.lock().unwrap().clone();
        if let Some(p) = self.planner.lock().unwrap().as_ref() {
            s.downloaded = p.downloaded();
            s.segments = p
                .segments()
                .iter()
                .map(|g| SegmentView {
                    start: g.start,
                    cursor: g.cursor,
                    end: g.end,
                })
                .collect();
        }
        s
    }

    fn set_state(&self, state: State) {
        self.view.lock().unwrap().state = state;
    }

    /// Add to the event log.
    fn event(&self, text: impl Into<String>) {
        let mut v = self.view.lock().unwrap();
        let seq = v.events.last().map_or(1, |e| e.seq + 1);
        if v.events.len() >= EVENTS {
            v.events.remove(0);
        }
        v.events.push(Event {
            seq,
            at: std::time::SystemTime::now(),
            text: text.into(),
        });
    }

    fn conn(&self, id: WorkerId, f: impl FnOnce(&mut ConnView)) {
        let mut v = self.view.lock().unwrap();
        let i = match v.connections.iter().position(|c| c.id == id) {
            Some(i) => i,
            None => {
                v.connections.push(ConnView {
                    id,
                    downloaded: 0,
                    status: ConnStatus::Connecting,
                    speed: 0,
                });
                v.connections.len() - 1
            }
        };
        f(&mut v.connections[i]);
    }

    fn with_planner<T>(&self, f: impl FnOnce(&mut Planner) -> T) -> T {
        f(self
            .planner
            .lock()
            .unwrap()
            .as_mut()
            .expect("planner exists while workers run"))
    }
}

/// The address ranges are fetched from, with the headers for it. `generation` counts switches.
#[derive(Clone)]
struct Target {
    url: Url,
    headers: HeaderMap,
    generation: u64,
}

/// After asking the original address didn't help, how long until it may be asked again.
const REFRESH_EVERY: Duration = Duration::from_secs(120);

/// What every connection needs to fetch a segment.
struct Ctx {
    /// The download's client: every connection's requests go through its pool.
    client: Client,
    /// The file's version; each range request's `If-Range` is chosen from them.
    validators: Mutex<Validators>,
    /// Where ranges come from now (after redirects); switched when it expires.
    target: Mutex<Target>,
    /// The address the user gave and its headers, when it redirected: asked again when the
    /// target expires.
    origin: Option<(Url, HeaderMap)>,
    /// When asking the origin last failed to help (one refresh at a time; after a failure,
    /// none for `REFRESH_EVERY`).
    refreshed: tokio::sync::Mutex<Option<std::time::Instant>>,
    size: u64,
    file: Arc<File>,
    opts: Options,
    shared: Arc<Shared>,
    cancel: CancellationToken,
    /// `RANGES_*`: learned from the first range answer.
    ranges: AtomicU8,
    /// The file itself is a web page; otherwise an HTML answer is a sign-in wall or an error.
    web_page_ok: bool,
    /// An FTP download's server and its idle sessions (else HTTP through `client`).
    ftp: Option<Arc<crate::ftp::Site>>,
}

/// An answer's body coming in: an HTTP response, or an FTP transfer.
enum Body {
    Http(Response),
    Ftp(crate::ftp::Transfer),
}

impl Body {
    /// The next bytes; None at the end.
    async fn chunk(&mut self) -> Result<Option<bytes::Bytes>, Error> {
        match self {
            Body::Http(r) => Ok(r.chunk().await?),
            Body::Ftp(t) => t.chunk().await,
        }
    }

    /// The bytes it will carry, if known.
    fn len(&self) -> Option<u64> {
        match self {
            Body::Http(r) => r.content_length(),
            Body::Ftp(t) => t.expected(),
        }
    }

    /// Done with it, at its end or before: its connection may serve the next request.
    async fn done(self, at_end: bool) {
        match self {
            // Read the end of the body so the connection goes back to the pool.
            Body::Http(mut r) => {
                if !at_end {
                    let _ = r.chunk().await;
                }
            }
            Body::Ftp(t) => t.done(at_end).await,
        }
    }
}

impl Ctx {
    fn target(&self) -> Target {
        self.target.lock().unwrap().clone()
    }

    /// After `generation`'s address answered as a dead link: ask the original address for
    /// today's target (once for all connections, at most every `REFRESH_EVERY`) and switch to
    /// it if it is the same file. True when there is a newer target to retry with.
    async fn refresh_target(&self, generation: u64) -> bool {
        let Some((origin, headers)) = &self.origin else {
            return false;
        };
        let mut failed_at = self.refreshed.lock().await;
        if self.target.lock().unwrap().generation != generation {
            return true; // another connection already switched
        }
        // Signed links may live minutes while the download takes hours: refresh as often as
        // that helps, but after an unhelpful refresh wait before asking again.
        if failed_at.is_some_and(|t| t.elapsed() < REFRESH_EVERY) {
            return false;
        }
        let Some(url) = refresh_final_url(&self.client, origin, headers, self.size, &self.cancel)
            .await
            .filter(|u| *u != self.target.lock().unwrap().url)
        else {
            *failed_at = Some(std::time::Instant::now());
            return false;
        };
        *failed_at = None;
        self.shared.event(format!(
            "The address stopped working; the original address now leads to {}",
            url.host_str().unwrap_or("?")
        ));
        let mut t = self.target.lock().unwrap();
        t.headers = scoped_headers(headers, origin, &url);
        t.url = url;
        t.generation += 1;
        true
    }

    fn learn_ranges(&self, yes: bool) {
        let v = if yes { RANGES_YES } else { RANGES_NO };
        if self
            .ranges
            .compare_exchange(RANGES_UNKNOWN, v, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.shared.view.lock().unwrap().resumable = Some(yes);
        }
    }
}

/// How long an idle keep-alive socket is kept: shorter than most servers' keep-alive, so a
/// paused download doesn't come back to sockets the server already closed.
const POOL_IDLE: Duration = Duration::from_secs(15);

/// The client for a download of `url`.
fn client(opts: &Options, url: &Url) -> Result<Client, Error> {
    let port = url.port_or_known_default().unwrap_or(443);
    let builder = Client::builder();
    let builder = match &opts.proxy {
        Proxy::None => builder.no_proxy(),
        // reqwest reads the system's proxy by default.
        Proxy::System => builder,
        Proxy::Manual {
            url: proxy,
            login,
            http,
            https,
        } => {
            let with_login = |p: reqwest::Proxy| match login {
                Some((user, pass)) => p.basic_auth(user, pass),
                None => p,
            };
            let mut b = builder.no_proxy();
            if *http {
                b = b.proxy(with_login(reqwest::Proxy::http(proxy.as_str())?));
            }
            if *https {
                b = b.proxy(with_login(reqwest::Proxy::https(proxy.as_str())?));
            }
            b
        }
        // The script's first route MDM can use: a proxy, or straight to the server.
        Proxy::Pac { script } => match crate::pac::routes(script, url) {
            Ok(routes) => {
                let first = routes.into_iter().find_map(|r| match r {
                    crate::pac::Route::Direct => Some(None),
                    crate::pac::Route::Proxy(p) => reqwest::Proxy::all(&p).ok().map(Some),
                });
                match first {
                    Some(Some(p)) => builder.no_proxy().proxy(p),
                    Some(None) => builder.no_proxy(),
                    None => builder,
                }
            }
            Err(_) => builder,
        },
    };
    Ok(builder
        .dns_resolver(Arc::new(dns::Resolver::new(&opts.hosts, port)))
        .http1_only()
        .user_agent(&opts.user_agent)
        .connect_timeout(opts.connect_timeout)
        .read_timeout(opts.read_timeout)
        .pool_max_idle_per_host(opts.max_connections.max(1) as usize)
        .pool_idle_timeout(POOL_IDLE)
        .tcp_nodelay(true)
        .build()?)
}

/// Runs a download body and records its final state.
async fn run(
    body: impl Future<Output = Result<Outcome, Error>>,
    shared: Arc<Shared>,
) -> Result<Outcome, Error> {
    let result = body.await;
    shared.event(match &result {
        Ok(o) => format!("Complete: {}", o.path.display()),
        Err(Error::Cancelled) => "Stopped".to_owned(),
        Err(e) => format!("Failed: {e}"),
    });
    let mut v = shared.view.lock().unwrap();
    v.state = match &result {
        Ok(_) => State::Done,
        Err(Error::Cancelled) => State::Cancelled,
        Err(e) => {
            v.error = Some(e.to_string());
            v.failure = Some(e.clone());
            State::Failed
        }
    };
    drop(v);
    result
}

async fn run_fresh(
    req: Request,
    opts: Options,
    shared: Arc<Shared>,
    cancel: CancellationToken,
) -> Result<Outcome, Error> {
    shared.limiter.set(opts.speed_limit);
    let (shared, cancel) = (&shared, &cancel);
    // Probe on connection 0: a plain GET whose body becomes the segment starting at 0.
    let client0 = client(&opts, &req.url)?;
    shared.conn(0, |_| {});
    let (resp, ttfb) = first_get(&client0, &req.url, &req.headers, &opts, shared, cancel).await?;
    let preempt_after = (ttfb * PREEMPT_TTFB).max(PREEMPT_MIN);
    let size = match resp.status() {
        StatusCode::OK => resp.content_length(),
        // Some servers answer partially even without Range; accept it if it starts at 0.
        StatusCode::PARTIAL_CONTENT => match parse_content_range(resp.headers())? {
            (0, _, total) => total,
            (start, ..) => return Err(Error::BadRange(format!("probe started at {start}"))),
        },
        s => return Err(Error::Status(s)),
    };
    let ranges = if resp.status() == StatusCode::PARTIAL_CONTENT {
        RANGES_YES
    } else {
        RANGES_UNKNOWN
    };
    let final_url = resp.url().clone();
    shared.event(format!(
        "Connected to {}{}: {}, size {}",
        final_url.host_str().unwrap_or("?"),
        if final_url != req.url {
            " (redirected)"
        } else {
            ""
        },
        resp.status(),
        size.map_or("unknown".to_owned(), disk::human),
    ));
    // Credentials stay with the host they were given for (see `scoped_headers`).
    let headers = scoped_headers(&req.headers, &req.url, &final_url);
    let h = resp.headers();
    // The expected hash: the user's, else what a full (200) answer announces for its content.
    let checksum = opts.checksum.clone().or_else(|| {
        (resp.status() == StatusCode::OK)
            .then(|| crate::verify::Hash::from_headers(h))
            .flatten()
    });
    let validators = Validators::from_response(
        header_str(h, header::ETAG),
        header_str(h, header::LAST_MODIFIED),
        header_str(h, header::DATE),
        final_url.host_str(),
    );
    // The server's date for the finished file (Options::server_time): its Last-Modified as
    // sent, whether or not it is strong enough to resume by.
    let stamp = opts
        .server_time
        .then(|| header_str(h, header::LAST_MODIFIED))
        .flatten()
        .and_then(|lm| httpdate::parse_http_date(lm).ok());

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let (dest, _reservation, file) = open_destination(&req.dest, size, || {
        let cd = resp
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok());
        naming::file_name(cd, &final_url)
    })
    .await?;
    downloading(shared, &dest, size, ranges);
    let file = Arc::new(file);

    let size = if let Some(size) = size {
        let mut planner = Planner::new(size, opts.min_split);
        planner.set_preempt_after(preempt_after);
        *shared.planner.lock().unwrap() = Some(planner);
        let mut saved = ResumeState::new(
            req.url.clone(),
            final_url.clone(),
            dest.clone(),
            size,
            validators.clone(),
        );
        saved.content_type = content_type.clone();
        saved.checksum = checksum.clone();
        let ctx = Arc::new(Ctx {
            client: client0.clone(),
            validators: Mutex::new(validators),
            target: Mutex::new(Target {
                url: final_url.clone(),
                headers: headers.clone(),
                generation: 0,
            }),
            origin: (req.url != final_url).then(|| (req.url.clone(), req.headers.clone())),
            refreshed: Default::default(),
            size,
            file: file.clone(),
            opts: opts.clone(),
            shared: shared.clone(),
            // A child token: the supervisor stops its own connections without cancelling the
            // download (which may still fall back to one connection).
            cancel: cancel.child_token(),
            ranges: AtomicU8::new(ranges),
            web_page_ok: expects_web_page(&dest, content_type.as_deref()),
            ftp: None,
        });
        match segmented(ctx, saved, Some(Body::Http(resp))).await {
            Ok(()) => size,
            // The server ignores ranges and the probe's data can't be kept: start over on
            // one connection.
            Err(Error::NoRanges) => {
                *shared.planner.lock().unwrap() = None;
                shared.view.lock().unwrap().resumable = Some(false);
                let h = scoped_headers(&req.headers, &req.url, &final_url);
                let resp = get(&client0, final_url, h, cancel).await?;
                let body = Body::Http(resp);
                single_stream(body, &file, shared, opts.global.as_deref(), cancel).await?
            }
            Err(e) => return Err(e),
        }
    } else {
        let body = Body::Http(resp);
        single_stream(body, &file, shared, opts.global.as_deref(), cancel).await?
    };
    verify_part(shared, &dest, checksum.as_ref()).await?;
    finish(shared, file, &dest, size, stamp)
}

/// Prepare the destination of a new download of `size` bytes (if known), named `name()` in a
/// chosen folder: the folder exists, the file fits on the drive, the name is reserved in this
/// process, and the part file is created (exclusively, for a chosen folder: another program
/// may have taken the name meanwhile) at its full size.
async fn open_destination(
    dest: &Destination,
    size: Option<u64>,
    name: impl FnOnce() -> String,
) -> Result<(PathBuf, naming::Reservation, File), Error> {
    // The folder must exist: we create it, but a removed drive or offline share can't be.
    let folder = match dest {
        Destination::File(p) => p.parent().unwrap_or(Path::new("")).to_owned(),
        Destination::Dir(dir) => dir.clone(),
    };
    std::fs::create_dir_all(&folder).map_err(|_| Error::DestinationUnavailable(folder.clone()))?;
    if let Destination::File(p) = dest
        && !naming::path_fits(p)
    {
        return Err(Error::PathTooLong(p.clone()));
    }
    // Will it fit? Refuse now rather than fail half-way (a drive we can't identify is skipped).
    if let Some(size) = size {
        // Off the async threads, and bounded: a stale network drive can hang the volume probe.
        let probe = tokio::task::spawn_blocking(move || disk::volume_of(&folder));
        if let Ok(Ok(Some(volume))) = tokio::time::timeout(disk::PROBE_TIMEOUT, probe).await {
            disk::check(&volume, size)?;
        }
    }
    let (dest, reservation, file) = match dest {
        Destination::File(p) => {
            let r = naming::reserve(p).ok_or_else(|| Error::InUse(p.clone()))?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(naming::part_path(p))?;
            (p.clone(), r, file)
        }
        Destination::Dir(dir) => {
            let name = name();
            // Shortened to fit Windows' path limit; a folder too deep for any name is an error.
            let name =
                naming::fit_name(dir, &name).ok_or_else(|| Error::PathTooLong(dir.join(&name)))?;
            let mut attempt = 0;
            loop {
                let dest = naming::unique_path(dir, &name);
                if let Some(r) = naming::reserve(&dest) {
                    // Read too: reconnecting connections re-read their last bytes (overlap check).
                    match std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(naming::part_path(&dest))
                    {
                        Ok(file) => break (dest, r, file),
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                attempt += 1;
            }
        }
    };
    if let Some(size) = size {
        // Sparse first: writes far into a merely extended file make NTFS zero-fill up to them.
        if size >= sparse::MIN_SIZE {
            sparse::set(&file, true);
        }
        file.set_len(size)?;
    }
    Ok((dest, reservation, file))
}

/// The download has its file and is receiving it.
fn downloading(shared: &Shared, dest: &Path, size: Option<u64>, ranges: u8) {
    let mut v = shared.view.lock().unwrap();
    v.state = State::Downloading;
    v.file = Some(dest.to_owned());
    v.size = size;
    v.resumable = (ranges == RANGES_YES)
        .then_some(true)
        .or(size.is_none().then_some(false));
}

/// A new FTP download: as `run_fresh`, with the server's `SIZE` and `MDTM` for the answer's
/// headers and `REST` for ranges.
async fn run_fresh_ftp(
    req: Request,
    opts: Options,
    shared: Arc<Shared>,
    cancel: CancellationToken,
) -> Result<Outcome, Error> {
    shared.limiter.set(opts.speed_limit);
    let (shared, cancel) = (&shared, &cancel);
    let site = Arc::new(crate::ftp::Site::new(&req.url, &req.headers, &opts)?);
    // Connection 0 asks about the file and receives it from byte 0 (the probe).
    shared.conn(0, |_| {});
    let ((facts, first), ttfb) = first_try(&opts, shared, cancel, || site.start()).await?;
    let preempt_after = (ttfb * PREEMPT_TTFB).max(PREEMPT_MIN);
    let size = facts.size;
    shared.event(format!(
        "Connected to {} ({}), size {}",
        req.url.host_str().unwrap_or("?"),
        match req.url.scheme() {
            "ftp" => "FTP",
            _ => "FTPS",
        },
        size.map_or("unknown".to_owned(), disk::human),
    ));
    let ranges = if facts.rest {
        RANGES_YES
    } else {
        RANGES_UNKNOWN
    };
    // The modification time stands in for Last-Modified: resume checks it.
    let validators = Validators {
        last_modified: facts.modified.map(httpdate::fmt_http_date),
        ..Validators::default()
    };
    let stamp = opts.server_time.then_some(facts.modified).flatten();
    let (dest, _reservation, file) =
        open_destination(&req.dest, size, || naming::file_name(None, &req.url)).await?;
    downloading(shared, &dest, size, ranges);
    let file = Arc::new(file);
    let first = Body::Ftp(first);
    let size = if let Some(size) = size {
        let mut planner = Planner::new(size, opts.min_split);
        planner.set_preempt_after(preempt_after);
        *shared.planner.lock().unwrap() = Some(planner);
        // No password on disk: whoever resumes gives the login again.
        let url = crate::ftp::without_password(&req.url);
        let mut saved = ResumeState::new(
            url.clone(),
            url.clone(),
            dest.clone(),
            size,
            validators.clone(),
        );
        saved.checksum = opts.checksum.clone();
        let ctx = Arc::new(Ctx {
            client: client(&opts, &req.url)?,
            validators: Mutex::new(validators),
            target: Mutex::new(Target {
                url,
                headers: HeaderMap::new(),
                generation: 0,
            }),
            origin: None,
            refreshed: Default::default(),
            size,
            file: file.clone(),
            opts: opts.clone(),
            shared: shared.clone(),
            cancel: cancel.child_token(),
            ranges: AtomicU8::new(ranges),
            web_page_ok: true,
            ftp: Some(site.clone()),
        });
        match segmented(ctx, saved, Some(first)).await {
            Ok(()) => size,
            // REST refused: the probe's data can't be kept; the whole file on one connection.
            Err(Error::NoRanges) => {
                *shared.planner.lock().unwrap() = None;
                shared.view.lock().unwrap().resumable = Some(false);
                let t = cancellable(cancel, site.open_at(0, Some(size))).await?;
                single_stream(Body::Ftp(t), &file, shared, opts.global.as_deref(), cancel).await?
            }
            Err(e) => return Err(e),
        }
    } else {
        single_stream(first, &file, shared, opts.global.as_deref(), cancel).await?
    };
    verify_part(shared, &dest, opts.checksum.as_ref()).await?;
    finish(shared, file, &dest, size, stamp)
}

async fn run_resume(
    saved: ResumeState,
    state_file: PathBuf,
    mut headers: HeaderMap,
    opts: Options,
    shared: Arc<Shared>,
    cancel: CancellationToken,
) -> Result<Outcome, Error> {
    shared.limiter.set(opts.speed_limit);
    let bad = |what: &str| Error::BadState(format!("{}: {what}", state_file.display()));
    if state_file != state_path(&saved.dest) {
        return Err(bad("does not belong to the file it names"));
    }
    let _reservation =
        naming::reserve(&saved.dest).ok_or_else(|| Error::InUse(saved.dest.clone()))?;
    let part = naming::part_path(&saved.dest);
    // A crash between the final rename and removing the state file: the download is complete.
    if !part.exists() && std::fs::metadata(&saved.dest).is_ok_and(|m| m.len() == saved.size) {
        let _ = std::fs::remove_file(&state_file);
        shared.set_state(State::Completing);
        return Ok(Outcome {
            path: saved.dest.clone(),
            size: saved.size,
        });
    }
    // Signed links from a redirect expire: ask the original address again for today's target
    // (it must still be the same file), else keep the stored one.
    let client = client(&opts, &saved.url)?;
    let url = if saved.url != saved.final_url {
        refresh_final_url(&client, &saved.url, &headers, saved.size, &cancel)
            .await
            .unwrap_or_else(|| saved.final_url.clone())
    } else {
        saved.final_url.clone()
    };
    let origin = (saved.url != url).then(|| (saved.url.clone(), headers.clone()));
    // Credentials stay with the host they were given for.
    headers = scoped_headers(&headers, &saved.url, &url);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&part)
        .map_err(|e| bad(&format!("part file: {e}")))?;
    if file.metadata()?.len() != saved.size {
        return Err(bad("part file has the wrong size"));
    }
    // FTP: the server's size and modification time must still be those of the data on disk.
    let ftp = if crate::ftp::is_ftp(&url) {
        let site = Arc::new(crate::ftp::Site::new(&url, &headers, &opts)?);
        match cancellable(&cancel, site.check()).await {
            Ok(facts) => {
                let modified = facts.modified.map(httpdate::fmt_http_date);
                let time_changed = !opts.ignore_mtime
                    && modified.is_some()
                    && saved.validators.last_modified.is_some()
                    && modified != saved.validators.last_modified;
                if time_changed || facts.size.is_some_and(|n| n != saved.size) {
                    return Err(Error::FileChanged);
                }
            }
            // Not answering now: the connections retry, and each checks its overlap.
            Err(e) if e.transient() => {}
            Err(e) => return Err(e),
        }
        Some(site)
    } else {
        None
    };
    shared.event(format!(
        "Resuming: {} of {} on disk, from {}",
        disk::human(saved.downloaded()),
        disk::human(saved.size),
        url.host_str().unwrap_or("?"),
    ));
    let mut planner = Planner::resume(saved.size, opts.min_split, &saved.segments)
        .map_err(|_| bad("segments do not cover the file"))?;
    planner.set_preempt_after(PREEMPT_MIN);
    *shared.planner.lock().unwrap() = Some(planner);
    {
        let mut v = shared.view.lock().unwrap();
        v.state = State::Downloading;
        v.file = Some(saved.dest.clone());
        v.size = Some(saved.size);
        v.resumable = Some(true);
    }
    let file = Arc::new(file);
    let (dest, size) = (saved.dest.clone(), saved.size);
    let checksum = opts.checksum.clone().or_else(|| saved.checksum.clone());
    let web_page_ok = expects_web_page(&dest, saved.content_type.as_deref());
    let ctx = Arc::new(Ctx {
        client,
        validators: Mutex::new({
            let mut v = saved.validators.clone();
            if opts.ignore_mtime {
                v.last_modified = None;
            }
            v
        }),
        target: Mutex::new(Target {
            url,
            headers,
            generation: 0,
        }),
        origin,
        refreshed: Default::default(),
        size,
        file: file.clone(),
        opts,
        shared: shared.clone(),
        cancel: cancel.child_token(),
        // Ranges worked before: a 200 now is a changed file or a range-less edge.
        ranges: AtomicU8::new(RANGES_YES),
        web_page_ok,
        ftp,
    });
    let stamp = ctx
        .opts
        .server_time
        .then(|| ctx.validators.lock().unwrap().last_modified.clone())
        .flatten()
        .and_then(|lm| httpdate::parse_http_date(&lm).ok());
    segmented(ctx, saved, None).await?;
    verify_part(&shared, &dest, checksum.as_ref()).await?;
    finish(&shared, file, &dest, size, stamp)
}

/// Follow the original address again (with its own headers) and return where it leads now,
/// if that is still the same file: a ranged answer with the saved size. None keeps the stored
/// address (no redirect any more, a different file, or the site is unreachable).
async fn refresh_final_url(
    client: &Client,
    origin: &Url,
    headers: &HeaderMap,
    size: u64,
    cancel: &CancellationToken,
) -> Option<Url> {
    let req = client
        .get(origin.clone())
        .headers(headers.clone())
        .header(header::RANGE, "bytes=0-0");
    let resp = cancellable(cancel, async { Ok(req.send().await?) })
        .await
        .ok()?;
    if resp.status() != StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let (_, _, total) = parse_content_range(resp.headers()).ok()?;
    (total == Some(size)).then(|| resp.url().clone())
}

/// Check the finished part file against `hash` (at most two files at once: it reads them
/// whole). No hash, nothing to do.
async fn verify_part(
    shared: &Shared,
    dest: &Path,
    hash: Option<&crate::verify::Hash>,
) -> Result<(), Error> {
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let Some(hash) = hash.cloned() else {
        return Ok(());
    };
    shared.set_state(State::Verifying);
    let _slot = SLOTS.acquire().await.map_err(|_| Error::Cancelled)?;
    let part = naming::part_path(dest);
    let algo = hash.algo;
    let ok = tokio::task::spawn_blocking(move || crate::verify::matches(&part, &hash))
        .await
        .expect("verifier panicked")?;
    if ok {
        shared.event(format!("Checksum verified ({})", algo.name()));
        Ok(())
    } else {
        Err(Error::ChecksumMismatch(algo.name()))
    }
}

/// Flush and rename the part file to its final name.
fn finish(
    shared: &Shared,
    file: Arc<File>,
    dest: &Path,
    size: u64,
    stamp: Option<std::time::SystemTime>,
) -> Result<Outcome, Error> {
    shared.set_state(State::Completing);
    let file = Arc::try_unwrap(file).map_err(|_| io::Error::other("part file still in use"))?;
    file.set_len(size)?;
    // Every byte is written: a normal file again (nothing left to allocate).
    if size >= sparse::MIN_SIZE {
        sparse::set(&file, false);
    }
    file.sync_all()?;
    // The server's date (Options::server_time), set before the rename moves the file.
    if let Some(t) = stamp {
        let times = std::fs::FileTimes::new().set_modified(t).set_accessed(t);
        #[cfg(windows)]
        let times = {
            use std::os::windows::fs::FileTimesExt;
            times.set_created(t)
        };
        let _ = file.set_times(times);
    }
    drop(file);
    let part = naming::part_path(dest);
    // Never replace a file that appeared at the destination meanwhile (rename replaces on
    // Windows): take the next free numbered name instead.
    let original = dest;
    let dest = if dest.exists() {
        let dir = dest.parent().unwrap_or(Path::new("."));
        let name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download");
        naming::unique_path(dir, name)
    } else {
        dest.to_owned()
    };
    // Antivirus scanners and indexers briefly lock new files on Windows: retry the rename.
    craft_atomic::retry_rename(craft_atomic::RenameRetry::platform(), || {
        std::fs::rename(&part, &dest)
    })?;
    // Only now is the resume state obsolete.
    let _ = std::fs::remove_file(state_path(original));
    Ok(Outcome { path: dest, size })
}

/// Run the connections with the resume state saved every `save_interval`, and once more when
/// they stop. The state file is removed when the download completes.
async fn segmented(
    ctx: Arc<Ctx>,
    mut saved: ResumeState,
    first: Option<Body>,
) -> Result<(), Error> {
    let path = state_path(&saved.dest);
    let mut synced = Synced {
        at: std::time::Instant::now(),
        bytes: 0,
    };
    save_state(&ctx, &mut saved, &path, &mut synced, true).await?;
    let interval = ctx.opts.save_interval;
    let result = {
        // Runs until a save fails; resolves to that error.
        let saver = async {
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = save_state(&ctx, &mut saved, &path, &mut synced, false).await {
                    return e;
                }
            }
        };
        tokio::select! {
            r = supervise(ctx.clone(), first) => r,
            e = saver => Err(e),
        }
    };
    match &result {
        // Not resumable on one connection: the state is useless.
        Err(Error::NoRanges) => {
            let _ = std::fs::remove_file(&path);
        }
        // Complete: the final state stays until `finish` has renamed the part file, so a
        // crash in between still resumes (to an immediate rename).
        Ok(()) | Err(_) => save_state(&ctx, &mut saved, &path, &mut synced, true).await?,
    }
    result
}

/// While downloading, the part file is flushed (and the state rewritten) only after 1/16 of
/// the file (1–64 MiB) has arrived or this long, at most once per save tick, and always when
/// the connections stop. Flushing every tick stalled HDDs and churned antivirus; IDM saves
/// "several times per minute" [IDM-doc]. A crash costs at most that much re-download.
const SYNC_BYTES_MAX: u64 = 64 << 20;
const SYNC_BYTES_MIN: u64 = 1 << 20;
const SYNC_EVERY: Duration = Duration::from_secs(30);

fn sync_bytes(size: u64) -> u64 {
    (size / 16).clamp(SYNC_BYTES_MIN, SYNC_BYTES_MAX)
}

/// When the part file was last flushed, and how many bytes were on disk then.
struct Synced {
    at: std::time::Instant,
    bytes: u64,
}

/// Save the resume state if it is due (`force`: always). `saved` is the state last written; it
/// never claims bytes that weren't flushed (snapshot → flush → write).
async fn save_state(
    ctx: &Ctx,
    saved: &mut ResumeState,
    path: &Path,
    synced: &mut Synced,
    force: bool,
) -> Result<(), Error> {
    let segments = ctx.shared.with_planner(|p| p.save());
    let validators = ctx.validators.lock().unwrap().clone();
    let final_url = ctx.target().url;
    let meta_changed = validators != saved.validators || final_url != saved.final_url;
    let bytes: u64 = segments.iter().map(|s| s.cursor - s.start).sum();
    let due = force
        || bytes >= synced.bytes.saturating_add(sync_bytes(ctx.size))
        || synced.at.elapsed() >= SYNC_EVERY;
    let file = ctx.file.clone();
    let path = path.to_owned();
    if !due {
        // Between flushes only a new validator or address is written (with segments on disk).
        if meta_changed {
            saved.validators = validators;
            saved.final_url = final_url;
            let s = saved.clone();
            tokio::task::spawn_blocking(move || s.write(&path))
                .await
                .expect("state writer panicked")?;
        }
        return Ok(());
    }
    *synced = Synced {
        at: std::time::Instant::now(),
        bytes,
    };
    if segments == saved.segments && !meta_changed && path.exists() {
        return Ok(()); // nothing new since the last write
    }
    let mut s = saved.clone();
    s.validators = validators;
    s.final_url = final_url;
    *saved = tokio::task::spawn_blocking(move || s.save(&path, segments, &file).map(|()| s))
        .await
        .expect("state writer panicked")?;
    Ok(())
}

/// A plain `GET` (no Range) that must answer 200 or 206.
async fn get(
    client: &Client,
    url: Url,
    headers: HeaderMap,
    cancel: &CancellationToken,
) -> Result<Response, Error> {
    let req = client.get(url).headers(headers);
    let resp = cancellable(cancel, async { Ok(req.send().await?) }).await?;
    match resp.status() {
        StatusCode::OK | StatusCode::PARTIAL_CONTENT => Ok(resp),
        s if s == StatusCode::TOO_MANY_REQUESTS || s == StatusCode::SERVICE_UNAVAILABLE => {
            Err(Error::Throttled(s, retry_after(resp.headers())))
        }
        s => Err(Error::Status(s)),
    }
}

/// The first request of a download, retried as a connection is (`retry_wait`) when it fails
/// in a way a later try may not: a server busy at the moment the download starts no longer
/// fails the whole download. With the answer, how long the try that got it waited for it.
async fn first_get(
    client: &Client,
    url: &Url,
    headers: &HeaderMap,
    opts: &Options,
    shared: &Shared,
    cancel: &CancellationToken,
) -> Result<(Response, Duration), Error> {
    first_try(opts, shared, cancel, || {
        get(client, url.clone(), headers.clone(), cancel)
    })
    .await
}

/// `attempt` retried as `first_get` does; with its result, how long the try that got it took.
async fn first_try<T, F, Fut>(
    opts: &Options,
    shared: &Shared,
    cancel: &CancellationToken,
    mut attempt: F,
) -> Result<(T, Duration), Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let mut failures = 0;
    loop {
        let asked = std::time::Instant::now();
        match cancellable(cancel, attempt()).await {
            Ok(got) => return Ok((got, asked.elapsed())),
            Err(e) if e.transient() && failures < opts.max_retries => {
                failures += 1;
                shared.event(format!("Connection 1: {e}; retrying"));
                shared.conn(0, |c| c.status = ConnStatus::Retrying(e.clone()));
                if let Some(delay) = retry_wait(opts, failures, &e) {
                    cancellable(cancel, async {
                        tokio::time::sleep(delay).await;
                        Ok(())
                    })
                    .await?;
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// Run connections until the planner is complete. Respawns a connection when work is left
/// and none is running (e.g. the others retired).
async fn supervise(ctx: Arc<Ctx>, first: Option<Body>) -> Result<(), Error> {
    let mut set = JoinSet::new();
    let mut next_id: WorkerId = 0;
    if let Some(body) = first {
        // The probe's body is byte 0 onward: reserve that segment for connection 0 before any
        // other connection can split or take it.
        ctx.shared.with_planner(|p| p.assign(0, None));
        set.spawn(worker(ctx.clone(), 0, Some(body)));
        next_id = 1;
    }
    // No more connections than the host is known to allow.
    let host = ctx.target().url.host_str().unwrap_or("").to_owned();
    let learned = ctx.opts.host_limits.as_ref().and_then(|h| h.cap(&host));
    if let Some(cap) = learned.filter(|c| *c < ctx.opts.max_connections) {
        ctx.shared
            .event(format!("{host} allows {cap} connection(s) (learned)"));
    }
    let max = learned.map_or(ctx.opts.max_connections, |c| {
        c.min(ctx.opts.max_connections)
    });
    // A small file starts with fewer connections (one per INITIAL_PIECE): opened all at once
    // they would cut it into slivers and fetch some twice (benchmark 2026-10-30: 256 KiB took
    // 13-19 requests; IDM, which ramps up in waves, took 4). Later splits still go down to
    // `min_split` as connections free up.
    let size = ctx.shared.with_planner(|p| p.size());
    let by_size = u32::try_from(size.div_ceil(INITIAL_PIECE)).unwrap_or(u32::MAX);
    let max = max.min(by_size.max(1));
    while next_id < max.max(1) {
        set.spawn(worker(ctx.clone(), next_id, None));
        next_id += 1;
    }
    let mut last_err = None;
    while let Some(joined) = set.join_next().await {
        match joined.expect("worker panicked") {
            Err(e) if e.is_fatal() => {
                ctx.cancel.cancel();
                set.shutdown().await;
                return Err(e);
            }
            Err(e) => last_err = Some(e),
            Ok(()) => {}
        }
        let complete = ctx.shared.with_planner(|p| p.is_complete());
        if set.is_empty() && !complete {
            if let Some(e) = last_err.take() {
                return Err(e); // the last connection exhausted its retries
            }
            set.spawn(worker(ctx.clone(), next_id, None));
            next_id += 1;
        }
    }
    Ok(())
}

/// One connection: take an assignment, fetch it, repeat until there is nothing left to do.
async fn worker(ctx: Arc<Ctx>, id: WorkerId, mut first: Option<Body>) -> Result<(), Error> {
    let mut after = None;
    let mut failures = 0u32;
    // Bytes this connection had received at its last failure: progress since resets the budget.
    let mut at_last_failure = 0u64;
    loop {
        if ctx.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let Some(a) = ctx.shared.with_planner(|p| p.assign(id, after.take())) else {
            ctx.shared.conn(id, |c| c.status = ConnStatus::Finished);
            return Ok(());
        };
        ctx.shared.conn(id, |c| {
            c.status = ConnStatus::Connecting;
            c.speed = 0;
        });
        let target = ctx.target();

        // The server ignores ranges: only the probe's plain GET can bring data. While it
        // still streams it takes this segment over as it gets there; once it has stopped,
        // the download starts over on one connection.
        if first.is_none() && ctx.ranges.load(Ordering::SeqCst) == RANGES_NO {
            ctx.shared.with_planner(|p| p.release(id));
            return if ctx.shared.with_planner(|p| p.holds(0)) {
                ctx.shared.conn(id, |c| c.status = ConnStatus::Finished);
                Ok(())
            } else {
                Err(Error::NoRanges)
            };
        }
        let whole = first.is_some();
        // Re-fetch the last few bytes already on disk and compare them before going on.
        let overlap = if first.is_some() {
            0
        } else {
            a.written.min(ctx.opts.verify_overlap)
        };
        let result = match first.take() {
            Some(body) if a.from == 0 => Ok(body),
            Some(_) => Err(Error::BadRange(
                "probe segment no longer starts at 0".into(),
            )),
            None => request(&ctx, &target, a.from - overlap, a.to).await,
        };
        let result = match result {
            Ok(body) => receive(&ctx, id, body, a.from, a.to, overlap, whole).await,
            Err(e) => Err(e),
        };
        match result {
            Ok(Some(end)) => {
                failures = 0;
                after = Some(end);
            }
            Ok(None) => {
                // Preempted (another connection took the segment over): ask again.
                ctx.shared.with_planner(|p| p.release(id));
            }
            Err(Error::NoRanges) => {
                // The server ignores ranges: connection 0 (the probe) keeps the whole file if it
                // can, every other connection stops. If it can't have it all at once (it was
                // split), it still takes each segment over as it reaches it, while it streams.
                ctx.shared.with_planner(|p| p.release(id));
                return if ctx.shared.with_planner(|p| p.make_single(0) || p.holds(0)) {
                    ctx.shared.conn(id, |c| c.status = ConnStatus::Finished);
                    Ok(())
                } else {
                    Err(Error::NoRanges)
                };
            }
            Err(e) => {
                ctx.shared.with_planner(|p| p.release(id));
                if e.is_fatal() {
                    return Err(e);
                }
                // Shed: reconnect at once for the rest of the segment (not a failure).
                if matches!(e, Error::SlowConnection) {
                    ctx.shared.event(format!("Connection {}: {e}", id + 1));
                    continue;
                }
                let (working, receiving) = ctx
                    .shared
                    .with_planner(|p| (p.active_workers(), p.receiving_workers()));
                let others = working > 0;
                // An extra connection refused while others work: the host's connection limit.
                // Once learned (twice seen), this connection retires instead of retrying — if the
                // others fill the cap. Below it the server most likely still counts a connection
                // of ours that just closed (a shed or a split reconnects at once): retry as usual
                // (benchmark 2026-10-29: retiring here left 50 MiB on one connection for 12 s).
                if e.refused()
                    && others
                    && let Some(limits) = &ctx.opts.host_limits
                {
                    let host = ctx.target().url.host_str().unwrap_or("").to_owned();
                    if let Some(cap) = limits.refused(&host, receiving as u32)
                        && working as u32 >= cap
                    {
                        ctx.shared.event(format!(
                            "Connection {}: {host} allows {cap} connection(s) (learned)",
                            id + 1
                        ));
                        ctx.shared
                            .conn(id, |c| c.status = ConnStatus::Retired { host, cap });
                        return Ok(());
                    }
                }
                // A dead or forbidden link is not retried. While other connections work it
                // means this extra one is refused (a per-client connection limit): retire it.
                if e.link_dead() {
                    // A redirected address that expired (signed CDN links): go where the
                    // original address leads now, if that is still this file.
                    if !matches!(e, Error::LinkExpired)
                        && ctx.refresh_target(target.generation).await
                    {
                        ctx.shared.conn(id, |c| c.status = ConnStatus::Connecting);
                        continue;
                    }
                    let e = match e {
                        Error::Status(_) if ctx.origin.is_some() => Error::LinkExpired,
                        e => e,
                    };
                    ctx.shared
                        .conn(id, |c| c.status = ConnStatus::Failed(e.clone()));
                    return if others { Ok(()) } else { Err(e) };
                }
                let mut received = 0;
                ctx.shared.conn(id, |c| received = c.downloaded);
                if received > at_last_failure {
                    failures = 0;
                }
                at_last_failure = received;
                failures += 1;
                if failures > ctx.opts.max_retries {
                    let others = ctx.shared.with_planner(|p| p.active_workers()) > 0;
                    ctx.shared
                        .conn(id, |c| c.status = ConnStatus::Failed(e.clone()));
                    return if others { Ok(()) } else { Err(e) };
                }
                ctx.shared
                    .event(format!("Connection {}: {e}; retrying", id + 1));
                ctx.shared
                    .conn(id, |c| c.status = ConnStatus::Retrying(e.clone()));
                if let Some(delay) = retry_wait(&ctx.opts, failures, &e) {
                    cancellable(&ctx.cancel, async {
                        tokio::time::sleep(delay).await;
                        Ok(())
                    })
                    .await?;
                }
            }
        }
    }
}

/// `GET` bytes `[from, to)`; checks the server returned exactly that range of the same file.
/// FTP: `REST from` and `RETR`.
async fn request(ctx: &Ctx, target: &Target, from: u64, to: u64) -> Result<Body, Error> {
    if let Some(site) = &ctx.ftp {
        return match cancellable(&ctx.cancel, site.open_at(from, Some(ctx.size))).await {
            Ok(t) => {
                if from > 0 {
                    ctx.learn_ranges(true);
                }
                Ok(Body::Ftp(t))
            }
            // REST refused ("not implemented", "syntax error") where it wasn't known to work.
            Err(Error::FtpReply {
                code: 500 | 501 | 502 | 504,
                ..
            }) if from > 0 && ctx.ranges.load(Ordering::SeqCst) != RANGES_YES => {
                ctx.learn_ranges(false);
                Err(Error::NoRanges)
            }
            Err(e) => Err(e),
        };
    }
    let if_range = ctx
        .validators
        .lock()
        .unwrap()
        .if_range(target.url.host_str())
        .and_then(|v| HeaderValue::from_str(v).ok());
    let mut req = ctx
        .client
        .get(target.url.clone())
        .headers(target.headers.clone())
        .header(header::RANGE, format!("bytes={from}-{}", to - 1));
    if let Some(v) = &if_range {
        req = req.header(header::IF_RANGE, v);
    }
    let resp = cancellable(&ctx.cancel, async { Ok(req.send().await?) }).await?;
    if resp.status().is_success() && !ctx.web_page_ok && is_web_page(resp.headers()) {
        return Err(Error::GotWebPage);
    }
    match resp.status() {
        StatusCode::PARTIAL_CONTENT => ctx.learn_ranges(true),
        // The same file under another ETag (each CDN node has its own): go on with the date.
        StatusCode::OK if per_node_etag(ctx, if_range.as_ref(), &resp) => {
            return Err(Error::RangesUnavailable);
        }
        // A full body for a ranged request once ranges are known to work: If-Range failed (the
        // file changed), unless it is still the same file and this answer just lacks the range.
        StatusCode::OK if ctx.ranges.load(Ordering::SeqCst) == RANGES_YES => {
            return Err(if same_file(ctx, &resp) {
                Error::RangesUnavailable
            } else {
                Error::FileChanged
            });
        }
        StatusCode::OK => {
            ctx.learn_ranges(false);
            return Err(Error::NoRanges);
        }
        s if s == StatusCode::TOO_MANY_REQUESTS || s == StatusCode::SERVICE_UNAVAILABLE => {
            return Err(Error::Throttled(s, retry_after(resp.headers())));
        }
        s => return Err(Error::Status(s)),
    }
    let (start, _, total) = parse_content_range(resp.headers())?;
    if start != from || total.is_some_and(|t| t != ctx.size) {
        return Err(Error::BadRange(format!(
            "asked {from}-{}, got {start}..",
            to - 1
        )));
    }
    Ok(Body::Http(resp))
}

/// A full answer that still describes the file being downloaded: the same length and, when the
/// download has validators, the same one it would send.
fn same_file(ctx: &Ctx, resp: &Response) -> bool {
    let host = resp.url().host_str();
    let v = ctx.validators.lock().unwrap();
    let h = resp.headers();
    let theirs = match v.if_range(host) {
        None => return resp.content_length() == Some(ctx.size),
        Some(ours) if Some(ours) == v.etag.as_deref() => header_str(h, header::ETAG),
        Some(_) => header_str(h, header::LAST_MODIFIED),
    };
    resp.content_length() == Some(ctx.size) && theirs.is_some() && theirs == v.if_range(host)
}

/// A full answer to an ETag `If-Range` that still matches by length and Last-Modified: the
/// ETag differs per node (CDNs, mirrors), not the file. The ETag is dropped for the rest of the
/// download, so the next request validates with the date (and the overlap check).
fn per_node_etag(ctx: &Ctx, sent: Option<&HeaderValue>, resp: &Response) -> bool {
    let mut v = ctx.validators.lock().unwrap();
    // Sent with an ETag (dates never start with a quote) — possibly one another connection has
    // just dropped, so don't compare it with the current one.
    let sent_etag = sent.is_some_and(|s| s.as_bytes().starts_with(b"\""));
    let same = sent_etag
        && resp.content_length() == Some(ctx.size)
        && v.last_modified.is_some()
        && header_str(resp.headers(), header::LAST_MODIFIED) == v.last_modified.as_deref();
    if same {
        v.etag = None;
        v.etag_host = None;
    }
    same
}

fn header_str(h: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

fn is_web_page(h: &HeaderMap) -> bool {
    h.get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(is_web_page_type)
}

fn is_web_page_type(content_type: &str) -> bool {
    let mime = content_type.split(';').next().unwrap_or("").trim();
    mime.eq_ignore_ascii_case("text/html") || mime.eq_ignore_ascii_case("application/xhtml+xml")
}

/// Whether the download is itself a web page (it was served as one, or is named like one).
fn expects_web_page(dest: &Path, content_type: Option<&str>) -> bool {
    content_type.is_some_and(is_web_page_type)
        || dest.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["htm", "html", "xhtml", "shtml"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        })
}

/// Stream a response into the file at `from`. `Some(end)` when the segment is complete,
/// `None` when the segment was taken over by another connection.
///
/// Every chunk is claimed from the planner as soon as it arrives (not when the buffer is
/// flushed), so a split can never fall inside data this connection has already received.
async fn receive(
    ctx: &Ctx,
    id: WorkerId,
    mut body: Body,
    from: u64,
    to: u64,
    mut verify: u64,
    whole: bool,
) -> Result<Option<u64>, Error> {
    ctx.shared.conn(id, |c| c.status = ConnStatus::Receiving);
    // Written and committed up to `pos`; `buf` holds claimed bytes not yet written.
    let mut pos = from;
    let mut buf = Vec::with_capacity(BATCH);
    let mut flushed_at = std::time::Instant::now();
    let started = std::time::Instant::now();
    let mut received = 0u64;
    let mut checked = started;
    loop {
        let next = tokio::select! {
            _ = ctx.cancel.cancelled() => {
                // Paused or stopped: keep what arrived (it is claimed, so it is ours to write).
                flush(ctx, id, &mut pos, &mut buf).await?;
                return Err(Error::Cancelled);
            }
            n = body.chunk() => n,
        };
        let (ended, err) = match next {
            Ok(Some(mut chunk)) => {
                // The response starts `verify` bytes before `from`: they must equal the disk.
                if verify > 0 {
                    let k = verify.min(chunk.len() as u64);
                    let (file, at) = (ctx.file.clone(), from - verify);
                    let disk = tokio::task::spawn_blocking(move || read_at(&file, at, k as usize))
                        .await
                        .expect("reader panicked")?;
                    if disk[..] != chunk[..k as usize] {
                        return Err(Error::FileChanged);
                    }
                    verify -= k;
                    chunk = chunk.slice(k as usize..);
                    if chunk.is_empty() {
                        continue;
                    }
                }
                let want = chunk.len() as u64;
                // Pay first: the wait is when other connections may split this segment, and
                // the claim below then grants only what still belongs to it.
                ctx.shared.limiter.acquire(want).await;
                if let Some(g) = &ctx.opts.global {
                    g.acquire(want).await;
                }
                let at = pos + buf.len() as u64;
                // An answer that runs to the end of the file (`whole`: the probe's plain GET),
                // while ranges aren't known to work: at its segment's end it takes over the
                // split-off segments nothing has arrived for yet, rather than dropping bytes it
                // is already receiving for a range request the server may ignore.
                let keep_reading = whole && ctx.ranges.load(Ordering::SeqCst) != RANGES_YES;
                let claimed = ctx.shared.with_planner(|p| {
                    let Claim::Granted(mut n) = p.claim(id, at, want) else {
                        return None;
                    };
                    while keep_reading && n < want && p.absorb_next(id) {
                        match p.claim(id, at + n, want - n) {
                            Claim::Granted(m) => n += m,
                            Claim::Lost => break,
                        }
                    }
                    // Ending exactly at the boundary: take the next segment over too.
                    let full =
                        p.room(id, at + n) == Some(0) && !(keep_reading && p.absorb_next(id));
                    Some((n, full))
                });
                // Claimed bytes can't be taken over, so a lost segment has an empty buffer.
                let refund = |n: u64| {
                    ctx.shared.limiter.refund(n);
                    if let Some(g) = &ctx.opts.global {
                        g.refund(n);
                    }
                };
                let Some((n, full)) = claimed else {
                    refund(want);
                    return Ok(None);
                };
                if n < want {
                    refund(want - n);
                }
                if received == 0
                    && n > 0
                    && let Some(limits) = &ctx.opts.host_limits
                {
                    // This connection's first data: tell the host's learned cap how many are
                    // receiving now.
                    let now = ctx.shared.with_planner(|p| p.receiving_workers()) as u32;
                    limits.working(ctx.target().url.host_str().unwrap_or(""), now);
                }
                buf.extend_from_slice(&chunk[..n as usize]);
                received += n;
                if checked.elapsed() >= Duration::from_millis(500) {
                    checked = std::time::Instant::now();
                    let speed = (received as f64 / started.elapsed().as_secs_f64()) as u64;
                    ctx.shared.conn(id, |c| c.speed = speed);
                    note_best(ctx, received, started);
                    if should_shed(ctx, id, speed, started.elapsed(), pos + buf.len() as u64) {
                        flush(ctx, id, &mut pos, &mut buf).await?;
                        return Err(Error::SlowConnection);
                    }
                }
                if buf.len() < BATCH && !full && flushed_at.elapsed() < FLUSH_EVERY {
                    continue;
                }
                (false, None)
            }
            Err(e) => (true, Some(e)),
            Ok(None) => (true, None),
        };
        flushed_at = std::time::Instant::now();
        match flush(ctx, id, &mut pos, &mut buf).await? {
            Some(end) => {
                note_best(ctx, received, started);
                // A segment another connection shortened leaves an HTTP body far from its end
                // (dropped); an FTP session is kept either way.
                if err.is_none() && (pos >= to || matches!(body, Body::Ftp(_))) {
                    body.done(ended).await;
                }
                return Ok(Some(end));
            }
            None if ended => return Err(err.unwrap_or(Error::ShortBody)),
            None => {}
        }
    }
}

/// Remember the speed of a response that carried enough (256 KiB) to say what this server can
/// do over one connection.
fn note_best(ctx: &Ctx, received: u64, started: std::time::Instant) {
    let secs = started.elapsed().as_secs_f64();
    if received >= 256 * 1024 && secs > 0.0 {
        ctx.shared
            .best_speed
            .fetch_max((received as f64 / secs) as u64, Ordering::Relaxed);
    }
}

/// Whether connection `id` (at `speed` for `elapsed` on this response, next byte `at`) should
/// reconnect: far slower than the best a connection of this download sustained (comparing
/// with the others fails once they are all stuck on slow paths), with a lot left at its pace.
/// Never while a speed limit makes connections slow on purpose.
fn should_shed(ctx: &Ctx, id: WorkerId, speed: u64, elapsed: Duration, at: u64) -> bool {
    let limited = ctx.shared.limiter.get().is_some()
        || ctx.opts.global.as_ref().is_some_and(|g| g.get().is_some());
    if !ctx.opts.shed_slow || limited || elapsed < SHED_GRACE {
        return false;
    }
    let best = ctx.shared.best_speed.load(Ordering::Relaxed);
    let left = ctx.shared.with_planner(|p| p.room(id, at)).unwrap_or(0);
    speed.saturating_mul(SHED_RATIO) < best
        && left as f64 / speed.max(1) as f64 > SHED_LEFT.as_secs_f64()
}

/// Write the claimed bytes in `buf` at `pos` and commit them. `Some(end)` when the segment is
/// now complete.
async fn flush(
    ctx: &Ctx,
    id: WorkerId,
    pos: &mut u64,
    buf: &mut Vec<u8>,
) -> Result<Option<u64>, Error> {
    let data = std::mem::replace(buf, Vec::with_capacity(BATCH));
    let n = data.len() as u64;
    if n > 0 {
        let file = ctx.file.clone();
        let at = *pos;
        tokio::task::spawn_blocking(move || write_all_at(&file, at, &data))
            .await
            .expect("writer panicked")?;
    }
    *pos += n;
    ctx.shared.conn(id, |c| c.downloaded += n);
    Ok(ctx.shared.with_planner(|p| p.commit(id, *pos)))
}

/// Servers without range support: one connection, written in order. Returns the size.
async fn single_stream(
    mut body: Body,
    file: &Arc<File>,
    shared: &Arc<Shared>,
    global: Option<&Limiter>,
    cancel: &CancellationToken,
) -> Result<u64, Error> {
    shared.conn(0, |c| c.status = ConnStatus::Receiving);
    let expected = body.len();
    let mut pos = 0u64;
    loop {
        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            n = body.chunk() => n,
        };
        let Some(chunk) = next? else { break };
        shared.limiter.acquire(chunk.len() as u64).await;
        if let Some(g) = global {
            g.acquire(chunk.len() as u64).await;
        }
        let (f, at, n) = (file.clone(), pos, chunk.len() as u64);
        tokio::task::spawn_blocking(move || write_all_at(&f, at, &chunk))
            .await
            .expect("writer panicked")?;
        pos += n;
        let mut v = shared.view.lock().unwrap();
        v.downloaded = pos;
        if let Some(c) = v.connections.first_mut() {
            c.downloaded = pos;
        }
    }
    if expected.is_some_and(|e| e != pos) {
        return Err(Error::ShortBody);
    }
    shared.conn(0, |c| c.status = ConnStatus::Finished);
    Ok(pos)
}

fn read_at(file: &File, mut at: u64, len: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    let mut got = 0;
    while got < len {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(file, &mut buf[got..], at)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(file, &mut buf[got..], at)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        got += n;
        at += n as u64;
    }
    Ok(buf)
}

fn write_all_at(file: &File, mut at: u64, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_write(file, data, at)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::write_at(file, data, at)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        at += n as u64;
        data = &data[n..];
    }
    Ok(())
}

/// `Content-Range: bytes a-b/total` → `(a, b, total)`; `total` is `None` for `*`.
fn parse_content_range(h: &HeaderMap) -> Result<(u64, u64, Option<u64>), Error> {
    let raw = h
        .get(header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bad = || Error::BadRange(raw.to_owned());
    let rest = raw.strip_prefix("bytes ").ok_or_else(bad)?;
    let (range, total) = rest.split_once('/').ok_or_else(bad)?;
    let (a, b) = range.split_once('-').ok_or_else(bad)?;
    let a = a.trim().parse().map_err(|_| bad())?;
    let b = b.trim().parse().map_err(|_| bad())?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().map_err(|_| bad())?),
    };
    Ok((a, b, total))
}

/// `Retry-After`: delay in seconds, or an HTTP date.
fn retry_after(h: &HeaderMap) -> Option<Duration> {
    let v = h.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let when = httpdate::parse_http_date(v).ok()?;
    Some(
        when.duration_since(std::time::SystemTime::now())
            .unwrap_or_default(),
    )
}

async fn cancellable<T>(
    cancel: &CancellationToken,
    fut: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::select! {
        _ = cancel.cancelled() => Err(Error::Cancelled),
        r = fut => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_step_scales_with_the_file() {
        assert_eq!(sync_bytes(24 << 20), 3 << 19);
        assert_eq!(sync_bytes(100 << 10), 1 << 20);
        assert_eq!(sync_bytes(10 << 30), 64 << 20);
    }

    #[tokio::test]
    async fn network_errors_read_plainly() {
        // Nothing listens on port 1.
        let e = reqwest::get("http://127.0.0.1:1/").await.unwrap_err();
        let msg = Error::from(e).to_string();
        assert!(msg.starts_with("cannot connect to the server"), "{msg}");
        assert!(
            !msg.contains("127.0.0.1") && !msg.contains("os error"),
            "{msg}"
        );
    }

    #[test]
    fn retry_after_seconds_or_date() {
        let mut h = HeaderMap::new();
        h.insert(header::RETRY_AFTER, HeaderValue::from_static("7"));
        assert_eq!(retry_after(&h), Some(Duration::from_secs(7)));
        let later = std::time::SystemTime::now() + Duration::from_secs(90);
        h.insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&httpdate::fmt_http_date(later)).unwrap(),
        );
        let d = retry_after(&h).unwrap();
        assert!(
            d > Duration::from_secs(80) && d <= Duration::from_secs(90),
            "{d:?}"
        );
        h.insert(header::RETRY_AFTER, HeaderValue::from_static("soon"));
        assert_eq!(retry_after(&h), None);
    }

    #[test]
    fn dead_links_are_not_retried() {
        for s in [401, 403, 404, 410, 416] {
            assert!(
                Error::Status(StatusCode::from_u16(s).unwrap()).link_dead(),
                "{s}"
            );
        }
        for s in [500, 502] {
            assert!(
                !Error::Status(StatusCode::from_u16(s).unwrap()).link_dead(),
                "{s}"
            );
        }
        assert!(!Error::Throttled(StatusCode::SERVICE_UNAVAILABLE, None).link_dead());
    }

    #[test]
    fn credentials_stay_with_their_host() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("sid=1"));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic x"));
        h.insert(header::REFERER, HeaderValue::from_static("https://site/"));
        let u = |s: &str| Url::parse(s).unwrap();
        let same = scoped_headers(&h, &u("https://site/a"), &u("https://site/b"));
        assert!(same.contains_key(header::COOKIE) && same.contains_key(header::AUTHORIZATION));
        let cdn = scoped_headers(&h, &u("https://site/a"), &u("https://cdn.example/a"));
        assert!(!cdn.contains_key(header::COOKIE) && !cdn.contains_key(header::AUTHORIZATION));
        assert!(
            cdn.contains_key(header::REFERER),
            "non-credential headers stay"
        );
        let down = scoped_headers(&h, &u("https://site/a"), &u("http://site/a"));
        assert!(
            !down.contains_key(header::COOKIE),
            "no credentials over a https -> http downgrade"
        );
        let port = scoped_headers(&h, &u("http://site/a"), &u("http://site:8080/a"));
        assert!(
            !port.contains_key(header::COOKIE),
            "another port is another origin"
        );
    }

    #[test]
    fn content_range() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("bytes 0-99/1000"),
        );
        assert_eq!(parse_content_range(&h).unwrap(), (0, 99, Some(1000)));
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("bytes 5-9/*"),
        );
        assert_eq!(parse_content_range(&h).unwrap(), (5, 9, None));
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("items 0-1/2"),
        );
        assert!(parse_content_range(&h).is_err());
    }
}

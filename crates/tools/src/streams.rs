//! A video's streams downloaded by MDM's engine (#769). yt-dlp looks the page up and chooses the
//! formats (its `-f`); MDM's engine downloads each stream as it downloads any file, over the
//! connections Options → Connection gives, under the speed limits, with pause and resume and
//! retries, into the very file yt-dlp would have written (`<stem>.f<id>.<ext>` for a stream it
//! merges, `<stem>.<ext>` for a choice of one). yt-dlp then does what it does after its own
//! download: it finds the streams there ("has already been downloaded"), merges them, saves and
//! embeds the subtitles and the metadata, converts the sound. Its post-processing stays the one
//! source of truth for the finished file; only the bytes come down through the engine.
//!
//! What the engine can't download stays yt-dlp's to download ([`Helper`]): a live stream still
//! being recorded, a protocol other than HTTP (RTMP, RTSP, MMS, FTP…), protected streams.
//!
//! A stream's address is signed and expires (YouTube's after six hours). The engine asks for
//! today's one when it dies, or when a download continues after a restart, through
//! [`StreamLink`], an [`Unlocker`] that looks the page up again and takes the same format's
//! address.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use mdm_engine::download::{ConnView, SegmentView, State as DlState};
use mdm_engine::{Destination, Download, Request, ServiceFault, Unlocked, Unlocker};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Why yt-dlp downloads a choice itself rather than the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Helper {
    /// A live stream still being recorded: it has no end to download to.
    Live,
    /// Pieces of a stream (HLS, DASH), yt-dlp's name for its protocol.
    Pieces(String),
    /// A protocol the engine doesn't speak (RTMP, RTSP, MMS, FTP…), yt-dlp's name for it.
    Protocol(String),
    /// Protected (DRM): yt-dlp refuses it, as before.
    Protected,
    /// yt-dlp gave no address (or no file name) to download it from.
    NoAddress,
}

impl Helper {
    /// For the download's log (English, as yt-dlp's own lines).
    pub fn english(&self) -> String {
        match self {
            Helper::Live => "a live stream still being recorded".into(),
            Helper::Pieces(p) => format!("a stream in pieces ({p})"),
            Helper::Protocol(p) => format!("its protocol ({p})"),
            Helper::Protected => "a protected stream".into(),
            Helper::NoAddress => "yt-dlp gave no address for it".into(),
        }
    }
}

/// One stream of a choice as the engine downloads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    /// yt-dlp's format id (`137`, `hls-720p`).
    pub format_id: String,
    /// The file yt-dlp looks for: where the engine saves it.
    pub path: PathBuf,
    /// Its size: exact (yt-dlp's `filesize`) or yt-dlp's estimate (`exact` false).
    pub size: Option<u64>,
    pub exact: bool,
    pub url: Url,
    /// yt-dlp's headers for it (`http_headers`) and the cookies it would send (`Cookie`).
    pub headers: Vec<(String, String)>,
    /// yt-dlp's `http_chunk_size`: the most each request asks for (`Options::request_cap`).
    pub chunk: Option<u64>,
}

/// A choice as the engine downloads it: its streams, one after another, and the file yt-dlp makes
/// of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub output: PathBuf,
    pub streams: Vec<Stream>,
}

impl Plan {
    /// Every stream's size, as far as known: the progress's total from the start.
    pub fn total(&self) -> Option<u64> {
        self.streams.iter().map(|s| s.size).sum()
    }
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn number(v: &Value, key: &str) -> Option<u64> {
    let n = v.get(key)?;
    n.as_u64()
        .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
        .filter(|n| *n > 0)
}

/// Python's `os.path.splitext` on a file name: (`a.b`, `.mp4`) of `a.b.mp4`; a name made of a
/// leading dot alone (`.mp4`) has no extension.
fn splitext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if name[..i].chars().any(|c| c != '.') => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Where yt-dlp saves format `id` (extension `ext`) of a merged choice whose file is `output`
/// (its extension `merged`): `prepend_extension(correct_ext(output, ext), f<id>, ext)`, as its
/// `process_info` names each of `requested_formats` (`<stem>.f137.mp4`, `<stem>.f251.webm`).
pub fn stream_path(output: &Path, merged: &str, id: &str, ext: &str) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // correct_ext: the output's extension, when it is the merge's or the stream's, gives way.
    let (stem, real) = splitext(&name);
    let real = real.trim_start_matches('.');
    let base = if real == merged || real == ext {
        stem
    } else {
        &name
    };
    let corrected = format!("{base}.{ext}");
    // prepend_extension: `f<id>` before the extension it now has.
    let (stem, real) = splitext(&corrected);
    let named = if real.trim_start_matches('.') == ext {
        format!("{stem}.f{id}{real}")
    } else {
        format!("{corrected}.f{id}")
    };
    output.with_file_name(named)
}

/// The `Cookie` header of yt-dlp's `cookies` for a format (`a=1; Domain=.x.com; Path=/; b=2;
/// Secure`): the pairs, without their attributes.
pub fn cookie_header(cookies: &str) -> Option<String> {
    const ATTRIBUTES: [&str; 9] = [
        "domain", "path", "expires", "max-age", "secure", "httponly", "version", "samesite",
        "comment",
    ];
    let pairs: Vec<&str> = cookies
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter(|p| {
            let name = p.split_once('=').map_or(*p, |(n, _)| n).trim();
            p.contains('=') && !ATTRIBUTES.contains(&name.to_ascii_lowercase().as_str())
        })
        .collect();
    (!pairs.is_empty()).then(|| pairs.join("; "))
}

/// yt-dlp's headers for format `f` (`http_headers`), with its cookies as a `Cookie` header.
fn headers_of(f: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = f
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|h| {
            h.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .filter(|(k, _)| !k.eq_ignore_ascii_case("cookie"))
                .collect()
        })
        .unwrap_or_default();
    if let Some(c) = text(f, "cookies").and_then(cookie_header) {
        out.push(("Cookie".into(), c));
    }
    out
}

/// yt-dlp's protocol for format `f`: its `protocol`, else the address's (`determine_protocol`).
fn protocol_of(f: &Value) -> String {
    if let Some(p) = text(f, "protocol") {
        return p.to_owned();
    }
    let url = text(f, "url").unwrap_or("");
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("rtmp") {
        "rtmp".into()
    } else if lower.contains(".m3u8") {
        "m3u8".into()
    } else if lower.contains(".f4m") {
        "f4m".into()
    } else {
        url.split_once(':')
            .map_or_else(String::new, |(s, _)| s.to_ascii_lowercase())
    }
}

/// Format `f` as the engine downloads it into `path`, or why yt-dlp downloads it.
fn stream(f: &Value, path: PathBuf) -> Result<Stream, Helper> {
    if f.get("has_drm").and_then(Value::as_bool) == Some(true) {
        return Err(Helper::Protected);
    }
    let protocol = protocol_of(f);
    let fragments = f
        .get("fragments")
        .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()));
    match protocol.as_str() {
        "http" | "https" if !fragments => {}
        "m3u8"
        | "m3u8_native"
        | "http_dash_segments"
        | "http_dash_segments_generator"
        | "f4m"
        | "ism"
        | "https"
        | "http" => return Err(Helper::Pieces(protocol)),
        _ => return Err(Helper::Protocol(protocol)),
    }
    let url = text(f, "url")
        .and_then(|u| Url::parse(u).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or(Helper::NoAddress)?;
    let exact = number(f, "filesize");
    Ok(Stream {
        format_id: text(f, "format_id").unwrap_or("0").to_owned(),
        path,
        size: exact.or_else(|| number(f, "filesize_approx")),
        exact: exact.is_some(),
        url,
        headers: headers_of(f),
        chunk: f
            .get("downloader_options")
            .and_then(|o| number(o, "http_chunk_size")),
    })
}

/// Whether yt-dlp's page is a live stream still being recorded.
pub fn is_live(info: &Value) -> bool {
    info.get("is_live").and_then(Value::as_bool) == Some(true)
        || text(info, "live_status").is_some_and(|s| s == "is_live" || s == "is_upcoming")
}

/// The engine's plan for yt-dlp's choice (`-O "%()j"` after its format selection: the formats in
/// `requested_formats`, or the one format's own fields, and `filename`), or why yt-dlp downloads
/// it. One stream that can't be the engine's leaves the whole choice to yt-dlp.
pub fn plan(selected: &Value) -> Result<Plan, Helper> {
    if is_live(selected) {
        return Err(Helper::Live);
    }
    let output = text(selected, "filename")
        .map(PathBuf::from)
        .ok_or(Helper::NoAddress)?;
    let merged = text(selected, "ext").unwrap_or("");
    let streams = match selected.get("requested_formats").and_then(Value::as_array) {
        Some(formats) if !formats.is_empty() => formats
            .iter()
            .map(|f| {
                let id = text(f, "format_id").unwrap_or("0");
                let ext = text(f, "ext").unwrap_or(merged);
                stream(f, stream_path(&output, merged, id, ext))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => vec![stream(selected, output.clone())?],
    };
    Ok(Plan { output, streams })
}

/// The chosen format `id` in yt-dlp's choice `selected` (a look-up chosen again with `-f <id>`):
/// its address and headers.
pub fn format_link(selected: &Value, id: &str) -> Option<(Url, Vec<(String, String)>)> {
    let formats = selected
        .get("requested_formats")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect::<Vec<_>>())
        .unwrap_or_else(|| vec![selected]);
    let f = formats
        .into_iter()
        .find(|f| text(f, "format_id") == Some(id))?;
    let s = stream(f, PathBuf::new()).ok()?;
    Some((s.url, s.headers))
}

fn header_map(headers: &[(String, String)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(k), Ok(v)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            map.insert(k, v);
        }
    }
    map
}

/// Looks the page up again for today's address of a format (blocking: yt-dlp runs).
pub type Relink = Arc<dyn Fn(&str) -> Result<(Url, Vec<(String, String)>), String> + Send + Sync>;

/// A stream's address, from yt-dlp (#769): the engine's [`Unlocker`] for a stream. The first ask
/// takes the address this run's look-up gave; every later one (the address died, the download
/// continues after a stop) looks the page up again for the same format's.
pub struct StreamLink {
    format_id: String,
    size: Option<u64>,
    first: Mutex<Option<Unlocked>>,
    relink: Relink,
}

impl std::fmt::Debug for StreamLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StreamLink({})", self.format_id)
    }
}

fn unlocked(url: Url, headers: &[(String, String)], size: Option<u64>) -> Unlocked {
    Unlocked {
        url,
        file_name: None,
        size,
        max_connections: None,
        headers: header_map(headers),
    }
}

impl StreamLink {
    pub fn new(s: &Stream, relink: Relink) -> StreamLink {
        let size = s.exact.then_some(s.size).flatten();
        StreamLink {
            format_id: s.format_id.clone(),
            size,
            first: Mutex::new(Some(unlocked(s.url.clone(), &s.headers, size))),
            relink,
        }
    }
}

impl Unlocker for StreamLink {
    fn name(&self) -> &str {
        "yt-dlp"
    }

    fn unlock<'a>(
        &'a self,
        _url: &'a Url,
        note: &'a (dyn Fn(String, Option<f32>) + Send + Sync),
        _cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<Option<Unlocked>, ServiceFault>> {
        Box::pin(async move {
            if let Some(u) = self.first.lock().unwrap().take() {
                return Ok(Some(u));
            }
            note(
                format!(
                    "Asking yt-dlp for a new address for format {}",
                    self.format_id
                ),
                None,
            );
            let (relink, id) = (self.relink.clone(), self.format_id.clone());
            let got = tokio::task::spawn_blocking(move || relink(&id))
                .await
                .map_err(|_| ServiceFault::Other("yt-dlp stopped".into()))?;
            match got {
                Ok((url, headers)) => Ok(Some(unlocked(url, &headers, self.size))),
                Err(e) => Err(ServiceFault::Other(e)),
            }
        })
    }

    fn noted(&self, u: &Unlocked) -> String {
        format!(
            "yt-dlp's address for format {}: from {}",
            self.format_id,
            u.url.host_str().unwrap_or("?")
        )
    }
}

/// MDM's engine for a video's streams: the app's runtime, and the options a file from the page
/// would get (connections, proxy, network adapter, DNS, the limit for all downloads, the hosts'
/// learned limits, this download's own speed limit).
#[derive(Clone)]
pub struct Engine {
    pub handle: tokio::runtime::Handle,
    pub opts: mdm_engine::Options,
}

/// The engine's download under way and its speed limit, shared with [`crate::video::VideoJob`]:
/// a new limit applies to it at once, and Stop cancels it.
#[derive(Default)]
pub struct Current {
    dl: Mutex<Option<Download>>,
    limit: Mutex<Option<u64>>,
    /// An engine download is under way or ending: its resume state isn't saved yet.
    busy: AtomicBool,
}

impl Current {
    pub fn new(limit: Option<u64>) -> Current {
        Current {
            limit: Mutex::new(limit),
            ..Default::default()
        }
    }

    pub fn set_limit(&self, rate: Option<u64>) {
        *self.limit.lock().unwrap() = rate;
        if let Some(dl) = self.dl.lock().unwrap().as_ref() {
            dl.set_speed_limit(rate);
        }
    }

    pub fn limit(&self) -> Option<u64> {
        *self.limit.lock().unwrap()
    }

    pub fn cancel(&self) {
        if let Some(dl) = self.dl.lock().unwrap().as_ref() {
            dl.cancel();
        }
    }

    /// An engine download is still ending (Stop was asked, its state isn't saved yet).
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

/// What the engine's part showed last: the streams' segments one after another, the
/// connections of the stream under way, its server.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub segments: Vec<SegmentView>,
    pub connections: Vec<ConnView>,
    pub server: Option<mdm_engine::Server>,
}

/// How the engine's part of a run ended.
#[derive(Debug)]
pub enum Fetched {
    /// Every stream is in place.
    Done,
    Stopped,
    Failed(String),
}

/// What [`fetch`] reports as it goes: bytes in, the total, the speed (bytes/s), the view, and the
/// engine's own log lines.
pub trait Report {
    fn progress(&self, downloaded: u64, total: Option<u64>, speed: f64, view: View);
    fn note(&self, text: String);
    fn stopped(&self) -> bool;
}

/// Each stream's size for the total: the engine's own once it knows it, else the plan's.
fn sizes(plan: &Plan, known: &[Option<u64>]) -> Vec<Option<u64>> {
    plan.streams
        .iter()
        .zip(known)
        .map(|(s, k)| k.or(s.size))
        .collect()
}

/// The streams' segments one after another: those before `at` whole, `at`'s from the engine
/// (`now`), those after it waiting.
fn segments(sizes: &[Option<u64>], at: usize, now: &[SegmentView]) -> Vec<SegmentView> {
    let mut out = Vec::new();
    let mut offset = 0u64;
    for (i, size) in sizes.iter().enumerate() {
        let len = size.unwrap_or(0);
        if i < at {
            out.push(SegmentView {
                start: offset,
                cursor: offset + len,
                end: offset + len,
            });
        } else if i == at {
            out.extend(now.iter().map(|s| SegmentView {
                start: offset + s.start,
                cursor: offset + s.cursor,
                end: offset + s.end,
            }));
        } else if len > 0 {
            out.push(SegmentView {
                start: offset,
                cursor: offset,
                end: offset + len,
            });
        }
        offset += len;
    }
    out
}

/// Download `plan`'s streams with the engine, one after another, each over every connection it
/// may open, into the files yt-dlp looks for. A stream already whole is kept; one with a resume
/// state continues (its address asked for again through `relink`). `page` stands for every
/// stream's address in the list and the resume state (its own address expires).
pub fn fetch(
    plan: &Plan,
    engine: &Engine,
    page: &Url,
    relink: &Relink,
    current: &Current,
    report: &dyn Report,
) -> Fetched {
    let mut known: Vec<Option<u64>> = vec![None; plan.streams.len()];
    for (i, s) in plan.streams.iter().enumerate() {
        if report.stopped() {
            return Fetched::Stopped;
        }
        let state = mdm_engine::state::state_path(&s.path);
        // Whole already (an earlier run's, its merge not done): yt-dlp takes it as it is.
        if !state.exists() && s.path.is_file() {
            known[i] = std::fs::metadata(&s.path).ok().map(|m| m.len());
            continue;
        }
        if let Some(dir) = s.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut opts = engine.opts.clone();
        opts.unlocker = Some(Arc::new(StreamLink::new(s, relink.clone())));
        opts.request_cap = s.chunk;
        opts.speed_limit = current.limit();
        // yt-dlp's headers are the stream's: no other User-Agent, address, type or form.
        if let Some((_, ua)) = s
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        {
            opts.speak_as(ua.clone());
        }
        opts.agent_fallback = None;
        opts.browser_final = None;
        opts.expected_type = None;
        opts.post = None;
        opts.checksum = None;
        opts.on_file_created = None;
        let dl = {
            let _rt = engine.handle.enter();
            if state.exists() {
                match Download::resume(state.clone(), HeaderMap::new(), opts.clone()) {
                    Ok(dl) => dl,
                    Err(e) => {
                        // A state that can't be read (another release's, damaged): the stream
                        // starts again rather than the video failing.
                        report.note(format!(
                            "Format {}: its saved progress can't be continued ({e}); starting it again",
                            s.format_id
                        ));
                        let _ = std::fs::remove_file(&state);
                        let _ = std::fs::remove_file(mdm_engine::naming::part_path(&s.path));
                        fresh(page, s, opts)
                    }
                }
            } else {
                fresh(page, s, opts)
            }
        };
        current.busy.store(true, Ordering::Release);
        *current.dl.lock().unwrap() = Some(dl);
        let ended = follow(plan, i, &mut known, current, report);
        let dl = current.dl.lock().unwrap().take();
        let out = dl.map(|dl| engine.handle.block_on(dl.wait()));
        current.busy.store(false, Ordering::Release);
        match (ended, out) {
            (DlState::Done, Some(Ok(o))) if o.path == s.path => known[i] = Some(o.size),
            (DlState::Done, Some(Ok(o))) => {
                return Fetched::Failed(format!(
                    "the stream was saved as {} instead of {}",
                    o.path.display(),
                    s.path.display()
                ));
            }
            (DlState::Cancelled, _) | (_, Some(Err(mdm_engine::Error::Cancelled))) => {
                return Fetched::Stopped;
            }
            (_, Some(Err(e))) => return Fetched::Failed(e.to_string()),
            (_, _) => return Fetched::Failed("the engine stopped".into()),
        }
    }
    Fetched::Done
}

fn fresh(page: &Url, s: &Stream, opts: mdm_engine::Options) -> Download {
    Download::start(
        Request {
            url: page.clone(),
            headers: HeaderMap::new(),
            dest: Destination::File(s.path.clone()),
        },
        opts,
    )
}

/// Follow stream `i`'s engine download until it ends, reporting the whole choice's progress;
/// Stop cancels it. Its state at the end.
fn follow(
    plan: &Plan,
    i: usize,
    known: &mut [Option<u64>],
    current: &Current,
    report: &dyn Report,
) -> DlState {
    let mut seen = 0u64;
    let mut cancelled = false;
    loop {
        let Some(s) = current.dl.lock().unwrap().as_ref().map(Download::snapshot) else {
            return DlState::Failed;
        };
        let from = seen;
        for e in s.events.iter().filter(|e| e.seq > from) {
            seen = e.seq;
            report.note(e.text.clone());
        }
        if s.size.is_some() {
            known[i] = s.size;
        }
        let sizes = sizes(plan, known);
        let before: u64 = sizes[..i].iter().map(|s| s.unwrap_or(0)).sum();
        let total = sizes.iter().copied().sum::<Option<u64>>();
        let speed: u64 = s.connections.iter().map(|c| c.speed).sum();
        report.progress(
            before + s.downloaded,
            total,
            speed as f64,
            View {
                segments: segments(&sizes, i, &s.segments),
                connections: s.connections.clone(),
                server: s.server.clone(),
            },
        );
        if matches!(
            s.state,
            DlState::Done | DlState::Failed | DlState::Cancelled
        ) {
            return s.state;
        }
        if report.stopped() && !cancelled {
            cancelled = true;
            current.cancel();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn yt_dlp_names_each_stream_of_a_merge() {
        let out = Path::new("/v/Big Buck.mp4");
        assert_eq!(
            stream_path(out, "mp4", "399", "mp4"),
            Path::new("/v/Big Buck.f399.mp4")
        );
        assert_eq!(
            stream_path(out, "mp4", "251", "webm"),
            Path::new("/v/Big Buck.f251.webm")
        );
        // A stem with dots of its own keeps them.
        assert_eq!(
            stream_path(Path::new("/v/a.b.c.mkv"), "mkv", "hls-720p", "mp4"),
            Path::new("/v/a.b.c.fhls-720p.mp4")
        );
    }

    #[test]
    fn cookies_lose_their_attributes() {
        assert_eq!(
            cookie_header("a=1; Domain=.example.com; Path=/; Secure; Expires=1794448205; b=x=y")
                .as_deref(),
            Some("a=1; b=x=y")
        );
        assert_eq!(cookie_header("Domain=.x; Path=/"), None);
        assert_eq!(cookie_header(""), None);
    }

    fn youtube() -> Value {
        json!({
            "id": "aqz-KE-bpKQ", "ext": "mp4", "format_id": "399+258",
            "filename": "/v/Big Buck.mp4", "is_live": false,
            "requested_formats": [
                {"format_id": "399", "ext": "mp4", "protocol": "https", "filesize": 124386876,
                 "url": "https://rr2---sn-a.googlevideo.com/videoplayback?itag=399",
                 "http_headers": {"User-Agent": "UA/1", "Accept": "*/*"},
                 "downloader_options": {"http_chunk_size": 10485760}},
                {"format_id": "258", "ext": "m4a", "protocol": "https", "filesize_approx": 30767602,
                 "url": "https://rr2---sn-a.googlevideo.com/videoplayback?itag=258",
                 "http_headers": {"User-Agent": "UA/1"},
                 "cookies": "SID=s; Domain=.googlevideo.com; Path=/"}
            ]
        })
    }

    #[test]
    fn a_merged_choice_is_its_streams_one_after_another() {
        let p = plan(&youtube()).unwrap();
        assert_eq!(p.output, Path::new("/v/Big Buck.mp4"));
        assert_eq!(p.streams.len(), 2);
        let (v, a) = (&p.streams[0], &p.streams[1]);
        assert_eq!(v.format_id, "399");
        assert_eq!(v.path, Path::new("/v/Big Buck.f399.mp4"));
        assert_eq!((v.size, v.exact), (Some(124386876), true));
        assert_eq!(v.chunk, Some(10485760));
        assert!(v.headers.contains(&("User-Agent".into(), "UA/1".into())));
        assert_eq!(a.path, Path::new("/v/Big Buck.f258.m4a"));
        assert_eq!((a.size, a.exact, a.chunk), (Some(30767602), false, None));
        assert!(a.headers.contains(&("Cookie".into(), "SID=s".into())));
        assert_eq!(p.total(), Some(124386876 + 30767602));
        let (url, headers) = format_link(&youtube(), "258").unwrap();
        assert!(url.as_str().ends_with("itag=258"));
        assert!(headers.iter().any(|(k, _)| k == "Cookie"));
        assert!(format_link(&youtube(), "137").is_none());
    }

    #[test]
    fn a_choice_of_one_format_is_one_stream_at_the_file() {
        let one = json!({
            "format_id": "18", "ext": "mp4", "protocol": "https", "filename": "/v/Talk.mp4",
            "url": "https://upload.example/talk.mp4", "filesize": 1000
        });
        let p = plan(&one).unwrap();
        assert_eq!(p.streams.len(), 1);
        assert_eq!(p.streams[0].path, Path::new("/v/Talk.mp4"));
        assert_eq!(p.streams[0].chunk, None);
        // No protocol said: the address's.
        let mut bare = one.clone();
        bare.as_object_mut().unwrap().remove("protocol");
        assert!(plan(&bare).is_ok());
    }

    #[test]
    fn what_stays_with_yt_dlp() {
        let with = |key: &str, v: Value| {
            let mut f = youtube();
            f["requested_formats"][1][key] = v;
            plan(&f)
        };
        assert_eq!(
            with("protocol", json!("m3u8_native")),
            Err(Helper::Pieces("m3u8_native".into()))
        );
        assert_eq!(
            with("protocol", json!("http_dash_segments")),
            Err(Helper::Pieces("http_dash_segments".into()))
        );
        assert_eq!(
            with("fragments", json!([{"url": "a"}])),
            Err(Helper::Pieces("https".into()))
        );
        assert_eq!(
            with("protocol", json!("rtmp")),
            Err(Helper::Protocol("rtmp".into()))
        );
        assert_eq!(with("has_drm", json!(true)), Err(Helper::Protected));
        assert_eq!(with("url", json!("ftp://x/y")), Err(Helper::NoAddress));
        let mut live = youtube();
        live["is_live"] = json!(true);
        assert_eq!(plan(&live), Err(Helper::Live));
        let mut live = youtube();
        live["live_status"] = json!("is_live");
        assert_eq!(plan(&live), Err(Helper::Live));
        let mut unnamed = youtube();
        unnamed.as_object_mut().unwrap().remove("filename");
        assert_eq!(plan(&unnamed), Err(Helper::NoAddress));
    }

    #[test]
    fn the_segment_map_runs_through_every_stream() {
        let sizes = [Some(100), Some(50)];
        // The video under way, half of its first half in.
        let now = [
            SegmentView {
                start: 0,
                cursor: 25,
                end: 50,
            },
            SegmentView {
                start: 50,
                cursor: 60,
                end: 100,
            },
        ];
        let map = segments(&sizes, 0, &now);
        assert_eq!(map.len(), 3);
        assert_eq!(
            map[2],
            SegmentView {
                start: 100,
                cursor: 100,
                end: 150
            }
        );
        // The sound under way: the video whole before it.
        let map = segments(&sizes, 1, &now[..1]);
        assert_eq!(
            map[0],
            SegmentView {
                start: 0,
                cursor: 100,
                end: 100
            }
        );
        assert_eq!(
            map[1],
            SegmentView {
                start: 100,
                cursor: 125,
                end: 150
            }
        );
    }
}

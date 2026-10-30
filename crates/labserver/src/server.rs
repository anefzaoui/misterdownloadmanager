//! The lab HTTP server: generated files with Range support, a request log, and fault injection.
//!
//! Routes:
//! - `GET|HEAD /gen/{size}[/{name}]?seed=N&<faults>` — deterministic content (see `content`)
//! - `GET /redirect?to=URL[&ttl_ms=N]` — 302 to `URL` (cross-host redirects, credential scoping);
//!   with `ttl_ms` the target gets `expires=<unix ms>` and answers 403 after it (signed CDN links)
//! - `GET /_log` — the request log as JSON; `DELETE /_log` clears it

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::content;
use crate::faults::{Faults, every};

const CHUNK: u64 = 64 * 1024;

/// One served request. `peer` is the client's ip:port, so the port identifies the TCP
/// connection; `conn_req` > 1 means the connection was reused (keep-alive).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub req: u64,
    pub peer: SocketAddr,
    pub conn_req: u64,
    pub t_start_ms: u64,
    pub t_end_ms: u64,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub range: Option<String>,
    pub if_range: Option<String>,
    pub user_agent: Option<String>,
    /// The `Cookie` header, to check credentials don't follow cross-host redirects.
    #[serde(default)]
    pub cookie: Option<String>,
    pub status: u16,
    /// First body byte offset within the file, and planned body length.
    pub body_start: u64,
    pub body_len: u64,
    pub bytes_sent: u64,
    /// `complete`, `dropped` (fault), `aborted` (client closed early), `head`, `error`, `fault:*`.
    pub outcome: String,
    pub rate: Option<u64>,
    pub drop_at: Option<u64>,
}

pub type LogSink = Arc<dyn Fn(&LogEntry) + Send + Sync>;

pub struct Config {
    pub bind: SocketAddr,
    pub faults: Faults,
    /// Called for every finished request (e.g. print JSON lines).
    pub on_log: Option<LogSink>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            faults: Faults::default(),
            on_log: None,
        }
    }
}

struct Shared {
    faults: Faults,
    started: Instant,
    requests: AtomicU64,
    conn_reqs: Mutex<HashMap<SocketAddr, u64>>,
    active: Mutex<HashMap<IpAddr, u32>>,
    log: Mutex<Vec<LogEntry>>,
    on_log: Option<LogSink>,
}

impl Shared {
    fn ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn record(&self, mut entry: LogEntry, status: u16, outcome: &str) {
        entry.status = status;
        entry.outcome = outcome.to_owned();
        entry.t_end_ms = self.ms();
        if let Some(sink) = &self.on_log {
            sink(&entry);
        }
        self.log.lock().unwrap().push(entry);
    }
}

/// A running lab server. Stops when dropped.
pub struct LabServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl LabServer {
    pub async fn start(cfg: Config) -> io::Result<Self> {
        let listener = TcpListener::bind(cfg.bind).await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            faults: cfg.faults,
            started: Instant::now(),
            requests: AtomicU64::new(0),
            conn_reqs: Mutex::default(),
            active: Mutex::default(),
            log: Mutex::default(),
            on_log: cfg.on_log,
        });
        let app = Router::new()
            .route("/", get(help))
            .route("/gen/{size}", get(generated))
            .route("/gen/{size}/{*name}", get(generated))
            .route("/redirect", get(redirect))
            .route("/page", get(page))
            .route("/_log", get(get_log).delete(clear_log))
            .with_state(shared.clone());
        let task = tokio::spawn(async move {
            let svc = app.into_make_service_with_connect_info::<SocketAddr>();
            if let Err(e) = axum::serve(listener, svc).await {
                eprintln!("labserver: {e}");
            }
        });
        Ok(LabServer { addr, shared, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://<addr><path>`; `path` starts with `/`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn log(&self) -> Vec<LogEntry> {
        self.shared.log.lock().unwrap().clone()
    }

    pub fn clear_log(&self) {
        self.shared.log.lock().unwrap().clear();
    }

    /// Resolves when the server task ends (only on error; runs until dropped otherwise).
    pub async fn wait(&mut self) {
        let _ = (&mut self.task).await;
    }
}

impl Drop for LabServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn help() -> &'static str {
    "labserver\n\nGET|HEAD /gen/{size}[/{name}]?seed=N&<faults>   size: 12345, 64K, 100M, 1G\n\
     faults: rate slow_every slow_rate max_conns drop_after drop_every fail_every fail_first\n\
     \x20       no_range no_range_every html etag_change_every etag=weak|per-request|none latency_ms\n\
     GET /redirect?to=URL[&ttl_ms=N]   (signed link expiring after N ms)\n\
     GET /page?link=PATH&video=PATH   (a page with a link and a 640x360 video: browser tests)\n\
     GET /_log   DELETE /_log\n"
}

/// `/page?link=PATH&video=PATH`: a web page with a link (`#link`) and a 640 × 360 video
/// (`#video`), each there when given: what the browser extension acts on
/// (tools/extension/e2e.mjs).
async fn page(RawQuery(q): RawQuery) -> Response {
    let m = q
        .and_then(|q| serde_urlencoded::from_str::<HashMap<String, String>>(&q).ok())
        .unwrap_or_default();
    let attr = |s: &str| {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
    };
    let mut body = String::from(
        "<!doctype html><meta charset=utf-8><title>labserver page</title>\
         <body style=\"margin:20px;font:14px sans-serif\">",
    );
    if let Some(link) = m.get("link") {
        body += &format!("<p><a id=link href=\"{}\">download</a></p>", attr(link));
    }
    if let Some(video) = m.get("video") {
        body += &format!(
            "<video id=video src=\"{}\" width=640 height=360 controls style=\"background:#000\">\
             </video>",
            attr(video)
        );
    }
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

/// `/redirect?to=URL[&ttl_ms=N]`: 302 to `URL`; with `ttl_ms`, to a signed link that expires.
async fn redirect(RawQuery(q): RawQuery) -> Response {
    let mut m = q
        .and_then(|q| serde_urlencoded::from_str::<HashMap<String, String>>(&q).ok())
        .unwrap_or_default();
    let Some(mut to) = m.remove("to") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if let Some(ttl) = m.get("ttl_ms").and_then(|t| t.parse::<u64>().ok()) {
        let sep = if to.contains('?') { '&' } else { '?' };
        to = format!("{to}{sep}expires={}", unix_ms() + ttl);
    }
    (StatusCode::FOUND, [(header::LOCATION, to)]).into_response()
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

async fn get_log(State(s): State<Arc<Shared>>) -> Json<Vec<LogEntry>> {
    Json(s.log.lock().unwrap().clone())
}

async fn clear_log(State(s): State<Arc<Shared>>) -> StatusCode {
    s.log.lock().unwrap().clear();
    StatusCode::NO_CONTENT
}

fn header_str(h: &HeaderMap, name: header::HeaderName) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
}

/// Releases a client's concurrent-response slot (for `max_conns`) when dropped.
struct ActiveSlot {
    shared: Arc<Shared>,
    ip: IpAddr,
}

impl Drop for ActiveSlot {
    fn drop(&mut self) {
        let mut active = self.shared.active.lock().unwrap();
        if let Some(n) = active.get_mut(&self.ip) {
            *n = n.saturating_sub(1);
        }
    }
}

async fn generated(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    Path(params): Path<HashMap<String, String>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let n = shared.requests.fetch_add(1, Ordering::SeqCst) + 1;
    let conn_req = {
        let mut m = shared.conn_reqs.lock().unwrap();
        let c = m.entry(peer).or_insert(0);
        *c += 1;
        *c
    };
    let size_param = params.get("size").cloned().unwrap_or_default();
    let mut entry = LogEntry {
        req: n,
        peer,
        conn_req,
        t_start_ms: shared.ms(),
        t_end_ms: 0,
        method: method.to_string(),
        path: format!(
            "/gen/{size_param}{}",
            params
                .get("name")
                .map(|x| format!("/{x}"))
                .unwrap_or_default()
        ),
        query: query.clone(),
        range: header_str(&headers, header::RANGE),
        if_range: header_str(&headers, header::IF_RANGE),
        user_agent: header_str(&headers, header::USER_AGENT),
        cookie: header_str(&headers, header::COOKIE),
        status: 0,
        body_start: 0,
        body_len: 0,
        bytes_sent: 0,
        outcome: String::new(),
        rate: None,
        drop_at: None,
    };

    let Some(size) = content::parse_size(&size_param) else {
        shared.record(entry, 404, "error");
        return (StatusCode::NOT_FOUND, "bad size\n").into_response();
    };

    // `seed` selects the content; every other query key is a fault override.
    let (seed, expires, over) = match split_query(query.as_deref().unwrap_or("")) {
        Ok(v) => v,
        Err(msg) => {
            shared.record(entry, 400, "error");
            return (StatusCode::BAD_REQUEST, msg).into_response();
        }
    };
    if expires.is_some_and(|t| unix_ms() > t) {
        shared.record(entry, 403, "expired");
        return (StatusCode::FORBIDDEN, "link expired\n").into_response();
    }
    let f = shared.faults.overridden_by(&over);

    if let Some(ms) = f.latency_ms {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
    if every(n, f.fail_every) {
        shared.record(entry, 503, "fault:fail_every");
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if f.fail_first.is_some_and(|k| n <= k) {
        shared.record(entry, 503, "fault:fail_first");
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if f.html == Some(true) {
        shared.record(entry, 200, "fault:html");
        return (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<!doctype html><title>Sign in</title><form>Please sign in</form>\n",
        )
            .into_response();
    }
    let slot = {
        let mut active = shared.active.lock().unwrap();
        let cur = active.entry(peer.ip()).or_insert(0);
        if f.max_conns.is_some_and(|max| *cur >= max) {
            drop(active);
            shared.record(entry, 503, "fault:max_conns");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        *cur += 1;
        ActiveSlot {
            shared: shared.clone(),
            ip: peer.ip(),
        }
    };

    let generation = f.etag_change_every.filter(|k| *k > 0).map_or(0, |k| n / k);
    let etag = match f.etag.as_deref() {
        Some("weak") => Some(format!("W/\"{seed:x}-{size:x}-{generation}\"")),
        Some("per-request") => Some(format!("\"{seed:x}-{size:x}-{generation}-r{n}\"")),
        Some("none") => None,
        _ => Some(format!("\"{seed:x}-{size:x}-{generation}\"")),
    };
    let last_modified =
        httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(1_790_000_000 + generation * 60));

    // Decide the byte range to send.
    let if_range_ok = entry
        .if_range
        .as_deref()
        .is_none_or(|v| Some(v) == etag.as_deref() || v == last_modified);
    let mut range = None;
    // A syntactically invalid Range is ignored (RFC 9110 §14.2), as is a multi-range request.
    if f.no_range != Some(true)
        && !every(n, f.no_range_every)
        && if_range_ok
        && let Some(r) = entry.range.as_deref()
        && let Ok(parsed) = http_range_header::parse_range_header(r)
    {
        match parsed.validate(size) {
            Ok(v) if v.len() == 1 => range = Some((*v[0].start(), *v[0].end() + 1)),
            Ok(_) => {}
            Err(_) => {
                shared.record(entry, 416, "error");
                let mut resp = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                resp.headers_mut().insert(
                    header::CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{size}")).unwrap(),
                );
                return resp;
            }
        }
    }
    let (start, end, status) = match range {
        Some((s, e)) => (s, e, StatusCode::PARTIAL_CONTENT),
        None => (0, size, StatusCode::OK),
    };
    entry.body_start = start;
    entry.body_len = end - start;

    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, end - start)
        .header(header::LAST_MODIFIED, &last_modified);
    if f.no_range != Some(true) {
        builder = builder.header(header::ACCEPT_RANGES, "bytes");
    }
    if let Some(etag) = &etag {
        builder = builder.header(header::ETAG, etag);
    }
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{size}", end - 1),
        );
    }
    if let Some(name) = params.get("name") {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        );
    }

    if method == Method::HEAD {
        drop(slot);
        shared.record(entry, status.as_u16(), "head");
        return builder.body(Body::empty()).unwrap();
    }

    let rate = if every(n, f.slow_every) {
        f.slow_rate.or(f.rate)
    } else {
        f.rate
    };
    let drop_at = f
        .drop_after
        .filter(|_| f.drop_every.is_none() || every(n, f.drop_every));
    entry.rate = rate;
    entry.drop_at = drop_at;
    entry.status = status.as_u16();

    let transfer = Transfer {
        shared: shared.clone(),
        entry: Some(entry),
        seed,
        pos: start,
        end,
        rate,
        drop_at,
        started: Instant::now(),
        sent: 0,
        dropped: false,
        _slot: slot,
    };
    builder
        .body(Body::from_stream(transfer.into_stream()))
        .unwrap()
}

/// `seed`, `expires` (a signed link's end, unix ms) and the fault overrides.
fn split_query(q: &str) -> Result<(u64, Option<u64>, Faults), String> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(q).map_err(|e| e.to_string())?;
    let mut seed = 0;
    let mut expires = None;
    let mut rest = Vec::new();
    for (k, v) in pairs {
        if k == "seed" {
            seed = v.parse().map_err(|_| format!("invalid seed `{v}`\n"))?;
        } else if k == "expires" {
            expires = Some(v.parse().map_err(|_| format!("invalid expires `{v}`\n"))?);
        } else {
            rest.push((k, v));
        }
    }
    let rest = serde_urlencoded::to_string(rest).map_err(|e| e.to_string())?;
    let faults = Faults::parse(&rest).map_err(|e| format!("{e}\n"))?;
    Ok((seed, expires, faults))
}

/// Streams one response body; logs the request when dropped. The outcome is decided there,
/// because hyper stops polling once Content-Length bytes are out and never sees end-of-stream.
struct Transfer {
    shared: Arc<Shared>,
    entry: Option<LogEntry>,
    seed: u64,
    pos: u64,
    end: u64,
    rate: Option<u64>,
    drop_at: Option<u64>,
    started: Instant,
    sent: u64,
    dropped: bool,
    _slot: ActiveSlot,
}

impl Transfer {
    fn into_stream(self) -> impl futures::Stream<Item = io::Result<Bytes>> + Send + 'static {
        futures::stream::unfold(self, |mut t| async move {
            if t.pos >= t.end {
                return None;
            }
            if t.drop_at.is_some_and(|d| t.sent >= d) {
                // hyper discards unflushed output when a body errors; returning Pending once
                // lets it flush, so the client really receives the bytes sent before the drop.
                tokio::task::yield_now().await;
                t.dropped = true;
                return Some((Err(io::Error::other("fault: drop_after")), t));
            }
            let mut n = CHUNK.min(t.end - t.pos);
            if let Some(r) = t.rate.filter(|r| *r > 0) {
                n = n.min((r / 20).max(1024));
                let due = Duration::from_secs_f64((t.sent + n) as f64 / r as f64);
                tokio::time::sleep_until((t.started + due).into()).await;
            }
            if let Some(d) = t.drop_at {
                n = n.min(d - t.sent);
            }
            let mut buf = vec![0u8; n as usize];
            content::fill(t.seed, t.pos, &mut buf);
            t.pos += n;
            t.sent += n;
            Some((Ok(Bytes::from(buf)), t))
        })
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        if let Some(mut entry) = self.entry.take() {
            entry.bytes_sent = self.sent;
            let outcome = if self.dropped {
                "dropped"
            } else if self.sent == entry.body_len {
                "complete"
            } else {
                "aborted"
            };
            let status = entry.status;
            self.shared.record(entry, status, outcome);
        }
    }
}

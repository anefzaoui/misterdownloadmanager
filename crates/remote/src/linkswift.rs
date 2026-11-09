//! A separate, opt-in loopback endpoint for LinkSwift's Aria2 push commands.

use std::io;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use serde_json::{Value, json};

use crate::tls::PlainListener;

pub const PORT: u16 = 16881;
pub const PATH: &str = "/jsonrpc";
const MAX_BODY: usize = 128 * 1024;

/// How long a taken port waits before the next try (`Bridge::apply`).
const RETRY: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    Listening,
    /// Another program holds the port; it is tried again every [`RETRY`].
    PortInUse,
    Failed,
}

#[derive(Clone)]
pub struct Bridge {
    rt: tokio::runtime::Handle,
    key_path: PathBuf,
    sink: mdm_ipc::Sink,
    port: u16,
    retry: Duration,
    state: Arc<Mutex<Inner>>,
}

struct Inner {
    status: Status,
    running: Option<Running>,
    /// The port was taken: the task trying again.
    retrying: Option<tokio::task::JoinHandle<()>>,
    /// The port was found taken and nobody was told yet ([`Bridge::take_port_notice`]).
    port_notice: bool,
}

struct Running {
    request: Arc<Endpoint>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        *self.request.enabled.lock().unwrap() = false;
        self.task.abort();
    }
}

struct Endpoint {
    port: u16,
    key: Mutex<String>,
    sink: mdm_ipc::Sink,
    enabled: Mutex<bool>,
}

impl Bridge {
    pub fn new(rt: tokio::runtime::Handle, key_path: PathBuf, sink: mdm_ipc::Sink) -> Self {
        Self::on_port(rt, key_path, sink, PORT, RETRY)
    }

    fn on_port(
        rt: tokio::runtime::Handle,
        key_path: PathBuf,
        sink: mdm_ipc::Sink,
        port: u16,
        retry: Duration,
    ) -> Self {
        Self {
            rt,
            key_path,
            sink,
            port,
            retry,
            state: Arc::new(Mutex::new(Inner {
                status: Status::Off,
                running: None,
                retrying: None,
                port_notice: false,
            })),
        }
    }

    /// Started or stopped. A port another program holds is tried again until it is free, or
    /// until this is turned off.
    pub fn apply(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if !enabled {
            if let Some(retrying) = state.retrying.take() {
                retrying.abort();
            }
            state.running = None;
            state.status = Status::Off;
            state.port_notice = false;
        } else if state.retrying.is_none()
            && state
                .running
                .as_ref()
                .is_none_or(|running| running.task.is_finished())
        {
            self.try_start(&mut state);
            if state.status == Status::PortInUse {
                let bridge = self.clone();
                state.retrying = Some(self.rt.spawn(async move {
                    loop {
                        tokio::time::sleep(bridge.retry).await;
                        if bridge.retried() {
                            return;
                        }
                    }
                }));
            }
        }
    }

    /// One more try at a taken port; false while it is still taken.
    fn retried(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.status != Status::PortInUse {
            state.retrying = None;
            return true;
        }
        self.try_start(&mut state);
        if state.status == Status::PortInUse {
            return false;
        }
        state.retrying = None;
        true
    }

    fn try_start(&self, state: &mut Inner) {
        match self.start() {
            Ok(running) => {
                state.running = Some(running);
                state.status = Status::Listening;
                tracing::info!("LinkSwift: listening on port {}", self.port);
            }
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                state.running = None;
                // Once, though it is tried again every few seconds.
                if state.status != Status::PortInUse {
                    tracing::warn!(
                        "LinkSwift: port {} is used by another program; trying again every {} s",
                        self.port,
                        self.retry.as_secs()
                    );
                    state.port_notice = true;
                }
                state.status = Status::PortInUse;
            }
            Err(e) => {
                state.running = None;
                state.status = Status::Failed;
                tracing::warn!("LinkSwift: could not start: {e}");
            }
        }
    }

    pub fn status(&self) -> Status {
        let state = self.state.lock().unwrap();
        if state
            .running
            .as_ref()
            .is_some_and(|running| running.task.is_finished())
        {
            Status::Failed
        } else {
            state.status
        }
    }

    /// True once each time the port is found taken, for the notice saying so.
    pub fn take_port_notice(&self) -> bool {
        std::mem::take(&mut self.state.lock().unwrap().port_notice)
    }

    pub fn key(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        state
            .running
            .as_ref()
            .filter(|running| !running.task.is_finished())
            .map(|running| running.request.key.lock().unwrap().clone())
    }

    /// Replaces the service key: the old one is refused from the next request on.
    pub fn new_key(&self) -> io::Result<()> {
        let key = make_key(&self.key_path)?;
        if let Some(running) = &self.state.lock().unwrap().running {
            *running.request.key.lock().unwrap() = key;
        }
        Ok(())
    }

    fn start(&self) -> io::Result<Running> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, self.port))?;
        listener.set_nonblocking(true)?;
        let key = load_key(&self.key_path)?;
        let _runtime = self.rt.enter();
        let listener = PlainListener::new(
            tokio::net::TcpListener::from_std(listener)?,
            crate::tls::MAX_CONNECTIONS,
            crate::tls::IDLE,
        );
        let request = Arc::new(Endpoint {
            port: self.port,
            key: Mutex::new(key),
            sink: self.sink.clone(),
            enabled: Mutex::new(true),
        });
        let app = router(request.clone());
        let task = self.rt.spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Running { request, task })
    }
}

fn valid_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A new key, saved where only this user can read it.
fn make_key(path: &Path) -> io::Result<String> {
    let key = hex::encode(crate::devices::random(32));
    mdm_ipc::private_write(path, key.as_bytes())?;
    Ok(key)
}

/// The kept key; a new one when there is none or the file is damaged (the person only has to
/// paste the new one into LinkSwift, there is nothing else to lose).
fn load_key(path: &Path) -> io::Result<String> {
    let kept = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    match kept {
        None => make_key(path),
        Some(bytes) => {
            let key = String::from_utf8_lossy(&bytes);
            let key = key.trim();
            if valid_key(key) {
                Ok(key.to_owned())
            } else {
                tracing::warn!("LinkSwift: the service key file was damaged; made a new key");
                make_key(path)
            }
        }
    }
}

fn router(state: Arc<Endpoint>) -> Router {
    Router::new().route(PATH, post(handle)).with_state(state)
}

async fn handle(State(state): State<Arc<Endpoint>>, request: Request) -> Response {
    // A browser using an arbitrary name that resolves to loopback cannot access this endpoint.
    let hosts = request.headers().get_all(header::HOST);
    let mut values = hosts.iter();
    let host = values.next().and_then(|value| value.to_str().ok());
    if values.next().is_some()
        || !host.is_some_and(|host| {
            host == format!("127.0.0.1:{}", state.port)
                || host.eq_ignore_ascii_case(&format!("localhost:{}", state.port))
        })
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // A web page is refused even if it has the key; the script manager LinkSwift runs in isn't.
    let own = [
        format!("http://127.0.0.1:{}", state.port),
        format!("http://localhost:{}", state.port),
    ];
    if !crate::http::own_origin(request.headers(), &own) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let body = match tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), MAX_BODY),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let (id, result) = match serde_json::from_slice::<Value>(&body) {
        Ok(body) => {
            let id = body
                .get("id")
                .filter(|value| value.is_number() || value.is_string())
                .cloned()
                .unwrap_or(Value::Null);
            (id, dispatch(&state, &body))
        }
        Err(_) => (Value::Null, Err((-32700, "Invalid JSON"))),
    };
    let answer = match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        }
    };
    ([(header::CACHE_CONTROL, "no-store")], Json(answer)).into_response()
}

type RpcError = (i32, &'static str);

fn dispatch(state: &Endpoint, request: &Value) -> Result<Value, RpcError> {
    if request["jsonrpc"] != "2.0" {
        return Err((-32600, "Expected a JSON-RPC 2.0 request"));
    }
    let params = request["params"]
        .as_array()
        .ok_or((-32001, "RPC secret required"))?;
    let (secret, args) = params
        .split_first()
        .ok_or((-32001, "RPC secret required"))?;
    let secret = secret
        .as_str()
        .and_then(|s| s.strip_prefix("token:"))
        .ok_or((-32001, "RPC secret required"))?
        .trim();
    let key = state.key.lock().unwrap().clone();
    if secret.len() != key.len()
        || secret
            .bytes()
            .zip(key.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            != 0
    {
        return Err((-32001, "Incorrect RPC secret"));
    }
    if !*state.enabled.lock().unwrap() {
        return Err((-32002, "Local RPC disabled"));
    }
    match request["method"].as_str() {
        Some("aria2.getVersion") if args.is_empty() => {
            Ok(json!({"version": env!("CARGO_PKG_VERSION"), "enabledFeatures": []}))
        }
        Some("aria2.addUri") => {
            let command = add_command(args)?;
            if !state.sink.submit(command) {
                return Err((-32002, "MDM is closing"));
            }
            // This is an acknowledgement for LinkSwift; the desktop command has no task ID.
            Ok(json!(hex::encode(crate::devices::random(8))))
        }
        _ => Err((-32601, "Method not supported")),
    }
}

fn add_command(args: &[Value]) -> Result<mdm_ipc::Command, RpcError> {
    let [urls, options] = args else {
        return Err((-32602, "Expected a URL list and options"));
    };
    let [url] = urls.as_array().map(Vec::as_slice).unwrap_or_default() else {
        return Err((-32602, "Expected one download URL"));
    };
    let Some(address) = url.as_str().filter(|s| is_http_url(s)) else {
        return Err((-32602, "Expected an HTTP(S) download URL"));
    };
    let options = options
        .as_object()
        .ok_or((-32602, "Expected an options object"))?;
    if options
        .get("dir")
        .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
    {
        return Err((
            -32602,
            "Leave the directory blank to use MDM's save location",
        ));
    }
    let mut command = mdm_ipc::Command {
        url: Some(address.to_owned()),
        silent: true,
        no_window: true,
        ..Default::default()
    };
    if let Some(name) = options.get("out").filter(|v| !v.is_null()) {
        let name = name.as_str().ok_or((-32602, "Invalid file name"))?;
        if name.trim().is_empty()
            || matches!(name, "." | "..")
            || name.contains(['/', '\\'])
            || name.chars().any(char::is_control)
        {
            return Err((-32602, "Expected one file name"));
        }
        command.name = Some(name.to_owned());
    }
    if let Some(headers) = options.get("header").filter(|v| !v.is_null()) {
        let headers = headers
            .as_array()
            .ok_or((-32602, "Expected a list of headers"))?;
        for header in headers {
            let (name, value) = header
                .as_str()
                .and_then(|s| s.split_once(':'))
                .ok_or((-32602, "Invalid request header"))?;
            let value = value.trim_matches([' ', '\t']);
            if value.chars().any(char::is_control) {
                return Err((-32602, "Invalid request header value"));
            }
            match name.trim().to_ascii_lowercase().as_str() {
                "cookie" if !value.is_empty() => match command.cookies.as_mut() {
                    Some(cookies) => {
                        cookies.push_str("; ");
                        cookies.push_str(value);
                    }
                    None => command.cookies = Some(value.to_owned()),
                },
                "referer" if !value.is_empty() => command.referer = Some(value.to_owned()),
                "user-agent" if !value.is_empty() => command.user_agent = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    for (name, field) in [
        ("referer", &mut command.referer),
        ("user-agent", &mut command.user_agent),
    ] {
        if field.is_none()
            && let Some(option) = options.get(name).filter(|v| !v.is_null())
        {
            let value = option
                .as_str()
                .filter(|s| !s.chars().any(char::is_control))
                .ok_or((-32602, "Invalid request header value"))?;
            if !value.is_empty() {
                *field = Some(value.to_owned());
            }
        }
    }
    if command.referer.as_deref().is_some_and(|s| !is_http_url(s)) {
        return Err((-32602, "Invalid Referer"));
    }
    Ok(command)
}

fn is_http_url(value: &str) -> bool {
    !value.chars().any(char::is_control)
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn link_swift_can_check_and_submit_a_browser_download() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let key = "a".repeat(64);
        let endpoint = Arc::new(Endpoint {
            port: addr.port(),
            key: Mutex::new(key.clone()),
            sink: mdm_ipc::Sink::new(tx, Arc::new(|| {})),
            enabled: Mutex::new(true),
        });
        let app = router(endpoint.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{addr}{PATH}");
        let request = |host: String, method: &str, params: Value| {
            // LinkSwift 1.1.3 can send JSON as a text body without a JSON content type.
            client.post(&url).header(header::HOST, host).body(
                json!({"jsonrpc": "2.0", "id": 42, "method": method, "params": params}).to_string(),
            )
        };
        let host = format!("localhost:{}", addr.port());
        let token = format!("token:{key}");

        let invalid_host = request("untrusted.test".into(), "aria2.getVersion", json!([token]))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_host.status(), StatusCode::FORBIDDEN);
        let denied: Value = request(host.clone(), "aria2.getVersion", json!(["token:"]))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(denied["error"]["code"], -32001);
        let version = request(host.clone(), "aria2.getVersion", json!([token]))
            .send()
            .await
            .unwrap();
        assert!(
            !version
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
        let version: Value = version.json().await.unwrap();
        assert_eq!(version["id"], 42);
        assert!(version["result"]["version"].is_string());

        let options = json!({
            "out": "测试.txt",
            "header": ["cOoKiE: a=b", "Cookie: c=d", "User-Agent: LinkSwift/1.1.4"],
            "referer": "https://example.test/share?id=1"
        });
        for path in ["first", "second"] {
            let answer: Value = request(
                host.clone(),
                "aria2.addUri",
                json!([token, [format!("https://example.test/{path}")], options]),
            )
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
            assert_eq!(answer["result"].as_str().unwrap().len(), 16);
            let command = rx.try_recv().unwrap().cmd;
            assert!(command.silent);
            assert!(command.no_window, "a push opens no window");
            assert_eq!(command.url.unwrap(), format!("https://example.test/{path}"));
            assert_eq!(command.name.as_deref(), Some("测试.txt"));
            assert_eq!(command.cookies.as_deref(), Some("a=b; c=d"));
            assert_eq!(
                command.referer.as_deref(),
                Some("https://example.test/share?id=1")
            );
            assert_eq!(command.user_agent.as_deref(), Some("LinkSwift/1.1.4"));
        }
        let denied: Value = request(
            host.clone(),
            "aria2.addUri",
            json!([token, ["https://example.test/file"], {"dir": "/tmp"}]),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(denied["error"]["code"], -32602);
        *endpoint.enabled.lock().unwrap() = false;
        let disabled: Value = request(host, "aria2.getVersion", json!([token]))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(disabled["error"]["code"], -32002);
        assert!(rx.try_recv().is_err());
        task.abort();
    }

    /// `Host`, `Origin` and `Sec-Fetch-Site` as a browser's page or extension would send them.
    async fn answer(
        client: &reqwest::Client,
        addr: std::net::SocketAddr,
        origin: Option<&str>,
        fetch_site: Option<&str>,
        token: &str,
    ) -> reqwest::Response {
        let mut request = client
            .post(format!("http://{addr}{PATH}"))
            .header(header::HOST, format!("127.0.0.1:{}", addr.port()))
            .body(
                json!({"jsonrpc": "2.0", "id": 1, "method": "aria2.getVersion", "params": [token]})
                    .to_string(),
            );
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if let Some(fetch_site) = fetch_site {
            request = request.header("sec-fetch-site", fetch_site);
        }
        request.send().await.unwrap()
    }

    fn test_sink() -> (mdm_ipc::Sink, std::sync::mpsc::Receiver<mdm_ipc::Incoming>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (mdm_ipc::Sink::new(tx, Arc::new(|| {})), rx)
    }

    #[tokio::test]
    async fn web_pages_are_refused_and_script_managers_are_not() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let (sink, _rx) = test_sink();
        let key = "b".repeat(64);
        let endpoint = Arc::new(Endpoint {
            port: addr.port(),
            key: Mutex::new(key.clone()),
            sink,
            enabled: Mutex::new(true),
        });
        let task = tokio::spawn(async move {
            axum::serve(listener, router(endpoint)).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let token = format!("token:{key}");
        // With the right key, none of these may be served.
        for (origin, fetch_site) in [
            (Some("https://example.test"), None),
            (Some("http://example.test"), None),
            (Some("http://127.0.0.1:1"), None),
            (Some("null"), None),
            (None, Some("cross-site")),
            (Some("https://example.test"), Some("cross-site")),
        ] {
            let got = answer(&client, addr, origin, fetch_site, &token).await;
            assert_eq!(
                got.status(),
                StatusCode::FORBIDDEN,
                "{origin:?} {fetch_site:?}"
            );
        }
        // Extensions, script managers and programs that are no browser.
        let own = format!("http://localhost:{}", addr.port());
        for (origin, fetch_site) in [
            (None, None),
            (None, Some("none")),
            (Some(own.as_str()), None),
            (Some("chrome-extension://abcdefghijklmnop"), Some("none")),
            (
                Some("chrome-extension://abcdefghijklmnop"),
                Some("cross-site"),
            ),
            (Some("moz-extension://0a1b2c3d"), None),
            (Some("safari-web-extension://0a1b2c3d"), None),
        ] {
            let got = answer(&client, addr, origin, fetch_site, &token).await;
            assert_eq!(got.status(), StatusCode::OK, "{origin:?} {fetch_site:?}");
            let body: Value = got.json().await.unwrap();
            assert!(body["result"]["version"].is_string());
        }
        task.abort();
    }

    #[test]
    fn a_damaged_key_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let first = load_key(&path).unwrap();
        assert!(valid_key(&first));
        assert_eq!(load_key(&path).unwrap(), first, "the same key each start");
        for damaged in [&b"short"[..], &[0xff, 0xfe, 0x00, 0xc3, 0x28], &[b'g'; 64]] {
            std::fs::write(&path, damaged).unwrap();
            let new = load_key(&path).unwrap();
            assert!(valid_key(&new), "a damaged file makes a usable key");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), new);
        }
    }

    /// A port free right now on the loopback.
    fn free_port() -> u16 {
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        probe.local_addr().unwrap().port()
    }

    async fn wait_for(bridge: &Bridge, wanted: Status) -> bool {
        for _ in 0..200 {
            if bridge.status() == wanted {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    #[tokio::test]
    async fn a_taken_port_is_said_and_tried_again() {
        let port = free_port();
        let dir = tempfile::tempdir().unwrap();
        let (sink, _rx) = test_sink();
        let bridge = Bridge::on_port(
            tokio::runtime::Handle::current(),
            dir.path().join("key"),
            sink,
            port,
            Duration::from_millis(50),
        );
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
        bridge.apply(true);
        assert_eq!(bridge.status(), Status::PortInUse);
        assert!(bridge.key().is_none());
        assert!(bridge.take_port_notice(), "told once");
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(bridge.status(), Status::PortInUse, "still taken");
        assert!(!bridge.take_port_notice(), "not told again");

        drop(held);
        assert!(wait_for(&bridge, Status::Listening).await);
        assert!(bridge.key().is_some());

        // Turned off while it waits for the port: it doesn't start after all.
        bridge.apply(false);
        assert!(wait_for(&bridge, Status::Off).await);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, port));
        let held = match held {
            Ok(held) => held,
            // The task that listened is aborted a moment later than the status changes.
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap()
            }
        };
        bridge.apply(true);
        assert_eq!(bridge.status(), Status::PortInUse);
        bridge.apply(false);
        drop(held);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(bridge.status(), Status::Off, "off stops the retries");
        assert!(bridge.key().is_none());
    }

    #[tokio::test]
    async fn a_new_key_replaces_the_old_one_at_once() {
        let port = free_port();
        let dir = tempfile::tempdir().unwrap();
        let (sink, _rx) = test_sink();
        let bridge = Bridge::on_port(
            tokio::runtime::Handle::current(),
            dir.path().join("key"),
            sink,
            port,
            Duration::from_millis(50),
        );
        bridge.apply(true);
        assert_eq!(bridge.status(), Status::Listening);
        let old = bridge.key().unwrap();
        bridge.new_key().unwrap();
        let new = bridge.key().unwrap();
        assert_ne!(old, new);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("key")).unwrap(),
            new
        );

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let addr = (Ipv4Addr::LOCALHOST, port).into();
        let refused: Value = answer(&client, addr, None, None, &format!("token:{old}"))
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(refused["error"]["code"], -32001);
        let accepted: Value = answer(&client, addr, None, None, &format!("token:{new}"))
            .await
            .json()
            .await
            .unwrap();
        assert!(accepted["result"]["version"].is_string());
        bridge.apply(false);
    }
}

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

pub const PORT: u16 = 16881;
pub const PATH: &str = "/jsonrpc";
const MAX_BODY: usize = 128 * 1024;

#[derive(Clone, Copy)]
pub enum Status {
    Off,
    Listening,
    PortInUse,
    Failed,
}

#[derive(Clone)]
pub struct Bridge {
    rt: tokio::runtime::Handle,
    key_path: PathBuf,
    sink: mdm_ipc::Sink,
    state: Arc<Mutex<(Status, Option<Running>)>>,
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
    key: String,
    sink: mdm_ipc::Sink,
    enabled: Mutex<bool>,
}

impl Bridge {
    pub fn new(rt: tokio::runtime::Handle, key_path: PathBuf, sink: mdm_ipc::Sink) -> Self {
        Self {
            rt,
            key_path,
            sink,
            state: Arc::new(Mutex::new((Status::Off, None))),
        }
    }

    pub fn apply(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if !enabled {
            *state = (Status::Off, None);
        } else if state
            .1
            .as_ref()
            .is_none_or(|running| running.task.is_finished())
        {
            *state = match self.start() {
                Ok(running) => (Status::Listening, Some(running)),
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => (Status::PortInUse, None),
                Err(_) => (Status::Failed, None),
            };
        }
    }

    pub fn status(&self) -> Status {
        let state = self.state.lock().unwrap();
        if state
            .1
            .as_ref()
            .is_some_and(|running| running.task.is_finished())
        {
            Status::Failed
        } else {
            state.0
        }
    }

    pub fn key(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        state
            .1
            .as_ref()
            .filter(|running| !running.task.is_finished())
            .map(|running| running.request.key.clone())
    }

    fn start(&self) -> io::Result<Running> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, PORT))?;
        listener.set_nonblocking(true)?;
        let key = load_key(&self.key_path)?;
        let _runtime = self.rt.enter();
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let request = Arc::new(Endpoint {
            port: PORT,
            key,
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

fn load_key(path: &Path) -> io::Result<String> {
    let key = match std::fs::read_to_string(path) {
        Ok(key) => key.trim().to_owned(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let key = hex::encode(crate::devices::random(32));
            mdm_ipc::private_write(path, key.as_bytes())?;
            key
        }
        Err(e) => return Err(e),
    };
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid RPC key",
        ));
    }
    Ok(key)
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
    if secret.len() != state.key.len()
        || secret
            .bytes()
            .zip(state.key.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            != 0
    {
        return Err((-32001, "Incorrect RPC secret"));
    }
    let enabled = state.enabled.lock().unwrap();
    if !*enabled {
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
            key: key.clone(),
            sink: mdm_ipc::Sink::new(tx, || {}),
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
}

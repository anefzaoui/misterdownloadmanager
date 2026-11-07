//! Fake torrents at the three fake services (#44): a magnet link sent to the account is "fetched"
//! over a few polls, then its files are there, each a link the service hands back. Answers as each
//! service's public API documentation shows them (paths, JSON shapes, status words and codes);
//! nothing is copied from the services.
//!
//! - The torrent's name is the magnet's `dn` (else "torrent"). It holds two files:
//!   `<name>/video.mkv` (2 MB) and `<name>/extras/notes.txt` (64 KB).
//! - Its info hash (`xt=urn:btih:…`) says how it goes: `dead…` dies at the service; `bad…` gives
//!   a file whose path climbs out of the folder (`../../escape.bin`); anything else is ready
//!   after two status polls.
//! - Real-Debrid's and AllDebrid's file links are the fake host's (`http://hoster.test/file/…`),
//!   unrestricted as any link; Premiumize's are the lab's own direct links (`/gen/…`).
//! - `GET /_debrid/torrents` lists what the accounts still hold (a test checks the cleanup).

use std::collections::HashMap;
use std::sync::Mutex;

use axum::extract::{Path, Query};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use serde_json::{Value, json};

use crate::debrid::{HOSTER, seed};

struct Torrent {
    service: &'static str,
    name: String,
    hash: String,
    polls: u32,
    selected: bool,
}

static TORRENTS: Mutex<Option<HashMap<String, Torrent>>> = Mutex::new(None);
static NEXT: Mutex<u64> = Mutex::new(1);

fn with<T>(f: impl FnOnce(&mut HashMap<String, Torrent>) -> T) -> T {
    f(TORRENTS.lock().unwrap().get_or_insert_with(HashMap::new))
}

/// The files of a torrent: (path in it, size text, bytes).
fn files(t: &Torrent) -> Vec<(String, &'static str, u64)> {
    let mut v = vec![
        (format!("{}/video.mkv", t.name), "2M", 2 << 20),
        (format!("{}/extras/notes.txt", t.name), "64K", 64 << 10),
    ];
    if t.hash.starts_with("bad") {
        v.push(("../../escape.bin".to_owned(), "1K", 1 << 10));
    }
    v
}

/// The fake host's link for a file: the same bytes on every unlock.
fn host_link(path: &str, size: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("file");
    format!("http://{HOSTER}/file/{size}/{name}")
}

/// The lab's direct link for a file (Premiumize's cloud), the bytes the fake host's would be.
fn direct_link(headers: &HeaderMap, path: &str, size: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("file");
    let lab = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1");
    format!(
        "http://{lab}/gen/{size}/{name}?seed={}",
        seed(&format!("{HOSTER}/file/{size}/{name}"))
    )
}

/// A magnet link's name and hash.
fn magnet(text: &str) -> Option<(String, String)> {
    let url = url::Url::parse(text).ok()?;
    if url.scheme() != "magnet" {
        return None;
    }
    let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let hash = q.get("xt")?.strip_prefix("urn:btih:")?.to_ascii_lowercase();
    Some((
        q.get("dn").cloned().unwrap_or_else(|| "torrent".into()),
        hash,
    ))
}

fn add(service: &'static str, text: &str) -> Option<(String, String, String)> {
    let (name, hash) = magnet(text)?;
    let id = {
        let mut n = NEXT.lock().unwrap();
        *n += 1;
        match service {
            "rd" => format!("RDT{}", *n),
            "ad" => n.to_string(),
            _ => format!("pmt{}", *n),
        }
    };
    with(|m| {
        m.insert(
            id.clone(),
            Torrent {
                service,
                name: name.clone(),
                hash: hash.clone(),
                polls: 0,
                selected: false,
            },
        )
    });
    Some((id, name, hash))
}

/// One poll of a torrent: (still there, its polls so far, dead?).
fn poll(id: &str, service: &str) -> Option<(u32, bool)> {
    with(|m| {
        let t = m.get_mut(id).filter(|t| t.service == service)?;
        t.polls += 1;
        Some((t.polls, t.hash.starts_with("dead")))
    })
}

fn form(body: &Bytes) -> Vec<(String, String)> {
    serde_urlencoded::from_bytes(body).unwrap_or_default()
}

fn field(f: &[(String, String)], name: &str) -> Option<String> {
    f.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

fn token_ok(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| t == "good" || t.starts_with("good-") || t.starts_with("at-"))
}

/// `GET /_debrid/torrents`: the ids the fake accounts still hold.
pub async fn held() -> Response {
    let ids: Vec<String> = with(|m| m.keys().cloned().collect());
    axum::Json(json!(ids)).into_response()
}

// ---- Real-Debrid -----------------------------------------------------------------------------

fn rd_error(status: StatusCode, error: &str, code: i64) -> Response {
    (
        status,
        axum::Json(json!({"error": error, "error_code": code})),
    )
        .into_response()
}

/// `POST /rd/rest/1.0/torrents/addMagnet`.
pub async fn rd_add(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return rd_error(StatusCode::UNAUTHORIZED, "bad_token", 8);
    }
    match field(&form(&body), "magnet").and_then(|m| add("rd", &m)) {
        Some((id, ..)) => (
            StatusCode::CREATED,
            axum::Json(json!({"id": id, "uri": format!("https://api.real-debrid.com/rest/1.0/torrents/info/{id}")})),
        )
            .into_response(),
        None => rd_error(StatusCode::BAD_REQUEST, "wrong_parameter", 2),
    }
}

/// `GET /rd/rest/1.0/torrents/info/{id}`.
pub async fn rd_info(headers: HeaderMap, Path(id): Path<String>) -> Response {
    if !token_ok(&headers) {
        return rd_error(StatusCode::UNAUTHORIZED, "bad_token", 8);
    }
    let Some((polls, dead)) = poll(&id, "rd") else {
        return rd_error(StatusCode::NOT_FOUND, "unknown_ressource", 7);
    };
    let (name, hash, selected, files) = with(|m| {
        let t = &m[&id];
        (t.name.clone(), t.hash.clone(), t.selected, files(t))
    });
    let (status, progress) = if dead {
        ("dead", 0)
    } else if polls == 1 {
        ("magnet_conversion", 0)
    } else if !selected {
        ("waiting_files_selection", 0)
    } else if polls < 4 {
        ("downloading", 50)
    } else {
        ("downloaded", 100)
    };
    let ready = status == "downloaded";
    let list: Vec<Value> = files
        .iter()
        .enumerate()
        .map(|(i, (p, _, b))| json!({"id": i + 1, "path": format!("/{p}"), "bytes": b, "selected": i32::from(selected)}))
        .collect();
    let links: Vec<String> = if ready {
        files.iter().map(|(p, s, _)| host_link(p, s)).collect()
    } else {
        Vec::new()
    };
    let bytes: u64 = files.iter().map(|f| f.2).sum();
    axum::Json(json!({
        "id": id, "filename": name, "original_filename": name, "hash": hash, "bytes": bytes,
        "original_bytes": bytes, "host": "real-debrid.com", "split": 2000, "progress": progress,
        "status": status, "added": "2026-11-06T00:00:00.000Z", "files": list, "links": links,
    }))
    .into_response()
}

/// `POST /rd/rest/1.0/torrents/selectFiles/{id}` (`files`): 204.
pub async fn rd_select(headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return rd_error(StatusCode::UNAUTHORIZED, "bad_token", 8);
    }
    if field(&form(&body), "files").as_deref() != Some("all") {
        return rd_error(StatusCode::BAD_REQUEST, "wrong_parameter", 2);
    }
    match with(|m| m.get_mut(&id).map(|t| t.selected = true)) {
        Some(()) => StatusCode::NO_CONTENT.into_response(),
        None => rd_error(StatusCode::NOT_FOUND, "unknown_ressource", 7),
    }
}

/// `DELETE /rd/rest/1.0/torrents/delete/{id}`: 204.
pub async fn rd_delete(headers: HeaderMap, Path(id): Path<String>) -> Response {
    if !token_ok(&headers) {
        return rd_error(StatusCode::UNAUTHORIZED, "bad_token", 8);
    }
    match with(|m| m.remove(&id)) {
        Some(_) => StatusCode::NO_CONTENT.into_response(),
        None => rd_error(StatusCode::NOT_FOUND, "unknown_ressource", 7),
    }
}

// ---- AllDebrid -------------------------------------------------------------------------------

fn ad_ok(data: Value) -> Response {
    axum::Json(json!({"status": "success", "data": data})).into_response()
}

fn ad_error(code: &str) -> Response {
    axum::Json(
        json!({"status": "error", "error": {"code": code, "message": format!("lab: {code}")}}),
    )
    .into_response()
}

/// `POST /ad/v4/magnet/upload` (`magnets[]`).
pub async fn ad_upload(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return ad_error("AUTH_BAD_APIKEY");
    }
    let f = form(&body);
    let Some(m) = field(&f, "magnets[]") else {
        return ad_error("MAGNET_NO_URI");
    };
    match add("ad", &m) {
        Some((id, name, hash)) => ad_ok(json!({"magnets": [{
            "magnet": m, "hash": hash, "name": name, "size": (2 << 20) + (64 << 10),
            "ready": false, "id": id.parse::<i64>().unwrap_or(0)}]})),
        None => ad_ok(json!({"magnets": [{"magnet": m,
            "error": {"code": "MAGNET_INVALID_URI", "message": "Magnet is not valid"}}]})),
    }
}

/// `POST /ad/v4.1/magnet/status` (`id`).
pub async fn ad_status(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return ad_error("AUTH_BAD_APIKEY");
    }
    let id = field(&form(&body), "id").unwrap_or_default();
    let Some((polls, dead)) = poll(&id, "ad") else {
        return ad_error("MAGNET_INVALID_ID");
    };
    let name = with(|m| m[&id].name.clone());
    let size: u64 = (2 << 20) + (64 << 10);
    let (status, code, done) = if dead {
        ("File not available - no peer", 15, 0)
    } else if polls < 3 {
        ("Downloading", 1, size / 2)
    } else {
        ("Ready", 4, size)
    };
    ad_ok(json!({"magnets": [{
        "id": id.parse::<i64>().unwrap_or(0), "filename": name, "size": size, "status": status,
        "statusCode": code, "downloaded": done, "uploaded": 0, "seeders": 3,
        "downloadSpeed": 0, "uploadSpeed": 0, "uploadDate": 0, "completionDate": 0}]}))
}

/// `POST /ad/v4/magnet/files` (`id[]`): each file a node `{n, s, l}`, each folder `{n, e}`.
pub async fn ad_files(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return ad_error("AUTH_BAD_APIKEY");
    }
    let id = field(&form(&body), "id[]").unwrap_or_default();
    let Some(files) = with(|m| m.get(&id).filter(|t| t.service == "ad").map(files)) else {
        return ad_ok(
            json!({"magnets": [{"id": id, "error": {"code": "MAGNET_INVALID_ID", "message": "lab"}}]}),
        );
    };
    // The tree: the torrent's folder, its files and its "extras" folder.
    let mut root: Vec<Value> = Vec::new();
    for (path, size, bytes) in &files {
        let parts: Vec<&str> = path.split('/').collect();
        let leaf = json!({"n": parts[parts.len() - 1], "s": bytes, "l": host_link(path, size)});
        let mut level = &mut root;
        for dir in &parts[..parts.len() - 1] {
            let at = match level
                .iter()
                .position(|n| n["n"] == *dir && n.get("e").is_some())
            {
                Some(i) => i,
                None => {
                    level.push(json!({"n": dir, "e": []}));
                    level.len() - 1
                }
            };
            level = level[at]["e"].as_array_mut().expect("a folder");
        }
        level.push(leaf);
    }
    ad_ok(json!({"magnets": [{"id": id.parse::<i64>().unwrap_or(0), "files": root}]}))
}

/// `POST /ad/v4/magnet/delete` (`id`).
pub async fn ad_delete(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return ad_error("AUTH_BAD_APIKEY");
    }
    let id = field(&form(&body), "id").unwrap_or_default();
    match with(|m| m.remove(&id)) {
        Some(_) => ad_ok(json!({"message": "Magnet was successfully deleted"})),
        None => ad_error("MAGNET_INVALID_ID"),
    }
}

// ---- Premiumize ------------------------------------------------------------------------------

fn pm_error(code: &str) -> Response {
    axum::Json(json!({"status": "error", "message": format!("lab: {code}"), "code": code}))
        .into_response()
}

/// `POST /pm/api/transfer/create` (`src`).
pub async fn pm_create(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return pm_error("authentication_failed");
    }
    match field(&form(&body), "src").and_then(|m| add("pm", &m)) {
        Some((id, name, _)) => {
            axum::Json(json!({"status": "success", "id": id, "name": name, "type": "torrent"}))
                .into_response()
        }
        None => pm_error("invalid_request"),
    }
}

/// `GET /pm/api/transfer/list`: every transfer, each asked once more.
pub async fn pm_list(headers: HeaderMap) -> Response {
    if !token_ok(&headers) {
        return pm_error("authentication_failed");
    }
    let ids: Vec<String> = with(|m| {
        m.iter()
            .filter(|(_, t)| t.service == "pm")
            .map(|(id, _)| id.clone())
            .collect()
    });
    let transfers: Vec<Value> = ids
        .iter()
        .filter_map(|id| {
            let (polls, dead) = poll(id, "pm")?;
            let name = with(|m| m[id].name.clone());
            let (status, progress, folder) = if dead {
                ("error", 0.0, Value::Null)
            } else if polls < 3 {
                ("running", 0.5, Value::Null)
            } else {
                ("finished", 1.0, json!(format!("f-{id}")))
            };
            Some(
                json!({"id": id, "name": name, "message": if dead { "lab: no peers" } else { "" },
                "status": status, "progress": progress, "folder_id": folder, "file_id": null,
                "created_at": 0}),
            )
        })
        .collect();
    axum::Json(json!({"status": "success", "transfers": transfers})).into_response()
}

/// `GET /pm/api/folder/list?id=`: `f-<transfer>` is the torrent's folder, `f-<transfer>-x` its
/// "extras".
pub async fn pm_folder(headers: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    if !token_ok(&headers) {
        return pm_error("authentication_failed");
    }
    let fid = q.get("id").cloned().unwrap_or_default();
    let (tid, extras) = match fid.strip_prefix("f-") {
        Some(rest) => match rest.strip_suffix("-x") {
            Some(t) => (t.to_owned(), true),
            None => (rest.to_owned(), false),
        },
        None => return pm_error("not_found"),
    };
    let Some(files) = with(|m| m.get(&tid).map(files)) else {
        return pm_error("not_found");
    };
    let mut content = Vec::new();
    for (path, size, bytes) in &files {
        let in_extras = path.contains("/extras/");
        let name = path.rsplit('/').next().unwrap_or("file");
        if in_extras == extras {
            content.push(
                json!({"id": format!("i-{name}"), "name": name, "type": "file",
                "size": bytes, "mime_type": "application/octet-stream", "created_at": 0,
                "link": direct_link(&headers, path, size)}),
            );
        }
    }
    if !extras {
        content.push(json!({"id": format!("f-{tid}-x"), "name": "extras", "type": "folder", "created_at": 0}));
    }
    axum::Json(json!({"status": "success", "name": "x", "parent_id": "root", "folder_id": fid, "content": content}))
        .into_response()
}

/// `POST /pm/api/transfer/delete` (`id`).
pub async fn pm_delete(headers: HeaderMap, body: Bytes) -> Response {
    if !token_ok(&headers) {
        return pm_error("authentication_failed");
    }
    let id = field(&form(&body), "id").unwrap_or_default();
    match with(|m| m.remove(&id)) {
        Some(_) => axum::Json(json!({"status": "success"})).into_response(),
        None => pm_error("not_found"),
    }
}

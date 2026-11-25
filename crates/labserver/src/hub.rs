//! A fake Hugging Face and a fake GitHub API for Link grabber's packages (#1772): made-up
//! repositories and releases listed as the sites' APIs list them, their files the generator's
//! bytes (ranges as `/gen`'s), each large file with its real SHA-256.
//!
//! Hugging Face, at `/hub/hf/` (its API under `api/`):
//! - `GET api/models/lab/tiny` — the repository's information (`sha`, `gated`); `…/revision/<rev>`
//!   the same at a revision (`main`, `v1`, the commit itself; anything else 404)
//! - `GET api/models/lab/tiny/tree/<sha>[/<path>]?recursive=true` — its files and folders,
//!   `PAGE` entries a page with a `Link: <…cursor=N>; rel="next"` to the next, the hostile
//!   paths a hostile API could send among them (`../`, absolute, a drive, `CON`, a backslash)
//! - `lab/gated`: gated, its information and tree only with `Authorization: Bearer hf_lab`
//!   (else 401, as for a private repository), its files too
//! - `datasets/lab/rows`: a dataset of two files in `data/`
//! - `GET <repo>/resolve/<sha>/<path>` — a file; `bad/corrupt.bin`'s listed SHA-256 is wrong
//! - `lab/busy`: 429 with Hugging Face's `RateLimit` header
//!
//! GitHub, its API at `/hub/ghapi/` and its site at `/hub/gh/`:
//! - `GET repos/lab/tool/releases/tags/v1.0`, `…/releases/latest` — the release, its assets
//!   with their digests (and a hostile name, an asset still uploading); `…/tags/v0.9` 404 but
//!   `…/git/ref/tags/v0.9` 200: a tag without a release
//! - `repos/busy/*` — 403 with `x-ratelimit-remaining: 0` and a reset a minute on
//! - `GET lab/tool/releases/download/<tag>/<name>`, `…/archive/refs/tags/<tag>.zip|.tar.gz` —
//!   the files

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::Digest as _;

use crate::server::{Shared, serve};

/// Entries a tree page lists.
pub const PAGE: usize = 3;
/// `lab/tiny`'s commit.
pub const SHA: &str = "1772177217721772177217721772177217721772";
/// The token `lab/gated` takes.
pub const TOKEN: &str = "hf_lab";

/// `lab/tiny`'s files (path, size) as its tree lists them; `bad/corrupt.bin`'s digest is wrong.
pub const TINY: &[(&str, u64)] = &[
    ("config.json", 1 << 10),
    ("model.safetensors", 300 << 10),
    ("onnx/model.onnx", 200 << 10),
    ("onnx/deep/extra.bin", 64 << 10),
    ("bad/corrupt.bin", 32 << 10),
];
/// The hostile paths among `lab/tiny`'s entries: none may be listed or asked for.
pub const HOSTILE: &[&str] = &[
    "../evil.bin",
    "/abs.bin",
    "C:/evil.bin",
    "onnx/../../up.bin",
    "CON/x.bin",
    "a\\b.bin",
];
/// `lab/tool`'s assets at `v1.0` (name, size, with a digest).
pub const TOOL: &[(&str, u64, bool)] = &[
    ("tool-linux.tar.gz", 100 << 10, true),
    ("tool.exe", 50 << 10, true),
    ("notes.txt", 1 << 10, false),
];

/// The bytes the generator makes for `path` (`seed_of`), whole.
pub fn bytes(path: &str, size: u64) -> Vec<u8> {
    let mut buf = vec![0; size as usize];
    crate::content::fill(crate::tree::seed_of(path), 0, &mut buf);
    buf
}

/// `bytes`' SHA-256 in hex.
pub fn sha256(path: &str, size: u64) -> String {
    hex::encode(sha2::Sha256::digest(bytes(path, size)))
}

/// A file's key, what its bytes are made from: `<repo>/<path>`.
fn key(repo: &str, path: &str) -> String {
    format!("{repo}/{path}")
}

fn json(
    shared: &Shared,
    entry: crate::server::LogEntry,
    status: StatusCode,
    body: String,
) -> Response {
    shared.record(entry, status.as_u16(), "complete");
    (
        status,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

/// This server's own address, as the request named it.
fn origin(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| format!("http://{h}"))
        .unwrap_or_default()
}

fn has_token(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == format!("Bearer {TOKEN}"))
}

/// A tree entry: (path, size, directory, its listed SHA-256).
type TreeEntry = (String, u64, bool, Option<String>);

/// A repository's tree entries.
fn tree(repo: &str) -> Option<Vec<TreeEntry>> {
    let files: Vec<(&str, u64)> = match repo {
        "models/lab/tiny" => TINY.to_vec(),
        "models/lab/gated" => vec![("weights.bin", 8 << 10)],
        "datasets/lab/rows" => vec![("data/train.csv", 4 << 10), ("data/test.csv", 2 << 10)],
        _ => return None,
    };
    let mut out = Vec::new();
    let mut dirs = std::collections::BTreeSet::new();
    for (path, _) in &files {
        let mut d = String::new();
        for part in path
            .split('/')
            .rev()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if !d.is_empty() {
                d.push('/');
            }
            d.push_str(part);
            dirs.insert(d.clone());
        }
    }
    for d in dirs {
        out.push((d, 0, true, None));
    }
    let short = repo.split_once('/').map_or(repo, |(_, r)| r);
    for (path, size) in files {
        let digest = if path == "config.json" {
            // Small, kept in git: no SHA-256 of its own.
            None
        } else if path == "bad/corrupt.bin" {
            Some("00".repeat(32))
        } else {
            Some(sha256(&key(short, path), size))
        };
        out.push((path.to_owned(), size, false, digest));
    }
    if repo == "models/lab/tiny" {
        for h in HOSTILE {
            out.push(((*h).to_owned(), 10, false, None));
        }
    }
    Some(out)
}

/// `GET /hub/hf/{*rest}`: the API (`api/…`), else a file.
pub(crate) async fn hf(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    Path(rest): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    match rest.strip_prefix("api/") {
        Some(api) => hf_api(shared, peer, method, api.to_owned(), query, headers).await,
        None => hf_file(shared, peer, method, rest, headers).await,
    }
}

/// The API: `api/{rest}`.
async fn hf_api(
    shared: Arc<Shared>,
    peer: SocketAddr,
    method: Method,
    rest: String,
    query: Option<String>,
    headers: HeaderMap,
) -> Response {
    let entry = shared.begin(
        peer,
        &method,
        format!("/hub/hf/api/{rest}"),
        query.clone(),
        &headers,
        &[],
    );
    let parts: Vec<&str> = rest.split('/').collect();
    let (kind, owner, name, tail) = match parts.as_slice() {
        [kind @ ("models" | "datasets"), owner, name, tail @ ..] => (*kind, *owner, *name, tail),
        _ => {
            return json(
                &shared,
                entry,
                StatusCode::NOT_FOUND,
                r#"{"error":"Not Found"}"#.into(),
            );
        }
    };
    let repo = format!("{kind}/{owner}/{name}");
    if repo == "models/lab/busy" {
        shared.record(entry, 429, "rate-limited");
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [
                ("ratelimit", "\"api\";r=0;t=140"),
                ("ratelimit-policy", "\"fixed window\";\"api\";q=500;w=300"),
            ],
            "Too Many Requests",
        )
            .into_response();
    }
    let Some(entries) = tree(&repo) else {
        return json(
            &shared,
            entry,
            StatusCode::UNAUTHORIZED,
            r#"{"error":"Invalid username or password."}"#.into(),
        );
    };
    let gated = repo == "models/lab/gated";
    if gated && !has_token(&headers) {
        return json(
            &shared,
            entry,
            StatusCode::UNAUTHORIZED,
            r#"{"error":"Invalid username or password."}"#.into(),
        );
    }
    let info = || {
        let gate = if gated { "\"manual\"" } else { "false" };
        format!(r#"{{"id":"{owner}/{name}","private":false,"sha":"{SHA}","gated":{gate}}}"#)
    };
    match tail {
        [] => json(&shared, entry, StatusCode::OK, info()),
        ["revision", rev] if ["main", "v1", SHA].contains(rev) => {
            json(&shared, entry, StatusCode::OK, info())
        }
        ["revision", _] => json(
            &shared,
            entry,
            StatusCode::NOT_FOUND,
            r#"{"error":"Invalid rev id"}"#.into(),
        ),
        ["tree", sha, path @ ..] if *sha == SHA => {
            let within = path.join("/");
            let listed: Vec<_> = entries
                .into_iter()
                .filter(|(p, ..)| within.is_empty() || p.starts_with(&format!("{within}/")))
                .collect();
            let cursor: usize = query
                .as_deref()
                .unwrap_or_default()
                .split('&')
                .find_map(|kv| kv.strip_prefix("cursor="))
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let page: Vec<String> = listed
                .iter()
                .skip(cursor)
                .take(PAGE)
                .map(|(p, size, dir, digest)| {
                    let p = serde_json::to_string(p).unwrap();
                    match (dir, digest) {
                        (true, _) => format!(r#"{{"type":"directory","oid":"0","size":0,"path":{p}}}"#),
                        (false, Some(d)) => format!(
                            r#"{{"type":"file","oid":"1","size":{size},"lfs":{{"oid":"{d}","size":{size},"pointerSize":131}},"path":{p}}}"#
                        ),
                        (false, None) => format!(r#"{{"type":"file","oid":"1","size":{size},"path":{p}}}"#),
                    }
                })
                .collect();
            let body = format!("[{}]", page.join(","));
            let mut entry = entry;
            entry.body_len = body.len() as u64;
            shared.record(entry, 200, "complete");
            let mut resp = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                body,
            )
                .into_response();
            if cursor + PAGE < listed.len() {
                let next = format!(
                    "<{}/hub/hf/api/{rest}?recursive=true&expand=false&cursor={}>; rel=\"next\"",
                    origin(&headers),
                    cursor + PAGE
                );
                resp.headers_mut()
                    .insert(header::LINK, next.parse().unwrap());
            }
            resp
        }
        _ => json(
            &shared,
            entry,
            StatusCode::NOT_FOUND,
            r#"{"error":"Not Found"}"#.into(),
        ),
    }
}

/// A file: `[datasets/]<owner>/<name>/resolve/<sha>/<path>`.
async fn hf_file(
    shared: Arc<Shared>,
    peer: SocketAddr,
    method: Method,
    rest: String,
    headers: HeaderMap,
) -> Response {
    let entry = shared.begin(
        peer,
        &method,
        format!("/hub/hf/{rest}"),
        None,
        &headers,
        &[],
    );
    let (kind, rest_repo) = match rest.strip_prefix("datasets/") {
        Some(r) => ("datasets", r),
        None => ("models", rest.as_str()),
    };
    let parts: Vec<&str> = rest_repo.splitn(5, '/').collect();
    let [owner, name, "resolve", sha, path] = parts.as_slice() else {
        shared.record(entry, 404, "error");
        return (StatusCode::NOT_FOUND, "Entry not found").into_response();
    };
    let repo = format!("{kind}/{owner}/{name}");
    if repo == "models/lab/gated" && !has_token(&headers) {
        shared.record(entry, 401, "error");
        return (
            StatusCode::UNAUTHORIZED,
            "Access to model lab/gated is restricted.",
        )
            .into_response();
    }
    let size = tree(&repo).and_then(|t| {
        t.into_iter()
            .find(|(p, _, dir, _)| !dir && p == path)
            .map(|(_, s, ..)| s)
    });
    match size.filter(|_| *sha == SHA) {
        Some(size) => {
            let seed = crate::tree::seed_of(&key(&format!("{owner}/{name}"), path));
            let file = path.rsplit('/').next().unwrap_or(path);
            serve(
                shared,
                entry,
                &method,
                &size.to_string(),
                Some(file),
                &format!("seed={seed}"),
            )
            .await
        }
        None => {
            shared.record(entry, 404, "error");
            (StatusCode::NOT_FOUND, "Entry not found").into_response()
        }
    }
}

/// `GET /hub/ghapi/{*rest}`.
pub(crate) async fn gh_api(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    Path(rest): Path<String>,
    headers: HeaderMap,
) -> Response {
    let entry = shared.begin(
        peer,
        &method,
        format!("/hub/ghapi/{rest}"),
        None,
        &headers,
        &[],
    );
    let parts: Vec<&str> = rest.split('/').collect();
    let not_found = r#"{"message":"Not Found","status":"404"}"#.to_owned();
    match parts.as_slice() {
        ["repos", "busy", ..] => {
            let reset = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + 60;
            shared.record(entry, 403, "rate-limited");
            (
                StatusCode::FORBIDDEN,
                [
                    ("x-ratelimit-limit", "60".to_owned()),
                    ("x-ratelimit-remaining", "0".to_owned()),
                    ("x-ratelimit-reset", reset.to_string()),
                ],
                r#"{"message":"API rate limit exceeded"}"#,
            )
                .into_response()
        }
        ["repos", "lab", "tool", "releases", "latest"]
        | ["repos", "lab", "tool", "releases", "tags", "v1.0"] => {
            let mut assets: Vec<String> = TOOL
                .iter()
                .map(|(name, size, digest)| {
                    let digest = if *digest {
                        format!(r#""sha256:{}""#, sha256(&key("lab/tool/v1.0", name), *size))
                    } else {
                        "null".to_owned()
                    };
                    format!(
                        r#"{{"name":"{name}","size":{size},"state":"uploaded","digest":{digest},"browser_download_url":"https://evil.example/{name}"}}"#
                    )
                })
                .collect();
            assets.push(r#"{"name":"../evil.exe","size":1,"state":"uploaded","digest":null}"#.into());
            assets.push(r#"{"name":"half.bin","size":1,"state":"starter","digest":null}"#.into());
            let body = format!(r#"{{"id":1,"tag_name":"v1.0","assets":[{}]}}"#, assets.join(","));
            json(&shared, entry, StatusCode::OK, body)
        }
        ["repos", "lab", "tool", "git", "ref", "tags", "v0.9"] => json(
            &shared,
            entry,
            StatusCode::OK,
            r#"{"ref":"refs/tags/v0.9","object":{"sha":"0909090909090909090909090909090909090909","type":"commit"}}"#.into(),
        ),
        _ => json(&shared, entry, StatusCode::NOT_FOUND, not_found),
    }
}

/// `GET /hub/gh/{*rest}`: a release's file, or a tag's source code.
pub(crate) async fn gh_file(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    Path(rest): Path<String>,
    headers: HeaderMap,
) -> Response {
    let entry = shared.begin(
        peer,
        &method,
        format!("/hub/gh/{rest}"),
        None,
        &headers,
        &[],
    );
    let parts: Vec<&str> = rest.split('/').collect();
    let found = match parts.as_slice() {
        ["lab", "tool", "releases", "download", "v1.0", name] => TOOL
            .iter()
            .find(|(n, ..)| n == name)
            .map(|(n, size, _)| (key("lab/tool/v1.0", n), *size, (*n).to_owned())),
        ["lab", "tool", "archive", "refs", "tags", file] => {
            let tag = file
                .strip_suffix(".zip")
                .or_else(|| file.strip_suffix(".tar.gz"));
            tag.filter(|t| ["v1.0", "v0.9"].contains(t)).map(|t| {
                let ext = if file.ends_with(".zip") {
                    "zip"
                } else {
                    "tar.gz"
                };
                (
                    key("lab/tool/src", file),
                    8 << 10,
                    format!("tool-{}.{ext}", t.trim_start_matches('v')),
                )
            })
        }
        _ => None,
    };
    match found {
        Some((key, size, name)) => {
            let seed = crate::tree::seed_of(&key);
            serve(
                shared,
                entry,
                &method,
                &size.to_string(),
                Some(&name),
                &format!("seed={seed}"),
            )
            .await
        }
        None => {
            shared.record(entry, 404, "error");
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }
}

//! Packages for Link grabber (#1772): a Hugging Face model or dataset (the whole repository, or
//! one folder of it, at a branch, tag or commit) and a GitHub release (or a tag without one),
//! listed through the sites' public APIs, each file with its size, the folders it is in and the
//! digest the site publishes for it (Hugging Face's SHA-256 of a large file, GitHub's of a
//! release file), which the finished download is checked against.
//!
//! What an API sends is the site's word, read defensively: a file's path must be a relative path
//! of plain names (no `..`, `.`, empty names, `\`, drive letters or device names such as `CON`),
//! matched by its names against the folder asked, so nothing lands outside the folder chosen; a
//! file's address is built here from the repository and its names, never taken from the answer;
//! the next page of a listing must be the same API's; answers are read up to `ANSWER_MAX`, at
//! most `MAX_PAGES` pages and `MAX_FILES` files.

use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;
use url::Url;

use crate::download::{Error, Options, http_client};
use crate::verify::{Algo, Hash};

/// The most files a package lists.
pub const MAX_FILES: usize = crate::folder::MAX_FILES;
/// The most pages of one listing asked for (Hugging Face sends 1,000 entries a page).
const MAX_PAGES: usize = 50;
/// The most of one API answer read.
const ANSWER_MAX: usize = 16 << 20;
/// The deepest path a file may have, and its longest.
const MAX_COMPONENTS: usize = 64;
const MAX_PATH: usize = 4096;

/// The site a package comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    HuggingFace,
    GitHub,
}

impl Site {
    /// Its name, as the site writes it (not translated).
    pub fn name(self) -> &'static str {
        match self {
            Site::HuggingFace => "Hugging Face",
            Site::GitHub => "GitHub",
        }
    }
}

/// A Hugging Face repository's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Model,
    Dataset,
}

/// What an address names, as Link grabber reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A Hugging Face repository, or the folder `path` in it, at `revision` (a branch, tag or
    /// commit; None: the repository's default branch).
    HuggingFace {
        kind: Kind,
        owner: String,
        name: String,
        revision: Option<String>,
        path: Vec<String>,
    },
    /// A GitHub release by its tag (None: the latest release).
    GitHub {
        owner: String,
        repo: String,
        tag: Option<String>,
    },
}

/// The first part of a Hugging Face address that is the site's own page, not a model's owner.
const HF_PAGES: &[&str] = &[
    "api",
    "avatars",
    "blog",
    "chat",
    "collections",
    "datasets",
    "docs",
    "enterprise",
    "join",
    "learn",
    "login",
    "logout",
    "models",
    "new",
    "new-dataset",
    "new-space",
    "notifications",
    "oauth",
    "organizations",
    "papers",
    "posts",
    "pricing",
    "search",
    "settings",
    "spaces",
    "tasks",
];

impl Source {
    /// What `url` names: a Hugging Face model's or dataset's page (`huggingface.co/<owner>/
    /// <name>`, `…/datasets/<owner>/<name>`), or a folder's (`…/tree/<revision>/<path>`), a
    /// GitHub release's or tag's page (`github.com/<owner>/<repo>/releases/tag/<tag>`,
    /// `…/releases/latest`); None for anything else (a file's own address among them: it is
    /// downloaded as any file).
    pub fn of(url: &Url) -> Option<Source> {
        if !matches!(url.scheme(), "https" | "http")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return None;
        }
        let host = url.host_str()?.to_ascii_lowercase();
        let mut parts: Vec<String> = url
            .path_segments()?
            .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
            .collect();
        while parts.last().is_some_and(String::is_empty) {
            parts.pop();
        }
        match host.as_str() {
            "huggingface.co" | "www.huggingface.co" | "hf.co" => hugging_face(&parts),
            "github.com" | "www.github.com" => github(&parts),
            _ => None,
        }
    }

    pub fn site(&self) -> Site {
        match self {
            Source::HuggingFace { .. } => Site::HuggingFace,
            Source::GitHub { .. } => Site::GitHub,
        }
    }
}

/// `parts`, a Hugging Face address's path: a repository's page, or a folder's.
fn hugging_face(parts: &[String]) -> Option<Source> {
    let (kind, rest) = match parts.first().map(String::as_str) {
        Some("datasets") => (Kind::Dataset, &parts[1..]),
        Some(first) if HF_PAGES.contains(&first) => return None,
        _ => (Kind::Model, parts),
    };
    let (owner, name) = (rest.first()?, rest.get(1)?);
    if !repo_name(owner) || !repo_name(name) {
        return None;
    }
    let (revision, path) = match rest.get(2).map(String::as_str) {
        None => (None, Vec::new()),
        Some("tree") => {
            let rev = rest.get(3)?;
            if !revision_name(rev) {
                return None;
            }
            let path = &rest[4..];
            if !path.iter().all(|c| plain_component(c)) {
                return None;
            }
            (Some(rev.clone()), path.to_vec())
        }
        _ => return None,
    };
    Some(Source::HuggingFace {
        kind,
        owner: owner.clone(),
        name: name.clone(),
        revision,
        path,
    })
}

/// `parts`, a GitHub address's path: a release's page.
fn github(parts: &[String]) -> Option<Source> {
    let [owner, repo, releases, rest @ ..] = parts else {
        return None;
    };
    if releases != "releases" || !repo_name(owner) || !repo_name(repo) {
        return None;
    }
    let tag = match rest {
        [latest] if latest == "latest" => None,
        [tag_word, tag @ ..] if tag_word == "tag" && !tag.is_empty() => {
            let tag = tag.join("/");
            if !revision_name(&tag) {
                return None;
            }
            Some(tag)
        }
        _ => return None,
    };
    Some(Source::GitHub {
        owner: owner.clone(),
        repo: repo.clone(),
        tag,
    })
}

/// An owner's or a repository's name as both sites write them: letters, digits, `-`, `_` and
/// `.`, and not `.` or `..`.
fn repo_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !s.trim_matches('.').is_empty()
}

/// A branch, tag or commit as a site's address carries it: no control characters, no `..`
/// as one of its parts (git refuses those), at most 255 bytes.
fn revision_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.chars().any(char::is_control)
        && !s.contains('\\')
        && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

/// One name of a path a site sent, safe to be a file's or a folder's name on every system: not
/// empty, `.` or `..` (also as Windows reads a name, without its trailing dots and spaces), no
/// `/`, `\`, `:` (a drive letter, a stream), no control characters, and not a device's name
/// (`CON`, `nul.txt`, `COM1`).
pub fn plain_component(name: &str) -> bool {
    crate::folder::plain_name(name)
        && name.trim_end_matches(['.', ' ']) == name
        && !name.contains(':')
        && !crate::naming::is_windows_reserved(name)
}

/// A file's path as a site sent it (`onnx/model.onnx`), as its plain names; None when it isn't a
/// relative path of plain names ([`plain_component`]): an absolute path (`/etc/passwd`), a drive
/// (`C:/x`), `..`, a backslash, an empty name (`a//b`), a device's name.
pub fn relative_path(path: &str) -> Option<Vec<String>> {
    if path.is_empty() || path.len() > MAX_PATH {
        return None;
    }
    let parts: Vec<String> = path.split('/').map(str::to_owned).collect();
    (parts.len() <= MAX_COMPONENTS && parts.iter().all(|p| plain_component(p))).then_some(parts)
}

/// Where the APIs and the files are (the sites' own; a test's lab server).
#[derive(Clone, Debug)]
pub struct Apis {
    /// Hugging Face: its API is `<hf>api/…`, its files `<hf><owner>/<name>/resolve/…`.
    pub hf: Url,
    /// GitHub's API (`<gh_api>repos/…`).
    pub gh_api: Url,
    /// GitHub's site, where a release's files are (`<gh>owner/repo/releases/download/…`).
    pub gh: Url,
}

impl Default for Apis {
    fn default() -> Self {
        Apis {
            hf: Url::parse("https://huggingface.co/").unwrap(),
            gh_api: Url::parse("https://api.github.com/").unwrap(),
            gh: Url::parse("https://github.com/").unwrap(),
        }
    }
}

/// One file of a package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub url: Url,
    /// The folders it is in, from the repository's root (a folder asked keeps its place in the
    /// repository), each a plain name ([`plain_component`]).
    pub folders: Vec<String>,
    /// Its name, plain.
    pub name: String,
    pub size: Option<u64>,
    /// The digest the site publishes for it: the finished file must have it.
    pub digest: Option<Hash>,
}

/// A package's files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub site: Site,
    /// Its name: `owner/name`, `owner/repo tag`.
    pub name: String,
    /// The page its files are on (the repository's, the release's): their referer.
    pub page: Url,
    /// Where its files go under the download folder, each a plain name: the address's path
    /// from the site's root, as a site's tree is kept (`owner/name`, `datasets/owner/name`,
    /// `owner/repo/tag`).
    pub folder: Vec<String>,
    pub files: Vec<File>,
    /// More files than were listed (`MAX_FILES`, `MAX_PAGES`).
    pub capped: bool,
    /// Entries left out: a path that isn't a relative path of plain names, or outside the
    /// folder asked.
    pub refused: usize,
    /// A Hugging Face repository whose files need its terms accepted, and an access token.
    pub gated: bool,
}

/// Why a package couldn't be listed.
#[derive(Clone, Debug, thiserror::Error)]
pub enum HubError {
    /// The site's limit of requests without an account is reached: try again in `wait` (when
    /// the site says).
    #[error("{} limits requests without an account", .site.name())]
    Limited { site: Site, wait: Option<Duration> },
    /// No such repository, folder, release or tag, or a private one.
    #[error("not found on {}", .0.name())]
    NotFound(Site),
    /// Another refusal (its HTTP status).
    #[error("{} answered {}", .0.name(), .1)]
    Status(Site, u16),
    /// An answer that isn't what the API sends.
    #[error("{}'s answer couldn't be read", .0.name())]
    Unreadable(Site),
    #[error(transparent)]
    Net(#[from] Error),
}

/// `source`'s files from the sites' own APIs ([`list_from`]).
pub async fn list(
    source: &Source,
    headers: HeaderMap,
    opts: &Options,
) -> Result<Package, HubError> {
    list_from(source, &Apis::default(), headers, opts).await
}

/// `source`'s files from `apis`. `headers` are a download's from the page's site (Sites Logins'
/// Hugging Face token, #1772): only Hugging Face's `Authorization` goes on, to Hugging Face's
/// API; GitHub's is asked without any.
pub async fn list_from(
    source: &Source,
    apis: &Apis,
    headers: HeaderMap,
    opts: &Options,
) -> Result<Package, HubError> {
    match source {
        Source::HuggingFace {
            kind,
            owner,
            name,
            revision,
            path,
        } => {
            let mut h = HeaderMap::new();
            if let Some(auth) = headers.get(header::AUTHORIZATION) {
                h.insert(header::AUTHORIZATION, auth.clone());
            }
            h.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
            let repo = Repo {
                kind: *kind,
                owner,
                name,
            };
            list_hf(apis, &repo, revision.as_deref(), path, &h, opts).await
        }
        Source::GitHub { owner, repo, tag } => {
            let mut h = HeaderMap::new();
            h.insert(
                header::ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            );
            h.insert(
                "x-github-api-version",
                HeaderValue::from_static("2022-11-28"),
            );
            list_gh(apis, owner, repo, tag.as_deref(), &h, opts).await
        }
    }
}

/// An API's answer: its status, headers and body.
struct Answer {
    status: u16,
    headers: HeaderMap,
    body: Vec<u8>,
}

/// `url` asked as a download's look asks (the user's proxy, adapter, DNS and agent), its answer
/// read up to `ANSWER_MAX`. An answer from another server (a redirect elsewhere) is no answer.
async fn ask(
    site: Site,
    url: &Url,
    headers: &HeaderMap,
    opts: &Options,
) -> Result<Answer, HubError> {
    let client = http_client(opts, url)?;
    let mut resp = client
        .get(url.clone())
        .headers(headers.clone())
        .send()
        .await
        .map_err(Error::from)?;
    if !same_origin(resp.url(), url) {
        return Err(HubError::Unreadable(site));
    }
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(Error::from)? {
        if body.len() + chunk.len() > ANSWER_MAX {
            return Err(HubError::Unreadable(site));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Answer {
        status,
        headers,
        body,
    })
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// A refusal as the error it is: the site's limit of requests (429, GitHub's 403 with no
/// requests left), a missing or private repository (404, Hugging Face's 401), another status.
fn refusal(site: Site, a: &Answer) -> HubError {
    let text = |name: &str| a.headers.get(name).and_then(|v| v.to_str().ok());
    let none_left = text("x-ratelimit-remaining").is_some_and(|v| v.trim() == "0");
    if a.status == 429 || (a.status == 403 && none_left) {
        return HubError::Limited {
            site,
            wait: wait_of(&a.headers, std::time::SystemTime::now()),
        };
    }
    match a.status {
        404 => HubError::NotFound(site),
        // Hugging Face's answer for a repository that isn't there, or is private.
        401 if site == Site::HuggingFace => HubError::NotFound(site),
        s => HubError::Status(site, s),
    }
}

/// How long a site asks to wait before its limit lets requests through again: `Retry-After`,
/// GitHub's `x-ratelimit-reset` (when, in seconds since 1970), Hugging Face's `RateLimit`
/// (`"api";r=0;t=140`: `t` seconds).
fn wait_of(h: &HeaderMap, now: std::time::SystemTime) -> Option<Duration> {
    if let Some(after) = crate::download::retry_after(h) {
        return Some(after);
    }
    let text = |name: &str| h.get(name).and_then(|v| v.to_str().ok());
    if let Some(reset) = text("x-ratelimit-reset").and_then(|v| v.trim().parse::<u64>().ok()) {
        let at = std::time::UNIX_EPOCH + Duration::from_secs(reset);
        return Some(at.duration_since(now).unwrap_or_default());
    }
    text("ratelimit")?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("t="))
        .and_then(|t| t.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// A path segment as an address writes it (unreserved characters as they are).
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn segment(s: &str) -> String {
    utf8_percent_encode(s, SEGMENT).to_string()
}

/// `names` as an address's path (each one encoded, joined by `/`).
fn segments<S: AsRef<str>>(names: &[S]) -> String {
    names
        .iter()
        .map(|n| segment(n.as_ref()))
        .collect::<Vec<_>>()
        .join("/")
}

/// `base` joined with a path written here; a path that doesn't join is unreadable.
fn at(site: Site, base: &Url, path: &str) -> Result<Url, HubError> {
    base.join(path).map_err(|_| HubError::Unreadable(site))
}

/// The next page of a listing (`Link: <…>; rel="next"`), when it is the same API's.
fn next_page(h: &HeaderMap, api: &Url) -> Option<Url> {
    let link = h.get(header::LINK)?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (target, params) = part.split_once(';')?;
        let target = target.trim().strip_prefix('<')?.strip_suffix('>')?;
        let next = params
            .split(';')
            .any(|p| p.trim().eq_ignore_ascii_case("rel=\"next\"") || p.trim() == "rel=next");
        let url = Url::parse(target).ok()?;
        (next && same_origin(&url, api) && url.path().starts_with(api.path())).then_some(url)
    })
}

/// A Hugging Face repository.
struct Repo<'a> {
    kind: Kind,
    owner: &'a str,
    name: &'a str,
}

impl Repo<'_> {
    /// Its part of an address: `owner/name`, `datasets/owner/name`.
    fn path(&self) -> String {
        let repo = format!("{}/{}", segment(self.owner), segment(self.name));
        match self.kind {
            Kind::Model => repo,
            Kind::Dataset => format!("datasets/{repo}"),
        }
    }

    /// Its API's address: `api/models/owner/name`, `api/datasets/owner/name`.
    fn api(&self) -> String {
        let repo = format!("{}/{}", segment(self.owner), segment(self.name));
        match self.kind {
            Kind::Model => format!("api/models/{repo}"),
            Kind::Dataset => format!("api/datasets/{repo}"),
        }
    }

    /// Where its files go under the download folder: its address's path.
    fn folder(&self) -> Vec<String> {
        let mut f = Vec::new();
        if self.kind == Kind::Dataset {
            f.push("datasets".to_owned());
        }
        f.push(self.owner.to_owned());
        f.push(self.name.to_owned());
        f
    }
}

/// A repository's information (`api/models/<repo>`, `…/revision/<rev>`): the commit its
/// revision is at, and whether it is gated.
#[derive(Deserialize)]
struct HfInfo {
    sha: Option<String>,
    #[serde(default)]
    gated: serde_json::Value,
}

/// One entry of a repository's tree (`api/models/<repo>/tree/<rev>/<path>?recursive=true`).
#[derive(Deserialize)]
struct HfEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: Option<u64>,
    lfs: Option<HfLfs>,
}

#[derive(Deserialize)]
struct HfLfs {
    oid: String,
}

/// A file of a tree page: its plain names from the repository's root, its size, its SHA-256.
type TreeFile = (Vec<String>, Option<u64>, Option<Hash>);
/// A release's file: its name, size and SHA-256.
type Asset = (String, Option<u64>, Option<Hash>);

/// A commit's name: 40 hex digits.
fn commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The commit and gating of a repository's revision, from its information.
fn hf_info(body: &[u8]) -> Option<(String, bool)> {
    let info: HfInfo = serde_json::from_slice(body).ok()?;
    let sha = info.sha.filter(|s| commit(s))?.to_ascii_lowercase();
    // `false`, or how access is given ("auto", "manual").
    let gated = !matches!(
        info.gated,
        serde_json::Value::Bool(false) | serde_json::Value::Null
    );
    Some((sha, gated))
}

/// A tree page's files under `within` (the folder asked), each with its plain names from the
/// repository's root, its size and its LFS SHA-256, and how many entries were left out.
fn hf_files(body: &[u8], within: &[String]) -> Option<(Vec<TreeFile>, usize)> {
    let entries: Vec<HfEntry> = serde_json::from_slice(body).ok()?;
    let mut files = Vec::new();
    let mut refused = 0;
    for e in entries {
        if e.kind != "file" {
            continue;
        }
        match relative_path(&e.path) {
            Some(parts) if parts.len() > within.len() && parts.starts_with(within) => {
                let digest = e.lfs.and_then(|l| sha256(&l.oid));
                files.push((parts, e.size, digest));
            }
            _ => refused += 1,
        }
    }
    Some((files, refused))
}

/// A SHA-256 written in hex.
fn sha256(hex: &str) -> Option<Hash> {
    Hash::parse(&format!("sha256:{}", hex.trim())).filter(|h| h.algo == Algo::Sha256)
}

async fn list_hf(
    apis: &Apis,
    repo: &Repo<'_>,
    revision: Option<&str>,
    within: &[String],
    headers: &HeaderMap,
    opts: &Options,
) -> Result<Package, HubError> {
    let site = Site::HuggingFace;
    // The revision's commit: every file is then asked at it, so a branch that moves on while
    // they download never mixes two versions of the repository.
    let info = match revision {
        Some(rev) => format!("{}/revision/{}", repo.api(), segment(rev)),
        None => repo.api(),
    };
    let a = ask(site, &at(site, &apis.hf, &info)?, headers, opts).await?;
    if a.status != 200 {
        return Err(refusal(site, &a));
    }
    let (sha, gated) = hf_info(&a.body).ok_or(HubError::Unreadable(site))?;
    let mut tree = at(
        site,
        &apis.hf,
        &format!("{}/tree/{sha}/{}", repo.api(), segments(within)),
    )?;
    tree.set_query(Some("recursive=true&expand=false"));
    let api = at(site, &apis.hf, "api/")?;
    let mut files = Vec::new();
    let (mut refused, mut capped) = (0, false);
    let mut next = Some(tree);
    let mut pages = 0;
    while let Some(url) = next.take() {
        if pages == MAX_PAGES {
            capped = true;
            break;
        }
        pages += 1;
        let a = ask(site, &url, headers, opts).await?;
        if a.status != 200 {
            return Err(refusal(site, &a));
        }
        let (found, left) = hf_files(&a.body, within).ok_or(HubError::Unreadable(site))?;
        refused += left;
        for (parts, size, digest) in found {
            if files.len() == MAX_FILES {
                capped = true;
                break;
            }
            let url = at(
                site,
                &apis.hf,
                &format!("{}/resolve/{sha}/{}", repo.path(), segments(&parts)),
            )?;
            let mut parts = parts;
            let name = parts.pop().unwrap_or_default();
            files.push(File {
                url,
                folders: parts,
                name,
                size,
                digest,
            });
        }
        if capped {
            break;
        }
        next = next_page(&a.headers, &api);
    }
    let mut page = format!("{}/", repo.path());
    if let Some(rev) = revision {
        page = format!("{page}tree/{}/{}", segment(rev), segments(within));
    }
    Ok(Package {
        site,
        name: format!("{}/{}", repo.owner, repo.name),
        page: at(site, &apis.hf, &page)?,
        folder: repo.folder(),
        files,
        capped,
        refused,
        gated,
    })
}

/// A release (`repos/<owner>/<repo>/releases/tags/<tag>`, `…/latest`).
#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    size: Option<u64>,
    state: Option<String>,
    digest: Option<String>,
}

/// A release's files: its uploaded assets whose names are plain, each with its size and its
/// SHA-256 when GitHub gives one; and how many were left out.
fn gh_assets(release: &GhRelease) -> (Vec<Asset>, usize) {
    let mut out = Vec::new();
    let mut refused = 0;
    for a in &release.assets {
        if a.state.as_deref().is_some_and(|s| s != "uploaded") {
            continue;
        }
        if !plain_component(&a.name) {
            refused += 1;
            continue;
        }
        let digest = a
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .and_then(sha256);
        out.push((a.name.clone(), a.size, digest));
    }
    (out, refused)
}

/// A tag as one folder's name: its `/` made `_`; `release` when it can't be one.
fn tag_folder(tag: &str) -> String {
    let name = tag.replace(['/', '\\'], "_");
    if plain_component(&name) {
        name
    } else {
        "release".to_owned()
    }
}

async fn list_gh(
    apis: &Apis,
    owner: &str,
    repo: &str,
    tag: Option<&str>,
    headers: &HeaderMap,
    opts: &Options,
) -> Result<Package, HubError> {
    let site = Site::GitHub;
    let base = format!("repos/{}/{}", segment(owner), segment(repo));
    let asked = match tag {
        Some(t) => format!("{base}/releases/tags/{}", segment(t)),
        None => format!("{base}/releases/latest"),
    };
    let a = ask(site, &at(site, &apis.gh_api, &asked)?, headers, opts).await?;
    let (tag, assets, refused) = match (a.status, tag) {
        (200, _) => {
            let release: GhRelease =
                serde_json::from_slice(&a.body).map_err(|_| HubError::Unreadable(site))?;
            if !revision_name(&release.tag_name) || tag.is_some_and(|t| t != release.tag_name) {
                return Err(HubError::Unreadable(site));
            }
            let (assets, refused) = gh_assets(&release);
            (release.tag_name, assets, refused)
        }
        // A tag without a release: its source code only, as its page shows.
        (404, Some(t)) => {
            let ref_url = at(
                site,
                &apis.gh_api,
                &format!("{base}/git/ref/tags/{}", segment(t)),
            )?;
            let r = ask(site, &ref_url, headers, opts).await?;
            if r.status != 200 {
                return Err(refusal(site, &r));
            }
            (t.to_owned(), Vec::new(), 0)
        }
        _ => return Err(refusal(site, &a)),
    };
    let tag_path = segments(&tag.split('/').collect::<Vec<_>>());
    let web = format!("{}/{}", segment(owner), segment(repo));
    let mut files = Vec::new();
    for (name, size, digest) in assets.into_iter().take(MAX_FILES) {
        files.push(File {
            url: at(
                site,
                &apis.gh,
                &format!("{web}/releases/download/{tag_path}/{}", segment(&name)),
            )?,
            folders: Vec::new(),
            name,
            size,
            digest,
        });
    }
    // Its source code, as GitHub's page lists it last: made by GitHub when asked, so neither a
    // size nor a digest is known before.
    for ext in ["zip", "tar.gz"] {
        files.push(File {
            url: at(
                site,
                &apis.gh,
                &format!("{web}/archive/refs/tags/{tag_path}.{ext}"),
            )?,
            folders: Vec::new(),
            name: format!("{repo}-{}.{ext}", tag_folder(&tag)),
            size: None,
            digest: None,
        });
    }
    Ok(Package {
        site,
        name: format!("{owner}/{repo} {tag}"),
        page: at(site, &apis.gh, &format!("{web}/releases/tag/{tag_path}"))?,
        folder: vec![owner.to_owned(), repo.to_owned(), tag_folder(&tag)],
        files,
        capped: false,
        refused,
        gated: false,
    })
}

/// Does `url` take a Hugging Face access token, `Authorization: Bearer` (Sites Logins' password
/// for the site, #1772), rather than a user name and password? Hugging Face's own hosts only.
pub fn takes_token(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|h| matches!(h.to_ascii_lowercase().as_str(), "huggingface.co" | "hf.co"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn hf(kind: Kind, owner: &str, name: &str, rev: Option<&str>, path: &[&str]) -> Source {
        Source::HuggingFace {
            kind,
            owner: owner.into(),
            name: name.into(),
            revision: rev.map(str::to_owned),
            path: path.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    #[test]
    fn hugging_face_addresses() {
        let of = |s: &str| Source::of(&url(s));
        assert_eq!(
            of("https://huggingface.co/hf-internal-testing/tiny-random-bert"),
            Some(hf(
                Kind::Model,
                "hf-internal-testing",
                "tiny-random-bert",
                None,
                &[]
            ))
        );
        assert_eq!(
            of("https://huggingface.co/hf-internal-testing/tiny-random-bert/tree/main/onnx/"),
            Some(hf(
                Kind::Model,
                "hf-internal-testing",
                "tiny-random-bert",
                Some("main"),
                &["onnx"]
            ))
        );
        // A revision with a `/` comes encoded; a commit; a dataset.
        assert_eq!(
            of("https://huggingface.co/datasets/org/set/tree/refs%2Fpr%2F3/data/train"),
            Some(hf(
                Kind::Dataset,
                "org",
                "set",
                Some("refs/pr/3"),
                &["data", "train"]
            ))
        );
        assert_eq!(
            of("https://hf.co/org/m/tree/0123456789abcdef0123456789abcdef01234567"),
            Some(hf(
                Kind::Model,
                "org",
                "m",
                Some("0123456789abcdef0123456789abcdef01234567"),
                &[]
            ))
        );
        // Not packages: a file's own address, the site's pages, a space, a discussion, a
        // folder that leads out of the repository, another port.
        for s in [
            "https://huggingface.co/org/m/resolve/main/model.onnx",
            "https://huggingface.co/org/m/blob/main/README.md",
            "https://huggingface.co/org/m/discussions",
            "https://huggingface.co/spaces/org/app",
            "https://huggingface.co/docs/hub",
            "https://huggingface.co/models",
            "https://huggingface.co/gpt2",
            "https://huggingface.co/org/m/tree/main/..%2F..%2Fx",
            "https://huggingface.co/org/m/tree/main/a%5Cb",
            "https://huggingface.co/org/m/tree/main/CON",
            "https://huggingface.co/org/m/tree/..%2F..",
            "https://huggingface.co:8443/org/m",
            "ftp://huggingface.co/org/m",
            "https://huggingface.co.evil.example/org/m",
        ] {
            assert_eq!(of(s), None, "{s}");
        }
    }

    #[test]
    fn github_addresses() {
        let of = |s: &str| Source::of(&url(s));
        let gh = |tag: Option<&str>| Source::GitHub {
            owner: "anefzaoui".into(),
            repo: "misterdownloadmanager".into(),
            tag: tag.map(str::to_owned),
        };
        assert_eq!(
            of("https://github.com/anefzaoui/misterdownloadmanager/releases/tag/v0.5.2"),
            Some(gh(Some("v0.5.2")))
        );
        assert_eq!(
            of("https://github.com/anefzaoui/misterdownloadmanager/releases/latest"),
            Some(gh(None))
        );
        assert_eq!(
            of("https://github.com/anefzaoui/misterdownloadmanager/releases/tag/app/v1"),
            Some(gh(Some("app/v1")))
        );
        for s in [
            "https://github.com/o/r/releases",
            "https://github.com/o/r/releases/download/v1/file.zip",
            "https://github.com/o/r/tree/v1",
            "https://github.com/o/r/releases/tag/",
            "https://github.com/o/r/releases/tag/a%2F..%2Fb",
            "https://api.github.com/repos/o/r/releases/latest",
        ] {
            assert_eq!(of(s), None, "{s}");
        }
    }

    #[test]
    fn paths_must_stay_inside() {
        assert_eq!(
            relative_path("onnx/model.onnx"),
            Some(vec!["onnx".to_owned(), "model.onnx".to_owned()])
        );
        assert_eq!(relative_path("my file (1).bin").map(|p| p.len()), Some(1));
        for hostile in [
            "",
            "/etc/passwd",
            "../evil.bin",
            "a/../../evil.bin",
            "a/./b",
            "a//b",
            "a/",
            "C:/Windows/evil.dll",
            "C:evil.bin",
            "a\\..\\evil.bin",
            "\\\\server\\share\\x",
            "CON",
            "sub/nul.txt",
            "COM1.log",
            "lpt9",
            "file.txt:stream",
            "trailing.",
            "trailing ",
            "...",
            "a\u{0}b",
            "a\nb",
        ] {
            assert_eq!(relative_path(hostile), None, "{hostile:?}");
        }
        let deep = vec!["d"; MAX_COMPONENTS + 1].join("/");
        assert_eq!(relative_path(&deep), None);
    }

    /// A model's information and whole tree, as Hugging Face answered for a tiny model: every
    /// file from the repository's root, the large ones with their SHA-256, folders left out.
    #[test]
    fn a_hugging_face_repository() {
        let (sha, gated) = hf_info(include_bytes!("../tests/data/hub/hf-model-info.json")).unwrap();
        assert_eq!(sha, "f171d7baecaf37b5da5a3616d8833b9969753535");
        assert!(!gated);
        let (files, refused) =
            hf_files(include_bytes!("../tests/data/hub/hf-model-tree.json"), &[]).unwrap();
        assert_eq!(refused, 0);
        let paths: Vec<String> = files.iter().map(|(p, _, _)| p.join("/")).collect();
        assert!(paths.contains(&"onnx/model.onnx".to_owned()));
        assert!(!paths.contains(&"onnx".to_owned()), "a folder is no file");
        let onnx = files
            .iter()
            .find(|(p, _, _)| p.join("/") == "onnx/model.onnx")
            .unwrap();
        assert_eq!(onnx.1, Some(449_599));
        assert_eq!(
            onnx.2.as_ref().map(Hash::display).as_deref(),
            Some("sha256:083459756ab4f6e37a2ebbd0c9ab4cebbda7638d2015958dd2a7ac9047588f93")
        );
        // A small file kept in git has no SHA-256 (its `oid` is git's own, not the file's).
        let config = files
            .iter()
            .find(|(p, _, _)| p.join("/") == "config.json")
            .unwrap();
        assert_eq!((config.1, &config.2), (Some(548), &None));
    }

    /// A folder asked: only the files inside it, matched by their names, keep their place from
    /// the repository's root; an entry outside it or a hostile path is left out and counted.
    #[test]
    fn a_hugging_face_folder() {
        let body = br#"[
            {"type":"file","path":"onnx/model.onnx","size":3,"lfs":{"oid":"083459756ab4f6e37a2ebbd0c9ab4cebbda7638d2015958dd2a7ac9047588f93","size":3}},
            {"type":"directory","path":"onnx/deep","size":0},
            {"type":"file","path":"onnx/deep/x.bin","size":5},
            {"type":"file","path":"onnxother/y.bin","size":1},
            {"type":"file","path":"config.json","size":1},
            {"type":"file","path":"onnx/../../evil","size":1},
            {"type":"file","path":"/onnx/abs","size":1},
            {"type":"file","path":"onnx/CON","size":1},
            {"type":"file","path":"onnx","size":1}
        ]"#;
        let (files, refused) = hf_files(body, &["onnx".to_owned()]).unwrap();
        let paths: Vec<String> = files.iter().map(|(p, _, _)| p.join("/")).collect();
        assert_eq!(paths, ["onnx/model.onnx", "onnx/deep/x.bin"]);
        assert_eq!(refused, 6);
    }

    /// A revision's information: its commit, and gating as Hugging Face writes it.
    #[test]
    fn a_revision_and_gating() {
        let rev =
            br#"{"id":"org/m","sha":"0123456789ABCDEF0123456789abcdef01234567","gated":"manual"}"#;
        assert_eq!(
            hf_info(rev),
            Some(("0123456789abcdef0123456789abcdef01234567".to_owned(), true))
        );
        // A commit that isn't one is no answer.
        assert_eq!(hf_info(br#"{"sha":"../../x"}"#), None);
        assert_eq!(hf_info(br#"{"sha":null}"#), None);
        assert_eq!(hf_info(b"not json"), None);
    }

    /// Hugging Face's next page, as its answer for a large dataset gave it, followed only when it
    /// is the same API's.
    #[test]
    fn pagination() {
        let api = url("https://huggingface.co/api/");
        let recorded = include_str!("../tests/data/hub/hf-dataset-tree-page1.link");
        let value = recorded.trim().strip_prefix("Link: ").unwrap();
        let mut h = HeaderMap::new();
        h.insert(header::LINK, HeaderValue::from_str(value).unwrap());
        let next = next_page(&h, &api).unwrap();
        assert_eq!(next.path(), "/api/datasets/HuggingFaceFW/fineweb/tree/main");
        assert!(next.query().unwrap().contains("cursor="));
        let (files, refused) = hf_files(
            include_bytes!("../tests/data/hub/hf-dataset-tree-page1.json"),
            &[],
        )
        .unwrap();
        assert_eq!(refused, 0);
        assert_eq!(files.len(), 3);
        assert!(files[2].2.is_some(), "a parquet file's SHA-256");
        for elsewhere in [
            r#"<https://evil.example/api/x?cursor=1>; rel="next""#,
            r#"<https://huggingface.co/datasets/x?cursor=1>; rel="next""#,
            r#"<http://huggingface.co/api/x?cursor=1>; rel="next""#,
            r#"<https://huggingface.co/api/x?cursor=1>; rel="prev""#,
        ] {
            let mut h = HeaderMap::new();
            h.insert(header::LINK, HeaderValue::from_str(elsewhere).unwrap());
            assert_eq!(next_page(&h, &api), None, "{elsewhere}");
        }
    }

    /// A release as GitHub's API answered: its assets with their sizes and SHA-256.
    #[test]
    fn a_github_release() {
        let release: GhRelease =
            serde_json::from_slice(include_bytes!("../tests/data/hub/gh-release.json")).unwrap();
        assert_eq!(release.tag_name, "v0.5.2");
        let (assets, refused) = gh_assets(&release);
        assert_eq!(refused, 0);
        let sums = assets.iter().find(|a| a.0 == "SHA256SUMS.txt").unwrap();
        assert_eq!(sums.1, Some(2357));
        assert_eq!(
            sums.2.as_ref().map(Hash::display).as_deref(),
            Some("sha256:c3ef09de1e2a635694b2ca9774dea53569de8189518e5c653e87fffc9138d948")
        );
        // Hostile or unfinished assets are left out.
        let hostile: GhRelease = serde_json::from_str(
            r#"{"tag_name":"v1","assets":[
                {"name":"ok.zip","size":1,"state":"uploaded","digest":"sha1:abc"},
                {"name":"../evil.exe","size":1,"state":"uploaded"},
                {"name":"C:evil","size":1,"state":"uploaded"},
                {"name":"part.bin","size":1,"state":"starter"}
            ]}"#,
        )
        .unwrap();
        let (assets, refused) = gh_assets(&hostile);
        assert_eq!(assets, [("ok.zip".to_owned(), Some(1), None)]);
        assert_eq!(refused, 2);
    }

    /// A tag's folder name, and a tag that can't be one.
    #[test]
    fn a_tag() {
        assert_eq!(tag_folder("v0.5.2"), "v0.5.2");
        assert_eq!(tag_folder("app/v1"), "app_v1");
        assert_eq!(tag_folder("nul"), "release");
        let recorded = br#"{"ref":"refs/tags/v0.5.2","object":{"sha":"605a0c74e50361afe77bf1721d7172636bd5c57e","type":"commit"}}"#;
        assert!(serde_json::from_slice::<serde_json::Value>(recorded).is_ok());
    }

    #[test]
    fn rate_limits() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut gh = HeaderMap::new();
        gh.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        gh.insert("x-ratelimit-reset", HeaderValue::from_static("1000600"));
        assert_eq!(wait_of(&gh, now), Some(Duration::from_secs(600)));
        let a = Answer {
            status: 403,
            headers: gh,
            body: Vec::new(),
        };
        assert!(matches!(
            refusal(Site::GitHub, &a),
            HubError::Limited {
                site: Site::GitHub,
                wait: Some(_)
            }
        ));
        let mut hf = HeaderMap::new();
        hf.insert("ratelimit", HeaderValue::from_static("\"api\";r=0;t=140"));
        assert_eq!(wait_of(&hf, now), Some(Duration::from_secs(140)));
        // A 403 with requests left is a refusal, not the limit.
        let mut left = HeaderMap::new();
        left.insert("x-ratelimit-remaining", HeaderValue::from_static("12"));
        let a = Answer {
            status: 403,
            headers: left,
            body: Vec::new(),
        };
        assert!(matches!(
            refusal(Site::GitHub, &a),
            HubError::Status(Site::GitHub, 403)
        ));
        let a = Answer {
            status: 401,
            headers: HeaderMap::new(),
            body: Vec::new(),
        };
        assert!(matches!(
            refusal(Site::HuggingFace, &a),
            HubError::NotFound(_)
        ));
    }

    #[test]
    fn tokens_go_to_hugging_face_only() {
        assert!(takes_token(&url("https://huggingface.co/api/models/o/m")));
        assert!(takes_token(&url("https://hf.co/o/m/resolve/main/x")));
        for other in [
            "http://huggingface.co/o/m",
            "https://cdn-lfs.hf.co/x",
            "https://cas-bridge.xethub.hf.co/x",
            "https://huggingface.co.evil.example/x",
            "https://github.com/o/r",
        ] {
            assert!(!takes_token(&url(other)), "{other}");
        }
    }
}

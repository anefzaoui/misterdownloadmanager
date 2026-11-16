//! The Android interface's requests and snapshots (android.rs runs the app there): a child of
//! `app`, so it reaches the app's own state as the desktop windows do. Each request is a JSON
//! object with an `op`; each answer a JSON line (`{"error": …, "code": …}` when it fails).

use std::time::Duration;

use base64::Engine as _;
use mdm_ipc as ipc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::App;
use crate::model::{Category, Item, Status};

/// One download as the interface lists it.
#[derive(Serialize)]
struct Row {
    id: u64,
    file: String,
    url: String,
    path: String,
    /// `Queued`, `Downloading`, `Stopped`, `Complete` or `Error`.
    status: String,
    /// What the engine is doing, while it runs: `Probing`, `Downloading`, `Verifying`,
    /// `Merging`, `Completing`.
    stage: Option<String>,
    size: Option<u64>,
    downloaded: u64,
    /// 0 to 1, when the size is known.
    fraction: Option<f64>,
    speed: u64,
    /// Seconds left at this speed, when it can be said.
    eta: Option<u64>,
    connections: usize,
    category: String,
    /// `file`, `video`, `torrent` or `gallery`.
    kind: &'static str,
    error: Option<String>,
    added: Option<String>,
    resumable: Option<bool>,
    can_start: bool,
    running: bool,
}

#[derive(Serialize)]
struct Snapshot {
    downloads: Vec<Row>,
    speed: u64,
    queues: Vec<ipc::api::QueueInfo>,
    categories: Vec<ipc::api::CategoryInfo>,
    free: Option<u64>,
    /// The interface language's code, and whether it is written right to left.
    language: String,
    rtl: bool,
    speed_limit_on: bool,
    /// A notice for the user (title, text), until `dismiss_notice`.
    notice: Option<(String, String)>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Request {
    op: String,
    urls: Vec<String>,
    url: Option<String>,
    category: Option<String>,
    queue: Option<String>,
    later: bool,
    name: Option<String>,
    dir: Option<String>,
    referer: Option<String>,
    cookies: Option<String>,
    user_agent: Option<String>,
    /// A .torrent file's bytes, base64.
    data: Option<String>,
    action: Option<String>,
    ids: Vec<u64>,
    id: Option<u64>,
    run: Option<bool>,
    patch: Option<Value>,
    token: Option<String>,
    choice: Option<usize>,
    address: Option<String>,
    title: Option<String>,
    texts: Vec<String>,
    code: Option<String>,
    on: Option<bool>,
    kb: Option<u64>,
}

/// Torrents being looked up for the interface's file choice, by token (with whether from a
/// magnet link).
type Looked = std::collections::HashMap<String, (crate::remote::Slot, bool)>;
static TORRENTS: std::sync::Mutex<Option<Looked>> = std::sync::Mutex::new(None);
static TORRENT_TOKENS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn torrents() -> impl std::ops::DerefMut<Target = Looked> {
    struct Guard(std::sync::MutexGuard<'static, Option<Looked>>);
    impl std::ops::Deref for Guard {
        type Target = Looked;
        fn deref(&self) -> &Looked {
            self.0.as_ref().expect("made")
        }
    }
    impl std::ops::DerefMut for Guard {
        fn deref_mut(&mut self) -> &mut Looked {
            self.0.as_mut().expect("made")
        }
    }
    let mut g = TORRENTS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(Default::default);
    Guard(g)
}

/// Playlists being listed for the interface, by token.
static PLAYLISTS: std::sync::Mutex<
    Option<std::collections::HashMap<String, mdm_tools::playlist::Listing>>,
> = std::sync::Mutex::new(None);

fn playlists()
-> impl std::ops::DerefMut<Target = std::collections::HashMap<String, mdm_tools::playlist::Listing>>
{
    struct Guard(
        std::sync::MutexGuard<
            'static,
            Option<std::collections::HashMap<String, mdm_tools::playlist::Listing>>,
        >,
    );
    impl std::ops::Deref for Guard {
        type Target = std::collections::HashMap<String, mdm_tools::playlist::Listing>;
        fn deref(&self) -> &Self::Target {
            self.0.as_ref().expect("made")
        }
    }
    impl std::ops::DerefMut for Guard {
        fn deref_mut(&mut self) -> &mut Self::Target {
            self.0.as_mut().expect("made")
        }
    }
    let mut g = PLAYLISTS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(Default::default);
    Guard(g)
}

/// The interface's sign-in by code under way, and its service.
static SIGNIN: std::sync::Mutex<Option<(mdm_debrid::ServiceKind, crate::debrid::SignIn)>> =
    std::sync::Mutex::new(None);

/// The helper being installed (`ytdlp` or `gallery`, one at a time), and each one's last
/// install's failure: each Settings row says its own.
#[derive(Default)]
struct Install {
    running: Option<&'static str>,
    ytdlp_error: Option<String>,
    gallery_error: Option<String>,
}
static INSTALL: std::sync::Mutex<Install> = std::sync::Mutex::new(Install {
    running: None,
    ytdlp_error: None,
    gallery_error: None,
});

/// Starts installing `helper` unless one is: false when one already runs.
fn install_begin(helper: &'static str) -> bool {
    let mut i = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    if i.running.is_some() {
        return false;
    }
    i.running = Some(helper);
    match helper {
        "ytdlp" => i.ytdlp_error = None,
        _ => i.gallery_error = None,
    }
    true
}

fn install_end(helper: &'static str, error: Option<String>) {
    let mut i = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    i.running = None;
    match helper {
        "ytdlp" => i.ytdlp_error = error,
        _ => i.gallery_error = error,
    }
}

fn line(v: &impl Serialize) -> String {
    ipc::api::line(v)
}

fn fail(code: &str, text: &str) -> String {
    crate::android::error(code, text)
}

fn kind(i: &Item) -> &'static str {
    if i.torrent.is_some() {
        "torrent"
    } else if i.video.is_some() {
        "video"
    } else if i.gallery.is_some() {
        "gallery"
    } else {
        "file"
    }
}

fn row(i: &Item) -> Row {
    let speed = if i.live.is_some() { i.rate() as u64 } else { 0 };
    let eta = match (i.size, speed) {
        (Some(size), s) if s > 0 && size > i.downloaded => Some((size - i.downloaded) / s),
        _ => None,
    };
    let snap = i.snap.as_ref().filter(|_| i.live.is_some());
    Row {
        id: i.id,
        file: i.file_name.clone(),
        url: i.url.to_string(),
        path: i.dest.to_string_lossy().into_owned(),
        status: format!("{:?}", i.status),
        stage: snap.map(|s| format!("{:?}", s.state)),
        size: i.size,
        downloaded: i.downloaded,
        fraction: i.fraction(),
        speed,
        eta,
        connections: snap.map_or(0, |s| s.connections.len()),
        category: i.category.key().into_owned(),
        kind: kind(i),
        error: i.error.clone().filter(|_| i.status == Status::Error),
        added: i.added.clone(),
        resumable: i.resumable,
        can_start: i.can_start(),
        running: i.live.is_some(),
    }
}

impl App {
    /// What the Android core starts (android.rs): headless, with someone to answer.
    pub fn android(mut self) -> Self {
        self.android = true;
        self
    }

    /// The list as the interface shows it, as JSON (sent when it changes).
    pub fn android_snapshot(&mut self) -> String {
        let speed: f64 = self
            .items
            .iter()
            .filter(|i| i.live.is_some())
            .map(|i| i.rate())
            .sum();
        let snapshot = Snapshot {
            downloads: self.items.iter().map(row).collect(),
            speed: speed as u64,
            queues: self
                .queues
                .iter()
                .map(|q| ipc::api::QueueInfo {
                    name: q.name.clone(),
                    running: q.running,
                    ids: q.items.clone(),
                })
                .collect(),
            categories: Category::all()
                .into_iter()
                .map(|c| ipc::api::CategoryInfo {
                    key: c.key().into_owned(),
                    name: c.label().into_owned(),
                })
                .collect(),
            free: self.free_space(),
            language: crate::i18n::code().to_owned(),
            rtl: crate::i18n::rtl(),
            speed_limit_on: self.settings.speed_limit_on,
            notice: self
                .notice
                .as_ref()
                .map(|(title, text)| (crate::i18n::t(title).to_owned(), text.clone())),
        };
        line(&snapshot)
    }

    /// One request of the interface (android.rs `Core::request`).
    pub fn android_request(&mut self, json: &str, ctx: &egui::Context) -> String {
        let req: Request = match serde_json::from_str(json) {
            Ok(r) => r,
            Err(e) => return fail("bad-request", &e.to_string()),
        };
        match req.op.as_str() {
            "snapshot" => self.android_snapshot(),
            "add" => self.android_add(&req),
            "add_torrent_file" => {
                let Some(bytes) = req
                    .data
                    .as_deref()
                    .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
                else {
                    return fail("bad-request", "no torrent data");
                };
                let cmd = ipc::Command {
                    torrent_file: Some(bytes),
                    category: req.category.clone(),
                    queue: req.queue.clone(),
                    later: req.later,
                    ..Default::default()
                };
                // i18n: where a download came from (its Properties): added in MDM's Android app
                self.add_without_window(cmd, crate::i18n::mark("added on this phone"))
            }
            "classify" => {
                let Some(url) = req
                    .url
                    .as_deref()
                    .and_then(|u| url::Url::parse(u.trim()).ok())
                else {
                    return line(&json!({ "kind": "none" }));
                };
                // A DRM streaming service (Spotify, Netflix…): said at once, rather than its page
                // downloaded or yt-dlp asked in vain.
                if let Some(service) = mdm_tools::pages::drm_service(&url) {
                    return line(&json!({
                        "kind": "protected",
                        "message": crate::i18n::fmt(
                            // i18n: Android's Add: the link is on a streaming service that copy-protects its music and videos (DRM); {service} = its name (Spotify, Netflix…)
                            crate::i18n::t("{service} protects its music and videos (DRM): no download manager can save them."),
                            &[("service", service)],
                        ),
                    }));
                }
                let kind = if url.scheme() == "magnet"
                    || crate::torrents::is_torrent_url(&url, None, None)
                {
                    "torrent"
                } else if mdm_tools::pages::is_video_page(&url) {
                    "video"
                } else if mdm_tools::pages::is_gallery_page(&url) {
                    "gallery"
                } else if mdm_engine::supported(&url) {
                    "file"
                } else {
                    "none"
                };
                line(&json!({ "kind": kind }))
            }
            "act" => {
                let action = req.action.as_deref().unwrap_or_default();
                let answer = match self.act(action, &req.ids) {
                    Ok(done) => line(&done),
                    Err(e) => line(&e),
                };
                self.dirty = true;
                answer
            }
            "queue" => {
                let at = req
                    .queue
                    .as_deref()
                    .map_or(Some(0), |n| self.queues.iter().position(|q| q.name == n));
                let Some(q) = at.filter(|q| *q < self.queues.len()) else {
                    return fail("unknown-queue", "no such queue");
                };
                if req.run.unwrap_or(true) {
                    self.start_queue(q);
                } else {
                    self.stop_queue(q);
                }
                self.dirty = true;
                line(&json!({ "ok": true }))
            }
            "details" => {
                let Some(id) = req.id else {
                    return fail("bad-request", "no id");
                };
                let Some(i) = self.items.iter().find(|i| i.id == id) else {
                    return fail("unknown-id", "no such download");
                };
                let snap = i.snap.as_ref();
                line(&json!({
                    "download": row(i),
                    "description": i.description,
                    "referer": i.referer,
                    "page": i.page,
                    "segments": snap.map(|s| s.segments.iter().map(|g| [g.start, g.cursor, g.end]).collect::<Vec<_>>()),
                    "connections": snap.map(|s| s.connections.iter().map(|c| json!({
                        "downloaded": c.downloaded,
                        "speed": c.speed,
                        "status": format!("{:?}", c.status),
                    })).collect::<Vec<_>>()),
                    // Where its server is, as the desktop's progress window says it: as the
                    // running download sees it, else as its last run did (null meanwhile while
                    // the location table loads; the screen asks again).
                    "server": snap
                        .and_then(|s| s.server.as_ref())
                        .map(crate::whereis::Seen::from)
                        .or_else(|| i.server.clone())
                        .as_ref()
                        .and_then(crate::whereis::summary),
                    "log": i.log.iter().rev().take(50).map(|(at, text)| json!({"at": at, "text": text})).collect::<Vec<_>>(),
                }))
            }
            "settings" => match serde_json::to_value(&self.settings) {
                Ok(v) => line(&v),
                Err(e) => fail("failed", &e.to_string()),
            },
            "set_settings" => {
                let Some(Value::Object(patch)) = req.patch.clone() else {
                    return fail("bad-request", "no patch");
                };
                let mut now = match serde_json::to_value(&self.settings) {
                    Ok(Value::Object(m)) => m,
                    _ => return fail("failed", "settings unreadable"),
                };
                for (k, v) in patch {
                    if !now.contains_key(&k) {
                        return fail("unknown-setting", &k);
                    }
                    now.insert(k, v);
                }
                match serde_json::from_value::<crate::settings::Settings>(Value::Object(now)) {
                    Ok(new) => {
                        self.apply_options(new);
                        line(&json!({ "ok": true }))
                    }
                    Err(e) => fail("bad-setting", &e.to_string()),
                }
            }
            "speed_limit" => {
                if let Some(kb) = req.kb {
                    self.settings.speed_limit_kb = kb;
                }
                if let Some(on) = req.on {
                    self.settings.speed_limit_on = on;
                }
                self.apply_speed_limit();
                self.dirty = true;
                line(&json!({ "ok": true }))
            }
            "video_lookup" => {
                let Some(address) = req.address.clone().or(req.url.clone()) else {
                    return fail("bad-request", "no address");
                };
                let cmd = ipc::Command {
                    video_lookup: Some(ipc::VideoLookup {
                        address,
                        title: req.title.clone(),
                        store_id: None,
                    }),
                    referer: req.referer.clone(),
                    cookies: req.cookies.clone(),
                    user_agent: req.user_agent.clone(),
                    ..Default::default()
                };
                line(&self.quality_menu(&cmd))
            }
            "video_poll" => {
                let cmd = ipc::Command {
                    video_poll: req.token.clone(),
                    ..Default::default()
                };
                line(&self.quality_menu(&cmd))
            }
            "video_pick" => {
                let cmd = ipc::Command {
                    video_pick: Some(ipc::VideoPick {
                        token: req.token.clone().unwrap_or_default(),
                        choice: req.choice.unwrap_or_default(),
                        store_id: None,
                    }),
                    ..Default::default()
                };
                line(&self.quality_menu(&cmd))
            }
            "torrent_lookup" => {
                let source = if let Some(data) = req.data.as_deref() {
                    match base64::engine::general_purpose::STANDARD.decode(data) {
                        Ok(b) => mdm_torrent::Source::File(b),
                        Err(_) => return fail("bad-request", "torrent data isn't base64"),
                    }
                } else {
                    match req
                        .url
                        .as_deref()
                        .and_then(|u| mdm_torrent::Source::magnet(u.trim()))
                    {
                        Some(s) => s,
                        None => return fail("bad-request", "not a magnet link"),
                    }
                };
                let from_magnet = req.data.is_none();
                let slot = self.look_up_torrent(source);
                let token = format!(
                    "t{}",
                    TORRENT_TOKENS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                );
                torrents().insert(token.clone(), (slot, from_magnet));
                line(&json!({ "token": token }))
            }
            "torrent_poll" => {
                let token = req.token.clone().unwrap_or_default();
                let map = torrents();
                let Some((slot, _)) = map.get(&token) else {
                    return line(&json!({ "state": "gone" }));
                };
                let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_ref() {
                    None => line(&json!({ "state": "pending" })),
                    Some(Err(e)) => line(&json!({ "state": "failed", "message": e })),
                    Some(Ok(meta)) => {
                        let files: Vec<Value> = meta
                            .files
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| !f.padding)
                            .map(|(i, f)| json!({ "index": i, "path": f.path.to_string_lossy(), "size": f.size }))
                            .collect();
                        line(&json!({
                            "state": "ready",
                            "name": meta.name,
                            "size": meta.total(),
                            "files": files,
                            "known": self.torrent_hashes().contains(&meta.info_hash),
                        }))
                    }
                }
            }
            "torrent_add" => {
                let token = req.token.clone().unwrap_or_default();
                let Some((slot, from_magnet)) = torrents().remove(&token) else {
                    return fail("gone", "no such torrent look-up");
                };
                let Some(Ok(meta)) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else {
                    return fail("not-ready", "the torrent's files aren't known yet");
                };
                if self.torrent_hashes().contains(&meta.info_hash) {
                    return fail("duplicate", "this torrent is in the list already");
                }
                let category = req
                    .category
                    .as_deref()
                    .and_then(|k| Category::all().into_iter().find(|c| c.key() == k))
                    .unwrap_or_else(|| crate::torrentdlg::category_of(&meta));
                let dir = crate::model::category_folder(
                    category,
                    &self.downloads,
                    &self.settings.category_dirs,
                );
                let files =
                    (!req.ids.is_empty()).then(|| req.ids.iter().map(|&i| i as usize).collect());
                let id = self.add_torrent(crate::torrentdlg::TorrentAdd {
                    meta,
                    dir,
                    files,
                    category,
                    start_now: !req.later,
                    from_magnet,
                    in_order: req.on.unwrap_or(false),
                    queue: req.later.then_some(0),
                    quiet: true,
                });
                self.dirty = true;
                line(&json!({ "id": id }))
            }
            "queues" => match serde_json::to_value(&self.queues) {
                Ok(v) => line(&json!({ "queues": v })),
                Err(e) => fail("failed", &e.to_string()),
            },
            "set_queue" => {
                // A queue's settings and schedule (the Scheduler), merged as `set_settings` is; a
                // new queue when `queue` names none yet.
                let name = req.queue.clone().unwrap_or_default();
                if name.trim().is_empty() {
                    return fail("bad-request", "no queue name");
                }
                let at = match self.queues.iter().position(|q| q.name == name) {
                    Some(at) => at,
                    None => {
                        self.queues
                            .push(crate::queue::Queue::new(name.trim(), false));
                        self.queues.len() - 1
                    }
                };
                if let Some(Value::Object(patch)) = req.patch.clone() {
                    let mut now = match serde_json::to_value(&self.queues[at]) {
                        Ok(Value::Object(m)) => m,
                        _ => return fail("failed", "queue unreadable"),
                    };
                    for (k, v) in patch {
                        if k == "schedule"
                            && let (Some(Value::Object(old)), Value::Object(new)) =
                                (now.get_mut("schedule"), &v)
                        {
                            for (k2, v2) in new {
                                old.insert(k2.clone(), v2.clone());
                            }
                            continue;
                        }
                        if !now.contains_key(&k) || k == "items" || k == "name" {
                            return fail("unknown-setting", &k);
                        }
                        now.insert(k, v);
                    }
                    match serde_json::from_value::<crate::queue::Queue>(Value::Object(now)) {
                        Ok(mut q) => {
                            // Android: a start time is an exact alarm, which starts the app.
                            q.schedule.wake =
                                q.schedule.start_at.is_some() || q.schedule.once.is_some();
                            q.running = self.queues[at].running;
                            self.queues[at] = q;
                        }
                        Err(e) => return fail("bad-setting", &e.to_string()),
                    }
                }
                self.dirty = true;
                line(&json!({ "ok": true }))
            }
            "debrid" => {
                let services: Vec<Value> = mdm_debrid::ServiceKind::ALL
                    .iter()
                    .map(|k| {
                        json!({
                            "key": k,
                            "name": k.name(),
                            "token_page": k.token_page(),
                            "by_code": k.signs_in_by_code(),
                        })
                    })
                    .collect();
                let accounts: Vec<Value> = self
                    .settings
                    .debrid_accounts
                    .iter()
                    .map(|a| {
                        json!({
                            "id": a.id,
                            "service": a.service,
                            "name": a.service.name(),
                            "enabled": a.enabled,
                            "user": a.user,
                            "state": a.state,
                        })
                    })
                    .collect();
                line(
                    &json!({ "services": services, "accounts": accounts, "on": self.settings.debrid_on }),
                )
            }
            "debrid_add" => {
                // An account by its API token (key), as Options' "paste a token" does.
                let Some(service) = req
                    .code
                    .as_deref()
                    .and_then(|c| serde_json::from_value::<mdm_debrid::ServiceKind>(json!(c)).ok())
                else {
                    return fail("bad-request", "unknown service");
                };
                let Some(token) = req.data.as_deref().map(str::trim).filter(|t| !t.is_empty())
                else {
                    return fail("bad-request", "no token");
                };
                let secret = match serde_json::to_string(&mdm_debrid::Credential::Token {
                    token: token.to_owned(),
                }) {
                    Ok(s) => s,
                    Err(e) => return fail("failed", &e.to_string()),
                };
                let id = self
                    .settings
                    .debrid_accounts
                    .iter()
                    .map(|a| a.id)
                    .max()
                    .unwrap_or(0)
                    + 1;
                self.settings
                    .debrid_accounts
                    .push(crate::settings::DebridAccount {
                        id,
                        service,
                        enabled: true,
                        secret,
                        ..Default::default()
                    });
                self.settings.debrid_on = true;
                self.dirty = true;
                line(&json!({ "id": id }))
            }
            "debrid_signin" => {
                // Sign in by code (Real-Debrid's device flow, AllDebrid's PIN, TorBox's code),
                // as Options → Accounts does: the code to type at the service's page, then a poll.
                let Some(service) = req
                    .code
                    .as_deref()
                    .and_then(|c| serde_json::from_value::<mdm_debrid::ServiceKind>(json!(c)).ok())
                    .filter(|s| s.signs_in_by_code())
                else {
                    return fail("bad-request", "this service signs in with a token");
                };
                let mut g = SIGNIN.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((_, old)) = g.take() {
                    old.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let signin = crate::debrid::SignIn::default();
                signin.by_code(service, ctx);
                *g = Some((service, signin));
                line(&json!({ "started": true }))
            }
            "debrid_signin_state" => {
                use crate::debrid::SignInState as S;
                let mut g = SIGNIN.lock().unwrap_or_else(|e| e.into_inner());
                let Some((service, signin)) = g.as_ref() else {
                    return line(&json!({ "state": "idle" }));
                };
                let service = *service;
                match signin.get() {
                    S::Idle => line(&json!({ "state": "idle" })),
                    S::Starting => line(&json!({ "state": "starting" })),
                    S::Waiting { code, page, until } => line(&json!({
                        "state": "waiting",
                        "code": code,
                        "page": page,
                        "seconds": until.saturating_duration_since(std::time::Instant::now()).as_secs(),
                    })),
                    S::Checking => line(&json!({ "state": "checking" })),
                    S::Failed(why) => {
                        *g = None;
                        line(&json!({ "state": "failed", "message": why }))
                    }
                    S::Done {
                        user,
                        state,
                        credential,
                    } => {
                        *g = None;
                        let id = self
                            .settings
                            .debrid_accounts
                            .iter()
                            .map(|a| a.id)
                            .max()
                            .unwrap_or(0)
                            + 1;
                        self.settings
                            .debrid_accounts
                            .push(crate::settings::DebridAccount {
                                id,
                                service,
                                enabled: true,
                                user: user.clone(),
                                state,
                                secret: credential,
                            });
                        self.settings.debrid_on = true;
                        self.dirty = true;
                        line(&json!({ "state": "done", "user": user, "id": id }))
                    }
                }
            }
            "debrid_signin_cancel" => {
                if let Some((_, s)) = SIGNIN.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                line(&json!({ "ok": true }))
            }
            "debrid_remove" => {
                let id = req.id.unwrap_or_default();
                self.settings.debrid_accounts.retain(|a| a.id != id);
                self.dirty = true;
                line(&json!({ "ok": true }))
            }
            "playlist_start" => {
                let Some(url) = req.url.clone() else {
                    return fail("bad-request", "no address");
                };
                let Some(tool) = mdm_tools::tools_dir()
                    .as_deref()
                    .and_then(mdm_tools::installed)
                else {
                    return fail("no-ytdlp", "yt-dlp isn't installed");
                };
                match mdm_tools::playlist::Listing::start(
                    &tool.exe,
                    &url,
                    &mdm_tools::proc::FromBrowser::default(),
                    1,
                    None,
                ) {
                    Ok(listing) => {
                        let token = format!(
                            "p{}",
                            TORRENT_TOKENS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        );
                        playlists().insert(token.clone(), listing);
                        line(&json!({ "token": token }))
                    }
                    Err(e) => fail("failed", &e.to_string()),
                }
            }
            "playlist_poll" => {
                let map = playlists();
                let Some(listing) = map.get(req.token.as_deref().unwrap_or_default()) else {
                    return line(&json!({ "state": "gone" }));
                };
                let p = listing.progress();
                let entries: Vec<Value> = listing
                    .entries_from(0)
                    .into_iter()
                    .map(|e| {
                        json!({
                            "index": e.index,
                            "title": e.title,
                            "duration": e.duration,
                            "unavailable": e.unavailable,
                        })
                    })
                    .collect();
                let merge = mdm_tools::tools_dir()
                    .as_deref()
                    .and_then(mdm_tools::ffmpeg::installed)
                    .is_some();
                let picks: Vec<String> = crate::playlistdlg::picks(merge)
                    .into_iter()
                    .map(|p| p.label)
                    .collect();
                line(&json!({
                    "state": if p.done { "done" } else { "listing" },
                    "title": p.title,
                    "total": p.total,
                    "error": p.error,
                    "entries": entries,
                    "picks": picks,
                }))
            }
            "playlist_add" => {
                let Some(listing) = playlists().remove(req.token.as_deref().unwrap_or_default())
                else {
                    return fail("gone", "no such playlist");
                };
                let title = listing
                    .progress()
                    .title
                    .unwrap_or_else(|| "Playlist".into());
                let wanted: std::collections::HashSet<u64> = req.ids.iter().copied().collect();
                let entries: Vec<(url::Url, String)> = listing
                    .entries_from(0)
                    .into_iter()
                    .filter(|e| !e.unavailable && (wanted.is_empty() || wanted.contains(&e.index)))
                    .filter_map(|e| Some((url::Url::parse(&e.url).ok()?, e.title)))
                    .collect();
                listing.cancel();
                let merge = mdm_tools::tools_dir()
                    .as_deref()
                    .and_then(mdm_tools::ffmpeg::installed)
                    .is_some();
                let picks = crate::playlistdlg::picks(merge);
                let pick = &picks[req.choice.unwrap_or(0).min(picks.len() - 1)];
                let category = if mdm_tools::resolve::sound_only(&pick.selector, pick.ext) {
                    Category::Music
                } else {
                    Category::Video
                };
                let folder = self.settings.category_dir(category, &self.downloads);
                let n = entries.len();
                self.add_playlist(crate::playlistdlg::PlaylistAdd {
                    entries,
                    selector: pick.selector.clone(),
                    ext: pick.ext,
                    folder: folder.join(mdm_engine::naming::sanitize(&title)),
                    category,
                    start_now: !req.later,
                    context: Default::default(),
                });
                self.dirty = true;
                line(&json!({ "added": n }))
            }
            "downloads_dir" => {
                // Where new downloads go: Download/MisterDownloadManager, or the app's folder on
                // an SD card (the interface offers only folders it can write without a picker).
                if let Some(dir) = req.dir.as_deref().filter(|d| !d.is_empty()) {
                    let dir = std::path::PathBuf::from(dir);
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        return fail("failed", &e.to_string());
                    }
                    self.downloads = dir;
                    self.dirty = true;
                }
                line(&json!({ "dir": self.downloads.to_string_lossy() }))
            }
            "helpers" => {
                let dir = mdm_tools::tools_dir();
                let ytdlp = dir.as_deref().and_then(mdm_tools::installed);
                let ffmpeg = dir.as_deref().and_then(mdm_tools::ffmpeg::installed);
                let gallery = dir.as_deref().and_then(mdm_tools::gallery::installed);
                let i = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
                line(&json!({
                    "ytdlp": ytdlp.map(|i| i.version),
                    "ffmpeg": ffmpeg.map(|i| i.version),
                    "gallery": gallery.map(|i| i.version),
                    // Which one is being installed: `ytdlp`, `gallery` or null.
                    "installing": i.running,
                    "ytdlp_error": i.ytdlp_error,
                    "gallery_error": i.gallery_error,
                }))
            }
            "install_gallery" => {
                // gallery-dl's wheel, checked against its signed SHA256SUMS (mdm-tools gallery.rs).
                let Some(dir) = mdm_tools::tools_dir() else {
                    return fail("failed", "no data folder");
                };
                if !install_begin("gallery") {
                    return line(&json!({ "started": false }));
                }
                let net = crate::settings::own_net();
                self.rt.spawn(async move {
                    let error = mdm_tools::gallery::install_latest(&dir, &net, None)
                        .await
                        .err()
                        .map(|e| e.to_string());
                    if let Some(e) = &error {
                        tracing::warn!("gallery-dl install: {e}");
                    }
                    install_end("gallery", error);
                });
                line(&json!({ "started": true }))
            }
            "gallery_add" => {
                // An image gallery's page (Wikimedia Commons, Flickr…), as Download gallery adds
                // it: every picture into the General folder's own folder for it.
                let Some(page) = req
                    .url
                    .as_deref()
                    .and_then(|u| url::Url::parse(u.trim()).ok())
                else {
                    return fail("bad-request", "no address");
                };
                if mdm_tools::tools_dir()
                    .as_deref()
                    .and_then(mdm_tools::gallery::installed)
                    .is_none()
                {
                    return fail("no-gallery-dl", "gallery-dl isn't installed");
                }
                let name = mdm_tools::gallery::name_of(&page);
                let folder = self
                    .settings
                    .category_dir(Category::General, &self.downloads)
                    .join(&name);
                self.add_gallery(crate::gallerydlg::GalleryAdd {
                    page,
                    folder,
                    name,
                    limit: None,
                    start_now: !req.later,
                });
                self.dirty = true;
                line(&json!({ "ok": true }))
            }
            "install_ytdlp" => {
                let Some(dir) = mdm_tools::tools_dir() else {
                    return fail("failed", "no data folder");
                };
                if !install_begin("ytdlp") {
                    return line(&json!({ "started": false }));
                }
                let net = crate::settings::own_net();
                self.rt.spawn(async move {
                    let result = mdm_tools::install_latest(&dir, &net, None).await;
                    let error = result.err().map(|e| e.to_string());
                    if let Some(e) = &error {
                        tracing::warn!("yt-dlp install: {e}");
                    }
                    install_end("ytdlp", error);
                });
                line(&json!({ "started": true }))
            }
            "dismiss_notice" => {
                self.notice = None;
                line(&json!({ "ok": true }))
            }
            "tr" => {
                let texts: Vec<&str> = req.texts.iter().map(|t| crate::i18n::t(t)).collect();
                line(&json!({ "texts": texts }))
            }
            "languages" => {
                let list: Vec<Value> = crate::i18n::languages()
                    .into_iter()
                    .map(|(_, code, name)| json!({ "code": code, "name": name }))
                    .collect();
                line(&json!({
                    "languages": list,
                    "chosen": self.settings.language,
                    "current": crate::i18n::code(),
                }))
            }
            "set_language" => {
                crate::i18n::set_lang(req.code.as_deref());
                self.settings.language = req.code.clone();
                self.dirty = true;
                line(&json!({ "current": crate::i18n::code(), "rtl": crate::i18n::rtl() }))
            }
            "save" => {
                self.dirty = true;
                line(&json!({ "ok": true }))
            }
            "exit" => {
                self.quit(ctx);
                line(&json!({ "ok": true }))
            }
            other => fail("unknown-op", &format!("unknown op {other:?}")),
        }
    }

    /// `add`: each address (a file, a magnet link, a torrent's address) added with no window,
    /// started now or queued (`later`).
    fn android_add(&mut self, req: &Request) -> String {
        let mut urls: Vec<String> = req.urls.clone();
        urls.extend(req.url.clone());
        let mut ids = Vec::new();
        let mut pending = 0;
        let mut errors = Vec::new();
        for url in urls.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
            let cmd = ipc::Command {
                url: Some(url.to_owned()),
                category: req.category.clone(),
                queue: req.queue.clone(),
                later: req.later,
                name: req.name.clone(),
                dir: req.dir.clone(),
                referer: req.referer.clone(),
                cookies: req.cookies.clone(),
                user_agent: req.user_agent.clone(),
                silent: true,
                ..Default::default()
            };
            // i18n: where a download came from (its Properties): added in MDM's Android app
            let answer = self.add_without_window(cmd, crate::i18n::mark("added on this phone"));
            match serde_json::from_str::<Value>(&answer) {
                Ok(v) if v.get("error").is_some() => errors.push(json!({ "url": url, "error": v })),
                Ok(v) => {
                    if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        ids.push(id);
                    }
                    if v.get("pending").and_then(Value::as_bool) == Some(true) {
                        pending += 1;
                    }
                }
                Err(_) => errors.push(json!({ "url": url, "error": answer })),
            }
        }
        // Wait a moment so the first snapshot after this shows the new rows.
        let _ = Duration::ZERO;
        line(&json!({ "ids": ids, "pending": pending, "errors": errors }))
    }
}

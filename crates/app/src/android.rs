//! Android's platform calls, which `unix.rs` re-exports under the names the rest of the app calls
//! on Linux and macOS (as `win32.rs` holds Windows'). What only Java can do there (the keystore,
//! exact alarms, wake locks) the Android app does for the core through a [`Host`], given once at
//! its start ([`set_host`]).
//!
//! Built on this system too with the `android-core` feature, so its tests run here:
//! `cargo test -p mdm-app --features android-core --lib android`.
//!
//! [`run`] is the app on Android: the desktop's start-up (lib.rs `app`) without what a phone has
//! no use for, and eframe's window on the Android app's activity.

use std::sync::{Arc, OnceLock};

/// What only the Android side can do, called from the core's threads.
pub trait Host: Send + Sync + 'static {
    /// Sign-ins (debrid accounts, site logins, the proxy's password) in the Android keystore:
    /// `op` is `probe`, `read`, `write`, `delete` or `list`. `Ok(None)` for "not there"; a
    /// list's accounts come one per line.
    fn keychain(
        &self,
        op: &str,
        service: &str,
        account: &str,
        secret: Option<&str>,
    ) -> Result<Option<String>, String>;
    /// Start the app at `at` (seconds since 1970, UTC) for the Scheduler, or cancel (None):
    /// an exact alarm.
    fn wake_at(&self, at: Option<i64>) -> Result<(), String>;
    /// Hold the CPU and Wi-Fi awake while downloads run (`Some(why)`), or let go (None).
    fn keep_awake(&self, reason: Option<&str>) -> bool;
    /// Send the app's activity to the background, as Home does (Back on the main screen); the
    /// downloads go on. False when there's no activity to send.
    fn to_back(&self) -> bool;
    /// Hand `target` to the app Android picks for it: `kind` is `url` (a web page), `view` (a
    /// file, opened by the app set for its type), `chooser` (a file, the system's choice of apps:
    /// Open with) or `folder` (the system's Downloads, as Android has no folder view to give).
    fn open(&self, kind: &str, target: &str) -> Result<(), String>;
    /// The downloads' notification (while the downloads keep the phone awake): `line` says how
    /// they go ("2 downloading, 1.25 MB/sec"), and `stop` is its Stop All button's label.
    fn status(&self, line: &str, stop: &str);
    /// The clipboard (Android's, only while the app is in front): `op` is `get` (its text, None
    /// when it holds none), `set` (`text` on it) or `changes` (a number that changes whenever a
    /// new clip is copied, so the watch for copied links reads it only then).
    fn clipboard(&self, op: &str, text: Option<&str>) -> Result<Option<String>, String>;
    /// Android's picker, waiting for the person: `kind` is `folder` (its path on the shared
    /// storage) or `file` (a copy in the app's cache MDM can read); `dir`, where to start. None:
    /// cancelled, or a place MDM can't use.
    fn pick(&self, kind: &str, dir: Option<&str>) -> Option<String>;
    /// The downloads' own notifications: `json` holds each running one (its name, how far, its
    /// figures) and those just completed, with the words their notifications need (phone.rs
    /// `Notice`, [`Notices`]).
    fn downloads(&self, json: &str);
}

/// The clipboard's text ([`Host::clipboard`]), as lib.rs `clipboard_text` reads it elsewhere.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn clipboard_read() -> crate::clipboard::Read {
    match host().map(|h| h.clipboard("get", None)) {
        Some(Ok(Some(text))) => crate::clipboard::Read::Text(text),
        Some(Ok(None)) => crate::clipboard::Read::NoText,
        Some(Err(e)) => {
            tracing::debug!("clipboard: not read now ({e})");
            crate::clipboard::Read::Failed
        }
        None => crate::clipboard::Read::NoText,
    }
}

/// `text` on the clipboard; whether it took it.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn clipboard_write(text: &str) -> bool {
    host().is_some_and(|h| h.clipboard("set", Some(text)).is_ok())
}

/// A number that changes with each new clip ([`Host::clipboard`]'s `changes`), for the watch.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn clipboard_changes() -> Option<u64> {
    host()?
        .clipboard("changes", None)
        .ok()
        .flatten()
        .and_then(|n| n.parse().ok())
}

/// [`Host::open`]: a page, a file or a folder handed to Android's apps (`launch::open`, Open and
/// Open with, Open folder).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn open_with_system(kind: &str, target: &str) -> Result<(), String> {
    host()
        .ok_or_else(|| "no Android host".to_owned())?
        .open(kind, target)
}

/// Back on the main screen: the app goes to the background ([`Host::to_back`]).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn to_back() -> bool {
    host().is_some_and(|h| h.to_back())
}

static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();

/// The Android app's [`Host`], given once before the core starts; false when one was given
/// already (the first stays).
pub fn set_host(host: Arc<dyn Host>) -> bool {
    HOST.set(host).is_ok()
}

fn host() -> Option<&'static Arc<dyn Host>> {
    HOST.get()
}

/// The app on Android, until its activity ends: `crates/android`'s `android_main` calls it once
/// the data folder is set (`mdm_ipc::set_data_dir`) and the [`Host`] given. As the desktop's
/// start-up (lib.rs `app`), without the crash monitor's process (Android starts no second copy of
/// the app), the command line, the hand-over to a running copy (one process holds the activity)
/// and the notification-area icon.
#[cfg(target_os = "android")]
pub fn run(
    android_app: egui_winit::winit::platform::android::activity::AndroidApp,
) -> eframe::Result {
    let no_start = |why: &str| eframe::Error::AppCreation(why.to_owned().into());
    let data = mdm_ipc::data_dir().ok_or_else(|| no_start("no data folder"))?;
    std::fs::create_dir_all(&data).map_err(|e| no_start(&e.to_string()))?;
    let _ = mdm_ipc::private_dir(&data);
    // Panics in crash.log, as on the desktop (and on to the default hook: logcat).
    let default_hook = std::panic::take_hook();
    let at = data.clone();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let _ = std::fs::write(at.join("crash.log"), format!("{info}\n\n{bt}"));
        default_hook(info);
    }));
    // Android may start the activity again in the same process (opened again after Back, or
    // after the system destroyed it): this run waits for the last one to have ended (its
    // activity's destruction ends it, [`activity_ending`]) and saved the list.
    static ONE_RUN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE_RUN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ENDING.store(false, std::sync::atomic::Ordering::SeqCst);
    let Some(_instance) = mdm_ipc::instance_lock() else {
        return Err(no_start("the data folder is in use"));
    };
    crate::clock::start();
    crate::theme::prepare_fonts();
    let (tx, commands) = std::sync::mpsc::channel();
    let window: Arc<OnceLock<egui::Context>> = Default::default();
    let wake = window.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if let Some(ctx) = wake.get() {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    });
    let sink = mdm_ipc::Sink::new(tx.clone(), wake.clone());
    // The file and folder pickers ("..." buttons): Android's, through the Host.
    let _ = rfd::set_picker(Box::new(|kind, dir| {
        let kind = match kind {
            rfd::Pick::Folder => "folder",
            rfd::Pick::File => "file",
        };
        let dir = dir.map(|d| d.to_string_lossy().into_owned());
        host()?
            .pick(kind, dir.as_deref())
            .map(std::path::PathBuf::from)
    }));
    // Links other apps hand over (Share, a link opened with MDM) reach the app from here on,
    // those that came before it started first.
    links_channel(Some((tx.clone(), wake.clone())));
    let _ = mdm_ipc::serve(tx, move || wake());
    let rt = tokio::runtime::Runtime::new().map_err(|e| no_start(&e.to_string()))?;
    let handle = rt.handle().clone();
    let mut loaded = crate::store::load();
    // A phone's first start: the list's rows show each download's progress, and the system's
    // notification its end; the desktop's progress and "Download complete" windows, which would
    // cover the whole screen there, open only when asked for (Options can have them back).
    if loaded.items.is_empty() && !loaded.settings.language_asked {
        loaded.settings.show_progress = false;
        loaded.settings.show_complete = false;
    }
    let z = crate::scale::startup(loaded.settings.interface_size);
    mdm_crash::logs::init(&data, mdm_crash::logs::APP);
    tracing::info!(
        "Mister Download Manager {} ({}) starting on Android",
        crate::help::VERSION,
        crate::help::COMMIT
    );
    crate::theme::init_smooth_text(loaded.settings.smooth_text);
    crate::theme::set_ui_font(loaded.settings.interface_font.clone(), false, false);
    crate::theme::init_text_size(loaded.settings.interface_text_size);
    let _ = loaded.settings.for_disk();
    let _ = ACTIVITY.set(android_app.clone());
    let options = eframe::NativeOptions {
        android_app: Some(android_app),
        viewport: crate::window_builder().with_title(crate::MAIN_TITLE),
        ..Default::default()
    };
    let result = eframe::run_native(
        crate::APP_ID,
        options,
        Box::new(move |cc| {
            crate::scale::start(&cc.egui_ctx, z);
            let _ = window.set(cc.egui_ctx.clone());
            crate::clipboard::wake(&cc.egui_ctx);
            let _ = CONTEXT.set(cc.egui_ctx.clone());
            Ok(Box::new(OnAndroid {
                app: crate::app::App::new(
                    &cc.egui_ctx,
                    handle,
                    commands,
                    sink,
                    loaded,
                    crate::Start::Shown,
                ),
                typed: String::new(),
                typing: false,
                told: Told::default(),
                interrupted: take_interrupted(),
                notices: Notices::default(),
            }))
        }),
    );
    links_channel(None);
    tracing::info!("the activity ended");
    mdm_crash::logs::flush(std::time::Duration::from_secs(2));
    result
}

/// The app's window, for what reaches it from Java between frames ([`back_pressed`]).
#[cfg(target_os = "android")]
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Back gestures and Back presses Java has seen since the last frame ([`back_pressed`]).
#[cfg(target_os = "android")]
static BACKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The downloads that were running when the app ended, saved beside the list (`resume.json`):
/// Android ends the app itself (the activity closed, the app swiped away, the process killed),
/// and what was downloading goes on at the next start ([`take_interrupted`]).
#[cfg(target_os = "android")]
pub fn note_interrupted(ids: &[u64]) {
    if ids.is_empty() {
        return;
    }
    let Some(path) = mdm_ipc::data_dir().map(|d| d.join("resume.json")) else {
        return;
    };
    let mut all = read_interrupted(&path);
    for id in ids {
        if !all.contains(id) {
            all.push(*id);
        }
    }
    if let Ok(text) = serde_json::to_string(&all)
        && let Err(e) = std::fs::write(&path, text)
    {
        tracing::warn!("{}: {e}", path.display());
    }
}

#[cfg(target_os = "android")]
fn read_interrupted(path: &std::path::Path) -> Vec<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// [`note_interrupted`]'s downloads, once: the file goes.
#[cfg(target_os = "android")]
fn take_interrupted() -> Vec<u64> {
    let Some(path) = mdm_ipc::data_dir().map(|d| d.join("resume.json")) else {
        return Vec::new();
    };
    let ids = read_interrupted(&path);
    let _ = std::fs::remove_file(&path);
    ids
}

/// The notification's Stop All was tapped (`DownloadService`); the next frame stops them.
#[cfg(target_os = "android")]
static STOP_ALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `DownloadService.nativeStopAll()`: the notification's Stop All ([`STOP_ALL`]).
#[cfg(target_os = "android")]
pub fn stop_all_asked() {
    STOP_ALL.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// The downloads whose own notification's Stop was tapped (`DownloadService`), for the next frame.
#[cfg(target_os = "android")]
static STOP_ONE: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

/// `DownloadService.nativeStop(id)`: a download's notification's Stop ([`STOP_ONE`]).
#[cfg(target_os = "android")]
pub fn stop_asked(id: u64) {
    STOP_ONE.lock().unwrap_or_else(|e| e.into_inner()).push(id);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// The downloads' notification, kept up to date ([`Host::status`]): what it last said, and when.
#[cfg(target_os = "android")]
#[derive(Default)]
struct Told {
    line: Option<String>,
    at: Option<std::time::Instant>,
}

#[cfg(target_os = "android")]
impl Told {
    /// `line` (the app's activity) to the notification when it changed, at most once a second.
    fn tell(&mut self, line: Option<String>) {
        let Some(line) = line else {
            self.line = None;
            return;
        };
        let due = self
            .at
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if self.line.as_ref() == Some(&line) || !due {
            return;
        }
        if let Some(h) = host() {
            // The toolbar's Stop All, translated as it is there.
            h.status(&line, crate::i18n::tc("toolbar", "Stop All"));
        }
        self.line = Some(line);
        self.at = Some(std::time::Instant::now());
    }
}

/// The downloads' own notifications ([`Host::downloads`]), kept up to date: a notification
/// for each running download, and a "Download complete" one with Open for each that completes
/// while the app runs. Told at most once a second, and only when something changed.
#[cfg(target_os = "android")]
#[derive(Default)]
struct Notices {
    /// The running downloads last told.
    running: Vec<crate::app::phone::Notice>,
    at: Option<std::time::Instant>,
}

#[cfg(target_os = "android")]
impl Notices {
    fn tell(&mut self, app: &crate::app::App) {
        if self
            .at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1))
        {
            return;
        }
        let running = app.running_notices();
        // Those running last time and not now that have completed.
        let done: Vec<_> = self
            .running
            .iter()
            .filter(|n| !running.iter().any(|r| r.id == n.id))
            .filter_map(|n| app.completed_notice(n.id))
            .collect();
        if running == self.running && done.is_empty() {
            return;
        }
        let words = serde_json::json!({
            "running": running,
            "done": done,
            "complete": crate::i18n::t("Download complete"),
            "open": crate::i18n::t("Open"),
            "stop": crate::i18n::tc("toolbar", "Stop"),
        });
        if let Some(h) = host() {
            h.downloads(&words.to_string());
        }
        self.running = running;
        self.at = Some(std::time::Instant::now());
    }
}

/// Where links other apps hand over go ([`links_given`]): the app's command channel and what
/// wakes it, while the app runs; the links that came while it didn't.
type LinkChannel = (
    std::sync::mpsc::Sender<mdm_ipc::Incoming>,
    Arc<dyn Fn() + Send + Sync>,
);
static LINKS: std::sync::Mutex<(Vec<String>, Option<LinkChannel>)> =
    std::sync::Mutex::new((Vec::new(), None));

/// The app's command channel for handed-over links (None when the app ends); the links that
/// waited for it go now.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn links_channel(channel: Option<LinkChannel>) {
    let mut links = LINKS.lock().unwrap_or_else(|e| e.into_inner());
    links.1 = channel;
    let waiting = std::mem::take(&mut links.0);
    drop(links);
    for url in waiting {
        hand_link(url);
    }
}

/// Text another app handed MDM (`MainActivity`: Share, or a link opened with MDM): each link in
/// it goes to the app as the desktop's command line gives one (Download File Info opens for it).
/// Before the app runs, they wait for it.
pub fn links_given(text: &str) {
    for url in links_in(text) {
        hand_link(url);
    }
}

/// A file another app handed MDM (`MainActivity`: a `.torrent` opened or shared), copied into
/// the app's cache: Add torrent opens for it, as for one given on the desktop's command line.
pub fn file_given(path: &str) {
    hand_link(path.to_owned());
}

fn hand_link(url: String) {
    let mut links = LINKS.lock().unwrap_or_else(|e| e.into_inner());
    let sent = links.1.as_ref().is_some_and(|(tx, wake)| {
        let cmd = mdm_ipc::Command {
            url: Some(url.clone()),
            ..Default::default()
        };
        let sent = tx.send(mdm_ipc::Incoming { cmd, reply: None }).is_ok();
        if sent {
            wake();
        }
        sent
    });
    if !sent {
        links.0.push(url);
    }
}

/// The links in `text`, as a page's shared text has them ("Look at this: https://…"): web, FTP
/// and magnet addresses, in order, without the punctuation a sentence puts after them.
pub fn links_in(text: &str) -> Vec<String> {
    const SCHEMES: [&str; 4] = ["http://", "https://", "ftp://", "magnet:?"];
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_start_matches(['(', '<', '"', '\'']);
        let lower = word.to_ascii_lowercase();
        if !SCHEMES.iter().any(|s| lower.starts_with(s)) {
            continue;
        }
        let link = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '>', '"', '\'']);
        if link.len() > 9 && !out.iter().any(|l| l == link) {
            out.push(link.to_owned());
        }
    }
    out
}

/// The soft keyboard's action key (Done, Go) since the last frame ([`editor_action`]).
#[cfg(target_os = "android")]
static ENTERS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The soft keyboard's action key (its Done or Go, where a keyboard shows Enter): the next frame
/// gets it as Enter, which the text box answers as on the desktop (Add URL's OK).
#[cfg(target_os = "android")]
pub fn editor_action() {
    ENTERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// Whether Back has something to close: a window besides the main one, the drawer or a menu
/// sheet, as of the last frame. Without one, the activity goes to the background as Home sends
/// it (`MainActivity`'s back callback), the downloads going on.
#[cfg(target_os = "android")]
static BACK_CLOSES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Back, as the system's back gesture or button (the activity's back dispatcher, which Android
/// 13 and later use instead of a key): the next frame gets it as Esc ([`OnAndroid`]).
#[cfg(target_os = "android")]
pub fn back_pressed() {
    BACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// The activity is being destroyed ([`activity_ending`]): the app closes as Exit closes it.
#[cfg(target_os = "android")]
static ENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The activity is being destroyed (the user closed it, or the system reclaims it): Android waits
/// for the app's thread to end before it goes on, so the app closes now, saving the list and
/// stopping its downloads where they are as Exit does. Called before the activity's own
/// destruction, which waits.
#[cfg(target_os = "android")]
pub fn activity_ending() {
    ENDING.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// The app as eframe runs it on Android: Back is Esc, which closes the window in front as on the
/// desktop. A key's Back (a keyboard's, an emulator's KEYCODE_BACK) comes to the window as
/// winit's BrowserBack; the gesture's through [`back_pressed`].
#[cfg(target_os = "android")]
struct OnAndroid {
    app: crate::app::App,
    /// The soft keyboard's text as last seen ([`typed_events`]).
    typed: String,
    /// Whether a text box had the keys at the last frame (the keyboard shown for it).
    typing: bool,
    /// What the downloads' notification says.
    told: Told,
    /// The downloads running when the app last ended, to resume ([`take_interrupted`]).
    interrupted: Vec<u64>,
    /// The downloads' own notifications.
    notices: Notices,
}

/// The activity, for its soft keyboard's text (GameActivity's text input state).
#[cfg(target_os = "android")]
static ACTIVITY: OnceLock<egui_winit::winit::platform::android::activity::AndroidApp> =
    OnceLock::new();

#[cfg(target_os = "android")]
impl eframe::App for OnAndroid {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::logic(&mut self.app, ctx, frame);
        // What was downloading when the app last ended goes on.
        if !self.interrupted.is_empty() {
            let ids = std::mem::take(&mut self.interrupted);
            self.app.resume_interrupted(&ids);
        }
        // The notification's Stop All, and what the notification says.
        if STOP_ALL.swap(false, std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("the notification's Stop All");
            self.app.stop_all();
        }
        let stops = std::mem::take(&mut *STOP_ONE.lock().unwrap_or_else(|e| e.into_inner()));
        for id in stops {
            tracing::info!("the notification's Stop for download {id}");
            self.app.stop_by_hand(id);
        }
        self.told.tell(self.app.activity());
        self.notices.tell(&self.app);
        if self.told.line.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        // The activity's window is gone and no frame is drawn: eframe's logic-only pass gets
        // here, and the closed window ends `run`.
        if ENDING.load(std::sync::atomic::Ordering::SeqCst) {
            tracing::info!("the activity is being destroyed: closing");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        let closes = crate::crash::other_windows() || self.app.phone_overlay(ui.ctx());
        BACK_CLOSES.store(closes, std::sync::atomic::Ordering::Relaxed);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.typed_events(ctx, raw);
        // Back: a key's (BrowserBack) and the gesture's ([`back_pressed`]).
        let mut backs = BACKS.swap(0, std::sync::atomic::Ordering::Relaxed);
        raw.events.retain(|e| match e {
            egui::Event::Key {
                key: egui::Key::BrowserBack,
                pressed,
                ..
            } => {
                backs += u32::from(*pressed);
                false
            }
            _ => true,
        });
        // Nothing for Back to close: the app goes to the background, as Android's apps do.
        if backs > 0 && !BACK_CLOSES.load(std::sync::atomic::Ordering::Relaxed) && to_back() {
            backs = 0;
        }
        let enters = ENTERS.swap(0, std::sync::atomic::Ordering::Relaxed);
        for pressed in (0..enters).flat_map(|_| [true, false]) {
            raw.events.push(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        // Else Back is Esc, which closes the window, the drawer or the menu in front.
        for pressed in (0..backs).flat_map(|_| [true, false]) {
            raw.events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
}

#[cfg(target_os = "android")]
impl OnAndroid {
    /// The soft keyboard's typing. GameActivity hands the keyboard's text to the app as a text
    /// input state (the whole text being edited), which winit doesn't pass on: the change since
    /// the last frame becomes egui's events (Backspaces for what went, then the text that came),
    /// for the text box that has the keys. Without one, the state starts again empty.
    fn typed_events(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        use egui_winit::winit::platform::android::activity::input::{TextInputState, TextSpan};
        let Some(activity) = ACTIVITY.get() else {
            return;
        };
        if !ctx.egui_wants_keyboard_input() {
            // No text box has the keys any more (its dialog closed): the keyboard goes.
            if std::mem::take(&mut self.typing) {
                activity.hide_soft_input(false);
            }
            if !self.typed.is_empty() {
                self.typed.clear();
                activity.set_text_input_state(TextInputState {
                    text: String::new(),
                    selection: TextSpan { start: 0, end: 0 },
                    compose_region: None,
                });
            }
            return;
        }
        self.typing = true;
        let now = activity.text_input_state().text;
        if now == self.typed {
            return;
        }
        let (gone, came) = typed_change(&self.typed, &now);
        for pressed in (0..gone).flat_map(|_| [true, false]) {
            raw.events.push(egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        // A line break is the keyboard's Enter (its action key, or Enter typed): a key press,
        // which a one-line text box answers (Add URL's OK), not text.
        for (k, part) in came.split('\n').enumerate() {
            if k > 0 {
                for pressed in [true, false] {
                    raw.events.push(egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
            }
            if !part.is_empty() {
                raw.events.push(egui::Event::Text(part.to_owned()));
            }
        }
        self.typed = now;
    }
}

/// What changed from `old` to `new` (a soft keyboard's text, [`OnAndroid::typed_events`]): how
/// many characters went from the end of what they share, and what came after it.
pub fn typed_change(old: &str, new: &str) -> (usize, String) {
    let shared = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((i, a), _)| i + a.len_utf8());
    (old[shared..].chars().count(), new[shared..].to_owned())
}

/// One of Android's font families (/system/etc/fonts.xml): its name (`sans-serif`…) or the
/// languages it is for (`und-Arab`, `zh-Hans`, `ja`…), and its regular font's file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFamily {
    pub name: Option<String>,
    pub langs: Vec<String>,
    pub file: std::path::PathBuf,
}

/// Android's font families, in the order the system falls back through them (fonts.xml's own),
/// read once. Empty where there is no fonts.xml (the tests' Linux).
pub fn font_families() -> &'static [FontFamily] {
    static FAMILIES: OnceLock<Vec<FontFamily>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let xml = std::fs::read_to_string("/system/etc/fonts.xml").unwrap_or_default();
        parse_fonts_xml(&xml, std::path::Path::new("/system/fonts"))
    })
}

/// fonts.xml's families: each one's regular upright font (weight 400, else its first), in
/// `dir`. A family whose file isn't named is left out.
pub fn parse_fonts_xml(xml: &str, dir: &std::path::Path) -> Vec<FontFamily> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("family"))
        .filter_map(|family| {
            let fonts: Vec<_> = family
                .children()
                .filter(|n| n.has_tag_name("font") && n.attribute("style") != Some("italic"))
                .collect();
            let font = fonts
                .iter()
                .find(|f| f.attribute("weight").is_none_or(|w| w == "400"))
                .or(fonts.first())?;
            // The file is the font element's own text (an <axis> may follow it).
            let file = font
                .children()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .map(str::trim)
                .find(|t| !t.is_empty())?;
            Some(FontFamily {
                name: family.attribute("name").map(str::to_owned),
                langs: family
                    .attribute("lang")
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
                file: dir.join(file),
            })
        })
        .collect()
}

static DOWNLOADS: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Where downloads go unless Options says otherwise: the phone's Download folder, MDM's own
/// folder in it (given by `crates/android` at the start). Made if it isn't there: it is MDM's,
/// and the first download shouldn't ask to create it (Download File Info asks for a folder
/// the user typed).
pub fn set_downloads_dir(dir: std::path::PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("the downloads folder {}: {e}", dir.display());
    }
    let _ = DOWNLOADS.set(dir);
}

/// [`set_downloads_dir`]'s folder (the desktop's Downloads has no counterpart here).
pub fn downloads_dir() -> Option<std::path::PathBuf> {
    DOWNLOADS.get().cloned()
}

/// The default browser, for the User-Agent of links added by hand: none known here.
pub fn default_browser() -> Option<mdm_engine::agent::Found> {
    None
}

/// A file type's icon from the system: none here (MDM's own are drawn).
pub fn file_icon(_what: &crate::fileicons::What, _px: u32) -> Option<crate::fileicons::Native> {
    None
}

/// The system's picture of a file, for the preview pane: none asked of Android yet (the pane
/// shows the type's icon).
pub fn thumbnail(_path: &std::path::Path, _px: u32) -> Option<crate::preview::Picture> {
    None
}

/// The Scheduler's start time, as an exact alarm (the host's).
pub fn wake_at(at: Option<i64>) -> Result<(), String> {
    match host() {
        Some(h) => h.wake_at(at),
        None => Err("no Android host".into()),
    }
}

/// Whether the Scheduler can start the app at a time: with the host's exact alarm, once there
/// is a host.
pub fn wake_support() -> crate::wake::Support {
    if host().is_some() {
        crate::wake::Support::Ready
    } else {
        crate::wake::Support::NoAlarm
    }
}

/// A wake lock and a Wi-Fi lock while downloads run (the host's).
pub fn keep_awake(reason: Option<&str>) -> bool {
    host().is_some_and(|h| h.keep_awake(reason))
}

fn keychain(
    op: &str,
    service: &str,
    account: &str,
    secret: Option<&str>,
) -> Result<Option<String>, crate::secrets::Fault> {
    let Some(h) = host() else {
        return Err(crate::secrets::Fault::Missing);
    };
    h.keychain(op, service, account, secret)
        .map_err(crate::secrets::Fault::Failed)
}

pub fn keychain_probe(service: &str) -> Result<(), crate::secrets::Fault> {
    keychain("probe", service, "", None).map(|_| ())
}

pub fn keychain_read(
    service: &str,
    account: &str,
) -> Result<Option<String>, crate::secrets::Fault> {
    keychain("read", service, account, None)
}

pub fn keychain_write(
    service: &str,
    account: &str,
    _label: &str,
    secret: &str,
) -> Result<(), crate::secrets::Fault> {
    keychain("write", service, account, Some(secret)).map(|_| ())
}

pub fn keychain_delete(service: &str, account: &str) -> Result<(), crate::secrets::Fault> {
    keychain("delete", service, account, None).map(|_| ())
}

pub fn keychain_list(service: &str) -> Result<Vec<String>, crate::secrets::Fault> {
    keychain("list", service, "", None).map(|l| {
        l.unwrap_or_default()
            .lines()
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Fault;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// The Android app's side, as a test stands in for it: a keystore in memory, the alarm and
    /// the wake lock remembered.
    #[derive(Default)]
    struct Fake {
        keys: Mutex<BTreeMap<(String, String), String>>,
        alarm: Mutex<Option<i64>>,
        awake: Mutex<Option<String>>,
        opened: Mutex<Vec<String>>,
        clip: Mutex<(Option<String>, u64)>,
    }

    impl Host for Fake {
        fn keychain(
            &self,
            op: &str,
            service: &str,
            account: &str,
            secret: Option<&str>,
        ) -> Result<Option<String>, String> {
            let mut keys = self.keys.lock().unwrap();
            let key = (service.to_owned(), account.to_owned());
            match op {
                "probe" => Ok(None),
                "read" => Ok(keys.get(&key).cloned()),
                "write" => {
                    keys.insert(key, secret.ok_or("no secret")?.to_owned());
                    Ok(None)
                }
                "delete" => Ok(keys.remove(&key).map(|_| String::new())),
                "list" => Ok(Some(
                    keys.keys()
                        .filter(|(s, _)| s == service)
                        .map(|(_, a)| format!("{a}\n"))
                        .collect(),
                )),
                _ => Err(format!("unknown op {op}")),
            }
        }
        fn wake_at(&self, at: Option<i64>) -> Result<(), String> {
            *self.alarm.lock().unwrap() = at;
            Ok(())
        }
        fn keep_awake(&self, reason: Option<&str>) -> bool {
            *self.awake.lock().unwrap() = reason.map(str::to_owned);
            true
        }
        fn to_back(&self) -> bool {
            true
        }
        fn downloads(&self, json: &str) {
            self.opened
                .lock()
                .unwrap()
                .push(format!("downloads {json}"));
        }
        fn pick(&self, kind: &str, _dir: Option<&str>) -> Option<String> {
            Some(format!("/picked/{kind}"))
        }
        fn clipboard(&self, op: &str, text: Option<&str>) -> Result<Option<String>, String> {
            let mut clip = self.clip.lock().unwrap();
            match op {
                "get" => Ok(clip.0.clone()),
                "set" => {
                    clip.0 = text.map(str::to_owned);
                    clip.1 += 1;
                    Ok(None)
                }
                "changes" => Ok(Some(clip.1.to_string())),
                _ => Err(format!("unknown op {op}")),
            }
        }
        fn status(&self, line: &str, stop: &str) {
            self.opened
                .lock()
                .unwrap()
                .push(format!("status {line} / {stop}"));
        }
        fn open(&self, kind: &str, target: &str) -> Result<(), String> {
            self.opened.lock().unwrap().push(format!("{kind} {target}"));
            Ok(())
        }
    }

    /// One test for the whole module: the host is given once per process.
    #[test]
    fn calls_reach_the_host() {
        assert!(matches!(keychain_probe("mdm"), Err(Fault::Missing)));
        assert!(wake_at(Some(1)).is_err());
        assert_eq!(wake_support(), crate::wake::Support::NoAlarm);
        assert!(!keep_awake(Some("downloads")));
        assert!(!to_back(), "no activity to send back yet");
        assert!(open_with_system("view", "/sdcard/Download/a.bin").is_err());

        let fake = Arc::new(Fake::default());
        assert!(set_host(fake.clone()));
        assert!(!set_host(Arc::new(Fake::default())), "the first host stays");

        assert!(keychain_probe("mdm").is_ok());
        assert_eq!(keychain_read("mdm", "debrid").unwrap(), None);
        keychain_write("mdm", "debrid", "label", "token-1").unwrap();
        keychain_write("mdm", "proxy", "label", "secret").unwrap();
        keychain_write("other", "x", "label", "y").unwrap();
        assert_eq!(
            keychain_read("mdm", "debrid").unwrap().as_deref(),
            Some("token-1")
        );
        assert_eq!(keychain_list("mdm").unwrap(), ["debrid", "proxy"]);
        keychain_delete("mdm", "debrid").unwrap();
        assert_eq!(keychain_list("mdm").unwrap(), ["proxy"]);

        assert_eq!(wake_support(), crate::wake::Support::Ready);
        wake_at(Some(1_800_000_000)).unwrap();
        assert_eq!(*fake.alarm.lock().unwrap(), Some(1_800_000_000));
        wake_at(None).unwrap();
        assert_eq!(*fake.alarm.lock().unwrap(), None);
        assert!(keep_awake(Some("downloads")));
        assert_eq!(fake.awake.lock().unwrap().as_deref(), Some("downloads"));
        assert!(keep_awake(None));
        assert_eq!(*fake.awake.lock().unwrap(), None);
        assert!(to_back());
        assert_eq!(clipboard_read(), crate::clipboard::Read::NoText);
        let before = clipboard_changes();
        assert!(clipboard_write("https://example.com/a.zip"));
        assert_ne!(clipboard_changes(), before);
        assert_eq!(
            clipboard_read(),
            crate::clipboard::Read::Text("https://example.com/a.zip".into())
        );
        open_with_system("chooser", "/sdcard/Download/a.bin").unwrap();
        assert_eq!(
            *fake.opened.lock().unwrap(),
            ["chooser /sdcard/Download/a.bin"]
        );

        assert!(default_browser().is_none());
    }

    /// Shared text's links: in order, once each, without a sentence's punctuation.
    #[test]
    fn links_are_found_in_shared_text() {
        assert_eq!(
            links_in("Look at this: https://example.com/a.zip, and (http://x.org/b.iso)."),
            ["https://example.com/a.zip", "http://x.org/b.iso"]
        );
        assert_eq!(
            links_in("magnet:?xt=urn:btih:abc&dn=x\nftp://h/f.bin https://example.com/a.zip"),
            ["magnet:?xt=urn:btih:abc&dn=x", "ftp://h/f.bin"]
                .into_iter()
                .chain(["https://example.com/a.zip"])
                .collect::<Vec<_>>()
        );
        assert!(links_in("no links here, just www.example.com").is_empty());
        assert_eq!(
            links_in("HTTPS://Example.com/x https://Example.com/x"),
            ["HTTPS://Example.com/x", "https://Example.com/x"]
        );
    }

    /// The soft keyboard's text, frame to frame: typed on, corrected, replaced.
    #[test]
    fn typed_text_becomes_backspaces_and_text() {
        assert_eq!(typed_change("", "h"), (0, "h".to_owned()));
        assert_eq!(typed_change("http", "https:/"), (0, "s:/".to_owned()));
        assert_eq!(typed_change("helo", "hello"), (1, "lo".to_owned()));
        assert_eq!(typed_change("abc", ""), (3, String::new()));
        assert_eq!(typed_change("same", "same"), (0, String::new()));
        assert_eq!(typed_change("наб", "набо"), (0, "о".to_owned()));
        assert_eq!(typed_change("日本", "日本語"), (0, "語".to_owned()));
        assert_eq!(typed_change("café", "cafe"), (1, "e".to_owned()));
    }

    /// fonts.xml as Android 14 writes it: named families, language families (one with two
    /// languages, one variable font with axes), italics passed over.
    #[test]
    fn fonts_xml_is_read_in_its_order() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<familyset version="23">
    <family name="sans-serif">
        <font weight="100" style="normal">Roboto-Regular.ttf
            <axis tag="wght" stylevalue="100" />
        </font>
        <font weight="400" style="normal">Roboto-Regular.ttf
            <axis tag="wght" stylevalue="400" />
        </font>
        <font weight="400" style="italic">Roboto-Italic.ttf</font>
    </family>
    <alias name="arial" to="sans-serif" />
    <family lang="und-Arab" variant="elegant">
        <font weight="400" style="normal">NotoNaskhArabic-Regular.ttf</font>
        <font weight="700" style="normal">NotoNaskhArabic-Bold.ttf</font>
    </family>
    <family lang="zh-Hans">
        <font weight="400" style="normal" index="2">NotoSansCJK-Regular.ttc</font>
    </family>
    <family lang="und-Zsye und-Zsym">
        <font weight="400" style="normal">NotoColorEmoji.ttf</font>
    </family>
    <family lang="und-Thai"><font style="italic">ItalicOnly.ttf</font></family>
</familyset>"#;
        let dir = std::path::Path::new("/system/fonts");
        let got = parse_fonts_xml(xml, dir);
        let files: Vec<_> = got.iter().map(|f| f.file.clone()).collect();
        assert_eq!(
            files,
            [
                dir.join("Roboto-Regular.ttf"),
                dir.join("NotoNaskhArabic-Regular.ttf"),
                dir.join("NotoSansCJK-Regular.ttc"),
                dir.join("NotoColorEmoji.ttf"),
            ]
        );
        assert_eq!(got[0].name.as_deref(), Some("sans-serif"));
        assert_eq!(got[1].langs, ["und-Arab"]);
        assert_eq!(got[3].langs, ["und-Zsye", "und-Zsym"]);
        assert!(parse_fonts_xml("not xml", dir).is_empty());
    }
}

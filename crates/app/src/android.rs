//! Android (android/ in the repository): the app's core without a window, as `--headless` runs it
//! on a server (#157), driven by the Android interface through JSON requests and answered with
//! JSON snapshots. Also this system's platform calls, which `unix.rs` re-exports under the names
//! the rest of the app calls on Linux and macOS: the interface does what only Java can (the
//! keystore, alarms, wake locks), through [`Host`].
//!
//! The interface's library (crates/android, `libmdm.so`) holds the JNI side: it starts a [`Core`]
//! with a [`Host`] that calls back into Kotlin, and hands each request to [`Core::request`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// What only the Android side can do, called from the core's threads.
pub trait Host: Send + Sync + 'static {
    /// The list changed (a snapshot, [`crate::app::android_ui`]'s `Snapshot` as JSON).
    fn changed(&self, snapshot: &str);
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
}

static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();
static CONFIG: OnceLock<Config> = OnceLock::new();

/// The folders the core was started with (the helpers find their programs there).
pub fn config() -> Option<&'static Config> {
    CONFIG.get()
}

fn host() -> Option<&'static Arc<dyn Host>> {
    HOST.get()
}

/// Where the core keeps and puts things: the interface's own folders.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// The app's private files (the list, settings, state): `Context.getFilesDir()/mdm`.
    pub data_dir: PathBuf,
    /// Where downloads go (each category's folder inside): `Download/MisterDownloadManager`.
    pub downloads_dir: PathBuf,
    /// Temporary files: `Context.getCacheDir()`.
    pub cache_dir: PathBuf,
    /// The programs shipped in the APK (`ApplicationInfo.nativeLibraryDir`): python, ffmpeg.
    pub native_lib_dir: PathBuf,
    /// The system's language tag ("en-US", "ar"), for an interface language never chosen.
    pub language: Option<String>,
    /// The WebView's User-Agent: what links added here are downloaded as.
    pub user_agent: Option<String>,
}

type Job = Box<dyn FnOnce(&mut crate::app::App, &egui::Context) + Send>;

/// The running core: one thread runs the app's passes; requests are done between them.
pub struct Core {
    jobs: Sender<Job>,
    wake: SyncSender<()>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// How long a request waits for the core's answer.
const ANSWER_WAIT: Duration = Duration::from_secs(20);

impl Core {
    /// Start the core with `config`; `host` gets the snapshots and the platform calls. Only one
    /// core per process (the data folder's lock): a second start fails.
    pub fn start(config: Config, host: Arc<dyn Host>) -> Result<Core, String> {
        let _ = HOST.set(host.clone());
        mdm_ipc::set_data_dir(config.data_dir.clone());
        std::fs::create_dir_all(&config.data_dir).map_err(|e| e.to_string())?;
        mdm_ipc::private_dir(&config.data_dir).map_err(|e| e.to_string())?;
        if !config.cache_dir.as_os_str().is_empty() {
            // The helpers' and the engine's temporary files (TMPDIR is unset on Android).
            // SAFETY of set_var: called before the core's threads start.
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var("TMPDIR", &config.cache_dir);
                // The helpers' programs under their own names (7-Zip's `7zz` for archives,
                // mdm-tools' android.rs makes the links), first on the PATH.
                let bin = config.data_dir.join("helpers").join("bin");
                let path = std::env::var_os("PATH").unwrap_or_else(|| "/system/bin".into());
                let mut dirs = vec![bin];
                dirs.extend(std::env::split_paths(&path));
                if let Ok(joined) = std::env::join_paths(dirs) {
                    std::env::set_var("PATH", joined);
                }
            }
        }
        let _ = CONFIG.set(config.clone());
        // yt-dlp's Python, ffmpeg and QuickJS from the APK, unpacked on a thread of their own.
        #[cfg(target_os = "android")]
        mdm_tools::android::init(&config.native_lib_dir, &config.data_dir, &config.cache_dir);
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        let (wake, wake_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let thread = {
            let stop = stop.clone();
            let wake = wake.clone();
            std::thread::Builder::new()
                .name("mdm-core".into())
                .spawn(move || run(config, rx, wake, wake_rx, stop, started_tx))
                .map_err(|e| e.to_string())?
        };
        match started_rx.recv() {
            Ok(Ok(())) => Ok(Core {
                jobs,
                wake,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("the core stopped while starting".into()),
        }
    }

    /// One request from the interface (JSON, `{"op": …}`), answered in JSON
    /// ([`crate::app::App::android_request`]).
    pub fn request(&self, json: &str) -> String {
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1);
        let json = json.to_owned();
        let job: Job = Box::new(move |app, ctx| {
            let _ = tx.send(app.android_request(&json, ctx));
        });
        if self.jobs.send(job).is_err() {
            return error("stopped", "the core has stopped");
        }
        let _ = self.wake.try_send(());
        rx.recv_timeout(ANSWER_WAIT)
            .unwrap_or_else(|_| error("timeout", "the core didn't answer in time"))
    }

    /// Save the list and stop every download where it is (as Exit does); returns once done.
    pub fn stop(mut self) {
        self.stop_now();
    }

    fn stop_now(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.wake.try_send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.stop_now();
    }
}

/// A JSON error answer.
pub fn error(code: &str, text: &str) -> String {
    mdm_ipc::api::line(&mdm_ipc::api::ErrorAnswer {
        error: text.into(),
        code: Some(code.into()),
    })
}

/// The core's thread: `headless::main`'s loop, with the interface's requests done between
/// passes and a snapshot sent whenever the list changed.
fn run(
    config: Config,
    jobs: Receiver<Job>,
    wake: SyncSender<()>,
    wake_rx: Receiver<()>,
    stop: Arc<AtomicBool>,
    started: Sender<Result<(), String>>,
) {
    let Some(data) = mdm_ipc::data_dir() else {
        let _ = started.send(Err("no data folder".into()));
        return;
    };
    let Some(_instance) = mdm_ipc::instance_lock() else {
        let _ = started.send(Err("another core uses the data folder".into()));
        return;
    };
    mdm_crash::logs::init(&data, mdm_crash::logs::APP);
    crate::clock::start();
    // Links added here go as this phone's browser (its WebView) would ask for them.
    mdm_engine::agent::set_automatic(None);
    tracing::info!(
        "Mister Download Manager {} ({}) starting on Android",
        crate::help::VERSION,
        crate::help::COMMIT
    );
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = started.send(Err(format!("no async runtime: {e}")));
            return;
        }
    };
    let (tx, commands) = std::sync::mpsc::channel();
    let to_core = wake.clone();
    let sink = mdm_ipc::Sink::new(
        tx,
        Arc::new(move || {
            let _ = to_core.try_send(());
        }),
    );
    let mut loaded = crate::store::load();
    let first_run = loaded.items.is_empty() && !loaded.settings.language_asked;
    let s = &mut loaded.settings;
    // Nobody at a desktop: the interface asks what it needs, and no egui window ever opens.
    s.language_asked = true;
    s.show_start_dialog = false;
    s.show_complete = false;
    s.show_progress = false;
    s.drop_target = false;
    s.clipboard_autostart = false;
    s.update_check = false;
    if first_run {
        if let Some(lang) = &config.language {
            s.language = closest_language(lang);
        }
        // A phone's connection: fewer at once than a desktop's 8 by default.
        s.max_connections = s.max_connections.min(8);
    }
    if let Some(ua) = config.user_agent.as_deref().filter(|u| !u.is_empty())
        && s.user_agent.is_empty()
    {
        s.user_agent = ua.to_owned();
    }
    let ctx = egui::Context::default();
    let mut app = crate::app::App::new(
        &ctx,
        rt.handle().clone(),
        commands,
        sink,
        loaded,
        crate::Start::Shown,
    )
    .headless(Some(config.downloads_dir.clone()))
    .android();
    let _ = started.send(Ok(()));
    let mut last = String::new();
    let mut sent = Instant::now() - Duration::from_secs(10);
    loop {
        while let Ok(job) = jobs.try_recv() {
            job(&mut app, &ctx);
        }
        let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| app.tick(ctx));
        let delay = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::from_secs(1), |v| v.repaint_delay);
        // A pass draws nothing: its textures are dropped, not uploaded anywhere.
        out.textures_delta.clear();
        // At most 4 snapshots a second, and only when something changed.
        if sent.elapsed() >= Duration::from_millis(250) {
            let now = app.android_snapshot();
            if now != last {
                if let Some(h) = host() {
                    h.changed(&now);
                }
                last = now;
                sent = Instant::now();
            }
        }
        if stop.load(Ordering::SeqCst) || app.wants_exit() {
            break;
        }
        let _ =
            wake_rx.recv_timeout(delay.clamp(Duration::from_millis(100), Duration::from_secs(1)));
    }
    tracing::info!("stopping: the list is saved");
    drop(app);
    mdm_crash::logs::flush(Duration::from_secs(2));
}

/// The interface language closest to the system's `tag` ("pt-BR", "zh-Hant-TW", "fr"), as a
/// catalog's code; None when no catalog fits (English then).
pub fn closest_language(tag: &str) -> Option<String> {
    let i = crate::i18n::i18n();
    let tag = tag.to_ascii_lowercase().replace('_', "-");
    let mut parts = tag.split('-');
    let lang = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    let mut tries = vec![tag.clone()];
    if lang == "zh" {
        let traditional = rest
            .iter()
            .any(|p| matches!(*p, "hant" | "tw" | "hk" | "mo"));
        tries.push(if traditional { "zh-hant" } else { "zh-hans" }.to_owned());
    }
    if let Some(region) = rest.first() {
        tries.push(format!("{lang}-{region}"));
    }
    tries.push(lang.to_owned());
    tries
        .iter()
        .find_map(|t| i.from_code(t))
        .map(|l| i.code(l).to_owned())
}

// The platform calls `unix.rs` re-exports on Android.

/// The default browser, for the User-Agent of links added by hand: none here (the interface
/// gives its WebView's instead, [`Config::user_agent`]).
pub fn default_browser() -> Option<mdm_engine::agent::Found> {
    None
}

/// A file type's icon: the interface draws its own.
pub fn file_icon(_what: &crate::fileicons::What, _px: u32) -> Option<crate::fileicons::Native> {
    None
}

/// The Scheduler's start time, as an exact alarm (the interface's).
pub fn wake_at(at: Option<i64>) -> Result<(), String> {
    match host() {
        Some(h) => h.wake_at(at),
        None => Err("not started".into()),
    }
}

/// A wake lock and a Wi-Fi lock while downloads run (the interface's).
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

//! Crash and freeze reports, the app's side (mdm-crash has the rest). At start MDM launches its
//! monitor (itself with `--crash-monitor`), which writes a minidump when MDM:
//! - crashes natively (win32.rs / unix.rs catch it through crash-handler),
//! - panics (the hook here sends the message, then asks for the dump),
//! - or freezes: a watchdog thread sees a frame of the window run past [`FREEZE_AFTER`] and asks
//!   for a dump of every thread while MDM is still stuck (a deadlock, an endless loop).
//!
//! The monitor then shows the report window (crashui.rs). Without a monitor (it couldn't start)
//! MDM runs on, and a panic still leaves a text report.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use mdm_crash::client::Reporter;
use mdm_crash::protocol::{AppState, Freeze, Panic, Session};

/// A frame running this long is a freeze (Windows calls a window "Not responding" after 5 s).
pub const FREEZE_AFTER: Duration = Duration::from_secs(8);

static REPORTER: OnceLock<Arc<Reporter>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// When the current frame started (ms since EPOCH, + 1); 0 between frames.
static FRAME_START: AtomicU64 = AtomicU64::new(0);
/// A freeze was reported and the window hasn't answered since.
static FROZEN: AtomicBool = AtomicBool::new(false);
/// A panic was reported: later ones (a burst on worker threads) are logged, not dumped again.
static PANICKED: AtomicBool = AtomicBool::new(false);
/// The window thread (the dump's id for it; 0 where it can't be known).
static UI_THREAD: OnceLock<u32> = OnceLock::new();

thread_local! {
    /// The windows drawn this frame and last frame (ids), for the log and the report.
    static WINDOWS: RefCell<(BTreeSet<String>, BTreeSet<String>)> =
        RefCell::new(Default::default());
}

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Start the monitor and catch crashes, panics and freezes. Called on the window thread, after
/// this process became MDM's instance.
pub fn start(data: &Path, session: Session) {
    let _ = EPOCH.set(Instant::now());
    #[cfg(windows)]
    let _ = UI_THREAD.set(crate::win32::thread_id());
    #[cfg(unix)]
    let _ = UI_THREAD.set(crate::unix::thread_id());
    let Some(exe) = std::env::current_exe().ok() else {
        return;
    };
    let Some(reporter) = Reporter::start(&exe, data) else {
        return;
    };
    reporter.session(&session);
    let reporter = Arc::new(reporter);
    #[cfg(windows)]
    let attached = crate::win32::attach_crash_handler(reporter.clone());
    #[cfg(unix)]
    let attached = crate::unix::attach_crash_handler(reporter.clone());
    if !attached {
        tracing::warn!("crash reports: native crashes won't be caught");
    }
    let _ = REPORTER.set(reporter);
    hook_panics();
    let _ = std::thread::Builder::new()
        .name("freeze watchdog".into())
        .spawn(watchdog);
    tracing::info!("crash reports on");
}

/// Ask the monitor for a dump of every thread now.
fn dump_now() -> bool {
    #[cfg(windows)]
    return crate::win32::dump_now();
    #[cfg(unix)]
    return crate::unix::dump_now();
}

/// A panic: its message and Rust's backtrace to the monitor, then a dump; then the default hook
/// (the message on stderr) and the panic goes on (on the window thread it ends MDM).
fn hook_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let p = Panic {
            message: info
                .payload_as_str()
                .unwrap_or("(a panic without a message)")
                .to_owned(),
            location: info.location().map(|l| l.to_string()).unwrap_or_default(),
            thread: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned(),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        tracing::error!("panic: {} at {} ({})", p.message, p.location, p.thread);
        if let Some(r) = REPORTER.get()
            && !PANICKED.swap(true, Ordering::AcqRel)
        {
            r.panic(&p);
            dump_now();
        }
        default(info);
    }));
}

/// The window starts a frame (top of `App::ui`).
pub fn frame_begin() {
    FRAME_START.store(now_ms() + 1, Ordering::Release);
}

/// The window finished a frame: no freeze; windows opened or closed since the last are logged.
pub fn frame_end() {
    FRAME_START.store(0, Ordering::Release);
    if FROZEN.swap(false, Ordering::AcqRel) {
        tracing::warn!("the window answers again");
        if let Some(r) = REPORTER.get() {
            r.recovered();
        }
    }
    WINDOWS.with(|w| {
        let mut w = w.borrow_mut();
        let (now, last) = &mut *w;
        if now != last {
            for opened in now.difference(last) {
                tracing::info!("window opened: {opened}");
            }
            for closed in last.difference(now) {
                tracing::info!("window closed: {closed}");
            }
        }
        *last = std::mem::take(now);
    });
}

/// A window is drawn this frame (dialogs::dialog_window, the progress windows…).
pub fn window(id: &str) {
    WINDOWS.with(|w| {
        let mut w = w.borrow_mut();
        if !w.0.contains(id) {
            w.0.insert(id.to_owned());
        }
    });
}

/// What MDM has open and running, for the report (sent when it changes).
pub fn state(downloads: usize, running: usize, queues: usize, scale: f32) {
    let Some(r) = REPORTER.get() else { return };
    let windows = WINDOWS.with(|w| w.borrow().1.iter().cloned().collect());
    r.state(&AppState {
        downloads,
        running,
        queues,
        windows,
        scale,
    });
}

/// MDM is closing normally: the monitor leaves without a report.
pub fn clean_exit() {
    if let Some(r) = REPORTER.get() {
        tracing::info!("closing");
        r.clean();
    }
}

/// Watch the window's frames: one running past FREEZE_AFTER is reported once. Time the computer
/// spent asleep (this loop's own tick much longer than asked) isn't counted.
fn watchdog() {
    let tick = Duration::from_millis(500);
    let mut last = now_ms();
    // Frames that started before this are past a sleep: judged from `awake_from` instead.
    let mut awake_from = 0u64;
    let mut reported_frame = 0u64;
    loop {
        std::thread::sleep(tick);
        let now = now_ms();
        if now.saturating_sub(last) > 3_000 {
            awake_from = now;
        }
        last = now;
        let start = FRAME_START.load(Ordering::Acquire);
        if start == 0 || start == reported_frame {
            continue;
        }
        let since = (start - 1).max(awake_from);
        let ran = now.saturating_sub(since);
        if ran < FREEZE_AFTER.as_millis() as u64 {
            continue;
        }
        reported_frame = start;
        FROZEN.store(true, Ordering::Release);
        tracing::error!(
            "the window stopped responding ({} s into a frame)",
            ran / 1000
        );
        if let Some(r) = REPORTER.get() {
            r.freeze(&Freeze {
                ui_thread: UI_THREAD.get().copied().unwrap_or(0),
                main: true,
                seconds: ran / 1000,
            });
            dump_now();
        }
    }
}

/// The session's description for the monitor.
pub fn session(language: &str, dark: bool) -> Session {
    #[cfg(windows)]
    let os_version = crate::win32::windows_version();
    #[cfg(unix)]
    let os_version = crate::unix::os_version();
    Session {
        version: crate::help::VERSION.to_owned(),
        commit: crate::help::COMMIT.to_owned(),
        install: mdm_tools::update::Install::detect().label().to_owned(),
        os: if cfg!(windows) {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else {
            "Linux"
        }
        .to_owned(),
        os_version,
        arch: std::env::consts::ARCH.to_owned(),
        language: language.to_owned(),
        dark,
        pid: std::process::id(),
        exe: std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        started: chrono::Utc::now().timestamp(),
    }
}

/// MDM_CRASH_TEST=1 lets `/show crash-test:<what>` crash MDM on purpose (the crash reports'
/// own end-to-end check): panic, crash (an access violation / SIGSEGV), freeze (a deadlock on
/// the window thread), abort.
pub fn test(what: &str) {
    if std::env::var_os("MDM_CRASH_TEST").is_none_or(|v| v != "1") {
        return;
    }
    tracing::warn!("crash test: {what}");
    match what {
        "panic" => panic!("crash test: a panic on purpose"),
        "crash" => {
            #[cfg(windows)]
            crate::win32::crash_now();
            #[cfg(unix)]
            crate::unix::crash_now();
        }
        "freeze" => crash_test_freeze(),
        "abort" => std::process::abort(),
        _ => {}
    }
}

/// The window thread waits on itself, as 0.2.2's Add URL did.
#[inline(never)]
fn crash_test_freeze() {
    let lock = std::sync::Mutex::new(());
    let _held = lock.lock();
    let _never = lock.lock();
}

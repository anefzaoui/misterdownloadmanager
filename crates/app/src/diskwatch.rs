//! The window thread never waits on the disk (#1292). An antivirus that checks every file a
//! program opens (Bitdefender's `bdhkm64.dll` and `atcuf64.dll`) held a file open made on the
//! window's thread for 8 s, and the window froze meanwhile: a file is opened, listed, renamed or
//! deleted beside the window, and the window takes what was found once it is there.
//!
//! This keeps it so, from the window's first frame on (what the start reads before is the
//! start's):
//! - Debug builds, and any build with `MDM_DISK_WATCH=1`: every file call the program makes is
//!   seen here ([`call`]), and one made on the window's thread is logged once per file, with
//!   the calls that led to it. Windows sees them through the program's own import table
//!   (`win32::watch_disk`). Linux and macOS have no such table to watch (their programs call the
//!   C library, whose calls can't be watched from inside without a debugger): there `strace -f`
//!   on the test machine shows the window thread's calls (the verify skill says how).
//! - Off-screen test copies, `MDM_TEST_SLOW_DISK=<ms>`: every file call, on any thread, waits
//!   that long first, as such an antivirus makes it wait. The window must go on drawing.
//! - Off-screen test copies, `MDM_TEST_FRAME_STATS=1`: every 10 s the log says how long the
//!   window's frames took (each frame's logic and UI, timed as the freeze watchdog times them).

// Linux and macOS: no watch hands calls in ([`call`] and what it uses are Windows' alone).
#![cfg_attr(not(windows), allow(dead_code))]

use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

thread_local! {
    /// This thread is the window's ([`start`]).
    static WINDOW: Cell<bool> = const { Cell::new(false) };
    /// A call is being looked at on this thread: the logging's own calls aren't looked at again.
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// From the window's first frame on ([`first_frame`]).
static ARMED: AtomicBool = AtomicBool::new(false);
/// The window thread's file calls are logged ([`start`]).
static LOGGED: AtomicBool = AtomicBool::new(false);
/// How long each file call waits first (ms; `MDM_TEST_SLOW_DISK`), 0: not at all.
static SLOW: AtomicU64 = AtomicU64::new(0);
/// The files already logged (each once).
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// How many file calls the window thread made since it was armed (the tests' and the log's count).
static ON_WINDOW: AtomicU64 = AtomicU64::new(0);

/// Whether the watch runs in this process: debug builds, `MDM_DISK_WATCH=1`, or a test copy's
/// slow disk.
fn wanted() -> bool {
    cfg!(debug_assertions)
        || std::env::var_os("MDM_DISK_WATCH").is_some_and(|v| v == "1")
        || slow_ms() > 0
}

/// `MDM_TEST_SLOW_DISK`, in an off-screen test copy only.
fn slow_ms() -> u64 {
    if !crate::offscreen() {
        return 0;
    }
    std::env::var("MDM_TEST_SLOW_DISK")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

/// Start watching (lib.rs, on the window's thread, before the window opens): that thread is the
/// window's from now on.
pub fn start() {
    WINDOW.with(|w| w.set(true));
    if !wanted() {
        return;
    }
    LOGGED.store(true, Ordering::Relaxed);
    let slow = slow_ms();
    SLOW.store(slow, Ordering::Relaxed);
    #[cfg(windows)]
    let watched = crate::win32::watch_disk();
    // Linux and macOS: see the module's notes (strace on the test machine).
    #[cfg(not(windows))]
    let watched = false;
    if slow > 0 {
        tracing::warn!("disk test: every file call waits {slow} ms from the first frame on");
    }
    tracing::info!(
        "disk watch: {}",
        if watched {
            "the window thread's file calls are logged"
        } else {
            "not on this system"
        }
    );
}

/// The window's first frame was drawn (renderer.rs): its file calls count from now on.
pub fn first_frame() {
    ARMED.store(true, Ordering::Release);
}

/// Whether the calling thread is the window's ([`start`]).
pub fn on_window_thread() -> bool {
    WINDOW.try_with(Cell::get).unwrap_or(false)
}

/// How many file calls the window thread made since its first frame (logged ones and repeats).
pub fn window_calls() -> u64 {
    ON_WINDOW.load(Ordering::Relaxed)
}

/// A file call (`what`: open, list, rename…) on `path`, about to be made on the calling thread
/// (win32.rs's watch). Waits first for a test's slow disk; logs it once per file when the
/// window's thread makes it.
pub fn call(what: &str, path: &dyn Fn() -> String) {
    if !ARMED.load(Ordering::Acquire) || INSIDE.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    let _ = INSIDE.try_with(|i| i.set(true));
    let slow = SLOW.load(Ordering::Relaxed);
    if slow > 0 {
        std::thread::sleep(Duration::from_millis(slow));
    }
    if LOGGED.load(Ordering::Relaxed) && on_window_thread() {
        ON_WINDOW.fetch_add(1, Ordering::Relaxed);
        let path = path();
        let first = SEEN.lock().is_ok_and(|mut seen| {
            let key = format!("{what} {path}");
            let new = !seen.contains(&key);
            if new && seen.len() < 500 {
                seen.push(key);
            }
            new
        });
        if first {
            tracing::warn!(
                "disk watch: the window thread made a file call ({what} {path}), from:\n{}",
                our_frames(&std::backtrace::Backtrace::force_capture().to_string())
            );
        }
    }
    let _ = INSIDE.try_with(|i| i.set(false));
}

/// A backtrace's lines that are MDM's own (its crates' frames and their places), at most 24: the
/// calls that led to a file call, without the system's and the watch's own.
fn our_frames(trace: &str) -> String {
    let lines: Vec<&str> = trace.lines().collect();
    let mut out = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let ours = ["misterdownloadmanager::", "mdm_"]
            .iter()
            .any(|p| t.split_once(": ").is_some_and(|(_, f)| f.starts_with(p)));
        if !ours || t.contains("diskwatch::") || t.contains("win32::watch") {
            continue;
        }
        out.push(t.to_owned());
        if let Some(at) = lines.get(n + 1).map(|l| l.trim_start())
            && at.starts_with("at ")
        {
            out.push(format!("    {at}"));
        }
        if out.len() >= 48 {
            break;
        }
    }
    if out.is_empty() {
        "  (no frames of MDM's: a debug build names them)".to_owned()
    } else {
        out.iter()
            .map(|l| format!("  {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The frame times since the last summary ([`stretch`]).
struct Frames {
    since: Instant,
    count: u64,
    total_ms: u64,
    slowest: u64,
    over_50: u64,
    over_500: u64,
}

static FRAMES: Mutex<Option<Frames>> = Mutex::new(None);

/// How often [`stretch`] sums the frames up.
const STATS_EVERY: Duration = Duration::from_secs(10);

/// A frame's logic or UI took `ms` (crash.rs, as each ends). Off-screen test copies with
/// `MDM_TEST_FRAME_STATS=1` log a summary every [`STATS_EVERY`].
pub fn stretch(ms: u64) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| {
        crate::offscreen() && std::env::var_os("MDM_TEST_FRAME_STATS").is_some_and(|v| v == "1")
    }) {
        return;
    }
    let Ok(mut frames) = FRAMES.lock() else {
        return;
    };
    let f = frames.get_or_insert_with(|| Frames {
        since: Instant::now(),
        count: 0,
        total_ms: 0,
        slowest: 0,
        over_50: 0,
        over_500: 0,
    });
    f.count += 1;
    f.total_ms += ms;
    f.slowest = f.slowest.max(ms);
    f.over_50 += u64::from(ms >= 50);
    f.over_500 += u64::from(ms >= 500);
    if f.since.elapsed() >= STATS_EVERY {
        tracing::info!(
            "frame stats: {} stretches in {} ms, mean {:.1} ms, slowest {} ms, {} of 50 ms or more, {} of 500 ms or more; the window thread's file calls so far: {}",
            f.count,
            f.since.elapsed().as_millis(),
            f.total_ms as f64 / f.count as f64,
            f.slowest,
            f.over_50,
            f.over_500,
            window_calls()
        );
        *frames = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calls that led to a file call are MDM's own frames, with their places, and not the
    /// watch's or the system's.
    #[test]
    fn a_backtrace_keeps_mdms_own_frames() {
        let trace = "   0: std::backtrace::Backtrace::force_capture\n             at /rustc/std/src/backtrace.rs:312\n   1: misterdownloadmanager::diskwatch::call\n             at .\\crates\\app\\src\\diskwatch.rs:120\n   2: misterdownloadmanager::win32::watch::create_file\n   3: std::sys::fs::windows::File::open_native\n   4: misterdownloadmanager::theme::read_font\n             at .\\crates\\app\\src\\theme.rs:724\n   5: mdm_engine::x\n   6: kernel32!BaseThreadInitThunk\n";
        let ours = our_frames(trace);
        assert!(ours.contains("theme::read_font"), "{ours}");
        assert!(ours.contains("theme.rs:724"), "{ours}");
        assert!(ours.contains("mdm_engine::x"), "{ours}");
        assert!(
            !ours.contains("diskwatch") && !ours.contains("open_native"),
            "{ours}"
        );
        assert!(our_frames("   0: ntdll!x\n").contains("no frames"));
    }

    /// A call before the first frame, or on another thread, is neither waited for nor counted.
    #[test]
    fn calls_count_on_the_window_thread_after_its_first_frame_only() {
        let before = window_calls();
        call("open", &|| "x".into());
        assert_eq!(window_calls(), before);
    }
}

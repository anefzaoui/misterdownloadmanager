//! The text of a report, in the manner of Minecraft's: what happened, the stack of the thread that
//! crashed or froze, what MDM was doing, the system, then every other thread and the modules.
//! English on purpose: it is read by whoever fixes the bug, whatever language MDM speaks.
//!
//! Nothing personal on purpose: the home folder shows as `~`, and MDM's state is counts only (no
//! addresses or file names). A panic's message and the log can hold text the user typed; the
//! report window shows all of it before anything is sent.

use std::fmt::Write as _;
use std::path::Path;

use crate::analyze::{Analysis, Frame, Thread};
use crate::protocol::{Kind, Notes};

/// Frames of a thread shown in full; other threads show this many.
const OTHER_THREAD_FRAMES: usize = 12;

/// Everything a report is made from.
#[derive(Clone, Debug, Default)]
pub struct Input<'a> {
    pub kind: Option<Kind>,
    /// When it happened (RFC 3339, UTC).
    pub time: String,
    pub notes: Notes,
    pub analysis: Option<&'a Analysis>,
    pub log_tail: Vec<String>,
    /// The dump's file name, when there is one.
    pub dump: Option<String>,
}

/// One line that changes with each report (Minecraft's has its jokes; ours stay kind).
const SORRY: &[&str] = &[
    "Sorry about that.",
    "That wasn't supposed to happen.",
    "Even download managers have bad days.",
    "This one is on us.",
    "Thank you for sending this; it helps.",
];

pub fn text(input: &Input, home: Option<&Path>) -> String {
    let kind = input.kind.unwrap_or(Kind::Crash);
    let mut s = String::new();
    let _ = writeln!(s, "---- Mister Download Manager crash report ----");
    let n = input.time.bytes().map(usize::from).sum::<usize>();
    let _ = writeln!(s, "// {}", SORRY[n % SORRY.len()]);
    let _ = writeln!(s);
    let _ = writeln!(s, "Time: {}", input.time);
    let _ = writeln!(s, "Description: {}", description(kind, &input.notes));
    let _ = writeln!(s, "Signature: {}", signature(kind, input));
    if let Some(d) = &input.dump {
        let _ = writeln!(s, "Dump: {d}");
    }
    let _ = writeln!(s);

    // What went wrong.
    match kind {
        Kind::Panic => {
            if let Some(p) = &input.notes.panic {
                let _ = writeln!(s, "{}", p.message);
                let _ = writeln!(s, "  at {}", p.location);
                let _ = writeln!(s, "  thread: {}", p.thread);
            }
        }
        Kind::Freeze => {
            if let Some(f) = &input.notes.freeze {
                let _ = writeln!(
                    s,
                    "The window did not answer for {} s: one frame never finished (a deadlock or an endless loop).",
                    f.seconds
                );
            }
            if input.notes.recovered {
                let _ = writeln!(s, "It answered again later.");
            }
        }
        Kind::Crash => {
            if let Some(a) = input.analysis {
                let _ = writeln!(
                    s,
                    "{} at {:#x}",
                    a.reason.as_deref().unwrap_or("Crash"),
                    a.address.unwrap_or(0)
                );
            }
        }
        Kind::Exit => {
            let _ = writeln!(
                s,
                "MDM ended without closing normally, and no crash was caught (it may have been ended from outside, or the system stopped it)."
            );
        }
    }
    let _ = writeln!(s);

    // The thread that matters.
    let key = key_thread(kind, input);
    if let Some(t) = key {
        let what = if kind == Kind::Freeze {
            "Window thread (frozen)"
        } else {
            "Thread that crashed"
        };
        let _ = writeln!(s, "-- {what}: {} --", thread_title(t));
        write_frames(&mut s, &t.frames, usize::MAX);
        let _ = writeln!(s);
    } else if let Some(p) = &input.notes.panic
        && !p.backtrace.trim().is_empty()
    {
        let _ = writeln!(s, "-- Rust backtrace --");
        let _ = writeln!(s, "{}", p.backtrace.trim_end());
        let _ = writeln!(s);
    }

    // What MDM was doing.
    if !input.log_tail.is_empty() {
        let _ = writeln!(s, "-- What MDM was doing (its log) --");
        for l in &input.log_tail {
            let _ = writeln!(s, "{l}");
        }
        let _ = writeln!(s);
    }

    // The system.
    let _ = writeln!(s, "-- System details --");
    if let Some(ss) = &input.notes.session {
        let _ = writeln!(s, "MDM: {} ({})", ss.version, ss.commit);
        let _ = writeln!(s, "Installed with: {}", ss.install);
        let _ = writeln!(s, "System: {} ({})", ss.os_version, ss.arch);
        let _ = writeln!(s, "Language: {}", ss.language);
        let _ = writeln!(s, "Theme: {}", if ss.dark { "dark" } else { "light" });
        let _ = writeln!(s, "Program: {}", ss.exe);
        if let Ok(now) = chrono::DateTime::parse_from_rfc3339(&input.time) {
            let up = now.timestamp() - ss.started;
            if up >= 0 {
                let _ = writeln!(s, "Running for: {}", duration(up as u64));
            }
        }
    }
    if let Some(a) = input.analysis {
        let _ = writeln!(
            s,
            "CPU: {} {} ({} cores)",
            a.cpu,
            a.cpu_info.as_deref().unwrap_or(""),
            a.cpu_count
        );
        let _ = writeln!(
            s,
            "OS (dump): {} {}",
            a.os,
            a.os_version.as_deref().unwrap_or("")
        );
    }
    if let Some(st) = &input.notes.state {
        let _ = writeln!(
            s,
            "Downloads: {} ({} running), queues: {}",
            st.downloads, st.running, st.queues
        );
        let _ = writeln!(s, "Windows open: {}", st.windows.join(", "));
        let _ = writeln!(s, "Display scale: {}", st.scale);
    }
    if let Some(a) = input.analysis
        && !a.missing_symbols.is_empty()
    {
        let _ = writeln!(s, "No symbols for: {}", a.missing_symbols.join(", "));
    }
    let _ = writeln!(s);

    // Every other thread, then the modules.
    if let Some(a) = input.analysis {
        for t in &a.threads {
            if key.is_some_and(|k| k.id == t.id) {
                continue;
            }
            let _ = writeln!(s, "-- Thread {} --", thread_title(t));
            write_frames(&mut s, &t.frames, OTHER_THREAD_FRAMES);
            let _ = writeln!(s);
        }
        let _ = writeln!(s, "-- Modules --");
        for m in &a.modules {
            let _ = writeln!(
                s,
                "{:#014x}  {:<32} {:<16} {}",
                m.base,
                m.name,
                m.version.as_deref().unwrap_or(""),
                m.debug_id.as_deref().unwrap_or("")
            );
        }
    }
    scrub(&s, home)
}

/// A report made and saved.
#[derive(Clone, Debug)]
pub struct Made {
    pub kind: Kind,
    pub text: String,
    pub signature: String,
    /// The `.txt` beside the dump (or alone, with no dump).
    pub path: std::path::PathBuf,
    pub dump: Option<std::path::PathBuf>,
    pub analysis: Option<Analysis>,
    pub notes: Notes,
}

/// Read `dump` (none for an exit nothing caught), write the report beside it in `data`'s crashes
/// folder, and return it. `symbols`: Breakpad symbol stores to try (the release's, once fetched).
pub fn make(
    dump: Option<&Path>,
    kind: Kind,
    notes: Notes,
    data: &Path,
    symbols: &[std::path::PathBuf],
) -> Made {
    let analysis = dump.and_then(|d| match crate::analyze::analyze(d, symbols) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!("crash report: the dump couldn't be read: {e}");
            None
        }
    });
    let input = Input {
        kind: Some(kind),
        time: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        notes: notes.clone(),
        analysis: analysis.as_ref(),
        log_tail: crate::logs::tail(data, 150),
        dump: dump
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned()),
    };
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from);
    let text = text(&input, home.as_deref());
    let signature = signature(kind, &input);
    let path = match dump {
        Some(d) => d.with_extension("txt"),
        None => {
            crate::monitor::crashes_dir(data).join(format!("{}.txt", crate::monitor::stem(kind)))
        }
    };
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(&path, &text);
    Made {
        kind,
        text,
        signature,
        path,
        dump: dump.map(Path::to_path_buf),
        analysis,
        notes,
    }
}

/// The kind in a sentence.
pub fn description(kind: Kind, notes: &Notes) -> &'static str {
    match kind {
        Kind::Crash => "MDM crashed",
        Kind::Panic => "MDM hit an internal error and closed",
        Kind::Freeze if notes.recovered => "The window stopped responding (it recovered)",
        Kind::Freeze => "The window stopped responding",
        Kind::Exit => "MDM closed unexpectedly",
    }
}

/// A short, stable name for the problem, the same for every report of it (it finds the issue
/// already filed for it): the kind and the first of MDM's own functions on the stack that
/// matters ("freeze in misterdownloadmanager::winui::combo_edit").
pub fn signature(kind: Kind, input: &Input) -> String {
    let main_module = input
        .analysis
        .and_then(|a| a.modules.first())
        .map(|m| m.name.as_str());
    let ours = key_thread(kind, input).and_then(|t| {
        // A panic's stack starts in the panic hook (the reporter asking for the dump): what
        // panicked is below the last frame of Rust's panic machinery.
        // (Not the last such frame: every stack ends in start-up's std::panicking::catch_unwind.)
        let name = |i: usize| t.frames[i].function.as_deref().map(trim_function);
        let below = if kind == Kind::Panic {
            let entry = (0..t.frames.len()).find(|&i| {
                name(i).is_some_and(|n| {
                    n.starts_with("core::panicking")
                        || n.starts_with("std::panicking::begin_panic")
                        || n.contains("rust_begin_unwind")
                })
            });
            entry.map_or(0, |e| {
                (e..t.frames.len())
                    .find(|&i| {
                        !name(i).is_some_and(|n| n.starts_with("std::") || n.starts_with("core::"))
                    })
                    .unwrap_or(e)
            })
        } else {
            0
        };
        first_own_frame(&t.frames[below.min(t.frames.len())..], main_module)
    });
    let place = match (ours, &input.notes.panic) {
        (Some(f), _) => f,
        // No symbols: a panic still says where.
        (None, Some(p)) => strip_column(&p.location),
        (None, None) => key_thread(kind, input)
            .and_then(|t| t.frames.first())
            .map(|f| format!("{}+{:#x}", f.module.as_deref().unwrap_or("?"), f.offset))
            .unwrap_or_else(|| "unknown".to_owned()),
    };
    format!("{} in {place}", kind.word())
}

/// The thread a report is about: the window thread for a freeze, else the one the dump was
/// taken for.
fn key_thread<'a>(kind: Kind, input: &Input<'a>) -> Option<&'a Thread> {
    let a = input.analysis?;
    if kind == Kind::Freeze {
        let f = input.notes.freeze.as_ref()?;
        return a
            .thread(f.ui_thread)
            .filter(|_| f.ui_thread != 0)
            .or_else(|| f.main.then(|| a.threads.first()).flatten());
    }
    a.thread(a.requesting?)
}

/// The first frame in the program's own code, past the system, Rust's standard library, locks,
/// panics and the crash reporter itself: a function of the program's own crates first (named as
/// its main module, the dump's first: "misterdownloadmanager", or MDM's `mdm_*` crates), else
/// of any crate linked into it (egui, winit…).
fn first_own_frame(frames: &[Frame], main_module: Option<&str>) -> Option<String> {
    let own_crate = main_module
        .map(|m| m.split('.').next().unwrap_or(m).replace('-', "_"))
        .unwrap_or_else(|| "misterdownloadmanager".to_owned());
    let paths: Vec<String> = frames
        .iter()
        .filter(|f| main_module.is_none_or(|m| f.module.as_deref() == Some(m)))
        .filter_map(|f| f.function.as_deref())
        .map(trim_function)
        .filter(|p| is_code(p))
        .collect();
    let crate_of = |p: &str| p.split("::").next().unwrap_or("").to_owned();
    paths
        .iter()
        .find(|p| {
            let c = crate_of(p);
            c == own_crate || (c.starts_with("mdm_") && c != "mdm_crash")
        })
        .or_else(|| paths.first())
        .cloned()
}

/// A Rust function path outside the machinery (not a C symbol, not std, locks or panics).
fn is_code(path: &str) -> bool {
    let machinery = [
        "std::",
        "core::",
        "alloc::",
        "parking_lot",
        "lock_api",
        "mdm_crash::",
        "crash_handler",
        "minidumper",
        "backtrace",
        "__rust",
        "rust_begin",
        "rust_panic",
    ];
    path.contains("::") && !machinery.iter().any(|m| path.starts_with(m))
}

/// A function name without its generic arguments and closure numbers, so the same place reads
/// the same in every build.
fn trim_function(f: &str) -> String {
    // Its parameters too ("crashy::frozen(std::sync::Mutex<…>*)": Windows' symbols carry them).
    let f = f.split('(').next().unwrap_or(f);
    let f = f.trim_start_matches(['<', '&']);
    let mut out = String::new();
    let mut depth = 0;
    for c in f.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split("::{{closure}}")
        .next()
        .unwrap_or(&out)
        .trim_end_matches("::")
        .to_owned()
}

fn strip_column(location: &str) -> String {
    // "src/a.rs:12:5" → "src/a.rs:12"
    match location.rsplit_once(':') {
        Some((head, col)) if col.chars().all(|c| c.is_ascii_digit()) && head.contains(':') => {
            head.to_owned()
        }
        _ => location.to_owned(),
    }
}

fn thread_title(t: &Thread) -> String {
    match &t.name {
        Some(n) => format!("{n} ({})", t.id),
        None => t.id.to_string(),
    }
}

fn write_frames(s: &mut String, frames: &[Frame], max: usize) {
    for (i, f) in frames.iter().take(max).enumerate() {
        let module = f.module.as_deref().unwrap_or("?");
        match &f.function {
            Some(func) => {
                let _ = write!(s, "{i:>3}: {func}");
                if let Some(file) = &f.file {
                    let _ = write!(
                        s,
                        "\n       at {}:{}",
                        short_path(file),
                        f.line.unwrap_or(0)
                    );
                }
                let _ = writeln!(s);
            }
            None => {
                let _ = writeln!(s, "{i:>3}: {module} + {:#x}", f.offset);
            }
        }
    }
    if frames.len() > max {
        let _ = writeln!(s, "     … {} more", frames.len() - max);
    }
}

/// A source path from where it means something: `crates/app/src/winui.rs` rather than the build
/// machine's checkout, `library/std/src/…` for the standard library, a crate's own path under
/// the cargo registry.
fn short_path(p: &str) -> String {
    let p = p.replace('\\', "/");
    for mark in ["/crates/", "/library/", "/registry/src/", "/git/checkouts/"] {
        if let Some(i) = p.find(mark) {
            return p[i + 1..].to_owned();
        }
    }
    p
}

fn duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min {} s", s / 60, s % 60),
        s => format!("{} h {} min", s / 3600, s % 3600 / 60),
    }
}

/// The home folder as `~` (in paths of either kind of separator).
pub fn scrub(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home.map(|h| h.display().to_string()) else {
        return text.to_owned();
    };
    if home.len() < 3 {
        return text.to_owned();
    }
    let mut out = text.replace(&home, "~");
    let flipped = home.replace('\\', "/");
    if flipped != home {
        out = out.replace(&flipped, "~");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Freeze, Panic, Session};

    fn frame(f: &str) -> Frame {
        Frame {
            function: Some(f.to_owned()),
            file: Some(r"D:\build\mdm\crates\app\src\winui.rs".to_owned()),
            line: Some(2088),
            module: Some("misterdownloadmanager.exe".to_owned()),
            offset: 0x1234,
        }
    }

    fn frozen() -> Analysis {
        Analysis {
            requesting: Some(2),
            threads: vec![
                Thread {
                    id: 1,
                    name: Some("main".into()),
                    frames: vec![
                        frame("parking_lot::raw_rwlock::RawRwLock::lock_exclusive_slow"),
                        frame("egui::context::Context::write<egui::memory::Memory>"),
                        frame("misterdownloadmanager::winui::combo_edit::{{closure}}"),
                        frame("misterdownloadmanager::dialogs::AddUrl::show"),
                    ],
                },
                Thread {
                    id: 2,
                    name: Some("watchdog".into()),
                    frames: vec![frame("mdm_crash::client::dump")],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_freeze_names_the_window_threads_own_frame() {
        let a = frozen();
        let input = Input {
            kind: Some(Kind::Freeze),
            time: "2026-11-01T04:00:00+00:00".into(),
            notes: Notes {
                freeze: Some(Freeze {
                    ui_thread: 1,
                    main: false,
                    seconds: 9,
                }),
                session: Some(Session {
                    version: "0.2.3".into(),
                    exe: r"C:\Users\Someone\AppData\Local\MDM\mdm.exe".into(),
                    started: 1_793_505_000,
                    ..Default::default()
                }),
                ..Default::default()
            },
            analysis: Some(&a),
            ..Default::default()
        };
        assert_eq!(
            signature(Kind::Freeze, &input),
            "freeze in misterdownloadmanager::winui::combo_edit"
        );
        let t = text(&input, Some(Path::new(r"C:\Users\Someone")));
        assert!(t.contains("-- Window thread (frozen): main (1) --"));
        assert!(t.contains("at crates/app/src/winui.rs:2088"));
        assert!(t.contains(r"Program: ~\AppData\Local\MDM\mdm.exe"));
        assert!(!t.contains("Someone"));
        // The watchdog's thread is listed among the others.
        assert!(t.contains("-- Thread watchdog (2) --"));
    }

    #[test]
    fn a_panic_without_symbols_still_says_where() {
        let input = Input {
            kind: Some(Kind::Panic),
            notes: Notes {
                panic: Some(Panic {
                    message: "index out of bounds".into(),
                    location: "crates/app/src/list.rs:120:9".into(),
                    thread: "main".into(),
                    backtrace: String::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            signature(Kind::Panic, &input),
            "panic in crates/app/src/list.rs:120"
        );
    }

    #[test]
    fn function_names_lose_generics_and_closures() {
        assert_eq!(
            trim_function("misterdownloadmanager::winui::combo_edit::{{closure}}"),
            "misterdownloadmanager::winui::combo_edit"
        );
        assert_eq!(
            trim_function("mdm_engine::job::Job<alloc::string::String>::run"),
            "mdm_engine::job::Job::run"
        );
    }
}

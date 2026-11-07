//! Other programs MDM starts and waits for, and the files and pages it hands to the system,
//! never waited for on the window's thread (#222): a frame that waits for a program freezes the
//! window for as long as that program takes, and a program can take forever (a file manager
//! that never answers over D-Bus froze the window until the freeze report caught it). So a
//! window starts them here, on a thread of their own, each with a time limit, and goes on.

use std::ffi::OsStr;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Run `cmd` quietly on a thread of its own, waiting at most `limit` for it; when it can't
/// start, fails or runs out of time (it is then ended), `fallback` runs, on that thread.
/// Returns at once.
// Windows' callers start their programs (Explorer) without waiting for them.
#[cfg_attr(windows, allow(dead_code))]
pub fn run_detached_with_fallback(
    mut cmd: Command,
    limit: Duration,
    fallback: impl FnOnce() + Send + 'static,
) {
    let name = program_name(&cmd);
    let started = std::thread::Builder::new()
        .name("launch".into())
        .spawn(move || {
            if !run_within(&mut cmd, limit) {
                fallback();
            }
        });
    if let Err(e) = started {
        tracing::warn!("{name}: no thread to run it on: {e}");
    }
}

/// Hand `target` (a file, a folder or an address) to the system's program for it, on a thread
/// of its own: the `open` crate waits for the launcher it starts (up to 250 ms each on Windows
/// and Linux, until it exits on macOS).
pub fn open(target: impl AsRef<OsStr>) {
    let target = target.as_ref().to_owned();
    let started = std::thread::Builder::new()
        .name("launch".into())
        .spawn(move || {
            let _ = open::that_detached(&target);
        });
    if let Err(e) = started {
        tracing::warn!("open: no thread to run it on: {e}");
    }
}

/// Run `cmd` quietly, waiting at most `limit` for it: true when it ran and succeeded. One that
/// runs out of time is ended. For a thread other than the window's.
pub fn run_within(cmd: &mut Command, limit: Duration) -> bool {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => wait_within(&mut child, limit).is_some_and(|s| s.success()),
        Err(e) => {
            tracing::debug!("{}: {e}", program_name(cmd));
            false
        }
    }
}

/// What `cmd` writes (its output, its exit status), waiting at most `limit` for it; None when it
/// couldn't start or ran out of time (it is then ended). Its errors are dropped.
// Windows reads what these ask (versions, settings) from the registry, without a program.
#[cfg_attr(windows, allow(dead_code))]
pub fn output_within(cmd: &mut Command, limit: Duration) -> Option<Output> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Read as it writes, so a long output can't fill the pipe and stall it; a program it
    // started that keeps the pipe open can't hold this caller either (the reader is let go).
    let mut pipe = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("launch-output".into())
        .spawn(move || {
            let mut out = Vec::new();
            let _ = pipe.read_to_end(&mut out);
            let _ = tx.send(out);
        })
        .ok()?;
    let start = Instant::now();
    let status = wait_within(&mut child, limit)?;
    let left = limit.saturating_sub(start.elapsed()) + Duration::from_millis(100);
    let stdout = rx.recv_timeout(left).ok()?;
    Some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

/// Wait for `child` at most `limit`: its exit status, or None when it ran out of time (it is
/// ended and reaped) or couldn't be waited for.
fn wait_within(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if start.elapsed() < limit => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                tracing::warn!("a program still running after {limit:?} was ended");
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(e) => {
                tracing::warn!("a program couldn't be waited for: {e}");
                return None;
            }
        }
    }
}

fn program_name(cmd: &Command) -> String {
    cmd.get_program().to_string_lossy().into_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A shell script `name` in `dir`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        file
    }

    /// The script run by `sh`: executing a file just written can fail with "text file busy"
    /// while another test's thread starts a program (it holds the file open for a moment).
    fn sh(file: &Path) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(file);
        cmd
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdm-launch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_hanging_program_never_holds_the_caller_and_falls_back() {
        let dir = temp("hang");
        let hang = script(&dir, "hang", "exec sleep 30");
        let (tx, rx) = std::sync::mpsc::channel();
        let start = Instant::now();
        run_detached_with_fallback(sh(&hang), Duration::from_millis(500), move || {
            let _ = tx.send(Instant::now());
        });
        assert!(
            start.elapsed() < Duration::from_millis(200),
            "the caller waited"
        );
        let fell_back = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("no fallback");
        // Ended at its limit, not after its 30 s.
        assert!(fell_back - start < Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_program_that_succeeds_has_no_fallback_and_one_that_fails_has_one() {
        let dir = temp("exit");
        for (body, falls_back) in [("exit 0", false), ("exit 3", true)] {
            let prog = script(&dir, "prog", body);
            let (tx, rx) = std::sync::mpsc::channel();
            run_detached_with_fallback(sh(&prog), Duration::from_secs(5), move || {
                let _ = tx.send(());
            });
            let got = rx.recv_timeout(Duration::from_secs(3)).is_ok();
            assert_eq!(got, falls_back, "{body}");
        }
        let missing = dir.join("no-such-program");
        let (tx, rx) = std::sync::mpsc::channel();
        run_detached_with_fallback(Command::new(missing), Duration::from_secs(5), move || {
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok(), "missing");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn output_within_reads_the_output_and_gives_up_on_a_hang() {
        let dir = temp("output");
        let say = script(&dir, "say", "echo blue");
        let out = output_within(&mut sh(&say), Duration::from_secs(5)).unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "blue");
        let hang = script(&dir, "hang", "exec sleep 30");
        let start = Instant::now();
        assert!(output_within(&mut sh(&hang), Duration::from_millis(300)).is_none());
        assert!(start.elapsed() < Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(dir);
    }
}

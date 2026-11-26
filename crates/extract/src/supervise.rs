//! Running 7-Zip or HatchZip as its own program, watched while it runs. One loop for every place
//! MDM runs one (extracting, testing an archive, extracting in place):
//! - a cancel set before it starts means it never starts;
//! - a cancel set while it runs ends it (killed, then waited for), within a fraction of a second;
//! - what it wrote is looked at every so often while it runs (and once more when it has ended), and
//!   a look that says stop ([`Tally::over`]: more bytes or entries than the limits) ends it too.
//!
//! Its errors are kept (the start of them) for the caller to read; its output, when the caller
//! wants it (7-Zip's progress), is handed over as it comes. Both are read on threads of their own,
//! so a full pipe never stalls it.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{Error, Limits};

/// How often a running program is checked for a cancel and for its end.
const POLL: Duration = Duration::from_millis(50);
/// How often what it wrote is looked at: at most this often, and never more than a fifth of the
/// time (a look through a large folder waits longer before the next one).
const LOOK_EVERY: Duration = Duration::from_millis(250);
/// The most of its error output kept: every line of a long list of damaged entries, and a bound.
const ERRORS_KEPT: usize = 4 << 20;

/// Told what a watched program writes on its output, as it comes (on a thread of its own).
pub(crate) type OnOutput<'a> = &'a mut (dyn FnMut(&[u8]) + Send);

/// How a watched program ended by itself.
pub(crate) struct Ended {
    pub status: ExitStatus,
    /// What it wrote on its error output (the start of it), as text.
    pub errors: String,
}

/// Start `cmd` and watch it until it ends. `input`: one line written to its input, which is then
/// closed (a password 7-Zip asks for); None: no input. `on_output`: told what it writes on its
/// output as it comes; None: its output is discarded. `cancel` ends it, and so does an `Err` from
/// `look`, which is called while it runs and once more after it ended by itself; either way it has
/// ended (killed and waited for) when this returns.
pub(crate) fn run(
    mut cmd: Command,
    input: Option<&str>,
    on_output: Option<OnOutput<'_>>,
    cancel: Option<&AtomicBool>,
    look: &mut dyn FnMut() -> Result<(), Error>,
) -> Result<Ended, Error> {
    let stopped = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    if stopped() {
        return Err(Error::Cancelled);
    }
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(if on_output.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stderr(Stdio::piped());
    // No console window flashing up for it.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    if let (Some(line), Some(mut stdin)) = (input, child.stdin.take()) {
        // One line, then the end (dropped): asked again, it finds nothing and stops. Not asked,
        // it never reads it.
        let _ = writeln!(stdin, "{line}");
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    std::thread::scope(|scope| {
        let errors = scope.spawn(move || {
            let mut kept = Vec::new();
            if let Some(mut pipe) = stderr {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let room = ERRORS_KEPT.saturating_sub(kept.len());
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
            }
            String::from_utf8_lossy(&kept).into_owned()
        });
        let output = scope.spawn(move || {
            if let (Some(mut pipe), Some(tell)) = (stdout, on_output) {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    tell(&buf[..n]);
                }
            }
        });
        let ended = watch(&mut child, &stopped, look);
        let _ = output.join();
        let errors = errors.join().unwrap_or_default();
        ended.map(|status| Ended { status, errors })
    })
}

/// Wait for `child`, ending it on a cancel or a look that says stop.
fn watch(
    child: &mut Child,
    stopped: &dyn Fn() -> bool,
    look: &mut dyn FnMut() -> Result<(), Error>,
) -> Result<ExitStatus, Error> {
    let end = |child: &mut Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    let mut next = Instant::now() + LOOK_EVERY;
    loop {
        if stopped() {
            end(child);
            return Err(Error::Cancelled);
        }
        match child.try_wait() {
            // What it wrote right before it ended counts as well.
            Ok(Some(status)) => return look().map(|()| status),
            Ok(None) => {}
            Err(e) => {
                end(child);
                return Err(e.into());
            }
        }
        if Instant::now() >= next {
            let began = Instant::now();
            if let Err(e) = look() {
                end(child);
                return Err(e);
            }
            next = Instant::now() + LOOK_EVERY.max(began.elapsed() * 5);
        }
        std::thread::sleep(POLL);
    }
}

/// What is in a folder another program writes into: its files' bytes and its entries (files,
/// folders, anything else), links never followed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    pub bytes: u64,
    pub entries: u64,
}

impl Tally {
    /// Count what is under `root`. An entry that goes away while it is counted (the program
    /// still writing) is passed over.
    pub fn of(root: &Path) -> Tally {
        let mut t = Tally::default();
        // Without limits, nothing stops it early.
        let _ = t.walk(root, None);
        t
    }

    /// Like [`Tally::of`], but `Err` as soon as it passes `limits` (counting stops there):
    /// [`Error::TooMany`] for the entries, [`Error::TooBig`] for the bytes.
    pub fn over(root: &Path, limits: Limits) -> Result<Tally, Error> {
        let mut t = Tally::default();
        t.walk(root, Some(limits))?;
        Ok(t)
    }

    fn walk(&mut self, root: &Path, limits: Option<Limits>) -> Result<(), Error> {
        let mut todo = vec![root.to_path_buf()];
        while let Some(dir) = todo.pop() {
            let Ok(rd) = fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let Ok(m) = fs::symlink_metadata(e.path()) else {
                    continue;
                };
                self.entries += 1;
                if m.is_file() {
                    self.bytes = self.bytes.saturating_add(m.len());
                } else if m.is_dir() {
                    todo.push(e.path());
                }
                if let Some(l) = limits {
                    if self.entries > l.entries {
                        return Err(Error::TooMany);
                    }
                    if self.bytes > l.total {
                        return Err(Error::TooBig {
                            would: self.bytes,
                            limit: l.total,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a test of its own: the program the tests below watch, this test binary run again with
    /// `MDM_TEST_SLEEPER` set, which then waits a minute. Run as a test, it returns at once.
    #[test]
    fn sleeper() {
        if std::env::var_os("MDM_TEST_SLEEPER").is_some() {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    fn sleeper_cmd() -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "supervise::tests::sleeper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("MDM_TEST_SLEEPER", "1");
        cmd
    }

    /// A program that would run a minute is ended by a cancel within seconds.
    #[test]
    fn a_cancel_while_it_runs_ends_it() {
        let stop = AtomicBool::new(false);
        let began = Instant::now();
        let r = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                stop.store(true, Ordering::Relaxed);
            });
            run(sleeper_cmd(), None, None, Some(&stop), &mut || Ok(()))
        });
        assert!(matches!(r, Err(Error::Cancelled)), "{:?}", r.err());
        // A minute's program, ended: well under that even on a slow machine.
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "{:?}",
            began.elapsed()
        );
    }

    /// A look that says stop ends it too, with the look's error.
    #[test]
    fn a_look_that_says_stop_ends_it() {
        let began = Instant::now();
        let mut looks = 0;
        let r = run(sleeper_cmd(), None, None, None, &mut || {
            looks += 1;
            if looks >= 2 {
                Err(Error::TooMany)
            } else {
                Ok(())
            }
        });
        assert!(matches!(r, Err(Error::TooMany)), "{:?}", r.err());
        assert_eq!(looks, 2);
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "{:?}",
            began.elapsed()
        );
    }

    /// A cancel set before: nothing is started (a program that isn't there would fail to start).
    #[test]
    fn a_cancel_before_starts_nothing() {
        let stop = AtomicBool::new(true);
        let cmd = Command::new("no-such-program-anywhere-here");
        let r = run(cmd, None, None, Some(&stop), &mut || Ok(()));
        assert!(matches!(r, Err(Error::Cancelled)), "{:?}", r.err());
    }

    #[test]
    fn a_tally_stops_at_the_limits() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("d")).unwrap();
        fs::write(t.path().join("d").join("a"), [0u8; 4096]).unwrap();
        fs::write(t.path().join("b"), [0u8; 4096]).unwrap();
        assert_eq!(
            Tally::of(t.path()),
            Tally {
                bytes: 8192,
                entries: 3
            }
        );
        let limits = |total, entries| Limits {
            total,
            entries,
            depth: 8,
        };
        assert!(Tally::over(t.path(), limits(8192, 3)).is_ok());
        assert!(matches!(
            Tally::over(t.path(), limits(8191, 3)),
            Err(Error::TooBig { limit: 8191, .. })
        ));
        assert!(matches!(
            Tally::over(t.path(), limits(8192, 2)),
            Err(Error::TooMany)
        ));
    }
}

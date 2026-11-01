//! The log MDM keeps in release builds too: `<data>/logs/mdm.log`, rolled over at a size into
//! `mdm.1.log` … `mdm.3.log`. Each line is written through at once (a crash loses nothing that
//! was logged), and a report takes the last lines of it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A log file grows to this, then rolls over.
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Rolled-over files kept (`mdm.1.log` newest).
pub const KEEP: usize = 3;

/// The log of a data folder.
pub fn dir(data: &Path) -> PathBuf {
    data.join("logs")
}

fn current(dir: &Path) -> PathBuf {
    dir.join("mdm.log")
}

fn rolled(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("mdm.{n}.log"))
}

/// The writer behind the log: appends to `mdm.log`, rolling it over past [`MAX_BYTES`].
#[derive(Clone)]
pub struct Rolling {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    dir: PathBuf,
    file: Option<File>,
    size: u64,
}

impl Rolling {
    pub fn new(dir: PathBuf) -> io::Result<Rolling> {
        fs::create_dir_all(&dir)?;
        let path = current(&dir);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Rolling {
            inner: Arc::new(Mutex::new(Inner {
                dir,
                file: Some(file),
                size,
            })),
        })
    }
}

impl Inner {
    fn roll(&mut self) {
        self.file = None;
        for n in (1..KEEP).rev() {
            let _ = fs::rename(rolled(&self.dir, n), rolled(&self.dir, n + 1));
        }
        let _ = fs::rename(current(&self.dir), rolled(&self.dir, 1));
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(current(&self.dir))
            .ok();
        self.size = 0;
    }
}

impl Write for Rolling {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // A poisoned lock (a panic while logging) must not stop the log: take it back.
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.size + buf.len() as u64 > MAX_BYTES {
            inner.roll();
        }
        if let Some(f) = inner.file.as_mut() {
            f.write_all(buf)?;
            inner.size += buf.len() as u64;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.file.as_mut() {
            Some(f) => f.flush(),
            None => Ok(()),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Rolling {
    type Writer = Rolling;
    fn make_writer(&'a self) -> Rolling {
        self.clone()
    }
}

/// Start logging into `<data>/logs`: MDM's own crates at `info`, everything else (egui, winit,
/// the network stack) at `warn`; `MDM_LOG` overrides it with an `EnvFilter` (e.g.
/// `MDM_LOG=debug`). Returns false when the folder can't be written (MDM runs on without a log).
pub fn init(data: &Path) -> bool {
    use tracing_subscriber::EnvFilter;
    let Ok(writer) = Rolling::new(dir(data)) else {
        return false;
    };
    let filter = std::env::var("MDM_LOG")
        .ok()
        .and_then(|f| EnvFilter::try_new(f).ok())
        .unwrap_or_else(|| EnvFilter::new("warn,mdm=info,mdm_app=info,misterdownloadmanager=info,mdm_engine=info,mdm_torrent=info,mdm_crash=info,mdm_tools=info,mdm_ipc=info"));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_env_filter(filter)
        .try_init()
        .is_ok()
}

/// The last `n` lines of the log (across the roll-over: the end of `mdm.1.log` when `mdm.log`
/// is short).
pub fn tail(data: &Path, n: usize) -> Vec<String> {
    let dir = dir(data);
    let mut lines: Vec<String> = Vec::new();
    for path in [current(&dir), rolled(&dir, 1)] {
        let Ok(text) = fs::read(&path) else { continue };
        let text = String::from_utf8_lossy(&text);
        let mut these: Vec<String> = text.lines().map(str::to_owned).collect();
        these.append(&mut lines);
        lines = these;
        if lines.len() >= n {
            break;
        }
    }
    let skip = lines.len().saturating_sub(n);
    lines.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_rolls_over_and_the_tail_spans_the_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut w = Rolling::new(dir(tmp.path())).unwrap();
        let line = "x".repeat(1023) + "\n";
        // Past the size: the first lines move to mdm.1.log.
        for _ in 0..(MAX_BYTES / 1024 + 10) {
            w.write_all(line.as_bytes()).unwrap();
        }
        writeln!(w, "last").unwrap();
        assert!(rolled(&dir(tmp.path()), 1).exists());
        let t = tail(tmp.path(), 20);
        assert_eq!(t.len(), 20);
        assert_eq!(t.last().unwrap(), "last");
    }
}

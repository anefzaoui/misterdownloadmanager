//! A video download run by yt-dlp under supervision: our file name, progress read from a fixed
//! `--progress-template`, the final path from `--print after_move`. Stopping kills it; starting
//! again continues (`--continue`). Each start looks the page up afresh (`-J`), points links at
//! servers that answer ([`crate::servers`]) and downloads from that list (`--load-info-json`),
//! so the page is read once.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::proc::{self, ProcError};
use crate::servers;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Running,
    Done(PathBuf),
    Failed(String),
    Stopped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub state: State,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Bytes/s.
    pub speed: f64,
    /// Notes for the download's log, oldest first (a server swapped for another).
    pub events: Vec<(std::time::SystemTime, String)>,
}

pub struct VideoJob {
    /// yt-dlp once the page is looked up (`None` before).
    child: Arc<Mutex<Option<proc::Spawned>>>,
    progress: Arc<Mutex<Progress>>,
}

/// A private file for the looked-up page (its links are signed; removed when the run ends).
fn info_file() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "mdm-video-{}-{}.json",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The page's video information from yt-dlp (one video, not a playlist).
fn look_up(exe: &Path, url: &str) -> Result<serde_json::Value, String> {
    let args: Vec<String> = ["-J", "--no-playlist", "--no-warnings"]
        .map(String::from)
        .to_vec();
    let cmd = proc::ytdlp(exe, &[], &args, url).map_err(|e| e.to_string())?;
    let out = proc::output(cmd, "yt-dlp", std::time::Duration::from_secs(90))
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&out).map_err(|_| "yt-dlp gave no video information".to_owned())
}

type Child = Arc<Mutex<Option<proc::Spawned>>>;

/// How one run of yt-dlp ended.
enum Attempt {
    Done(PathBuf),
    Stopped,
    /// `rerouted`: links were moved to other servers for this run.
    Failed {
        error: String,
        rerouted: bool,
    },
}

/// Runs the download; once more with a fresh look-up when a run whose links had to be moved to
/// other servers fails (new links, often other servers: YouTube's alternatives refuse some
/// streams some of the time).
fn supervise(exe: &Path, url: &str, ours: &[String], progress: &Mutex<Progress>, child: &Child) {
    for n in 0..2 {
        match attempt(exe, url, ours, progress, child) {
            Attempt::Stopped => return,
            Attempt::Done(f) => {
                let mut p = progress.lock().unwrap();
                if p.state != State::Stopped {
                    p.speed = 0.0;
                    p.state = State::Done(f);
                }
                return;
            }
            Attempt::Failed { error, rerouted } if rerouted && n == 0 => {
                progress.lock().unwrap().events.push((
                    std::time::SystemTime::now(),
                    format!("{error}; looking the page up again"),
                ));
            }
            Attempt::Failed { error, .. } => {
                let mut p = progress.lock().unwrap();
                if p.state != State::Stopped {
                    p.speed = 0.0;
                    p.state = State::Failed(error);
                }
                return;
            }
        }
    }
}

/// One run: look the page up, move links off unreachable servers, download from that list.
fn attempt(
    exe: &Path,
    url: &str,
    ours: &[String],
    progress: &Mutex<Progress>,
    child: &Child,
) -> Attempt {
    let failed = |error: String, rerouted: bool| Attempt::Failed { error, rerouted };
    let mut info = match look_up(exe, url) {
        Ok(v) => v,
        Err(e) => return failed(e, false),
    };
    let prober = servers::Prober::new();
    let swaps = servers::reroute(&mut info, servers::reachable, |l, h| prober.serves(l, h));
    for (dead, alt) in &swaps {
        progress.lock().unwrap().events.push((
            std::time::SystemTime::now(),
            format!("The server {dead} can't be reached; downloading from {alt}"),
        ));
    }
    let rerouted = !swaps.is_empty();
    let file = info_file();
    if let Err(e) = std::fs::write(&file, info.to_string()) {
        return failed(format!("Cannot write the video information: {e}"), rerouted);
    }
    let _cleanup = Remove(file.clone());
    let stdout = {
        // Under the child's lock: stop() either came first (nothing starts) or finds the child
        // to kill.
        let mut slot = child.lock().unwrap();
        if progress.lock().unwrap().state == State::Stopped {
            return Attempt::Stopped;
        }
        let spawned =
            proc::ytdlp_info(exe, &[], ours, &file).and_then(|cmd| proc::spawn(cmd, "yt-dlp"));
        match spawned {
            Ok(mut s) => {
                let out = s.child.stdout.take().expect("piped");
                *slot = Some(s);
                out
            }
            Err(e) => return failed(e.to_string(), rerouted),
        }
    };
    let mut saved = None;
    // Each stream (video, then audio) reports from 0: earlier ones add up.
    let mut finished_streams = 0u64;
    let mut last_total = 0u64;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("MDMF ") {
            saved = Some(PathBuf::from(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("MDMP ") {
            let f: Vec<Option<f64>> = rest.split_whitespace().map(|v| v.parse().ok()).collect();
            let got = f.first().copied().flatten().unwrap_or(0.0) as u64;
            let total = f.get(1).copied().flatten().or(f.get(2).copied().flatten());
            let mut p = progress.lock().unwrap();
            if got + finished_streams < p.downloaded && last_total > 0 {
                finished_streams += last_total;
            }
            last_total = total.map_or(last_total, |t| t as u64);
            p.downloaded = finished_streams + got;
            p.total = total.map(|t| finished_streams + t as u64);
            p.speed = f.get(3).copied().flatten().unwrap_or(0.0);
        }
    }
    // Poll (the lock is free in between, so stop() can still kill it).
    let status = loop {
        let mut slot = child.lock().unwrap();
        let Some(s) = slot.as_mut() else {
            break Err(std::io::Error::other("yt-dlp is gone"));
        };
        match s.child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        drop(slot);
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    if progress.lock().unwrap().state == State::Stopped {
        return Attempt::Stopped;
    }
    match (status, saved) {
        (Ok(s), Some(f)) if s.success() => Attempt::Done(f),
        (Ok(s), None) if s.success() => {
            failed("yt-dlp didn't say where it saved the file".into(), rerouted)
        }
        _ => failed(
            child
                .lock()
                .unwrap()
                .as_ref()
                .map_or_else(|| "yt-dlp stopped".to_owned(), |s| s.failure().to_string()),
            rerouted,
        ),
    }
}

/// Removes a file when dropped.
struct Remove(PathBuf);

impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `%` would be read as a template field by yt-dlp.
fn template_text(s: &str) -> String {
    s.replace('%', "%%")
}

/// Our progress line: `MDMP <downloaded> <total> <estimate> <speed>` (`NA` when unknown).
const PROGRESS: &str = "download:MDMP %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s";
/// The final file: `MDMF <path>`.
const FINAL: &str = "after_move:MDMF %(filepath)s";

impl VideoJob {
    /// Download `url` in format `selector` to `dir/<stem>.<ext>`, at most `rate` bytes/s;
    /// `ffmpeg` (its exe) merges separate video and audio into an MP4.
    pub fn start(
        exe: &Path,
        url: &str,
        selector: &str,
        dir: &Path,
        stem: &str,
        rate: Option<u64>,
        ffmpeg: Option<&Path>,
    ) -> Result<VideoJob, ProcError> {
        proc::safe(url)?;
        let output = dir.join(format!("{}.%(ext)s", template_text(stem)));
        let mut ours: Vec<String> = vec![
            "-f".into(),
            selector.into(),
            "-o".into(),
            output.to_string_lossy().into_owned(),
            "--no-playlist".into(),
            "--newline".into(),
            // `--print` makes yt-dlp quiet; this keeps the progress lines.
            "--progress".into(),
            "--continue".into(),
            "--no-mtime".into(),
            "--progress-template".into(),
            PROGRESS.into(),
            "--print".into(),
            FINAL.into(),
        ];
        if let Some(r) = rate {
            ours.extend(["--limit-rate".into(), r.to_string()]);
        }
        if let Some(dir) = ffmpeg.and_then(Path::parent) {
            ours.extend([
                "--ffmpeg-location".into(),
                dir.to_string_lossy().into_owned(),
                "--merge-output-format".into(),
                "mp4".into(),
            ]);
        }
        for a in &ours {
            proc::safe(a)?;
        }
        let progress = Arc::new(Mutex::new(Progress {
            state: State::Running,
            downloaded: 0,
            total: None,
            speed: 0.0,
            events: Vec::new(),
        }));
        let child: Child = Arc::new(Mutex::new(None));
        {
            let (progress, child) = (progress.clone(), child.clone());
            let (exe, url) = (exe.to_owned(), url.to_owned());
            std::thread::spawn(move || {
                // A panic in here must not leave the download "running" forever.
                let run = std::panic::AssertUnwindSafe(|| {
                    supervise(&exe, &url, &ours, &progress, &child)
                });
                if std::panic::catch_unwind(run).is_err() {
                    let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                    if p.state == State::Running {
                        p.speed = 0.0;
                        p.state =
                            State::Failed("the video download stopped on an internal error".into());
                    }
                }
            });
        }
        Ok(VideoJob { child, progress })
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().unwrap().clone()
    }

    /// Stop (the partial file stays; starting again continues it).
    pub fn stop(&self) {
        self.progress.lock().unwrap().state = State::Stopped;
        if let Some(s) = self.child.lock().unwrap().as_mut() {
            let _ = s.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live (network, yt-dlp): `MDM_LIVE_YTDLP=<yt-dlp.exe> MDM_LIVE_PAGE=<video page>
    /// cargo test -p mdm-tools live_download -- --ignored --nocapture`. Downloads the smallest
    /// audio and prints the log (a swapped server shows there).
    #[test]
    #[ignore]
    fn live_download() {
        let exe = PathBuf::from(std::env::var("MDM_LIVE_YTDLP").expect("MDM_LIVE_YTDLP"));
        let page = std::env::var("MDM_LIVE_PAGE").expect("MDM_LIVE_PAGE");
        let dir = tempfile::tempdir().unwrap();
        let format = std::env::var("MDM_LIVE_FORMAT").unwrap_or_else(|_| "worstaudio".into());
        let ffmpeg = std::env::var("MDM_LIVE_FFMPEG").ok().map(PathBuf::from);
        let job = VideoJob::start(
            &exe,
            &page,
            &format,
            dir.path(),
            "a",
            None,
            ffmpeg.as_deref(),
        )
        .unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let p = loop {
            let p = job.progress();
            if p.state != State::Running || std::time::Instant::now() > end {
                break p;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        for (_, e) in &p.events {
            println!("event: {e}");
        }
        println!("state: {:?}, {} bytes", p.state, p.downloaded);
        assert!(
            matches!(p.state, State::Done(ref f) if f.exists()),
            "{:?}",
            p.state
        );
    }

    #[test]
    fn percent_signs_in_titles_stay_text() {
        assert_eq!(template_text("100% fun"), "100%% fun");
    }
}

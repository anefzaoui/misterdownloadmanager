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
    /// Saved beside the video once it is done: its subtitle files (#68).
    pub side_files: Vec<PathBuf>,
    /// Subtitles that didn't come with the video, once it is done (#68): the video is saved all
    /// the same.
    pub sub_problems: Vec<SubProblem>,
}

/// A subtitle language the site wouldn't give (#68).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubProblem {
    /// yt-dlp's code (`en`) and the site's name for it (`English`; the code when none).
    pub code: String,
    pub name: String,
    pub reason: SubReason,
    /// They came on the second try, but beside the video, not in it (they were to be embedded).
    pub beside: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubReason {
    /// HTTP 429: the site limits how often subtitles may be fetched (YouTube does, most of all
    /// for its machine-translated captions).
    TooManyRequests,
    /// yt-dlp's own words.
    Other(String),
}

/// How long to wait before asking once more for the subtitles a site refused. A site that
/// answers "too many requests" wants a pause, not a quick retry. `MDM_SUBS_RETRY_SECS`
/// shortens it for the tests (tests/fake_ytdlp.rs).
fn subs_retry_delay() -> std::time::Duration {
    let secs = std::env::var("MDM_SUBS_RETRY_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    std::time::Duration::from_secs(secs)
}

/// The subtitle languages yt-dlp said it couldn't download (with `--ignore-errors` it goes on
/// with the video and only warns: `Unable to download video subtitles for 'en': HTTP Error 429:
/// Too Many Requests`), and why.
fn subtitle_failures(warnings: &[String]) -> Vec<(String, SubReason)> {
    const SAID: &str = "Unable to download video subtitles for '";
    let mut out: Vec<(String, SubReason)> = Vec::new();
    for w in warnings {
        let Some(rest) = w.split_once(SAID).map(|(_, r)| r) else {
            continue;
        };
        let Some((code, why)) = rest.split_once('\'') else {
            continue;
        };
        let why = why.trim_start_matches(':').trim();
        let reason =
            if why.contains("429") || why.to_ascii_lowercase().contains("too many requests") {
                SubReason::TooManyRequests
            } else {
                SubReason::Other(why.to_owned())
            };
        if !out.iter().any(|(c, _)| c == code) {
            out.push((code.to_owned(), reason));
        }
    }
    out
}

/// The site's name for a subtitle language, from the looked-up page (`code` when none).
fn sub_name(info: &serde_json::Value, code: &str) -> String {
    ["subtitles", "automatic_captions"]
        .iter()
        .find_map(|k| info[k][code][0]["name"].as_str())
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| code.to_owned())
}

/// What a second, subtitles-only run needs (#68): what the first asked for, and its own
/// arguments (the output name, the browser's context, where ffmpeg is).
#[derive(Clone, Debug)]
struct SubsRetry {
    automatic: bool,
    embed: bool,
    ffmpeg: bool,
    base: Vec<String>,
}

impl SubsRetry {
    /// yt-dlp's arguments to fetch `codes` again, beside the video (a second run can't put them
    /// into a video already saved).
    fn args(&self, codes: &[String]) -> Vec<String> {
        let again = Subs {
            langs: codes.to_vec(),
            automatic: self.automatic,
            embed: false,
        };
        let mut out = self.base.clone();
        out.push("--skip-download".into());
        out.extend(subtitle_args(&again, self.ffmpeg));
        out
    }
}

/// Which subtitles to save with a video (#68). `langs` empty: none.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Subs {
    /// yt-dlp's language codes, or [`ALL_UPLOADERS`] alone for every one the uploader gave.
    pub langs: Vec<String>,
    /// The site's automatic captions rather than the uploader's.
    pub automatic: bool,
    /// Put into the video file instead of beside it (needs ffmpeg; otherwise beside it).
    pub embed: bool,
}

/// `Subs::langs` for all of the uploader's subtitles.
pub const ALL_UPLOADERS: &str = "all";

/// The files subtitles are saved as.
const SUB_EXTS: [&str; 4] = ["srt", "vtt", "ass", "ttml"];

/// yt-dlp's arguments for `subs`: written beside the video as `<stem>.<lang>.srt` (converted to
/// SRT by ffmpeg, which every player reads; without ffmpeg SRT when the site has it, else its
/// own format, usually WebVTT), or embedded into the video with ffmpeg (yt-dlp removes the
/// separate files then).
fn subtitle_args(subs: &Subs, ffmpeg: bool) -> Vec<String> {
    if subs.langs.is_empty() {
        return Vec::new();
    }
    let langs = if subs.langs.len() == 1 && subs.langs[0] == ALL_UPLOADERS {
        // Not the chat replay of a live stream, which yt-dlp lists with them.
        "all,-live_chat".to_owned()
    } else {
        subs.langs.join(",")
    };
    let embed = subs.embed && ffmpeg;
    let mut out: Vec<String> = Vec::new();
    // `--embed-subs` fetches the uploader's itself, and keeps no files behind when it is the
    // only one asked (`--write-subs` too would keep them).
    match (subs.automatic, embed) {
        (true, _) => out.push("--write-auto-subs".into()),
        (false, false) => out.push("--write-subs".into()),
        (false, true) => {}
    }
    out.extend(["--sub-langs".into(), langs]);
    if ffmpeg {
        out.extend([
            "--sub-format".into(),
            "srt/best".into(),
            "--convert-subs".into(),
            "srt".into(),
        ]);
    } else {
        out.extend(["--sub-format".into(), "srt/vtt/best".into()]);
    }
    if embed {
        out.push("--embed-subs".into());
    }
    // A subtitle the site refuses must not cost the video: yt-dlp stops on it unless told to
    // go on, and then only warns (`subtitle_failures`). Whether the video was saved is checked
    // on its own (`attempt`).
    out.push("--ignore-errors".into());
    out
}

/// The subtitle files saved beside `video` (`<stem>.<lang>.<srt|vtt…>`, as yt-dlp names them).
fn side_files(video: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem()) else {
        return Vec::new();
    };
    let prefix = format!("{}.", stem.to_string_lossy());
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p != video && p.is_file())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            name.is_some_and(|n| n.starts_with(&prefix))
                && ext.is_some_and(|e| SUB_EXTS.contains(&e.as_str()))
        })
        .collect();
    out.sort();
    out
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
    /// The video saved, and the subtitles that didn't come with it.
    Done(PathBuf, Vec<SubProblem>),
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
fn supervise(
    exe: &Path,
    url: &str,
    ours: &[String],
    progress: &Mutex<Progress>,
    child: &Child,
    expected: Option<u64>,
    retry: Option<&SubsRetry>,
) {
    for n in 0..2 {
        match attempt(exe, url, ours, progress, child, expected, retry) {
            Attempt::Stopped => return,
            Attempt::Done(f, problems) => {
                // The saved file's own size: a merged video or a converted MP3 isn't the size
                // of what came down.
                let saved = std::fs::metadata(&f).ok().map(|m| m.len());
                let side = side_files(&f);
                let mut p = progress.lock().unwrap();
                if p.state != State::Stopped {
                    p.speed = 0.0;
                    p.side_files = side;
                    p.sub_problems = problems;
                    if let Some(len) = saved {
                        p.total = Some(len);
                        p.downloaded = len;
                    }
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

/// yt-dlp's progress lines (`MDMP <downloaded> <total> <estimate> <speed>`) made into one
/// steady figure. Each stream of a merged download (video, then audio) reports from 0 with its
/// own total, and a fragmented stream's total (HLS, DASH) is an estimate that moves with every
/// fragment: the streams are added up, the total never falls below the size the video window
/// showed (`expected`, every stream), and the fraction done never goes back.
struct Meter {
    expected: Option<u64>,
    /// The streams already finished, and the current one's last total and bytes.
    finished: u64,
    last_total: u64,
    last_got: u64,
    /// The highest fraction shown so far.
    frac: f64,
}

impl Meter {
    fn new(expected: Option<u64>) -> Self {
        Meter {
            expected: expected.filter(|e| *e > 0),
            finished: 0,
            last_total: 0,
            last_got: 0,
            frac: 0.0,
        }
    }

    fn line(&mut self, rest: &str, p: &mut Progress) {
        let f: Vec<Option<f64>> = rest.split_whitespace().map(|v| v.parse().ok()).collect();
        let got = f.first().copied().flatten().unwrap_or(0.0).max(0.0) as u64;
        let total = f
            .get(1)
            .copied()
            .flatten()
            .or(f.get(2).copied().flatten())
            .map(|t| t.max(0.0) as u64);
        // Back near 0: the next stream started; the one before counts in full.
        if got < self.last_got && self.last_got > 0 {
            self.finished += self.last_total.max(self.last_got);
            self.last_total = 0;
        }
        self.last_got = got;
        if let Some(t) = total {
            self.last_total = t;
        }
        let downloaded = self.finished + got;
        let mut whole = self.finished + self.last_total.max(got);
        if let Some(e) = self.expected {
            whole = whole.max(e);
        }
        p.downloaded = downloaded;
        p.speed = f.get(3).copied().flatten().unwrap_or(0.0);
        if whole == 0 {
            p.total = None;
            return;
        }
        let frac = downloaded as f64 / whole as f64;
        if frac >= self.frac {
            self.frac = frac;
            p.total = Some(whole);
        } else {
            // The estimate grew: keep the bar where it was rather than move it back.
            p.total = Some(
                (downloaded as f64 / self.frac)
                    .round()
                    .max(downloaded as f64) as u64,
            );
        }
    }
}

/// How one yt-dlp run on the looked-up page ended.
struct Ran {
    status: std::io::Result<std::process::ExitStatus>,
    /// Where it said it saved the file (`MDMF`).
    saved: Option<PathBuf>,
    warnings: Vec<String>,
    /// What it said when it failed.
    failure: String,
}

/// Run yt-dlp with `ours` on the page looked up into `file`, following its progress with
/// `meter`. None: stopped before or while it ran.
fn run(
    exe: &Path,
    ours: &[String],
    file: &Path,
    progress: &Mutex<Progress>,
    child: &Child,
    mut meter: Option<&mut Meter>,
) -> Result<Option<Ran>, String> {
    let stdout = {
        // Under the child's lock: stop() either came first (nothing starts) or finds the child
        // to kill.
        let mut slot = child.lock().unwrap();
        if progress.lock().unwrap().state == State::Stopped {
            return Ok(None);
        }
        let spawned =
            proc::ytdlp_info(exe, &[], ours, file).and_then(|cmd| proc::spawn(cmd, "yt-dlp"));
        match spawned {
            Ok(mut s) => {
                let out = s.child.stdout.take().expect("piped");
                *slot = Some(s);
                out
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let mut saved = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("MDMF ") {
            saved = Some(PathBuf::from(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("MDMP ")
            && let Some(m) = meter.as_deref_mut()
        {
            m.line(rest, &mut progress.lock().unwrap());
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
        return Ok(None);
    }
    let slot = child.lock().unwrap();
    let (warnings, failure) = slot.as_ref().map_or_else(
        || (Vec::new(), "yt-dlp stopped".to_owned()),
        |s| (s.warnings(), s.failure().to_string()),
    );
    Ok(Some(Ran {
        status,
        saved,
        warnings,
        failure,
    }))
}

/// Wait `d`, or less if the download is stopped meanwhile (false then).
fn pause(d: std::time::Duration, progress: &Mutex<Progress>) -> bool {
    let end = std::time::Instant::now() + d;
    while std::time::Instant::now() < end {
        if progress.lock().unwrap().state == State::Stopped {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    progress.lock().unwrap().state != State::Stopped
}

/// One run: look the page up, move links off unreachable servers, download from that list.
/// Subtitles the site refused are asked for once more after a pause (`retry`), beside the video;
/// the ones still refused are reported with it, never failing it.
fn attempt(
    exe: &Path,
    url: &str,
    ours: &[String],
    progress: &Mutex<Progress>,
    child: &Child,
    expected: Option<u64>,
    retry: Option<&SubsRetry>,
) -> Attempt {
    let failed = |error: String, rerouted: bool| Attempt::Failed { error, rerouted };
    let mut info = match look_up(exe, url) {
        Ok(v) => v,
        Err(e) => return failed(e, false),
    };
    let prober = servers::Prober::new();
    let swaps = servers::reroute(&mut info, servers::reachable, |l, h| prober.serves(l, h));
    for (dead, alt) in &swaps {
        note(
            progress,
            format!("The server {dead} can't be reached; downloading from {alt}"),
        );
    }
    let rerouted = !swaps.is_empty();
    let file = info_file();
    if let Err(e) = std::fs::write(&file, info.to_string()) {
        return failed(format!("Cannot write the video information: {e}"), rerouted);
    }
    let _cleanup = Remove(file.clone());
    let mut meter = Meter::new(expected);
    let ran = match run(exe, ours, &file, progress, child, Some(&mut meter)) {
        Ok(Some(r)) => r,
        Ok(None) => return Attempt::Stopped,
        Err(e) => return failed(e, rerouted),
    };
    let video = match (&ran.status, ran.saved) {
        // `--ignore-errors` (subtitles asked) lets yt-dlp end well with no file: the file
        // itself is the proof.
        (Ok(s), Some(f)) if s.success() && f.is_file() => f,
        (Ok(s), Some(_)) if s.success() => {
            return failed(
                "yt-dlp said it saved the file, but it isn't there".into(),
                rerouted,
            );
        }
        (Ok(s), None) if s.success() => {
            return failed("yt-dlp didn't say where it saved the file".into(), rerouted);
        }
        _ => return failed(ran.failure, rerouted),
    };
    let refused = subtitle_failures(&ran.warnings);
    if refused.is_empty() {
        return Attempt::Done(video, Vec::new());
    }
    for (code, why) in &refused {
        note(
            progress,
            format!(
                "Subtitles in {code} couldn't be downloaded: {}",
                english(why)
            ),
        );
    }
    // Once more after a pause, for the ones refused (a site limiting requests often gives them
    // a little later).
    let mut still = refused.clone();
    if let Some(r) = retry {
        let delay = subs_retry_delay();
        note(
            progress,
            format!("Asking for the subtitles again in {} s", delay.as_secs()),
        );
        if !pause(delay, progress) {
            return Attempt::Done(video, Vec::new());
        }
        let codes: Vec<String> = refused.iter().map(|(c, _)| c.clone()).collect();
        match run(exe, &r.args(&codes), &file, progress, child, None) {
            Ok(Some(again)) if again.status.as_ref().is_ok_and(|s| s.success()) => {
                still = subtitle_failures(&again.warnings);
            }
            Ok(None) => return Attempt::Done(video, Vec::new()),
            _ => {}
        }
    }
    let mut problems = Vec::new();
    for (code, why) in &refused {
        let gone = still.iter().find(|(c, _)| c == code);
        match gone {
            Some((_, why)) => problems.push(SubProblem {
                code: code.clone(),
                name: sub_name(&info, code),
                reason: why.clone(),
                beside: false,
            }),
            None => {
                note(
                    progress,
                    format!("Subtitles in {code} came on the second try"),
                );
                // Asked into the video, they're beside it now.
                if retry.is_some_and(|r| r.embed) {
                    problems.push(SubProblem {
                        code: code.clone(),
                        name: sub_name(&info, code),
                        reason: why.clone(),
                        beside: true,
                    });
                }
            }
        }
    }
    Attempt::Done(video, problems)
}

/// A line for the download's log (English, as yt-dlp's own).
fn note(progress: &Mutex<Progress>, text: String) {
    progress
        .lock()
        .unwrap()
        .events
        .push((std::time::SystemTime::now(), text));
}

/// A subtitle refusal in the log's words.
fn english(r: &SubReason) -> String {
    match r {
        SubReason::TooManyRequests => "the site refused (too many requests)".into(),
        SubReason::Other(why) => why.clone(),
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
    /// `ffmpeg` (its exe) merges separate video and audio into an MP4. `expected`: the size the
    /// choice was shown with (every stream), so the progress starts from the whole. `browser`:
    /// what the browser's request for the page carried (proc::FromBrowser). `subs`: the
    /// subtitles to save with it (#68; none for an MP3, which has no picture to put them on).
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        exe: &Path,
        url: &str,
        selector: &str,
        dir: &Path,
        stem: &str,
        rate: Option<u64>,
        ffmpeg: Option<&Path>,
        expected: Option<u64>,
        browser: &proc::FromBrowser,
        subs: &Subs,
    ) -> Result<VideoJob, ProcError> {
        proc::safe(url)?;
        let output = dir.join(format!("{}.%(ext)s", template_text(stem)));
        // An MP3 choice (`resolve::MP3`): the best sound, converted by ffmpeg (#32).
        let mp3 = crate::resolve::mp3_quality(selector);
        let format = if mp3.is_some() { "ba/b" } else { selector };
        // What comes down is the sound as the site has it, not the MP3 the choice's size
        // estimated: the progress follows yt-dlp's own total.
        let expected = if mp3.is_some() { None } else { expected };
        // Subtitles go with a picture only (none for an MP3).
        let with_subs = mp3.is_none() && !subs.langs.is_empty();
        let mut ours: Vec<String> = vec![
            "-f".into(),
            format.into(),
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
        // A subtitles-only second run, for the ones a site refuses (#68): the same output name.
        let mut retry_base: Vec<String> = vec![
            "-o".into(),
            output.to_string_lossy().into_owned(),
            "--no-playlist".into(),
        ];
        match mp3 {
            Some(q) => ours.extend([
                "-x".into(),
                "--audio-format".into(),
                "mp3".into(),
                "--audio-quality".into(),
                q,
            ]),
            None => ours.extend(subtitle_args(subs, ffmpeg.is_some())),
        }
        let (theirs, cookies) = browser.args(url)?;
        retry_base.extend(theirs.iter().cloned());
        ours.extend(theirs);
        if let Some(dir) = ffmpeg.and_then(Path::parent) {
            ours.extend([
                "--ffmpeg-location".into(),
                dir.to_string_lossy().into_owned(),
                "--merge-output-format".into(),
                "mp4".into(),
            ]);
            retry_base.extend([
                "--ffmpeg-location".into(),
                dir.to_string_lossy().into_owned(),
            ]);
        }
        let retry = with_subs.then(|| SubsRetry {
            automatic: subs.automatic,
            embed: subs.embed && ffmpeg.is_some(),
            ffmpeg: ffmpeg.is_some(),
            base: retry_base,
        });
        for a in retry.iter().flat_map(|r| r.base.iter()) {
            proc::safe(a)?;
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
            side_files: Vec::new(),
            sub_problems: Vec::new(),
        }));
        let child: Child = Arc::new(Mutex::new(None));
        {
            let (progress, child) = (progress.clone(), child.clone());
            let (exe, url) = (exe.to_owned(), url.to_owned());
            std::thread::spawn(move || {
                // The cookie file lives as long as the run.
                let _cookies = cookies;
                // A panic in here must not leave the download "running" forever.
                let run = std::panic::AssertUnwindSafe(|| {
                    supervise(
                        &exe,
                        &url,
                        &ours,
                        &progress,
                        &child,
                        expected,
                        retry.as_ref(),
                    )
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
            s.kill();
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
            None,
            &Default::default(),
            &Default::default(),
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

    /// Live (#61): `MDM_LIVE_YTDLP=<yt-dlp.exe> MDM_LIVE_SLOW=<a slow file's address>
    /// cargo test -p mdm-tools live_stop -- --ignored --nocapture` (the lab server:
    /// `/gen/20M/clip.mp4?rate=50K`). Stopping a video download ends every yt-dlp process it
    /// started (Windows: the launcher and its child), and the part file stops growing.
    #[test]
    #[ignore]
    fn live_stop_ends_every_process() {
        let exe = PathBuf::from(std::env::var("MDM_LIVE_YTDLP").expect("MDM_LIVE_YTDLP"));
        let url = std::env::var("MDM_LIVE_SLOW").expect("MDM_LIVE_SLOW");
        let dir = tempfile::tempdir().unwrap();
        let job = VideoJob::start(
            &exe,
            &url,
            "b",
            dir.path(),
            "clip",
            None,
            None,
            None,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let size = || {
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.metadata().map_or(0, |m| m.len()))
                .sum::<u64>()
        };
        std::thread::sleep(std::time::Duration::from_secs(8));
        println!("before stop: {} bytes, {:?}", size(), job.progress().state);
        job.stop();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let stopped = size();
        std::thread::sleep(std::time::Duration::from_secs(4));
        println!("after stop: {stopped} then {} bytes", size());
        assert_eq!(size(), stopped, "nothing downloads after Stop");
        // Started again: it continues the same part file (nothing else holds it now).
        let again = VideoJob::start(
            &exe,
            &url,
            "b",
            dir.path(),
            "clip",
            None,
            None,
            None,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(8));
        // yt-dlp's own count (a file open in another process shows a stale size on Windows).
        let p = again.progress();
        println!("resumed: yt-dlp at {} bytes, {:?}", p.downloaded, p.state);
        assert!(p.downloaded > stopped, "it went on from where it stopped");
        assert!(
            !matches!(p.state, State::Failed(_)),
            "resumed without an error: {:?}",
            p.state
        );
        again.stop();
    }

    fn run(expected: Option<u64>, lines: &[&str]) -> Vec<(u64, Option<u64>)> {
        let mut m = Meter::new(expected);
        let mut p = Progress {
            state: State::Running,
            downloaded: 0,
            total: None,
            speed: 0.0,
            events: Vec::new(),
            side_files: Vec::new(),
            sub_problems: Vec::new(),
        };
        lines
            .iter()
            .map(|l| {
                m.line(l, &mut p);
                (p.downloaded, p.total)
            })
            .collect()
    }

    fn fractions(v: &[(u64, Option<u64>)]) -> Vec<f64> {
        v.iter()
            .map(|(d, t)| *d as f64 / t.unwrap() as f64)
            .collect()
    }

    /// A fragmented 1080p stream whose estimate grows, then the separate audio from 0: the bar
    /// only moves forward, and ends at the whole.
    #[test]
    fn video_then_audio_never_moves_back() {
        let lines = [
            "100 NA 1000 5",
            "300 NA 1300 5",
            "600 NA 1500 5",
            "1400 NA 1600 5",
            "1600 1600 NA 5",
            "50 200 NA 5",
            "150 200 NA 5",
            "200 200 NA 5",
        ];
        for expected in [None, Some(1800)] {
            let f = fractions(&run(expected, &lines));
            for w in f.windows(2) {
                assert!(w[1] >= w[0] - 1e-9, "went back: {f:?}");
            }
            let last = run(expected, &lines).pop().unwrap();
            assert_eq!(last.0, 1800);
            assert!((f[f.len() - 1] - 1.0).abs() < 1e-9, "{f:?}");
        }
    }

    /// The size the window showed counts both streams from the start: the first stream alone
    /// never looks like the whole download.
    #[test]
    fn expected_size_holds_the_bar_back() {
        let v = run(Some(1000), &["400 400 NA 0"]);
        assert_eq!(v[0], (400, Some(1000)));
        let v = run(None, &["NA NA NA NA"]);
        assert_eq!(v[0], (0, None));
    }

    /// The subtitle arguments (#68): none asked, none passed; beside the video as SRT with
    /// ffmpeg, the site's own format without; embedded only with ffmpeg; every uploader's
    /// language but a live stream's chat.
    #[test]
    fn subtitle_arguments() {
        let subs = |langs: &[&str], automatic: bool, embed: bool| Subs {
            langs: langs.iter().map(|l| l.to_string()).collect(),
            automatic,
            embed,
        };
        let s = |v: Vec<String>| v.join(" ");
        assert!(subtitle_args(&Subs::default(), true).is_empty());
        assert_eq!(
            s(subtitle_args(&subs(&["fr"], false, false), true)),
            "--write-subs --sub-langs fr --sub-format srt/best --convert-subs srt --ignore-errors"
        );
        assert_eq!(
            s(subtitle_args(&subs(&["fr"], false, false), false)),
            "--write-subs --sub-langs fr --sub-format srt/vtt/best --ignore-errors"
        );
        assert_eq!(
            s(subtitle_args(&subs(&["en-orig"], true, true), true)),
            "--write-auto-subs --sub-langs en-orig --sub-format srt/best --convert-subs srt --embed-subs --ignore-errors"
        );
        assert_eq!(
            s(subtitle_args(&subs(&[ALL_UPLOADERS], false, true), true)),
            "--sub-langs all,-live_chat --sub-format srt/best --convert-subs srt --embed-subs --ignore-errors"
        );
        // Embedding needs ffmpeg: without it they're saved beside the video.
        assert_eq!(
            s(subtitle_args(&subs(&["de", "fr"], false, true), false)),
            "--write-subs --sub-langs de,fr --sub-format srt/vtt/best --ignore-errors"
        );
    }

    /// yt-dlp's warnings for subtitles it couldn't get (#68): each language once, a site's
    /// "too many requests" told apart from other reasons, other warnings ignored.
    #[test]
    fn refused_subtitles_from_the_warnings() {
        let w: Vec<String> = [
            "Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests",
            "[youtube] abc: Some formats are missing",
            "Unable to download video subtitles for 'de': HTTP Error 404: Not Found",
            "Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            subtitle_failures(&w),
            [
                ("en".to_string(), SubReason::TooManyRequests),
                (
                    "de".to_string(),
                    SubReason::Other("HTTP Error 404: Not Found".into())
                ),
            ]
        );
        assert!(subtitle_failures(&[]).is_empty());
        // The retry fetches only those, beside the video, never into it.
        let r = SubsRetry {
            automatic: true,
            embed: true,
            ffmpeg: true,
            base: vec!["-o".into(), "x.%(ext)s".into()],
        };
        assert_eq!(
            r.args(&["en".into()]).join(" "),
            "-o x.%(ext)s --skip-download --write-auto-subs --sub-langs en --sub-format srt/best --convert-subs srt --ignore-errors"
        );
        let info = serde_json::json!({"automatic_captions": {"en": [{"name": "English"}]}});
        assert_eq!(sub_name(&info, "en"), "English");
        assert_eq!(sub_name(&info, "ru"), "ru");
    }

    /// The files found beside a finished video are its own subtitles only.
    #[test]
    fn side_files_are_the_videos_subtitles() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "Talk.mp4",
            "Talk.fr.srt",
            "Talk.en-orig.vtt",
            "Talk.part",
            "Talk 2.en.srt",
            "Other.fr.srt",
            "Talk.jpg",
        ] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        let names: Vec<String> = side_files(&dir.path().join("Talk.mp4"))
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Talk.en-orig.vtt", "Talk.fr.srt"]);
    }

    #[test]
    fn percent_signs_in_titles_stay_text() {
        assert_eq!(template_text("100% fun"), "100%% fun");
    }
}

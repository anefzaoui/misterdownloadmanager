//! Supervised helper processes (yt-dlp, gallery-dl): every child is in one Windows Job Object
//! that kills it when we exit (even on a crash), runs without a console window, gets its
//! arguments in fixed layers (the user's extras, ours, then the fixed ones and `--` before the
//! URL) with CR / LF / NUL refused, keeps only a short stderr tail for messages, and goes through
//! the chosen network adapter (#35, `crate::relay`).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Lines of stderr kept for error messages.
const TAIL: usize = 20;
/// How many `WARNING:` lines a run keeps (`Spawned::warnings`).
const WARNINGS_KEPT: usize = 50;

#[derive(Debug, thiserror::Error)]
pub enum ProcError {
    #[error("an argument contains a line break or NUL")]
    BadArgument,
    #[error("could not start {0}: {1}")]
    Start(String, std::io::Error),
    #[error("{0} took too long and was stopped")]
    Timeout(String),
    #[error("{tool} failed: {message}")]
    Failed { tool: String, message: String },
    /// The chosen network adapter is down, or yt-dlp can't be held to it here (#35).
    #[error(transparent)]
    Adapter(#[from] mdm_engine::NetError),
}

/// Options → Proxy / Socks → Network adapter (#35) for the helpers and the media-server checks;
/// the app shares it as it changes ([`set_route`]).
static ROUTE: std::sync::RwLock<mdm_engine::Route> = std::sync::RwLock::new(mdm_engine::Route::Any);
/// Options → Connection → DNS (#36): what the relay looks the helpers' servers up with.
static DNS: std::sync::RwLock<Option<mdm_engine::DnsChoice>> = std::sync::RwLock::new(None);
/// The relay the helpers go through while an adapter is chosen, started when first needed.
static RELAY: Mutex<Option<Arc<crate::relay::Relay>>> = Mutex::new(None);

/// The network adapter the helpers and the media-server checks go through (the app's setting).
/// A change closes what the relay has open (it was held to the old route).
pub fn set_route(route: mdm_engine::Route) {
    if *ROUTE.read().unwrap() != route {
        *ROUTE.write().unwrap() = route;
        if let Some(relay) = RELAY.lock().unwrap().as_ref() {
            relay.route_changed();
        }
    }
}

pub fn route() -> mdm_engine::Route {
    ROUTE.read().unwrap().clone()
}

/// The DNS the helpers' servers are looked up with while an adapter is chosen (the app's
/// setting; the system's until set).
pub fn set_dns(dns: mdm_engine::DnsChoice) {
    if DNS.read().unwrap().as_ref() != Some(&dns) {
        *DNS.write().unwrap() = Some(dns);
    }
}

fn dns() -> mdm_engine::DnsChoice {
    DNS.read().unwrap().clone().unwrap_or_default()
}

/// The helpers' relay, started on first use.
fn relay() -> Result<Arc<crate::relay::Relay>, ProcError> {
    let mut slot = RELAY.lock().unwrap();
    if let Some(r) = slot.as_ref() {
        return Ok(r.clone());
    }
    let relay = crate::relay::Relay::start(Arc::new(|| (route(), dns())))
        .map(Arc::new)
        .map_err(|e| ProcError::Start("the network adapter's relay".into(), e))?;
    *slot = Some(relay.clone());
    Ok(relay)
}

/// Which helper [`through_adapter`] sets up.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Helper {
    YtDlp,
    GalleryDl,
}

/// A helper and the chosen adapter (#35): nothing to do when there's none; refused while it is
/// down; else every connection the helper and what it starts make goes through the relay
/// (`crate::relay`), which holds them to the adapter. The arguments returned go before `--`.
///
/// - yt-dlp: `--proxy` for its own requests (and the external downloaders it passes it to), and
///   `--geo-verification-proxy`. The ffmpeg it starts gets the proxy from it (`http_proxy`), and
///   may only open what goes through it: HTTP(S) and the layers under them, local files, `data:`
///   and `crypto:` (`-protocol_whitelist`). RTMP and FTP formats go to that ffmpeg, which refuses
///   them, instead of rtmpdump or yt-dlp's own FTP, which would ignore the proxy; RTSP and MMS
///   formats (mpv or mplayer, in older yt-dlp) stop the download (`--break-match-filters`; `?`
///   lets an entry not yet chosen pass, a playlist's).
/// - gallery-dl: `--proxy`.
/// - Both, and anything they start: `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` (curl, aria2c,
///   gallery-dl's Python), with `NO_PROXY` taken away so no host is let around it.
fn through_adapter(cmd: &mut Command, helper: Helper) -> Result<Vec<String>, ProcError> {
    match route() {
        mdm_engine::Route::Any => Ok(Vec::new()),
        mdm_engine::Route::Down { label } => Err(mdm_engine::NetError::Down { label }.into()),
        mdm_engine::Route::Bound(_) => {
            let url = relay()?.url();
            for var in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                cmd.env(var, &url);
            }
            cmd.env_remove("NO_PROXY").env_remove("no_proxy");
            Ok(match helper {
                Helper::YtDlp => vec![
                    "--proxy".into(),
                    url.clone(),
                    "--geo-verification-proxy".into(),
                    url,
                    // yt-dlp's keys are http, ftp, m3u8, dash and rtmp only: anything else
                    // ("rtsp:ffmpeg") would make ffmpeg the downloader for everything.
                    "--downloader".into(),
                    "rtmp:ffmpeg".into(),
                    "--downloader".into(),
                    "ftp:ffmpeg".into(),
                    "--downloader-args".into(),
                    format!("ffmpeg_i:-protocol_whitelist {FFMPEG_PROTOCOLS}"),
                    "--break-match-filters".into(),
                    "protocol!*=?rtsp & protocol!*=?mms".into(),
                ],
                Helper::GalleryDl => vec!["--proxy".into(), url],
            })
        }
    }
}

/// What the ffmpeg yt-dlp starts may open while an adapter is chosen: HTTP(S) through the relay
/// (`httpproxy`, `tls` and `tcp` are the layers under them; HLS and DASH open nothing else) and
/// what stays on this computer.
const FFMPEG_PROTOCOLS: &str = "file,crypto,data,http,https,tls,tcp,httpproxy";

/// `gallery-dl` (#58) with arguments in the same layers as [`ytdlp`]: `ours`, then the fixed
/// ones (no configuration file of the person's, never a question on the console), then `--` and
/// the address. Its output is read as UTF-8, as yt-dlp's is (#79). With an adapter chosen it goes
/// through the relay (#35, [`through_adapter`]).
pub fn gallery_dl(exe: &Path, ours: &[String], url: &str) -> Result<Command, ProcError> {
    let mut cmd = Command::new(exe);
    let route = through_adapter(&mut cmd, Helper::GalleryDl)?;
    cmd.env("PYTHONIOENCODING", "utf-8");
    for a in ours {
        cmd.arg(safe(a)?);
    }
    cmd.args(["--ignore-config", "--no-input"]);
    cmd.args(route);
    cmd.arg("--");
    cmd.arg(safe(url)?);
    Ok(cmd)
}

/// Text that may go into an argument list: no line breaks or NUL (they would let a page title
/// or URL smuggle in options or split a line-based protocol).
pub fn safe(s: &str) -> Result<&str, ProcError> {
    if s.contains(['\r', '\n', '\0']) {
        Err(ProcError::BadArgument)
    } else {
        Ok(s)
    }
}

/// What the browser's request for a page carried, so yt-dlp asks as the browser did: the page
/// that embeds a player (Referer), the browser's User-Agent, and the page's cookies (a Cookie
/// header's `name=value; …`), which sign a member's or a site's own videos in.
#[derive(Clone, Debug, Default)]
pub struct FromBrowser {
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub cookies: Option<String>,
}

/// A private cookie file in the Netscape format yt-dlp's `--cookies` reads, removed when
/// dropped (yt-dlp may write it back at its end: keep it until then).
pub struct CookieFile(PathBuf);

impl Drop for CookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl FromBrowser {
    /// yt-dlp's arguments for `url`'s page, and the cookie file they name. Cookies never go on
    /// the command line, where other programs can read them.
    pub fn args(&self, url: &str) -> Result<(Vec<String>, Option<CookieFile>), ProcError> {
        fn given(s: &Option<String>) -> Option<&str> {
            s.as_deref().map(str::trim).filter(|s| !s.is_empty())
        }
        let mut args = Vec::new();
        if let Some(r) = given(&self.referer) {
            args.extend(["--referer".to_owned(), safe(r)?.to_owned()]);
        }
        if let Some(ua) = given(&self.user_agent) {
            args.extend(["--user-agent".to_owned(), safe(ua)?.to_owned()]);
        }
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned));
        let file = match (given(&self.cookies), host) {
            (Some(cookies), Some(host)) => {
                let file = cookie_file(&host, cookies)
                    .map_err(|e| ProcError::Start("yt-dlp".into(), e))?;
                args.extend([
                    "--cookies".to_owned(),
                    file.0.to_string_lossy().into_owned(),
                ]);
                Some(file)
            }
            _ => None,
        };
        Ok((args, file))
    }
}

/// `cookies` (`a=1; b=2`) for `host` and its subdomains, as session cookies, in a file only this
/// user can read.
fn cookie_file(host: &str, cookies: &str) -> std::io::Result<CookieFile> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "mdm-cookies-{}-{}.txt",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let mut text = String::from("# Netscape HTTP Cookie File\n");
    for pair in cookies.split(';') {
        let Some((name, value)) = pair.trim().split_once('=') else {
            continue;
        };
        if [host, name, value]
            .iter()
            .any(|s| s.contains(['\t', '\r', '\n', '\0']))
        {
            continue;
        }
        text.push_str(&format!(".{host}\tTRUE\t/\tFALSE\t0\t{name}\t{value}\n"));
    }
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    // The temporary folder is the user's own on Windows; elsewhere it may be shared.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    let file = CookieFile(path);
    open.open(&file.0)?.write_all(text.as_bytes())?;
    Ok(file)
}

/// What yt-dlp prints is read as UTF-8 (#79): on a Windows with a legacy code page (Turkish,
/// Russian…) Python writes to a pipe in that code page, and a title's letters made the
/// final-path line unreadable. `--encoding` sets yt-dlp's own output, `PYTHONIOENCODING`
/// Python's.
const UTF8: [&str; 2] = ["--encoding", "utf-8"];

fn ytdlp_command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd
}

/// `yt-dlp` with arguments in layers: `user` extras, then `ours`, then the fixed ones last
/// (so nothing can switch them off), then `--` and the URL (so a URL starting with `-` is never
/// read as an option).
pub fn ytdlp(
    exe: &Path,
    user: &[String],
    ours: &[String],
    url: &str,
) -> Result<Command, ProcError> {
    let mut cmd = ytdlp_command(exe);
    let route = through_adapter(&mut cmd, Helper::YtDlp)?;
    for a in user.iter().chain(ours) {
        cmd.arg(safe(a)?);
    }
    cmd.args(["--ignore-config", "--no-exec", "--no-update"]);
    cmd.args(UTF8);
    cmd.args(route);
    cmd.arg("--");
    cmd.arg(safe(url)?);
    Ok(cmd)
}

/// `yt-dlp` working from a looked-up page (`-J` output, maybe edited) instead of an address:
/// the same layers, then `--load-info-json` and the file.
pub fn ytdlp_info(
    exe: &Path,
    user: &[String],
    ours: &[String],
    info: &Path,
) -> Result<Command, ProcError> {
    let mut cmd = ytdlp_command(exe);
    let route = through_adapter(&mut cmd, Helper::YtDlp)?;
    for a in user.iter().chain(ours) {
        cmd.arg(safe(a)?);
    }
    cmd.args(["--ignore-config", "--no-exec", "--no-update"]);
    cmd.args(UTF8);
    cmd.args(route);
    cmd.arg("--load-info-json");
    cmd.arg(info);
    Ok(cmd)
}

/// A job of its own for one helper and every process it starts (#61): on Windows `yt-dlp.exe`
/// is a launcher whose child does the work, and ffmpeg runs under that child. Closing the job
/// (`Spawned::kill`, or MDM ending in any way) ends them all; ending the launcher alone left the
/// download running, holding its file and the bandwidth.
#[cfg(windows)]
fn new_job() -> Option<win32job::Job> {
    let mut info = win32job::ExtendedLimitInfo::new();
    info.limit_kill_on_job_close();
    win32job::Job::create_with_limit_info(&info).ok()
}

/// A running helper with its stderr tail.
pub struct Spawned {
    pub child: Child,
    tail: Arc<Mutex<VecDeque<String>>>,
    /// Every `WARNING:` line (a few dozen at most): a subtitle yt-dlp couldn't get is said early,
    /// and the tail has moved past it by the end (#68).
    warnings: Arc<Mutex<Vec<String>>>,
    /// The stderr reader has read to the end (the helper and all it started closed it).
    stderr_done: Arc<std::sync::atomic::AtomicBool>,
    name: String,
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

impl Spawned {
    /// End the helper and everything it started (Windows: its job; elsewhere yt-dlp runs as one
    /// process, its ffmpeg ending with it).
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        #[cfg(windows)]
        drop(self.job.take());
    }

    /// The last stderr lines, for an error message.
    pub fn tail(&self) -> String {
        self.tail
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The tool's `WARNING:` lines so far, oldest first, without the word.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.lock().unwrap().clone()
    }

    /// A failure with what the tool said (its last `ERROR:` line, else its last line). Called
    /// once it has ended: its last lines are waited for (the stderr reader is another thread, and
    /// on a busy machine it came after the exit was seen: "stopped without saying why"), up to
    /// two seconds, as something it started may keep stderr open.
    pub fn failure(&self) -> ProcError {
        let end = Instant::now() + Duration::from_secs(2);
        while !self.stderr_done.load(std::sync::atomic::Ordering::Acquire) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        let tail = self.tail.lock().unwrap();
        let message = tail
            .iter()
            .rev()
            .find(|l| l.starts_with("ERROR:"))
            .or(tail.back())
            .map(|l| l.trim_start_matches("ERROR:").trim().to_owned())
            .unwrap_or_else(|| "it stopped without saying why".into());
        ProcError::Failed {
            tool: self.name.clone(),
            message,
        }
    }
}

/// Start `cmd` supervised: no window, in the job, stdout piped for the caller, stderr kept as a
/// tail.
pub fn spawn(mut cmd: Command, name: &str) -> Result<Spawned, ProcError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| ProcError::Start(name.to_owned(), e))?;
    #[cfg(windows)]
    let job = new_job().filter(|job| {
        use std::os::windows::io::AsRawHandle;
        job.assign_process(child.as_raw_handle() as isize).is_ok()
    });
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let stderr_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(err) = child.stderr.take() {
        let (tail, warnings, done) = (tail.clone(), warnings.clone(), stderr_done.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(w) = line.strip_prefix("WARNING:") {
                    let mut ws = warnings.lock().unwrap();
                    if ws.len() < WARNINGS_KEPT {
                        ws.push(w.trim().to_owned());
                    }
                }
                let mut t = tail.lock().unwrap();
                if t.len() >= TAIL {
                    t.pop_front();
                }
                t.push_back(line);
            }
            done.store(true, std::sync::atomic::Ordering::Release);
        });
    } else {
        stderr_done.store(true, std::sync::atomic::Ordering::Release);
    }
    Ok(Spawned {
        child,
        tail,
        warnings,
        stderr_done,
        name: name.to_owned(),
        #[cfg(windows)]
        job,
    })
}

/// Run `cmd` to the end within `timeout` and return its stdout (killed on timeout).
pub fn output(cmd: Command, name: &str, timeout: Duration) -> Result<Vec<u8>, ProcError> {
    let mut s = spawn(cmd, name)?;
    let mut out = s.child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    loop {
        match s.child.try_wait() {
            Ok(Some(status)) => {
                let stdout = reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(stdout)
                } else {
                    // `failure` waits for the stderr reader's last lines.
                    Err(s.failure())
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                s.kill();
                let _ = s.child.wait();
                return Err(ProcError::Timeout(name.to_owned()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(ProcError::Start(name.to_owned(), e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browsers_context_for_ytdlp() {
        let b = FromBrowser {
            referer: Some("https://course.test/lesson".into()),
            user_agent: Some("UA/1".into()),
            cookies: Some("sid=abc; theme=dark".into()),
        };
        let (args, file) = b.args("https://player.test/video/1").unwrap();
        assert_eq!(
            &args[..4],
            [
                "--referer",
                "https://course.test/lesson",
                "--user-agent",
                "UA/1"
            ]
        );
        assert_eq!(args[4], "--cookies");
        let file = file.unwrap();
        let text = std::fs::read_to_string(&file.0).unwrap();
        assert!(text.starts_with("# Netscape HTTP Cookie File"));
        assert!(text.contains(".player.test\tTRUE\t/\tFALSE\t0\tsid\tabc\n"));
        assert!(
            !args.iter().any(|a| a.contains("abc")),
            "no cookie on the command line"
        );
        let path = file.0.clone();
        drop(file);
        assert!(!path.exists(), "removed once done");
        let none = FromBrowser::default().args("https://x.test/").unwrap();
        assert!(none.0.is_empty() && none.1.is_none());
    }

    #[test]
    fn hostile_text_never_reaches_the_argument_list() {
        let exe = Path::new("yt-dlp");
        assert!(ytdlp(exe, &[], &[], "https://x/\n--exec calc").is_err());
        assert!(ytdlp(exe, &["--format\0best".into()], &[], "https://x/").is_err());
    }

    #[test]
    fn fixed_arguments_come_last_and_the_url_after_the_separator() {
        let cmd = ytdlp(
            Path::new("yt-dlp"),
            &["--user".into()],
            &["-J".into()],
            "-https://x/",
        )
        .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--user",
                "-J",
                "--ignore-config",
                "--no-exec",
                "--no-update",
                "--encoding",
                "utf-8",
                "--",
                "-https://x/"
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn output_and_failures_of_a_real_process() {
        let mut ok = Command::new("cmd");
        ok.args(["/C", "echo hello"]);
        let out = output(ok, "cmd", Duration::from_secs(10)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out).trim(), "hello");
        let mut bad = Command::new("cmd");
        bad.args(["/C", "echo ERROR: no such video 1>&2 & exit 1"]);
        let err = output(bad, "cmd", Duration::from_secs(10)).unwrap_err();
        assert!(err.to_string().contains("no such video"), "{err}");
        let mut slow = Command::new("cmd");
        slow.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
        let err = output(slow, "cmd", Duration::from_millis(500)).unwrap_err();
        assert!(matches!(err, ProcError::Timeout(_)), "{err}");
    }

    /// #61: killing a helper ends what it started too (yt-dlp.exe is a launcher whose child
    /// downloads). Here a script starts a child that writes a line every second: once killed,
    /// the file stops growing.
    #[cfg(windows)]
    #[test]
    fn killing_a_helper_ends_its_children() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker.txt");
        let child = dir.path().join("child.cmd");
        let parent = dir.path().join("parent.cmd");
        std::fs::write(
            &child,
            format!(
                "@for /l %%i in (1,1,60) do @(echo x>>\"{}\" & ping -n 2 127.0.0.1 >nul)\r\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::write(
            &parent,
            format!(
                "@start \"\" /b cmd /c \"{}\"\r\n@ping -n 60 127.0.0.1 >nul\r\n",
                child.display()
            ),
        )
        .unwrap();
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(&parent);
        let mut s = spawn(cmd, "cmd").unwrap();
        let size = || std::fs::metadata(&marker).map_or(0, |m| m.len());
        std::thread::sleep(Duration::from_secs(3));
        assert!(size() > 0, "the child runs");
        s.kill();
        std::thread::sleep(Duration::from_millis(1500));
        let after_kill = size();
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(size(), after_kill, "the child ended with the helper");
    }
}

//! Supervised helper processes (yt-dlp, later ffmpeg): every child is in one Windows Job Object
//! that kills it when we exit (even on a crash), runs without a console window, gets its
//! arguments in fixed layers (the user's extras, ours, then the fixed ones and `--` before the
//! URL) with CR / LF / NUL refused, and keeps only a short stderr tail for messages.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Lines of stderr kept for error messages.
const TAIL: usize = 20;

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

/// `yt-dlp` with arguments in layers: `user` extras, then `ours`, then the fixed ones last
/// (so nothing can switch them off), then `--` and the URL (so a URL starting with `-` is never
/// read as an option).
pub fn ytdlp(
    exe: &Path,
    user: &[String],
    ours: &[String],
    url: &str,
) -> Result<Command, ProcError> {
    let mut cmd = Command::new(exe);
    for a in user.iter().chain(ours) {
        cmd.arg(safe(a)?);
    }
    cmd.args(["--ignore-config", "--no-exec", "--no-update", "--"]);
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
    let mut cmd = Command::new(exe);
    for a in user.iter().chain(ours) {
        cmd.arg(safe(a)?);
    }
    cmd.args([
        "--ignore-config",
        "--no-exec",
        "--no-update",
        "--load-info-json",
    ]);
    cmd.arg(info);
    Ok(cmd)
}

/// The one job every child joins; it lives as long as the process (dropping it would kill
/// them).
#[cfg(windows)]
fn job() -> Option<&'static win32job::Job> {
    static JOB: std::sync::OnceLock<Option<win32job::Job>> = std::sync::OnceLock::new();
    JOB.get_or_init(|| {
        let mut info = win32job::ExtendedLimitInfo::new();
        info.limit_kill_on_job_close();
        win32job::Job::create_with_limit_info(&info).ok()
    })
    .as_ref()
}

/// A running helper with its stderr tail.
pub struct Spawned {
    pub child: Child,
    tail: Arc<Mutex<VecDeque<String>>>,
    name: String,
}

impl Spawned {
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

    /// A failure with what the tool said (its last `ERROR:` line, else its last line).
    pub fn failure(&self) -> ProcError {
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
    if let Some(job) = job() {
        use std::os::windows::io::AsRawHandle;
        let _ = job.assign_process(child.as_raw_handle() as isize);
    }
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(err) = child.stderr.take() {
        let tail = tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let mut t = tail.lock().unwrap();
                if t.len() >= TAIL {
                    t.pop_front();
                }
                t.push_back(line);
            }
        });
    }
    Ok(Spawned {
        child,
        tail,
        name: name.to_owned(),
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
                    // Let the stderr reader catch up with the last lines.
                    std::thread::sleep(Duration::from_millis(50));
                    Err(s.failure())
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = s.child.kill();
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
}

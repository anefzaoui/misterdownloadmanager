//! Subtitles a site refuses never cost the video (#68), against a fake yt-dlp that answers as the
//! real one did for YouTube's automatic English captions: `Unable to download video subtitles for
//! 'en': HTTP Error 429: Too Many Requests`. Without `--ignore-errors` the fake fails the whole
//! run, as yt-dlp does; with it, it warns and saves the video.
//!
//! One program, three roles (no test harness, `harness = false`):
//! - run as `fake-ytdlp[.exe]` (a copy of itself): it is yt-dlp;
//! - run with `MDM_SUBS_RETRY_SECS` set (the retry's pause, shortened): it runs the checks;
//! - run plainly (`cargo test`): it runs itself again with that variable, the way to set it
//!   without touching its own environment.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use mdm_tools::video::{State, SubReason, Subs, VideoJob};

fn main() -> ExitCode {
    let me = std::env::current_exe().expect("own path");
    let name = me.file_stem().map(|s| s.to_string_lossy().into_owned());
    if name.as_deref() == Some("fake-ytdlp") {
        return fake_ytdlp();
    }
    if std::env::var_os("MDM_SUBS_RETRY_SECS").is_some() {
        checks(&me);
        println!("fake_ytdlp: ok");
        return ExitCode::SUCCESS;
    }
    let status = Command::new(&me)
        .env("MDM_SUBS_RETRY_SECS", "1")
        .status()
        .expect("run the checks");
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// What the fake does, from a file beside the output: `refuse-always` (English is refused every
/// time) or `refuse-once` (the first time only).
fn mode(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("mode.txt")).unwrap_or_default()
}

fn fake_ytdlp() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("-J") {
        const PAGE: &str = r#"{"title": "Talk", "language": "ru", "formats": [
            {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a"}],
            "automatic_captions": {"en": [{"name": "English"}], "ru": [{"name": "Russian"}]}}"#;
        println!("{PAGE}");
        return ExitCode::SUCCESS;
    }
    let template = after("-o").expect("-o");
    let dir = Path::new(&template).parent().expect("a folder").to_owned();
    let mode = mode(&dir);
    let tries = dir.join("tries.txt");
    let tried = std::fs::read_to_string(&tries)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .unwrap_or(0);
    std::fs::write(&tries, (tried + 1).to_string()).unwrap();
    let langs = after("--sub-langs").unwrap_or_default();
    for lang in langs.split(',').filter(|l| !l.is_empty()) {
        let refused = lang == "en" && (mode == "refuse-always" || tried == 0);
        if refused {
            let why =
                "Unable to download video subtitles for 'en': HTTP Error 429: Too Many Requests";
            if has("--ignore-errors") {
                eprintln!("WARNING: {why}");
                continue;
            }
            eprintln!("ERROR: {why}");
            return ExitCode::FAILURE;
        }
        let file = template.replace("%(ext)s", &format!("{lang}.srt"));
        std::fs::write(file, "1\n00:00:00,000 --> 00:00:01,000\nHello\n").unwrap();
    }
    if !has("--skip-download") {
        let video = template.replace("%(ext)s", "mp4");
        std::fs::write(&video, vec![0u8; 4096]).unwrap();
        println!("MDMF {video}");
    }
    ExitCode::SUCCESS
}

/// The fake, as a copy of this program named `fake-ytdlp` in `dir`.
fn fake_in(me: &Path, dir: &Path) -> PathBuf {
    let exe = dir.join(format!("fake-ytdlp{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(me, &exe).expect("copy the fake");
    exe
}

/// Download with `subs` while the fake behaves as `mode`; the end state, its log and folder.
fn download(me: &Path, mode: &str, subs: &Subs) -> (mdm_tools::video::Progress, Vec<String>) {
    let tools = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("mode.txt"), mode).unwrap();
    let exe = fake_in(me, tools.path());
    let job = VideoJob::start(
        &exe,
        "https://video.example/talk",
        "b",
        out.path(),
        "Talk",
        None,
        None,
        None,
        &Default::default(),
        subs,
    )
    .expect("started");
    let end = Instant::now() + Duration::from_secs(60);
    let p = loop {
        let p = job.progress();
        if p.state != State::Running {
            break p;
        }
        assert!(Instant::now() < end, "still running: {p:?}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut files: Vec<String> = std::fs::read_dir(out.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".txt"))
        .collect();
    files.sort();
    (p, files)
}

fn checks(me: &Path) {
    let subs = |langs: &[&str], embed: bool| Subs {
        langs: langs.iter().map(|l| l.to_string()).collect(),
        automatic: true,
        embed,
    };

    // English refused every time: the video and the Russian ones are saved, English is told.
    let (p, files) = download(me, "refuse-always", &subs(&["en", "ru"], false));
    assert!(matches!(p.state, State::Done(_)), "{:?}", p.state);
    assert_eq!(files, ["Talk.mp4", "Talk.ru.srt"]);
    assert_eq!(p.sub_problems.len(), 1, "{:?}", p.sub_problems);
    let problem = &p.sub_problems[0];
    assert_eq!(
        (
            problem.code.as_str(),
            problem.name.as_str(),
            &problem.reason,
            problem.beside
        ),
        ("en", "English", &SubReason::TooManyRequests, false)
    );
    let log: Vec<&str> = p.events.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        log.iter()
            .any(|t| t.starts_with("Subtitles in en couldn't be downloaded")),
        "{log:?}"
    );

    // Refused once only: the second try, after the pause, brings them.
    let (p, files) = download(me, "refuse-once", &subs(&["en"], false));
    assert!(matches!(p.state, State::Done(_)), "{:?}", p.state);
    assert_eq!(files, ["Talk.en.srt", "Talk.mp4"]);
    assert!(p.sub_problems.is_empty(), "{:?}", p.sub_problems);
    assert_eq!(
        p.side_files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["Talk.en.srt"]
    );

    // Asked into the video (no ffmpeg here, so beside it): still never the video's failure.
    let (p, files) = download(me, "refuse-always", &subs(&["en"], true));
    assert!(matches!(p.state, State::Done(_)), "{:?}", p.state);
    assert_eq!(files, ["Talk.mp4"]);
    assert_eq!(p.sub_problems.len(), 1);
}

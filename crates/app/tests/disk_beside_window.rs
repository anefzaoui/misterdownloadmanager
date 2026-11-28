//! #1292: the window never waits on the disk. An antivirus that checks each file a program opens
//! (Bitdefender) held a file open made on the window's thread for 8 s, and the window froze
//! meanwhile. The real app, off the screens (MDM_OFFSCREEN), with a data folder and a download
//! folder of its own and the lab server on 127.0.0.1: from its first frame on, every file call
//! MDM makes waits [`SLOW_MS`] first, on whatever thread (MDM_TEST_SLOW_DISK, as such an
//! antivirus makes it wait), the window thread's own calls are logged (MDM_DISK_WATCH) and its
//! frames timed (MDM_TEST_FRAME_STATS). Downloads named in several scripts (their names need
//! fonts the window doesn't have yet) come down and finish, windows open, and the main window
//! goes to the notification area: the window thread makes no file call, no frame is slow, and no
//! freeze is reported.
//!
//! Windows only: the watch and the slow disk go through the program's import table, which only
//! Windows programs have (diskwatch.rs); Linux is checked with strace on the test machine.
//! It opens windows, which needs OpenGL or Direct3D, so it runs on request:
//! `cargo test -p mdm-app --test disk_beside_window -- --ignored`.

#![cfg(windows)]

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use mdm_labserver::{Config, LabServer};

const APP: &str = env!("CARGO_BIN_EXE_misterdownloadmanager");

/// How long each file call waits: past the slow frame the log names (500 ms), well short of a
/// freeze (8 s), so a call on the window thread shows as a slow frame and the test ends in time.
const SLOW_MS: u64 = 1_500;

/// The app's and its monitor's logs so far (every file of `logs/`).
fn log(data: &Path) -> String {
    let mut all = String::new();
    if let Ok(dir) = std::fs::read_dir(data.join("logs")) {
        for f in dir.flatten() {
            all += &std::fs::read_to_string(f.path()).unwrap_or_default();
        }
    }
    all
}

/// Wait until `done` holds, failing if the app quits first or `wait` passes.
fn wait_until(app: &mut Child, data: &Path, what: &str, wait: Duration, done: impl Fn() -> bool) {
    let until = Instant::now() + wait;
    loop {
        let text = log(data);
        assert!(!text.contains("panic"), "the app panicked:\n{text}");
        if done() {
            return;
        }
        if let Some(status) = app.try_wait().unwrap() {
            panic!("the app closed ({status}) before {what}:\n{text}");
        }
        assert!(Instant::now() < until, "no {what} after {wait:?}:\n{text}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// A second launch with `args`: it hands them to the running app and quits.
fn second_launch(data: &Path, args: &[&str]) {
    let status = Command::new(APP)
        .args(args)
        .env("MDM_DATA_DIR", data)
        .env("MDM_OFFSCREEN", "1")
        .status()
        .unwrap();
    assert!(status.success(), "second launch {args:?}: {status}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
async fn the_window_draws_on_while_every_file_call_waits() {
    let server = LabServer::start(Config::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let files = dir.path().join("files");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&files).unwrap();
    // No update check, no first-start questions; the torrent engine stays on this computer.
    let settings = serde_json::json!({
        "language": "en", "update_check": false, "helpers_asked": true, "language_asked": true,
        "extension_never_ask": true, "clipboard_autostart": false,
        "torrent_dht": false, "torrent_lsd": false, "torrent_upnp": false,
        "torrent_incoming": false,
        "category_dirs": [["General", files.to_string_lossy()]],
    });
    std::fs::write(
        data.join("downloads.json"),
        serde_json::json!({ "next_id": 1, "items": [], "version": 2, "settings": settings })
            .to_string(),
    )
    .unwrap();
    let mut app = Command::new(APP)
        .env("MDM_DATA_DIR", &data)
        .env("MDM_OFFSCREEN", "1")
        .env("MDM_DISK_WATCH", "1")
        .env("MDM_TEST_FRAME_STATS", "1")
        .env("MDM_TEST_SLOW_DISK", SLOW_MS.to_string())
        .spawn()
        .unwrap();
    let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_until(
            &mut app,
            &data,
            "first frame",
            Duration::from_secs(120),
            || log(&data).contains("first frame drawn"),
        );
        assert!(
            log(&data).contains("disk watch: the window thread's file calls are logged"),
            "the watch is on:\n{}",
            log(&data)
        );
        // Names in scripts the interface's fonts don't have: their fonts are found on demand.
        let names = [
            "plain.bin",
            "\u{6587}\u{4EF6}.bin",
            "\u{0645}\u{0644}\u{0641}.zip",
            "\u{0E44}\u{0E1F}\u{0E25}.mp4",
            "\u{10D0}\u{10D1}.pdf",
        ];
        for (n, name) in names.iter().enumerate() {
            let url = server.url(&format!("/gen/256K/f{n}.bin?seed={n}&rate=128K"));
            let at = files.to_string_lossy().into_owned();
            second_launch(&data, &["/d", &url, "/p", &at, "/f", name, "/n"]);
        }
        wait_until(
            &mut app,
            &data,
            "every download",
            Duration::from_secs(120),
            || names.iter().all(|n| files.join(n).is_file()),
        );
        for window in ["addurl", "find", "about", "options:connection", "tray"] {
            second_launch(&data, &["/show", window]);
            std::thread::sleep(Duration::from_secs(3));
        }
        // Long enough for a frame stats line after the last window.
        std::thread::sleep(Duration::from_secs(11));
        let text = log(&data);
        let after = &text[text.find("first frame drawn").unwrap()..];
        let calls: Vec<&str> = after
            .lines()
            .filter(|l| l.contains("disk watch: the window thread made a file call"))
            .collect();
        assert!(
            calls.is_empty(),
            "file calls on the window thread:\n{after}"
        );
        let slow: Vec<&str> = after
            .lines()
            .filter(|l| l.contains("a slow frame") || l.contains("stopped responding"))
            .collect();
        assert!(
            slow.is_empty(),
            "slow frames while the disk waits:\n{after}"
        );
        assert!(after.contains("frame stats:"), "frames timed:\n{after}");
    }));
    second_launch(&data, &["/exit"]);
    let until = Instant::now() + Duration::from_secs(60);
    while app.try_wait().unwrap().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(250));
    }
    let _ = app.kill();
    if let Err(e) = ended {
        std::panic::resume_unwind(e);
    }
}

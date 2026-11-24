//! Issue #1651: `/exit` (a second launch's, as the installer and Tasks → Exit send it) sometimes
//! didn't quit MDM while a dialog was open or the main window was minimized, and a hand-over
//! could reach the app twice (a "Duplicate download link" no second `/d` asked for). The real
//! app, off the screens (MDM_OFFSCREEN), with a data folder and a download folder of its own:
//! `/exit` in each window state must arrive and quit it, and many `/d` sent at once, with a
//! dialog open and the main window minimized, must each be in the list exactly once.
//!
//! It opens windows, which needs OpenGL or Direct3D, so it runs on request:
//! `cargo test -p mdm-app --test exit_handover -- --ignored --test-threads=1`.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const APP: &str = env!("CARGO_BIN_EXE_misterdownloadmanager");

/// The app's log so far (every file of `logs/`).
fn log(data: &Path) -> String {
    let mut all = String::new();
    if let Ok(dir) = std::fs::read_dir(data.join("logs")) {
        for f in dir.flatten() {
            all += &std::fs::read_to_string(f.path()).unwrap_or_default();
        }
    }
    all
}

/// Wait until the log has `line`, failing if the app quits first or `wait` passes.
fn wait_for(app: &mut Child, data: &Path, line: &str, wait: Duration) {
    let until = Instant::now() + wait;
    loop {
        let text = log(data);
        assert!(!text.contains("panic"), "the app panicked:\n{text}");
        if text.contains(line) {
            return;
        }
        if let Some(status) = app.try_wait().unwrap() {
            panic!("the app closed ({status}) before '{line}':\n{text}");
        }
        assert!(
            Instant::now() < until,
            "no '{line}' after {wait:?}:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(100));
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

/// A data folder with a download folder of its own beside it, and no first-start questions.
fn data_folder(root: &Path) -> PathBuf {
    let data = root.join("data");
    let downloads = root.join("downloads");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&downloads).unwrap();
    let settings = serde_json::json!({
        "update_check": false, "helpers_asked": true, "language_asked": true,
        "extension_never_ask": true, "minimize_to_tray": false,
        "torrent_dht": false, "torrent_lsd": false, "torrent_upnp": false,
        "torrent_incoming": false,
        "category_dirs": [["General", downloads.to_string_lossy()]],
    });
    std::fs::write(
        data.join("downloads.json"),
        serde_json::json!({ "next_id": 1, "items": [], "version": 2, "settings": settings })
            .to_string(),
    )
    .unwrap();
    data
}

fn start(data: &Path) -> Child {
    let mut app = Command::new(APP)
        .env("MDM_DATA_DIR", data)
        .env("MDM_OFFSCREEN", "1")
        .spawn()
        .unwrap();
    wait_for(
        &mut app,
        data,
        "window opened: main",
        Duration::from_secs(60),
    );
    app
}

/// Wait up to `wait` for the app to quit; it is stopped if it doesn't. Whether it quit.
fn quits_within(app: &mut Child, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if app.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = app.kill();
    let _ = app.wait();
    false
}

const MAGNET: &str = "magnet:?xt=urn:btih:5b4f0c3a9e1d27f86c0b3a4e9d1f2a7c6e8b0d14&dn=MDM%20test";

/// `/exit` after `before` (second launches, each waited for by its window's line), five times.
fn exit_after(before: &[(&[&str], Option<&str>)]) {
    for round in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let data = data_folder(dir.path());
        let mut app = start(&data);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for (args, opened) in before {
                second_launch(&data, args);
                if let Some(opened) = opened {
                    wait_for(&mut app, &data, opened, Duration::from_secs(30));
                }
            }
            std::thread::sleep(Duration::from_millis(500));
            second_launch(&data, &["/exit"]);
        }));
        let quit = quits_within(&mut app, Duration::from_secs(15));
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
        let text = log(&data);
        assert!(
            text.contains("command Exit"),
            "round {round}: /exit never arrived:\n{text}"
        );
        assert!(
            quit,
            "round {round}: MDM didn't quit 15 s after /exit:\n{text}"
        );
    }
}

#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn exit_with_the_main_window_only() {
    exit_after(&[]);
}

#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn exit_with_about_open_and_the_main_window_minimized() {
    exit_after(&[
        (&["/show", "about"], Some("window opened: about")),
        (&["/show", "minimized"], None),
    ]);
}

#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn exit_with_add_torrent_open_and_the_main_window_minimized() {
    exit_after(&[
        (&[MAGNET], Some("window opened: add-torrent")),
        (&["/show", "minimized"], None),
    ]);
}

#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn exit_with_about_open_and_the_main_window_in_the_tray() {
    exit_after(&[
        (&["/show", "about"], Some("window opened: about")),
        (&["/show", "tray"], None),
    ]);
}

/// `/exit` while the channel is full (as many connections as it handles at once, each saying
/// nothing, as slow clients do): it was closed unanswered, and the second launch gave `/exit` up.
/// Now it is told `busy` and tries again until the app has it.
#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn exit_while_the_channel_is_full() {
    for round in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let data = data_folder(dir.path());
        let mut app = start(&data);
        let ipc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(data.join("ipc.json")).unwrap()).unwrap();
        let port = ipc["port"].as_u64().unwrap() as u16;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // The channel handles 8 at once (mdm_ipc's MAX_IN_FLIGHT).
            let silent: Vec<_> = (0..8)
                .map(|_| std::net::TcpStream::connect(("127.0.0.1", port)).unwrap())
                .collect();
            std::thread::sleep(Duration::from_millis(300));
            second_launch(&data, &["/exit"]);
            drop(silent);
        }));
        let quit = quits_within(&mut app, Duration::from_secs(15));
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
        let text = log(&data);
        assert!(
            text.contains("command Exit"),
            "round {round}: /exit never arrived:\n{text}"
        );
        assert!(quit, "round {round}: MDM didn't quit after /exit:\n{text}");
    }
}

/// The downloads' addresses in the list, as saved.
fn listed(data: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(data.join("downloads.json")).unwrap_or_default();
    let list: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    list["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i["url"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Many `/d` at once (Download later: no window each), a dialog open and the main window
/// minimized: each link is in the list once, and no "Duplicate download link" opens.
#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn many_downloads_at_once_each_arrive_once() {
    let dir = tempfile::tempdir().unwrap();
    let data = data_folder(dir.path());
    let mut app = start(&data);
    // Port 9 on this computer: nothing answers, and nothing leaves it (and nothing starts).
    let urls: Vec<String> = (0..48)
        .map(|i| format!("http://127.0.0.1:9/mdm-test-1651-{i:03}.zip"))
        .collect();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        second_launch(&data, &["/show", "about"]);
        wait_for(
            &mut app,
            &data,
            "window opened: about",
            Duration::from_secs(30),
        );
        second_launch(&data, &["/show", "minimized"]);
        let launches: Vec<_> = urls
            .iter()
            .map(|url| {
                let (data, url) = (data.clone(), url.clone());
                std::thread::spawn(move || second_launch(&data, &["/d", &url, "/a"]))
            })
            .collect();
        for l in launches {
            l.join().unwrap();
        }
        std::thread::sleep(Duration::from_secs(2));
        second_launch(&data, &["/exit"]);
    }));
    let quit = quits_within(&mut app, Duration::from_secs(20));
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
    let text = log(&data);
    assert!(quit, "MDM didn't quit after /exit:\n{text}");
    assert!(
        !text.contains("window opened: duplicate"),
        "a Duplicate download link opened:\n{text}"
    );
    let mut got = listed(&data);
    got.sort();
    assert_eq!(got, urls, "each link once");
}

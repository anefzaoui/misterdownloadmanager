//! Torrent settings' DHT switch, at the first frame: a magnet link Windows starts MDM for is
//! handled before the clock's first pass gave the torrent engine its settings, and its session
//! started with the engine's defaults (the DHT, local discovery, UPnP and incoming connections
//! on) whatever Torrent settings said. The real app, off the screens (MDM_OFFSCREEN), with a data
//! folder of its own and the DHT off in its saved settings, started for a magnet link: its
//! session must say the DHT is off.
//!
//! Every socket stays on this computer: the session is held to a test adapter on 127.0.0.2
//! (MDM_TEST_ADAPTER, debug builds), so even the defaults never reach the network.
//!
//! It opens windows, which needs OpenGL or Direct3D, so it runs on request:
//! `cargo test -p mdm-app --test dht_off_at_launch -- --ignored`.
//!
//! Windows only: the test adapter is bound by its address there (Linux binds a device by name,
//! which a made-up adapter doesn't have).
#![cfg(windows)]

use std::path::Path;
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

/// Wait until the log has one of `lines`, failing if the app quits first or `wait` passes.
fn wait_for_any(app: &mut Child, data: &Path, lines: &[&str], wait: Duration) -> String {
    let until = Instant::now() + wait;
    loop {
        let text = log(data);
        assert!(!text.contains("panic"), "the app panicked:\n{text}");
        if lines.iter().any(|l| text.contains(l)) {
            return text;
        }
        if let Some(status) = app.try_wait().unwrap() {
            panic!("the app closed ({status}) before any of {lines:?}:\n{text}");
        }
        assert!(
            Instant::now() < until,
            "none of {lines:?} after {wait:?}:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "opens the app's windows (off-screen): needs OpenGL or Direct3D; run with --ignored"]
fn a_magnet_mdm_was_started_for_keeps_the_dht_off() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let settings = serde_json::json!({
        "update_check": false, "helpers_asked": true, "language_asked": true, "language": "en",
        "extension_never_ask": true,
        "torrent_dht": false, "torrent_lsd": false, "torrent_upnp": false,
        "torrent_incoming": false, "net_adapter": "Lab adapter",
    });
    std::fs::write(
        data.join("downloads.json"),
        serde_json::json!({ "next_id": 1, "items": [], "version": 2, "settings": settings })
            .to_string(),
    )
    .unwrap();
    let adapter = "Lab adapter=127.0.0.2";
    // Started for the magnet, as Windows starts MDM when a link is opened and MDM isn't running.
    let magnet = "magnet:?xt=urn:btih:5b4f0c3a9e1d27f86c0b3a4e9d1f2a7c6e8b0d14&dn=MDM%20test";
    let mut app = Command::new(APP)
        .arg(magnet)
        .env("MDM_DATA_DIR", data)
        .env("MDM_OFFSCREEN", "1")
        .env("MDM_TEST_ADAPTER", adapter)
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let text = wait_for_any(
            &mut app,
            data,
            &["torrents: DHT off", "torrents: DHT on UDP"],
            Duration::from_secs(60),
        );
        assert!(
            text.contains("torrents: DHT off") && !text.contains("torrents: DHT on UDP"),
            "the session took the DHT the settings turned off:\n{text}"
        );
        assert!(
            text.contains("torrents: not listening for peers"),
            "incoming connections were off too:\n{text}"
        );
    }));
    let _ = Command::new(APP)
        .arg("/exit")
        .env("MDM_DATA_DIR", data)
        .env("MDM_OFFSCREEN", "1")
        .env("MDM_TEST_ADAPTER", adapter)
        .status();
    let until = Instant::now() + Duration::from_secs(15);
    while app.try_wait().unwrap().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = app.kill();
    let _ = app.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

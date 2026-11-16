//! The Android core (src/android.rs, src/android_ui.rs) end to end on this system: started with a
//! host that records its snapshots, as the Android app's is, it adds a download from the lab
//! server, reports it running and complete, pauses and resumes another, and answers the
//! interface's requests. Its own process: the data folder is the process's.
//!
//! `cargo test -p mdm-app --features android-core --test android_core`
#![cfg(feature = "android-core")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdm_app::android::{Config, Core, Host};
use mdm_labserver::{Config as LabConfig, Faults, LabServer};
use serde_json::{Value, json};

#[derive(Default)]
struct Recorder {
    snapshots: Mutex<Vec<String>>,
    secrets: Mutex<Vec<(String, String, String)>>,
}

impl Host for Recorder {
    fn changed(&self, snapshot: &str) {
        self.snapshots.lock().unwrap().push(snapshot.to_owned());
    }
    fn keychain(
        &self,
        op: &str,
        service: &str,
        account: &str,
        secret: Option<&str>,
    ) -> Result<Option<String>, String> {
        let mut s = self.secrets.lock().unwrap();
        Ok(match op {
            "write" => {
                s.retain(|(sv, a, _)| !(sv == service && a == account));
                s.push((
                    service.into(),
                    account.into(),
                    secret.unwrap_or_default().into(),
                ));
                None
            }
            "read" => s
                .iter()
                .find(|(sv, a, _)| sv == service && a == account)
                .map(|e| e.2.clone()),
            _ => None,
        })
    }
    fn wake_at(&self, _at: Option<i64>) -> Result<(), String> {
        Ok(())
    }
    fn keep_awake(&self, _reason: Option<&str>) -> bool {
        true
    }
}

impl Recorder {
    fn last(&self) -> Value {
        let s = self.snapshots.lock().unwrap();
        serde_json::from_str(s.last().expect("a snapshot")).unwrap()
    }
}

fn d_services(core: &Core) -> Vec<Value> {
    ask(core, json!({"op": "debrid"}))["services"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn ask(core: &Core, req: Value) -> Value {
    let answer = core.request(&req.to_string());
    serde_json::from_str(&answer).unwrap_or_else(|e| panic!("not JSON ({e}): {answer}"))
}

/// Wait until the latest snapshot's download `id` satisfies `ok`.
fn wait_for(rec: &Recorder, id: u64, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        if let Some(d) = rec.snapshots.lock().unwrap().last().and_then(|s| {
            let v: Value = serde_json::from_str(s).ok()?;
            v["downloads"]
                .as_array()?
                .iter()
                .find(|d| d["id"] == id)
                .cloned()
        }) && ok(&d)
        {
            return d;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timed out waiting for {what}: {}",
            rec.snapshots
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_core_downloads_reports_and_answers() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = rt
        .block_on(LabServer::start(LabConfig {
            faults: Faults::parse("rate=512K").unwrap(),
            ..LabConfig::default()
        }))
        .unwrap();
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let core = Core::start(
        Config {
            data_dir: data.path().join("mdm"),
            downloads_dir: downloads.path().to_owned(),
            cache_dir: cache.path().to_owned(),
            native_lib_dir: cache.path().to_owned(),
            language: Some("en-US".into()),
            user_agent: Some("Mozilla/5.0 (Linux; Android 15) test".into()),
        },
        rec.clone(),
    )
    .expect("the core starts");

    // The first snapshot: an empty list, the categories and the main queue.
    let first = ask(&core, json!({"op": "snapshot"}));
    assert_eq!(first["downloads"], json!([]));
    assert!(first["categories"].as_array().unwrap().len() >= 5);
    assert!(!first["queues"].as_array().unwrap().is_empty());
    assert_eq!(first["language"], "en");

    // Classifying links.
    assert_eq!(
        ask(&core, json!({"op": "classify", "url": lab.url("/gen/1M")}))["kind"],
        "file"
    );
    assert_eq!(
        ask(
            &core,
            json!({"op": "classify", "url": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"})
        )["kind"],
        "torrent"
    );
    assert_eq!(
        ask(
            &core,
            json!({"op": "classify", "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"})
        )["kind"],
        "video"
    );
    // A DRM streaming service is named, with the reason, instead of being downloaded.
    let spotify = ask(
        &core,
        json!({"op": "classify", "url": "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"}),
    );
    assert_eq!(spotify["kind"], "protected", "{spotify}");
    assert!(
        spotify["message"].as_str().unwrap().contains("Spotify"),
        "{spotify}"
    );

    // A download added and started: it runs, then completes, and the file is there.
    let added = ask(
        &core,
        json!({"op": "add", "urls": [lab.url("/gen/3M/phone.bin?seed=7")]}),
    );
    let id = added["ids"][0]
        .as_u64()
        .unwrap_or_else(|| panic!("{added}"));
    let done = wait_for(&rec, id, "complete", |d| d["status"] == "Complete");
    assert_eq!(done["size"], 3 << 20);
    let path = std::path::PathBuf::from(done["path"].as_str().unwrap());
    assert!(path.starts_with(downloads.path()), "{path:?}");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 3 << 20);

    // Details: the download, its log, and where its server is in words (the desktop's): the
    // test server is on this computer, so no country; asked again while the location table
    // loads.
    let start = Instant::now();
    let details = loop {
        let d = ask(&core, json!({"op": "details", "id": id}));
        if !d["server"].is_null() || start.elapsed() > Duration::from_secs(20) {
            break d;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(details["download"]["id"], id);
    assert!(!details["log"].as_array().unwrap().is_empty());
    let server = &details["server"];
    assert_eq!(server["place"], "this computer", "{details}");
    assert!(server["country"].is_null(), "{server}");
    assert!(
        server["about"].as_str().unwrap().contains("127.0.0.1"),
        "{server}"
    );

    // Another one queued (later), started, paused, resumed, completed.
    let added = ask(
        &core,
        json!({"op": "add", "urls": [lab.url("/gen/12M/second.bin?seed=9")], "later": true}),
    );
    let id2 = added["ids"][0].as_u64().unwrap();
    wait_for(&rec, id2, "queued", |d| d["status"] == "Queued");
    ask(&core, json!({"op": "act", "action": "start", "ids": [id2]}));
    wait_for(&rec, id2, "running", |d| {
        d["running"] == true && d["downloaded"].as_u64() > Some(0)
    });
    ask(&core, json!({"op": "act", "action": "pause", "ids": [id2]}));
    let paused = wait_for(&rec, id2, "paused", |d| d["running"] == false);
    assert_eq!(paused["status"], "Stopped");
    ask(&core, json!({"op": "act", "action": "start", "ids": [id2]}));
    wait_for(&rec, id2, "complete", |d| d["status"] == "Complete");

    // Settings: read, changed, read back; an unknown one refused.
    let s = ask(&core, json!({"op": "settings"}));
    assert!(s["max_connections"].as_u64().unwrap() >= 1);
    let r = ask(
        &core,
        json!({"op": "set_settings", "patch": {"max_connections": 3}}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(ask(&core, json!({"op": "settings"}))["max_connections"], 3);
    let r = ask(
        &core,
        json!({"op": "set_settings", "patch": {"no_such_setting": 1}}),
    );
    assert_eq!(r["code"], "unknown-setting");

    // Translations and languages.
    let langs = ask(&core, json!({"op": "languages"}));
    assert!(langs["languages"].as_array().unwrap().len() > 50);
    let r = ask(&core, json!({"op": "set_language", "code": "ar"}));
    assert_eq!(r["rtl"], true);
    ask(&core, json!({"op": "set_language", "code": null}));

    // Removed from the list, its file kept.
    ask(&core, json!({"op": "act", "action": "remove", "ids": [id]}));
    let start = Instant::now();
    while rec.last()["downloads"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"] == id)
    {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(path.exists());

    // The Scheduler: a queue's start time and downloads at once; a new queue.
    let main = ask(&core, json!({"op": "queues"}))["queues"][0]["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = ask(
        &core,
        json!({"op": "set_queue", "queue": main, "patch": {"files_at_once": 2, "schedule": {"start_at": [2, 30, 0]}}}),
    );
    assert_eq!(r["ok"], true, "{r}");
    let q = &ask(&core, json!({"op": "queues"}))["queues"][0];
    assert_eq!(q["files_at_once"], 2);
    assert_eq!(q["schedule"]["start_at"], json!([2, 30, 0]));
    assert_eq!(
        q["schedule"]["wake"], true,
        "a start time is an alarm on Android"
    );
    ask(
        &core,
        json!({"op": "set_queue", "queue": "Night", "patch": {}}),
    );
    assert!(
        ask(&core, json!({"op": "queues"}))["queues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|q| q["name"] == "Night")
    );

    // Another downloads folder (an SD card's).
    let card = downloads.path().join("card");
    let r = ask(
        &core,
        json!({"op": "downloads_dir", "dir": card.to_string_lossy()}),
    );
    assert_eq!(r["dir"], card.to_string_lossy().as_ref());
    assert!(card.is_dir());

    // A torrent that isn't one is refused, and the core goes on.
    let r = ask(
        &core,
        json!({"op": "torrent_lookup", "data": "bm90IGEgdG9ycmVudA=="}),
    );
    let token = r["token"].as_str().unwrap().to_owned();
    let start = Instant::now();
    loop {
        let p = ask(&core, json!({"op": "torrent_poll", "token": token}));
        if p["state"] != "pending" {
            assert_eq!(p["state"], "failed", "{p}");
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(100));
    }

    // yt-dlp isn't installed here: the helpers say so.
    let h = ask(&core, json!({"op": "helpers"}));
    assert!(h["installing"].is_null());
    assert!(
        h["ytdlp_error"].is_null() && h["gallery_error"].is_null(),
        "{h}"
    );

    // A debrid account by its token: listed without its secret.
    let services = ask(&core, json!({"op": "debrid"}));
    let key = services["services"][0]["key"].as_str().unwrap().to_owned();
    let r = ask(
        &core,
        json!({"op": "debrid_add", "code": key, "data": "test-token-123"}),
    );
    assert!(r["id"].as_u64().is_some(), "{r}");
    let d = ask(&core, json!({"op": "debrid"}));
    assert_eq!(d["accounts"].as_array().unwrap().len(), 1);
    assert!(
        !d.to_string().contains("test-token-123"),
        "the token isn't shown: {d}"
    );
    // The keychain's worker writes it (Android's keystore, through the host); until it has,
    // the private data folder keeps it, as on the desktop (secrets.rs).
    let start = Instant::now();
    while !rec
        .secrets
        .lock()
        .unwrap()
        .iter()
        .any(|(_, _, s)| s.contains("test-token-123"))
    {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the token reached the keychain"
        );
        ask(&core, json!({"op": "snapshot"}));
        std::thread::sleep(Duration::from_millis(200));
    }
    // A save once it's confirmed writes only the reference.
    ask(&core, json!({"op": "save"}));
    std::thread::sleep(Duration::from_millis(1500));
    ask(&core, json!({"op": "save"}));

    // Sign-in by code: offered for the services that have it, refused for the others; a
    // gallery needs gallery-dl first.
    let services = d_services(&core);
    let by_code: Vec<&Value> = services.iter().filter(|s| s["by_code"] == true).collect();
    let by_token: Vec<&Value> = services.iter().filter(|s| s["by_code"] == false).collect();
    assert!(!by_code.is_empty() && !by_token.is_empty(), "{services:?}");
    let r = ask(
        &core,
        json!({"op": "debrid_signin", "code": by_token[0]["key"]}),
    );
    assert_eq!(r["code"], "bad-request", "{r}");
    assert_eq!(
        ask(&core, json!({"op": "debrid_signin_state"}))["state"],
        "idle"
    );
    let r = ask(
        &core,
        json!({"op": "gallery_add", "url": "https://commons.wikimedia.org/wiki/Category:Cats"}),
    );
    assert_eq!(r["code"], "no-gallery-dl", "{r}");
    assert!(
        ask(&core, json!({"op": "helpers"}))
            .get("gallery")
            .is_some()
    );

    // An unknown op is an error, not a crash.
    assert_eq!(ask(&core, json!({"op": "nope"}))["code"], "unknown-op");

    // Stopped: the list is saved, and a new core finds it.
    core.stop();
    let list = std::fs::read_to_string(data.path().join("mdm/downloads.json")).unwrap();
    assert!(list.contains("second.bin"), "{list}");
    // The token went to the keychain (Android's keystore through the host), not into the list.
    assert!(
        !list.contains("test-token-123"),
        "the token isn't in the saved list"
    );
    assert!(
        rec.secrets
            .lock()
            .unwrap()
            .iter()
            .any(|(_, _, secret)| secret.contains("test-token-123")),
        "the token is in the keychain"
    );
}

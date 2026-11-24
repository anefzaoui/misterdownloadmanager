//! #333: Move/Rename with several downloads selected. The real app, off the screens, with a data
//! folder and a download folder of its own: every row selected (`/show context:all`), then the
//! command (`/show move-rename`), the folder dialog answered by MDM_TEST_MOVE_TO. What can go goes
//! there under its own name (two not started under one name get a free one each), a part-way
//! download with its part file and resume state (#1739).
//!
//! It opens windows, which needs OpenGL or Direct3D, so it runs on request:
//! `cargo test -p mdm-app --test move_many -- --ignored`.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const APP: &str = env!("CARGO_BIN_EXE_misterdownloadmanager");

fn item(id: u64, name: &str, status: &str, downloaded: u64, dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "id": id, "url": format!("https://files.example/{name}"), "file_name": name,
        "dest": dir.join(name), "size": 1000, "downloaded": downloaded, "status": status,
        "category": "Compressed", "description": "", "last_try": null, "error": null,
        "resumable": null,
    })
}

fn log(data: &Path) -> String {
    std::fs::read_to_string(data.join("logs").join("mdm.log")).unwrap_or_default()
}

fn second_launch(data: &Path, downloads: &Path, what: &str) {
    let status = Command::new(APP)
        .args(["/show", what])
        .env("MDM_DATA_DIR", data)
        .env("MDM_OFFSCREEN", "1")
        .env("MDM_DOWNLOADS", downloads)
        .status()
        .unwrap();
    assert!(status.success(), "/show {what}: {status}");
}

fn wait_for(what: &str, data: &Path, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(45);
    while !done() {
        assert!(Instant::now() < until, "no {what}:\n{}", log(data));
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A download's saved place: its path relative to `root`, with forward slashes.
fn saved_place(j: &serde_json::Value, id: u64, root: &Path) -> String {
    let it = j["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap();
    Path::new(it["dest"].as_str().unwrap())
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
#[ignore = "opens windows; run on request"]
fn a_selection_moves_to_one_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (data, dl, to) = (root.join("data"), root.join("dl"), root.join("to"));
    for d in [&data, &dl, &to] {
        std::fs::create_dir_all(d).unwrap();
    }
    for name in ["done-1.bin", "done-2.bin"] {
        std::fs::write(dl.join(name), [b'x'; 1000]).unwrap();
    }
    // Stopped at 300 of 1000 bytes: its part file and resume state.
    std::fs::write(dl.join("partial.bin.mdm.part"), [b'p'; 1000]).unwrap();
    let state = serde_json::json!({
        "version": 1, "url": "https://files.example/partial.bin",
        "final_url": "https://files.example/partial.bin", "dest": dl.join("partial.bin"),
        "size": 1000, "segments": [{ "start": 0, "cursor": 300, "end": 1000 }],
    });
    std::fs::write(dl.join("partial.bin.mdm.json"), state.to_string()).unwrap();
    let items = [
        item(1, "waiting.zip", "Queued", 0, &dl),
        item(2, "same.zip", "Queued", 0, &dl),
        item(3, "same.zip", "Queued", 0, &dl),
        item(4, "done-1.bin", "Complete", 1000, &dl),
        item(5, "done-2.bin", "Complete", 1000, &dl),
        item(6, "partial.bin", "Stopped", 300, &dl),
    ];
    let settings = serde_json::json!({
        "language": "en", "update_check": false, "helpers_asked": true, "language_asked": true,
        "extension_never_ask": true, "clipboard_autostart": false,
    });
    std::fs::write(
        data.join("downloads.json"),
        serde_json::json!({ "next_id": 7, "items": items, "version": 2, "settings": settings })
            .to_string(),
    )
    .unwrap();
    let mut app = Command::new(APP)
        .env("MDM_DATA_DIR", &data)
        .env("MDM_OFFSCREEN", "1")
        .env("MDM_DOWNLOADS", &dl)
        .env("MDM_TEST_MOVE_TO", &to)
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for("main window", &data, || {
            log(&data).contains("window opened: main")
        });
        second_launch(&data, &dl, "context:all");
        wait_for("Select all", &data, || {
            log(&data).contains("command SelectAll")
        });
        second_launch(&data, &dl, "move-rename");
        wait_for("finished files in the new folder", &data, || {
            to.join("done-1.bin").exists()
                && to.join("done-2.bin").exists()
                && to.join("partial.bin.mdm.json").exists()
        });
        // Saved within a second or so of the change: every row in the new folder, none still
        // moving (a part-way download's move saves the list as it begins, before the finished
        // files' moves are done).
        wait_for("the new places saved", &data, || {
            std::fs::read_to_string(data.join("downloads.json")).is_ok_and(|t| {
                serde_json::from_str::<serde_json::Value>(&t).is_ok_and(|j| {
                    !t.contains("move_from")
                        && (1..=6).all(|id| saved_place(&j, id, root).starts_with("to/"))
                })
            })
        });
    }));
    let _ = app.kill();
    let _ = app.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("downloads.json")).unwrap())
            .unwrap();
    assert_eq!(saved_place(&j, 1, root), "to/waiting.zip");
    assert_eq!(saved_place(&j, 2, root), "to/same.zip");
    assert_eq!(saved_place(&j, 3, root), "to/same (1).zip");
    assert_eq!(saved_place(&j, 4, root), "to/done-1.bin");
    assert_eq!(saved_place(&j, 5, root), "to/done-2.bin");
    assert_eq!(
        saved_place(&j, 6, root),
        "to/partial.bin",
        "part-way: it goes too"
    );
    assert_eq!(
        std::fs::read(to.join("partial.bin.mdm.part")).unwrap(),
        [b'p'; 1000]
    );
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(to.join("partial.bin.mdm.json")).unwrap()).unwrap();
    assert_eq!(
        state["dest"].as_str().map(Path::new),
        Some(to.join("partial.bin").as_path())
    );
    assert!(!dl.join("partial.bin.mdm.part").exists() && !dl.join("partial.bin.mdm.json").exists());
    assert!(!dl.join("done-1.bin").exists());
}

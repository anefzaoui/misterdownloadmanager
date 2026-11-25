//! Stop and keep (#1780): an answer that says no length and takes no ranges (an internet radio,
//! a camera feed) may never end. `Download::keep` ends it where it is and keeps what arrived as
//! the finished file; every other download is stopped as Pause stops it, to resume as before.

use std::time::{Duration, Instant};

use mdm_engine::download::State;
use mdm_engine::{Destination, Download, Error, Options, Request, Snapshot};
use mdm_labserver::{Config, Faults, LabServer, content};

/// How long a test waits for a download to get somewhere: far more than it needs on a slow
/// runner (a few hundred KB at the paced rate), never a fixed sleep.
const PATIENCE: Duration = Duration::from_secs(60);

async fn server(faults: &str) -> LabServer {
    LabServer::start(Config {
        faults: Faults::parse(faults).unwrap(),
        ..Config::default()
    })
    .await
    .unwrap()
}

fn start(s: &LabServer, path: &str, dir: &std::path::Path, opts: Options) -> Download {
    let req = Request {
        url: s.url(path).parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.to_owned()),
    };
    Download::start(req, opts)
}

/// Polls `dl` until `ready` says so about its snapshot, which it returns.
async fn until(dl: &Download, ready: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let s = dl.snapshot();
        if ready(&s) {
            return s;
        }
        assert!(
            !matches!(s.state, State::Done | State::Failed | State::Cancelled),
            "ended before it got there: {:?} {:?}",
            s.state,
            s.error
        );
        assert!(Instant::now() < deadline, "never got there: {s:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The files in `dir`, by name.
fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn stop_and_keep_ends_an_endless_stream_as_a_finished_file() {
    let s = server("rate=256K").await;
    let dir = tempfile::tempdir().unwrap();
    let dl = start(
        &s,
        "/gen/1M/radio.mp3?endless=1&seed=31",
        dir.path(),
        Options::default(),
    );
    let seen = until(&dl, |s| s.endless() && s.downloaded >= 64 << 10).await;
    assert_eq!(seen.size, None);
    assert_eq!(seen.resumable, Some(false));
    assert!(dl.keep(), "an endless stream is kept");
    let out = tokio::time::timeout(PATIENCE, dl.wait())
        .await
        .expect("it ends once kept")
        .expect("kept as a finished file");
    // The file under its own name, every byte that arrived as the server sent it, no part file
    // or resume state left.
    assert_eq!(out.path, dir.path().join("radio.mp3"));
    let data = std::fs::read(&out.path).unwrap();
    assert_eq!(data.len() as u64, out.size);
    assert!(out.size >= 64 << 10, "kept {} bytes", out.size);
    let mut want = vec![0; data.len()];
    content::fill(31, 0, &mut want);
    assert!(data == want, "content mismatch");
    assert_eq!(names(dir.path()), ["radio.mp3"]);
    // Its connection is closed: the server stops sending.
    tokio::time::timeout(PATIENCE, s.settled())
        .await
        .expect("the stream's request ends");
    let log = s.log();
    assert_eq!(log.len(), 1, "one request: {log:?}");
    assert_eq!(log[0].outcome, "aborted");
    assert!(log[0].bytes_sent >= out.size);
}

/// A checksum can't be for the part of a stream that arrived: a kept stream isn't checked
/// against it, and is kept all the same.
#[tokio::test]
async fn a_kept_stream_is_not_checked_against_a_checksum() {
    let s = server("rate=256K").await;
    let dir = tempfile::tempdir().unwrap();
    let opts = Options {
        checksum: mdm_engine::verify::Hash::parse(&format!("sha256:{}", "ab".repeat(32))),
        ..Options::default()
    };
    assert!(opts.checksum.is_some());
    let dl = start(&s, "/gen/1M/cam.mjpeg?endless=1&seed=32", dir.path(), opts);
    until(&dl, |s| s.endless() && s.downloaded >= 32 << 10).await;
    assert!(dl.keep());
    let out = tokio::time::timeout(PATIENCE, dl.wait())
        .await
        .expect("it ends once kept")
        .expect("kept, unchecked");
    assert_eq!(std::fs::metadata(&out.path).unwrap().len(), out.size);
}

/// Cancel (the progress window's Cancel, MDM's own stops) still stops an endless stream without
/// keeping it: no finished file.
#[tokio::test]
async fn cancel_still_stops_an_endless_stream_without_keeping_it() {
    let s = server("rate=256K").await;
    let dir = tempfile::tempdir().unwrap();
    let dl = start(
        &s,
        "/gen/1M/radio.mp3?endless=1&seed=33",
        dir.path(),
        Options::default(),
    );
    until(&dl, |s| s.endless() && s.downloaded >= 32 << 10).await;
    dl.cancel();
    let r = tokio::time::timeout(PATIENCE, dl.wait())
        .await
        .expect("it ends once cancelled");
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert!(!dir.path().join("radio.mp3").exists());
}

/// A download with a length is never endless: keep stops it as Pause does, and it resumes from
/// its state as before. So is one with a length from a server without ranges (it starts over).
#[tokio::test]
async fn keep_stops_a_download_with_a_length_as_pause_does() {
    let s = server("rate=512K").await;
    for (path, resumable) in [
        ("/gen/8M/a.bin?seed=34", true),
        ("/gen/8M/b.bin?seed=35&no_range=1", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let dl = start(&s, path, dir.path(), Options::default());
        let seen = until(&dl, |s| {
            s.state == State::Downloading && s.downloaded >= 64 << 10 && s.resumable.is_some()
        })
        .await;
        assert_eq!(seen.size, Some(8 << 20), "{path}");
        assert!(!seen.endless(), "{path}");
        assert!(!dl.keep(), "{path}: not kept");
        let r = tokio::time::timeout(PATIENCE, dl.wait())
            .await
            .expect("it stops");
        assert!(matches!(r, Err(Error::Cancelled)), "{path}: {r:?}");
        let name = path.trim_start_matches("/gen/8M/");
        let name = &name[..name.find('?').unwrap()];
        assert!(!dir.path().join(name).exists(), "{path}: no finished file");
        // A resumable one leaves its resume state to go on from.
        if resumable {
            let state = mdm_engine::state::state_path(&dir.path().join(name));
            assert!(state.exists(), "{path}: its resume state");
        }
    }
}

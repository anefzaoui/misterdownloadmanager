//! End-to-end: the engine downloads from a labserver (with faults) and the file is verified
//! byte for byte against the generator.

use mdm_engine::{Destination, Download, Options, Request};
use mdm_labserver::{Config, Faults, LabServer, LogEntry, content};

async fn server(faults: &str) -> LabServer {
    LabServer::start(Config {
        faults: Faults::parse(faults).unwrap(),
        ..Config::default()
    })
    .await
    .unwrap()
}

fn fast_opts() -> Options {
    Options {
        retry_delay: std::time::Duration::from_millis(50),
        ..Options::default()
    }
}

/// Download `path` from `s` into a temp dir; returns the log and the verified size.
async fn fetch(s: &LabServer, path: &str, seed: u64, opts: Options) -> (Vec<LogEntry>, u64) {
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url(path).parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let out = Download::start(req, opts)
        .wait()
        .await
        .expect("download succeeds");
    let data = std::fs::read(&out.path).unwrap();
    let mut want = vec![0; data.len()];
    content::fill(seed, 0, &mut want);
    assert!(data == want, "content mismatch");
    assert_eq!(data.len() as u64, out.size);
    // The server logs a request when its response body is dropped, which can trail the client.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    (s.log(), out.size)
}

fn max_concurrent(log: &[LogEntry]) -> usize {
    let mut edges: Vec<(u64, i32)> = log
        .iter()
        .flat_map(|e| [(e.t_start_ms, 1), (e.t_end_ms, -1)])
        .collect();
    edges.sort();
    edges
        .iter()
        .scan(0, |n, (_, d)| {
            *n += d;
            Some(*n)
        })
        .max()
        .unwrap_or(0) as usize
}

#[tokio::test]
async fn segmented_download_uses_many_connections() {
    let s = server("rate=4M").await;
    let (log, size) = fetch(&s, "/gen/8M/a.bin?seed=1", 1, fast_opts()).await;
    assert_eq!(size, 8 << 20);
    // The probe plus one ranged request per other connection (more only if a split happens,
    // which depends on timing when all connections run at the same speed).
    let ranged = log.iter().filter(|e| e.status == 206).count();
    let others = Options::default().max_connections as usize - 1;
    assert!(
        ranged >= others,
        "expected ≥ {others} ranged requests, got {ranged}"
    );
    assert!(max_concurrent(&log) >= 4, "connections ran in parallel");
}

#[tokio::test]
async fn connections_skip_a_dead_address() {
    let s = server("").await;
    let port = s.url("/").parse::<url::Url>().unwrap().port().unwrap();
    // The host's first address never answers (TEST-NET-1); the second is the lab server.
    let opts = Options {
        hosts: vec![(
            "mirror.test".into(),
            vec!["192.0.2.1".parse().unwrap(), "127.0.0.1".parse().unwrap()],
        )],
        ..Options::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: format!("http://mirror.test:{port}/gen/2M?seed=19")
            .parse()
            .unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let started = std::time::Instant::now();
    let out = Download::start(req, opts).wait().await.expect("completes");
    // Without the race every connection would wait 15 s (half the 30 s connect timeout).
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "took {:?}",
        started.elapsed()
    );
    let mut want = vec![0; 2 << 20];
    content::fill(19, 0, &mut want);
    assert!(
        std::fs::read(&out.path).unwrap() == want,
        "content mismatch"
    );
}

#[tokio::test]
async fn a_slow_download_does_not_rewrite_its_state_every_tick() {
    let s = server("rate=4K").await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/1M/slow.bin?seed=22").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let opts = Options {
        save_interval: std::time::Duration::from_millis(50),
        ..fast_opts()
    };
    let dl = Download::start(req, opts);
    let state = loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Some(f) = dl.snapshot().file {
            let p = mdm_engine::state::state_path(&f);
            if p.exists() {
                break p;
            }
        }
    };
    let first = std::fs::read(&state).unwrap();
    // 20 save ticks while bytes trickle in: no flush is due, nothing is rewritten.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    assert_eq!(
        std::fs::read(&state).unwrap(),
        first,
        "rewritten between flushes"
    );
    // Pausing always flushes and records the progress.
    dl.cancel();
    let _ = dl.wait().await;
    let last = mdm_engine::state::ResumeState::load(&state).unwrap();
    assert!(last.downloaded() > 0);
    assert_ne!(std::fs::read(&state).unwrap(), first);
}

#[tokio::test]
async fn a_global_cap_is_shared_by_all_downloads() {
    let s = server("").await;
    let global = std::sync::Arc::new(mdm_engine::limiter::Limiter::new(Some(1 << 20)));
    let opts = Options {
        global: Some(global),
        ..fast_opts()
    };
    let started = std::time::Instant::now();
    let (a, b) = tokio::join!(
        fetch(&s, "/gen/1M/a.bin?seed=25", 25, opts.clone()),
        fetch(&s, "/gen/1M/b.bin?seed=26", 26, opts.clone()),
    );
    assert_eq!((a.1, b.1), (1 << 20, 1 << 20));
    // 2 MiB through one 1 MiB/s cap (fetch adds 0.3 s of its own per download).
    let took = started.elapsed();
    assert!(
        took >= std::time::Duration::from_millis(1700),
        "took {took:?}"
    );
}

/// The SHA-256 of generated content `seed` of `size` bytes.
fn sha256_of(seed: u64, size: usize) -> mdm_engine::verify::Hash {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("want");
    let mut want = vec![0; size];
    content::fill(seed, 0, &mut want);
    std::fs::write(&p, &want).unwrap();
    let algo = mdm_engine::verify::Algo::Sha256;
    mdm_engine::verify::Hash {
        algo,
        value: mdm_engine::verify::digest(&p, algo).unwrap(),
    }
}

#[tokio::test]
async fn a_known_checksum_is_verified() {
    let s = server("").await;
    let opts = Options {
        checksum: Some(sha256_of(27, 1 << 20)),
        ..fast_opts()
    };
    let (_, size) = fetch(&s, "/gen/1M/ok.bin?seed=27", 27, opts).await;
    assert_eq!(size, 1 << 20);
}

#[tokio::test]
async fn a_wrong_checksum_fails_and_keeps_the_data() {
    let s = server("").await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/1M/bad.bin?seed=28").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let opts = Options {
        checksum: Some(sha256_of(99, 1 << 20)),
        ..fast_opts()
    };
    let err = Download::start(req, opts).wait().await.unwrap_err();
    assert!(
        matches!(err, mdm_engine::Error::ChecksumMismatch("SHA-256")),
        "{err}"
    );
    assert!(
        dir.path().join("bad.bin.mdm.part").exists(),
        "the data is kept"
    );
    assert!(!dir.path().join("bad.bin").exists());
}

#[tokio::test]
async fn the_event_log_tells_what_happened() {
    let s = server("rate=8M&drop_after=300K&drop_every=3").await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/4M/log.bin?seed=29").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let dl = Download::start(req, fast_opts());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let snap = loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let snap = dl.snapshot();
        if snap.state == mdm_engine::download::State::Done {
            break snap;
        }
        assert!(std::time::Instant::now() < deadline, "never finished");
    };
    let texts: Vec<&str> = snap.events.iter().map(|e| e.text.as_str()).collect();
    assert!(texts[0].starts_with("Connected to 127.0.0.1"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("retrying")), "{texts:?}");
    assert!(texts.last().unwrap().starts_with("Complete"), "{texts:?}");
    assert!(snap.events.windows(2).all(|w| w[0].seq < w[1].seq));
}

#[tokio::test]
async fn a_host_connection_limit_is_learned_and_reused() {
    let s = server("rate=2M&max_conns=2").await;
    let limits = std::sync::Arc::new(mdm_engine::hosts::HostLimits::default());
    let opts = Options {
        host_limits: Some(limits.clone()),
        ..fast_opts()
    };
    let (first, _) = fetch(&s, "/gen/4M/a.bin?seed=30", 30, opts.clone()).await;
    assert!(first.iter().any(|e| e.outcome == "fault:max_conns"));
    assert_eq!(limits.cap("127.0.0.1"), Some(2), "{:?}", limits.learned());
    // The next download from the host opens only what it allows: nothing is refused.
    s.clear_log();
    let (second, size) = fetch(&s, "/gen/4M/b.bin?seed=31", 31, opts).await;
    assert_eq!(size, 4 << 20);
    assert!(
        !second.iter().any(|e| e.outcome == "fault:max_conns"),
        "{:?}",
        second.iter().map(|e| &e.outcome).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_refusal_below_the_learned_cap_is_retried() {
    // The cap (2) is known, but a slot is still held elsewhere (here another client; in practice
    // a connection of ours that just closed and the server hasn't noticed yet): the second
    // connection is refused while only one works. It must come back once the slot frees, not
    // leave the whole file to one connection (benchmark 2026-10-29).
    let s = server("rate=2M&max_conns=2").await;
    let limits = std::sync::Arc::new(mdm_engine::hosts::HostLimits::default());
    limits.refused("127.0.0.1", 2);
    limits.refused("127.0.0.1", 2);
    let blocker = tokio::spawn(
        reqwest::get(s.url("/gen/512K/blocker.bin?seed=1"))
            .await
            .unwrap()
            .bytes(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let opts = Options {
        host_limits: Some(limits),
        ..fast_opts()
    };
    let (log, size) = fetch(&s, "/gen/4M/c.bin?seed=33", 33, opts).await;
    blocker.await.unwrap().unwrap();
    assert_eq!(size, 4 << 20);
    let ours: Vec<_> = log.iter().filter(|e| e.path.contains("c.bin")).collect();
    assert!(
        ours.iter().any(|e| e.outcome == "fault:max_conns"),
        "the blocker held a slot"
    );
    // Retired, connection 2's half would wait for connection 1 to finish its own: sequential.
    let served: Vec<LogEntry> = ours
        .iter()
        .filter(|e| e.status != 503)
        .map(|e| (*e).clone())
        .collect();
    assert_eq!(
        max_concurrent(&served),
        2,
        "a second connection joined once the slot freed: {:?}",
        ours.iter()
            .map(|e| (e.status, e.t_start_ms, e.t_end_ms, &e.outcome))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn slow_connections_reconnect_instead_of_holding_the_end() {
    // Every third request crawls at 20 KB/s: without shedding, all connections end up on
    // such requests and 8 MiB takes ~12 s.
    let s = server("rate=8M&slow_every=3&slow_rate=20K").await;
    let started = std::time::Instant::now();
    let (log, size) = fetch(&s, "/gen/8M/slow.bin?seed=32", 32, fast_opts()).await;
    assert_eq!(size, 8 << 20);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "took {:?}",
        started.elapsed()
    );
    let slow = log.iter().filter(|e| e.rate == Some(20 * 1024)).count();
    assert!(slow > 0, "the fault was there");
}

#[tokio::test]
async fn a_weak_etag_means_no_if_range() {
    let s = server("rate=4M&etag=weak").await;
    let (log, _) = fetch(&s, "/gen/4M?seed=20", 20, fast_opts()).await;
    assert!(log.iter().filter(|e| e.status == 206).count() > 1);
    assert!(
        log.iter().all(|e| e.if_range.is_none()),
        "neither the weak ETag nor the date behind it"
    );
}

#[tokio::test]
async fn per_node_etags_fall_back_to_the_date() {
    // Every answer carries a new ETag for the same file, as CDN nodes do.
    let s = server("rate=4M&etag=per-request").await;
    let (log, size) = fetch(&s, "/gen/8M?seed=21", 21, fast_opts()).await;
    assert_eq!(size, 8 << 20);
    assert!(
        log.iter().filter(|e| e.status == 206).count() > 1,
        "still segmented, not one connection"
    );
    assert!(
        log.iter()
            .any(|e| e.status == 206 && e.if_range.as_deref().is_some_and(|v| !v.starts_with('"'))),
        "validated with Last-Modified"
    );
}

#[tokio::test]
async fn sockets_are_reused_between_segments() {
    // Every other connection is slow: the fast ones finish early and take over the rest of the
    // slow ones' segments, on the keep-alive sockets they already have.
    let s = server("rate=4M&slow_every=2&slow_rate=512K").await;
    let opts = Options {
        max_connections: 4,
        ..fast_opts()
    };
    let (log, _) = fetch(&s, "/gen/8M?seed=18", 18, opts).await;
    let sockets: std::collections::HashSet<_> = log.iter().map(|e| e.peer).collect();
    assert!(
        log.iter().any(|e| e.conn_req > 1) && sockets.len() < log.len(),
        "{} sockets for {} requests",
        sockets.len(),
        log.len()
    );
}

#[tokio::test]
async fn survives_drops_and_503s() {
    let s = server("rate=8M&drop_after=300K&drop_every=3&fail_every=5").await;
    let (log, _) = fetch(&s, "/gen/6M?seed=2", 2, fast_opts()).await;
    assert!(log.iter().any(|e| e.outcome == "dropped"));
}

/// A server busy when the download starts (its first answers 503): the first request is retried
/// like any connection instead of failing the download (night e2e, 2026-10-30).
#[tokio::test]
async fn a_server_busy_at_the_start_is_retried() {
    let s = server("fail_first=3").await;
    let (log, _) = fetch(&s, "/gen/2M?seed=4", 4, fast_opts()).await;
    assert_eq!(
        log.iter()
            .filter(|e| e.outcome == "fault:fail_first")
            .count(),
        3
    );
}

/// A dead link still fails at once: only failures a later try may not have are retried.
#[tokio::test]
async fn a_dead_link_is_not_retried() {
    let s = server("").await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/not-a-size").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let err = Download::start(req, fast_opts()).wait().await.unwrap_err();
    assert!(err.to_string().contains("404"), "{err}");
    assert_eq!(s.log().len(), 1, "one request, no retries");
}

#[tokio::test]
async fn respects_server_connection_limit() {
    let s = server("rate=4M&max_conns=2").await;
    let (log, _) = fetch(&s, "/gen/4M?seed=3", 3, fast_opts()).await;
    assert!(log.iter().any(|e| e.outcome == "fault:max_conns"));
}

#[tokio::test]
async fn no_range_server_uses_one_connection() {
    // Rate-limited: unlimited, the 200 sent to a range attempt can fit whole into the socket
    // buffers before the engine drops it, and the server logs it complete (macOS runners).
    let s = server("no_range=true&rate=8M").await;
    let (log, size) = fetch(&s, "/gen/3M?seed=4", 4, fast_opts()).await;
    assert_eq!(size, 3 << 20);
    // Like IDM (lab S6): range attempts get 200 and are dropped; the probe finishes alone.
    let complete: Vec<_> = log.iter().filter(|e| e.outcome == "complete").collect();
    let requests: Vec<_> = log
        .iter()
        .map(|e| {
            (
                e.range.clone(),
                e.outcome.clone(),
                e.bytes_sent,
                e.t_start_ms,
                e.t_end_ms,
            )
        })
        .collect();
    assert_eq!(complete.len(), 1, "{requests:#?}");
    assert_eq!(complete[0].range, None, "the probe is a plain GET");
}

#[tokio::test]
async fn probe_is_a_plain_get() {
    let s = server("rate=4M").await;
    let (log, _) = fetch(&s, "/gen/2M?seed=6", 6, fast_opts()).await;
    assert_eq!(log.iter().filter(|e| e.range.is_none()).count(), 1);
    assert!(
        log.iter()
            .all(|e| e.user_agent.as_deref() == Some(mdm_engine::download::DEFAULT_USER_AGENT))
    );
}

#[tokio::test]
async fn tiny_file_is_not_split() {
    let s = server("").await;
    let (log, _) = fetch(&s, "/gen/16K?seed=5", 5, fast_opts()).await;
    assert_eq!(log.iter().filter(|e| e.bytes_sent > 0).count(), 1);
}

/// Start a slow download, cancel it once some data is on disk; returns (dir, state file).
async fn paused(s: &LabServer, path: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url(path).parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let opts = Options {
        save_interval: std::time::Duration::from_millis(100),
        ..fast_opts()
    };
    let dl = Download::start(req, opts);
    // Wait for the real condition (not a fixed time: a busy machine is slower): the probe is
    // done and some data is on disk.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let file = loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let snap = dl.snapshot();
        if let Some(f) = snap.file.filter(|_| snap.downloaded > 0) {
            break f;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the download never started"
        );
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    dl.cancel();
    assert!(matches!(dl.wait().await, Err(mdm_engine::Error::Cancelled)));
    (dir, mdm_engine::state::state_path(&file))
}

#[tokio::test]
async fn pause_and_resume_continues_where_it_stopped() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M/r.bin?seed=7").await;
    let saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    let before = saved.downloaded();
    assert!(
        before > 0 && before < 8 << 20,
        "partial progress saved: {before}"
    );
    // The server logs a request when its response body is dropped, which trails the cancel:
    // let the paused phase's entries land before clearing, so only the resume is counted.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    s.clear_log();

    let out = Download::resume(state.clone(), Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .unwrap();
    let data = std::fs::read(&out.path).unwrap();
    let mut want = vec![0; 8 << 20];
    content::fill(7, 0, &mut want);
    assert!(data == want, "content mismatch after resume");
    assert!(!state.exists(), "state file removed on completion");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let refetched: u64 = s.log().iter().map(|e| e.bytes_sent).sum();
    assert!(
        refetched <= (8 << 20) - before + (1 << 20),
        "resume re-downloaded too much: {refetched} after {before}"
    );
    assert!(
        s.log().iter().all(|e| e.if_range.is_some()),
        "resume validates with If-Range"
    );
}

#[tokio::test]
async fn resume_detects_changed_file() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M?seed=8").await;
    // Pretend the data so far was fetched under another version of the file (another ETag
    // and date; another ETag alone is what CDN nodes do — see per_node_etags_...).
    let mut saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    saved.validators.etag = Some("\"another-version\"".into());
    saved.validators.last_modified = Some("Wed, 21 Oct 2015 07:28:00 GMT".into());
    std::fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    let err = Download::resume(state, Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, mdm_engine::Error::FileChanged), "{err}");
}

/// Pause, then drop the saved validator (a server without ETag/Last-Modified): the resume can
/// only notice a changed file through the overlap check.
async fn paused_without_validator(
    s: &LabServer,
    path: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, state) = paused(s, path).await;
    let mut saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    saved.validators = Default::default();
    std::fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    (dir, state)
}

#[tokio::test]
async fn resume_without_validator_checks_overlap_and_completes() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused_without_validator(&s, "/gen/8M/o.bin?seed=11").await;
    s.clear_log();
    let out = Download::resume(state, Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .unwrap();
    let mut want = vec![0; 8 << 20];
    content::fill(11, 0, &mut want);
    assert!(
        std::fs::read(&out.path).unwrap() == want,
        "content mismatch"
    );
}

#[tokio::test]
async fn resume_overlap_check_detects_changed_data() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused_without_validator(&s, "/gen/8M/c.bin?seed=12").await;
    // Make the data on disk differ from the server's just before every resume point, as if
    // the file had changed on the server since.
    let saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    let part = mdm_engine::naming::part_path(&saved.dest);
    let mut bytes = std::fs::read(&part).unwrap();
    let mut flipped = 0;
    for seg in &saved.segments {
        if seg.cursor > seg.start && seg.cursor < seg.end {
            bytes[seg.cursor as usize - 1] ^= 0xff;
            flipped += 1;
        }
    }
    assert!(flipped > 0, "some segment was partly downloaded");
    std::fs::write(&part, bytes).unwrap();
    let err = Download::resume(state, Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, mdm_engine::Error::FileChanged), "{err}");
}

#[tokio::test]
async fn cookies_do_not_follow_a_cross_host_redirect() {
    let s = server("").await;
    let port = s.addr().port();
    // 127.0.0.1 redirects to localhost: another host, as when a site hands off to a CDN.
    let target = format!("http://localhost:{port}/gen/2M/r.bin?seed=3");
    let path = format!(
        "/redirect?to={}",
        target
            .replace(':', "%3A")
            .replace('/', "%2F")
            .replace('?', "%3F")
            .replace('=', "%3D")
    );
    let dir = tempfile::tempdir().unwrap();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::COOKIE, "sid=secret".parse().unwrap());
    let req = Request {
        url: s.url(&path).parse().unwrap(),
        headers,
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let out = Download::start(req, fast_opts()).wait().await.unwrap();
    let mut want = vec![0; 2 << 20];
    content::fill(3, 0, &mut want);
    assert!(
        std::fs::read(&out.path).unwrap() == want,
        "content mismatch"
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let log = s.log();
    assert!(!log.is_empty());
    assert!(
        log.iter().all(|e| e.cookie.is_none()),
        "the cookie for 127.0.0.1 reached localhost: {:?}",
        log.iter().map(|e| &e.cookie).collect::<Vec<_>>()
    );
}

/// `/redirect?to=<target>` with the target percent-encoded.
fn redirect_path(target: &str) -> String {
    let enc: String = target
        .chars()
        .map(|c| match c {
            ':' => "%3A".to_owned(),
            '/' => "%2F".to_owned(),
            '?' => "%3F".to_owned(),
            '=' => "%3D".to_owned(),
            '&' => "%26".to_owned(),
            c => c.to_string(),
        })
        .collect();
    format!("/redirect?to={enc}")
}

#[tokio::test]
async fn signed_links_that_expire_while_downloading_are_refreshed() {
    // Each redirect hands out a link valid for 400 ms; connections drop every 256 KB, so they
    // keep asking for new ranges after their link expired.
    let s = server("rate=1M&drop_after=256K").await;
    let target = s.url("/gen/4M/signed.bin?seed=23");
    let path = format!("{}&ttl_ms=400", redirect_path(&target));
    let opts = Options {
        max_connections: 4,
        ..fast_opts()
    };
    let (log, size) = fetch(&s, &path, 23, opts).await;
    assert_eq!(size, 4 << 20);
    assert!(
        log.iter().any(|e| e.outcome == "expired"),
        "a link expired during the download"
    );
}

#[tokio::test]
async fn a_link_that_cannot_be_refreshed_reports_link_expired() {
    let s = server("rate=1M").await;
    let target = s.url("/gen/8M/x.bin?seed=24");
    let (_dir, state) = paused(&s, &redirect_path(&target)).await;
    // The stored link has expired and the original address is gone too.
    let mut saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    saved.final_url = format!("{target}&expires=1").parse().unwrap();
    saved.url = s.url("/gone").parse().unwrap();
    std::fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    let err = Download::resume(state.clone(), Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, mdm_engine::Error::LinkExpired), "{err}");
    assert!(state.exists(), "the data is kept for a fresh address");
}

#[tokio::test]
async fn resume_follows_the_original_address_when_the_redirected_one_expired() {
    let s = server("rate=1M").await;
    let target = s.url("/gen/8M/e.bin?seed=14");
    let (_dir, state) = paused(&s, &redirect_path(&target)).await;
    // The signed CDN link has expired: the stored final address now answers 404.
    let mut saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    assert_ne!(saved.url, saved.final_url, "the download was redirected");
    saved.final_url = s.url("/expired-link").parse().unwrap();
    std::fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    let out = Download::resume(state, Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .expect("resumed from the refreshed address");
    let mut want = vec![0; 8 << 20];
    content::fill(14, 0, &mut want);
    assert!(
        std::fs::read(&out.path).unwrap() == want,
        "content mismatch"
    );
}

#[tokio::test]
async fn a_dead_link_fails_fast_instead_of_retrying() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M/d.bin?seed=15").await;
    // The address now answers 404 (and there is no other address to fall back to).
    let mut saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    let gone: url::Url = s.url("/gone").parse().unwrap();
    saved.url = gone.clone();
    saved.final_url = gone;
    std::fs::write(&state, serde_json::to_vec(&saved).unwrap()).unwrap();
    let started = std::time::Instant::now();
    // Default options: retries back off from 1 s, so retrying a 404 would take far longer.
    let err = Download::resume(state, Default::default(), Options::default())
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(
        matches!(err, mdm_engine::Error::Status(s) if s.as_u16() == 404),
        "{err}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "took {:?}",
        started.elapsed()
    );
}

/// Point a paused download's state at its own address plus the fault overrides `faults`.
fn with_faults(state: &std::path::Path, faults: &str) {
    let mut saved = mdm_engine::state::ResumeState::load(state).unwrap();
    for url in [&mut saved.url, &mut saved.final_url] {
        let q = format!("{}&{faults}", url.query().unwrap_or(""));
        url.set_query(Some(&q));
    }
    std::fs::write(state, serde_json::to_vec(&saved).unwrap()).unwrap();
}

#[tokio::test]
async fn a_web_page_instead_of_the_file_stops_the_resume() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M/w.bin?seed=16").await;
    // The link now leads to a sign-in page.
    with_faults(&state, "html=true");
    let started = std::time::Instant::now();
    let err = Download::resume(state.clone(), Default::default(), Options::default())
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, mdm_engine::Error::GotWebPage), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(state.exists(), "the data is kept for a later resume");
}

#[tokio::test]
async fn a_server_that_sometimes_ignores_ranges_still_completes() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M/r.bin?seed=17").await;
    // Every other request is answered in full (same file, same validator): not a changed file.
    with_faults(&state, "no_range_every=2");
    let out = Download::resume(state, Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .expect("completes");
    let mut want = vec![0; 8 << 20];
    content::fill(17, 0, &mut want);
    assert!(
        std::fs::read(&out.path).unwrap() == want,
        "content mismatch"
    );
}

#[tokio::test]
async fn a_missing_folder_is_reported_and_nothing_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("unplugged");
    let err = Download::resume(gone.join("a.bin.mdm.json"), Default::default(), fast_opts())
        .err()
        .expect("refused");
    assert!(
        matches!(&err, mdm_engine::Error::DestinationUnavailable(p) if *p == gone),
        "{err}"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn a_path_too_long_for_windows_is_refused_before_anything_is_created() {
    let s = server("").await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join(format!("{}.bin", "n".repeat(250)));
    let req = Request {
        url: s.url("/gen/64K/x.bin").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::File(dest),
    };
    let err = Download::start(req, fast_opts()).wait().await.unwrap_err();
    assert!(matches!(err, mdm_engine::Error::PathTooLong(_)), "{err}");
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn a_file_bigger_than_the_drive_is_refused_before_it_starts() {
    let s = server("").await;
    let dir = tempfile::tempdir().unwrap();
    // 1 PiB: no test machine has that free.
    let req = Request {
        url: s.url("/gen/1125899906842624/huge.bin").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let err = Download::start(req, fast_opts()).wait().await.unwrap_err();
    assert!(matches!(err, mdm_engine::Error::NoSpace { .. }), "{err}");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "nothing is left behind"
    );
}

#[tokio::test]
async fn cookies_are_sent_to_their_own_host() {
    let s = server("").await;
    let dir = tempfile::tempdir().unwrap();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::COOKIE, "sid=secret".parse().unwrap());
    let req = Request {
        url: s.url("/gen/2M/c.bin?seed=4").parse().unwrap(),
        headers,
        dest: Destination::Dir(dir.path().to_owned()),
    };
    Download::start(req, fast_opts()).wait().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        s.log()
            .iter()
            .all(|e| e.cookie.as_deref() == Some("sid=secret"))
    );
}

#[tokio::test]
async fn crash_after_the_final_rename_completes_on_resume() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M/f.bin?seed=13").await;
    // Simulate a crash right after the part file was renamed, before the state was removed.
    let saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    std::fs::rename(mdm_engine::naming::part_path(&saved.dest), &saved.dest).unwrap();
    let out = Download::resume(state.clone(), Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .expect("recognised as complete");
    assert_eq!(out.path, saved.dest);
    assert!(!state.exists(), "state removed");
}

#[tokio::test]
async fn resume_rejects_damaged_part_file() {
    let s = server("rate=1M").await;
    let (_dir, state) = paused(&s, "/gen/8M?seed=9").await;
    let saved = mdm_engine::state::ResumeState::load(&state).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(mdm_engine::naming::part_path(&saved.dest))
        .unwrap()
        .set_len(10)
        .unwrap();
    let err = Download::resume(state, Default::default(), fast_opts())
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(matches!(err, mdm_engine::Error::BadState(_)), "{err}");
}

#[tokio::test]
async fn changed_file_fails() {
    // Requests 1–4 see the original validators (probe + 3 range answers confirm ranges);
    // from request 5 on the file has "changed", so If-Range yields 200.
    let s = server("rate=2M&etag_change_every=5").await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/4M").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let err = Download::start(req, fast_opts()).wait().await.unwrap_err();
    assert!(matches!(err, mdm_engine::Error::FileChanged), "{err}");
}

#[tokio::test]
async fn speed_limit_caps_the_whole_download() {
    // 8 connections, but the limit is per download (IDM: "for one file").
    let s = server("").await;
    let opts = Options {
        speed_limit: Some(1 << 20),
        ..fast_opts()
    };
    let t = std::time::Instant::now();
    let (_, size) = fetch(&s, "/gen/2M?seed=12", 12, opts).await;
    let secs = t.elapsed().as_secs_f64() - 0.3; // fetch() waits 300 ms for the server log
    assert_eq!(size, 2 << 20);
    assert!((1.5..3.0).contains(&secs), "2 MB at 1 MB/s took {secs:.2}s");
}

#[tokio::test]
async fn the_finished_file_can_take_the_servers_date() {
    // Options → Downloads → "Set file creation date as provided by the server": the lab server
    // dates its files 1_790_000_000 s after the epoch.
    let s = server("").await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/1M/dated.bin?seed=40").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let opts = Options {
        server_time: true,
        ..fast_opts()
    };
    let out = Download::start(req, opts)
        .wait()
        .await
        .expect("download succeeds");
    let meta = std::fs::metadata(&out.path).unwrap();
    let server = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
    assert_eq!(meta.modified().unwrap(), server);
    #[cfg(windows)]
    assert_eq!(meta.created().unwrap(), server);
    // Without the option the file keeps the time it was written.
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: s.url("/gen/1M/undated.bin?seed=41").parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let out = Download::start(req, fast_opts()).wait().await.unwrap();
    assert_ne!(
        std::fs::metadata(&out.path).unwrap().modified().unwrap(),
        server
    );
}

#[tokio::test]
async fn a_pac_script_routes_through_its_proxy() {
    // The lab server stands in for the proxy: a proxy is sent the whole address and the lab
    // server answers by path, so a host that can't exist only works through the script's proxy.
    let s = server("").await;
    let port = s.url("/").parse::<url::Url>().unwrap().port().unwrap();
    let script = format!(
        "function FindProxyForURL(url, host) {{ \
           return dnsDomainIs(host, '.invalid') ? 'PROXY 127.0.0.1:{port}' : 'DIRECT'; }}"
    );
    let opts = Options {
        proxy: mdm_engine::Proxy::Pac {
            script: std::sync::Arc::from(script.as_str()),
        },
        max_connections: 1,
        ..fast_opts()
    };
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: "http://nowhere.invalid/gen/64K?seed=23".parse().unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let out = Download::start(req, opts)
        .wait()
        .await
        .expect("downloaded through the proxy");
    let mut want = vec![0; 64 << 10];
    content::fill(23, 0, &mut want);
    assert!(
        std::fs::read(&out.path).unwrap() == want,
        "content mismatch"
    );
}

//! End-to-end: a real HTTP client against a running labserver.

use std::time::Duration;

use mdm_labserver::{Config, Faults, LabServer, LogEntry, content};

async fn server(faults: &str) -> LabServer {
    LabServer::start(Config {
        faults: Faults::parse(faults).unwrap(),
        ..Config::default()
    })
    .await
    .unwrap()
}

fn expected(seed: u64, start: u64, len: usize) -> Vec<u8> {
    let mut v = vec![0; len];
    content::fill(seed, start, &mut v);
    v
}

/// Log entries are written when a response body is dropped, which can trail the client slightly.
async fn wait_log(s: &LabServer, count: usize) -> Vec<LogEntry> {
    for _ in 0..200 {
        let log = s.log();
        if log.len() >= count {
            return log;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected {count} log entries, got {:?}", s.log());
}

#[tokio::test]
async fn full_get() {
    let s = server("").await;
    let r = reqwest::get(s.url("/gen/100K/a.bin?seed=3")).await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["accept-ranges"], "bytes");
    assert_eq!(
        r.headers()["content-disposition"],
        "attachment; filename=\"a.bin\""
    );
    assert_eq!(r.bytes().await.unwrap(), expected(3, 0, 100 * 1024));
    let log = wait_log(&s, 1).await;
    assert_eq!(log[0].outcome, "complete");
    assert_eq!(log[0].bytes_sent, 100 * 1024);
}

#[tokio::test]
async fn range_and_keepalive_reuse() {
    let s = server("").await;
    let c = reqwest::Client::new();
    for (i, (a, b)) in [(10u64, 19u64), (5000, 9999)].into_iter().enumerate() {
        let r = c
            .get(s.url("/gen/10000"))
            .header("Range", format!("bytes={a}-{b}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 206);
        assert_eq!(r.headers()["content-range"], format!("bytes {a}-{b}/10000"));
        assert_eq!(
            r.bytes().await.unwrap(),
            expected(0, a, (b - a + 1) as usize)
        );
        let log = wait_log(&s, i + 1).await;
        assert_eq!(log[i].conn_req, i as u64 + 1, "same TCP connection reused");
    }
}

#[tokio::test]
async fn head_and_416() {
    let s = server("").await;
    let c = reqwest::Client::new();
    let r = c.head(s.url("/gen/1M")).send().await.unwrap();
    assert_eq!(r.headers()["content-length"], "1048576");
    let r = c
        .get(s.url("/gen/100"))
        .header("Range", "bytes=100-")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 416);
    assert_eq!(r.headers()["content-range"], "bytes */100");
    let log = wait_log(&s, 2).await;
    assert_eq!(log[0].outcome, "head");
    assert_eq!(log[1].status, 416);
}

#[tokio::test]
async fn no_range_and_if_range() {
    let s = server("").await;
    let c = reqwest::Client::new();
    let r = c
        .get(s.url("/gen/1000?no_range=true"))
        .header("Range", "bytes=10-")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers().get("accept-ranges").is_none());

    // Matching If-Range -> 206; stale validator -> whole file with 200.
    let etag = r.headers()["etag"].to_str().unwrap().to_owned();
    let get = |v: String| {
        c.get(s.url("/gen/1000"))
            .header("Range", "bytes=10-")
            .header("If-Range", v)
            .send()
    };
    assert_eq!(get(etag).await.unwrap().status(), 206);
    assert_eq!(get("\"stale\"".into()).await.unwrap().status(), 200);
}

#[tokio::test]
async fn etag_changes_every_n() {
    let s = server("etag_change_every=2").await;
    let c = reqwest::Client::new();
    let mut tags = Vec::new();
    for _ in 0..3 {
        let r = c.head(s.url("/gen/10")).send().await.unwrap();
        tags.push(r.headers()["etag"].to_str().unwrap().to_owned());
    }
    assert_ne!(tags[0], tags[1]);
    assert_eq!(tags[1], tags[2]);
}

#[tokio::test]
async fn drop_after() {
    let s = server("drop_after=10K").await;
    let r = reqwest::get(s.url("/gen/1M")).await.unwrap();
    assert!(r.bytes().await.is_err(), "body must fail mid-stream");
    let log = wait_log(&s, 1).await;
    assert_eq!(log[0].outcome, "dropped");
    assert_eq!(log[0].bytes_sent, 10 * 1024);
}

#[tokio::test]
async fn max_conns_and_fail_every() {
    let s = server("max_conns=1&rate=20K").await;
    let c = reqwest::Client::new();
    let first = c.get(s.url("/gen/1M")).send().await.unwrap(); // body in flight, slot held
    let second = c.get(s.url("/gen/1M")).send().await.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(second.status(), 503);
    drop(first);
    let log = wait_log(&s, 2).await;
    assert!(log.iter().any(|e| e.outcome == "fault:max_conns"));

    let s = server("fail_every=2").await;
    let codes: Vec<u16> = futures::future::join_all((0..4).map(|_| async {
        reqwest::get(s.url("/gen/10"))
            .await
            .unwrap()
            .status()
            .as_u16()
    }))
    .await;
    assert_eq!(codes.iter().filter(|c| **c == 503).count(), 2);
}

#[tokio::test]
async fn rate_limit_is_respected() {
    let s = server("").await;
    let t = std::time::Instant::now();
    let body = reqwest::get(s.url("/gen/100K?rate=200K"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(body.len(), 100 * 1024);
    assert!((0.4..1.5).contains(&secs), "100K at 200K/s took {secs:.2}s");
}

#[tokio::test]
async fn bad_query_is_rejected() {
    let s = server("").await;
    let r = reqwest::get(s.url("/gen/10?speed=1")).await.unwrap();
    assert_eq!(r.status(), 400);
}

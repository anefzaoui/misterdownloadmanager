//! End to end: a download whose name is answered by the DNS chosen in Options (#36), not the
//! system's. A small DNS server here (plain, over TCP, on 127.0.0.1) answers `*.mdm.test` with
//! 127.0.0.1 and logs every name it is asked; the system's DNS knows no such names.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use mdm_engine::{Destination, Dns, DnsChoice, Download, Options, Request};
use mdm_labserver::{Config, LabServer, content};

/// The answer to DNS query `q`: an A record 127.0.0.1 for names ending in `.mdm.test` (others:
/// NXDOMAIN), and the name asked; None for a query it can't read.
fn answer(q: &[u8]) -> Option<(Vec<u8>, String)> {
    if q.len() < 12 {
        return None;
    }
    // The question: labels up to the zero byte, then type and class.
    let mut i = 12;
    let mut labels = Vec::new();
    while i < q.len() && q[i] != 0 {
        let l = q[i] as usize;
        labels.push(String::from_utf8_lossy(q.get(i + 1..i + 1 + l)?).to_lowercase());
        i += 1 + l;
    }
    let qend = i + 5;
    if qend > q.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([q[i + 1], q[i + 2]]);
    let name = labels.join(".");
    let ours = name.ends_with(".mdm.test");
    let mut r = Vec::with_capacity(64);
    r.extend_from_slice(&q[..2]); // the id
    // A response, recursion available; NXDOMAIN when the name isn't ours.
    r.extend_from_slice(if ours { &[0x81, 0x80] } else { &[0x81, 0x83] });
    let answers: u16 = (ours && qtype == 1).into();
    r.extend_from_slice(&[0, 1]);
    r.extend_from_slice(&answers.to_be_bytes());
    r.extend_from_slice(&[0, 0, 0, 0]);
    r.extend_from_slice(&q[12..qend]);
    if answers == 1 {
        // The name (a pointer to the question's), A, IN, 60 s, 127.0.0.1.
        r.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 127, 0, 0, 1]);
    }
    Some((r, name))
}

/// A DNS server over TCP on 127.0.0.1 (MDM asks plain DNS over TCP only), answering as
/// [`answer`] and recording each name asked.
async fn fake_dns() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                // Each message has a 2-byte length in front (RFC 1035 4.2.2).
                loop {
                    let mut len = [0u8; 2];
                    if conn.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let mut q = vec![0u8; u16::from_be_bytes(len) as usize];
                    if conn.read_exact(&mut q).await.is_err() {
                        return;
                    }
                    let Some((r, name)) = answer(&q) else { return };
                    log.lock().unwrap().push(name);
                    let mut out = (r.len() as u16).to_be_bytes().to_vec();
                    out.extend_from_slice(&r);
                    if conn.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (addr, asked)
}

fn options(dns: Dns) -> Options {
    Options {
        dns: DnsChoice {
            dns,
            fallback: false,
            label: "the test DNS".into(),
        },
        retry_delay: std::time::Duration::from_millis(50),
        max_retries: 1,
        ..Options::default()
    }
}

#[tokio::test]
async fn a_download_goes_by_the_chosen_dns() {
    let lab = LabServer::start(Config::default()).await.unwrap();
    let port = lab.url("/").parse::<url::Url>().unwrap().port().unwrap();
    let (dns, asked) = fake_dns().await;
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: format!("http://files.mdm.test:{port}/gen/1M/dns.bin?seed=36")
            .parse()
            .unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let out = Download::start(req, options(Dns::Plain(vec![dns])))
        .wait()
        .await
        .expect("the chosen DNS finds files.mdm.test");
    let data = std::fs::read(&out.path).unwrap();
    assert_eq!(data.len(), 1 << 20);
    let mut want = vec![0; data.len()];
    content::fill(36, 0, &mut want);
    assert!(data == want, "content mismatch");
    assert!(
        asked.lock().unwrap().iter().any(|n| n == "files.mdm.test"),
        "{:?}",
        asked.lock().unwrap()
    );
}

#[tokio::test]
async fn a_dead_dns_fails_the_download_and_names_itself() {
    let lab = LabServer::start(Config::default()).await.unwrap();
    let port = lab.url("/").parse::<url::Url>().unwrap().port().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: format!("http://files.mdm.test:{port}/gen/64K/x.bin")
            .parse()
            .unwrap(),
        headers: Default::default(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    // TEST-NET-1 never answers; strict: no system fallback, so the download fails.
    let err = Download::start(
        req,
        options(Dns::Plain(vec!["192.0.2.1:53".parse().unwrap()])),
    )
    .wait()
    .await
    .unwrap_err();
    let text = format!("{err} {err:?}");
    assert!(text.contains("the test DNS"), "{text}");
}

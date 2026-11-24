//! Real-Debrid against the lab server's fake (labserver `debrid`), end to end through MDM's
//! engine: the account, the host list, the unlock, the download, the device sign-in, and the
//! service's refusals. The fake host `hoster.test` resolves nowhere: a download that completes
//! came through the service's direct link.

use std::sync::Arc;
use std::time::Duration;

use mdm_debrid::{Account, Credential, Debrid, Fault, Http, ServiceKind, Services, check, signin};
use mdm_engine::{Destination, Download, Error, Options, Refusal, Request, ServiceFault};
use mdm_labserver::{Config, LabServer, content, debrid::seed};
use url::Url;

async fn lab() -> (LabServer, Arc<Http>) {
    let lab = LabServer::start(Config::default()).await.unwrap();
    let base = Url::parse(&lab.url("/")).unwrap();
    let http = Arc::new(Http::new(&Options::default(), Services::at(&base)).unwrap());
    (lab, http)
}

async fn debrid(http: &Arc<Http>, credential: Credential) -> Debrid {
    let d = Debrid::default();
    d.set_accounts(
        vec![Account {
            id: 7,
            service: ServiceKind::RealDebrid,
            credential,
        }],
        http.clone(),
    );
    d.refresh_hosts().await.unwrap();
    d
}

fn token(t: &str) -> Credential {
    Credential::Token { token: t.into() }
}

async fn download(d: &Debrid, link: &str) -> Result<std::path::PathBuf, Error> {
    let url: Url = link.parse().unwrap();
    let dir = tempfile_dir();
    let opts = Options {
        unlocker: d.unlocker_for(&url),
        max_retries: 1,
        retry_delay: Duration::from_millis(50),
        ..Options::default()
    };
    let req = Request {
        url,
        headers: Default::default(),
        dest: Destination::Dir(dir),
    };
    Download::start(req, opts).wait().await.map(|o| o.path)
}

fn tempfile_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "mdm-debrid-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn expected(link_path: &str, len: usize) -> Vec<u8> {
    let mut want = vec![0; len];
    content::fill(seed(&format!("hoster.test{link_path}")), 0, &mut want);
    want
}

#[tokio::test]
async fn a_host_link_downloads_through_the_account() {
    let (lab, http) = lab().await;
    let info = check(&http, ServiceKind::RealDebrid, token("good"))
        .await
        .unwrap();
    assert_eq!((info.user.as_str(), info.premium), ("labuser", Some(true)));
    let d = debrid(&http, token("good")).await;
    let link = "http://hoster.test/file/2M/movie.mkv?chunks=4";
    assert_eq!(
        d.account_for(&link.parse().unwrap()),
        Some((7, ServiceKind::RealDebrid))
    );
    let path = download(&d, link).await.expect("through Real-Debrid");
    assert_eq!(path.file_name().unwrap(), "movie.mkv");
    assert!(std::fs::read(&path).unwrap() == expected("/file/2M/movie.mkv", 2 << 20));
    // The token went to the API only, never to the direct link's server.
    let files: Vec<_> = lab
        .log()
        .into_iter()
        .filter(|e| e.path.starts_with("/gen/"))
        .collect();
    assert!(!files.is_empty() && files.iter().all(|e| e.authorization.is_none()));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn other_hosts_are_not_the_services() {
    let (_lab, http) = lab().await;
    let d = debrid(&http, token("good")).await;
    for other in [
        "http://example.com/a.zip",
        "http://nothoster.test/file/1M/a",
        "ftp://hoster.test/a",
    ] {
        assert!(d.unlocker_for(&other.parse().unwrap()).is_none(), "{other}");
    }
    assert!(d.hosts_known());
}

/// #1868: a 503 without an error code (the service down for maintenance, a proxy in front of it)
/// is asked again, said in the event log, and taken as the file being gone only once the
/// service's hour is up (the lab's: 400 ms).
#[tokio::test]
async fn a_bare_503_is_asked_again_then_taken_as_the_file_gone() {
    let (_lab, http) = lab().await;
    let premium = debrid(&http, token("good")).await;
    let url: Url = "http://hoster.test/file/64K/busy.bin?busy=2"
        .parse()
        .unwrap();
    let dir = tempfile_dir();
    let dl = Download::start(
        Request {
            url: url.clone(),
            headers: Default::default(),
            dest: Destination::Dir(dir.clone()),
        },
        Options {
            unlocker: premium.unlocker_for(&url),
            max_retries: 1,
            retry_delay: Duration::from_millis(50),
            ..Options::default()
        },
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !matches!(
        dl.snapshot().state,
        mdm_engine::download::State::Done | mdm_engine::download::State::Failed
    ) {
        assert!(std::time::Instant::now() < deadline, "never ended");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let said: Vec<String> = dl.snapshot().events.into_iter().map(|e| e.text).collect();
    let path = dl
        .wait()
        .await
        .expect("through Real-Debrid once it answers")
        .path;
    assert!(std::fs::read(&path).unwrap() == expected("/file/64K/busy.bin", 64 << 10));
    assert_eq!(
        said.iter()
            .filter(|t| t.contains("Real-Debrid answers 503"))
            .count(),
        2,
        "{said:#?}"
    );
    let _ = std::fs::remove_dir_all(dir);
    // Busy past the hour: the file is gone.
    let gone = download(&premium, "http://hoster.test/file/64K/busy2.bin?busy=1000")
        .await
        .unwrap_err();
    assert!(
        matches!(
            gone,
            Error::Refused(Refusal::Service {
                fault: ServiceFault::FileUnavailable,
                ..
            })
        ),
        "{gone:?}"
    );
}

#[tokio::test]
async fn the_services_refusals_say_why() {
    let (_lab, http) = lab().await;
    let refused = |e: Error| match e {
        Error::Refused(Refusal::Service { service, fault }) => {
            assert_eq!(service, "Real-Debrid");
            fault
        }
        other => panic!("{other:?}"),
    };
    let premium = debrid(&http, token("good")).await;
    let gone = download(&premium, "http://hoster.test/file/1M/a.bin?gone=1")
        .await
        .unwrap_err();
    assert_eq!(refused(gone), ServiceFault::FileUnavailable);
    let down = download(&premium, "http://hoster.test/file/1M/a.bin?down=1")
        .await
        .unwrap_err();
    assert_eq!(refused(down), ServiceFault::HostDown);
    let free = debrid(&http, token("free")).await;
    let e = download(&free, "http://hoster.test/file/1M/a.bin")
        .await
        .unwrap_err();
    assert_eq!(refused(e), ServiceFault::NotPremium);
    let bad = debrid(&http, token("wrong")).await;
    let e = download(&bad, "http://hoster.test/file/1M/a.bin")
        .await
        .unwrap_err();
    assert_eq!(refused(e), ServiceFault::SignIn);
    assert_eq!(
        check(&http, ServiceKind::RealDebrid, token("wrong")).await,
        Err(Fault::Service(ServiceFault::SignIn))
    );
}

#[tokio::test]
async fn sign_in_by_code_then_download() {
    let (_lab, http) = lab().await;
    let code = signin::start(&http, ServiceKind::RealDebrid).await.unwrap();
    assert_eq!(code.shown, "LABCODE1");
    assert!(code.page.starts_with("https://real-debrid.com/"));
    // Not allowed for the first polls, then the user-bound credential.
    let mut credential = None;
    for _ in 0..10 {
        if let Some(c) = signin::poll(&http, &code).await.unwrap() {
            credential = Some(c);
            break;
        }
        tokio::time::sleep(Duration::from_millis(code.interval * 100)).await;
    }
    let credential = credential.expect("allowed after a few polls");
    assert!(matches!(credential, Credential::Oauth { .. }));
    let info = check(&http, ServiceKind::RealDebrid, credential.clone())
        .await
        .unwrap();
    assert_eq!(info.premium, Some(true));
    let d = debrid(&http, credential).await;
    let path = download(&d, "http://hoster.test/file/1M/signed.bin")
        .await
        .unwrap();
    assert!(std::fs::read(&path).unwrap() == expected("/file/1M/signed.bin", 1 << 20));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

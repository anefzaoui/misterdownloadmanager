//! #2032: a shared Google Drive folder listed for Link grabber from the lab's fake Drive
//! (`mdm_labserver::drive`, on Google's host names, resolved to the lab server): its files and
//! its subfolders' with their folders and sizes, a Google Doc as its Office file, hostile names
//! made plain or left out and never asked for, a folder seen twice read once, Drive's 50-item
//! page said as possibly incomplete, a private folder with the browser's sign-in only; and the
//! files downloaded into their folders through the engine, as Link grabber's Start does.

use std::path::PathBuf;
use std::time::Duration;

use mdm_engine::hub::{self, Apis, HubError, Package, Site, Source};
use mdm_engine::sites::gdrive;
use mdm_engine::{Destination, Download, Options, Request};
use mdm_labserver::{Config, LabServer, LogEntry, content, drive};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue};
use url::Url;

/// Google's hosts at the lab server.
fn opts() -> Options {
    Options {
        hosts: drive::HOSTS
            .iter()
            .map(|h| (h.to_string(), vec!["127.0.0.1".parse().unwrap()]))
            .collect(),
        retry_delay: Duration::from_millis(50),
        ..Options::default()
    }
}

/// The share link of the fake folder `name`.
fn folder(s: &LabServer, name: &str) -> Url {
    format!(
        "http://drive.google.com:{}/drive/folders/{}?usp=sharing",
        s.addr().port(),
        drive::folder_id(name)
    )
    .parse()
    .unwrap()
}

async fn list(s: &LabServer, name: &str, headers: HeaderMap) -> Result<Package, HubError> {
    let source = Source::of(&folder(s, name)).expect("a folder's link");
    assert_eq!(source.site(), Site::GoogleDrive);
    hub::list_from(&source, &Apis::default(), headers, &opts()).await
}

fn signed_in() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(COOKIE, HeaderValue::from_static("SID=lab; HSID=x"));
    h
}

/// Each file as `folder/…/name`.
fn paths(p: &Package) -> Vec<String> {
    p.files
        .iter()
        .map(|f| {
            let mut parts = f.folders.clone();
            parts.push(f.name.clone());
            parts.join("/")
        })
        .collect()
}

/// The requests the server logged, once it has logged them all.
async fn log(s: &LabServer) -> Vec<LogEntry> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    s.log()
}

#[tokio::test]
async fn a_folder_and_its_subfolders_are_listed() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let p = list(&s, "root", HeaderMap::new()).await.unwrap();
    assert_eq!(p.site, Site::GoogleDrive);
    assert_eq!(p.name, "Lab folder");
    assert_eq!(p.folder, ["Lab folder"]);
    assert_eq!(
        paths(&p),
        [
            "small.bin",
            "naïve résumé.bin",
            "Lab doc.docx",
            ".._.._evil.bin",
            "_CON",
            "Sub folder/small.bin",
        ]
    );
    // `..`, an ID that isn't one, another folder's item: left out and counted; a Form isn't a
    // file; nothing lands outside the folder.
    assert_eq!(p.refused, 3);
    assert_eq!(p.not_files, 1);
    assert!(!p.capped && !p.gated);
    for f in &p.files {
        assert!(
            f.folders
                .iter()
                .chain([&f.name])
                .all(|n| hub::plain_component(n)),
            "{f:?}"
        );
        // Each file's link is built from its ID, on Drive's own hosts, and downloads as any
        // Drive link does.
        assert!(gdrive::is_drive_host(&f.url), "{}", f.url);
        assert!(gdrive::parse(&f.url).is_some(), "{}", f.url);
        assert_eq!(f.digest, None);
    }
    let size = |path: &str| p.files[paths(&p).iter().position(|x| x == path).unwrap()].size;
    assert_eq!(size("small.bin"), Some(64 << 10));
    assert_eq!(size("Sub folder/small.bin"), Some(16 << 10));
    assert_eq!(
        size("Lab doc.docx"),
        None,
        "a Doc's size is its export's, unknown"
    );
    // The package's page (the files' referer): the folder's own, without the link's extras.
    assert_eq!(
        p.page.as_str(),
        format!(
            "http://drive.google.com:{}/drive/folders/{}",
            s.addr().port(),
            drive::folder_id("root")
        )
    );
    // Each folder's page asked once (the loop back to the folder isn't read again), with a
    // browser's agent; no file and no other address asked while listing.
    let entries = log(&s).await;
    let asked: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let pages = |name: &str| {
        let path = format!("/drive/folders/{}", drive::folder_id(name));
        asked.iter().filter(|p| **p == path).count()
    };
    assert_eq!(
        (pages("root"), pages("sub"), pages("empty")),
        (1, 1, 1),
        "{asked:?}"
    );
    assert_eq!(asked.len(), 3, "{asked:?}");
    assert!(
        entries
            .iter()
            .all(|e| e.user_agent.as_deref() == Some(gdrive::USER_AGENT)),
        "Drive is asked with a browser's agent"
    );
}

/// The folder's files downloaded as Link grabber's Start downloads them: each one into its
/// folders under the package's, under the name the listing gave, byte for byte; the Doc as
/// its export.
#[tokio::test]
async fn the_folders_files_download_into_their_folders() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let p = list(&s, "root", HeaderMap::new()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = p
        .folder
        .iter()
        .fold(dir.path().to_owned(), |d, f| d.join(f));
    for f in &p.files {
        let place = f.folders.iter().fold(root.clone(), |d, n| d.join(n));
        std::fs::create_dir_all(&place).unwrap();
        let req = Request {
            url: f.url.clone(),
            headers: Default::default(),
            dest: Destination::File(place.join(&f.name)),
        };
        let out = Download::start(req, opts()).wait().await.expect(&f.name);
        assert_eq!(out.path, place.join(&f.name));
        assert!(out.path.starts_with(&root));
        let id = gdrive::parse(&f.url).unwrap().id;
        let data = std::fs::read(&out.path).unwrap();
        let mut want = vec![0; data.len()];
        content::fill(drive::seed(&id), 0, &mut want);
        assert!(data == want, "content of {}", out.path.display());
        if let Some(size) = f.size {
            assert_eq!(data.len() as u64, size, "{}", f.name);
        } else {
            assert_eq!(data.len(), 4 << 10, "the Doc's export");
        }
    }
    assert!(root.join("Sub folder").join("small.bin").is_file());
    assert!(root.join("Lab doc.docx").is_file());
    // Nothing outside the package's folder.
    let outside: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(outside, ["Lab folder"]);
}

/// Drive's page lists a folder's first 50 items: a folder that shows 50 may hold more, said.
#[tokio::test]
async fn a_full_folder_says_it_may_hold_more() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let p = list(&s, "full", HeaderMap::new()).await.unwrap();
    assert_eq!(p.files.len(), drive::FOLDER_PAGE);
    assert_eq!(drive::FOLDER_PAGE, gdrive::FOLDER_PAGE);
    assert!(p.capped);
    let p = list(&s, "sub", HeaderMap::new()).await.unwrap();
    assert!(!p.capped);
}

/// A folder shared with an account only: Google's sign-in without the browser's cookies, its
/// files with them (and its file downloads with them too).
#[tokio::test]
async fn a_private_folder_needs_the_browsers_sign_in() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let e = list(&s, "private", HeaderMap::new()).await.unwrap_err();
    assert!(matches!(e, HubError::SignIn(Site::GoogleDrive)), "{e:?}");
    let p = list(&s, "private", signed_in()).await.unwrap();
    assert_eq!(paths(&p), ["private.bin"]);
    let dir = tempfile::tempdir().unwrap();
    let req = Request {
        url: p.files[0].url.clone(),
        headers: signed_in(),
        dest: Destination::Dir(dir.path().to_owned()),
    };
    let out = Download::start(req, opts()).wait().await.unwrap();
    assert_eq!(std::fs::metadata(&out.path).unwrap().len(), 64 << 10);
}

#[tokio::test]
async fn refusals_are_said_as_such() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let e = list(&s, "busy", HeaderMap::new()).await.unwrap_err();
    assert!(
        matches!(
            e,
            HubError::Limited {
                site: Site::GoogleDrive,
                ..
            }
        ),
        "{e:?}"
    );
    let e = list(&s, "gone", HeaderMap::new()).await.unwrap_err();
    assert!(matches!(e, HubError::NotFound(Site::GoogleDrive)), "{e:?}");
}

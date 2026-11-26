//! #1774: a folder given to Link grabber, an FTP folder or a web server's listing page ("Index
//! of"), read with its subfolders from the lab server's made-up trees (labserver's `tree.rs`):
//! every file with its folders, down to the depth asked and of the types asked, never out of
//! the folder or onto another site, at most `MAX_FILES`; and its files download right.

use std::time::Duration;

use mdm_engine::folder::{self, Folder, MAX_FILES, Read};
use mdm_engine::{Destination, Download, Options, Request};
use mdm_labserver::ftp::{FtpConfig, FtpServer};
use mdm_labserver::{Config, LabServer, content, tree};

fn opts() -> Options {
    Options {
        retry_delay: Duration::from_millis(50),
        ..Options::default()
    }
}

async fn read(url: &str, depth: u8, keep: impl Fn(&str) -> bool) -> Read {
    folder::read(
        url.parse().unwrap(),
        Default::default(),
        &opts(),
        depth,
        keep,
    )
    .await
    .expect("read")
}

async fn folder_at(url: &str, depth: u8) -> Folder {
    match read(url, depth, |_| true).await {
        Read::Folder(f) => f,
        Read::Page(at, links) => panic!("a page, not a folder: {at} {links:?}"),
    }
}

/// Each file as `folder/…/name`, in the order found.
fn paths(f: &Folder) -> Vec<String> {
    f.files
        .iter()
        .map(|file| {
            let name = file.url.path_segments().unwrap().next_back().unwrap();
            let name = percent_encoding::percent_decode_str(name).decode_utf8_lossy();
            let mut p = file.folders.join("/");
            if !p.is_empty() {
                p.push('/');
            }
            p + &name
        })
        .collect()
}

/// `tree`'s files, breadth first: the folder's own, then each subfolder's.
const TREE: [&str; 6] = [
    "a.bin",
    "notes.txt",
    "docs/guide.pdf",
    "my files/photo 1.jpg",
    "docs/deep/notes.txt",
    "docs/deep/deeper/last.bin",
];

async fn lab() -> LabServer {
    LabServer::start(Config::default()).await.unwrap()
}

async fn ftp(mlsd: bool) -> FtpServer {
    FtpServer::start(
        "127.0.0.1:0".parse().unwrap(),
        FtpConfig {
            mlsd,
            ..FtpConfig::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn each_servers_listing_is_read_with_its_subfolders() {
    let s = lab().await;
    for style in ["apache", "nginx", "iis"] {
        let f = folder_at(&s.url(&format!("/idx/{style}/tree/")), 10).await;
        assert_eq!(paths(&f), TREE, "{style}");
        assert_eq!(f.name, "tree");
        assert!(!f.capped);
        // Each file's listing is its own folder's page (the referer it is asked with).
        let deep = &f.files[5];
        assert!(
            deep.listing
                .as_str()
                .ends_with(&format!("/idx/{style}/tree/docs/deep/deeper/")),
            "{}",
            deep.listing
        );
        // A web page's sizes come from the look-up, not from the listing's rounded ones.
        assert!(f.files.iter().all(|f| f.size.is_none()));
    }
}

#[tokio::test]
async fn the_depth_and_the_types_asked_are_kept() {
    let s = lab().await;
    let url = s.url("/idx/nginx/tree/");
    assert_eq!(paths(&folder_at(&url, 0).await), ["a.bin", "notes.txt"]);
    assert_eq!(
        paths(&folder_at(&url, 1).await),
        [
            "a.bin",
            "notes.txt",
            "docs/guide.pdf",
            "my files/photo 1.jpg"
        ]
    );
    let Read::Folder(f) = read(&url, 10, |n| n.ends_with(".txt")).await else {
        panic!("a folder")
    };
    assert_eq!(paths(&f), ["notes.txt", "docs/deep/notes.txt"]);
    // Depth 0 lists the one folder: no subfolder was asked for.
    s.clear_log();
    folder_at(&url, 0).await;
    let asked: Vec<String> = s.log().into_iter().map(|e| e.path).collect();
    assert!(!asked.is_empty());
    assert!(asked.iter().all(|p| p == "/idx/nginx/tree/"), "{asked:?}");
}

/// The address without its `/`: the server's redirect to the folder is followed, as a browser
/// does, and the folder read from there.
#[tokio::test]
async fn a_folder_without_its_slash_is_followed_to_it() {
    let s = lab().await;
    let f = folder_at(&s.url("/idx/apache/tree"), 10).await;
    assert_eq!(paths(&f), TREE);
}

/// The hostile listing: `../`, absolute paths, other sites, encoded dots and slashes. Only what
/// is in the folder is listed, and nothing outside it is ever asked for.
#[tokio::test]
async fn a_hostile_listing_never_leads_out_of_its_folder() {
    let s = lab().await;
    for style in ["apache", "nginx", "iis"] {
        s.clear_log();
        let base = format!("/idx/{style}/hostile/");
        let f = folder_at(&s.url(&base), 10).await;
        assert_eq!(paths(&f), ["ok.bin", "sub/inner.bin"], "{style}");
        for file in &f.files {
            assert_eq!(file.url.host_str(), Some("127.0.0.1"));
            assert!(file.url.path().starts_with(&base), "{}", file.url);
        }
        let log = s.log();
        assert!(!log.is_empty());
        for e in log {
            assert!(e.path.starts_with(&base), "{style}: asked for {}", e.path);
        }
    }
}

#[tokio::test]
async fn a_huge_listing_stops_at_the_most_files() {
    let s = lab().await;
    let f = folder_at(&s.url("/idx/nginx/huge/"), 10).await;
    assert_eq!(f.files.len(), MAX_FILES);
    assert!(f.capped);
    assert_eq!(paths(&f)[0], tree::huge_name(1));
}

/// A page that isn't a listing is read for its links as before (#705).
#[tokio::test]
async fn another_page_is_read_for_its_links() {
    let s = lab().await;
    let url = s.url("/page?link=/gen/1K/a.bin&link=/idx/apache/tree/");
    match read(&url, 10, |_| true).await {
        Read::Page(_, links) => {
            let links: Vec<String> = links.iter().map(|u| u.path().to_owned()).collect();
            assert!(links.contains(&"/gen/1K/a.bin".to_owned()), "{links:?}");
            assert!(links.contains(&"/idx/apache/tree/".to_owned()), "{links:?}");
        }
        Read::Folder(f) => panic!("not a folder: {f:?}"),
    }
}

/// An FTP folder, by `MLSD` and, on a server without it, `LIST`: the same files, their sizes
/// from the list.
#[tokio::test]
async fn an_ftp_folder_is_read_by_mlsd_or_list() {
    for mlsd in [true, false] {
        let s = ftp(mlsd).await;
        let f = folder_at(&s.url("/tree/"), 10).await;
        assert_eq!(paths(&f), TREE, "mlsd {mlsd}");
        let sizes: Vec<Option<u64>> = f.files.iter().map(|f| f.size).collect();
        assert_eq!(
            sizes,
            [65536, 1024, 131072, 32768, 2048, 4096].map(Some),
            "mlsd {mlsd}"
        );
        let verb = if mlsd { "MLSD" } else { "LIST" };
        assert!(
            s.asked()
                .iter()
                .any(|a| a == &format!("{verb} tree/my files/")),
            "{:?}",
            s.asked()
        );
        assert_eq!(
            paths(&folder_at(&s.url("/tree/"), 0).await),
            ["a.bin", "notes.txt"]
        );
    }
}

#[tokio::test]
async fn a_hostile_ftp_folder_never_leads_out() {
    for mlsd in [true, false] {
        let s = ftp(mlsd).await;
        let f = folder_at(&s.url("/hostile/"), 10).await;
        assert_eq!(paths(&f), ["ok.bin", "sub/inner.bin"], "mlsd {mlsd}");
        let asked = s.asked();
        assert!(!asked.is_empty());
        for a in &asked {
            let path = a.split_once(' ').map_or("", |(_, p)| p);
            assert!(
                path == "hostile/" || path == "hostile/sub/",
                "mlsd {mlsd}: asked {a}"
            );
        }
    }
}

#[tokio::test]
async fn a_huge_ftp_folder_stops_at_the_most_files() {
    let s = ftp(true).await;
    let f = folder_at(&s.url("/huge/"), 10).await;
    assert_eq!(f.files.len(), MAX_FILES);
    assert!(f.capped);
}

/// The folder's files download from where the listing says, over HTTP and FTP: the bytes the
/// server's tree holds, each in its subfolder.
#[tokio::test]
async fn a_folders_files_download_into_their_subfolders() {
    let http = lab().await;
    let ftp = ftp(true).await;
    for url in [http.url("/idx/apache/tree/"), ftp.url("/tree/")] {
        let f = folder_at(&url, 10).await;
        let dir = tempfile::tempdir().unwrap();
        for (file, path) in f.files.iter().zip(TREE) {
            let mut place = dir.path().to_owned();
            for sub in &file.folders {
                place.push(sub);
            }
            std::fs::create_dir_all(&place).unwrap();
            let req = Request {
                url: file.url.clone(),
                headers: Default::default(),
                dest: Destination::Dir(place.clone()),
            };
            let out = Download::start(req, opts()).wait().await.expect(path);
            let want_path = dir.path().join(path);
            assert_eq!(out.path, want_path, "{url}");
            let data = std::fs::read(&out.path).unwrap();
            let mut want = vec![0; data.len()];
            content::fill(tree::seed_of(path), 0, &mut want);
            assert!(data == want, "{path}: content");
            if let Some(size) = file.size {
                assert_eq!(data.len() as u64, size, "{path}");
            }
        }
    }
}

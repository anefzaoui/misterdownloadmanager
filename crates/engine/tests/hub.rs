//! #1772: a Hugging Face repository (whole, a folder, a revision, paged), a GitHub release and a
//! tag, listed from the lab server's fake APIs (labserver's `hub.rs`): every file with its
//! folders from the repository's root and its digest, hostile paths left out and never asked
//! for, rate limits said as such; and the package downloaded as one group, each file into its
//! folders under the download folder, checked against its digest (a wrong one said, not
//! finished).

use std::path::PathBuf;
use std::time::Duration;

use mdm_engine::hub::{self, Apis, HubError, Kind, Package, Site, Source};
use mdm_engine::{Destination, Download, Error, Options, Request};
use mdm_labserver::{Config, LabServer, hub as lab};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

fn opts() -> Options {
    Options {
        retry_delay: Duration::from_millis(50),
        ..Options::default()
    }
}

fn apis(s: &LabServer) -> Apis {
    Apis {
        hf: s.url("/hub/hf/").parse().unwrap(),
        gh_api: s.url("/hub/ghapi/").parse().unwrap(),
        gh: s.url("/hub/gh/").parse().unwrap(),
    }
}

fn hf(kind: Kind, owner: &str, name: &str, rev: Option<&str>, path: &[&str]) -> Source {
    Source::HuggingFace {
        kind,
        owner: owner.into(),
        name: name.into(),
        revision: rev.map(str::to_owned),
        path: path.iter().map(|p| (*p).to_owned()).collect(),
    }
}

async fn list(s: &LabServer, source: &Source, headers: HeaderMap) -> Result<Package, HubError> {
    hub::list_from(source, &apis(s), headers, &opts()).await
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

#[tokio::test]
async fn a_repository_its_pages_and_its_digests() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let p = list(
        &s,
        &hf(Kind::Model, "lab", "tiny", None, &[]),
        HeaderMap::new(),
    )
    .await
    .unwrap();
    let want: Vec<&str> = lab::TINY.iter().map(|(p, _)| *p).collect();
    let mut got = paths(&p);
    got.sort();
    let mut sorted = want.clone();
    sorted.sort();
    assert_eq!(got, sorted);
    assert_eq!(p.folder, ["lab", "tiny"]);
    assert_eq!(p.refused, lab::HOSTILE.len(), "every hostile path left out");
    assert!(!p.capped && !p.gated);
    assert_eq!(p.site, Site::HuggingFace);
    for f in &p.files {
        let path = [f.folders.clone(), vec![f.name.clone()]].concat().join("/");
        let size = lab::TINY.iter().find(|(p, _)| *p == path).unwrap().1;
        assert_eq!(f.size, Some(size), "{path}");
        // Asked at the revision's commit, never at a branch that may move.
        assert!(
            f.url.path().contains(&format!("/resolve/{}/", lab::SHA)),
            "{}",
            f.url
        );
        match path.as_str() {
            "config.json" => assert!(f.digest.is_none()),
            "bad/corrupt.bin" => assert!(f.digest.is_some()),
            _ => assert_eq!(
                f.digest.as_ref().unwrap().display(),
                format!("sha256:{}", lab::sha256(&format!("lab/tiny/{path}"), size)),
                "{path}"
            ),
        }
    }
    // Its tree came in pages, each the API's own next one.
    let pages = s
        .log()
        .into_iter()
        .filter(|e| e.path.contains("/tree/"))
        .count();
    let entries = lab::TINY.len() + lab::HOSTILE.len() + 3; // and its folders
    assert_eq!(pages, entries.div_ceil(lab::PAGE));
    // Nothing a hostile path names was ever asked for.
    assert!(
        s.log()
            .iter()
            .all(|e| !e.path.contains("evil") && !e.path.contains("up.bin"))
    );
}

#[tokio::test]
async fn a_folder_at_a_revision_keeps_its_place_in_the_repository() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let p = list(
        &s,
        &hf(Kind::Model, "lab", "tiny", Some("v1"), &["onnx"]),
        HeaderMap::new(),
    )
    .await
    .unwrap();
    assert_eq!(paths(&p), ["onnx/model.onnx", "onnx/deep/extra.bin"]);
    assert!(
        p.page.as_str().ends_with("/lab/tiny/tree/v1/onnx"),
        "{}",
        p.page
    );
    assert!(s.log().iter().any(|e| e.path.ends_with("/revision/v1")));
    // A revision that isn't there, a repository that isn't (or is private).
    let none = list(
        &s,
        &hf(Kind::Model, "lab", "tiny", Some("nope"), &[]),
        HeaderMap::new(),
    )
    .await;
    assert!(
        matches!(none, Err(HubError::NotFound(Site::HuggingFace))),
        "{none:?}"
    );
    let none = list(
        &s,
        &hf(Kind::Model, "lab", "missing", None, &[]),
        HeaderMap::new(),
    )
    .await;
    assert!(
        matches!(none, Err(HubError::NotFound(Site::HuggingFace))),
        "{none:?}"
    );
    // A dataset's files go under `datasets/`.
    let d = list(
        &s,
        &hf(Kind::Dataset, "lab", "rows", None, &[]),
        HeaderMap::new(),
    )
    .await
    .unwrap();
    assert_eq!(d.folder, ["datasets", "lab", "rows"]);
    assert_eq!(paths(&d), ["data/train.csv", "data/test.csv"]);
}

/// A gated repository lists with its access token only, and the token goes to the API.
#[tokio::test]
async fn a_gated_repository_takes_its_token() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let gated = hf(Kind::Model, "lab", "gated", None, &[]);
    assert!(matches!(
        list(&s, &gated, HeaderMap::new()).await,
        Err(HubError::NotFound(_))
    ));
    let mut h = HeaderMap::new();
    h.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", lab::TOKEN)).unwrap(),
    );
    // A cookie for the page's site isn't the API's.
    h.insert("cookie", HeaderValue::from_static("session=page"));
    let p = list(&s, &gated, h).await.unwrap();
    assert!(p.gated);
    assert_eq!(paths(&p), ["weights.bin"]);
    assert!(s.log().iter().all(|e| e.cookie.is_none()));
}

#[tokio::test]
async fn a_release_and_a_tag_without_one() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let gh = |owner: &str, tag: Option<&str>| Source::GitHub {
        owner: owner.into(),
        repo: "tool".into(),
        tag: tag.map(str::to_owned),
    };
    for tag in [Some("v1.0"), None] {
        let p = list(&s, &gh("lab", tag), HeaderMap::new()).await.unwrap();
        assert_eq!(p.folder, ["lab", "tool", "v1.0"]);
        assert_eq!(p.refused, 1, "../evil.exe left out");
        let names: Vec<&str> = p.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "tool-linux.tar.gz",
                "tool.exe",
                "notes.txt",
                "tool-v1.0.zip",
                "tool-v1.0.tar.gz"
            ]
        );
        // The files' addresses are built from the release, not taken from the answer.
        assert!(
            p.files
                .iter()
                .all(|f| f.url.as_str().starts_with(&s.url("/hub/gh/lab/tool/")))
        );
        let exe = &p.files[1];
        assert_eq!(
            exe.digest.as_ref().unwrap().display(),
            format!("sha256:{}", lab::sha256("lab/tool/v1.0/tool.exe", 50 << 10))
        );
        assert!(p.files[2].digest.is_none() && p.files[3].size.is_none());
    }
    let tag = list(&s, &gh("lab", Some("v0.9")), HeaderMap::new())
        .await
        .unwrap();
    let names: Vec<&str> = tag.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["tool-v0.9.zip", "tool-v0.9.tar.gz"]);
    let none = list(&s, &gh("lab", Some("v9")), HeaderMap::new()).await;
    assert!(
        matches!(none, Err(HubError::NotFound(Site::GitHub))),
        "{none:?}"
    );
}

#[tokio::test]
async fn rate_limits_say_how_long_to_wait() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let busy = Source::GitHub {
        owner: "busy".into(),
        repo: "tool".into(),
        tag: None,
    };
    match list(&s, &busy, HeaderMap::new()).await {
        Err(HubError::Limited {
            site: Site::GitHub,
            wait: Some(w),
        }) => assert!(w <= Duration::from_secs(61), "{w:?}"),
        other => panic!("{other:?}"),
    }
    match list(
        &s,
        &hf(Kind::Model, "lab", "busy", None, &[]),
        HeaderMap::new(),
    )
    .await
    {
        Err(HubError::Limited {
            site: Site::HuggingFace,
            wait,
        }) => assert_eq!(wait, Some(Duration::from_secs(140))),
        other => panic!("{other:?}"),
    }
}

/// The package downloaded as Link grabber's Start downloads it: each file into its folders
/// under the package's folder, checked against its digest; the one whose digest is wrong is
/// said, and isn't left as a finished file.
#[tokio::test]
async fn the_package_downloads_with_its_folders_and_digests() {
    let s = LabServer::start(Config::default()).await.unwrap();
    let p = list(
        &s,
        &hf(Kind::Model, "lab", "tiny", None, &[]),
        HeaderMap::new(),
    )
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = p
        .folder
        .iter()
        .fold(dir.path().to_owned(), |d, f| d.join(f));
    for f in &p.files {
        let place = f.folders.iter().fold(root.clone(), |d, n| d.join(n));
        let req = Request {
            url: f.url.clone(),
            headers: Default::default(),
            dest: Destination::Dir(place.clone()),
        };
        let out = Download::start(
            req,
            Options {
                checksum: f.digest.clone(),
                ..opts()
            },
        )
        .wait()
        .await;
        let path = [f.folders.clone(), vec![f.name.clone()]].concat().join("/");
        if path == "bad/corrupt.bin" {
            assert!(matches!(out, Err(Error::ChecksumMismatch(_))), "{out:?}");
            assert!(!place.join("corrupt.bin").exists(), "not finished");
            continue;
        }
        let out = out.expect(&path);
        assert_eq!(out.path, place.join(&f.name));
        assert!(out.path.starts_with(&root));
        let size = f.size.unwrap();
        assert_eq!(
            std::fs::read(&out.path).unwrap(),
            lab::bytes(&format!("lab/tiny/{path}"), size)
        );
    }
    assert!(root.join("onnx").join("deep").join("extra.bin").is_file());
    assert!(root.join("config.json").is_file());
}

//! #1775: files taken out of a ZIP on its server, from the lab server's made-up ZIPs
//! (labserver's `zip.rs`): the table read with a few small requests, ZIP64 too; each picked file
//! fetched alone, unpacked (stored, Deflate) and checked against its CRC-32 and size; and every
//! refusal said: no ranges, an encrypted file, another method, names that leave the folder, a
//! table that lies, entries over each other, a table too large to read.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mdm_engine::download::State;
use mdm_engine::remotezip::{self, Entry, Fetch, Listing, Pick, Problem, TAIL};
use mdm_engine::{Error, Options};
use mdm_labserver::{Config, LabServer, zip};

fn opts() -> Options {
    Options {
        retry_delay: Duration::from_millis(50),
        ..Options::default()
    }
}

async fn lab() -> LabServer {
    LabServer::start(Config::default()).await.unwrap()
}

async fn list(s: &LabServer, path: &str) -> Result<Listing, Error> {
    remotezip::list(
        &s.url(path).parse().unwrap(),
        &Default::default(),
        &opts(),
        None,
    )
    .await
}

/// A sink that writes each file it reads, whole, into `dir` under its own name.
fn sink_into(dir: &Path) -> remotezip::Sink {
    let dir = dir.to_owned();
    Box::new(move |e: &Entry, r: &mut dyn Read| {
        let mut all = Vec::new();
        r.read_to_end(&mut all)?;
        let to = dir.join(e.file_name());
        std::fs::write(&to, &all)?;
        Ok(to)
    })
}

/// Run `pick` out of the ZIP at `path`; waits for the end, at most 30 s. The files taken out.
async fn run(s: &LabServer, path: &str, pick: Pick) -> Result<Vec<(String, PathBuf)>, Error> {
    let sink = sink_into(&pick.folder);
    let f = Fetch::start(
        s.url(path).parse().unwrap(),
        Default::default(),
        opts(),
        pick,
        sink,
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let snap = f.snapshot();
        match snap.state {
            State::Done => return Ok(f.done()),
            State::Failed => return Err(snap.failure.expect("why")),
            State::Cancelled => return Err(Error::Cancelled),
            _ => {}
        }
        assert!(Instant::now() < deadline, "still {:?}", snap.state);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Take `entry` alone out of the ZIP at `path` into `dir`: where it went.
async fn fetch(s: &LabServer, path: &str, entry: &str, dir: &Path) -> Result<PathBuf, Error> {
    let pick = Pick {
        files: vec![entry.to_owned()],
        folder: dir.to_owned(),
        ..Pick::default()
    };
    let mut done = run(s, path, pick).await?;
    assert_eq!(done.len(), 1);
    Ok(done.remove(0).1)
}

/// #1775: several files in one go, a run that goes on after some were taken out already, and
/// the start-download dialog's one look at the archive's end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn several_files_in_one_go_and_a_resume_skips_those_done() {
    let s = lab().await;
    let tmp = tempfile::tempdir().unwrap();
    let files = zip::files();
    let want = ["readme.txt", "docs/guide.txt", "my files/photo 1.txt"];
    s.clear_log();
    let done = run(
        &s,
        "/zip/files.zip",
        Pick {
            files: want.iter().map(|n| (*n).to_owned()).collect(),
            folder: tmp.path().to_owned(),
            ..Pick::default()
        },
    )
    .await
    .unwrap();
    let names: Vec<&str> = done.iter().map(|d| d.0.as_str()).collect();
    assert_eq!(names, want);
    for (name, path) in &done {
        let f = files.iter().find(|f| f.name == *name).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), f.data, "{name}");
    }
    // The table, then one request per file.
    assert_eq!(s.log().len(), 1 + want.len());
    // Two said done already: only the third is fetched, the others counted.
    let again = tmp.path().join("again");
    std::fs::create_dir_all(&again).unwrap();
    s.clear_log();
    let done = run(
        &s,
        "/zip/files.zip",
        Pick {
            files: want.iter().map(|n| (*n).to_owned()).collect(),
            done: want[..2].iter().map(|n| (*n).to_owned()).collect(),
            size: Some(zip::zip("files.zip").unwrap().len() as u64),
            folder: again.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].0, "my files/photo 1.txt");
    assert_eq!(s.log().len(), 2);
    // The size known: its end asked for with a plain range, not a suffix.
    assert!(
        s.log()[0]
            .range
            .as_deref()
            .is_some_and(|r| !r.contains("=-"))
    );

    // The dialog's look: one request; the table with it when it is in the end.
    s.clear_log();
    let url = s.url("/zip/files.zip").parse().unwrap();
    let size = Some(zip::zip("files.zip").unwrap().len() as u64);
    let look = remotezip::look(&url, &Default::default(), &opts(), size).await;
    assert_eq!(look.unwrap().unwrap().entries.len(), NAMES.len());
    assert_eq!(s.log().len(), 1);
    s.clear_log();
    let url = s.url("/zip/files.zip?no_range=1").parse().unwrap();
    match remotezip::look(&url, &Default::default(), &opts(), size).await {
        Err(Error::Zip(Problem::NoRanges)) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(s.log().len(), 1);
}

/// The bytes the server sent since its log was cleared.
fn sent(s: &LabServer) -> u64 {
    s.log().iter().map(|e| e.bytes_sent).sum()
}

const NAMES: [&str; 10] = [
    "readme.txt",
    "docs/",
    "docs/guide.txt",
    "docs/stream.txt",
    "data/random.bin",
    "data/empty.txt",
    "my files/photo 1.txt",
    "ünïcode/café.txt",
    "secret.txt",
    "old.bz2",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zip_on_its_server_lists_its_files_classic_and_zip64() {
    let s = lab().await;
    for name in ["files.zip", "zip64.zip"] {
        s.clear_log();
        let l = list(&s, &format!("/zip/{name}")).await.expect(name);
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, NAMES, "{name}");
        assert_eq!(l.left_out, 0);
        assert_eq!(l.size, zip::zip(name).unwrap().len() as u64);
        let random = &l.entries[4];
        assert_eq!((random.size, random.method), (1 << 20, 0));
        assert!(l.entries[1].dir && !l.entries[1].is_file());
        // The archiver's clock: 2025-09-01 00:00:00 (the lab's).
        assert_eq!(l.entries[0].modified(), Some((2025, 9, 1, 0, 0, 0)));
        // One small request from the end: the table is in it. The archive is over 1 MiB.
        let log = s.log();
        assert_eq!(
            log.len(),
            1,
            "{name}: {:?}",
            log.iter().map(|e| &e.range).collect::<Vec<_>>()
        );
        assert!(sent(&s) <= TAIL, "{name}: {} bytes", sent(&s));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn picked_files_come_out_whole_and_checked() {
    let s = lab().await;
    let tmp = tempfile::tempdir().unwrap();
    let files = zip::files();
    for archive in ["files.zip", "zip64.zip"] {
        for f in files
            .iter()
            .filter(|f| !f.name.ends_with('/') && f.flags & 1 == 0 && matches!(f.method, 0 | 8))
        {
            let dir = tmp.path().join(archive).join(f.name.replace('/', "_"));
            std::fs::create_dir_all(&dir).unwrap();
            s.clear_log();
            let path = fetch(&s, &format!("/zip/{archive}"), &f.name, &dir)
                .await
                .unwrap_or_else(|e| panic!("{archive} {}: {e}", f.name));
            assert_eq!(
                std::fs::read(&path).unwrap(),
                f.data,
                "{archive} {}",
                f.name
            );
            // The table, then the file's own bytes: never the whole archive.
            let log = s.log();
            assert_eq!(log.len(), 2, "{archive} {}", f.name);
            assert!(log.iter().all(|e| e.status == 206));
            let archive_len = zip::zip(archive).unwrap().len() as u64;
            assert!(
                sent(&s) < TAIL + f.data.len() as u64 + 2 * 0xFFFF,
                "{archive} {}: {} of {archive_len} bytes",
                f.name,
                sent(&s)
            );
        }
    }
    // 1 MiB out of a ZIP of a bit more, and a small file out of it in a few KiB.
    s.clear_log();
    fetch(&s, "/zip/files.zip", "readme.txt", tmp.path())
        .await
        .unwrap();
    assert!(sent(&s) < TAIL + 0x2_0000, "{}", sent(&s));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refusals_say_why_and_write_nothing() {
    let s = lab().await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let zip_err = |r: Result<PathBuf, Error>| match r {
        Err(Error::Zip(p)) => p,
        other => panic!("not a ZIP refusal: {other:?}"),
    };
    assert_eq!(
        zip_err(fetch(&s, "/zip/files.zip", "secret.txt", dir).await),
        Problem::Encrypted
    );
    assert_eq!(
        zip_err(fetch(&s, "/zip/files.zip", "old.bz2", dir).await),
        Problem::Method(12)
    );
    assert_eq!(
        zip_err(fetch(&s, "/zip/files.zip", "nothing.txt", dir).await),
        Problem::Missing
    );
    // A folder isn't a file to take out.
    assert_eq!(
        zip_err(fetch(&s, "/zip/files.zip", "docs/", dir).await),
        Problem::Missing
    );
    // No ranges: said at once. Its size not known, the whole answer to the suffix range may be a
    // ZIP smaller than it: one more request, for its first byte, tells (the dialog's look, whose
    // size is known, asks once: tested above).
    s.clear_log();
    match list(&s, "/zip/files.zip?no_range=1").await {
        Err(Error::Zip(Problem::NoRanges)) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(s.log().len(), 2);
    assert_eq!(
        zip_err(fetch(&s, "/zip/files.zip?no_range=1", "readme.txt", dir).await),
        Problem::NoRanges
    );
    assert_eq!(
        std::fs::read_dir(dir).unwrap().count(),
        0,
        "nothing written"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hostile_zip_offers_only_plain_names_and_its_lies_are_caught() {
    let s = lab().await;
    let l = list(&s, "/zip/hostile.zip").await.unwrap();
    let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ok.txt",
            "sub/inner.txt",
            "up",
            "bad-crc.txt",
            "liar.txt",
            "short.txt"
        ]
    );
    assert_eq!(l.left_out, 10);
    assert!(l.entries[2].link && !l.entries[2].is_file());
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let ok = fetch(&s, "/zip/hostile.zip", "ok.txt", dir).await.unwrap();
    assert_eq!(std::fs::read(ok).unwrap(), b"the one safe file\n");
    let inner = fetch(&s, "/zip/hostile.zip", "sub/inner.txt", dir)
        .await
        .unwrap();
    assert_eq!(std::fs::read(inner).unwrap(), b"and its neighbour\n");
    for (entry, want) in [
        ("bad-crc.txt", "its CRC-32 isn't the table's"),
        ("liar.txt", "its header gives another name"),
        ("short.txt", "fewer bytes than the table says"),
    ] {
        match fetch(&s, "/zip/hostile.zip", entry, dir).await {
            Err(Error::Zip(Problem::Mismatch(why))) => assert_eq!(why, want, "{entry}"),
            other => panic!("{entry}: {other:?}"),
        }
        assert!(!dir.join(entry).exists(), "{entry} written");
    }
    // A link and a name that leaves the folder are never taken out, whatever is asked.
    for entry in ["up", "../escape.txt", "/etc/passwd", "C:/evil.txt"] {
        match fetch(&s, "/zip/hostile.zip", entry, dir).await {
            Err(Error::Zip(Problem::Missing)) => {}
            other => panic!("{entry}: {other:?}"),
        }
    }
    let mut left: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["inner.txt", "ok.txt"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tables_that_dont_add_up_are_refused_unread() {
    let s = lab().await;
    match list(&s, "/zip/overlap.zip").await {
        Err(Error::Zip(Problem::Damaged("entries overlap"))) => {}
        other => panic!("{other:?}"),
    }
    // A table said to be 64 MiB long: refused from the end record alone, nothing more asked.
    // (A ZIP this small takes the lab's server up to three requests to read its end: it answers
    // a suffix range longer than the file whole, so its size is asked, then its end.)
    s.clear_log();
    match list(&s, "/zip/bigdir.zip").await {
        Err(Error::Zip(Problem::TooBig)) => {}
        other => panic!("{other:?}"),
    }
    assert!(s.log().len() <= 3, "{}", s.log().len());
    assert!(s.log().iter().all(|e| e.range.is_some()));
    // The whole archive's first answer is dropped unread: only its last answer counts.
    let last = s.log().last().unwrap().bytes_sent;
    assert!(last < 4096, "{last}");
    match list(&s, "/gen/64K").await {
        Err(Error::Zip(Problem::NotZip)) => {}
        other => panic!("{other:?}"),
    }
}

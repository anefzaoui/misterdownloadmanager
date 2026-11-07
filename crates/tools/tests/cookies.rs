//! Whose cookies the real yt-dlp sends (#229), run through `mdm_tools` as the app runs it,
//! against the lab server (its log keeps each request's `Cookie` header). Nothing leaves this
//! computer, and no real browser profile is read: Firefox's cookies come from a throwaway
//! profile folder made for the test.
//!
//! Ignored: it needs yt-dlp's program, and for the browser part a Firefox profile folder whose
//! `cookies.sqlite` has the cookie `ff=from-firefox` for 127.0.0.1:
//!
//!   MDM_TEST_YTDLP=…\yt-dlp.exe [MDM_TEST_FIREFOX_PROFILE=<folder>] cargo test -p mdm-tools --test cookies -- --ignored --nocapture

use std::time::{Duration, Instant};

use mdm_labserver::{Config, LabServer};
use mdm_tools::proc::{BrowserCookies, FromBrowser, set_browser_cookies};
use mdm_tools::video::{State, Subs, VideoJob};

/// The cookies the lab saw on each request for `path` since `from` (the log's index).
fn cookies_seen(lab: &LabServer, path: &str, from: usize) -> Vec<String> {
    lab.log()
        .into_iter()
        .skip(from)
        .filter(|e| e.path == path)
        .map(|e| e.cookie.unwrap_or_default())
        .collect()
}

fn download(exe: &std::path::Path, url: &str, browser: &FromBrowser) -> (State, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let job = VideoJob::start(
        exe,
        url,
        "b",
        dir.path(),
        "clip",
        None,
        None,
        None,
        browser,
        &Subs::default(),
    )
    .unwrap();
    let end = Instant::now() + Duration::from_secs(120);
    let p = loop {
        let p = job.progress();
        if p.state != State::Running || Instant::now() > end {
            break p;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    (p.state, p.events.into_iter().map(|(_, e)| e).collect())
}

#[test]
#[ignore]
fn whose_cookies_ytdlp_sends() {
    let exe = std::path::PathBuf::from(std::env::var("MDM_TEST_YTDLP").expect("MDM_TEST_YTDLP"));
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = rt.block_on(LabServer::start(Config::default())).unwrap();
    let path = "/gen/1M/clip.mp4";
    let url = lab.url(path);
    let ext = FromBrowser {
        cookies: Some("ext=from-extension".into()),
        ..Default::default()
    };

    // 1. The extension's cookies: on every request, the download's own look-up included
    //    (it had none before #229, and a page that wants a sign-in failed there).
    set_browser_cookies(None);
    let at = lab.log().len();
    mdm_tools::resolve::look_up(&exe, &url, &ext).unwrap();
    let seen = cookies_seen(&lab, path, at);
    println!("1. look-up, extension's cookies: {seen:?}");
    assert!(!seen.is_empty() && seen.iter().all(|c| c.contains("ext=from-extension")));
    let at = lab.log().len();
    let (state, events) = download(&exe, &url, &ext);
    let seen = cookies_seen(&lab, path, at);
    println!("1. download, extension's cookies: {state:?} {seen:?} {events:?}");
    assert!(matches!(state, State::Done(_)), "{state:?}");
    assert!(seen.len() >= 2, "the look-up and the download: {seen:?}");
    assert!(
        seen.iter().all(|c| c.contains("ext=from-extension")),
        "{seen:?}"
    );

    // 2. No cookies and the option off: none sent.
    let at = lab.log().len();
    mdm_tools::resolve::look_up(&exe, &url, &FromBrowser::default()).unwrap();
    let seen = cookies_seen(&lab, path, at);
    println!("2. look-up, nothing: {seen:?}");
    assert!(seen.iter().all(String::is_empty));

    let Ok(profile) = std::env::var("MDM_TEST_FIREFOX_PROFILE") else {
        println!("MDM_TEST_FIREFOX_PROFILE not set: the browser's part not run");
        return;
    };
    // 3. Options → Cookies for video sites: Firefox's (the throwaway profile) when the extension
    //    sent none: a pasted address, the look-up and the download.
    set_browser_cookies(BrowserCookies::new("firefox", &profile));
    let at = lab.log().len();
    mdm_tools::resolve::look_up(&exe, &url, &FromBrowser::default()).unwrap();
    let seen = cookies_seen(&lab, path, at);
    println!("3. look-up, Firefox's: {seen:?}");
    assert!(!seen.is_empty() && seen.iter().all(|c| c.contains("ff=from-firefox")));
    let at = lab.log().len();
    let (state, events) = download(&exe, &url, &FromBrowser::default());
    let seen = cookies_seen(&lab, path, at);
    println!("3. download, Firefox's: {state:?} {seen:?} {events:?}");
    assert!(matches!(state, State::Done(_)), "{state:?}");
    assert!(
        seen.iter().all(|c| c.contains("ff=from-firefox")),
        "{seen:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.starts_with("Cookies read from firefox on this computer")),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.contains(&profile) || e.contains("from-firefox")),
        "the log names the browser only"
    );

    // 4. Both: the extension's alone (that page's, that session's).
    let at = lab.log().len();
    let (state, events) = download(&exe, &url, &ext);
    let seen = cookies_seen(&lab, path, at);
    println!("4. download, both: {state:?} {seen:?} {events:?}");
    assert!(matches!(state, State::Done(_)), "{state:?}");
    assert!(
        seen.iter()
            .all(|c| c.contains("ext=from-extension") && !c.contains("ff=")),
        "{seen:?}"
    );

    // 5. A profile folder with no cookies: yt-dlp's refusal, kept on the error.
    let empty = tempfile::tempdir().unwrap();
    set_browser_cookies(BrowserCookies::new(
        "firefox",
        &empty.path().display().to_string(),
    ));
    let err = mdm_tools::resolve::look_up(&exe, &url, &FromBrowser::default()).unwrap_err();
    println!("5. look-up, empty profile: {err}");
    assert!(
        err.to_string()
            .contains("could not find firefox cookies database"),
        "{err}"
    );
    set_browser_cookies(None);
}

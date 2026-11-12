use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use mdm_tools::proc::FromBrowser;
use mdm_tools::streams::Engine;
use mdm_tools::video::{State, Subs, VideoJob};

#[cfg(unix)]
#[test]
fn issue_796_engine_403_falls_back_to_ytdlp_and_succeeds() {
    // 1. Real local HTTP server that always rejects MDM's direct engine with 403 Forbidden.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 403 server");
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 9\r\n\r\nForbidden");
            let _ = s.flush();
        }
    });

    // 2. Temporary workspace & fake yt-dlp script that:
    //    - on `-J`: returns metadata pointing to our 403 server
    //    - on `-O MDMJ %()j`: returns chosen format pointing to our 403 server
    //    - on actual download (fallback): creates the output file and exits 0
    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("downloads");
    std::fs::create_dir_all(&out_dir).unwrap();
    let ytdlp = tmp.path().join("yt-dlp");
    let script = format!(
        r#"#!/usr/bin/env bash
set -e
URL="http://{addr}/stream.mp4"
OUT_FILE="{out}/my_video.mp4"
for arg in "$@"; do
  if [ "$arg" = "-J" ]; then
    echo '{{"title":"my_video","duration":10,"formats":[{{"format_id":"18","ext":"mp4","height":360,"vcodec":"avc1","acodec":"mp4a","protocol":"https","filesize":12,"url":"'"$URL"'"}}]}}'
    exit 0
  fi
  if [[ "$arg" == MDMJ* ]]; then
    echo 'MDMJ {{"filename":"'"$OUT_FILE"'","requested_formats":[{{"format_id":"18","ext":"mp4","protocol":"https","filesize":12,"url":"'"$URL"'","http_headers":{{"User-Agent":"yt-dlp/test"}}}}]}}'
    exit 0
  fi
done
printf "FAKE_MP4_DATA" > "$OUT_FILE"
echo "after_move:filepath=$OUT_FILE"
exit 0
"#,
        addr = addr,
        out = out_dir.display()
    );
    std::fs::write(&ytdlp, script).unwrap();
    let mut perms = std::fs::metadata(&ytdlp).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&ytdlp, perms).unwrap();

    // 3. Run VideoJob with ByEngine enabled. Engine hits 403 -> falls back to fake yt-dlp -> Done!
    let rt = tokio::runtime::Runtime::new().unwrap();
    let engine = Engine {
        handle: rt.handle().clone(),
        opts: mdm_engine::Options::default(),
    };
    let job = VideoJob::start(
        &ytdlp,
        "https://youtube.com/watch?v=test403",
        "b[height=360]",
        &out_dir,
        "my_video",
        None,
        None,
        Some(12),
        &FromBrowser::default(),
        &Subs::default(),
        Some(engine),
    )
    .expect("VideoJob started");

    let deadline = Instant::now() + Duration::from_secs(20);
    let final_progress = loop {
        let p = job.progress();
        if !matches!(p.state, State::Running) || Instant::now() >= deadline {
            break p;
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    drop(server);
    assert!(
        matches!(final_progress.state, State::Done(ref path) if path.ends_with("my_video.mp4")),
        "expected State::Done, got {:?}; events: {:?}",
        final_progress.state,
        final_progress.events
    );
    assert!(
        final_progress
            .events
            .iter()
            .any(|(_, msg)| msg.contains("falling back to yt-dlp")),
        "expected fallback log entry in events: {:?}",
        final_progress.events
    );
}

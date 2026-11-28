//! HLS streams of real media through MDM's engine and the real yt-dlp and ffmpeg: MPEG-TS pieces
//! of several codecs (and fMP4 ones beside them), stopped and started again, joined, and made
//! the MP4 a player opens, checked with ffprobe and a full decode.
//!
//! They need yt-dlp and ffmpeg (with ffprobe beside it), so they're ignored by default:
//! `MDM_MEDIA_YTDLP=<yt-dlp> MDM_MEDIA_FFMPEG=<ffmpeg> cargo test -p mdm-tools --test hls_media
//! -- --ignored --test-threads 2`. The media is made by that ffmpeg from its own test sources,
//! served by the lab server on 127.0.0.1.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use mdm_tools::video::{State, VideoJob};

struct Tools {
    ytdlp: PathBuf,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

fn tools() -> Tools {
    let var = |k: &str| PathBuf::from(std::env::var(k).unwrap_or_else(|_| panic!("{k}")));
    let ffmpeg = var("MDM_MEDIA_FFMPEG");
    let name = ffmpeg
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace("ffmpeg", "ffprobe");
    Tools {
        ytdlp: var("MDM_MEDIA_YTDLP"),
        ffprobe: ffmpeg.with_file_name(name),
        ffmpeg,
    }
}

/// 12 s of ffmpeg's test picture and a beep each second.
const SOURCES: [&str; 8] = [
    "-f",
    "lavfi",
    "-i",
    "testsrc2=size=320x180:rate=25:duration=12",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=1000:beep_factor=4:sample_rate=48000:duration=12",
];
const H264: [&str; 12] = [
    "-c:v",
    "libx264",
    "-preset",
    "veryfast",
    "-g",
    "50",
    "-sc_threshold",
    "0",
    "-pix_fmt",
    "yuv420p",
    "-bf",
    "2",
];
const HLS: [&str; 8] = [
    "-f",
    "hls",
    "-hls_time",
    "2",
    "-hls_playlist_type",
    "vod",
    "-hls_list_size",
    "0",
];

/// An HLS stream of `args` (after the sources) made in `dir/<name>`, `index.m3u8` its playlist.
fn make(t: &Tools, dir: &Path, name: &str, args: &[&str]) {
    let out = dir.join(name);
    std::fs::create_dir_all(&out).unwrap();
    let status = Command::new(&t.ffmpeg)
        .current_dir(&out)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "ffmpeg made {name}");
}

fn stream(t: &Tools, dir: &Path, name: &str, video: &[&str], audio: &[&str], hls: &[&str]) {
    let mut a: Vec<&str> = SOURCES.to_vec();
    a.extend(video);
    a.extend(audio);
    a.extend(HLS);
    a.extend(hls);
    a.push("index.m3u8");
    make(t, dir, name, &a);
}

/// The lab server, serving `media` under `/files/`.
fn lab(rt: &tokio::runtime::Runtime, media: &Path) -> mdm_labserver::LabServer {
    rt.block_on(mdm_labserver::LabServer::start(mdm_labserver::Config {
        files: Some(media.to_owned()),
        ..Default::default()
    }))
    .unwrap()
}

fn start(
    t: &Tools,
    rt: &tokio::runtime::Runtime,
    url: &str,
    out: &Path,
    limit: Option<u64>,
) -> VideoJob {
    let opts = mdm_engine::Options {
        max_connections: 3,
        speed_limit: limit,
        retry_delay: Duration::from_millis(200),
        ..Default::default()
    };
    VideoJob::start(
        &t.ytdlp,
        url,
        "bv*+ba/b",
        out,
        "Clip",
        None,
        Some(&t.ffmpeg),
        None,
        &Default::default(),
        &Default::default(),
        Some(mdm_tools::streams::Engine {
            handle: rt.handle().clone(),
            opts,
        }),
    )
    .unwrap()
}

/// The job to its end (or Stop once `stop_at` bytes came): its last progress.
fn follow(job: &VideoJob, stop_at: Option<u64>) -> mdm_tools::video::Progress {
    let end = Instant::now() + Duration::from_secs(100);
    let mut stopped = false;
    loop {
        let p = job.progress();
        if !stopped && stop_at.is_some_and(|s| p.downloaded >= s) && p.state == State::Running {
            job.stop();
            stopped = true;
        }
        if p.state != State::Running {
            return p;
        }
        assert!(
            Instant::now() < end,
            "still running after 100 s: {:?}",
            p.events
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn log_of(p: &mdm_tools::video::Progress) -> Vec<String> {
    p.events.iter().map(|(_, e)| e.clone()).collect()
}

/// What ffprobe reads of `file`: its container, how long it plays, each stream's codec.
struct Probe {
    container: String,
    duration: f64,
    codecs: Vec<String>,
}

fn probe(t: &Tools, file: &Path) -> Probe {
    let out = Command::new(&t.ffprobe)
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    Probe {
        container: v["format"]["format_name"].as_str().unwrap().to_owned(),
        duration: v["format"]["duration"].as_str().unwrap().parse().unwrap(),
        codecs: v["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["codec_name"].as_str().unwrap_or("?").to_owned())
            .collect(),
    }
}

/// Every packet of every stream decodes without an error, and each stream's timestamps only go
/// forward (no "non-monotonic DTS" left in the file).
fn plays_through(t: &Tools, file: &Path) {
    let out = Command::new(&t.ffmpeg)
        .args(["-hide_banner", "-v", "error", "-i"])
        .arg(file)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && said.trim().is_empty(), "{said}");
    let out = Command::new(&t.ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "packet=stream_index,dts",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .unwrap();
    let mut last = std::collections::HashMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut f = line.split(',');
        let (Some(i), Some(Ok(dts))) = (f.next(), f.next().map(str::parse::<i64>)) else {
            continue;
        };
        if let Some(before) = last.insert(i.to_owned(), dts) {
            assert!(dts > before, "stream {i}: DTS {dts} after {before}");
        }
    }
}

fn done(p: &mdm_tools::video::Progress) -> PathBuf {
    match &p.state {
        State::Done(f) => f.clone(),
        other => panic!("{other:?}: {:#?}", log_of(p)),
    }
}

/// An MP4 of the whole 12 s with these streams, that plays through.
fn whole_mp4(t: &Tools, f: &Path, codecs: &[&str]) {
    assert_eq!(f.extension().unwrap(), "mp4", "{f:?}");
    let p = probe(t, f);
    assert!(p.container.starts_with("mov,mp4"), "{}", p.container);
    assert!(
        (p.duration - 12.0).abs() < 0.3,
        "{f:?} plays {} s",
        p.duration
    );
    assert_eq!(p.codecs, codecs, "{f:?}");
    plays_through(t, f);
}

/// Only the finished file in the folder (nothing of the pieces, no temporary file).
fn alone(out: &Path, f: &Path) {
    let names: Vec<_> = std::fs::read_dir(out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(names, [f.to_owned()]);
}

/// H.264 and AAC in MPEG-TS pieces, stopped part-way and started again (a new job, as after
/// MDM restarts): it goes on from the pieces on disk and ends an MP4 that plays the whole 12 s;
/// fMP4 pieces beside it do too.
#[test]
#[ignore]
fn ts_stopped_and_started_again_ends_a_whole_mp4() {
    let t = tools();
    let media = tempfile::tempdir().unwrap();
    stream(
        &t,
        media.path(),
        "ts",
        &H264,
        &["-c:a", "aac", "-b:a", "96k"],
        &[],
    );
    stream(
        &t,
        media.path(),
        "fmp4",
        &H264,
        &["-c:a", "aac", "-b:a", "96k"],
        &["-hls_segment_type", "fmp4"],
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = lab(&rt, media.path());
    for name in ["ts", "fmp4"] {
        let out = tempfile::tempdir().unwrap();
        let url = lab.url(&format!("/files/{name}/index.m3u8"));
        let job = start(&t, &rt, &url, out.path(), Some(150 * 1024));
        let first = follow(&job, Some(380 * 1024));
        assert_eq!(first.state, State::Stopped, "{:#?}", log_of(&first));
        drop(job);
        let again = follow(&start(&t, &rt, &url, out.path(), None), None);
        let log = log_of(&again);
        assert!(
            log.iter()
                .any(|l| l.contains("Resuming: ") && !l.contains("Resuming: 0 ")),
            "{log:#?}"
        );
        let f = done(&again);
        whole_mp4(&t, &f, &["h264", "aac"]);
        alone(out.path(), &f);
    }
}

/// Sounds an MP4 holds as they are (AC-3, MPEG audio layer 2) and a stream whose timestamps
/// jump: a discontinuity (two clips, each its own timeline) and a 33-bit clock that wraps around
/// mid-stream. Each ends an MP4 of the whole 12 s with its timestamps in order.
#[test]
#[ignore]
fn ts_codecs_and_timestamps_an_mp4_holds() {
    let t = tools();
    let media = tempfile::tempdir().unwrap();
    let m = media.path();
    stream(&t, m, "ac3", &H264, &["-c:a", "ac3", "-b:a", "192k"], &[]);
    stream(&t, m, "mp2", &H264, &["-c:a", "mp2", "-b:a", "192k"], &[]);
    let aac = ["-c:a", "aac", "-b:a", "96k"];
    stream(&t, m, "wrap", &H264, &aac, &["-output_ts_offset", "95438"]);
    // Two 6 s clips, one playlist with a discontinuity between them.
    let disc = m.join("disc");
    for (part, picture, tone) in [("a", "testsrc2", 1000), ("b", "smptebars", 500)] {
        let mut a: Vec<String> = [
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!("{picture}=size=320x180:rate=25:duration=6"),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!("sine=frequency={tone}:beep_factor=4:sample_rate=48000:duration=6"),
        ]
        .to_vec();
        a.extend(H264.iter().chain(&aac).chain(&HLS).map(|s| s.to_string()));
        a.extend([
            "-hls_segment_filename".into(),
            format!("{part}%d.ts"),
            format!("{part}.m3u8"),
        ]);
        make(
            &t,
            m,
            "disc",
            &a.iter().map(String::as_str).collect::<Vec<_>>(),
        );
    }
    let pieces = |part: &str| -> Vec<String> {
        std::fs::read_to_string(disc.join(format!("{part}.m3u8")))
            .unwrap()
            .lines()
            .filter(|l| l.starts_with("#EXTINF") || l.ends_with(".ts"))
            .map(str::to_owned)
            .collect()
    };
    let mut playlist = vec![
        "#EXTM3U".to_owned(),
        "#EXT-X-VERSION:3".into(),
        "#EXT-X-TARGETDURATION:3".into(),
        "#EXT-X-PLAYLIST-TYPE:VOD".into(),
    ];
    playlist.extend(pieces("a"));
    playlist.push("#EXT-X-DISCONTINUITY".into());
    playlist.extend(pieces("b"));
    playlist.push("#EXT-X-ENDLIST".into());
    std::fs::write(disc.join("index.m3u8"), playlist.join("\n")).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = lab(&rt, m);
    for (name, codecs) in [
        ("ac3", ["h264", "ac3"]),
        ("mp2", ["h264", "mp3"]),
        ("wrap", ["h264", "aac"]),
        ("disc", ["h264", "aac"]),
    ] {
        let out = tempfile::tempdir().unwrap();
        let url = lab.url(&format!("/files/{name}/index.m3u8"));
        let p = follow(&start(&t, &rt, &url, out.path(), None), None);
        let f = done(&p);
        whole_mp4(&t, &f, &codecs);
        alone(out.path(), &f);
    }
}

/// AES-128 pieces (their IV the sequence number's) and byte ranges of one file
/// (`#EXT-X-BYTERANGE`): decrypted and cut as the playlist says, an MP4 of the whole 12 s.
#[test]
#[ignore]
fn ts_encrypted_and_in_byte_ranges() {
    let t = tools();
    let media = tempfile::tempdir().unwrap();
    let m = media.path();
    let aac = ["-c:a", "aac", "-b:a", "96k"];
    let aes = m.join("aes");
    std::fs::create_dir_all(&aes).unwrap();
    std::fs::write(aes.join("key.bin"), (1..=16u8).collect::<Vec<_>>()).unwrap();
    std::fs::write(
        aes.join("keyinfo.txt"),
        format!("key.bin\n{}\n", aes.join("key.bin").display()),
    )
    .unwrap();
    stream(
        &t,
        m,
        "aes",
        &H264,
        &aac,
        &["-hls_key_info_file", "keyinfo.txt"],
    );
    stream(&t, m, "single", &H264, &aac, &["-hls_flags", "single_file"]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = lab(&rt, m);
    for name in ["aes", "single"] {
        let out = tempfile::tempdir().unwrap();
        let url = lab.url(&format!("/files/{name}/index.m3u8"));
        let f = done(&follow(&start(&t, &rt, &url, out.path(), None), None));
        whole_mp4(&t, &f, &["h264", "aac"]);
        alone(out.path(), &f);
    }
}

/// AAC in LATM (DVB's way of carrying it, stream type 0x11), which an MP4 can't hold as it is:
/// yt-dlp's fix-up fails. The download ends done all the same, its pieces joined into the
/// MPEG-TS it came as (`Clip.ts`, which plays as it is: every stream, the whole 12 s), the log
/// saying why it isn't an MP4; nothing of yt-dlp's failed fix-up is left beside it.
#[test]
#[ignore]
fn ts_an_mp4_cant_hold_is_kept_as_mpeg_ts() {
    let t = tools();
    let media = tempfile::tempdir().unwrap();
    stream(
        &t,
        media.path(),
        "latm",
        &H264,
        &["-c:a", "aac", "-b:a", "96k"],
        &["-hls_segment_options", "mpegts_flags=latm"],
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    let lab = lab(&rt, media.path());
    let out = tempfile::tempdir().unwrap();
    let url = lab.url("/files/latm/index.m3u8");
    let p = follow(&start(&t, &rt, &url, out.path(), None), None);
    let f = done(&p);
    assert_eq!(f, out.path().join("Clip.ts"));
    let probed = probe(&t, &f);
    assert_eq!(probed.container, "mpegts");
    assert!((probed.duration - 12.0).abs() < 0.3, "{}", probed.duration);
    assert_eq!(probed.codecs, ["h264", "aac_latm"]);
    plays_through(&t, &f);
    alone(out.path(), &f);
    let log = log_of(&p);
    assert!(
        log.iter()
            .any(|l| l.contains("kept as the MPEG-TS it came as")),
        "{log:#?}"
    );
}

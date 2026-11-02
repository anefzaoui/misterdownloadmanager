//! Adaptive streams for the browser tests: an HLS master playlist with two video variants and an
//! audio rendition, and a DASH manifest with the same tracks. Each track has `SEGMENTS` segments
//! of `SEGMENT_SECS` seconds; their bytes are deterministic fakes (TS segments start each
//! 188-byte packet with the 0x47 sync byte, DASH init segments with an `ftyp` box), enough for a
//! client that lists, fetches or joins segments, not for one that decodes them.

use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::content;

pub const SEGMENTS: u32 = 3;
pub const SEGMENT_SECS: u32 = 4;
const TS_PACKET: usize = 188;

/// A track: its name in paths, video size (none for audio), bandwidth, bytes per segment.
struct Track {
    name: &'static str,
    video: Option<(u32, u32)>,
    bandwidth: u32,
    segment_len: usize,
}

const TRACKS: [Track; 3] = [
    Track {
        name: "720p",
        video: Some((1280, 720)),
        bandwidth: 1_280_000,
        segment_len: TS_PACKET * 256,
    },
    Track {
        name: "360p",
        video: Some((640, 360)),
        bandwidth: 640_000,
        segment_len: TS_PACKET * 128,
    },
    Track {
        name: "audio",
        video: None,
        bandwidth: 128_000,
        segment_len: TS_PACKET * 32,
    },
];

fn track(name: &str) -> Option<(usize, &'static Track)> {
    TRACKS.iter().enumerate().find(|(_, t)| t.name == name)
}

fn text(ty: &'static str, body: String) -> Response {
    ([(header::CONTENT_TYPE, ty)], body).into_response()
}

fn bytes(ty: &'static str, body: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, ty)], body).into_response()
}

/// Segment `index` of track number `track`: seeded content, distinct per track and segment.
fn segment(track: usize, index: u32, len: usize) -> Vec<u8> {
    let mut buf = vec![0; len];
    content::fill(10_000 + track as u64 * 100 + index as u64, 0, &mut buf);
    buf
}

const M3U8: &str = "application/vnd.apple.mpegurl";

/// `/hls/master.m3u8`.
pub(crate) async fn hls_master() -> Response {
    let mut s = String::from(
        "#EXTM3U\n#EXT-X-VERSION:3\n\
         #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",LANGUAGE=\"en\",\
         DEFAULT=YES,AUTOSELECT=YES,URI=\"audio/index.m3u8\"\n",
    );
    for t in &TRACKS {
        if let Some((w, h)) = t.video {
            s += &format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={w}x{h},\
                 CODECS=\"avc1.4d401f,mp4a.40.2\",AUDIO=\"aud\"\n{}/index.m3u8\n",
                t.bandwidth, t.name
            );
        }
    }
    text(M3U8, s)
}

/// `/hls/{track}/index.m3u8` (a media playlist) and `/hls/{track}/seg{i}.ts`.
pub(crate) async fn hls_file(Path((name, file)): Path<(String, String)>) -> Response {
    let Some((ti, t)) = track(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if file == "index.m3u8" {
        let mut s = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:{SEGMENT_SECS}\n\
             #EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n"
        );
        for i in 0..SEGMENTS {
            s += &format!("#EXTINF:{SEGMENT_SECS}.000,\nseg{i}.ts\n");
        }
        s += "#EXT-X-ENDLIST\n";
        return text(M3U8, s);
    }
    match numbered(&file, "seg", ".ts") {
        Some(i) if i < SEGMENTS => {
            let mut buf = segment(ti, i, t.segment_len);
            for packet in buf.chunks_mut(TS_PACKET) {
                packet[0] = 0x47;
            }
            bytes("video/mp2t", buf)
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `N` of `{prefix}N{suffix}`.
fn numbered(file: &str, prefix: &str, suffix: &str) -> Option<u32> {
    file.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

/// `/dash/manifest.mpd`.
pub(crate) async fn dash_manifest() -> Response {
    let total = SEGMENTS * SEGMENT_SECS;
    let template = format!(
        "<SegmentTemplate initialization=\"$RepresentationID$/init.mp4\" \
         media=\"$RepresentationID$/seg$Number$.m4s\" startNumber=\"1\" \
         duration=\"{SEGMENT_SECS}\" timescale=\"1\"/>"
    );
    let mut video = String::new();
    let mut audio = String::new();
    for t in &TRACKS {
        match t.video {
            Some((w, h)) => {
                video += &format!(
                    "<Representation id=\"{}\" bandwidth=\"{}\" width=\"{w}\" height=\"{h}\" \
                     codecs=\"avc1.4d401f\"/>",
                    t.name, t.bandwidth
                )
            }
            None => {
                audio += &format!(
                    "<Representation id=\"{}\" bandwidth=\"{}\" codecs=\"mp4a.40.2\" \
                     audioSamplingRate=\"48000\"/>",
                    t.name, t.bandwidth
                )
            }
        }
    }
    let s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" type=\"static\" \
         profiles=\"urn:mpeg:dash:profile:isoff-live:2011\" minBufferTime=\"PT2S\" \
         mediaPresentationDuration=\"PT{total}S\">\n\
         <Period id=\"0\" start=\"PT0S\">\n\
         <AdaptationSet id=\"0\" contentType=\"video\" mimeType=\"video/mp4\" \
         segmentAlignment=\"true\">{template}{video}</AdaptationSet>\n\
         <AdaptationSet id=\"1\" contentType=\"audio\" mimeType=\"audio/mp4\" lang=\"en\" \
         segmentAlignment=\"true\">{template}{audio}</AdaptationSet>\n\
         </Period>\n</MPD>\n"
    );
    text("application/dash+xml", s)
}

/// `/dash/{rep}/init.mp4` and `/dash/{rep}/seg{n}.m4s` (n from 1).
pub(crate) async fn dash_file(Path((name, file)): Path<(String, String)>) -> Response {
    let Some((ti, t)) = track(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ty = if t.video.is_some() {
        "video/mp4"
    } else {
        "audio/mp4"
    };
    if file == "init.mp4" {
        let mut buf = segment(ti, 999, 1024);
        let ftyp = b"\0\0\0\x18ftypiso6\0\0\0\0iso6dash";
        buf[..ftyp.len()].copy_from_slice(ftyp);
        return bytes(ty, buf);
    }
    match numbered(&file, "seg", ".m4s") {
        Some(n) if (1..=SEGMENTS).contains(&n) => bytes(ty, segment(ti, n, t.segment_len)),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

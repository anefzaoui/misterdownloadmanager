//! What a page holds, by `yt-dlp -J`: the title and the formats, turned into the few choices a
//! person picks from (one per resolution, plus audio only), as IDM's video panel offers them.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::proc::{self, ProcError};

#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub title: String,
    /// Seconds.
    pub duration: Option<f64>,
    pub formats: Vec<Format>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub id: String,
    pub ext: String,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub video: bool,
    pub audio: bool,
    /// Bytes, exact or estimated.
    pub size: Option<u64>,
    /// Kbit/s, when the site says (a stream's variants differ by it alone).
    pub tbr: Option<f64>,
}

/// One line of the choice list: what to show and what to ask yt-dlp for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    /// yt-dlp's `-f` selector.
    pub selector: String,
    /// The container of the result (for the file name).
    pub ext: String,
    pub size: Option<u64>,
}

#[derive(Deserialize)]
struct RawVideo {
    title: Option<String>,
    duration: Option<f64>,
    #[serde(default)]
    formats: Vec<RawFormat>,
}

#[derive(Deserialize)]
struct RawFormat {
    format_id: String,
    ext: Option<String>,
    height: Option<u32>,
    fps: Option<f64>,
    vcodec: Option<String>,
    acodec: Option<String>,
    filesize: Option<u64>,
    filesize_approx: Option<u64>,
    tbr: Option<f64>,
}

/// Parse `yt-dlp -J` output.
pub fn parse(json: &[u8]) -> Option<Video> {
    let raw: RawVideo = serde_json::from_slice(json).ok()?;
    // yt-dlp: "none" means the stream isn't there; a missing codec is just unknown (plain
    // files on generic sites carry both).
    let has = |c: &Option<String>| c.as_deref() != Some("none");
    Some(Video {
        title: raw.title.unwrap_or_else(|| "video".into()),
        duration: raw.duration,
        formats: raw
            .formats
            .into_iter()
            .map(|f| Format {
                video: has(&f.vcodec),
                audio: has(&f.acodec),
                id: f.format_id,
                ext: f.ext.unwrap_or_default(),
                height: f.height,
                fps: f.fps,
                size: f.filesize.or(f.filesize_approx),
                tbr: f.tbr,
            })
            .collect(),
    })
}

/// Look `url` up with yt-dlp (one video, not a playlist).
pub fn resolve(exe: &Path, url: &str, browser: &proc::FromBrowser) -> Result<Video, ProcError> {
    let mut args: Vec<String> = ["-J", "--no-playlist", "--no-warnings"]
        .map(String::from)
        .to_vec();
    // Kept until yt-dlp has answered.
    let (theirs, _cookies) = browser.args(url)?;
    args.extend(theirs);
    let cmd = proc::ytdlp(exe, &[], &args, url)?;
    let out = proc::output(cmd, "yt-dlp", Duration::from_secs(90))?;
    parse(&out).ok_or_else(|| ProcError::Failed {
        tool: "yt-dlp".into(),
        message: "it gave no video information".into(),
    })
}

/// The audio choice's label starts with this (the app translates it).
pub const AUDIO_ONLY: &str = "Audio only";

/// Containers that only ever hold sound.
fn audio_container(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "m4a" | "mp3" | "opus" | "aac" | "flac" | "wav" | "weba" | "oga" | "mka"
    )
}

/// The choices, best first: each resolution (merged with the best audio when ffmpeg is there,
/// else only formats that carry both), then audio only.
pub fn choices(v: &Video, can_merge: bool) -> Vec<Choice> {
    let mut heights: Vec<u32> = v
        .formats
        .iter()
        .filter(|f| f.video && (can_merge || f.audio))
        .filter_map(|f| f.height)
        .collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    let best_audio = v
        .formats
        .iter()
        .filter(|f| f.audio && !f.video)
        .max_by_key(|f| f.size.unwrap_or(0));
    let mut out: Vec<Choice> = heights
        .into_iter()
        .map(|h| {
            // The largest format of that height stands for its size.
            let best = v
                .formats
                .iter()
                .filter(|f| f.video && f.height == Some(h) && (can_merge || f.audio))
                .max_by_key(|f| f.size.unwrap_or(0));
            let fps = best
                .and_then(|f| f.fps)
                .filter(|fps| *fps > 31.0)
                .map(|fps| format!(" {}fps", fps.round()))
                .unwrap_or_default();
            let (selector, ext, size) = if can_merge {
                let audio = best.filter(|f| !f.audio).and(best_audio);
                (
                    format!("bv*[height={h}]+ba/b[height={h}]"),
                    "mp4".to_owned(),
                    best.and_then(|f| f.size)
                        .map(|s| s + audio.and_then(|a| a.size).unwrap_or(0)),
                )
            } else {
                (
                    format!("b[height={h}]"),
                    best.map(|f| f.ext.clone()).unwrap_or_else(|| "mp4".into()),
                    best.and_then(|f| f.size),
                )
            };
            Choice {
                label: format!("{}p{fps} {}", h, ext.to_uppercase()),
                selector,
                ext,
                size,
            }
        })
        .collect();
    // Files whose resolution isn't known (generic sites, a stream's variants with a bitrate
    // only): offered by type, and by bitrate when there are several, best first. Audio
    // containers, and streams with no video codec named, aren't videos.
    let mut plain: Vec<&Format> = v
        .formats
        .iter()
        .filter(|f| f.video && f.audio && f.height.is_none() && !audio_container(&f.ext))
        .collect();
    plain.sort_by(|a, b| b.tbr.unwrap_or(0.0).total_cmp(&a.tbr.unwrap_or(0.0)));
    let several = plain.len() > 1;
    let mut labels = std::collections::HashSet::new();
    for f in plain {
        let label = match f.tbr.filter(|_| several) {
            Some(kbps) => format!("{} {} kbps", f.ext.to_uppercase(), kbps.round()),
            None => f.ext.to_uppercase(),
        };
        // Twins (the same type and rate under two IDs) once.
        if !labels.insert(label.clone()) {
            continue;
        }
        out.push(Choice {
            label,
            selector: f.id.clone(),
            ext: f.ext.clone(),
            size: f.size,
        });
    }
    if let Some(a) = best_audio {
        out.push(Choice {
            // The app shows it in its language (videodlg::label).
            label: format!("{AUDIO_ONLY} {}", a.ext.to_uppercase()),
            selector: "ba".into(),
            ext: a.ext.clone(),
            size: a.size,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streams_variants_by_bitrate() {
        let fmt = |id: &str, tbr: f64| Format {
            id: id.into(),
            ext: "mp4".into(),
            height: None,
            fps: None,
            video: true,
            audio: true,
            size: None,
            tbr: Some(tbr),
        };
        let v = Video {
            title: "t".into(),
            duration: None,
            formats: vec![
                fmt("hls-640", 640.0),
                fmt("hls-1280", 1280.0),
                fmt("hls-1280b", 1280.0),
            ],
        };
        let labels: Vec<String> = choices(&v, true).into_iter().map(|c| c.label).collect();
        assert_eq!(
            labels,
            ["MP4 1280 kbps", "MP4 640 kbps"],
            "best first, twins once"
        );
    }

    const SAMPLE: &str = r#"{
        "title": "A talk", "duration": 61.5,
        "formats": [
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "filesize": 1000000},
            {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a", "filesize": 5000000},
            {"format_id": "136", "ext": "mp4", "height": 720, "fps": 30, "vcodec": "avc1", "acodec": "none", "filesize": 20000000},
            {"format_id": "299", "ext": "mp4", "height": 1080, "fps": 60, "vcodec": "avc1", "acodec": "none", "filesize_approx": 60000000}
        ]}"#;

    #[test]
    fn choices_with_and_without_merging() {
        let v = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(v.title, "A talk");
        let merged = choices(&v, true);
        let labels: Vec<&str> = merged.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            ["1080p 60fps MP4", "720p MP4", "360p MP4", "Audio only M4A"]
        );
        assert_eq!(merged[0].size, Some(61_000_000), "video + best audio");
        // Without ffmpeg only the format that has both survives.
        let plain = choices(&v, false);
        let labels: Vec<&str> = plain.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["360p MP4", "Audio only M4A"]);
        assert_eq!(plain[0].selector, "b[height=360]");
    }

    #[test]
    fn plain_files_without_codec_or_resolution() {
        let json = r#"{"title": "Bunny", "formats": [
            {"format_id": "ogv", "ext": "ogv", "filesize": 100},
            {"format_id": "mp4", "ext": "mp4", "filesize": 200}]}"#;
        let v = parse(json.as_bytes()).unwrap();
        let c = choices(&v, false);
        let labels: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["OGV", "MP4"]);
        assert_eq!(c[1].selector, "mp4");
    }

    #[test]
    fn audio_without_codec_info_is_not_a_video_choice() {
        let json = r#"{"title": "T", "formats": [
            {"format_id": "773", "ext": "m4a", "filesize": 100},
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "filesize": 90},
            {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a", "filesize": 500}]}"#;
        let v = parse(json.as_bytes()).unwrap();
        let labels: Vec<String> = choices(&v, false).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["360p MP4", "Audio only M4A"]);
    }

    #[test]
    fn junk_is_not_a_video() {
        assert!(parse(b"not json").is_none());
    }
}

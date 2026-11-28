//! The video's thumbnail as an audio file's cover (#1962): once a sound alone is saved (the site's
//! own M4A, or an MP3, Opus or FLAC made of it), MDM brings in the picture the site gives for the
//! video, with its own connections (the network settings of the download, as every download),
//! and MDM's ffmpeg puts it into the file. The sound is copied as it is, never encoded again.
//!
//! - MP3, M4A and FLAC take it as an attached picture (ID3's front cover, M4A's `covr`, FLAC's
//!   picture block).
//! - Opus takes it as Ogg's `METADATA_BLOCK_PICTURE` comment (what music players read in an Ogg
//!   file; ffmpeg's Ogg writer has no picture stream), written from an ffmetadata file.
//! - A WebM (the site's Opus as it comes) can't hold one: it is saved as it is.
//!
//! Whatever fails, the file stays as it was saved, and the caller says why in the download's log.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;

/// The audio files a cover goes into, by extension.
pub const HOLDERS: [&str; 4] = ["mp3", "m4a", "flac", "opus"];

/// Whether a file of type `ext` can take a cover.
pub fn holds_cover(ext: &str) -> bool {
    HOLDERS.iter().any(|h| h.eq_ignore_ascii_case(ext.trim()))
}

/// The most of a picture taken: a site's thumbnail is a few hundred KB.
const MAX_PICTURE: usize = 8 << 20;

/// How many of the page's pictures are tried, best first, before giving up.
const TRIES: usize = 3;

/// How long one picture may take to come.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// The pictures yt-dlp's look-up `info` names for the video, best first: its `thumbnail` (the
/// one yt-dlp itself would embed), then its `thumbnails` by the site's preference and size. Web
/// addresses only, each once.
pub fn candidates(info: &serde_json::Value) -> Vec<String> {
    let web = |u: &str| {
        url::Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.has_host())
    };
    let mut listed: Vec<(f64, u64, String)> = info
        .get("thumbnails")
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|t| {
                    let url = t.get("url")?.as_str()?.to_owned();
                    let num = |k: &str| t.get(k).and_then(serde_json::Value::as_f64);
                    let area = num("width").unwrap_or(0.0) * num("height").unwrap_or(0.0);
                    Some((num("preference").unwrap_or(0.0), area as u64, url))
                })
                .collect()
        })
        .unwrap_or_default();
    listed.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    let first = info
        .get("thumbnail")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let mut out: Vec<String> = Vec::new();
    for u in first
        .into_iter()
        .chain(listed.into_iter().map(|(_, _, u)| u))
    {
        if web(&u) && !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// A picture's type by its first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picture {
    Jpeg,
    Png,
    /// WebP, GIF and the like: made a JPEG first.
    Other,
}

fn picture_type(bytes: &[u8]) -> Option<Picture> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Picture::Jpeg)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Picture::Png)
    } else if (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]))
        || bytes.starts_with(b"GIF8")
        || bytes.starts_with(b"BM")
    {
        Some(Picture::Other)
    } else {
        None
    }
}

/// A JPEG's or a PNG's width and height, when its header says (0 and 0 otherwise: FLAC's picture
/// block allows it).
fn dimensions(bytes: &[u8]) -> (u32, u32) {
    let be16 = |i: usize| {
        bytes
            .get(i..i + 2)
            .map_or(0, |b| u32::from(u16::from_be_bytes([b[0], b[1]])))
    };
    let be32 = |i: usize| {
        bytes
            .get(i..i + 4)
            .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    match picture_type(bytes) {
        Some(Picture::Png) if bytes.get(12..16) == Some(&b"IHDR"[..]) => (be32(16), be32(20)),
        Some(Picture::Jpeg) => {
            // The segments up to a start of frame (SOF0 to SOF15, not DHT, JPG or DAC).
            let mut i = 2;
            while i + 9 < bytes.len() {
                if bytes[i] != 0xFF {
                    return (0, 0);
                }
                let marker = bytes[i + 1];
                if marker == 0xFF {
                    i += 1;
                    continue;
                }
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    return (be16(i + 7), be16(i + 5));
                }
                let len = be16(i + 2) as usize;
                if len < 2 {
                    return (0, 0);
                }
                i += 2 + len;
            }
            (0, 0)
        }
        _ => (0, 0),
    }
}

/// FLAC's picture block (type 3, the front cover) of JPEG or PNG `bytes`, as Ogg's
/// `METADATA_BLOCK_PICTURE` carries it, base64.
fn picture_block(bytes: &[u8], png: bool) -> String {
    let mime: &[u8] = if png { b"image/png" } else { b"image/jpeg" };
    let (w, h) = dimensions(bytes);
    let mut block = Vec::with_capacity(bytes.len() + 64);
    let mut put = |n: u32| block.extend_from_slice(&n.to_be_bytes());
    put(3);
    put(mime.len() as u32);
    block.extend_from_slice(mime);
    let mut put = |n: u32| block.extend_from_slice(&n.to_be_bytes());
    // No description; the size, a colour depth of 24 and no palette.
    put(0);
    put(w);
    put(h);
    put(24);
    put(0);
    put(bytes.len() as u32);
    block.extend_from_slice(bytes);
    base64::engine::general_purpose::STANDARD.encode(block)
}

/// An ffmetadata file's text giving the file `key=value` (its `\`, `=`, `;` and `#` escaped, as
/// ffmpeg reads it).
fn ffmetadata(key: &str, value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + value.len() / 32);
    for c in value.chars() {
        if matches!(c, '\\' | '=' | ';' | '#' | '\n') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    format!(";FFMETADATA1\n{key}={escaped}\n")
}

/// The first of `urls` that brings a picture, through `opts` (the download's proxy, DNS and
/// network adapter): its bytes. Why none did, else.
pub async fn fetch(urls: &[String], opts: &mdm_engine::Options) -> Result<Vec<u8>, String> {
    let mut why = String::from("the site names no picture of the video");
    for u in urls.iter().take(TRIES) {
        match tokio::time::timeout(FETCH_TIMEOUT, fetch_one(u, opts)).await {
            Ok(Ok(bytes)) => return Ok(bytes),
            Ok(Err(e)) => why = e,
            Err(_) => why = format!("{} took too long", host_of(u)),
        }
    }
    Err(why)
}

fn host_of(u: &str) -> String {
    url::Url::parse(u)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}

async fn fetch_one(u: &str, opts: &mdm_engine::Options) -> Result<Vec<u8>, String> {
    let url = url::Url::parse(u).map_err(|e| e.to_string())?;
    let client = mdm_engine::http_client(opts, &url).map_err(|e| e.to_string())?;
    let mut resp = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("{}: {e}", host_of(u)))?;
    if !resp.status().is_success() {
        return Err(format!("{} answered {}", host_of(u), resp.status()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > MAX_PICTURE {
            return Err(format!("the picture at {} is too large", host_of(u)));
        }
        body.extend_from_slice(&chunk);
    }
    match picture_type(&body) {
        Some(_) => Ok(body),
        None => Err(format!("{} sent no picture", host_of(u))),
    }
}

/// A file of MDM's in the helpers' folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(ext: &str) -> Scratch {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        Scratch(crate::workdir::temp_dir().join(format!(
            "mdm-cover-{}-{}.{ext}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `p` for ffmpeg: `file:<p>`, never read as a protocol or an option.
fn ff_file(p: &Path) -> std::ffi::OsString {
    let mut s = std::ffi::OsString::from("file:");
    s.push(p);
    s
}

/// `picture` (a site's thumbnail, any type) put into `file` (an MP3, M4A, FLAC or Opus) as its
/// cover by `ffmpeg`: the sound copied as it is, into `<stem>.temp.<ext>` beside it, which then
/// takes the file's place. Kept only when ffmpeg ended well and its file holds at least the
/// sound; the temporary file goes otherwise, and `file` stays as it was. Why it didn't, else.
pub fn embed(file: &Path, picture: &[u8], ffmpeg: &Path) -> Result<(), String> {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !holds_cover(&ext) {
        return Err(format!("a .{ext} file can't hold a cover"));
    }
    let kind = picture_type(picture).ok_or("the site's picture isn't one ffmpeg reads")?;
    let len = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    // A WebP (YouTube's are, as often as not) made a JPEG first: every player reads those.
    let (picture, png) = match kind {
        Picture::Jpeg => (picture.to_vec(), false),
        Picture::Png => (picture.to_vec(), true),
        Picture::Other => (as_jpeg(picture, ffmpeg)?, false),
    };
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or(Path::new("."));
    // yt-dlp's own name for a file being made, which a download's files include: never a
    // finished file of the person's.
    let temp = dir.join(format!("{stem}.temp.{ext}"));
    let pic = Scratch::new(if png { "png" } else { "jpg" });
    std::fs::write(&pic.0, &picture).map_err(|e| e.to_string())?;
    let meta = Scratch::new("txt");
    let mut cmd = crate::proc::program(ffmpeg);
    cmd.args(["-nostdin", "-y", "-i"]).arg(ff_file(file));
    if ext == "opus" {
        let text = ffmetadata("METADATA_BLOCK_PICTURE", &picture_block(&picture, png));
        std::fs::write(&meta.0, text).map_err(|e| e.to_string())?;
        cmd.args(["-f", "ffmetadata", "-i"]).arg(ff_file(&meta.0));
        cmd.args(["-map", "0:a", "-map_metadata", "1", "-c", "copy"]);
    } else {
        cmd.arg("-i").arg(ff_file(&pic.0));
        cmd.args([
            "-map",
            "0:a",
            "-map",
            "1:0",
            "-c",
            "copy",
            "-disposition:v:0",
            "attached_pic",
            "-metadata:s:v:0",
            "title=Album cover",
            "-metadata:s:v:0",
            "comment=Cover (front)",
        ]);
        if ext == "mp3" {
            cmd.args(["-id3v2_version", "3"]);
        }
    }
    cmd.arg(ff_file(&temp));
    // A copy runs at the disk's speed: a minute, and a second for every 2 MB.
    let timeout = Duration::from_secs(60 + len / (2 << 20));
    let made = crate::proc::output(cmd, "ffmpeg", timeout)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            std::fs::metadata(&temp)
                .map(|m| m.len())
                .map_err(|_| "ffmpeg made no file".to_owned())
        })
        .and_then(|n| match n >= len {
            true => Ok(()),
            false => Err(format!("ffmpeg's file is {n} bytes, the sound {len}")),
        })
        .and_then(|()| {
            replace(&temp, file).map_err(|e| format!("{stem} couldn't take its place ({e})"))
        });
    if made.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    made
}

/// `from` renamed to `to` in its place, tried again for a few seconds (Windows: a player or an
/// antivirus may hold the file a moment).
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let end = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if std::time::Instant::now() >= end => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// `picture` (WebP, GIF…) made a JPEG by `ffmpeg`.
fn as_jpeg(picture: &[u8], ffmpeg: &Path) -> Result<Vec<u8>, String> {
    let from = Scratch::new("img");
    let to = Scratch::new("jpg");
    std::fs::write(&from.0, picture).map_err(|e| e.to_string())?;
    let mut cmd = crate::proc::program(ffmpeg);
    cmd.args(["-nostdin", "-y", "-i"])
        .arg(ff_file(&from.0))
        .args([
            "-frames:v",
            "1",
            "-q:v",
            "2",
            "-c:v",
            "mjpeg",
            "-f",
            "image2",
        ])
        .arg(ff_file(&to.0));
    crate::proc::output(cmd, "ffmpeg", Duration::from_secs(60)).map_err(|e| e.to_string())?;
    let jpeg = std::fs::read(&to.0).map_err(|e| e.to_string())?;
    match picture_type(&jpeg) {
        Some(Picture::Jpeg) => Ok(jpeg),
        _ => Err("ffmpeg couldn't make the site's picture a JPEG".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_picture_first() {
        let info: serde_json::Value = serde_json::json!({
            "thumbnail": "https://i.ytimg.com/vi/x/maxresdefault.jpg",
            "thumbnails": [
                {"url": "https://i.ytimg.com/vi/x/default.jpg", "preference": -10, "width": 120, "height": 90},
                {"url": "https://i.ytimg.com/vi_webp/x/maxresdefault.webp", "preference": 0, "width": 1280, "height": 720},
                {"url": "https://i.ytimg.com/vi/x/maxresdefault.jpg", "preference": -1},
                {"url": "file:///etc/passwd", "preference": 5},
                {"url": "javascript:alert(1)"}
            ]
        });
        assert_eq!(
            candidates(&info),
            [
                "https://i.ytimg.com/vi/x/maxresdefault.jpg",
                "https://i.ytimg.com/vi_webp/x/maxresdefault.webp",
                "https://i.ytimg.com/vi/x/default.jpg",
            ]
        );
        assert!(candidates(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn only_audio_files_take_one() {
        for ext in ["mp3", "M4A", "flac", "opus"] {
            assert!(holds_cover(ext), "{ext}");
        }
        for ext in ["webm", "mp4", "weba", "mkv", ""] {
            assert!(!holds_cover(ext), "{ext}");
        }
    }

    #[test]
    fn a_pictures_size_from_its_header() {
        // A PNG's IHDR: 1280 by 720.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&1280u32.to_be_bytes());
        png.extend_from_slice(&720u32.to_be_bytes());
        assert_eq!(dimensions(&png), (1280, 720));
        // A JPEG: APP0 (16 bytes), then SOF0 with 720 rows of 1280.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend_from_slice(&[0; 14]);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02, 0xD0, 0x05, 0x00, 0x03]);
        jpeg.extend_from_slice(&[0; 12]);
        assert_eq!(dimensions(&jpeg), (1280, 720));
        assert_eq!(dimensions(b"not a picture"), (0, 0));
    }

    #[test]
    fn the_picture_block_as_ogg_carries_it() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xD9];
        let block = base64::engine::general_purpose::STANDARD
            .decode(picture_block(&jpeg, false))
            .unwrap();
        // Front cover, "image/jpeg", no description, size unknown, 24 bits, the picture.
        assert_eq!(&block[..4], &3u32.to_be_bytes());
        assert_eq!(&block[4..8], &10u32.to_be_bytes());
        assert_eq!(&block[8..18], b"image/jpeg");
        assert_eq!(&block[18..22], &0u32.to_be_bytes());
        assert_eq!(&block[22..30], &[0; 8]);
        assert_eq!(&block[30..34], &24u32.to_be_bytes());
        assert_eq!(&block[34..38], &0u32.to_be_bytes());
        assert_eq!(&block[38..42], &4u32.to_be_bytes());
        assert_eq!(&block[42..], &jpeg);
    }

    #[test]
    fn ffmetadata_escapes_what_it_reads_as_syntax() {
        assert_eq!(
            ffmetadata("K", "a=b;c#d\\e"),
            ";FFMETADATA1\nK=a\\=b\\;c\\#d\\\\e\n"
        );
    }
}

//! Addresses of pages that hold a video, not of files: Add URL hands these to the video window
//! (yt-dlp) instead of downloading the page itself. Only addresses whose shape says "one video"
//! are listed — a channel, a search or a home page is left alone. Any other address that turns
//! out to be a web page is offered to the video window after the probe (Add URL).

use reqwest::Url;

/// Whether `url` is a video's page on a site yt-dlp reads (YouTube, Vimeo, Dailymotion, Twitch,
/// TikTok, X, Instagram, Facebook, Reddit, SoundCloud, Bilibili, Rumble).
pub fn is_video_page(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .unwrap_or(&host);
    let path = url.path();
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let query = |k: &str| url.query_pairs().any(|(key, v)| key == k && !v.is_empty());
    match host {
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            (path == "/watch" && query("v"))
                || matches!(segs.as_slice(), ["shorts" | "live" | "embed" | "v", _, ..])
        }
        "youtu.be" => segs.len() == 1,
        "vimeo.com" => segs.iter().any(|s| s.chars().all(|c| c.is_ascii_digit())),
        "player.vimeo.com" => matches!(segs.as_slice(), ["video", _, ..]),
        "dailymotion.com" => matches!(segs.as_slice(), ["video", _, ..]),
        "dai.ly" => segs.len() == 1,
        "twitch.tv" => matches!(segs.as_slice(), ["videos", _] | [_, "clip", _]),
        "clips.twitch.tv" => segs.len() == 1,
        "tiktok.com" => matches!(segs.as_slice(), [_, "video", _]),
        "x.com" | "twitter.com" => matches!(segs.as_slice(), [_, "status", _, ..]),
        "instagram.com" => matches!(segs.as_slice(), ["reel" | "reels" | "p" | "tv", _, ..]),
        "facebook.com" => {
            (path == "/watch" || path == "/watch/") && query("v")
                || matches!(segs.as_slice(), [_, "videos", ..] | ["reel", _, ..])
        }
        "fb.watch" => segs.len() == 1,
        "reddit.com" | "old.reddit.com" => matches!(segs.as_slice(), ["r", _, "comments", _, ..]),
        "v.redd.it" => segs.len() == 1,
        "soundcloud.com" => segs.len() == 2,
        "bilibili.com" => matches!(segs.as_slice(), ["video", _, ..]),
        "rumble.com" => segs
            .first()
            .is_some_and(|s| s.starts_with('v') && s.contains('-')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(s: &str) -> bool {
        is_video_page(&s.parse().unwrap())
    }

    #[test]
    fn video_pages_are_recognised() {
        // Reported 2026-10-29: a watch page from a mix (list=RD…, start_radio) — one video.
        assert!(page(
            "https://www.youtube.com/watch?v=Tg6byXUo8RU&list=RDTg6byXUo8RU&start_radio=1"
        ));
        assert!(page("https://youtube.com/watch?v=Tg6byXUo8RU"));
        assert!(page("https://m.youtube.com/watch?v=Tg6byXUo8RU&t=42s"));
        assert!(page("https://music.youtube.com/watch?v=Tg6byXUo8RU"));
        assert!(page("https://www.youtube.com/shorts/abcDEF12345"));
        assert!(page("https://youtu.be/Tg6byXUo8RU?si=x"));
        assert!(page("https://vimeo.com/76979871"));
        assert!(page("https://www.dailymotion.com/video/x8abcd"));
        assert!(page("https://www.twitch.tv/videos/123456"));
        assert!(page(
            "https://www.tiktok.com/@someone/video/7300000000000000000"
        ));
        assert!(page("https://x.com/someone/status/1800000000000000000"));
        assert!(page("https://www.instagram.com/reel/Cabc123/"));
        assert!(page(
            "https://www.reddit.com/r/videos/comments/abc123/title/"
        ));
        assert!(page("https://soundcloud.com/artist/track"));
    }

    #[test]
    fn other_pages_and_files_are_not() {
        assert!(!page("https://www.youtube.com/"));
        assert!(!page("https://www.youtube.com/watch"));
        assert!(!page("https://www.youtube.com/@channel/videos"));
        assert!(!page("https://www.youtube.com/results?search_query=x"));
        assert!(!page("https://www.youtube.com/playlist?list=PLx"));
        assert!(!page("https://vimeo.com/channels/staffpicks"));
        assert!(!page("https://example.com/video.mp4"));
        assert!(!page("https://x.com/someone"));
        assert!(!page("ftp://youtube.com/watch?v=x"));
    }
}

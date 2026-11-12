//! Separated media-extraction policies: format ranking, engine fallback decisions,
//! and media title/filename sanitization.
//!
//! Keeping these rules in a pure, side-effect-free module decouples `yt-dlp` selection
//! and fallback heuristics from process spawning (`video.rs`) and JSON parsing (`resolve.rs`).

/// Returns `true` when an internal engine error indicates the upstream CDN/host rejected
/// MDM's direct HTTP request (e.g. YouTube 403 Forbidden / expired signature / bot check)
/// and `yt-dlp` should be given a chance to download the stream itself.
pub fn should_fallback_to_helper(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("410")
        || lower.contains("gone")
}

/// Returns `true` when `name` looks like a server-side script endpoint (e.g. `remote_control.php`)
/// rather than a meaningful media title (#800).
pub fn is_generic_script_stem(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    [".php", ".asp", ".aspx", ".jsp", ".cgi", ".pl", ".py"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Minimal view of a video format needed for representative selection.
#[derive(Clone, Debug, PartialEq)]
pub struct FormatCandidate<'a> {
    pub index: usize,
    pub ext: &'a str,
    pub video: bool,
    pub audio: bool,
    pub height: Option<u32>,
    pub size: Option<u64>,
    pub tbr: Option<f64>,
}

/// Selects the representative format for `height` that best matches what `yt-dlp`'s
/// `bv*[height=H]` / `b[height=H]` selector will actually download (#801).
///
/// `yt-dlp -J` emits formats ordered from worst to best according to its own format
/// sorting; picking blindly by `max(size)` can select an outlier multi-gigabyte stream
/// while `yt-dlp` downloads a much smaller default stream. Instead, we prefer:
/// 1. Formats with a known size/estimate
/// 2. MP4 container (matching `yt-dlp`'s default merge/container preference)
/// 3. Highest `tbr` (bitrate), breaking ties by later position in `yt-dlp`'s sorted list.
pub fn pick_representative_index<'a, I>(
    candidates: I,
    height: u32,
    can_merge: bool,
) -> Option<usize>
where
    I: IntoIterator<Item = FormatCandidate<'a>>,
{
    candidates
        .into_iter()
        .filter(|f| f.video && f.height == Some(height) && (can_merge || f.audio))
        .max_by(|a, b| {
            let key = |f: &FormatCandidate<'_>| {
                (
                    f.size.is_some(),
                    f.ext.eq_ignore_ascii_case("mp4"),
                    f.tbr.map(|t| (t * 100.0) as u64).unwrap_or(0),
                    f.index,
                )
            };
            key(a).cmp(&key(b))
        })
        .map(|f| f.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_triggers_on_403_and_auth_errors() {
        assert!(should_fallback_to_helper("HTTP Error 403: Forbidden"));
        assert!(should_fallback_to_helper("server returned 403 Forbidden"));
        assert!(should_fallback_to_helper("HTTP 401 Unauthorized"));
        assert!(should_fallback_to_helper("410 Gone"));
        assert!(!should_fallback_to_helper("connection timed out"));
        assert!(!should_fallback_to_helper("disk full"));
    }

    #[test]
    fn detects_generic_script_endpoints() {
        assert!(is_generic_script_stem("remote_control.php"));
        assert!(is_generic_script_stem("  download.ASPX "));
        assert!(!is_generic_script_stem("My Vacation Video.mp4"));
        assert!(!is_generic_script_stem("Lecture 01"));
    }

    #[test]
    fn representative_format_avoids_non_mp4_size_outlier_801() {
        // Simulates #801: an outlier 3.79 GB webm/av1 or raw stream vs normal 532 MB mp4 stream.
        let formats = [
            FormatCandidate {
                index: 0,
                ext: "webm",
                video: true,
                audio: false,
                height: Some(1080),
                size: Some(3_790_000_000),
                tbr: Some(1800.0),
            },
            FormatCandidate {
                index: 1,
                ext: "mp4",
                video: true,
                audio: false,
                height: Some(1080),
                size: Some(532_260_000),
                tbr: Some(2500.0),
            },
        ];
        assert_eq!(pick_representative_index(formats, 1080, true), Some(1));
    }
}

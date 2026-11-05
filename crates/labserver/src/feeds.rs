//! RSS feeds for the app's RSS tests (#39): a made-up show's feed (RSS 2.0 or Atom) that grows
//! episode by episode, and a feed of the `.torrent` files in a folder (`--rss-dir`), served
//! here too. Never real feeds.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::server::Shared;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn pairs(q: Option<String>) -> Vec<(String, String)> {
    q.and_then(|q| serde_urlencoded::from_str(&q).ok())
        .unwrap_or_default()
}

/// A made-up info hash for a title (FNV-1a, spread over 20 bytes): the same on every request.
fn fake_hash(title: &str) -> String {
    let mut out = String::new();
    for round in 0u64..5 {
        let mut h: u32 = 0x811c_9dc5 ^ (round as u32);
        for b in title.bytes() {
            h ^= u32::from(b);
            h = h.wrapping_mul(0x0100_0193);
        }
        out.push_str(&format!("{h:08x}"));
    }
    out
}

struct Item {
    title: String,
    link: String,
    guid: String,
    size: u64,
    /// Minutes before "now" (a fixed date), for the dates.
    age: u64,
}

fn rss(title: &str, items: &[Item]) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<rss version=\"2.0\">\n<channel>\n<title>{}</title>\n<link>http://127.0.0.1/</link>\n",
        esc(title)
    );
    for i in items {
        let date = date_rfc822(i.age);
        s.push_str(&format!(
            "<item><title>{}</title><guid isPermaLink=\"false\">{}</guid><pubDate>{date}</pubDate>\
             <enclosure url=\"{}\" type=\"application/x-bittorrent\" length=\"{}\"/></item>\n",
            esc(&i.title),
            esc(&i.guid),
            esc(&i.link),
            i.size
        ));
    }
    s.push_str("</channel>\n</rss>\n");
    s
}

fn atom(title: &str, items: &[Item]) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n<title>{}</title>\n",
        esc(title)
    );
    for i in items {
        s.push_str(&format!(
            "<entry><id>{}</id><title>{}</title><updated>{}</updated>\
             <link rel=\"enclosure\" type=\"application/x-bittorrent\" href=\"{}\" length=\"{}\"/></entry>\n",
            esc(&i.guid),
            esc(&i.title),
            date_rfc3339(i.age),
            esc(&i.link),
            i.size
        ));
    }
    s.push_str("</feed>\n");
    s
}

/// 2026-11-01 12:00 UTC less `age` minutes (fixed: the same feed on every request).
fn base_minutes() -> u64 {
    29_850_000
}

fn civil(age: u64) -> (i64, u32, u32, u32, u32) {
    let mins = base_minutes() - age.min(base_minutes());
    let days = (mins / 1440) as i64;
    let (h, m) = (((mins % 1440) / 60) as u32, (mins % 60) as u32);
    // Days since 1970-01-01 to a date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(mo <= 2);
    (y, mo, d, h, m)
}

fn date_rfc822(age: u64) -> String {
    let (y, mo, d, h, m) = civil(age);
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{d:02} {} {y} {h:02}:{m:02}:00 GMT", MON[(mo - 1) as usize])
}

fn date_rfc3339(age: u64) -> String {
    let (y, mo, d, h, m) = civil(age);
    format!("{y}-{mo:02}-{d:02}T{h:02}:{m:02}:00Z")
}

fn xml(body: String, atom: bool) -> Response {
    let kind = if atom {
        "application/atom+xml; charset=utf-8"
    } else {
        "application/rss+xml; charset=utf-8"
    };
    ([(header::CONTENT_TYPE, kind)], body).into_response()
}

/// GET /rss/shows.xml?upto=N&atom=1&torrent=URL — "Open Show" episodes 1..N of season 1 (default
/// 3), each in 720p and 1080p, and "Other Show" 1x01..: magnet links of made-up hashes, or
/// `torrent` (every article the same .torrent address, for a download).
pub(crate) async fn shows(RawQuery(q): RawQuery) -> Response {
    let q = pairs(q);
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let upto: u64 = get("upto").and_then(|v| v.parse().ok()).unwrap_or(3).min(99);
    let torrent = get("torrent");
    let mut items = Vec::new();
    for e in (1..=upto).rev() {
        for (show, title) in [
            ("open", format!("Open Show S01E{e:02} 1080p WEB")),
            ("open", format!("Open Show S01E{e:02} 720p WEB")),
            ("other", format!("Other Show 1x{e:02} HDTV")),
        ] {
            let hash = fake_hash(&title);
            let link = match torrent {
                Some(t) => t.to_owned(),
                None => format!(
                    "magnet:?xt=urn:btih:{hash}&dn={}",
                    title.replace(' ', "+")
                ),
            };
            items.push(Item {
                guid: format!("{show}-{hash}"),
                title,
                link,
                size: (300 + e * 7) << 20,
                age: (upto - e) * 1440 + 30,
            });
        }
    }
    if get("atom").is_some() {
        xml(atom("Open Shows", &items), true)
    } else {
        xml(rss("Open Shows", &items), false)
    }
}

fn files(shared: &Shared) -> Option<(PathBuf, Vec<(String, u64, u64)>)> {
    let dir = shared.rss_dir.clone()?;
    let mut out: Vec<(String, u64, u64)> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = e.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            name.to_ascii_lowercase()
                .ends_with(".torrent")
                .then_some((name, meta.len(), modified))
        })
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    Some((dir, out))
}

/// GET /rss/dir.xml — the .torrent files in `--rss-dir`, newest first, each linked to
/// /rss/files/NAME (its title: the name without .torrent).
pub(crate) async fn dir(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let Some((_, list)) = files(&shared) else {
        return (StatusCode::NOT_FOUND, "no --rss-dir").into_response();
    };
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1");
    let items: Vec<Item> = list
        .iter()
        .enumerate()
        .map(|(i, (name, len, modified))| Item {
            title: name[..name.len() - ".torrent".len()].to_owned(),
            link: format!(
                "http://{host}/rss/files/{}",
                url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>()
            ),
            guid: format!("{name}-{modified}"),
            size: *len,
            age: i as u64 * 60,
        })
        .collect();
    xml(rss("Lab torrents", &items), false)
}

/// GET /rss/files/NAME — one of `--rss-dir`'s .torrent files.
pub(crate) async fn file(State(shared): State<Arc<Shared>>, Path(name): Path<String>) -> Response {
    let Some((dir, list)) = files(&shared) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !list.iter().any(|(n, _, _)| *n == name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match std::fs::read(dir.join(&name)) {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/x-bittorrent")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_and_hashes_are_fixed() {
        assert_eq!(super::date_rfc3339(0), super::date_rfc3339(0));
        assert!(super::date_rfc822(0).ends_with("GMT"));
        assert_eq!(super::fake_hash("a").len(), 40);
        assert_ne!(super::fake_hash("a"), super::fake_hash("b"));
    }
}

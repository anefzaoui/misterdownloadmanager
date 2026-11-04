//! Which of a torrent's trackers MDM lets librqbit contact (#40). A torrent names its trackers,
//! and a tracker on this computer or the local network (`127.0.0.1`, `192.168.1.1`, `localhost`)
//! lets a torrent make MDM send requests into the network it sits in: those are dropped. With a
//! proxy, UDP trackers are dropped too: librqbit sends them around the proxy.
//!
//! The list is changed in the torrent itself (`announce`, `announce-list`, outside the info
//! dictionary, so its hash stays the same) and in a magnet link (`tr=`).

use std::net::IpAddr;

/// Whether MDM may contact `tracker`.
pub fn allowed(tracker: &str, proxied: bool) -> bool {
    let Ok(url) = url::Url::parse(tracker.trim()) else {
        return false;
    };
    match url.scheme() {
        "http" | "https" => {}
        "udp" if !proxied => {}
        _ => return false,
    }
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            !(d == "localhost" || d.ends_with(".localhost") || d.ends_with(".local"))
        }
        Some(url::Host::Ipv4(a)) => public(IpAddr::V4(a)),
        Some(url::Host::Ipv6(a)) => public(IpAddr::V6(a)),
        None => false,
    }
}

/// An address on the internet (not this computer, the local network or a reserved range).
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            !(a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast()
                || a.is_multicast()
                // 100.64.0.0/10, carrier-grade NAT.
                || (a.octets()[0] == 100 && (a.octets()[1] & 0xc0) == 64))
        }
        IpAddr::V6(a) => {
            let seg = a.segments()[0];
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || (seg & 0xfe00) == 0xfc00 // unique local
                || (seg & 0xffc0) == 0xfe80 // link local
                || a.to_ipv4_mapped().is_some_and(|v4| !public(IpAddr::V4(v4))))
        }
    }
}

/// A magnet link with only the trackers MDM may contact.
pub fn filter_magnet(magnet: &str, proxied: bool) -> String {
    let Some((head, query)) = magnet.split_once('?') else {
        return magnet.to_owned();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| match kv.split_once('=') {
            Some((k, v)) if k.eq_ignore_ascii_case("tr") => {
                let v = url::form_urlencoded::parse(format!("x={v}").as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
                allowed(&v, proxied)
            }
            _ => true,
        })
        .collect();
    format!("{head}?{}", kept.join("&"))
}

/// A .torrent with only the trackers MDM may contact; everything else byte for byte, so its
/// info hash is the same. None when it isn't a bencoded dictionary.
pub fn filter_torrent(bytes: &[u8], proxied: bool) -> Option<Vec<u8>> {
    let entries = top_level(bytes)?;
    let mut out = b"d".to_vec();
    for (key, value) in entries {
        match key {
            b"announce" => {
                let url = string_at(value)?;
                if allowed(&String::from_utf8_lossy(url), proxied) {
                    push_string(&mut out, key);
                    out.extend_from_slice(value);
                }
            }
            b"announce-list" => {
                let tiers: Vec<Vec<&[u8]>> = list_of_lists(value)?
                    .into_iter()
                    .map(|tier| {
                        tier.into_iter()
                            .filter(|u| allowed(&String::from_utf8_lossy(u), proxied))
                            .collect::<Vec<_>>()
                    })
                    .filter(|tier| !tier.is_empty())
                    .collect();
                if !tiers.is_empty() {
                    push_string(&mut out, key);
                    out.push(b'l');
                    for tier in tiers {
                        out.push(b'l');
                        for u in tier {
                            push_string(&mut out, u);
                        }
                        out.push(b'e');
                    }
                    out.push(b'e');
                }
            }
            _ => {
                push_string(&mut out, key);
                out.extend_from_slice(value);
            }
        }
    }
    out.push(b'e');
    Some(out)
}

fn push_string(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(s);
}

/// The top-level dictionary's entries: each key and its value's bytes.
fn top_level(b: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    if b.first() != Some(&b'd') {
        return None;
    }
    let mut at = 1;
    let mut out = Vec::new();
    while *b.get(at)? != b'e' {
        let (key, next) = string(b, at)?;
        let end = skip(b, next, 0)?;
        out.push((key, &b[next..end]));
        at = end;
    }
    (at + 1 == b.len()).then_some(out)
}

/// A string at `at`: its bytes, and where it ends.
fn string(b: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let colon = at + b.get(at..)?.iter().position(|&c| c == b':')?;
    let len: usize = std::str::from_utf8(&b[at..colon]).ok()?.parse().ok()?;
    let start = colon + 1;
    let end = start.checked_add(len)?;
    Some((b.get(start..end)?, end))
}

fn string_at(value: &[u8]) -> Option<&[u8]> {
    let (s, end) = string(value, 0)?;
    (end == value.len()).then_some(s)
}

fn list_of_lists(value: &[u8]) -> Option<Vec<Vec<&[u8]>>> {
    if value.first() != Some(&b'l') {
        return None;
    }
    let mut at = 1;
    let mut tiers = Vec::new();
    while *value.get(at)? != b'e' {
        if value[at] != b'l' {
            return None;
        }
        at += 1;
        let mut tier = Vec::new();
        while *value.get(at)? != b'e' {
            let (s, next) = string(value, at)?;
            tier.push(s);
            at = next;
        }
        at += 1;
        tiers.push(tier);
    }
    Some(tiers)
}

/// Where the value at `at` ends; nesting is limited (librqbit 9.0.1's crash was unbounded depth).
fn skip(b: &[u8], at: usize, depth: usize) -> Option<usize> {
    if depth > 64 {
        return None;
    }
    match *b.get(at)? {
        b'i' => Some(at + b.get(at..)?.iter().position(|&c| c == b'e')? + 1),
        b'l' | b'd' => {
            let mut at = at + 1;
            while *b.get(at)? != b'e' {
                at = skip(b, at, depth + 1)?;
            }
            Some(at + 1)
        }
        b'0'..=b'9' => string(b, at).map(|(_, end)| end),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_odd_trackers_are_dropped() {
        for ok in [
            "http://tracker.example.org:80/announce",
            "https://t.example/announce",
            "udp://tracker.opentrackr.org:1337/announce",
            "http://8.8.8.8/announce",
        ] {
            assert!(allowed(ok, false), "{ok}");
        }
        for bad in [
            "http://127.0.0.1:8080/announce",
            "http://localhost/announce",
            "http://router.local/announce",
            "http://192.168.1.1/announce",
            "http://10.0.0.5/a",
            "http://169.254.1.1/a",
            "http://100.64.0.1/a",
            "http://[::1]/a",
            "http://[fd00::1]/a",
            "http://[::ffff:192.168.0.1]/a",
            "file:///etc/passwd",
            "wss://t.example/a",
            "not a url",
        ] {
            assert!(!allowed(bad, false), "{bad}");
        }
        assert!(
            !allowed("udp://tracker.example:1337/a", true),
            "UDP goes around the proxy"
        );
        assert!(allowed("https://t.example/a", true));
    }

    #[test]
    fn magnets_lose_only_the_trackers_dropped() {
        let m = "magnet:?xt=urn:btih:abc&dn=x&tr=udp%3A%2F%2Ft.example%3A1337&tr=http%3A%2F%2F127.0.0.1%2Fa";
        assert_eq!(
            filter_magnet(m, false),
            "magnet:?xt=urn:btih:abc&dn=x&tr=udp%3A%2F%2Ft.example%3A1337"
        );
        assert_eq!(filter_magnet(m, true), "magnet:?xt=urn:btih:abc&dn=x");
    }

    /// The trackers change; the info dictionary, and so the hash, doesn't.
    #[test]
    fn torrents_keep_their_info_byte_for_byte() {
        let info = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let s = |v: &str| format!("{}:{v}", v.len());
        let (local, udp) = ("http://127.0.0.1/announce", "udp://t.example:1337/a");
        let mut t = format!(
            "d8:announce{}13:announce-listll{}el{}ee4:info",
            s(local),
            s(local),
            s(udp)
        )
        .into_bytes();
        t.extend(info);
        t.push(b'e');
        let out = filter_torrent(&t, false).unwrap();
        let mut want = format!("d13:announce-listll{}ee4:info", s(udp)).into_bytes();
        want.extend(info);
        want.push(b'e');
        assert_eq!(out, want);
        // With a proxy nothing is left: no tracker keys at all.
        let mut bare = b"d4:info".to_vec();
        bare.extend(info);
        bare.push(b'e');
        assert_eq!(filter_torrent(&t, true).unwrap(), bare);
        assert_eq!(filter_torrent(b"not bencode", false), None);
        assert_eq!(filter_torrent(b"d4:infoi1e", false), None, "unterminated");
    }
}

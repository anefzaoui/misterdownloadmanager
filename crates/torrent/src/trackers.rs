//! Which of a torrent's trackers MDM lets librqbit contact (#40). A torrent names its trackers,
//! and a tracker on this computer or the local network (`127.0.0.1`, `192.168.1.1`, `localhost`)
//! lets a torrent make MDM send requests into the network it sits in: those are dropped. With a
//! proxy, UDP trackers are dropped too: librqbit sends them around the proxy.
//!
//! The list is changed in the torrent itself (`announce`, `announce-list`, outside the info
//! dictionary, so its hash stays the same) and in a magnet link (`tr=`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
        Some(url::Host::Domain(d)) => named(d),
        Some(url::Host::Ipv4(a)) => public(IpAddr::V4(a)),
        Some(url::Host::Ipv6(a)) => public(IpAddr::V6(a)),
        None => false,
    }
}

/// Names (and the names under them) that never lead to a tracker on the internet.
const NOT_PUBLIC_NAMES: [&str; 6] = [
    "localhost",
    "local",
    "localdomain",
    "home.arpa",
    "internal",
    "lan",
];

/// Whether a host given by name may be contacted. A host may reach this as written, and the
/// system then reads an address in it in any of the forms an address takes: a host given as an
/// address is checked as one, and one written in another form than the usual dotted one
/// (`167772161`, `0x0a000001`, `010.0.0.1`, `127.1`) is refused, as no tracker writes its
/// address so.
fn named(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6.parse::<Ipv6Addr>().is_ok_and(|a| public(IpAddr::V6(a)));
    }
    let host = host.trim_end_matches('.');
    // Plain names only: letters, digits, `-` and `_`, in labels between dots. An escape (`%31`)
    // or any other character could read differently to whatever reads the name next.
    let plain = |label: &str| {
        !label.is_empty()
            && label
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    };
    if !host.split('.').all(plain) {
        return false;
    }
    if let Ok(a) = host.parse::<Ipv4Addr>() {
        return public(IpAddr::V4(a));
    }
    // A name ending in a number is an address in another form (the URL standard's "ends in a
    // number": decimal, or hexadecimal after `0x`).
    let last = host.rsplit('.').next().unwrap_or_default();
    let number = last.bytes().all(|c| c.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .is_some_and(|h| h.bytes().all(|c| c.is_ascii_hexdigit()));
    // A name of one label (`router`) is found nearby, never on the internet.
    let under = |name: &str| {
        host == name
            || host
                .strip_suffix(name)
                .is_some_and(|rest| rest.ends_with('.'))
    };
    !(number || !host.contains('.') || NOT_PUBLIC_NAMES.iter().any(|n| under(n)))
}

/// An address on the internet (not this computer, the local network or a reserved range).
pub(crate) fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            !(a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast()
                || a.is_multicast()
                // 0.0.0.0/8, "this network": 0.0.0.1 reaches the machine's own stack.
                || o[0] == 0
                // 100.64.0.0/10, carrier-grade NAT.
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                // 198.18.0.0/15, the benchmark ranges.
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                // 240.0.0.0/4, reserved (255.255.255.255 is the broadcast, above).
                || o[0] >= 240)
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            // IPv4 inside IPv6 that isn't the mapped form: compatible (::a.b.c.d) and NAT64
            // (64:ff9b::a.b.c.d). Whichever it is, its IPv4 rules decide.
            let embedded = (s[..6] == [0; 6] && !a.is_unspecified() && !a.is_loopback())
                .then_some([s[6], s[7]])
                .or_else(|| {
                    (s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4]).then_some([s[6], s[7]])
                });
            if let Some([hi, lo]) = embedded {
                let v4 = Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
                return public(IpAddr::V4(v4));
            }
            !(a.is_loopback()
                || a.is_unspecified()
                || a.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || a.to_ipv4_mapped().is_some_and(|v4| !public(IpAddr::V4(v4))))
        }
    }
}

/// A magnet link as librqbit reads it (`Magnet::parse`): through the URL parser, which drops
/// tabs, line breaks and the fragment. None when it has no query or can't be read (librqbit
/// refuses it then, with the same parser).
fn magnet_url(magnet: &str) -> Option<url::Url> {
    url::Url::parse(magnet).ok().filter(|u| u.query().is_some())
}

/// A magnet link's trackers (`tr=`), each once, in its order.
pub fn in_magnet(magnet: &str) -> Vec<String> {
    let Some(url) = magnet_url(magnet) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for (k, v) in url.query_pairs() {
        let v = v.trim();
        if k.eq_ignore_ascii_case("tr") && !v.is_empty() && !out.iter().any(|t| t == v) {
            out.push(v.to_owned());
        }
    }
    out
}

/// A magnet link with only the trackers MDM may contact. Each pair is read decoded, its name as
/// well as its value (`%74r=` is a `tr=`); the pairs kept stay as they were written.
pub fn filter_magnet(magnet: &str, proxied: bool) -> String {
    let Some(url) = magnet_url(magnet) else {
        return magnet.to_owned();
    };
    let kept: Vec<&str> = url
        .query()
        .unwrap_or_default()
        .split('&')
        .filter(
            |pair| match url::form_urlencoded::parse(pair.as_bytes()).next() {
                Some((k, v)) if k.eq_ignore_ascii_case("tr") => allowed(&v, proxied),
                _ => true,
            },
        )
        .collect();
    format!("{}?{}", &url[..url::Position::AfterPath], kept.join("&"))
}

/// A .torrent with only the trackers MDM may contact; everything else byte for byte, so its
/// info hash is the same. An `announce` or `announce-list` that can't be read goes as a whole.
/// None when it isn't a bencoded dictionary: the torrent is then refused (`lib.rs`).
pub fn filter_torrent(bytes: &[u8], proxied: bool) -> Option<Vec<u8>> {
    let entries = tracker_entries(bytes)?;
    // An `announce-list` replaces `announce` ([`listed`]): when none of its trackers is kept,
    // `announce` goes too, or the torrent would ask a tracker it never named for its peers.
    let list = entries
        .iter()
        .find(|(k, _)| *k == b"announce-list")
        .map(|e| e.1);
    let replaced = list.and_then(list_of_lists).is_some_and(|t| !t.is_empty());
    let none_kept = list
        .and_then(|v| kept(v, proxied))
        .is_some_and(|t| t.is_empty());
    let mut out = b"d".to_vec();
    for (key, value) in entries {
        match key {
            b"announce" => {
                let ok = string_at(value).is_some_and(|url| may_contact(url, proxied));
                if ok && !(replaced && none_kept) {
                    push_string(&mut out, key);
                    out.extend_from_slice(value);
                }
            }
            b"announce-list" => {
                let tiers = kept(value, proxied).unwrap_or_default();
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

/// An `announce-list`'s tiers with only the trackers MDM may contact, and only the tiers left
/// with one. None when it isn't a list of lists.
fn kept(value: &[u8], proxied: bool) -> Option<Vec<Vec<&[u8]>>> {
    Some(
        list_of_lists(value)?
            .into_iter()
            .map(|tier| {
                tier.into_iter()
                    .filter(|u| may_contact(u, proxied))
                    .collect::<Vec<_>>()
            })
            .filter(|tier| !tier.is_empty())
            .collect(),
    )
}

/// [`allowed`] for a tracker as a torrent stores it: bytes, which must be text.
fn may_contact(url: &[u8], proxied: bool) -> bool {
    std::str::from_utf8(url).is_ok_and(|u| allowed(u, proxied))
}

/// A .torrent's trackers, each once, in its order: `announce-list`'s tiers, or `announce` when
/// it has no list (BEP 12: a list replaces it).
pub fn listed(bytes: &[u8]) -> Vec<String> {
    let Some(entries) = tracker_entries(bytes) else {
        return Vec::new();
    };
    let value = |k: &[u8]| entries.iter().find(|(key, _)| *key == k).map(|e| e.1);
    let urls: Vec<&[u8]> = match value(b"announce-list").and_then(list_of_lists) {
        Some(tiers) if !tiers.is_empty() => tiers.into_iter().flatten().collect(),
        _ => value(b"announce").and_then(string_at).into_iter().collect(),
    };
    let mut out: Vec<String> = Vec::new();
    for u in urls {
        let u = String::from_utf8_lossy(u).trim().to_owned();
        if !u.is_empty() && !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// A .torrent with `more` trackers (each a tier of its own, after the ones it has; those it
/// has already are skipped); everything else byte for byte, so its info hash is the same. One
/// with only `announce` gets a list holding it first; one with none gets `announce` too. None
/// when it isn't a bencoded dictionary.
pub fn add(bytes: &[u8], more: &[String]) -> Option<Vec<u8>> {
    let entries = tracker_entries(bytes)?;
    let have = listed(bytes);
    let mut fresh: Vec<&str> = Vec::new();
    for u in more.iter().map(|u| u.trim()) {
        if !u.is_empty() && !have.iter().any(|h| h == u) && !fresh.contains(&u) {
            fresh.push(u);
        }
    }
    if fresh.is_empty() {
        return Some(bytes.to_vec());
    }
    let mut tiers: Vec<Vec<&[u8]>> = match entries
        .iter()
        .find(|(k, _)| *k == b"announce-list")
        .map(|e| list_of_lists(e.1))
    {
        Some(None) => return None,
        Some(Some(list)) if !list.is_empty() => list,
        _ => have.iter().map(|u| vec![u.as_bytes()]).collect(),
    };
    tiers.extend(fresh.iter().map(|u| vec![u.as_bytes()]));
    let mut list = b"l".to_vec();
    for tier in &tiers {
        list.push(b'l');
        for u in tier {
            push_string(&mut list, u);
        }
        list.push(b'e');
    }
    list.push(b'e');
    let mut out: Vec<(&[u8], Vec<u8>)> = entries
        .into_iter()
        .filter(|(k, _)| *k != b"announce-list")
        .map(|(k, v)| (k, v.to_vec()))
        .collect();
    out.push((b"announce-list", list));
    if !out.iter().any(|(k, _)| *k == b"announce") {
        let mut first = Vec::new();
        push_string(&mut first, fresh[0].as_bytes());
        out.push((b"announce", first));
    }
    // A dictionary's keys are sorted (bencode); the info dictionary's bytes are untouched.
    out.sort_by(|a, b| a.0.cmp(b.0));
    let mut done = b"d".to_vec();
    for (k, v) in out {
        push_string(&mut done, k);
        done.extend_from_slice(&v);
    }
    done.push(b'e');
    Some(done)
}

/// Whether a .torrent is private (BEP 27: `private` is 1 in its info dictionary): its peers come
/// from its own trackers only, so MDM never adds one to it (#1776). One whose info dictionary
/// can't be read, or whose `private` is anything but a plain 0, counts as private; so does one
/// that gives `info` twice and either says so.
pub fn private(bytes: &[u8]) -> bool {
    let Some(entries) = top_level(bytes) else {
        return true;
    };
    let mut infos = entries.iter().filter(|(k, _)| *k == b"info").peekable();
    if infos.peek().is_none() {
        return true;
    }
    infos.any(|(_, info)| match top_level(info) {
        None => true,
        Some(fields) => fields.iter().any(|(k, v)| *k == b"private" && *v != b"i0e"),
    })
}

fn push_string(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(s);
}

/// The top-level dictionary's entries, with `announce` and `announce-list` each where it first
/// appears: a key a dictionary repeats (bencode doesn't allow it, a crafted file can) counts once,
/// so what [`listed`] reads, what [`filter_torrent`] checks and what is written back are the same
/// trackers, and a second list can't slip past the filter. Every other entry stays as it is.
fn tracker_entries(bytes: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    let mut entries = top_level(bytes)?;
    let (mut announce, mut list) = (false, false);
    entries.retain(|(key, _)| {
        let seen = match *key {
            b"announce" => &mut announce,
            b"announce-list" => &mut list,
            _ => return true,
        };
        !std::mem::replace(seen, true)
    });
    Some(entries)
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

/// Where the value at `at` ends; nesting is limited (librqbit 9.0.1's crash was unbounded depth),
/// as deep as librqbit reads (its `DEFAULT_MAX_DEPTH`): a torrent it reads is one read here.
fn skip(b: &[u8], at: usize, depth: usize) -> Option<usize> {
    if depth > 128 {
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

    /// "This" network, the reserved ranges and IPv4 addresses written as IPv6 reach this
    /// machine or go nowhere: no tracker there gets a request.
    #[test]
    fn this_networks_and_reserved_addresses_are_dropped() {
        for bad in [
            "http://0.0.0.1/announce",
            "http://0.255.255.254/announce",
            "http://240.0.0.1/announce",
            "http://192.0.0.170/announce",
            "http://198.18.0.1/announce",
            "http://198.19.255.254/announce",
            "http://[::7f00:1]/announce",
            "http://[64:ff9b::a00:1]/announce",
        ] {
            assert!(!allowed(bad, false), "{bad}");
        }
        // Their public neighbours still pass.
        assert!(allowed("http://192.0.1.1/announce", false));
        assert!(allowed("http://198.20.0.1/announce", false));
        assert!(allowed("http://[2001:4860:4860::8888]/announce", false));
    }

    #[test]
    fn magnets_lose_only_the_trackers_dropped() {
        let m = "magnet:?xt=urn:btih:abc&dn=x&tr=udp%3A%2F%2Ft.example%3A1337&tr=http%3A%2F%2F127.0.0.1%2Fa";
        assert_eq!(
            filter_magnet(m, false),
            "magnet:?xt=urn:btih:abc&dn=x&tr=udp%3A%2F%2Ft.example%3A1337"
        );
        assert_eq!(filter_magnet(m, true), "magnet:?xt=urn:btih:abc&dn=x");
        assert_eq!(
            in_magnet(&format!("{m}&tr=udp%3A%2F%2Ft.example%3A1337&tr=")),
            ["udp://t.example:1337", "http://127.0.0.1/a"]
        );
        assert!(in_magnet("magnet:?xt=urn:btih:abc").is_empty());
    }

    /// Add trackers (#39): new ones after the ones there, each once, keys still sorted, the info
    /// dictionary byte for byte.
    #[test]
    fn trackers_are_added_after_the_ones_there() {
        let info = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let s = |v: &str| format!("{}:{v}", v.len());
        let with = |head: &str| {
            let mut t = format!("d{head}4:info").into_bytes();
            t.extend(info);
            t.push(b'e');
            t
        };
        let (a, b, c) = (
            "http://a.example/announce",
            "udp://b.example:1337/announce",
            "https://c.example/announce",
        );
        let more = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Only `announce`: a list with it first, then the new ones, each a tier.
        let one = with(&format!("8:announce{}", s(a)));
        let out = add(&one, &more(&[b, a, b, " ", c])).unwrap();
        assert_eq!(
            out,
            with(&format!(
                "8:announce{}13:announce-listll{}el{}el{}ee",
                s(a),
                s(a),
                s(b),
                s(c)
            ))
        );
        assert_eq!(listed(&out), [a, b, c]);

        // A list already: kept as it is, the new one after; a comment key stays in its place.
        let listed_one = with(&format!(
            "8:announce{}13:announce-listll{}{}ee7:comment2:hi",
            s(a),
            s(a),
            s(b)
        ));
        let out = add(&listed_one, &more(&[c])).unwrap();
        assert_eq!(
            out,
            with(&format!(
                "8:announce{}13:announce-listll{}{}el{}ee7:comment2:hi",
                s(a),
                s(a),
                s(b),
                s(c)
            ))
        );

        // None at all (a magnet's): `announce` and a list.
        let bare = with("");
        let out = add(&bare, &more(&[b])).unwrap();
        assert_eq!(
            out,
            with(&format!("8:announce{}13:announce-listll{}ee", s(b), s(b)))
        );
        // Nothing new: the same bytes.
        assert_eq!(add(&out, &more(&[b])).unwrap(), out);
        assert_eq!(add(b"junk", &more(&[a])), None);

        // The hash is the same: the info bytes are untouched.
        let at = |t: &[u8]| t.windows(info.len()).position(|w| w == info).is_some();
        assert!(at(&add(&one, &more(&[c])).unwrap()));
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

    /// Filtering never brings a tracker in: an `announce` that a list replaced stays unused.
    #[test]
    fn filtering_only_takes_trackers_away() {
        let s = |v: &str| format!("{}:{v}", v.len());
        let (public, local) = ("http://t.example/announce", "http://127.0.0.1/announce");
        // The list names only trackers MDM won't ask: `announce` (not used) doesn't come back.
        let t = format!("d8:announce{}13:announce-listll{}eee", s(public), s(local));
        let out = filter_torrent(t.as_bytes(), false).unwrap();
        assert_eq!(listed(t.as_bytes()), [local]);
        assert!(listed(&out).is_empty(), "{}", String::from_utf8_lossy(&out));
        assert_eq!(out, b"de");
        // An empty tier is a list all the same.
        let t = format!("d8:announce{}13:announce-listlleee", s(public));
        assert!(listed(&filter_torrent(t.as_bytes(), false).unwrap()).is_empty());
        // With a tracker of the list kept, or no list at all, `announce` stays as it was.
        let t = format!(
            "d8:announce{}13:announce-listll{}{}eee",
            s(public),
            s(local),
            s(public)
        );
        assert_eq!(
            listed(&filter_torrent(t.as_bytes(), false).unwrap()),
            [public]
        );
        let t = format!("d8:announce{}e", s(public));
        assert_eq!(filter_torrent(t.as_bytes(), false).unwrap(), t.as_bytes());
    }

    /// Hosts no tracker on the internet has, each written after `scheme://` and before
    /// `:6969/announce`.
    const REFUSED_HOSTS: [&str; 30] = [
        "10.0.0.1",
        "127.0.0.1",
        "192.168.1.1",
        "172.16.0.1",
        "169.254.169.254",
        "0.0.0.0",
        "10.0.0.1.",
        "167772161",
        "2130706433",
        "0x0a000001",
        "0xa.0.0.1",
        "012.0.0.1",
        "0177.0.0.1",
        "127.1",
        "0",
        "%31%30.0.0.1",
        "[::1]",
        "[fd00::1]",
        "[fe80::1]",
        "[::ffff:10.0.0.1]",
        "[::ffff:a00:1]",
        "[::ffff:7f00:1]",
        "localhost",
        "LOCALHOST.",
        "tracker.localhost",
        "router.local",
        "nas.home.arpa",
        "tracker.internal",
        "nas",
        "router.",
    ];

    /// Trackers as they are really written, which stay.
    const KEPT: [&str; 7] = [
        "udp://tracker.opentrackr.org:1337/announce",
        "udp://open.stealth.si:80/announce",
        "udp://tracker.example.org.:6969/announce",
        "udp://8.8.8.8:6969/announce",
        "udp://[2001:4860:4860::8888]:6969/announce",
        "http://tracker.example.org:80/announce",
        "https://t.example/announce",
    ];

    /// The same host gets the same answer however it is written, for every scheme, with a
    /// proxy and without.
    #[test]
    fn hosts_written_as_addresses_are_checked_as_addresses() {
        for scheme in ["udp", "http", "https"] {
            for host in REFUSED_HOSTS {
                let t = format!("{scheme}://{host}:6969/announce");
                for proxied in [false, true] {
                    assert!(!allowed(&t, proxied), "{t} (proxied: {proxied})");
                }
            }
        }
        // Forms a name never takes.
        for t in [
            "udp://010.0.0.1:6969/announce",
            "udp://0x7f.1:6969/announce",
            "udp://tracker.example.1:6969/announce",
            "udp://t%2eexample:6969/announce",
            "udp://t!example.org:6969/announce",
            "udp://a..example.org:6969/announce",
        ] {
            assert!(!allowed(t, false), "{t}");
        }
        for t in KEPT {
            assert!(allowed(t, false), "{t}");
        }
        // A proxy keeps only what goes through it (http and https), as before.
        for t in KEPT {
            assert_eq!(allowed(t, true), !t.starts_with("udp:"), "{t}");
        }
    }

    /// What librqbit would read from a filtered magnet: only trackers MDM may contact, however
    /// the pair was written.
    #[test]
    fn magnets_keep_only_trackers_mdm_may_contact() {
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let mut m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x".to_owned();
        for host in REFUSED_HOSTS {
            for scheme in ["udp", "http", "https"] {
                m.push_str(&format!(
                    "&tr={}",
                    enc(&format!("{scheme}://{host}:6969/a"))
                ));
            }
        }
        // The pair's name encoded, or cut by a line break the parser drops.
        m.push_str("&%74r=udp%3A%2F%2F10.0.0.1%3A6969&TR=http%3A%2F%2F10.0.0.2%2Fa");
        m.push_str("&t\nr=udp%3A%2F%2F10.0.0.3%3A6969&tr=udp://167772161:6969");
        for t in KEPT {
            m.push_str(&format!("&tr={}", enc(t)));
        }
        let read = |m: &str| librqbit::Magnet::parse(m).unwrap().trackers;
        assert!(read(&m).len() > KEPT.len(), "{m}");
        assert_eq!(read(&filter_magnet(&m, false)), KEPT);
        let through_proxy: Vec<&str> = KEPT
            .into_iter()
            .filter(|t| !t.starts_with("udp:"))
            .collect();
        assert_eq!(read(&filter_magnet(&m, true)), through_proxy);
        assert_eq!(in_magnet(&filter_magnet(&m, false)), KEPT);
    }

    /// What librqbit would read from a filtered .torrent: only trackers MDM may contact, and
    /// the same info hash.
    #[test]
    fn torrents_keep_only_trackers_mdm_may_contact() {
        let info = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let s = |v: &str| format!("{}:{v}", v.len());
        let read = |t: &[u8]| -> Vec<String> {
            librqbit::torrent_from_bytes(t)
                .unwrap()
                .iter_announce()
                .map(|u| String::from_utf8_lossy(u.as_ref()).into_owned())
                .collect()
        };
        let hash = |t: &[u8]| librqbit::torrent_from_bytes(t).unwrap().info_hash;
        let mut refused = Vec::new();
        for host in REFUSED_HOSTS {
            for scheme in ["udp", "http", "https"] {
                refused.push(format!("{scheme}://{host}:6969/a"));
            }
        }
        // Every tracker refused, in a tier of its own and in one with a kept tracker.
        let mut list = String::from("l");
        for r in &refused {
            list.push_str(&format!("l{}e", s(r)));
        }
        for (i, k) in KEPT.iter().enumerate() {
            list.push_str(&format!("l{}{}e", s(&refused[i]), s(k)));
        }
        list.push('e');
        let mut t =
            format!("d8:announce{}13:announce-list{list}4:info", s(&refused[0])).into_bytes();
        t.extend(info);
        t.push(b'e');
        assert_eq!(read(&t).len(), refused.len() + 2 * KEPT.len());
        let out = filter_torrent(&t, false).unwrap();
        assert_eq!(read(&out), KEPT);
        assert_eq!(hash(&out), hash(&t));
        let through_proxy: Vec<&str> = KEPT
            .into_iter()
            .filter(|t| !t.starts_with("udp:"))
            .collect();
        assert_eq!(read(&filter_torrent(&t, true).unwrap()), through_proxy);

        // Only `announce`, refused in each form: nothing left to ask.
        for r in &refused {
            let mut t = format!("d8:announce{}4:info", s(r)).into_bytes();
            t.extend(info);
            t.push(b'e');
            for proxied in [false, true] {
                assert!(
                    read(&filter_torrent(&t, proxied).unwrap()).is_empty(),
                    "{r}"
                );
            }
        }
    }

    /// Tracker entries that can't be read go as a whole; the rest stays byte for byte.
    #[test]
    fn tracker_entries_that_cant_be_read_go() {
        let info = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let with = |head: &[u8]| {
            let mut t = b"d".to_vec();
            t.extend(head);
            t.extend(b"4:info");
            t.extend(info);
            t.push(b'e');
            t
        };
        let s = |v: &[u8]| {
            let mut out = format!("{}:", v.len()).into_bytes();
            out.extend(v);
            out
        };
        let cat = |parts: &[&[u8]]| parts.concat();
        let local = b"udp://10.0.0.1:6969/announce";
        let bare = with(b"");
        for head in [
            cat(&[b"8:announcei1e"]),
            cat(&[b"8:announcele"]),
            cat(&[b"8:announce", &s(b"udp://\xff\xfe.example:6969")]),
            cat(&[b"13:announce-list", &s(local)]),
            cat(&[b"13:announce-listl", &s(local), b"e"]),
            cat(&[b"13:announce-listli1ee"]),
            cat(&[
                b"13:announce-listll",
                &s(b"udp://\xff\xfe.example:6969"),
                b"ee",
            ]),
        ] {
            for proxied in [false, true] {
                assert_eq!(
                    filter_torrent(&with(&head), proxied).as_deref(),
                    Some(&bare[..]),
                    "{}",
                    String::from_utf8_lossy(&head)
                );
            }
        }
        // A list that can't be read takes nothing else with it.
        let keep = cat(&[
            b"8:announce",
            &s(b"udp://tracker.opentrackr.org:1337/announce"),
        ]);
        let t = with(&cat(&[&keep, b"13:announce-listi1e"]));
        assert_eq!(filter_torrent(&t, false).unwrap(), with(&keep));
        // A key given twice is read at its first; the second goes.
        let t = with(&cat(&[b"8:announce", &s(local), &keep]));
        assert_eq!(filter_torrent(&t, false).unwrap(), bare);
        let t = with(&cat(&[&keep, &keep]));
        assert_eq!(filter_torrent(&t, false).unwrap(), with(&keep));
        // As deep as librqbit reads, it is read here too.
        let deep = with(format!("6:nested{}{}", "l".repeat(100), "e".repeat(100)).as_bytes());
        assert!(librqbit::torrent_from_bytes(&deep).is_ok());
        assert_eq!(filter_torrent(&deep, false).unwrap(), deep);
    }

    /// A torrent that repeats `announce-list` or `announce` (fuzzing found one): only the first
    /// counts, for what's listed and what's written, so a second list never comes through the
    /// filter as a tracker the torrent didn't list. The info dictionary stays byte for byte.
    #[test]
    fn a_repeated_tracker_key_counts_once() {
        let info = b"d6:lengthi5e4:name1:a12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let s = |v: &str| format!("{}:{v}", v.len());
        let with = |head: String| {
            let mut t = format!("d{head}4:info").into_bytes();
            t.extend(info);
            t.push(b'e');
            t
        };
        let (public, local, other) = (
            "http://t.example/announce",
            "http://10.0.0.1:69/announce",
            "udp://t2.example:1337/announce",
        );
        let only_info = with(String::new());

        // The crash's shape: the first list names only a local tracker, a second list another.
        let lists = with(format!(
            "8:announce{}13:announce-listll{}ee13:announce-listll{}ee",
            s(public),
            s(local),
            s(other)
        ));
        assert_eq!(listed(&lists), [local]);
        for proxied in [false, true] {
            let out = filter_torrent(&lists, proxied).unwrap();
            assert_eq!(out, only_info, "{}", String::from_utf8_lossy(&out));
        }

        // A first list with one tracker kept: that one, and the second list doesn't come along.
        let kept = with(format!(
            "13:announce-listll{}{}ee13:announce-listll{}ee",
            s(public),
            s(local),
            s(other)
        ));
        let out = filter_torrent(&kept, false).unwrap();
        assert_eq!(out, with(format!("13:announce-listll{}ee", s(public))));
        assert_eq!(listed(&out), [public]);

        // `announce` twice, no list: the first, local, is dropped; the second never counted.
        let announces = with(format!("8:announce{}8:announce{}", s(local), s(public)));
        assert_eq!(listed(&announces), [local]);
        assert_eq!(filter_torrent(&announces, false).unwrap(), only_info);

        // Adding trackers reads the same first list; the second is gone from what's written.
        let more = ["https://c.example/announce".to_owned()];
        let out = add(&kept, &more).unwrap();
        assert_eq!(listed(&out), [public, local, more[0].as_str()]);
        assert_eq!(
            out,
            with(format!(
                "8:announce{}13:announce-listll{}{}el{}ee",
                s(&more[0]),
                s(public),
                s(local),
                s(&more[0])
            ))
        );

        // Whatever is filtered, nothing comes from nowhere.
        for t in [&lists, &kept, &announces] {
            for proxied in [false, true] {
                let out = filter_torrent(t, proxied).unwrap();
                for u in listed(&out) {
                    assert!(listed(t).contains(&u), "{u} came from nowhere");
                }
            }
        }
    }
}

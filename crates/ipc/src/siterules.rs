//! Per-site rules (#1778): settings for every download from the sites a pattern names, kept in
//! the app's settings (Options → Connection → Site rules) and read by the app and the command
//! line alike.
//!
//! A pattern is a server's name, as the per-server exceptions take it (Options → Connection →
//! Exceptions): `example.com`, `dl.example.com`, `*.example.com`, `files*.example.net`; `*`
//! stands for any text, dots included, so `*.example.com` names example.com's subdomains and not
//! example.com itself. A scheme in front (`https://example.com`) limits it to that scheme; a
//! port after the name (`example.com:8080`) to that port; anything after a `/` is ignored. The
//! letters' case never matters.
//!
//! Every setting comes from the first rule, in the list's order, that matches the address and
//! sets it (the first match wins, not the most specific, as for the exceptions): put a narrower
//! rule before a wider one to give it its own value. A rule's number of connections is an
//! exception in Options → Connection (`conn_exceptions`, the list older versions read), under
//! the same pattern: one list for them, so the two can never disagree.

use serde::{Deserialize, Serialize};
use url::Url;

/// One rule: the sites it is for and what their downloads get.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteRule {
    /// The pattern (see the module's notes).
    pub site: String,
    /// The User-Agent sent to these sites; empty: the download's own (the browser's, the one
    /// set in Options, or the automatic one).
    pub user_agent: String,
    /// Ask these sites the way a browser connects from the first request (#668), instead of
    /// only after one refused MDM's own connection.
    pub browser_like: bool,
    /// Request headers added to every request to these sites (name, value), in this order. One
    /// the download already has (a Referer, a Cookie) is replaced.
    pub headers: Vec<(String, String)>,
}

impl SiteRule {
    /// Whether the rule sets anything besides the number of connections (kept as an exception).
    pub fn sets_anything(&self) -> bool {
        !self.user_agent.trim().is_empty() || self.browser_like || !self.headers.is_empty()
    }
}

/// What the rules give a download from one address ([`for_url`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub user_agent: Option<String>,
    pub browser_like: bool,
    /// Each header from the first rule that sets it (names compared without case).
    pub headers: Vec<(String, String)>,
    /// From the first exception that matches.
    pub connections: Option<u32>,
}

/// `pattern` names `url`'s site (see the module's notes).
pub fn matches(url: &Url, pattern: &str) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let pattern = pattern.trim().to_ascii_lowercase();
    let (scheme, rest) = match pattern.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, pattern.as_str()),
    };
    if scheme.is_some_and(|s| s != url.scheme()) {
        return false;
    }
    let server = rest.split(['/', '?', '#']).next().unwrap_or("");
    // A port after the name, outside an IPv6 address's brackets.
    let (name, port) = match server.rsplit_once(':') {
        Some((name, port))
            if !port.is_empty()
                && port.bytes().all(|b| b.is_ascii_digit())
                && (!name.contains(':') || name.ends_with(']')) =>
        {
            (name, port.parse::<u16>().ok())
        }
        _ => (server, None),
    };
    let name = name.trim_end_matches('.');
    // "" names nothing, nor does "**" (a typo of "*", which names every site).
    if name.is_empty() || name.len() > 1 && name.bytes().all(|b| b == b'*') {
        return false;
    }
    if port.is_some_and(|p| Some(p) != url.port_or_known_default()) {
        return false;
    }
    glob(name.as_bytes(), host.as_bytes())
}

/// `*` matches any run of bytes (none included), every other byte itself.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    // Where the last `*` was, and the text position it is matched up to.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((sp, st)) = star {
            // The `*` takes one more byte.
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|b| *b == b'*')
}

/// Two patterns for the same sites, as the lists compare them (a rule and its exception).
pub fn same_pattern(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// What `rules` and `exceptions` (Options → Connection's per-server connections, in their
/// order) give a download from `url`: each setting from the first that matches and sets it.
pub fn for_url(rules: &[SiteRule], exceptions: &[(String, u32)], url: &Url) -> Applied {
    let mut out = Applied {
        connections: exceptions
            .iter()
            .find(|(site, _)| matches(url, site))
            .map(|(_, n)| *n),
        ..Applied::default()
    };
    for rule in rules.iter().filter(|r| matches(url, &r.site)) {
        let agent = rule.user_agent.trim();
        if out.user_agent.is_none() && !agent.is_empty() {
            out.user_agent = Some(agent.to_owned());
        }
        out.browser_like |= rule.browser_like;
        for (name, value) in &rule.headers {
            if !out
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name))
            {
                out.headers.push((name.clone(), value.clone()));
            }
        }
    }
    out
}

/// Headers MDM sets itself for each request, which a rule or a download may not: they say how
/// the request is made (its range, its connection), not what is asked.
pub const RESERVED_HEADERS: [&str; 12] = [
    "host",
    "range",
    "if-range",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "upgrade",
    "expect",
];

/// Why a line of headers was refused ([`parse_headers`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Line `n` (from 1) has no `Name: value` shape.
    NoColon(usize),
    /// Line `n`'s name has a character a header's name can't have.
    BadName(usize),
    /// Line `n`'s value has a control character.
    BadValue(usize),
    /// Line `n` names a header MDM sets itself ([`RESERVED_HEADERS`]).
    Reserved(usize, String),
}

/// Headers typed one a line, `Name: value` (blank lines skipped). A name is a token (RFC 9110:
/// letters, digits and `!#$%&'*+-.^_`|~`); a value has no control characters but tabs.
pub fn parse_headers(text: &str) -> Result<Vec<(String, String)>, HeaderError> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(HeaderError::NoColon(n))?;
        let (name, value) = (name.trim(), value.trim());
        let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
        if name.is_empty() || !name.bytes().all(token) {
            return Err(HeaderError::BadName(n));
        }
        if value.chars().any(|c| c.is_control() && c != '\t') {
            return Err(HeaderError::BadValue(n));
        }
        if RESERVED_HEADERS
            .iter()
            .any(|r| r.eq_ignore_ascii_case(name))
        {
            return Err(HeaderError::Reserved(n, name.to_owned()));
        }
        out.push((name.to_owned(), value.to_owned()));
    }
    Ok(out)
}

/// `headers` one a line, as [`parse_headers`] reads them back.
pub fn headers_text(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(n, v)| format!("{n}: {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn rule(site: &str, agent: &str) -> SiteRule {
        SiteRule {
            site: site.into(),
            user_agent: agent.into(),
            ..SiteRule::default()
        }
    }

    #[test]
    fn a_name_matches_its_own_host_only() {
        let u = url("https://dl.example.com/file.zip");
        assert!(matches(&u, "dl.example.com"));
        assert!(matches(&u, "DL.Example.COM"));
        assert!(matches(&u, " dl.example.com "));
        assert!(matches(&url("https://dl.example.com./f"), "dl.example.com"));
        assert!(matches(&u, "dl.example.com."));
        assert!(!matches(&u, "example.com"));
        assert!(!matches(&u, "l.example.com"));
        assert!(!matches(&u, "dl.example.co"));
        assert!(!matches(&u, ""));
        assert!(!matches(&u, "   "));
    }

    /// `*` is any text, dots included, and `*.example.com` doesn't name example.com itself.
    #[test]
    fn a_star_is_any_text() {
        let sub = url("https://a.b.example.com/x");
        let bare = url("https://example.com/x");
        assert!(matches(&sub, "*.example.com"));
        assert!(!matches(&bare, "*.example.com"));
        assert!(!matches(&url("https://badexample.com/x"), "*.example.com"));
        assert!(matches(&url("https://badexample.com/x"), "*example.com"));
        assert!(matches(
            &url("https://files3.example.net/x"),
            "files*.example.net"
        ));
        assert!(!matches(
            &url("https://files3.example.org/x"),
            "files*.example.net"
        ));
        assert!(matches(&url("http://127.0.0.1:8771/x"), "127.0.0.*"));
        assert!(matches(&sub, "*"));
        assert!(matches(&sub, "*.*.example.com"));
        assert!(!matches(&url("https://b.example.com/x"), "*.*.example.com"));
        assert!(matches(&sub, "a.*.com"));
        assert!(!matches(&sub, "**"), "a typo names nothing");
    }

    #[test]
    fn a_scheme_or_a_port_narrows_it() {
        let http = url("http://example.com:8080/x");
        assert!(matches(&http, "http://example.com"));
        assert!(!matches(&http, "https://example.com"));
        assert!(matches(&http, "example.com:8080"));
        assert!(!matches(&http, "example.com:8081"));
        assert!(matches(&url("https://example.com/x"), "example.com:443"));
        assert!(matches(&http, "http://example.com:8080/any/path?q"));
        assert!(matches(&url("ftp://example.com/x"), "ftp://example.com"));
        assert!(matches(&url("http://[::1]:8080/x"), "[::1]"));
        assert!(matches(&url("http://[::1]:8080/x"), "[::1]:8080"));
        assert!(!matches(&url("http://[::1]:8080/x"), "[::1]:9"));
    }

    /// The first rule that matches and sets a setting gives it, whatever comes after: a wildcard
    /// before a name wins, and of two rules for one pattern, the first.
    #[test]
    fn the_first_match_wins_each_setting() {
        let u = url("https://dl.example.com/f");
        let rules = [
            rule("*.example.com", "Wide/1"),
            rule("dl.example.com", "Narrow/1"),
            rule("*.example.com", "Wide/2"),
        ];
        assert_eq!(
            for_url(&rules, &[], &u).user_agent.as_deref(),
            Some("Wide/1")
        );
        let narrow_first = [rules[1].clone(), rules[0].clone()];
        assert_eq!(
            for_url(&narrow_first, &[], &u).user_agent.as_deref(),
            Some("Narrow/1")
        );
        // A tie: the same pattern twice.
        let tie = [rule("dl.example.com", "A"), rule("dl.example.com", "B")];
        assert_eq!(for_url(&tie, &[], &u).user_agent.as_deref(), Some("A"));
        // A rule that leaves the User-Agent empty doesn't set it: the next one does.
        let empty_first = [
            rule("dl.example.com", "  "),
            rule("*.example.com", "Wide/1"),
        ];
        assert_eq!(
            for_url(&empty_first, &[], &u).user_agent.as_deref(),
            Some("Wide/1")
        );
        assert_eq!(
            for_url(&rules, &[], &url("https://other.org/f")),
            Applied::default()
        );
    }

    #[test]
    fn headers_come_from_the_first_rule_that_sets_each() {
        let u = url("https://dl.example.com/f");
        let rules = [
            SiteRule {
                site: "dl.example.com".into(),
                headers: vec![("X-Token".into(), "narrow".into())],
                ..SiteRule::default()
            },
            SiteRule {
                site: "*.example.com".into(),
                browser_like: true,
                headers: vec![
                    ("x-token".into(), "wide".into()),
                    ("Accept-Language".into(), "de".into()),
                ],
                ..SiteRule::default()
            },
        ];
        let got = for_url(&rules, &[], &u);
        assert_eq!(
            got.headers,
            vec![
                ("X-Token".to_owned(), "narrow".to_owned()),
                ("Accept-Language".to_owned(), "de".to_owned()),
            ]
        );
        assert!(
            got.browser_like,
            "on as soon as one rule that matches says so"
        );
        assert!(!for_url(&rules, &[], &url("https://example.com/f")).browser_like);
    }

    #[test]
    fn connections_come_from_the_first_exception() {
        let u = url("http://127.0.0.1:8771/f");
        let both = [("127.0.0.*".to_owned(), 4), ("127.0.0.1".to_owned(), 2)];
        assert_eq!(for_url(&[], &both, &u).connections, Some(4));
        let swapped = [both[1].clone(), both[0].clone()];
        assert_eq!(for_url(&[], &swapped, &u).connections, Some(2));
        assert_eq!(
            for_url(&[], &[("https://127.0.0.1".to_owned(), 1)], &u).connections,
            None,
            "an https exception isn't an http address's"
        );
    }

    #[test]
    fn headers_are_read_one_a_line() {
        assert_eq!(
            parse_headers("X-Api-Key: abc\n\n  Accept-Language :  de, en  \nX-Empty:"),
            Ok(vec![
                ("X-Api-Key".to_owned(), "abc".to_owned()),
                ("Accept-Language".to_owned(), "de, en".to_owned()),
                ("X-Empty".to_owned(), String::new()),
            ])
        );
        assert_eq!(
            parse_headers("Cookie: a=1; b=2:3"),
            Ok(vec![("Cookie".to_owned(), "a=1; b=2:3".to_owned())]),
            "a colon in the value stays in it"
        );
        assert_eq!(
            parse_headers("X-A: 1\nno colon"),
            Err(HeaderError::NoColon(2))
        );
        assert_eq!(parse_headers("Bad Name: 1"), Err(HeaderError::BadName(1)));
        assert_eq!(parse_headers(": 1"), Err(HeaderError::BadName(1)));
        assert_eq!(parse_headers("X-A: a\u{7}b"), Err(HeaderError::BadValue(1)));
        assert_eq!(
            parse_headers("range: bytes=0-"),
            Err(HeaderError::Reserved(1, "range".into()))
        );
        assert_eq!(
            parse_headers("Host: other.example"),
            Err(HeaderError::Reserved(1, "Host".into()))
        );
        let h = vec![
            ("A".to_owned(), "1".to_owned()),
            ("B".to_owned(), "x: y".to_owned()),
        ];
        assert_eq!(parse_headers(&headers_text(&h)), Ok(h));
    }
}

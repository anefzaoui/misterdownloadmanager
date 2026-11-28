//! A download's cookies (#1540), kept as a browser keeps them while it follows a link: the
//! browser's own (each with the domain and path it has there, so it goes to the hosts it belongs
//! to and no other), and every cookie a server sets on the way, a redirect's too, kept for the
//! hosts its `Domain` and `Path` say and sent there next (a file host's sign-in page that sets
//! its session for the CDN it sends the download to).
//!
//! One jar per download: its connections, its redirects and the look before it
//! ([`crate::download::probe_kept`]) all share it, nothing else does, and it is never saved.
//!
//! - **The Cookie header a download is given** (`Request::headers`, `Options`' callers): its
//!   cookies go where the download's own rules send it (`download::scoped_headers`: its address's
//!   host, one site's own other hosts), as before. On each request they are taken into the jar
//!   for that request's host ([`CookieJar::give`]), never over a cookie of the same name a server
//!   set since, nor bringing back one a server removed; the request then carries the jar's.
//! - **Never from https down to http** (as MDM's credentials never were): once a request of the
//!   download went over HTTPS, no request over plain HTTP carries a cookie at all.
//! - **`Secure`** cookies go over HTTPS only (and to this computer, as browsers send them there).
//! - **`SameSite`**: each request is the user's own download, as typing the address in a browser
//!   is, so `Lax` and `Strict` don't hold a cookie back. A `SameSite=None` cookie without
//!   `Secure` is refused, as browsers refuse it.
//! - A `Domain` of one label (`com`, `test`) is refused unless it is the host itself; there is
//!   no list of public suffixes here, and a cookie only ever reaches the hosts this one
//!   download asks.
//!
//! Cookies are credentials: their values are never logged (this type's `Debug` counts them).

use std::sync::Mutex;

use cookie_store::{CookieStore, RawCookie};
use reqwest::header::{self, HeaderMap, HeaderValue};
use url::Url;

/// Most cookies a jar keeps: a server that sets cookie after cookie can't make it grow without
/// end (browsers keep about 180 a domain).
const MAX_COOKIES: usize = 1000;
/// Longest cookie (name and value) kept, as browsers' 4 KB.
const MAX_COOKIE_LEN: usize = 4096;

/// One download's cookies. Shared (`Arc`) by its clients: reqwest asks it for each request
/// without a `Cookie` header of its own and hands it every answer's `Set-Cookie`.
#[derive(Default)]
pub struct CookieJar(Mutex<Jar>);

#[derive(Default)]
struct Jar {
    store: CookieStore,
    /// A request went over HTTPS: none over plain HTTP carries a cookie from then on.
    https: bool,
    count: usize,
}

impl std::fmt::Debug for CookieJar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the cookies themselves: they are credentials.
        let jar = self.0.lock().unwrap();
        f.debug_struct("CookieJar")
            .field("cookies", &jar.store.iter_unexpired().count())
            .finish()
    }
}

impl CookieJar {
    pub fn new() -> CookieJar {
        CookieJar::default()
    }

    /// One of the browser's cookies, as its extension reads it (`cookies.getAll`): for `domain`
    /// (`.example.com` for it and its subdomains, or a host name) and its subdomains unless
    /// `host_only`, at `path` and below, over HTTPS only when `secure`. One already expired
    /// (`expires`, seconds since 1970), without a name or a domain, or too long is left out.
    /// False when it was.
    #[allow(clippy::too_many_arguments)]
    pub fn add_browser(
        &self,
        name: &str,
        value: &str,
        domain: &str,
        host_only: bool,
        path: &str,
        secure: bool,
        expires: Option<f64>,
    ) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let host = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        if name.is_empty()
            || host.is_empty()
            || name.len() + value.len() > MAX_COOKIE_LEN
            || expires.is_some_and(|e| e <= now)
        {
            return false;
        }
        // The address it is filed from: its own host, which its domain matches.
        let scheme = if secure { "https" } else { "http" };
        let Ok(at) = Url::parse(&format!("{scheme}://{host}/")) else {
            return false;
        };
        let path = if path.starts_with('/') { path } else { "/" };
        let mut raw = RawCookie::build((name.to_owned(), value.to_owned()))
            .path(path.to_owned())
            .secure(secure);
        if !host_only {
            raw = raw.domain(host.clone());
        }
        let raw = raw.build();
        let mut jar = self.0.lock().unwrap();
        jar.insert(&raw, &at)
    }

    /// How many cookies it holds now (unexpired).
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().store.iter_unexpired().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A request is going to `url`: noted for the rule above (none over HTTP after HTTPS).
    pub(crate) fn note(&self, url: &Url) {
        if url.scheme() == "https" {
            self.0.lock().unwrap().https = true;
        }
    }

    /// The download's rules gave `url`'s request this `Cookie` header: each of its cookies is
    /// kept for `url`'s host (all its paths), unless the jar has one of that name for `url`
    /// already (a server's newer value) or a server removed it there.
    pub(crate) fn give(&self, url: &Url, cookie: &HeaderValue) {
        // UTF-8, as a browser may send a cookie's value (`to_str` takes visible ASCII only).
        let Ok(text) = std::str::from_utf8(cookie.as_bytes()) else {
            return;
        };
        let mut jar = self.0.lock().unwrap();
        if url.scheme() == "https" {
            jar.https = true;
        } else if jar.https {
            return;
        }
        let Ok(at) = url.join("/") else { return };
        for pair in text.split(';') {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() || name.len() + value.len() > MAX_COOKIE_LEN {
                continue;
            }
            let known = jar
                .store
                .iter_any()
                .any(|c| c.name() == name && c.domain.matches(url));
            if known {
                continue;
            }
            let raw = RawCookie::build((name.to_owned(), value.to_owned()))
                .path("/")
                .secure(url.scheme() == "https")
                .build();
            jar.insert(&raw, &at);
        }
    }

    /// The `Cookie` header a request to `url` carries: the jar's cookies for it, none over HTTP
    /// once the download went over HTTPS.
    pub(crate) fn header_for(&self, url: &Url) -> Option<HeaderValue> {
        let mut jar = self.0.lock().unwrap();
        match url.scheme() {
            "https" => jar.https = true,
            "http" if jar.https => return None,
            "http" => {}
            _ => return None,
        }
        // Longer paths first, as browsers send them (then by name: always in the same order).
        let mut found = jar.store.matches(url);
        found.sort_by(|a, b| {
            let (pa, pb): (&str, &str) = (a.path.as_ref(), b.path.as_ref());
            pb.len().cmp(&pa.len()).then_with(|| a.name().cmp(b.name()))
        });
        let pairs: Vec<String> = found
            .iter()
            .map(|c| format!("{}={}", c.name(), c.value()))
            .collect();
        if pairs.is_empty() {
            return None;
        }
        HeaderValue::from_str(&pairs.join("; ")).ok()
    }

    /// An answer from `url` arrived: the cookies its `Set-Cookie` headers set (or remove) are
    /// kept by their rules.
    pub(crate) fn take(&self, answer: &HeaderMap, url: &Url) {
        let mut jar = self.0.lock().unwrap();
        for v in answer.get_all(header::SET_COOKIE) {
            let Ok(text) = std::str::from_utf8(v.as_bytes()) else {
                continue;
            };
            let Ok(raw) = RawCookie::parse(text.to_owned()) else {
                continue;
            };
            if raw.name().len() + raw.value().len() > MAX_COOKIE_LEN {
                continue;
            }
            // Browsers refuse SameSite=None without Secure.
            if raw.same_site().is_some_and(|s| s.is_none()) && raw.secure() != Some(true) {
                continue;
            }
            // A Domain of a single label is the host's own or nothing (`com`, `test`).
            if let Some(d) = raw.domain().map(|d| d.trim_start_matches('.'))
                && !d.contains('.')
                && !url.host_str().is_some_and(|h| h.eq_ignore_ascii_case(d))
            {
                continue;
            }
            jar.insert(&raw, url);
        }
    }

    /// A request about to go to `url` with `headers`: the `Cookie` they carry is given to the
    /// jar ([`CookieJar::give`]) and taken out of them; the jar's own header for `url` goes in its
    /// place when `put` (a client that doesn't ask the jar itself: the browser-like connection).
    pub(crate) fn ready(&self, url: &Url, headers: &mut HeaderMap, put: bool) {
        self.note(url);
        if let Some(given) = headers.remove(header::COOKIE) {
            self.give(url, &given);
        }
        if put && let Some(h) = self.header_for(url) {
            headers.insert(header::COOKIE, h);
        }
    }
}

impl Jar {
    /// Filed by the store's rules (domain, path, expiry, removal): true if kept.
    fn insert(&mut self, raw: &RawCookie<'_>, at: &Url) -> bool {
        if self.count >= MAX_COOKIES {
            return false;
        }
        match self.store.insert_raw(raw, at) {
            Ok(cookie_store::StoreAction::Inserted) => {
                self.count += 1;
                true
            }
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

impl reqwest::cookie::CookieStore for CookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let mut answer = HeaderMap::new();
        for v in cookie_headers {
            answer.append(header::SET_COOKIE, v.clone());
        }
        self.take(&answer, url);
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        self.header_for(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn sent(jar: &CookieJar, url: &str) -> String {
        jar.header_for(&u(url))
            .map(|h| h.to_str().unwrap().to_owned())
            .unwrap_or_default()
    }

    fn set(lines: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for l in lines {
            h.append(header::SET_COOKIE, HeaderValue::from_str(l).unwrap());
        }
        h
    }

    #[test]
    fn a_browser_cookie_goes_to_its_own_hosts_only() {
        let jar = CookieJar::new();
        assert!(jar.add_browser("tok", "c", "files.cdn.test", true, "/", false, None));
        assert!(jar.add_browser("all", "1", ".cdn.test", false, "/", false, None));
        assert!(jar.add_browser("deep", "1", "files.cdn.test", true, "/f/", false, None));
        assert_eq!(sent(&jar, "http://files.cdn.test/a.bin"), "all=1; tok=c");
        assert_eq!(
            sent(&jar, "http://files.cdn.test/f/a.bin"),
            "deep=1; all=1; tok=c"
        );
        assert_eq!(
            sent(&jar, "http://other.cdn.test/a"),
            "all=1",
            "host-only stays"
        );
        assert_eq!(sent(&jar, "http://cdn.test.evil/a"), "");
        assert_eq!(sent(&jar, "http://site.test/a"), "");
    }

    #[test]
    fn expired_empty_and_oversized_browser_cookies_are_left_out() {
        let jar = CookieJar::new();
        assert!(!jar.add_browser("old", "1", "x.test", true, "/", false, Some(1.0)));
        assert!(!jar.add_browser("", "1", "x.test", true, "/", false, None));
        assert!(!jar.add_browser("a", "1", "", true, "/", false, None));
        let big = "v".repeat(MAX_COOKIE_LEN);
        assert!(!jar.add_browser("big", &big, "x.test", true, "/", false, None));
        assert!(jar.add_browser("ok", "1", "x.test", true, "", false, Some(4e9)));
        assert_eq!(sent(&jar, "http://x.test/any/where"), "ok=1");
        assert_eq!(jar.len(), 1);
    }

    #[test]
    fn a_redirects_cookie_goes_on_to_the_hosts_its_domain_says() {
        let jar = CookieJar::new();
        jar.take(
            &set(&[
                "gate=open; Domain=cdn.test; Path=/",
                "own=1; Path=/",
                "far=1; Domain=other.test",
            ]),
            &u("http://login.cdn.test/jump"),
        );
        assert_eq!(sent(&jar, "http://files.cdn.test/f.bin"), "gate=open");
        assert_eq!(sent(&jar, "http://login.cdn.test/x"), "gate=open; own=1");
        assert_eq!(
            sent(&jar, "http://other.test/"),
            "",
            "another site's domain is refused"
        );
    }

    #[test]
    fn a_single_label_domain_and_samesite_none_without_secure_are_refused() {
        let jar = CookieJar::new();
        jar.take(
            &set(&[
                "wide=1; Domain=test",
                "loose=1; SameSite=None",
                "strict=1; SameSite=Strict",
                "fine=1; SameSite=None; Secure",
            ]),
            &u("https://a.test/"),
        );
        assert_eq!(sent(&jar, "https://a.test/"), "fine=1; strict=1");
        assert_eq!(sent(&jar, "https://b.test/"), "");
        // The host's own name as its domain is the host's.
        let jar = CookieJar::new();
        jar.take(&set(&["me=1; Domain=localhost"]), &u("http://localhost:8/"));
        assert_eq!(sent(&jar, "http://localhost:9/"), "me=1");
    }

    #[test]
    fn secure_cookies_go_over_https_only() {
        let jar = CookieJar::new();
        jar.add_browser("s", "1", "x.test", true, "/", true, None);
        jar.add_browser("p", "1", "x.test", true, "/", false, None);
        assert_eq!(sent(&jar, "http://x.test/"), "p=1");
        let jar2 = CookieJar::new();
        jar2.add_browser("s", "1", "x.test", true, "/", true, None);
        jar2.add_browser("p", "1", "x.test", true, "/", false, None);
        assert_eq!(sent(&jar2, "https://x.test/"), "p=1; s=1");
    }

    #[test]
    fn nothing_goes_over_http_once_the_download_went_over_https() {
        let jar = CookieJar::new();
        jar.add_browser("p", "1", "x.test", true, "/", false, None);
        jar.add_browser("q", "1", "cdn.test", true, "/", false, None);
        assert_eq!(
            sent(&jar, "http://x.test/"),
            "p=1",
            "plain http from the start"
        );
        assert_eq!(sent(&jar, "https://x.test/"), "p=1");
        assert_eq!(sent(&jar, "http://x.test/"), "", "down from https");
        assert_eq!(sent(&jar, "http://cdn.test/"), "", "to another host too");
        // A given header isn't taken for http then either.
        jar.give(&u("http://new.test/"), &HeaderValue::from_static("n=1"));
        assert_eq!(jar.len(), 2);
        // Noting an https request is enough (a request whose own header the jar wasn't asked for).
        let jar = CookieJar::new();
        jar.add_browser("p", "1", "x.test", true, "/", false, None);
        jar.note(&u("https://a.test/"));
        assert_eq!(sent(&jar, "http://x.test/"), "");
    }

    #[test]
    fn a_given_header_never_overrides_or_brings_back_a_servers_cookie() {
        let jar = CookieJar::new();
        jar.give(
            &u("http://a.test/get?id=1"),
            &HeaderValue::from_static("sid=old; theme=dark"),
        );
        assert_eq!(sent(&jar, "http://a.test/other"), "sid=old; theme=dark");
        assert_eq!(sent(&jar, "http://b.test/"), "", "for its own host only");
        // The server rotates the session and removes the theme.
        jar.take(
            &set(&["sid=new; Path=/", "theme=; Path=/; Max-Age=0"]),
            &u("http://a.test/get?id=1"),
        );
        jar.give(
            &u("http://a.test/file"),
            &HeaderValue::from_static("sid=old; theme=dark; more=1"),
        );
        assert_eq!(sent(&jar, "http://a.test/file"), "more=1; sid=new");
    }

    #[test]
    fn a_given_cookie_in_utf8_goes_as_it_came() {
        let jar = CookieJar::new();
        let h = HeaderValue::from_bytes("name=Jos\u{e9}; a=1".as_bytes()).unwrap();
        jar.give(&u("http://a.test/"), &h);
        let sent = jar.header_for(&u("http://a.test/")).unwrap();
        assert_eq!(sent.as_bytes(), "a=1; name=Jos\u{e9}".as_bytes());
    }

    #[test]
    fn ready_takes_the_header_and_puts_the_jars() {
        let jar = CookieJar::new();
        jar.add_browser("c", "1", "a.test", true, "/", false, None);
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("g=2"));
        jar.ready(&u("http://a.test/"), &mut h, false);
        assert!(!h.contains_key(header::COOKIE), "reqwest asks the jar");
        h.insert(header::COOKIE, HeaderValue::from_static("g=2"));
        jar.ready(&u("http://a.test/"), &mut h, true);
        assert_eq!(h.get(header::COOKIE).unwrap(), "c=1; g=2");
    }

    #[test]
    fn its_debug_never_shows_a_value() {
        let jar = CookieJar::new();
        jar.add_browser("sid", "s3cr3t", "a.test", true, "/", false, None);
        let shown = format!("{jar:?}");
        assert!(
            !shown.contains("s3cr3t") && !shown.contains("sid"),
            "{shown}"
        );
    }

    #[test]
    fn it_keeps_at_most_so_many() {
        let jar = CookieJar::new();
        for i in 0..MAX_COOKIES + 5 {
            jar.take(&set(&[&format!("c{i}=1; Path=/")]), &u("http://a.test/"));
        }
        assert_eq!(jar.len(), MAX_COOKIES);
    }
}

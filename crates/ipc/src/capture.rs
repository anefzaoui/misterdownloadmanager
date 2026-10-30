//! What the browser extension may hand to Mister Download Manager: Options → General ("Use
//! advanced browser integration", the browsers to capture from) and Options → File types (the
//! types to take, the sites and addresses to leave alone). One implementation for the app and
//! the native messaging host: the host reads the app's saved settings at every hand-off (so a
//! change applies at once) and answers "not taken" for the rest, and the extension then lets the
//! browser download it (extension/background.js).

use serde::Deserialize;

/// Options → File types defaults (IDM 6.42).
pub const DEFAULT_AUTO_TYPES: &str = "3GP 7Z AAC ACE AIF APK ARJ ASF AVI BIN BZ2 EXE GZ GZIP IMG ISO LZH M4A M4V MKV MOV MP3 MP4 MPA MPE MPEG MPG MSI MSU OGG OGV PDF PLJ PPS PPT QT R0* R1* RA RAR RM RMVB SEA SIT SITX TAR TIF TIFF WAV WMA WMV Z ZIP";
pub const DEFAULT_EXCLUDED_SITES: &str = "*.update.microsoft.com download.windowsupdate.com *.download.windowsupdate.com siteseal.thawte.com ecom.cimetz.com *.voice2page.com";

/// The capture settings, named as the app saves them (`Settings` in downloads.json).
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CaptureRules {
    pub advanced_integration: bool,
    /// (browser name as Options lists it, capture from it?)
    pub browsers: Vec<(String, bool)>,
    pub auto_types: String,
    pub excluded_sites: String,
    pub excluded_addresses: Vec<String>,
    /// Options → General → Download panels: the download button over videos in the browsers.
    pub video_panel: bool,
    /// The sites it stays off (space-separated, `*.example.com` takes subdomains too).
    pub video_panel_sites: String,
}

impl Default for CaptureRules {
    fn default() -> Self {
        CaptureRules {
            advanced_integration: true,
            browsers: Vec::new(),
            auto_types: DEFAULT_AUTO_TYPES.into(),
            excluded_sites: DEFAULT_EXCLUDED_SITES.into(),
            excluded_addresses: Vec::new(),
            video_panel: true,
            video_panel_sites: String::new(),
        }
    }
}

/// Why a download is left to the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declined {
    IntegrationOff,
    Browser,
    FileType,
    Site,
    Address,
}

impl CaptureRules {
    /// The rules as the app last saved them (defaults when there is no list yet).
    pub fn load() -> CaptureRules {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Stored {
            settings: CaptureRules,
        }
        crate::data_dir()
            .and_then(|d| std::fs::read(d.join("downloads.json")).ok())
            .and_then(|b| serde_json::from_slice::<Stored>(&b).ok())
            .map(|s| s.settings)
            .unwrap_or_default()
    }

    /// Does the file name's extension appear in File types (`R0*` patterns too)?
    pub fn auto_type(&self, name: &str) -> bool {
        type_listed(&self.auto_types, name)
    }

    /// May the browser hand `url` (saved as `file_name`) over? `user_agent` tells which browser.
    pub fn accepts(&self, url: &str, file_name: &str, user_agent: &str) -> Result<(), Declined> {
        if !self.advanced_integration {
            return Err(Declined::IntegrationOff);
        }
        let browser = browser_of(user_agent);
        if self.browsers.iter().any(|(b, on)| b == browser && !on) {
            return Err(Declined::Browser);
        }
        if !self.auto_type(file_name) {
            return Err(Declined::FileType);
        }
        let host = host_of(url);
        if self
            .excluded_sites
            .split_whitespace()
            .any(|p| host_matches(&host, p))
        {
            return Err(Declined::Site);
        }
        if self
            .excluded_addresses
            .iter()
            .any(|p| wildcard(&p.to_ascii_lowercase(), &url.to_ascii_lowercase()))
        {
            return Err(Declined::Address);
        }
        Ok(())
    }
}

impl CaptureRules {
    /// May the download button show over videos on `page` (its address) in the browser of
    /// `user_agent`? Integration on, that browser captured, the button on and the site not one
    /// it stays off.
    pub fn panel_on(&self, page: &str, user_agent: &str) -> bool {
        let browser = browser_of(user_agent);
        self.advanced_integration
            && self.video_panel
            && !self.browsers.iter().any(|(b, on)| b == browser && !on)
            && !self.panel_hidden_on(page)
    }

    /// Is `page` (an address) on a site the button stays off?
    pub fn panel_hidden_on(&self, page: &str) -> bool {
        let host = host_of(page);
        self.video_panel_sites
            .split_whitespace()
            .any(|p| host_matches(&host, p))
    }
}

/// `sites` (space-separated) with `host` added, unless a pattern already covers it.
pub fn add_site(sites: &str, host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() || sites.split_whitespace().any(|p| host_matches(&host, p)) {
        return sites.to_owned();
    }
    let mut out = sites.trim().to_owned();
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(&host);
    out
}

/// Does `name`'s extension appear in `types` (Options → File types: `EXE ZIP R0*`)?
pub fn type_listed(types: &str, name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_uppercase();
    types.split_whitespace().any(|t| {
        let t = t.to_ascii_uppercase();
        match t.strip_suffix('*') {
            Some(prefix) => ext.starts_with(prefix),
            None => ext == t,
        }
    })
}

/// The browser a User-Agent belongs to, by the names Options lists. Brave says it is Chrome.
pub fn browser_of(user_agent: &str) -> &'static str {
    if user_agent.contains("Edg/") {
        "Microsoft Edge"
    } else if user_agent.contains("OPR/") || user_agent.contains("Opera") {
        "Opera"
    } else if user_agent.contains("Firefox/") {
        "Mozilla Firefox"
    } else if user_agent.contains("Chrome/") {
        "Google Chrome"
    } else if user_agent.contains("Safari/") {
        "Apple Safari"
    } else {
        ""
    }
}

/// The host of an http(s) address, lower case.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split(']').next().map_or(host, |h| &h[1..])
    } else {
        host.split(':').next().unwrap_or(host)
    };
    host.to_ascii_lowercase()
}

/// `host` is `pattern` (case-insensitive) or, for `*.example.com`, one of its subdomains.
pub fn host_matches(host: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    let pattern = pattern
        .split_once("://")
        .map_or(pattern.as_str(), |(_, r)| r);
    let pattern = pattern.split('/').next().unwrap_or("");
    match pattern.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => !pattern.is_empty() && host == pattern,
    }
}

/// `*` matches any run of characters (IDM's address patterns).
fn wildcard(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
    const EDGE: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
    const FIREFOX: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:145.0) Gecko/20100101 Firefox/145.0";

    #[test]
    fn the_video_button_follows_its_options() {
        let mut r = CaptureRules {
            video_panel_sites: "*.example.com tube.test".into(),
            ..CaptureRules::default()
        };
        assert!(r.panel_on("https://videos.site/watch?v=1", CHROME));
        assert!(
            !r.panel_on("https://www.example.com/v", CHROME),
            "a listed site"
        );
        assert!(
            !r.panel_on("https://example.com/v", CHROME),
            "*. takes the domain too"
        );
        assert!(!r.panel_on("https://TUBE.test/v", FIREFOX));
        r.browsers = vec![("Mozilla Firefox".into(), false)];
        assert!(
            !r.panel_on("https://videos.site/", FIREFOX),
            "a browser not captured"
        );
        assert!(r.panel_on("https://videos.site/", CHROME));
        r.video_panel = false;
        assert!(!r.panel_on("https://videos.site/", CHROME));
        r.video_panel = true;
        r.advanced_integration = false;
        assert!(!r.panel_on("https://videos.site/", CHROME));
    }

    #[test]
    fn a_site_is_added_once() {
        assert_eq!(add_site("", "Video.Site"), "video.site");
        assert_eq!(add_site("a.test", "b.test"), "a.test b.test");
        assert_eq!(
            add_site("*.b.test", "www.b.test"),
            "*.b.test",
            "already covered"
        );
        assert_eq!(add_site("a.test", " "), "a.test");
    }

    #[test]
    fn the_defaults_take_listed_types_from_any_browser() {
        let r = CaptureRules::default();
        assert_eq!(
            r.accepts("https://example.com/a/setup.exe", "setup.exe", CHROME),
            Ok(())
        );
        assert_eq!(
            r.accepts(
                "https://example.com/movie.part.r01",
                "movie.part.r01",
                FIREFOX
            ),
            Ok(())
        );
        assert_eq!(
            r.accepts("https://example.com/page.html", "page.html", CHROME),
            Err(Declined::FileType)
        );
    }

    #[test]
    fn excluded_sites_addresses_and_browsers_are_left_to_the_browser() {
        let mut r = CaptureRules::default();
        assert_eq!(
            r.accepts("https://dl.update.microsoft.com/x.exe", "x.exe", CHROME),
            Err(Declined::Site)
        );
        r.excluded_addresses = vec!["https://example.com/private/*".into()];
        assert_eq!(
            r.accepts("https://example.com/private/a.zip", "a.zip", CHROME),
            Err(Declined::Address)
        );
        assert_eq!(
            r.accepts("https://example.com/public/a.zip", "a.zip", CHROME),
            Ok(())
        );
        r.browsers = vec![
            ("Microsoft Edge".into(), false),
            ("Google Chrome".into(), true),
        ];
        assert_eq!(
            r.accepts("https://example.com/a.zip", "a.zip", EDGE),
            Err(Declined::Browser)
        );
        assert_eq!(
            r.accepts("https://example.com/a.zip", "a.zip", CHROME),
            Ok(())
        );
        r.advanced_integration = false;
        assert_eq!(
            r.accepts("https://example.com/a.zip", "a.zip", CHROME),
            Err(Declined::IntegrationOff)
        );
    }

    #[test]
    fn hosts_and_patterns() {
        assert_eq!(
            host_of("https://user:pw@Example.COM:8080/x?y"),
            "example.com"
        );
        assert_eq!(host_of("http://[::1]:80/"), "::1");
        assert!(host_matches("a.b.example.com", "*.example.com"));
        assert!(host_matches("example.com", "*.example.com"));
        assert!(!host_matches("badexample.com", "*.example.com"));
        assert!(wildcard("*/files/*.zip", "https://x.org/files/a.zip"));
        assert!(!wildcard("*/files/*.zip", "https://x.org/files/a.rar"));
    }
}

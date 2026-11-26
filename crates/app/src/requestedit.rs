//! Request settings (#1778): a User-Agent, request headers and a number of connections, edited
//! the same way for a site rule (Options → Connection → Site rules..., with "always the
//! browser-like connection") and for one download (its Properties → Connection..., with a
//! proxy), and applied to the engine's request.

use egui::{Pos2, Ui, vec2};
use mdm_ipc::siterules::{self, HeaderError};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::model::OwnRequest;
use crate::winui::{self, LIGHT_DISABLED, LIGHT_TEXT, r};

/// Headers a site rule or a download's own settings add (#1778): each replaces the one of its
/// name. A User-Agent among them goes to `agent` instead (the engine sends the download's
/// User-Agent on each of its connections, the browser-like one too). One the engine couldn't send
/// (a hand-edited file) is left out.
pub fn add_headers(headers: &mut HeaderMap, agent: &mut Option<String>, list: &[(String, String)]) {
    for (name, value) in list {
        if name.eq_ignore_ascii_case("user-agent") {
            *agent = Some(value.clone());
            continue;
        }
        if siterules::RESERVED_HEADERS
            .iter()
            .any(|r| r.eq_ignore_ascii_case(name))
        {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(n, v);
        }
    }
}

/// `agent` chosen by the user (a site rule's, a download's own): sent as it is, never another
/// in its place (no fallback to MDM's former or own name when a site refuses it).
pub fn choose_agent(opts: &mut mdm_engine::Options, agent: String) {
    opts.speak_as(agent);
    opts.agent_fallback = None;
    opts.own_agent = None;
}

/// A proxy's address as typed (`host:port`, `http://…`, `socks5://…`) with its scheme, or None
/// when it isn't one the engine takes.
pub fn proxy_url(typed: &str) -> Option<String> {
    let typed = typed.trim().trim_end_matches('/');
    if typed.is_empty() {
        return None;
    }
    let full = if typed.contains("://") {
        typed.to_owned()
    } else {
        format!("http://{typed}")
    };
    let url = url::Url::parse(&full).ok()?;
    let schemes = ["http", "https", "socks5", "socks5h", "socks4", "socks4a"];
    (schemes.contains(&url.scheme())
        && url.host_str().is_some_and(|h| !h.is_empty())
        && matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.username().is_empty())
    .then(|| full.trim_end_matches('/').to_owned())
}

/// A download's own settings (`Item::own`) on top of what Options and the site rules gave it.
pub fn apply_own(
    own: &OwnRequest,
    headers: &mut HeaderMap,
    opts: &mut mdm_engine::Options,
    limit: u32,
) {
    let mut agent = None;
    add_headers(headers, &mut agent, &own.headers);
    if let Some(agent) = own.user_agent.clone().or(agent) {
        choose_agent(opts, agent);
    }
    if let Some(n) = own.connections {
        opts.max_connections = n.clamp(1, limit);
    }
    match own.proxy.as_deref() {
        None => {}
        Some("") => opts.proxy = mdm_engine::Proxy::None,
        Some(typed) => {
            if let Some(url) = proxy_url(typed) {
                opts.proxy = mdm_engine::Proxy::Manual {
                    url,
                    login: None,
                    http: true,
                    https: true,
                };
            }
        }
    }
}

/// What the fields are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum For {
    /// A site rule: with "always the browser-like connection".
    Rule,
    /// One download: with its proxy.
    Download,
}

/// The proxy choices of a download's settings, in the list's order.
const PROXY_CHOICES: [&str; 3] = [
    // i18n: a download's Connection settings → Proxy: the proxy set in Options → Proxy / Socks
    crate::i18n::mark("As in Options"),
    // i18n: a download's Connection settings → Proxy: straight to the server, no proxy
    crate::i18n::mark("No proxy"),
    // i18n: a download's Connection settings → Proxy: the proxy typed in the box beside
    crate::i18n::mark("This proxy:"),
];

/// The fields as typed: a site rule's or a download's ([`For`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestEdit {
    pub what: For,
    pub agent: String,
    pub headers: String,
    pub own_conns: bool,
    pub conns: String,
    pub browser_like: bool,
    /// [`PROXY_CHOICES`]'s index.
    pub proxy: usize,
    pub proxy_addr: String,
    /// Why OK can't take them, shown under the fields.
    pub error: Option<String>,
}

/// What OK takes from the fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edited {
    pub user_agent: Option<String>,
    pub headers: Vec<(String, String)>,
    pub connections: Option<u32>,
    pub browser_like: bool,
    /// As `OwnRequest::proxy`.
    pub proxy: Option<String>,
}

impl RequestEdit {
    /// A site rule's fields: its own settings and its number of connections (its exception).
    pub fn rule(rule: &siterules::SiteRule, connections: Option<u32>, default: u32) -> Self {
        RequestEdit {
            what: For::Rule,
            agent: rule.user_agent.clone(),
            headers: siterules::headers_text(&rule.headers),
            own_conns: connections.is_some(),
            conns: connections.unwrap_or(default).to_string(),
            browser_like: rule.browser_like,
            proxy: 0,
            proxy_addr: String::new(),
            error: None,
        }
    }

    /// A download's fields, from its own settings; its connections box starts at what it gets
    /// now (`current`).
    pub fn download(own: &OwnRequest, current: u32) -> Self {
        let (proxy, proxy_addr) = match own.proxy.as_deref() {
            None => (0, String::new()),
            Some("") => (1, String::new()),
            Some(addr) => (2, addr.to_owned()),
        };
        RequestEdit {
            what: For::Download,
            agent: own.user_agent.clone().unwrap_or_default(),
            headers: siterules::headers_text(&own.headers),
            own_conns: own.connections.is_some(),
            conns: own.connections.unwrap_or(current).to_string(),
            browser_like: false,
            proxy,
            proxy_addr,
            error: None,
        }
    }

    /// The fields read, or why not (translated).
    pub fn read(&self, limit: u32) -> Result<Edited, String> {
        let headers = siterules::parse_headers(&self.headers).map_err(|e| header_error(&e))?;
        let connections = if self.own_conns {
            match self.conns.trim().parse::<u32>() {
                Ok(n) if (1..=limit).contains(&n) => Some(n),
                _ => {
                    return Err(crate::i18n::fmt(
                        // i18n: Site rule / a download's Connection settings: the number of connections typed is out of range; {max} is 32 (or 128)
                        crate::i18n::t("The number of connections must be from 1 to {max}"),
                        &[("max", &crate::i18n::integer(u64::from(limit)))],
                    ));
                }
            }
        } else {
            None
        };
        let proxy = match (self.what, self.proxy) {
            (For::Download, 1) => Some(String::new()),
            (For::Download, 2) => match proxy_url(&self.proxy_addr) {
                Some(_) => Some(self.proxy_addr.trim().to_owned()),
                None => {
                    return Err(crate::i18n::t(
                        // i18n: a download's Connection settings: the proxy typed isn't an address MDM takes; the examples stay as they are
                        "Type the proxy as host:port, http://host:port or socks5://host:port",
                    )
                    .to_owned());
                }
            },
            _ => None,
        };
        let agent = self.agent.trim();
        if agent.chars().any(char::is_control) {
            return Err(header_error(&HeaderError::BadValue(0)));
        }
        Ok(Edited {
            user_agent: (!agent.is_empty()).then(|| agent.to_owned()),
            headers,
            connections,
            browser_like: self.what == For::Rule && self.browser_like,
            proxy,
        })
    }

    /// The fields from English y `y0` down, in a window of English width `w`: User-Agent,
    /// headers, connections, then the browser-like connection (a rule) or the proxy (a
    /// download). Returns the English y under the last one. `enabled` false shows them read
    /// only (a running download). `limit`: the most connections Options allows.
    pub fn fields(
        &mut self,
        ui: &mut Ui,
        o: Pos2,
        y0: f32,
        w: f32,
        enabled: bool,
        limit: u32,
    ) -> f32 {
        let p = ui.painter().clone();
        let ink = if enabled {
            LIGHT_TEXT()
        } else {
            LIGHT_DISABLED()
        };
        let labels = [
            // i18n: Site rule / a download's Connection settings: label before the User-Agent box (the name the browser or program gives itself to sites)
            crate::i18n::mark("User-Agent:"),
            // i18n: Site rule / a download's Connection settings: label before the proxy choice
            crate::i18n::mark("Proxy:"),
        ];
        let col = labels
            .iter()
            .map(|l| 14.0 + winui::text_width(&p, crate::i18n::t(l)).ceil() + 8.0)
            .fold(100.0_f32, f32::max)
            .min(w * 0.45);
        let right = w - 14.0;
        let mut y = y0;
        winui::text_right(
            &p,
            o.x + col - 8.0,
            o.y + y + 15.0,
            crate::i18n::t(labels[0]),
            ink,
        );
        let agent = winui::edit_light(
            ui,
            r(o, col, y, right - col, 23.0),
            &mut self.agent,
            enabled,
            false,
        );
        winui::name(&agent, crate::i18n::t(labels[0]));
        if agent.changed() {
            self.error = None;
        }
        y += 27.0;
        winui::fit(
            ui,
            &p,
            o.x + col,
            o.y + y + 10.0,
            crate::i18n::t(
                // i18n: Site rule / a download's Connection settings: the hint under the User-Agent box
                "Empty: the browser's, or the one set in Options",
            ),
            LIGHT_DISABLED(),
            o.x + right,
            ("request-agent-hint", y0 as i32),
        );
        y += 22.0;
        // i18n: Site rule / a download's Connection settings: label above the box of request headers, one a line ("X-Token: abc")
        let headers_label = crate::i18n::t("Request headers, one a line (Name: value):");
        winui::fit(
            ui,
            &p,
            o.x + 14.0,
            o.y + y + 10.0,
            headers_label,
            ink,
            o.x + right,
            ("request-headers-label", y0 as i32),
        );
        y += 16.0;
        let headers = winui::multiline_light(
            ui,
            r(o, 14.0, y, right - 14.0, 74.0),
            &mut self.headers,
            &format!("request-headers-{}", y0 as i32),
            enabled,
        );
        winui::name(&headers, headers_label);
        if headers.changed() {
            self.error = None;
        }
        y += 84.0;
        // i18n: Site rule / a download's Connection settings: check box before the number box; unticked, the number set in Options → Connection applies
        let conns_caption = crate::i18n::t("Number of connections:");
        let box_x = 14.0 + 16.0 + winui::text_width(&p, conns_caption).ceil() + 14.0;
        let box_x = box_x.min(right - 64.0);
        let was = self.own_conns;
        winui::checkbox_light_until(
            ui,
            o + vec2(14.0, y + 5.0),
            &mut self.own_conns,
            conns_caption,
            enabled,
            Some(o.x + box_x - 2.0),
        );
        if was != self.own_conns {
            self.error = None;
        }
        let spin = winui::spin_box(
            ui,
            r(o, box_x, y, 64.0, 23.0),
            &mut self.conns,
            1,
            u64::from(limit),
            enabled && self.own_conns,
            &format!("request-conns-{}", y0 as i32),
        );
        winui::name(&spin, conns_caption);
        y += 31.0;
        match self.what {
            For::Rule => {
                winui::checkbox_light_until(
                    ui,
                    o + vec2(14.0, y + 3.0),
                    &mut self.browser_like,
                    // i18n: Site rule: check box; on, MDM connects the way a browser does from the first request to these sites, instead of only after a site turned it away ("this site lets only browsers through")
                    "Always connect the way a browser does",
                    enabled,
                    Some(o.x + right),
                );
                y += 24.0;
            }
            For::Download => {
                winui::text_right(
                    &p,
                    o.x + col - 8.0,
                    o.y + y + 15.0,
                    crate::i18n::t(labels[1]),
                    ink,
                );
                let names: Vec<&str> = PROXY_CHOICES.iter().map(|c| crate::i18n::t(c)).collect();
                let combo_w = names
                    .iter()
                    .map(|n| winui::text_width(&p, n).ceil() + 36.0)
                    .fold(120.0_f32, f32::max)
                    .min((right - col) * 0.55);
                if let Some(i) = winui::combo_light(
                    ui,
                    r(o, col, y + 1.0, combo_w, 21.0),
                    &names,
                    self.proxy,
                    enabled,
                    &format!("request-proxy-{}", y0 as i32),
                ) {
                    self.proxy = i;
                    self.error = None;
                }
                let addr_x = col + combo_w + 8.0;
                let addr = winui::edit_light(
                    ui,
                    r(o, addr_x, y, right - addr_x, 23.0),
                    &mut self.proxy_addr,
                    enabled && self.proxy == 2,
                    false,
                );
                winui::name(&addr, crate::i18n::t(PROXY_CHOICES[2]));
                if addr.changed() {
                    self.error = None;
                }
                y += 31.0;
            }
        }
        if let Some(e) = &self.error {
            winui::fit(
                ui,
                &p,
                o.x + 14.0,
                o.y + y + 10.0,
                e,
                winui::ERROR(),
                o.x + right,
                ("request-error", y0 as i32),
            );
        }
        y + 18.0
    }
}

/// A header line's problem, for the user.
fn header_error(e: &HeaderError) -> String {
    let (line, text) = match e {
        HeaderError::NoColon(n) => (
            *n,
            // i18n: Site rule / a download's Connection settings: a line of request headers isn't "Name: value"; {line} is its number
            crate::i18n::t("Line {line}: write a header as Name: value"),
        ),
        HeaderError::BadName(n) => (
            *n,
            // i18n: Site rule / a download's Connection settings: a header's name has a space or another character names can't have; {line} is the line's number
            crate::i18n::t("Line {line}: a header's name has letters, digits and dashes only"),
        ),
        HeaderError::BadValue(n) => (
            *n,
            // i18n: Site rule / a download's Connection settings: a header's value (or the User-Agent) holds a control character; {line} is the line's number
            crate::i18n::t("Line {line}: the value has a character a header can't hold"),
        ),
        HeaderError::Reserved(n, name) => {
            return crate::i18n::fmt(
                // i18n: Site rule / a download's Connection settings: a header MDM sets itself for each request (Range, Host…) can't be typed; {line} is the line's number, {name} the header's name
                crate::i18n::t("Line {line}: Mister Download Manager sets {name} itself"),
                &[("line", &n.to_string()), ("name", name)],
            );
        }
    };
    if line == 0 {
        // The User-Agent box's value.
        return crate::i18n::t(
            // i18n: Site rule / a download's Connection settings: the User-Agent typed holds a control character
            "The User-Agent has a character a header can't hold",
        )
        .to_owned();
    }
    crate::i18n::fmt(text, &[("line", &line.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proxy_is_read_with_its_scheme() {
        assert_eq!(
            proxy_url("10.0.0.1:3128").as_deref(),
            Some("http://10.0.0.1:3128")
        );
        assert_eq!(
            proxy_url(" socks5h://proxy.example.test:1080/ ").as_deref(),
            Some("socks5h://proxy.example.test:1080")
        );
        assert_eq!(
            proxy_url("https://p.example.test").as_deref(),
            Some("https://p.example.test")
        );
        assert_eq!(proxy_url(""), None);
        assert_eq!(proxy_url("ftp://p.example.test:21"), None);
        assert_eq!(proxy_url("http://p.example.test/path"), None);
        assert_eq!(
            proxy_url("http://user@p.example.test:8080"),
            None,
            "logins: Options"
        );
        assert_eq!(proxy_url("not a proxy at all"), None);
    }

    #[test]
    fn a_downloads_own_settings_come_last() {
        let own = OwnRequest {
            user_agent: None,
            headers: vec![
                ("User-Agent".into(), "Own/2".into()),
                ("X-Own".into(), "1".into()),
                ("Range".into(), "bytes=0-".into()),
            ],
            proxy: Some("127.0.0.1:3128".into()),
            connections: Some(500),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-own", "rule".parse().unwrap());
        let mut opts = mdm_engine::Options {
            agent_fallback: Some("Fallback".into()),
            own_agent: Some("Own name".into()),
            ..Default::default()
        };
        apply_own(&own, &mut headers, &mut opts, 32);
        assert_eq!(headers.get("x-own").unwrap(), "1", "replaces the rule's");
        assert!(
            headers.get("range").is_none(),
            "MDM's own header isn't taken"
        );
        assert!(
            headers.get("user-agent").is_none(),
            "the User-Agent goes to the options"
        );
        assert_eq!(opts.user_agent, "Own/2");
        assert!(!opts.automatic_agent);
        assert_eq!(
            opts.agent_fallback, None,
            "the user's choice, never another"
        );
        assert_eq!(opts.own_agent, None);
        assert_eq!(opts.max_connections, 32, "never beyond the limit");
        assert_eq!(
            opts.proxy,
            mdm_engine::Proxy::Manual {
                url: "http://127.0.0.1:3128".into(),
                login: None,
                http: true,
                https: true,
            }
        );
        let direct = OwnRequest {
            proxy: Some(String::new()),
            ..OwnRequest::default()
        };
        apply_own(&direct, &mut headers, &mut opts, 32);
        assert_eq!(opts.proxy, mdm_engine::Proxy::None);
    }

    #[test]
    fn the_fields_read_back_what_they_were_given() {
        let own = OwnRequest {
            user_agent: Some("Mine/1".into()),
            headers: vec![("X-A".into(), "1".into())],
            proxy: Some("socks5://127.0.0.1:1080".into()),
            connections: Some(4),
        };
        let edit = RequestEdit::download(&own, 8);
        let read = edit.read(32).unwrap();
        assert_eq!(read.user_agent, own.user_agent);
        assert_eq!(read.headers, own.headers);
        assert_eq!(read.proxy, own.proxy);
        assert_eq!(read.connections, own.connections);
        assert!(!read.browser_like);
        let mut bad = edit.clone();
        bad.headers = "X-A 1".into();
        assert!(bad.read(32).is_err());
        let mut bad = edit.clone();
        bad.conns = "33".into();
        assert!(bad.read(32).is_err());
        assert!(bad.read(128).is_ok());
        let mut bad = edit;
        bad.proxy_addr = "nothing here".into();
        assert!(bad.read(32).is_err());
        let rule = mdm_ipc::siterules::SiteRule {
            site: "*.example.test".into(),
            browser_like: true,
            ..Default::default()
        };
        let edit = RequestEdit::rule(&rule, None, 8);
        let read = edit.read(32).unwrap();
        assert!(read.browser_like);
        assert_eq!(read.connections, None, "unticked: the default");
        assert_eq!(read.proxy, None, "a rule has no proxy");
    }
}

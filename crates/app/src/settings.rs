//! App settings, saved with the download list (store.rs). Defaults are IDM 6.42's
//! (notes/ui-spec/options-dialog.md, scheduler-and-misc.md).

use mdm_ipc::capture::host_matches;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Downloads → Speed Limiter → Turn on / Turn off.
    pub speed_limit_on: bool,
    /// "Maximum download speed for one file", KBytes/sec (IDM default 10).
    pub speed_limit_kb: u64,
    /// "Always turn on Speed Limiter on … startup".
    pub limiter_on_startup: bool,
    /// Beyond IDM (D-012): one limit for all downloads together, optionally only while the
    /// user is at the computer. Off by default.
    pub global_limit_on: bool,
    pub global_limit_kb: u64,
    pub global_only_active: bool,
    /// Options → Connection → "Default max. conn. number" (IDM default 8).
    pub max_connections: u32,
    /// Show the "Download complete" dialog (Options → Downloads).
    pub show_complete: bool,
    /// List right-click → On Double click: Open (true) or Properties (IDM default).
    pub double_click_open: bool,
    /// Ask before deleting downloads ("Don't show this dialog again" clears it).
    pub confirm_delete: bool,
    /// The confirmation's "Also delete the files from disk", as last answered (IDM keeps it):
    /// its first state next time, and what happens when it no longer asks.
    pub delete_files_too: bool,
    /// Categories pane frame width (IDM: 143, frame x 6..149); dragged with the splitter.
    pub categories_width: u32,
    /// The main window as last left: position and size (points: x, y, width, height), and
    /// maximized.
    pub window: Option<[i32; 4]>,
    pub window_maximized: bool,
    /// The Categories pane shown, its branches open (All, Unfinished, Finished, Queues),
    /// and the node last shown.
    pub show_categories: bool,
    pub branches: [bool; 4],
    pub view: Option<crate::app::View>,
    /// View → Dark Mode support (IDM 6.37): follow Windows' dark app mode; off = always light.
    pub dark_mode_support: bool,
    /// Scheduler → Download limits: "Download no more than `limits_mb` MBytes every
    /// `limits_hours` hours" (IDM defaults 200 / 5), with a warning before stopping.
    pub limits_on: bool,
    pub limits_mb: u32,
    pub limits_hours: u32,
    pub limits_warn: bool,

    // ---- Options (notes/ui-spec/options-dialog.md; IDM 6.42 defaults unless noted) ----
    /// General: start with Windows. IDM's default is on; ours is off until the user asks
    /// (a download manager shouldn't register itself silently).
    pub launch_on_startup: bool,
    /// View → Language: a catalog's code (`ar`, `pt-br`…); `None` follows Windows.
    pub language: Option<String>,
    /// The language has been chosen once (in the installer's wizard or the first start's
    /// picker); until then the first start asks (langpick.rs).
    pub language_asked: bool,
    /// The video helpers (yt-dlp, ffmpeg) have been offered once, at a start (helpers.rs).
    pub helpers_asked: bool,
    /// General: offer to download (Download File Info) links of the listed file types copied
    /// to the clipboard. On by default: without the browser extension, copying a link is the
    /// quickest way in.
    pub clipboard_autostart: bool,
    pub mv2_extensions: bool,
    pub advanced_integration: bool,
    /// General: "Capture downloads from the following browsers" (name, on).
    pub browsers: Vec<(String, bool)>,
    /// File types: extensions taken over automatically (space separated).
    pub auto_types: String,
    /// File types: sites (space separated) and addresses never taken over.
    pub excluded_sites: String,
    pub excluded_addresses: Vec<String>,
    /// General → Download panels: the download button over videos in the browsers, and the
    /// sites it stays off (space separated; the native host reads both, mdm_ipc::capture).
    pub video_panel: bool,
    pub video_panel_sites: String,
    /// Options → Regional: how dates and times are written.
    pub regional: crate::regional::Regional,
    /// Save to: a folder per category (category label → folder); missing = the default.
    pub category_dirs: Vec<(String, String)>,
    pub change_folder_on_last: bool,
    /// The addresses typed into Add URL and the folders downloads were saved to, newest first
    /// (the Address and Save As drop lists, as IDM's UrlHistory.txt).
    pub url_history: Vec<String>,
    pub folder_history: Vec<String>,
    pub set_creation_date: bool,
    /// Downloads.
    pub show_start_dialog: bool,
    pub only_add_to_queue: bool,
    pub start_immediately: bool,
    pub queue_panel_later: bool,
    pub queue_panel_batch: bool,
    pub ignore_mtime: bool,
    /// "If a duplicate download link is added": 0 ask, 1 numbered name, 2 overwrite,
    /// 3 complete → show the complete dialog, else resume.
    pub duplicate_action: u8,
    pub user_agent: String,
    /// Connection: per-server connection limits (server, number).
    pub conn_exceptions: Vec<(String, u32)>,
    /// Proxy / Socks: 0 none, 1 system, 2 automatic script, 3 manual.
    pub proxy_from_browser: bool,
    pub proxy_mode: u8,
    pub proxy_script: String,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_user: String,
    pub proxy_pass: String,
    pub proxy_http: bool,
    pub proxy_https: bool,
    pub proxy_ftp: bool,
    /// "Use FTP in PASV mode": passive data connections (the client connects; works behind
    /// NAT), else active. Named anew when FTP arrived: `ftp_pasv` was saved as false while
    /// the box was greyed.
    pub ftp_passive: bool,
    /// Sites Logins: (site/path, user, password).
    pub site_logins: Vec<(String, String, String)>,
    /// Sounds: download complete, download failed, queue started, queue stopped/finished.
    pub sounds: [(bool, String); 4],
    /// The local automation API (beyond IDM): status queries over the command channel. Off by
    /// default; `misterdownloadmanager-cli api on` turns it on.
    pub automation_api: bool,
    /// Torrents keep sharing after they finish (D-018: off; a finished torrent stops, like any
    /// download).
    pub torrent_keep_sharing: bool,
    /// Windows opens magnet links and .torrent files with MDM (D-018: opt-in, current user).
    pub torrent_associations: bool,
    /// Help: look for a newer release once a week (D-020; its switch is in Check for updates:
    /// Options → General has no free row in IDM's layout).
    pub update_check: bool,
    /// When the weekly check last ran (Unix seconds; 0: never).
    pub update_checked: i64,
    /// The user's categories (D-021), after IDM's and Torrents.
    pub custom_categories: Vec<crate::model::CustomCategory>,
    /// Tasks → Show drop target: shown, and where (screen, logical points).
    pub drop_target: bool,
    pub drop_target_pos: Option<(i32, i32)>,
}

/// Options → File types defaults (IDM 6.42), shared with the browser host (mdm_ipc::capture).
pub use mdm_ipc::capture::{DEFAULT_AUTO_TYPES, DEFAULT_EXCLUDED_SITES};
/// The browsers IDM 6.42 lists (all captured on).
pub const BROWSERS: [&str; 8] = [
    "Apple Safari",          // not translated: a product name
    "Brave Browser",         // not translated: a product name
    "Google Chrome",         // not translated: a product name
    "Internet Explorer",     // not translated: a product name
    "Microsoft Edge",        // not translated: a product name
    "Microsoft Edge Legacy", // not translated: a product name
    "Mozilla Firefox",       // not translated: a product name
    "Opera",                 // not translated: a product name
];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            speed_limit_on: false,
            speed_limit_kb: 10,
            limiter_on_startup: false,
            global_limit_on: false,
            global_limit_kb: 500,
            global_only_active: false,
            max_connections: 8,
            show_complete: true,
            double_click_open: false,
            confirm_delete: true,
            delete_files_too: false,
            categories_width: 143,
            window: None,
            window_maximized: false,
            show_categories: true,
            branches: [true, false, false, false],
            view: None,
            dark_mode_support: true,
            limits_on: false,
            limits_mb: 200,
            limits_hours: 5,
            limits_warn: true,
            launch_on_startup: false,
            language: None,
            language_asked: false,
            helpers_asked: false,
            clipboard_autostart: true,
            mv2_extensions: true,
            advanced_integration: true,
            browsers: BROWSERS.iter().map(|b| (b.to_string(), true)).collect(),
            auto_types: DEFAULT_AUTO_TYPES.into(),
            excluded_sites: DEFAULT_EXCLUDED_SITES.into(),
            excluded_addresses: Vec::new(),
            video_panel: true,
            video_panel_sites: String::new(),
            regional: crate::regional::Regional::default(),
            category_dirs: Vec::new(),
            change_folder_on_last: true,
            url_history: Vec::new(),
            folder_history: Vec::new(),
            set_creation_date: false,
            show_start_dialog: true,
            only_add_to_queue: false,
            start_immediately: true,
            queue_panel_later: true,
            queue_panel_batch: true,
            ignore_mtime: false,
            duplicate_action: 0,
            user_agent: mdm_engine::download::DEFAULT_USER_AGENT.into(),
            conn_exceptions: Vec::new(),
            proxy_from_browser: true,
            proxy_mode: 0,
            proxy_script: String::new(),
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_user: String::new(),
            proxy_pass: String::new(),
            proxy_http: false,
            proxy_https: false,
            proxy_ftp: false,
            ftp_passive: true,
            site_logins: Vec::new(),
            sounds: Default::default(),
            automation_api: false,
            torrent_keep_sharing: false,
            torrent_associations: false,
            update_check: true,
            update_checked: 0,
            custom_categories: Vec::new(),
            drop_target: false,
            drop_target_pos: None,
        }
    }
}

/// The most entries a drop list keeps.
pub const HISTORY: usize = 20;

/// Put `value` first in a drop list's history: once, newest first, at most `HISTORY`.
pub fn remember(list: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    list.retain(|v| v != value);
    list.insert(0, value.to_owned());
    list.truncate(HISTORY);
}

impl Settings {
    /// `live` with the settings a dialog changed (`from` → `to`) put in, field by field: what
    /// changed elsewhere while it was open (the toolbar's Speed Limiter, Torrent settings, the
    /// Categories pane) is kept, not reverted to the copy the dialog started from.
    pub fn merge_changes(live: &Settings, from: &Settings, to: &Settings) -> Settings {
        let value = |s: &Settings| serde_json::to_value(s).unwrap_or_default();
        let (mut out, before, after) = (value(live), value(from), value(to));
        if let (Some(out), Some(before), Some(after)) =
            (out.as_object_mut(), before.as_object(), after.as_object())
        {
            for (k, v) in after {
                if before.get(k) != Some(v) {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        serde_json::from_value(out).unwrap_or_else(|_| to.clone())
    }

    /// Options → Save to: the category's folder (its own choice, else `Downloads\<Category>`).
    pub fn category_dir(
        &self,
        cat: crate::model::Category,
        downloads: &std::path::Path,
    ) -> std::path::PathBuf {
        crate::model::category_folder(cat, downloads, &self.category_dirs)
    }

    /// Options → Proxy / Socks for the engine. An automatic configuration script (PAC) is run
    /// once it has been fetched (pacfetch.rs); until then, the system's settings.
    pub fn proxy(&self) -> mdm_engine::Proxy {
        use mdm_engine::Proxy;
        match self.proxy_mode {
            1 => Proxy::System,
            2 => crate::pacfetch::script(&self.proxy_script)
                .map_or(Proxy::System, |script| Proxy::Pac { script }),
            3 if !self.proxy_host.trim().is_empty() => {
                let host = self.proxy_host.trim();
                let url = if host.contains("://") {
                    host.to_owned()
                } else {
                    format!("http://{host}")
                };
                let url = match self.proxy_port.trim() {
                    "" => url,
                    port => format!("{url}:{port}"),
                };
                Proxy::Manual {
                    url,
                    login: (!self.proxy_user.is_empty())
                        .then(|| (self.proxy_user.clone(), self.proxy_pass.clone())),
                    http: self.proxy_http,
                    https: self.proxy_https,
                }
            }
            // "Use proxy/socks from a browser's request": browsers use Windows' proxy settings,
            // so with no proxy of our own so do we (none set there: direct, as before).
            _ if self.proxy_from_browser => Proxy::System,
            _ => Proxy::None,
        }
    }

    /// Connections for a download from `url`: a server exception (Options → Connection), else
    /// the default.
    pub fn connections_for(&self, url: &url::Url) -> u32 {
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        self.conn_exceptions
            .iter()
            .find(|(server, _)| host_matches(&host, server))
            .map_or(self.max_connections, |(_, n)| (*n).max(1))
    }

    /// The Sites Logins entry for `url`: its site/path is a prefix of the address (without the
    /// scheme); `*.` matches subdomains.
    pub fn site_login(&self, url: &url::Url) -> Option<(String, String)> {
        let host = url.host_str()?.to_ascii_lowercase();
        let path = url.path();
        self.site_logins
            .iter()
            .find(|(site, _, _)| {
                let site = site.trim();
                let site = site.split_once("://").map_or(site, |(_, rest)| rest);
                let (site_host, site_path) = site.split_once('/').unwrap_or((site, ""));
                host_matches(&host, site_host)
                    && path
                        .trim_start_matches('/')
                        .starts_with(site_path.trim_start_matches('/'))
            })
            .map(|(_, user, pass)| (user.clone(), pass.clone()))
    }

    /// Options → File types: whether a file name has one of the listed types (`R0*` is a
    /// prefix pattern).
    pub fn auto_type(&self, name: &str) -> bool {
        mdm_ipc::capture::type_listed(&self.auto_types, name)
    }

    /// Set (or forget, with `None`) the category's own folder.
    pub fn set_category_dir(&mut self, cat: crate::model::Category, dir: Option<String>) {
        if let crate::model::Category::Custom(id) = cat {
            if let Some(c) = self.custom_categories.iter_mut().find(|c| c.id == id)
                && c.folder != dir
            {
                c.folder = dir;
                crate::model::set_custom(&self.custom_categories);
            }
            return;
        }
        let label = cat.label();
        self.category_dirs.retain(|(c, _)| *c != *label);
        if let Some(d) = dir {
            self.category_dirs.push((label.into_owned(), d));
        }
    }

    /// The per-download limit new downloads start with, bytes/s.
    pub fn speed_limit(&self) -> Option<u64> {
        self.speed_limit_on
            .then_some(self.speed_limit_kb.max(1) * 1024)
    }
}

/// The proxy MDM's own downloads go through (its helpers, its updates, a crash report's
/// symbols): Options → Proxy / Socks, kept in step by the app ([`share_proxy`]); Windows' own
/// settings in a process without them (the crash monitor).
static OWN_PROXY: std::sync::RwLock<Option<mdm_engine::Proxy>> = std::sync::RwLock::new(None);

/// Keep [`own_net`] on the settings' proxy (cheap when it didn't change).
pub fn share_proxy(proxy: mdm_engine::Proxy) {
    if OWN_PROXY.read().unwrap().as_ref() != Some(&proxy) {
        *OWN_PROXY.write().unwrap() = Some(proxy);
    }
}

/// How MDM downloads its own files: its engine, as itself, through the user's proxy.
pub fn own_net() -> mdm_engine::Options {
    let proxy = OWN_PROXY
        .read()
        .unwrap()
        .clone()
        .unwrap_or(mdm_engine::Proxy::System);
    mdm_tools::net(proxy)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_dialog_keeps_what_changed_elsewhere_meanwhile() {
        let from = super::Settings::default();
        let mut live = from.clone();
        live.speed_limit_on = !from.speed_limit_on; // the toolbar, while Options was open
        let mut to = from.clone();
        to.max_connections = 3; // Options
        let out = super::Settings::merge_changes(&live, &from, &to);
        assert_eq!(out.speed_limit_on, live.speed_limit_on);
        assert_eq!(out.max_connections, 3);
    }

    use super::*;

    #[test]
    fn history_keeps_each_entry_once_newest_first() {
        let mut h = Vec::new();
        for v in ["a", "b", "a", "  ", "c"] {
            remember(&mut h, v);
        }
        assert_eq!(h, ["c", "a", "b"]);
        for i in 0..30 {
            remember(&mut h, &i.to_string());
        }
        assert_eq!(h.len(), HISTORY);
        assert_eq!(h[0], "29");
    }

    #[test]
    fn site_logins_match_by_host_and_path() {
        let s = Settings {
            site_logins: vec![
                ("files.example.com/private".into(), "a".into(), "1".into()),
                ("*.corp.net".into(), "b".into(), "2".into()),
            ],
            ..Settings::default()
        };
        let url = |u: &str| url::Url::parse(u).unwrap();
        assert_eq!(
            s.site_login(&url("https://files.example.com/private/x.zip")),
            Some(("a".into(), "1".into()))
        );
        assert_eq!(
            s.site_login(&url("https://files.example.com/public/x.zip")),
            None
        );
        assert_eq!(
            s.site_login(&url("http://dl.corp.net/y")),
            Some(("b".into(), "2".into()))
        );
    }

    #[test]
    fn listed_file_types() {
        let s = Settings::default();
        assert!(s.auto_type("setup.EXE") && s.auto_type("a.r01") && s.auto_type("x.zip"));
        assert!(!s.auto_type("page.html") && !s.auto_type("noext"));
    }

    #[test]
    fn connection_exceptions_and_proxy() {
        let mut s = Settings {
            conn_exceptions: vec![("ftp.slow.org".into(), 1)],
            ..Settings::default()
        };
        let url = |u: &str| url::Url::parse(u).unwrap();
        assert_eq!(s.connections_for(&url("ftp://ftp.slow.org/a")), 1);
        assert_eq!(s.connections_for(&url("https://fast.org/a")), 8);
        // IDM's default: no proxy, but "use the browser's proxy" ticked — the browser's is
        // Windows' (none set there: direct).
        assert_eq!(s.proxy(), mdm_engine::Proxy::System);
        s.proxy_from_browser = false;
        assert_eq!(s.proxy(), mdm_engine::Proxy::None);
        s.proxy_mode = 3;
        s.proxy_host = "10.0.0.1".into();
        s.proxy_port = "3128".into();
        s.proxy_https = true;
        assert!(matches!(
            s.proxy(),
            mdm_engine::Proxy::Manual { ref url, https: true, http: false, .. }
                if url == "http://10.0.0.1:3128"
        ));
    }
}

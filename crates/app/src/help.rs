//! The Help menu (D-020): the user guide in the repository's docs/, the home page, problem reports on GitHub
//! (pre-filled), update checks against GitHub Releases (on demand and weekly, as IDM checks its
//! site once a week), About, Keyboard shortcuts and Tell a friend.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Color32, Painter, Sense, Stroke, Ui, pos2, vec2};

use crate::dialogs::{Outcome, window};
use crate::winui::{self, ButtonKind, r};

/// The repository: home page, user guide (`docs/`), problem reports, source, licences.
pub const REPO: &str = "https://github.com/anefzaoui/misterdownloadmanager";
const LATEST: &str = "https://api.github.com/repos/anefzaoui/misterdownloadmanager/releases/latest";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMMIT: &str = env!("MDM_COMMIT");
/// The weekly check: at most this often.
pub const CHECK_EVERY: i64 = 7 * 24 * 3600;

crate::theme::themed! {
    LINK = Color32::from_rgb(0x99, 0xc8, 0xff), Color32::from_rgb(0x00, 0x66, 0xcc);
    BOX = Color32::from_rgb(0x1f, 0x1f, 0x1f), Color32::from_rgb(0xff, 0xff, 0xff);
    BOX_EDGE = Color32::from_rgb(0x5a, 0x5a, 0x5a), Color32::from_rgb(0x82, 0x87, 0x90);
    MUTED = Color32::from_rgb(0xa0, 0xa0, 0xa0), Color32::from_rgb(0x6d, 0x6d, 0x6d);
}

/// Open a page of the user guide (`"README.md"`, `"video.md"`, `"README.md#tutorials"`…, in
/// the repository's docs/) in the browser; `""` is the home page (the repository).
pub fn open_page(path: &str) {
    let url = if path.is_empty() {
        REPO.to_owned()
    } else {
        format!("{REPO}/blob/main/docs/{path}")
    };
    let _ = open::that_detached(url);
}

/// A new GitHub issue on the Bug report form (.github/ISSUE_TEMPLATE/bug.yml), filled with
/// what MDM knows: its version, the system and its version, how it was installed, the
/// language. The parameters are the form's field ids; a dropdown takes an option's text.
pub fn report_problem() {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let url = report_fields().iter().fold(
        format!("{REPO}/issues/new?template=bug.yml"),
        |url, (k, v)| format!("{url}&{k}={}", enc(v)),
    );
    let _ = open::that_detached(url);
}

/// The Bug report form's fields MDM fills in: (field id, value).
fn report_fields() -> Vec<(&'static str, String)> {
    #[cfg(windows)]
    let os_version = crate::win32::windows_version();
    #[cfg(unix)]
    let os_version = crate::unix::os_version();
    let os = if cfg!(windows) {
        if os_version.contains("Windows 10") {
            "Windows 10"
        } else {
            "Windows 11"
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "macOS (Apple Silicon)"
        } else {
            "macOS (Intel)"
        }
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Other"
    };
    let mut fields = vec![
        ("os", os.to_owned()),
        ("os-version", os_version),
        ("version", format!("{VERSION} ({COMMIT})")),
        ("language", crate::i18n::code().to_owned()),
    ];
    if let Some(install) = installed_with() {
        fields.push(("install", install.to_owned()));
    }
    fields
}

/// How this copy was installed, as the Bug report form's "Installed with" words it; `None`
/// when it can't tell.
fn installed_with() -> Option<&'static str> {
    if cfg!(debug_assertions) {
        return Some("Built from source");
    }
    if mdm_ipc::portable_dir().is_some() {
        return Some("The portable zip");
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if cfg!(windows) {
        // The installer leaves its uninstaller next to the program; the plain zip doesn't.
        return Some(if dir.join("unins000.exe").is_file() {
            "The installer (setup.exe)"
        } else {
            "The plain zip"
        });
    }
    if cfg!(target_os = "macos") {
        return exe
            .to_string_lossy()
            .contains(".app/Contents/MacOS")
            .then_some("The .dmg");
    }
    if std::env::var_os("APPIMAGE").is_some() {
        return Some("The AppImage");
    }
    if dir == std::path::Path::new("/usr/bin") {
        return Some(if std::path::Path::new("/etc/debian_version").exists() {
            "The .deb"
        } else {
            "The .rpm"
        });
    }
    Some("The .tar.gz")
}

/// Text that opens something when clicked: link colour, underlined under the pointer.
fn link(ui: &Ui, p: &Painter, x: f32, baseline: f32, label: &str, salt: &str) -> bool {
    let rect = winui::text(p, x, baseline, crate::i18n::t(label), LINK());
    let resp = ui
        .interact(rect, ui.scope_id().with(("link", salt)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        let y = rect.bottom() + 0.5;
        p.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, LINK()),
        );
    }
    resp.clicked()
}

// ---------------------------------------------------------------------------------------------
// Updates

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub notes: String,
    /// The release's page (the download is there; nothing installs by itself).
    pub page: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    Latest,
    Newer(Release),
}

pub type Slot = Arc<Mutex<Option<Result<Found, String>>>>;

/// Ask GitHub for the latest release in the background; the answer lands in the slot.
pub fn check(rt: &tokio::runtime::Handle, ctx: &egui::Context) -> Slot {
    let slot: Slot = Arc::new(Mutex::new(None));
    let (out, ctx) = (slot.clone(), ctx.clone());
    rt.spawn(async move {
        let res = latest().await;
        *out.lock().unwrap() = Some(res);
        ctx.request_repaint();
    });
    slot
}

async fn latest() -> Result<Found, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("MisterDownloadManager/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    // No release published yet: nothing newer than this.
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Found::Latest);
    }
    let text = resp
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    parse_release(&text, VERSION)
}

/// GitHub's latest-release JSON against the running `current` version.
fn parse_release(json: &str, current: &str) -> Result<Found, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("").trim();
    let version = tag.trim_start_matches(['v', 'V']);
    if version.is_empty() {
        // i18n: why updates could not be checked: the newest release on the site has no number
        return Err(crate::i18n::t("the release has no version").to_owned());
    }
    if !newer(version, current) {
        return Ok(Found::Latest);
    }
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    Ok(Found::Newer(Release {
        version: version.to_owned(),
        date: s("published_at").chars().take(10).collect(),
        notes: s("body"),
        page: s("html_url"),
    }))
}

/// `a` is a later version than `b` (numeric parts; a pre-release `-…` counts before its
/// release).
pub fn newer(a: &str, b: &str) -> bool {
    fn key(v: &str) -> (Vec<u64>, bool) {
        let (main, pre) = v.split_once('-').map_or((v, false), |(m, _)| (m, true));
        (
            main.split('.')
                .map(|p| p.trim().parse().unwrap_or(0))
                .collect(),
            !pre,
        )
    }
    let (mut ka, mut kb) = (key(a), key(b));
    let n = ka.0.len().max(kb.0.len());
    ka.0.resize(n, 0);
    kb.0.resize(n, 0);
    ka > kb
}

/// Help → Check for updates... (and the weekly check when it finds one).
pub struct Updates {
    slot: Option<Slot>,
    result: Option<Result<Found, String>>,
}

impl Updates {
    pub fn check(rt: &tokio::runtime::Handle, ctx: &egui::Context) -> Self {
        Updates {
            slot: Some(check(rt, ctx)),
            result: None,
        }
    }

    /// The weekly check found `release`.
    pub fn found(release: Release) -> Self {
        Updates {
            slot: None,
            result: Some(Ok(Found::Newer(release))),
        }
    }

    /// `weekly`: the once-a-week check (its switch is here, under the answer).
    pub fn show(&mut self, ctx: &egui::Context, weekly: &mut bool) -> Outcome<()> {
        if let Some(r) = self.slot.as_ref().and_then(|s| s.lock().unwrap().take()) {
            self.result = Some(r);
            self.slot = None;
        }
        let result = self.result.clone();
        // Compact unless there are release notes to show (then a window of its own size).
        let notes = matches!(result, Some(Ok(Found::Newer(_))));
        let (id, h) = if notes {
            ("updates-new", 250.0)
        } else {
            ("updates", 96.0)
        };
        let bottom = h - 32.0;
        window(
            ctx,
            id,
            "Check for updates",
            [440.0, h],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let ink = winui::TEXT();
                let right = o.x + 429.0;
                let mut download = None;
                match &result {
                    None => {
                        winui::text(
                            &p,
                            o.x + 11.0,
                            o.y + 24.0,
                            crate::i18n::t("Checking for updates..."),
                            ink,
                        );
                    }
                    Some(Ok(Found::Latest)) => {
                        let line = crate::i18n::fmt(
                            crate::i18n::t(
                                "You have the latest version of Mister Download Manager ({version}).",
                            ),
                            &[("version", VERSION)],
                        );
                        winui::paragraph(&p, o.x + 11.0, o.y + 24.0, &line, ink, right, 13.0);
                    }
                    Some(Err(e)) => {
                        let line = crate::i18n::fmt(
                            crate::i18n::t("Updates could not be checked: {error}"),
                            &[("error", e)],
                        );
                        let (rect, cut) =
                            winui::text_fit(&p, o.x + 11.0, o.y + 24.0, &line, ink, right);
                        if cut {
                            winui::full_on_hover(ui, rect, "update-error", &line);
                        }
                    }
                    Some(Ok(Found::Newer(rel))) => {
                        let line = crate::i18n::fmt(
                            crate::i18n::t(
                                "Mister Download Manager {version} is available (released {date}).",
                            ),
                            &[("version", &rel.version), ("date", &rel.date)],
                        );
                        winui::text_fit(&p, o.x + 11.0, o.y + 24.0, &line, ink, right);
                        let have = crate::i18n::fmt(
                            crate::i18n::t("You have version {version}."),
                            &[("version", VERSION)],
                        );
                        winui::text(&p, o.x + 11.0, o.y + 42.0, &have, MUTED());
                        winui::text(
                            &p,
                            o.x + 11.0,
                            o.y + 66.0,
                            crate::i18n::t("What's new:"),
                            ink,
                        );
                        let notes = egui::Rect::from_min_max(
                            pos2(o.x + 11.0, o.y + 74.0),
                            pos2(right, o.y + 206.0),
                        );
                        // English, as the paragraph is laid out; made where it is on screen.
                        p.rect_filled(winui::rect(notes), 0.0, BOX());
                        p.rect_stroke(
                            winui::rect(notes),
                            0.0,
                            Stroke::new(1.0, BOX_EDGE()),
                            egui::StrokeKind::Inside,
                        );
                        let text = plain_notes(&rel.notes);
                        let painter = p.with_clip_rect(winui::rect(notes.shrink(2.0)));
                        winui::paragraph(
                            &painter,
                            notes.left() + 6.0,
                            notes.top() + 16.0,
                            &text,
                            ink,
                            notes.right() - 6.0,
                            13.0,
                        );
                        download = Some(rel.page.clone());
                    }
                }
                winui::checkbox(
                    ui,
                    o + vec2(11.0, bottom + 4.0),
                    weekly,
                    "Check for updates once a week",
                    true,
                );
                let close_label = if download.is_some() {
                    // i18n: button: not now (download the update later)
                    crate::i18n::mark("Later")
                } else {
                    crate::i18n::mark("OK")
                };
                if let Some(page) = &download {
                    let get = winui::button(
                        ui,
                        r(o, 274.0, bottom, 73.0, 21.0),
                        "Download",
                        ButtonKind::Default,
                        false,
                        None,
                    )
                    .clicked();
                    if get {
                        let _ = open::that_detached(page);
                        return Outcome::Done(());
                    }
                }
                let kind = if download.is_some() {
                    ButtonKind::Normal
                } else {
                    ButtonKind::Default
                };
                let close = winui::button(
                    ui,
                    r(o, 356.0, bottom, 73.0, 21.0),
                    close_label,
                    kind,
                    false,
                    None,
                )
                .clicked();
                if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    Outcome::Closed
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// Release notes (Markdown) as plain lines: headings, bullets and emphasis marks dropped.
fn plain_notes(md: &str) -> String {
    md.lines()
        .map(|l| {
            let l = l.trim_end().trim_start_matches('#').trim_start();
            let l = l
                .strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .map_or(l.to_owned(), |b| format!("• {b}"));
            l.replace("**", "").replace('`', "")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------------
// About

pub struct About;

impl About {
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        window(
            ctx,
            "about",
            "About Mister Download Manager",
            [420.0, 214.0],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let ink = winui::TEXT();
                egui::Image::from_bytes(
                    "bytes://mdm-48.png",
                    include_bytes!("../assets/icons/app/mdm-48.png"),
                )
                .paint_at(ui, winui::r(o, 16.0, 16.0, 48.0, 48.0));
                winui::text(
                    &p,
                    o.x + 80.0,
                    o.y + 28.0,
                    crate::i18n::t("Mister Download Manager"),
                    ink,
                );
                let version = crate::i18n::fmt(
                    crate::i18n::t("Version {version} ({commit})"),
                    &[("version", VERSION), ("commit", COMMIT)],
                );
                winui::text(&p, o.x + 80.0, o.y + 46.0, &version, MUTED());
                let lines = winui::paragraph(
                    &p,
                    o.x + 80.0,
                    o.y + 72.0,
                    crate::i18n::t(
                        "The download manager you already know, rebuilt to be faster. Free and open source under the GNU GPL, version 3 or later. Videos are found by yt-dlp; torrents run on librqbit.",
                    ),
                    ink,
                    o.x + 404.0,
                    13.0,
                );
                // The links in a row, 18 px apart; one that would pass the paragraph's right
                // edge (a long translation) starts the next row.
                let (mut x, mut y) = (o.x + 80.0, o.y + 72.0 + 13.0 * lines as f32 + 14.0);
                let mut place = |label: &str| {
                    let w = winui::text_width(&p, crate::i18n::t(label));
                    if x > o.x + 80.0 && x + w > o.x + 404.0 {
                        (x, y) = (o.x + 80.0, y + 16.0);
                    }
                    let at = (x, y);
                    x += w + 18.0;
                    at
                };
                let (home, source, credits) = (
                    place("Home page"),
                    place("Source code"),
                    place("Licenses and credits"),
                );
                if link(ui, &p, home.0, home.1, "Home page", "home") {
                    open_page("");
                }
                if link(ui, &p, source.0, source.1, "Source code", "source") {
                    let _ = open::that_detached(REPO);
                }
                if link(
                    ui,
                    &p,
                    credits.0,
                    credits.1,
                    "Licenses and credits",
                    "credits",
                ) {
                    let _ = open::that_detached(format!("{REPO}/blob/main/ATTRIBUTION.md"));
                }
                let ok = winui::button(
                    ui,
                    r(o, 336.0, 182.0, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                if ok
                    || ui.input(|i| {
                        i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)
                    })
                {
                    Outcome::Closed
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Keyboard shortcuts

/// Every shortcut the main window answers (keep in step with `app.rs` and `list.rs`).
const SHORTCUTS: [(&str, &str); 21] = [
    ("Ctrl+N", "Add a download (Add URL)"),
    ("Ctrl+F", "Find a download"),
    ("F3", "Find the next match"),
    ("Enter", "Open the selected download"),
    ("Alt+Enter", "Properties of the selected download"),
    ("Ctrl+M", "Move or rename the selected download"),
    ("Delete", "Delete the selected downloads (asks first)"),
    (
        "Shift+Delete",
        "Delete the selected downloads and their files (asks first)",
    ),
    ("Ctrl+A", "Select all downloads"),
    ("Ctrl+click", "Select or unselect one more download"),
    ("Shift+click", "Select every download up to this one"),
    ("Drag on blank space", "Select the downloads in the box"),
    ("Up / Down", "Select the previous / next download"),
    (
        "Shift+Up / Shift+Down",
        "Make the selection longer or shorter",
    ),
    ("Ctrl+Up / Ctrl+Down", "Move without changing the selection"),
    (
        "Ctrl+Space",
        "Select or unselect the download with the dotted outline",
    ),
    ("Page Up / Page Down", "Move a page up / down"),
    ("Home / End", "Select the first / last download"),
    (
        "Type a name",
        "Jump to the download whose name starts with it",
    ),
    ("Shift+F10", "Open the selected download's menu"),
    ("Esc", "Close a menu or dialog"),
];

pub struct Shortcuts;

impl Shortcuts {
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        let rows = SHORTCUTS.len() as f32;
        window(
            ctx,
            "shortcuts",
            "Keyboard shortcuts",
            [520.0, 52.0 + 18.0 * rows + 20.0],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let ink = winui::TEXT();
                for (i, (keys, what)) in SHORTCUTS.iter().enumerate() {
                    let y = o.y + 24.0 + 18.0 * i as f32;
                    // As this system writes it: ⌘N on macOS, the translated key names elsewhere.
                    let keys = crate::chrome::key_label(keys);
                    winui::text_fit(&p, o.x + 16.0, y, &keys, ink, o.x + 174.0);
                    winui::text_fit(&p, o.x + 184.0, y, crate::i18n::t(what), ink, o.x + 504.0);
                }
                let y = 24.0 + 18.0 * rows + 6.0;
                let ok = winui::button(
                    ui,
                    r(o, 436.0, y, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                if ok
                    || ui.input(|i| {
                        i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)
                    })
                {
                    Outcome::Closed
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Tell a friend

pub struct TellFriend {
    copied: bool,
}

impl TellFriend {
    pub fn new() -> Self {
        TellFriend { copied: false }
    }

    fn message() -> String {
        crate::i18n::fmt(
            crate::i18n::t(
                "I download with Mister Download Manager: fast, free, and it looks like the download manager you already know. {link}",
            ),
            &[("link", REPO)],
        )
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        let message = Self::message();
        window(
            ctx,
            "tell-a-friend",
            "Tell a friend",
            [420.0, 168.0],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let ink = winui::TEXT();
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 21.0,
                    crate::i18n::t("Send this to a friend:"),
                    ink,
                );
                let rect = egui::Rect::from_min_max(
                    pos2(o.x + 11.0, o.y + 30.0),
                    pos2(o.x + 409.0, o.y + 106.0),
                );
                // English, as the paragraph is laid out; made where it is on screen.
                p.rect_filled(winui::rect(rect), 0.0, BOX());
                p.rect_stroke(
                    winui::rect(rect),
                    0.0,
                    Stroke::new(1.0, BOX_EDGE()),
                    egui::StrokeKind::Inside,
                );
                winui::paragraph(
                    &p,
                    rect.left() + 6.0,
                    rect.top() + 17.0,
                    &message,
                    ink,
                    rect.right() - 6.0,
                    13.0,
                );
                if self.copied {
                    winui::text(
                        &p,
                        o.x + 11.0,
                        o.y + 150.0,
                        crate::i18n::t("Copied to the clipboard."),
                        MUTED(),
                    );
                }
                let copy = winui::button(
                    ui,
                    r(o, 172.0, 136.0, 73.0, 21.0),
                    "Copy",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                let email = winui::button(
                    ui,
                    r(o, 254.0, 136.0, 73.0, 21.0),
                    "Email",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                let close = winui::button(
                    ui,
                    r(o, 336.0, 136.0, 73.0, 21.0),
                    "Close",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                if copy {
                    // The user asked for it: the message goes to the clipboard.
                    self.copied = arboard::Clipboard::new()
                        .and_then(|mut c| c.set_text(message.clone()))
                        .is_ok();
                }
                if email {
                    let enc = |s: &str| {
                        url::form_urlencoded::byte_serialize(s.as_bytes())
                            .collect::<String>()
                            .replace('+', "%20")
                    };
                    let subject = crate::i18n::t("Mister Download Manager");
                    let _ = open::that_detached(format!(
                        "mailto:?subject={}&body={}",
                        enc(subject),
                        enc(&message)
                    ));
                }
                if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    Outcome::Closed
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.9.9"));
        assert!(
            newer("1.0.0", "1.0.0-rc.1"),
            "a release is newer than its pre-release"
        );
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0-beta", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn a_release_is_read_from_github() {
        let json = r###"{"tag_name":"v0.2.0","published_at":"2026-11-05T10:00:00Z",
            "html_url":"https://github.com/x/y/releases/tag/v0.2.0","body":"## New\n- Faster"}"###;
        assert_eq!(
            parse_release(json, "0.1.0").unwrap(),
            Found::Newer(Release {
                version: "0.2.0".into(),
                date: "2026-11-05".into(),
                notes: "## New\n- Faster".into(),
                page: "https://github.com/x/y/releases/tag/v0.2.0".into(),
            })
        );
        assert_eq!(parse_release(json, "0.2.0").unwrap(), Found::Latest);
        assert!(parse_release("{}", "0.1.0").is_err());
    }

    #[test]
    fn notes_become_plain_lines() {
        assert_eq!(
            plain_notes("## New\n- **Faster** `x`\n* b"),
            "New\n• Faster x\n• b"
        );
    }

    #[test]
    fn the_report_fills_fields_the_bug_form_has() {
        let form = include_str!("../../../.github/ISSUE_TEMPLATE/bug.yml").replace("\r\n", "\n");
        let has = |line: String| form.contains(&format!("{line}\n"));
        let fields = report_fields();
        for (id, value) in &fields {
            assert!(has(format!("id: {id}")), "no field {id} in bug.yml");
            // A dropdown takes one of its options' text.
            if matches!(*id, "os" | "install") {
                assert!(has(format!("- {value}")), "{id}: {value:?} isn't an option");
            }
        }
        assert!(fields.iter().any(|(id, _)| *id == "install"));
        // Every value installed_with() and report_fields() can give, on any system.
        for option in [
            "The installer (setup.exe)",
            "The portable zip",
            "The plain zip",
            "The .dmg",
            "The AppImage",
            "The .deb",
            "The .rpm",
            "The .tar.gz",
            "Built from source",
            "Windows 11",
            "Windows 10",
            "macOS (Apple Silicon)",
            "macOS (Intel)",
            "Linux",
            "Other",
        ] {
            assert!(has(format!("- {option}")), "{option}");
        }
    }
}

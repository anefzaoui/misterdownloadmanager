//! The browser extension's offer: at each start, until a browser's extension has reached the
//! app (the native host notes it: mdm_ipc::extension::seen), a window lists the browsers
//! installed and opens the extension's page in the one picked (addons.mozilla.org for Firefox
//! and its family, the Chrome Web Store for Chrome, Edge, Brave, Opera and Vivaldi), with the
//! tutorial that adds it by hand as the fallback. Chrome may ask by itself too: registering the
//! host also names the store's extension to it (crates/nmhost). "Don't ask again" stops the
//! offer; `/show extension` brings it back.

use std::path::PathBuf;
use std::time::Duration;

use egui::{FontFamily, FontId, Pos2, Ui};

use crate::dialogs::{Outcome, window};
use crate::winui::{self, ButtonKind, r};

const W: f32 = 520.0;
const BANNER: f32 = 84.0;
const ROW: f32 = 34.0;

/// Whether to offer the extension at this start: browser integration on and registered (not a
/// portable copy, which registers nothing), no browser's extension seen yet, not refused.
pub fn worth_asking(settings: &crate::settings::Settings) -> bool {
    !settings.extension_never_ask
        && settings.advanced_integration
        && mdm_ipc::portable_dir().is_none()
        && !mdm_ipc::extension::seen()
}

pub struct Extension {
    /// The browsers installed: name and program. Empty: one row for the system's browser.
    browsers: Vec<(String, Option<PathBuf>)>,
    /// A browser's extension has reached the app since the window opened.
    connected: bool,
    /// The browsers whose extension has (Options' names, mdm_ipc::extension::seen_browsers).
    seen: Vec<String>,
    /// The browsers can find the app (the native host is registered).
    registered: bool,
    /// When the status files were read last (once a second, not every frame).
    looked: std::time::Instant,
}

/// Whether `row` (a browser as the window lists it) is among the browsers seen (as Options
/// names them). Vivaldi, Chromium, Helium, Thorium and Arc say they are Chrome (no brand of
/// their own, or Chrome's on purpose): they share Chrome's status. Firefox's forks give their
/// own name (`capture::browser_named`).
fn is_seen(seen: &[String], row: &str) -> bool {
    let named = match row {
        "Brave" => "Brave Browser",
        "Vivaldi" | "Chromium" | "Helium" | "Thorium" | "Arc" => "Google Chrome",
        other => other,
    };
    seen.iter().any(|b| b == named)
}

/// A row for a browser of Firefox's family (Firefox, Zen, LibreWolf …): it gets the add-on's
/// page on addons.mozilla.org, not the Chrome Web Store.
fn is_gecko(row: &str) -> bool {
    mdm_ipc::browsers::named(row).is_some_and(|b| b.family == mdm_ipc::browsers::Family::Gecko)
}

impl Extension {
    pub fn new() -> Self {
        #[cfg(windows)]
        let found = crate::win32::browsers();
        #[cfg(unix)]
        let found = crate::unix::browsers();
        let mut browsers: Vec<_> = found.into_iter().map(|(n, p)| (n, Some(p))).collect();
        // The table's order (Chrome first, its button the accent one), others after.
        browsers.sort_by_key(|(n, _)| {
            mdm_ipc::browsers::BROWSERS
                .iter()
                .position(|b| b.name == n.as_str())
                .unwrap_or(usize::MAX)
        });
        if browsers.is_empty() {
            browsers.push((String::new(), None));
        }
        Extension {
            browsers,
            connected: mdm_ipc::extension::seen(),
            seen: seen_names(),
            registered: registered(),
            looked: std::time::Instant::now(),
        }
    }

    /// Open until closed; Done when "Don't ask again" was pressed.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        // The host marks the extension seen when a browser starts it: look again now and then
        // (each browser's own, as each adds it).
        if self.looked.elapsed() >= Duration::from_secs(1) {
            self.looked = std::time::Instant::now();
            self.connected = mdm_ipc::extension::seen();
            self.seen = seen_names();
            self.registered = registered();
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        let h = BANNER + 16.0 + self.browsers.len() as f32 * ROW + 104.0;
        let mut answer = None;
        window(
            ctx,
            "extension",
            // i18n: title of the window that offers the browser extension
            "Add Mister Download Manager to your browser",
            [W, h],
            false,
            winui::BG(),
            |ui, o| {
                self.body(ui, o, h, &mut answer);
                match answer.take() {
                    Some(true) => Outcome::Done(()),
                    Some(false) => Outcome::Closed,
                    None if winui::escape_pressed(ui) => Outcome::Closed,
                    None => Outcome::Open,
                }
            },
        )
    }

    fn body(&mut self, ui: &mut Ui, o: Pos2, h: f32, answer: &mut Option<bool>) {
        let p = ui.painter().clone();
        let (ink, muted) = (winui::TEXT(), winui::MUTED());
        // The banner: the logo, what the extension does, in one line, and how.
        p.rect_filled(winui::rect(r(o, 0.0, 0.0, W, BANNER)), 0.0, winui::FIELD());
        winui::fill_l(
            &p,
            o.x,
            o.y + BANNER,
            o.x + W - 1.0,
            o.y + BANNER,
            winui::FIELD_EDGE(),
        );
        egui::Image::new(egui::include_image!("../assets/icons/app/mdm-48.png"))
            .paint_at(ui, winui::rect(r(o, 18.0, 18.0, 48.0, 48.0)));
        winui::text_font(
            &p,
            FontId::new(16.0, FontFamily::Name("tahoma-bold".into())),
            o.x + 80.0,
            o.y + 36.0,
            // i18n: the headline of the window that offers the browser extension
            crate::i18n::t("Catch downloads from your browser"),
            ink,
        );
        winui::paragraph(
            &p,
            o.x + 80.0,
            o.y + 57.0,
            crate::i18n::t(
                "Add the free extension: files you download in your browser come here, and videos get a Download button.",
            ),
            muted,
            o.x + W - 18.0,
            14.0,
        );

        // One row per browser: its name, and the button that opens the extension's page in it:
        // Firefox's on addons.mozilla.org, the Chrome Web Store's for the others (the tutorial
        // while STORE_ID is a placeholder, as in a build of its own).
        let store = mdm_ipc::extension::store_page();
        for (k, (name, program)) in self.browsers.iter().enumerate() {
            let y = BANNER + 16.0 + k as f32 * ROW;
            let page = if is_gecko(name) {
                Some(mdm_ipc::extension::FIREFOX_PAGE.to_owned())
            } else {
                store.clone()
            };
            winui::text_font(
                &p,
                winui::tahoma_bold(),
                o.x + 22.0,
                o.y + y + 16.0,
                if name.is_empty() {
                    // i18n: a row of the browser extension's window when no browser was found by name
                    crate::i18n::t("Your browser")
                } else {
                    name.as_str()
                },
                ink,
            );
            // This browser's extension is there already: said, before its button.
            if is_seen(&self.seen, name) {
                winui::text_right(
                    &p,
                    o.x + W - 18.0 - 150.0 - 10.0,
                    o.y + y + 16.0,
                    &format!(
                        "\u{2714} {}",
                        // i18n: in the browser extension's window, beside a browser whose extension reached MDM
                        crate::i18n::t("Connected")
                    ),
                    winui::SUCCESS(),
                );
            }
            let label = if page.is_none() {
                // i18n: button in the browser extension's window: open the tutorial that adds the extension
                "Show me how"
            } else {
                // i18n: button in the browser extension's window: open the extension's page in that browser
                "Add the extension"
            };
            let add = winui::button(
                ui,
                r(o, W - 18.0 - 150.0, y, 150.0, 25.0),
                label,
                if k == 0 {
                    ButtonKind::Accent
                } else {
                    ButtonKind::Normal
                },
                false,
                None,
            );
            if add.clicked() {
                let url = page.as_deref().unwrap_or(mdm_ipc::extension::GUIDE_PAGE);
                match program {
                    #[cfg(windows)]
                    Some(program) => crate::win32::open_in(program, url),
                    #[cfg(unix)]
                    Some(program) => crate::unix::open_in(program, url),
                    None => {
                        let _ = open::that_detached(url);
                    }
                }
            }
        }

        // Where it stands, then the answer. Not set up at all (the setup's box unticked): no
        // extension could reach the app, so that comes first, with the fix.
        let y = h - 84.0;
        if !self.registered {
            winui::paragraph(
                &p,
                o.x + 18.0,
                o.y + y,
                crate::i18n::t(
                    "Browser integration isn't set up on this computer, so no browser can reach Mister Download Manager yet.",
                ),
                winui::ERROR(),
                o.x + W - 18.0 - 120.0,
                14.0,
            );
            let fix = winui::button(
                ui,
                r(o, W - 18.0 - 110.0, y - 4.0, 110.0, 25.0),
                // i18n: button in the browser extension's window: register MDM's helper with the browsers
                "Set it up",
                ButtonKind::Accent,
                false,
                None,
            );
            if fix.clicked() {
                set_up();
                self.registered = registered();
            }
        }
        let (line, color) = if !self.registered {
            (String::new(), muted)
        } else if self.connected {
            (
                crate::i18n::t("Connected: downloads from your browser now come here.").to_owned(),
                winui::SUCCESS(),
            )
        } else {
            (
                crate::i18n::t(
                    "Once it is added, this window says so. Chrome may also ask by itself at its next start.",
                )
                .to_owned(),
                muted,
            )
        };
        if !line.is_empty() {
            winui::paragraph(&p, o.x + 18.0, o.y + y, &line, color, o.x + W - 18.0, 14.0);
        }
        winui::etched_h(&p, o.x + 16.0, o.y + h - 46.0, W - 32.0);
        let y = h - 36.0;
        // The fallback, when a store can't be used (a managed browser, a store turned off): the
        // tutorial, which adds it by hand.
        if store.is_some()
            && winui::link(
                ui,
                &p,
                o.x + 18.0,
                o.y + y + 17.0,
                // i18n: link in the browser extension's window: the tutorial that adds it without the browser's store
                "Install it by hand",
                "by-hand",
            )
        {
            let _ = open::that_detached(mdm_ipc::extension::GUIDE_PAGE);
        }
        let close = winui::button(
            ui,
            r(o, W - 18.0 - 90.0, y, 90.0, 25.0),
            "Close",
            ButtonKind::Default,
            false,
            None,
        );
        if close.clicked() || winui::enter_pressed(ui) {
            *answer = Some(false);
        }
        if !self.connected {
            // i18n: button in the browser extension's window: stop offering it at each start
            let never = winui::button(
                ui,
                r(o, W - 18.0 - 90.0 - 8.0 - 130.0, y, 130.0, 25.0),
                "Don't ask again",
                ButtonKind::Normal,
                false,
                None,
            );
            if never.clicked() {
                *answer = Some(true);
            }
        }
    }
}

/// The browsers whose extension reached the app, by Options' names.
fn seen_names() -> Vec<String> {
    mdm_ipc::extension::seen_browsers()
        .into_iter()
        .map(|(b, _)| b)
        .collect()
}

/// Is browser integration registered on this computer (the native host's manifests, where the
/// browsers look)?
pub fn registered() -> bool {
    #[cfg(windows)]
    return crate::win32::host_registered();
    #[cfg(unix)]
    return crate::unix::host_registered();
}

/// Register it now, for a copy installed without it (the setup's box unticked): the native
/// host's own `--register`, for this user, waited for (the checks read the result next).
pub fn set_up() {
    #[cfg(unix)]
    crate::unix::register_browsers_now();
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe.with_file_name("misterdownloadmanager-nmhost.exe"))
            .arg("--register")
            .status();
    }
}

/// One line of Options → General → Test browser integration: what was checked, whether it
/// holds, and the help page's section when it doesn't.
pub struct Check {
    pub ok: bool,
    pub text: String,
    pub why: Option<&'static str>,
}

/// The checks in the order a connection needs them; the first failing one is what to fix.
pub fn checks(settings: &crate::settings::Settings) -> Vec<Check> {
    let check = |ok: bool, text: &str, why: Option<&'static str>| Check {
        ok,
        text: crate::i18n::t(text).to_owned(),
        why: (!ok).then_some(why).flatten(),
    };
    let mut out = vec![
        check(
            settings.advanced_integration,
            // i18n: a check of Test browser integration (Options → General)
            "Advanced browser integration is on",
            Some("options"),
        ),
        check(
            mdm_ipc::portable_dir().is_none(),
            // i18n: a check of Test browser integration: a portable copy can't be connected
            "This is the installed copy, not a portable one",
            Some("not-installed"),
        ),
        check(
            registered(),
            // i18n: a check of Test browser integration: the browsers find MDM's helper program
            "The browsers can find Mister Download Manager",
            Some("not-installed"),
        ),
    ];
    let seen = mdm_ipc::extension::seen_browsers();
    if seen.is_empty() {
        out.push(check(
            false,
            // i18n: a check of Test browser integration: no browser's extension has reached MDM
            "No browser's extension has connected yet",
            None,
        ));
    }
    for (browser, version) in seen {
        let name = if browser.is_empty() {
            crate::i18n::t("Your browser").to_owned()
        } else {
            browser
        };
        let text = if version.is_empty() {
            crate::i18n::fmt(
                // i18n: a check of Test browser integration: {browser} is a browser's name
                crate::i18n::t("{browser}: the extension connected"),
                &[("browser", &name)],
            )
        } else {
            crate::i18n::fmt(
                // i18n: a check of Test browser integration: {browser} is a browser's name, {version} the extension's version number
                crate::i18n::t("{browser}: the extension {version} connected"),
                &[("browser", &name), ("version", &version)],
            )
        };
        out.push(Check {
            ok: true,
            text,
            why: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #41: Helium says it is Chrome; Zen gives its own name; each row reads its own status.
    #[test]
    fn rows_match_the_names_browsers_report() {
        let seen = vec!["Google Chrome".to_owned(), "Zen Browser".to_owned()];
        assert!(is_seen(&seen, "Helium") && is_seen(&seen, "Google Chrome"));
        assert!(is_seen(&seen, "Zen Browser"));
        assert!(!is_seen(&seen, "Mozilla Firefox") && !is_seen(&seen, "Brave"));
        assert!(is_seen(&["Brave Browser".to_owned()], "Brave"));
    }

    /// Firefox's family gets the tutorial, whatever the store has.
    #[test]
    fn gecko_rows_are_firefoxs_family() {
        for b in [
            "Mozilla Firefox",
            "Zen Browser",
            "LibreWolf",
            "Floorp",
            "Waterfox",
        ] {
            assert!(is_gecko(b), "{b}");
        }
        for b in ["Google Chrome", "Helium", "Brave", "Your browser", ""] {
            assert!(!is_gecko(b), "{b}");
        }
    }
}

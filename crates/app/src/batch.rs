//! File → Add batch download (notes/ui-spec/scheduler-and-misc.md §3): an address with an
//! asterisk, replaced by numbers or letters over a range, previewed before it is added.

use egui::{Pos2, Ui, vec2};

use crate::dialogs::{Outcome, window};
use crate::winui::{self, ButtonKind, LIGHT_BG, LIGHT_TEXT, r, text};

pub struct Batch {
    address: String,
    letters: bool,
    from: String,
    to: String,
    size: String,
    auth: bool,
    login: String,
    password: String,
    focused: bool,
}

/// What OK adds: the addresses, and the login if one was given.
pub struct Added {
    pub urls: Vec<url::Url>,
    pub login: Option<(String, String)>,
}

impl Batch {
    /// Pre-filled from the clipboard when it holds an http(s) or ftp(s) address, as IDM's.
    pub fn new(from_clipboard: bool) -> Self {
        let address = if from_clipboard {
            arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
                .map(|t| t.trim().to_owned())
                .filter(|t| url::Url::parse(t).is_ok_and(|u| mdm_engine::supported(&u)))
                .unwrap_or_default()
        } else {
            String::new()
        };
        Batch {
            address,
            letters: false,
            from: "0".into(),
            to: "10".into(),
            size: "1".into(),
            auth: false,
            login: String::new(),
            password: String::new(),
            focused: false,
        }
    }

    /// The addresses the pattern stands for (empty while it isn't valid).
    fn expand(&self) -> Vec<url::Url> {
        let Some((head, tail)) = self.address.trim().split_once('*') else {
            return Vec::new();
        };
        if tail.contains('*') {
            return Vec::new();
        }
        let width: usize = self.size.parse().unwrap_or(1).clamp(1, 10);
        let parts: Vec<String> = if self.letters {
            let (Some(a), Some(b)) = (self.from.chars().next(), self.to.chars().next()) else {
                return Vec::new();
            };
            if !a.is_ascii_alphabetic() || !b.is_ascii_alphabetic() || a > b {
                return Vec::new();
            }
            // One letter per file; the wildcard size applies to numbers.
            (a..=b).map(|c| c.to_string()).collect()
        } else {
            let (Ok(a), Ok(b)) = (self.from.parse::<u64>(), self.to.parse::<u64>()) else {
                return Vec::new();
            };
            if a > b || b - a > 100_000 {
                return Vec::new();
            }
            (a..=b).map(|n| format!("{n:0width$}")).collect()
        };
        parts
            .iter()
            .filter_map(|p| url::Url::parse(&format!("{head}{p}{tail}")).ok())
            .filter(mdm_engine::supported)
            .collect()
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<Added> {
        let urls = self.expand();
        let outcome = window(
            ctx,
            "batch",
            "Batch download",
            [474.0, 387.0],
            false,
            LIGHT_BG,
            |ui, o| self.body(ui, o, &urls),
        );
        match outcome {
            Outcome::Done(()) => Outcome::Done(Added {
                urls,
                login: self
                    .auth
                    .then(|| (self.login.clone(), self.password.clone())),
            }),
            Outcome::Closed => Outcome::Closed,
            Outcome::Open => Outcome::Open,
        }
    }

    fn body(&mut self, ui: &mut Ui, o: Pos2, urls: &[url::Url]) -> Outcome<()> {
        let p = ui.painter().clone();
        let lines = winui::paragraph(
            &p,
            o.x + 11.0,
            o.y + 17.0,
            crate::i18n::t(
                "It's possible to add a group of sequential file names like img001.jpg, img002.jpg, etc.,\nimg100.jpg to MDM download queue. Use the asterisk wildcard for the file name pattern.",
            ),
            LIGHT_TEXT,
            o.x + 470.0,
            13.0,
        );
        text(
            &p,
            o.x + 11.0,
            o.y + 17.0 + 13.0 * lines as f32,
            &crate::i18n::fmt(
                // i18n: {address} = an example web address with a * in the file name
                crate::i18n::t("For example:  {address}"),
                &[(
                    "address",
                    &crate::i18n::ltr("http://www.example.com/pictures/img*.jpg"),
                )],
            ),
            LIGHT_TEXT,
        );
        // i18n: noun: the web address (URL) pattern
        text(
            &p,
            o.x + 11.0,
            o.y + 86.0,
            crate::i18n::t("Address"),
            LIGHT_TEXT,
        );
        let edit = winui::edit_light(
            ui,
            r(o, 59.0, 72.0, 405.0, 21.0),
            &mut self.address,
            true,
            false,
        );
        if !self.focused {
            edit.request_focus();
            self.focused = true;
        }

        // Replace asterisk to: Numbers / Letters, From, To, Wildcard size.
        // i18n: group box title: what the * in the address is replaced with (numbers or letters)
        let caption = crate::i18n::t("Replace asterisk to");
        let cw = winui::text_width(&p, caption);
        winui::group_box(
            &p,
            r(o, 11.0, 110.0, 453.0, 82.0),
            Some((o.x + 18.0, o.x + 22.0 + cw)),
        );
        text(&p, o.x + 20.0, o.y + 114.0, caption, LIGHT_TEXT);
        // i18n: the * in the address runs through numbers
        if winui::radio_light(ui, o + vec2(23.0, 129.0), !self.letters, "Numbers", true).clicked()
            && self.letters
        {
            self.letters = false;
            self.from = "0".into();
            self.to = "10".into();
        }
        // i18n: the * in the address runs through letters
        if winui::radio_light(ui, o + vec2(138.0, 129.0), self.letters, "Letters", true).clicked()
            && !self.letters
        {
            self.letters = true;
            self.from = "a".into();
            self.to = "z".into();
        }
        // i18n: label before the first value of the range (an input box follows)
        text(
            &p,
            o.x + 17.0,
            o.y + 173.0,
            crate::i18n::t("From:"),
            LIGHT_TEXT,
        );
        self.spin(ui, o, 80.0, 115.0, "from");
        // i18n: label before the last value of the range (an input box follows)
        text(
            &p,
            o.x + 161.0,
            o.y + 173.0,
            crate::i18n::t("To:"),
            LIGHT_TEXT,
        );
        self.spin(ui, o, 204.0, 239.0, "to");
        // A longer translation starts earlier instead of running into the box (down to just
        // after the To box), cut with "..." past that, whole on hover.
        let size_label = crate::i18n::t("Wildcard size");
        let (rect, cut) = winui::label_into(
            &p,
            o.x + 334.0,
            o.y + 173.0,
            size_label,
            LIGHT_TEXT,
            o.x + 402.0,
            o.x + 270.0,
        );
        if cut {
            winui::full_on_hover(ui, rect, "batch-size", size_label);
        }
        self.spin(ui, o, 402.0, 429.0, "size");

        // Use authorization (the group's title is the checkbox).
        winui::group_box(
            &p,
            r(o, 11.0, 211.0, 453.0, 40.0),
            Some(winui::caption_gap(
                &p,
                o.x + 20.0,
                o.x + 124.0,
                "Use authorization",
                true,
            )),
        );
        winui::checkbox_light(
            ui,
            o + vec2(20.0, 205.0),
            &mut self.auth,
            "Use authorization",
            true,
        );
        let on = self.auth;
        // i18n: noun: the user name for the server (an input box follows)
        embossed(&p, o.x + 85.0, o.y + 236.0, crate::i18n::t("Login"), on);
        winui::edit_light(
            ui,
            r(o, 116.0, 221.0, 117.0, 23.0),
            &mut self.login,
            on,
            false,
        );
        embossed(&p, o.x + 290.0, o.y + 236.0, crate::i18n::t("Password"), on);
        winui::password_light(ui, r(o, 342.0, 221.0, 113.0, 23.0), &mut self.password, on);

        // Preview: first, second and last file.
        let name = |i: Option<&url::Url>| {
            i.map(|u| mdm_engine::naming::file_name(None, u))
                .unwrap_or_default()
        };
        let rows = [
            // i18n: the first file name the pattern gives (its name follows)
            (
                crate::i18n::t("First file:"),
                280.0,
                268.0,
                name(urls.first()),
            ),
            (
                crate::i18n::t("Second file:"),
                301.0,
                289.0,
                name(urls.get(1)),
            ),
            (
                crate::i18n::t("Last file:"),
                337.0,
                323.0,
                if urls.len() > 1 {
                    name(urls.last())
                } else {
                    String::new()
                },
            ),
        ];
        for (label, base, y, value) in rows {
            text(&p, o.x + 11.0, o.y + base, label, LIGHT_TEXT);
            let rc = r(o, 102.0, y, 362.0, 18.0);
            preview(&p, rc);
            // An address, from the field's leading edge (an English pen in its English rect).
            let e = winui::rect(rc);
            let value = crate::i18n::ltr(&value);
            let (rect, cut) = winui::text_fit(
                &p,
                e.left() + 4.0,
                e.top() + 13.0,
                &value,
                LIGHT_TEXT,
                e.right() - 3.0,
            );
            if cut {
                winui::full_on_hover(ui, rect, ("batch-preview", y as i32), &value);
            }
        }
        text(&p, o.x + 23.0, o.y + 317.0, "...", LIGHT_TEXT);
        if urls.len() > 2 {
            // i18n: how many files the pattern gives in all
            let more = crate::i18n::tn(urls.len() as u64, "{n} file", "{n} files");
            text(&p, o.x + 102.0, o.y + 317.0, &more, LIGHT_TEXT);
        }

        let valid = !urls.is_empty();
        let ok = winui::button_light(
            ui,
            r(o, 274.0, 354.0, 73.0, 21.0),
            "OK",
            if valid {
                ButtonKind::Default
            } else {
                ButtonKind::Disabled
            },
            false,
        )
        .clicked();
        let cancel = winui::button_light(
            ui,
            r(o, 372.0, 354.0, 73.0, 21.0),
            "Cancel",
            ButtonKind::Normal,
            false,
        )
        .clicked();
        if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            Outcome::Closed
        } else if valid && (ok || ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            Outcome::Done(())
        } else {
            Outcome::Open
        }
    }

    /// An edit with its up-down buddy (`x` edit, `spin_x` buddy), 26 px tall.
    fn spin(&mut self, ui: &mut Ui, o: Pos2, x: f32, spin_x: f32, which: &str) {
        let letters = self.letters && which != "size";
        let value = match which {
            "from" => &mut self.from,
            "to" => &mut self.to,
            _ => &mut self.size,
        };
        winui::edit_light(ui, r(o, x, 154.0, spin_x - x, 26.0), value, true, false);
        if letters {
            value.retain(|c| c.is_ascii_alphabetic());
            value.truncate(1);
        } else {
            value.retain(|c| c.is_ascii_digit());
            value.truncate(if which == "size" { 2 } else { 9 });
        }
        let step = winui::updown_light(
            ui,
            r(o, spin_x, 154.0, 18.0, 26.0),
            true,
            &format!("batch-{which}"),
        );
        if step != 0 {
            *value = if letters {
                let c = value.chars().next().unwrap_or('a');
                let n = (c as u8 as i32 + step).clamp(b'a' as i32, b'z' as i32) as u8;
                (n as char).to_string()
            } else {
                let n = value.parse::<i64>().unwrap_or(0) + step as i64;
                let min = if which == "size" { 1 } else { 0 };
                n.max(min).to_string()
            };
        }
    }
}

/// A classic disabled static: `#a0a0a0` with a white copy at (+1, +1).
fn embossed(p: &egui::Painter, x: f32, base: f32, s: &str, enabled: bool) {
    if enabled {
        text(p, x, base, s, LIGHT_TEXT);
    } else {
        text(p, x + 1.0, base + 1.0, s, egui::Color32::WHITE);
        text(p, x, base, s, winui::rgb_hex(0xa0a0a0));
    }
}

/// A sunken preview field (WS_EX_STATICEDGE): `#a0a0a0` top/left, white bottom/right.
fn preview(p: &egui::Painter, rc: egui::Rect) {
    let (l, t, rr, b) = (rc.left(), rc.top(), rc.right() - 1.0, rc.bottom() - 1.0);
    winui::fill(p, l, t, rr, b, LIGHT_BG);
    winui::fill(p, l, t, rr, t, winui::rgb_hex(0xa0a0a0));
    winui::fill(p, l, t, l, b, winui::rgb_hex(0xa0a0a0));
    winui::fill(p, l, b, rr, b, egui::Color32::WHITE);
    winui::fill(p, rr, t, rr, b, egui::Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(address: &str) -> Batch {
        Batch {
            address: address.into(),
            ..Batch::new(false)
        }
    }

    #[test]
    fn numbers_with_a_wildcard_size() {
        let mut b = batch("http://x/img*.jpg");
        b.from = "1".into();
        b.to = "100".into();
        b.size = "3".into();
        let urls = b.expand();
        assert_eq!(urls.len(), 100);
        assert_eq!(urls[0].as_str(), "http://x/img001.jpg");
        assert_eq!(urls[99].as_str(), "http://x/img100.jpg");
    }

    #[test]
    fn letters_and_invalid_patterns() {
        let mut b = batch("http://x/part-*.rar");
        b.letters = true;
        b.from = "a".into();
        b.to = "c".into();
        let urls: Vec<String> = b.expand().iter().map(|u| u.to_string()).collect();
        assert_eq!(
            urls,
            [
                "http://x/part-a.rar",
                "http://x/part-b.rar",
                "http://x/part-c.rar"
            ]
        );
        assert!(batch("http://x/no-asterisk.jpg").expand().is_empty());
        assert!(
            batch("http://x/*/*.jpg").expand().is_empty(),
            "one asterisk only"
        );
        assert_eq!(
            batch("ftp://x/*.jpg").expand().len(),
            11,
            "ftp addresses too"
        );
    }
}

//! Tasks → Export / Import (D-021): the list's addresses to a text file or an IDM export file
//! (`.ef2`), and downloads from either. IDM's import finds every web address in a file and shows
//! them to check before they are added; so does ours. An `.ef2` file holds one block per
//! download — `<`, the address, `referer: …` / `cookie: …` / `User-Agent: …` lines, `>` — so a
//! list moves between IDM and MDM with its request context.

use egui::{Pos2, Rect, Sense, pos2, vec2};

use crate::dialogs::{Outcome, window};
use crate::model::Item;
use crate::winui::{self, ButtonKind, LIGHT_BG, LIGHT_PAGE, LIGHT_TEXT, r};

/// A download read from a file: its address and what the browser sent with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    pub url: String,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

/// One address per line.
pub fn export_text(items: &[Item]) -> String {
    items.iter().map(|i| format!("{}\r\n", i.url)).collect()
}

/// IDM's export format.
pub fn export_ef2(items: &[Item]) -> String {
    let mut out = String::new();
    for i in items {
        out.push_str("<\r\n");
        out.push_str(&format!("{}\r\n", i.url));
        let c = &i.context;
        if let Some(r) = c.referer.as_deref().filter(|r| !r.is_empty()) {
            out.push_str(&format!("referer: {r}\r\n"));
        }
        if let Some(k) = c.cookies.as_deref().filter(|k| !k.is_empty()) {
            out.push_str(&format!("cookie: {k}\r\n"));
        }
        if let Some(u) = c.user_agent.as_deref().filter(|u| !u.is_empty()) {
            out.push_str(&format!("User-Agent: {u}\r\n"));
        }
        out.push_str(">\r\n");
    }
    out
}

/// The downloads in a file: `.ef2` blocks when it has them, else every http(s) and ftp(s)
/// address in the text (each once, in order).
pub fn import(text: &str) -> Vec<Imported> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let blocks = ef2_blocks(text);
    if !blocks.is_empty() {
        return blocks;
    }
    let mut seen = std::collections::HashSet::new();
    addresses(text)
        .into_iter()
        .filter(|u| seen.insert(u.clone()))
        .map(|url| Imported {
            url,
            ..Default::default()
        })
        .collect()
}

fn ef2_blocks(text: &str) -> Vec<Imported> {
    let mut out = Vec::new();
    let mut cur: Option<Imported> = None;
    for line in text.lines().map(str::trim) {
        match line {
            "<" => cur = Some(Imported::default()),
            ">" => {
                if let Some(d) = cur.take().filter(|d| downloadable(&d.url)) {
                    out.push(d);
                }
            }
            _ => {
                let Some(d) = cur.as_mut() else { continue };
                if d.url.is_empty() {
                    d.url = line.to_owned();
                } else if let Some((k, v)) = line.split_once(':') {
                    let v = v.trim().to_owned();
                    match k.trim().to_ascii_lowercase().as_str() {
                        "referer" => d.referer = Some(v),
                        "cookie" => d.cookies = Some(v),
                        "user-agent" => d.user_agent = Some(v),
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

fn downloadable(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| mdm_engine::supported(&u))
}

/// Every http(s) and ftp(s) address in free text (a page saved as text, a list, a mail).
fn addresses(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rel) = text[at..].find("://") {
        let sep = at + rel;
        // The scheme: the letters right before "://".
        let start = text[..sep]
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_ascii_alphabetic())
            .map_or(0, |(i, c)| i + c.len_utf8());
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`'))
            .unwrap_or(rest.len());
        let cand = rest[..end].trim_end_matches(['.', ',', ';', ')', ']', '}']);
        if downloadable(cand) {
            out.push(cand.to_owned());
        }
        at = (start + end).max(sep + 3);
    }
    out
}

/// "Import downloads": the addresses found, each with a checkbox; OK adds the ticked ones.
pub struct ImportDialog {
    found: Vec<(Imported, bool)>,
    first: usize,
    source: String,
}

impl ImportDialog {
    pub fn new(found: Vec<Imported>, source: String) -> Self {
        ImportDialog {
            found: found.into_iter().map(|d| (d, true)).collect(),
            first: 0,
            source,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<Vec<Imported>> {
        const ROWS: usize = 14;
        const ROW_H: f32 = 18.0;
        let out = window(
            ctx,
            "import",
            "Import downloads",
            [560.0, 360.0],
            false,
            LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let line = crate::i18n::fmt(
                    crate::i18n::t("Web addresses found in {file}:"),
                    &[("file", &self.source)],
                );
                let (rect, cut) =
                    winui::text_fit(&p, o.x + 11.0, o.y + 20.0, &line, LIGHT_TEXT(), o.x + 549.0);
                if cut {
                    winui::full_on_hover(ui, rect, "import-source", &line);
                }
                // English, as the rows are laid out; made where it is on screen.
                let list = Rect::from_min_size(
                    pos2(o.x + 11.0, o.y + 30.0),
                    vec2(538.0, ROWS as f32 * ROW_H + 4.0),
                );
                let shown = winui::rect(list);
                p.rect_filled(shown, 0.0, LIGHT_PAGE());
                p.rect_stroke(
                    shown,
                    0.0,
                    egui::Stroke::new(1.0, winui::FIELD_EDGE()),
                    egui::StrokeKind::Inside,
                );
                // The wheel scrolls by whole rows.
                let resp = ui.interact(shown, ui.scope_id().with("import-list"), Sense::hover());
                if resp.hovered() {
                    let dy = ui.input(|i| i.smooth_scroll_delta.y);
                    if dy < 0.0 {
                        self.first = (self.first + 1).min(self.found.len().saturating_sub(ROWS));
                    } else if dy > 0.0 {
                        self.first = self.first.saturating_sub(1);
                    }
                }
                let clip = p.with_clip_rect(shown.shrink(2.0));
                for (k, (d, on)) in self
                    .found
                    .iter_mut()
                    .enumerate()
                    .skip(self.first)
                    .take(ROWS)
                {
                    let y = list.top() + 2.0 + (k - self.first) as f32 * ROW_H;
                    winui::checkbox_light(ui, Pos2::new(list.left() + 6.0, y + 2.0), on, "", true);
                    let url = crate::i18n::ltr(&d.url);
                    let (rect, cut) = winui::text_fit(
                        &clip,
                        list.left() + 26.0,
                        y + 13.0,
                        &url,
                        LIGHT_TEXT(),
                        list.right() - 6.0,
                    );
                    if cut {
                        winui::full_on_hover(ui, rect, ("import-row", k), &d.url);
                    }
                }
                let ticked = self.found.iter().filter(|(_, on)| *on).count();
                let count = crate::i18n::fmt(
                    crate::i18n::t("{n} of {total} selected"),
                    &[
                        ("n", &crate::i18n::integer(ticked as u64)),
                        ("total", &crate::i18n::integer(self.found.len() as u64)),
                    ],
                );
                winui::text(&p, o.x + 11.0, list.bottom() + 20.0, &count, LIGHT_TEXT());
                if winui::button_light(
                    ui,
                    r(o, 196.0, 296.0, 82.0, 21.0),
                    "Select all",
                    ButtonKind::Normal,
                    false,
                )
                .clicked()
                {
                    self.found.iter_mut().for_each(|(_, on)| *on = true);
                }
                if winui::button_light(
                    ui,
                    r(o, 286.0, 296.0, 82.0, 21.0),
                    "Select none",
                    ButtonKind::Normal,
                    false,
                )
                .clicked()
                {
                    self.found.iter_mut().for_each(|(_, on)| *on = false);
                }
                let ok = winui::button_light(
                    ui,
                    r(o, 395.0, 328.0, 73.0, 21.0),
                    "OK",
                    if ticked > 0 {
                        ButtonKind::Default
                    } else {
                        ButtonKind::Disabled
                    },
                    false,
                )
                .clicked();
                let cancel = winui::button_light(
                    ui,
                    r(o, 476.0, 328.0, 73.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    Outcome::Closed
                } else if ok && ticked > 0 {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        );
        match out {
            Outcome::Open => Outcome::Open,
            Outcome::Closed => Outcome::Closed,
            Outcome::Done(()) => Outcome::Done(
                self.found
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(d, _)| d.clone())
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idm_export_files_round_trip() {
        let mut a = Item::new(
            1,
            "https://x.org/a.zip".parse().unwrap(),
            "C:\\d\\a.zip".into(),
            crate::model::Category::Compressed,
        );
        a.context.referer = Some("https://x.org/".into());
        a.context.cookies = Some("sid=1".into());
        let b = Item::new(
            2,
            "https://y.org/b.iso".parse().unwrap(),
            "C:\\d\\b.iso".into(),
            crate::model::Category::General,
        );
        let text = export_ef2(&[a, b]);
        assert_eq!(
            import(&text),
            vec![
                Imported {
                    url: "https://x.org/a.zip".into(),
                    referer: Some("https://x.org/".into()),
                    cookies: Some("sid=1".into()),
                    user_agent: None,
                },
                Imported {
                    url: "https://y.org/b.iso".into(),
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn addresses_are_found_in_any_text() {
        let text = "See https://a.org/x.zip, and (http://b.org/y.rar). Again: https://a.org/x.zip\n\
                    <a href=\"https://c.org/z.7z\">z</a> ftp://f.org/f mailto://x httpnot";
        let urls: Vec<String> = import(text).into_iter().map(|d| d.url).collect();
        assert_eq!(
            urls,
            [
                "https://a.org/x.zip",
                "http://b.org/y.rar",
                "https://c.org/z.7z",
                "ftp://f.org/f"
            ]
        );
        assert_eq!(export_text(&[]), "");
    }
}

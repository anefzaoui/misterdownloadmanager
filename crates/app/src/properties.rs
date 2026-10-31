//! File Properties (double-click / right-click → Properties;
//! notes/ui-spec/scheduler-and-misc.md §5). Light in both modes, as IDM's.

use std::path::{Path, PathBuf};

use egui::{Pos2, Ui, vec2};

use crate::chrome::{IconKey, ShellIcons};
use crate::dialogs::{Outcome, window};
use crate::model::{Item, Status};
use crate::winui::{self, ButtonKind, LIGHT_BG, LIGHT_TEXT, r, text};

/// What OK changes.
pub struct Changes {
    pub id: u64,
    pub description: String,
    pub referer: Option<String>,
    pub login: Option<(String, String)>,
    /// A new address for a download that isn't finished (Refresh download address).
    pub address: Option<url::Url>,
    /// Move the file (finished) or its destination (not started) into this folder.
    pub move_to: Option<PathBuf>,
}

pub struct Properties {
    pub id: u64,
    address: String,
    description: String,
    referer: String,
    login: String,
    password: String,
    move_to: Option<PathBuf>,
    error: Option<String>,
}

impl Properties {
    pub fn new(item: &Item) -> Self {
        let (login, password) = item.login.clone().unwrap_or_default();
        Properties {
            id: item.id,
            address: item.url.to_string(),
            description: item.description.clone(),
            referer: item.context.referer.clone().unwrap_or_default(),
            login,
            password,
            move_to: None,
            error: None,
        }
    }

    /// `item` is read every frame, so status and size stay live.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        icons: &mut ShellIcons,
        item: &Item,
    ) -> Outcome<Changes> {
        let outcome = window(
            ctx,
            &format!("properties-{}", self.id),
            "File Properties",
            [477.0, 400.0],
            false,
            LIGHT_BG(),
            |ui, o| self.body(ui, o, icons, item),
        );
        match outcome {
            Outcome::Done(()) => {
                let address = url::Url::parse(self.address.trim())
                    .ok()
                    .filter(|u| *u != item.url);
                let referer = self.referer.trim();
                Outcome::Done(Changes {
                    id: self.id,
                    description: self.description.clone(),
                    referer: (!referer.is_empty()).then(|| referer.to_owned()),
                    login: (!self.login.is_empty())
                        .then(|| (self.login.clone(), self.password.clone())),
                    address,
                    move_to: self.move_to.take(),
                })
            }
            Outcome::Closed => Outcome::Closed,
            Outcome::Open => Outcome::Open,
        }
    }

    fn body(&mut self, ui: &mut Ui, o: Pos2, icons: &mut ShellIcons, item: &Item) -> Outcome<()> {
        let p = ui.painter().clone();
        let complete = item.status == Status::Complete;
        let running = item.live.is_some();
        // The address can be replaced while the download isn't finished or running.
        let editable = !complete && !running;
        let ext = Path::new(&item.file_name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        icons.paint(ui, IconKey::ExtLarge(ext.clone()), o + vec2(11.0, 11.0));
        let fit = |ui: &mut Ui, x: f32, base: f32, s: &str| {
            let (rect, cut) =
                winui::text_fit(&p, o.x + x, o.y + base, s, LIGHT_TEXT(), o.x + 466.0);
            winui::copyable(ui, rect, ("props", base as i32), s, cut);
        };
        fit(ui, 87.0, 31.0, &item.file_name);
        winui::etched_h(&p, o.x + 11.0, o.y + 50.0, 456.0);
        let size = item.size.map_or_else(String::new, |s| {
            crate::i18n::fmt(
                crate::i18n::t("{size} ({bytes} Bytes)"),
                &[
                    ("size", &crate::dialogs::dialog_size(s)),
                    ("bytes", &crate::i18n::integer(s)),
                ],
            )
        });
        // The values' column: IDM's 87, or after the widest label when a translation is longer
        // (the boxes keep their right edges).
        let col = [
            "Type:",
            "Status:",
            "Size:",
            "Save To:",
            "Address:",
            "Description:",
            "Referer:",
        ]
        .iter()
        .map(|l| 11.0 + winui::text_width(&p, crate::i18n::t(l)).ceil() + 6.0)
        .fold(87.0_f32, f32::max);
        for (label, base, value) in [
            // i18n: label before the kind of file ("ZIP File")
            (
                crate::i18n::mark("Type:"),
                69.0,
                file_type(&item.file_name, &ext),
            ),
            // i18n: label before the download's state
            (crate::i18n::mark("Status:"), 88.0, item.status_text()),
            (crate::i18n::mark("Size:"), 108.0, size),
        ] {
            text(
                &p,
                o.x + 11.0,
                o.y + base,
                crate::i18n::t(label),
                LIGHT_TEXT(),
            );
            fit(ui, col, base, &value);
        }

        text(
            &p,
            o.x + 11.0,
            o.y + 135.0,
            crate::i18n::t("Save To:"),
            LIGHT_TEXT(),
        );
        let dest = match &self.move_to {
            Some(dir) => dir.join(&item.file_name),
            None => item.dest.clone(),
        };
        readonly(
            ui,
            r(o, col, 120.0, 383.0 - col, 23.0),
            &dest.display().to_string(),
        );
        // Moving: a finished file is moved; one that hasn't started gets a new destination.
        let movable = !running && (complete || item.downloaded == 0);
        let pick = winui::button_light(
            ui,
            r(o, 393.0, 121.0, 73.0, 21.0),
            // i18n: verb: move the file to another folder
            "Move",
            if movable {
                ButtonKind::Normal
            } else {
                ButtonKind::Disabled
            },
            false,
        );
        let pick = winui::why_disabled(
            pick,
            !movable,
            if running {
                // i18n: tooltip of the greyed Move button in File Properties
                crate::i18n::mark("Pause the download first")
            } else {
                // i18n: tooltip of the greyed Move button in File Properties
                crate::i18n::mark("Only a finished download, or one not started yet, can be moved")
            },
        )
        .clicked();
        if pick
            && movable
            && let Some(dir) = rfd::FileDialog::new()
                .set_title(crate::i18n::t("Move to folder"))
                .set_directory(item.dest.parent().unwrap_or(Path::new(".")))
                .pick_folder()
        {
            self.move_to = Some(dir);
        }

        text(
            &p,
            o.x + 11.0,
            o.y + 166.0,
            crate::i18n::t("Address:"),
            LIGHT_TEXT(),
        );
        if editable {
            winui::edit_light(
                ui,
                r(o, col, 151.0, 467.0 - col, 23.0),
                &mut self.address,
                true,
                false,
            );
        } else {
            readonly(ui, r(o, col, 151.0, 467.0 - col, 23.0), &self.address);
        }
        text(
            &p,
            o.x + 11.0,
            o.y + 197.0,
            crate::i18n::t("Description:"),
            LIGHT_TEXT(),
        );
        winui::edit_light(
            ui,
            r(o, col, 182.0, 467.0 - col, 23.0),
            &mut self.description,
            true,
            false,
        );
        winui::etched_h(&p, o.x + 11.0, o.y + 213.0, 456.0);
        text(
            &p,
            o.x + 11.0,
            o.y + 231.0,
            crate::i18n::t("The web page from which this file was obtained:"),
            LIGHT_TEXT(),
        );
        // i18n: the web page is not known
        let page = item
            .context
            .referer
            .as_deref()
            .unwrap_or(crate::i18n::t("Unknown"));
        fit(ui, 11.0, 252.0, page);
        winui::etched_h(&p, o.x + 11.0, o.y + 262.0, 456.0);
        // i18n: HTTP term (sic): the page the download was started from
        text(
            &p,
            o.x + 11.0,
            o.y + 288.0,
            crate::i18n::t("Referer:"),
            LIGHT_TEXT(),
        );
        winui::edit_light(
            ui,
            r(o, col, 271.0, 467.0 - col, 23.0),
            &mut self.referer,
            editable,
            false,
        );
        // i18n: noun: label before the user name
        winui::text_right(
            &p,
            o.x + 110.0,
            o.y + 316.0,
            crate::i18n::t("Login"),
            LIGHT_TEXT(),
        );
        winui::edit_light(
            ui,
            r(o, 120.0, 301.0, 189.0, 21.0),
            &mut self.login,
            editable,
            false,
        );
        winui::text_right(
            &p,
            o.x + 110.0,
            o.y + 343.0,
            crate::i18n::t("Password"),
            LIGHT_TEXT(),
        );
        password(
            ui,
            r(o, 120.0, 330.0, 189.0, 21.0),
            &mut self.password,
            editable,
        );
        winui::etched_h(&p, o.x + 11.0, o.y + 358.0, 456.0);
        if !complete {
            let last = item.last_try.clone().unwrap_or_default();
            let last_try = crate::i18n::fmt(
                // i18n: {last} = the date and time of the last attempt to download
                crate::i18n::t("Last try  {last}"),
                &[("last", &last)],
            );
            fit(ui, 11.0, 376.0, &last_try);
            let result = item.error.clone().unwrap_or_else(|| item.status_text());
            let x1 = o.x + 250.0;
            let (rect, cut) = winui::text_fit(
                &p,
                o.x + 11.0,
                o.y + 395.0,
                &crate::i18n::fmt(
                    // i18n: {result} = how the last attempt ended (a status or an error message)
                    crate::i18n::t("Result: {result}"),
                    &[("result", &result)],
                ),
                LIGHT_TEXT(),
                x1,
            );
            if cut {
                winui::full_on_hover(ui, rect, "props-result", &result);
            }
        }
        if let Some(e) = &self.error {
            winui::fit(
                ui,
                &p,
                o.x + 11.0,
                o.y + 395.0,
                e,
                winui::ERROR(),
                o.x + 250.0,
                "props-error",
            );
        }
        let open = winui::button_light(
            ui,
            r(o, 259.0, 370.0, 73.0, 21.0),
            // i18n: verb: opens the file
            "Open",
            if complete {
                ButtonKind::Normal
            } else {
                ButtonKind::Disabled
            },
            false,
        );
        let open = winui::why_disabled(
            open,
            !complete,
            // i18n: tooltip of the greyed Open button in File Properties
            "The download isn't complete yet",
        )
        .clicked();
        if open && complete {
            let _ = open::that_detached(&item.dest);
        }
        let ok = winui::button_light(
            ui,
            r(o, 393.0, 370.0, 73.0, 21.0),
            "OK",
            ButtonKind::Default,
            false,
        )
        .clicked();
        let address_ok = !editable
            || url::Url::parse(self.address.trim()).is_ok_and(|u| mdm_engine::supported(&u));
        if winui::escape_pressed(ui) {
            Outcome::Closed
        } else if ok || winui::enter_pressed(ui) {
            if address_ok {
                Outcome::Done(())
            } else {
                self.error =
                    Some(crate::i18n::t("The address must be an http(s) or ftp(s) address").into());
                Outcome::Open
            }
        } else {
            Outcome::Open
        }
    }
}

/// Windows' name for the file type ("BIN File", "Compressed (zipped) Folder").
fn file_type(name: &str, ext: &str) -> String {
    #[cfg(windows)]
    if let Some(t) = crate::win32::type_name(name) {
        return t;
    }
    let _ = name;
    if ext.is_empty() {
        // i18n: noun: a file of no known type
        crate::i18n::t("File").into()
    } else {
        crate::i18n::fmt(
            // i18n: a kind of file, by its extension: {ext} = "ZIP", "PDF"…
            crate::i18n::t("{ext} File"),
            &[("ext", &ext.to_ascii_uppercase())],
        )
    }
}

/// A read-only field holding a path or an address (left-to-right data): the shared one, so it
/// can be copied.
fn readonly(ui: &mut Ui, rc: egui::Rect, value: &str) {
    winui::readonly_field(ui, rc, value, LIGHT_BG(), true, ("props", rc.top() as i32));
}

fn password(ui: &mut Ui, rc: egui::Rect, value: &mut String, enabled: bool) {
    winui::password_light_on(ui, rc, value, enabled, LIGHT_BG());
}

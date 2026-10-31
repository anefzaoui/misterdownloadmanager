//! Right-click → Verify checksum… (beyond IDM, D-015): paste the published hash; a finished file
//! is checked at once, an unfinished one when its download ends (the engine verifies it before
//! the final rename). The dialog is ours; it uses the light dialog style.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mdm_engine::verify::Hash;

use crate::dialogs::{Outcome, window};
use crate::winui::{self, ButtonKind, LIGHT_BG, LIGHT_TEXT, r, text};

/// A check of the file: its digest once read, or why it couldn't be.
type Check = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;

pub struct ChecksumDialog {
    pub id: u64,
    text: String,
    file: Option<PathBuf>,
    check: Option<Check>,
    focused: bool,
}

impl ChecksumDialog {
    /// `file`: the finished file (checked now); None for a download still to finish.
    pub fn new(id: u64, known: Option<&Hash>, file: Option<PathBuf>) -> Self {
        ChecksumDialog {
            id,
            text: known.map(Hash::display).unwrap_or_default(),
            file,
            check: None,
            focused: false,
        }
    }

    /// The file's own checksum when the check found it differs (shown, to copy, under the
    /// verdict).
    fn mismatch(&self) -> Option<Hash> {
        let want = Hash::parse(&self.text)?;
        let check = self.check.as_ref()?.lock().unwrap();
        match &*check {
            Some(Ok(got)) if *got != want.value => Some(Hash {
                algo: want.algo,
                value: got.clone(),
            }),
            _ => None,
        }
    }

    /// `Done(hash)`: keep it with the download. Close and the window's × both keep a hash that
    /// was checked.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<Hash> {
        let id = format!("checksum-{}", self.id);
        let got = self.mismatch();
        let extra = if got.is_some() { 30.0 } else { 0.0 };
        let out = window(
            ctx,
            &id,
            "Verify checksum",
            [430.0, 118.0 + extra],
            false,
            LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                text(
                    &p,
                    o.x + 11.0,
                    o.y + 21.0,
                    crate::i18n::t("Expected checksum (MD5, SHA-1, SHA-256, SHA-512 or CRC32):"),
                    LIGHT_TEXT(),
                );
                let edit = winui::edit_light(
                    ui,
                    r(o, 11.0, 31.0, 408.0, 23.0),
                    &mut self.text,
                    true,
                    false,
                );
                if !self.focused {
                    edit.request_focus();
                    self.focused = true;
                }
                let hash = Hash::parse(&self.text);
                if edit.changed() {
                    self.check = None;
                }
                let (line, color) = match (&hash, &self.check) {
                    (None, _) if !self.text.trim().is_empty() => (
                        crate::i18n::t("Not a checksum this can read").to_owned(),
                        winui::ERROR(),
                    ),
                    (None, _) => (String::new(), LIGHT_TEXT()),
                    (Some(h), Some(c)) => match &*c.lock().unwrap() {
                        None => (
                            crate::i18n::t("Checking...").to_owned(),
                            winui::LIGHT_DISABLED(),
                        ),
                        Some(Ok(v)) if *v == h.value => (
                            crate::i18n::fmt(
                                crate::i18n::t("The file matches ({algorithm})"),
                                &[("algorithm", h.algo.name())],
                            ),
                            winui::SUCCESS(),
                        ),
                        Some(Ok(_)) => (
                            crate::i18n::fmt(
                                crate::i18n::t("The file does NOT match ({algorithm})"),
                                &[("algorithm", h.algo.name())],
                            ),
                            winui::ERROR(),
                        ),
                        Some(Err(e)) => (
                            crate::i18n::fmt(
                                crate::i18n::t("Cannot read the file: {error}"),
                                &[("error", &e.to_string())],
                            ),
                            winui::ERROR(),
                        ),
                    },
                    (Some(h), None) if self.file.is_none() => (
                        crate::i18n::fmt(
                            crate::i18n::t("{algorithm}: checked when the download finishes"),
                            &[("algorithm", h.algo.name())],
                        ),
                        LIGHT_TEXT(),
                    ),
                    (Some(h), None) => (
                        crate::i18n::fmt(
                            crate::i18n::t("{algorithm} checksum"),
                            &[("algorithm", h.algo.name())],
                        ),
                        LIGHT_TEXT(),
                    ),
                };
                winui::fit(
                    ui,
                    &p,
                    o.x + 11.0,
                    o.y + 72.0,
                    &line,
                    color,
                    o.x + 419.0,
                    "checksum-status",
                );
                if let Some(got) = &got {
                    winui::readonly_field(
                        ui,
                        r(o, 11.0, 80.0, 408.0, 23.0),
                        &got.display(),
                        LIGHT_BG(),
                        true,
                        "checksum-got",
                    )
                    .on_hover_text(crate::i18n::t(
                        // i18n: tooltip of the box showing the checksum the file really has
                        "The file's own checksum",
                    ));
                }
                let o = o + egui::vec2(0.0, extra);
                // Verify waits while a check runs.
                let checking = self
                    .check
                    .as_ref()
                    .is_some_and(|c| c.lock().unwrap().is_none());
                let valid = hash.is_some() && !checking;
                let kind = if valid {
                    ButtonKind::Default
                } else {
                    ButtonKind::Disabled
                };
                let (primary, secondary) = if self.file.is_some() {
                    // i18n: both verbs: Verify = check the file now; Close = close the window
                    (crate::i18n::mark("Verify"), crate::i18n::mark("Close"))
                } else {
                    (crate::i18n::mark("OK"), crate::i18n::mark("Cancel"))
                };
                let go =
                    winui::button_light(ui, r(o, 265.0, 87.0, 73.0, 21.0), primary, kind, false)
                        .clicked();
                let close = winui::button_light(
                    ui,
                    r(o, 346.0, 87.0, 73.0, 21.0),
                    secondary,
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                let enter = winui::enter_pressed(ui);
                if close || winui::escape_pressed(ui) {
                    // A checked file keeps its hash with the download.
                    match (hash, &self.check) {
                        (Some(h), Some(_)) => Outcome::Done(h),
                        _ => Outcome::Closed,
                    }
                } else if let (Some(h), true) = (hash, go || (enter && valid)) {
                    match &self.file {
                        None => Outcome::Done(h),
                        Some(file) => {
                            let slot: Check = Arc::new(Mutex::new(None));
                            let (out, file, ctx) = (slot.clone(), file.clone(), ui.ctx().clone());
                            std::thread::spawn(move || {
                                let r = mdm_engine::verify::digest(&file, h.algo)
                                    .map_err(|e| e.to_string());
                                *out.lock().unwrap() = Some(r);
                                ctx.request_repaint();
                            });
                            self.check = Some(slot);
                            Outcome::Open
                        }
                    }
                } else {
                    Outcome::Open
                }
            },
        );
        // The window's ×: as Close.
        match (out, Hash::parse(&self.text), &self.check) {
            (Outcome::Closed, Some(h), Some(_)) => Outcome::Done(h),
            (out, ..) => out,
        }
    }
}

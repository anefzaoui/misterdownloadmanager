//! Options (Downloads → Options, the toolbar's Options): IDM's configuration dialog, nine pages
//! in a two-row tab control (notes/ui-spec/options-dialog.md). Light in both modes, as IDM's.
//! It edits a copy of the settings: OK applies it, the close box and Escape discard it.

use egui::{Color32, Pos2, Rect, Sense, Ui, pos2, vec2};

use crate::dialogs::{Outcome, window};
use crate::icons::Icon;
use crate::model::Category;
use crate::regional::DateOrder;
use crate::settings::{self, Settings};
// Pages are laid out in English pixels: `fill` makes each one where it is on screen, `text`
// starts at an English pen, and a widget given a screen rect (from `r`) turns it back into its
// English rect (`winui::rect` both ways) before laying out its inside.
use crate::winui::{
    self, ButtonKind, LIGHT_BG, LIGHT_DISABLED, LIGHT_PAGE, LIGHT_TEXT, fill_l as fill, r, text,
};

/// IDM's "Default max. conn. number" choices.
const CONNECTIONS: [u32; 7] = [1, 2, 4, 8, 16, 24, 32];
pub const DUPLICATE: [&str; 4] = [
    crate::i18n::mark("Show a dialog and ask what to do."),
    crate::i18n::mark("Add the duplicate with a numbered file name"),
    crate::i18n::mark("Add the duplicate and overwrite the existing file"),
    crate::i18n::mark(
        "If existing file is complete, show download complete dialog; otherwise resume it.",
    ),
];
const SOUND_EVENTS: [&str; 4] = [
    crate::i18n::mark("Download complete"),
    crate::i18n::mark("Download failed"),
    crate::i18n::mark("Queue processing started"),
    crate::i18n::mark("Queue processing stopped/finished"),
];

/// A small entry dialog opened from a page.
enum Sub {
    /// Sites Logins → New / Edit: (row being edited, site/path, user, password).
    Login(Option<usize>, String, String, String),
    /// Connection → New / Edit exception: (row, server, number).
    Exception(Option<usize>, String, String),
    /// General → Download panels → Edit...: (the button over videos on, the sites it stays off).
    Panels(bool, String),
}

pub struct OptionsDialog {
    s: Settings,
    /// The settings it opened with: OK applies only what changed from them.
    pub from: Settings,
    page: usize,
    /// The browsers check list's scroll and selection.
    browser_rows: winui::RowList,
    category: usize,
    cat_dir: String,
    mb: String,
    hours: String,
    /// The exceptions' and site logins' lists: scroll and selection.
    exc_rows: winui::RowList,
    login_rows: winui::RowList,
    sound_sel: Option<usize>,
    sub: Option<Sub>,
    /// Save to → New / Edit...: the category editor (D-021).
    cat_edit: Option<crate::categories::CategoryDialog>,
    /// Connection caps the engine learned, shown as "(auto)" rows under Exceptions.
    learned: Vec<(String, u32)>,
}

impl OptionsDialog {
    /// Open on tab `page` (the numbers in `tabs`: 0 General … 4 Connection, 5 Proxy / Socks,
    /// 6 Sites Logins, 8 Sounds); for `/show options:<tab>`.
    pub fn on_page(mut self, page: usize) -> Self {
        self.page = page;
        self
    }

    /// With General → Download panels → Edit... open (`/show options:panels`).
    pub fn with_panels(mut self) -> Self {
        self.sub = Some(Sub::Panels(
            self.s.video_panel,
            self.s.video_panel_sites.clone(),
        ));
        self
    }

    pub fn new(
        settings: &Settings,
        downloads: &std::path::Path,
        learned: Vec<(String, u32)>,
    ) -> Self {
        let s = settings.clone();
        OptionsDialog {
            from: s.clone(),
            browser_rows: Default::default(),
            cat_dir: display_dir(&s.category_dir(Category::General, downloads)),
            mb: s.limits_mb.to_string(),
            hours: s.limits_hours.to_string(),
            s,
            page: 0,
            category: 0,
            exc_rows: Default::default(),
            login_rows: Default::default(),
            sound_sel: None,
            sub: None,
            cat_edit: None,
            learned,
        }
    }

    /// `Done(settings)` on OK.
    pub fn show(&mut self, ctx: &egui::Context, downloads: &std::path::Path) -> Outcome<Settings> {
        let modal = self.sub.is_some() || self.cat_edit.is_some();
        let outcome = window(
            ctx,
            "options",
            "Mister Download Manager Configuration",
            [430.0, 506.0],
            false,
            LIGHT_BG(),
            |ui, o| {
                // Ctrl+Tab through the pages in the tabs' order (there is no page 7).
                const ORDER: [usize; 9] = [0, 1, 2, 3, 4, 5, 6, 8, 9];
                let at = ORDER.iter().position(|p| *p == self.page).unwrap_or(0);
                let by_key = winui::tab_keys(ui, ORDER.len(), at).map(|i| ORDER[i]);
                if let Some(p) = tabs(ui, o, self.page).or(by_key) {
                    self.page = p;
                }
                match self.page {
                    0 => self.general(ui, o),
                    1 => self.file_types(ui, o),
                    2 => self.save_to(ui, o, downloads),
                    3 => self.downloads(ui, o),
                    4 => self.connection(ui, o),
                    5 => self.proxy(ui, o),
                    6 => self.logins(ui, o),
                    8 => self.sounds(ui, o),
                    _ => self.regional(ui, o),
                }
                let ok = winui::button_light(
                    ui,
                    r(o, 269.0, 477.0, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                )
                .clicked();
                if winui::button_light(
                    ui,
                    r(o, 350.0, 477.0, 73.0, 21.0),
                    // i18n: noun: button that opens the help
                    "Help",
                    ButtonKind::Normal,
                    false,
                )
                .clicked()
                {
                    crate::help::open_page("help/#options");
                }
                if modal {
                    Outcome::Open
                } else if winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if ok || winui::enter_pressed(ui) {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        );
        if self.sub.is_some() {
            self.sub_dialog(ctx);
        }
        if let Some(d) = &mut self.cat_edit {
            match d.show(ctx, &crate::categories::names()) {
                Outcome::Open => {}
                Outcome::Closed => self.cat_edit = None,
                Outcome::Done(e) => {
                    self.cat_edit = None;
                    let cat = crate::categories::apply(&mut self.s, e, downloads);
                    // Show the category just made or changed.
                    if let Some(i) = Category::all().iter().position(|c| *c == cat) {
                        self.category = i;
                    }
                    self.cat_dir = display_dir(&self.s.category_dir(cat, downloads));
                }
            }
        }
        match outcome {
            Outcome::Done(()) => {
                let mut s = self.s.clone();
                s.limits_mb = self.mb.parse().unwrap_or(200).max(1);
                s.limits_hours = self.hours.parse().unwrap_or(5).max(1);
                Outcome::Done(s)
            }
            Outcome::Closed => Outcome::Closed,
            Outcome::Open => Outcome::Open,
        }
    }

    // ---- pages --------------------------------------------------------------------------

    fn general(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageGeneral,
            "Browser/System Integration",
            false,
            260.0,
            63.0,
            405.0,
        );
        let portable = mdm_ipc::portable_dir().is_some();
        let s = &mut self.s;
        if portable {
            s.launch_on_startup = false;
        }
        let checks: [(f32, &mut bool, &str); 5] = [
            (
                94.0,
                &mut s.launch_on_startup,
                crate::i18n::mark("Launch Mister Download Manager on startup"),
            ),
            (
                113.0,
                &mut s.clipboard_autostart,
                crate::i18n::mark("Offer to download links copied to the clipboard"),
            ),
            (
                133.0,
                &mut s.mv2_extensions,
                crate::i18n::mark("Extend the availability of Manifest 2 Chrome extensions"),
            ),
            (
                152.0,
                &mut s.advanced_integration,
                crate::i18n::mark("Use advanced browser integration"),
            ),
            // MDM's: turns back on the delete confirmation that "Don't show this dialog again"
            // turned off. The browsers' group below starts a row lower for it.
            (
                172.0,
                &mut s.confirm_delete,
                crate::i18n::mark("Confirm before deleting downloads"),
            ),
        ];
        for (i, (y, v, label)) in checks.into_iter().enumerate() {
            // Portable: nothing is written to the registry, so no start with Windows. Manifest 2
            // extensions: an IDM-era Chrome policy with no meaning for MDM's extension (MV3):
            // shown as IDM has it, greyed (D-021).
            let enabled = !(portable && i == 0) && i != 2;
            winui::checkbox_light(ui, o + vec2(30.0, y), v, label, enabled);
        }
        group(
            ui,
            o,
            16.0,
            196.0,
            392.0,
            198.0,
            "Capture downloads from the following browsers:",
        );
        // Check list: one row per browser in the shared row list (Space ticks the selected
        // one; the box and its name tick it too).
        let browsers = &mut s.browsers;
        let ev = winui::row_list(
            ui,
            "browsers",
            r(o, 30.0, 206.0, 362.0, 116.0),
            browsers.len(),
            17.0,
            &mut self.browser_rows,
            |ui, _, i, row, _| {
                let (name, on) = &mut browsers[i];
                winui::checkbox_light(ui, pos2(row.left() + 5.0, row.top() + 2.0), on, name, true);
            },
        );
        if let Some(i) = ev.space {
            browsers[i].1 = !browsers[i].1;
        }
        // Adding browsers, keys, and the browser menu / panel editors are the extension's
        // settings (M8): shown as IDM has them, not yet available.
        let off = |ui: &mut Ui, rc: Rect, label: &str| {
            winui::button_light(ui, rc, label, ButtonKind::Disabled, false);
        };
        off(
            ui,
            r(o, 286.0, 328.0, 106.0, 21.0),
            crate::i18n::mark("Add browser..."),
        );
        winui::etched_h(&p, o.x + 34.0, o.y + 356.0, 357.0);
        let label = crate::i18n::t("Customize keys to prevent or force downloading with MDM");
        // Inside the browsers' group box: it starts no further out than the group's frame.
        let (rect, cut) = winui::text_right_within(
            &p,
            o.x + 313.0,
            o.y + 379.0,
            label,
            LIGHT_DISABLED(),
            o.x + 22.0,
        );
        if cut {
            winui::full_on_hover(ui, rect, ("general-right", 379), label);
        }
        off(
            ui,
            r(o, 319.0, 365.0, 73.0, 21.0),
            crate::i18n::mark("Keys..."),
        );
        let label = crate::i18n::t("Customize MDM menu items in context menu of browsers");
        let (rect, cut) = winui::text_right(&p, o.x + 313.0, o.y + 419.0, label, LIGHT_DISABLED());
        if cut {
            winui::full_on_hover(ui, rect, ("general-right", 419), label);
        }
        // i18n: verb: edit the item
        off(ui, r(o, 319.0, 406.0, 73.0, 21.0), "Edit...");
        // The download button over videos in the browsers (our own panel, Sub::Panels).
        let label = crate::i18n::t("Customize MDM Download panels in browsers");
        let (rect, cut) = winui::text_right(&p, o.x + 313.0, o.y + 452.0, label, LIGHT_TEXT());
        if cut {
            winui::full_on_hover(ui, rect, ("general-right", 452), label);
        }
        if winui::button_light(
            ui,
            r(o, 319.0, 438.0, 73.0, 21.0),
            // i18n: verb: edit the item
            "Edit...",
            ButtonKind::Normal,
            false,
        )
        .clicked()
        {
            self.sub = Some(Sub::Panels(
                self.s.video_panel,
                self.s.video_panel_sites.clone(),
            ));
        }
    }

    fn file_types(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageFileTypes,
            "Downloaded file types",
            true,
            254.0,
            63.0,
            405.0,
        );
        text(
            &p,
            o.x + 21.0,
            o.y + 103.0,
            crate::i18n::t("Automatically start downloading the following file types:"),
            LIGHT_TEXT(),
        );
        multiline(
            ui,
            r(o, 22.0, 110.0, 380.0, 59.0),
            &mut self.s.auto_types,
            "auto-types",
        );
        if winui::button_light(
            ui,
            r(o, 316.0, 175.0, 73.0, 21.0),
            // i18n: button: put back the default list
            "Default",
            ButtonKind::Normal,
            false,
        )
        .clicked()
        {
            self.s.auto_types = settings::DEFAULT_AUTO_TYPES.into();
        }
        text(
            &p,
            o.x + 21.0,
            o.y + 222.0,
            crate::i18n::t("Don't start downloading automatically from the following sites:"),
            LIGHT_TEXT(),
        );
        multiline(
            ui,
            r(o, 22.0, 229.0, 380.0, 49.0),
            &mut self.s.excluded_sites,
            "excluded-sites",
        );
        text(
            &p,
            o.x + 21.0,
            o.y + 293.0,
            crate::i18n::t("(separate names by spaces)"),
            LIGHT_TEXT(),
        );
        if winui::button_light(
            ui,
            r(o, 316.0, 284.0, 73.0, 21.0),
            // i18n: button: put back the default list
            "Default",
            ButtonKind::Normal,
            false,
        )
        .clicked()
        {
            self.s.excluded_sites = settings::DEFAULT_EXCLUDED_SITES.into();
        }
        text(
            &p,
            o.x + 21.0,
            o.y + 327.0,
            crate::i18n::t("Don't start downloading automatically from the following addresses:"),
            LIGHT_TEXT(),
        );
        // The address list editor belongs to browser capture (M8).
        winui::button_light(
            ui,
            r(o, 22.0, 334.0, 135.0, 21.0),
            "Edit list ...",
            ButtonKind::Disabled,
            false,
        );
    }

    fn save_to(&mut self, ui: &mut Ui, o: Pos2, downloads: &std::path::Path) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageSaveTo,
            "Categories, file types, folders",
            true,
            201.0,
            58.0,
            398.0,
        );
        group(ui, o, 21.0, 94.0, 380.0, 201.0, "Save To...");
        // i18n: noun: label before the list of categories
        text(
            &p,
            o.x + 30.0,
            o.y + 127.0,
            crate::i18n::t("Category"),
            LIGHT_TEXT(),
        );
        // IDM's, Torrents and the user's (the registry follows this dialog's edits).
        let cats = Category::all();
        self.category = self.category.min(cats.len() - 1);
        let names: Vec<String> = cats.iter().map(|c| c.label().into_owned()).collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        if let Some(i) = winui::combo_light(
            ui,
            r(o, 30.0, 136.0, 212.0, 21.0),
            &labels,
            self.category,
            true,
            "opt-category",
        ) {
            // Keep the folder typed for the category being left.
            self.commit_cat_dir(downloads);
            self.category = i;
            self.cat_dir = display_dir(&self.s.category_dir(cats[i], downloads));
        }
        // New / Edit... (D-021): a category of the user's; a built-in one's folder.
        if winui::button_light(
            ui,
            r(o, 319.0, 105.0, 73.0, 21.0),
            // i18n: button: create a new item
            "New",
            ButtonKind::Normal,
            false,
        )
        .clicked()
        {
            self.commit_cat_dir(downloads);
            self.cat_edit = Some(crate::categories::CategoryDialog::new());
        }
        if winui::button_light(
            ui,
            r(o, 319.0, 137.0, 73.0, 21.0),
            // i18n: verb: edit the item
            "Edit...",
            ButtonKind::Normal,
            false,
        )
        .clicked()
        {
            self.commit_cat_dir(downloads);
            let cat = cats[self.category];
            let folder = self.s.category_dir(cat, downloads);
            self.cat_edit = Some(crate::categories::CategoryDialog::edit(cat, &folder));
        }
        let cat = cats[self.category];
        let label = cat.label();
        let with_category = |template: &str| crate::i18n::fmt(template, &[("category", &label)]);
        let line = with_category(crate::i18n::t(
            "Automatically put in \"{category}\" category the following file types:",
        ));
        winui::text_in_window(ui, &p, o.x + 31.0, o.y + 178.0, &line, LIGHT_TEXT());
        let types = cat.types_text();
        readonly(ui, r(o, 31.0, 185.0, 362.0, 23.0), &types);
        let dir_line = with_category(crate::i18n::t(
            "Default download directory for \"{category}\" category",
        ));
        winui::text_in_window(ui, &p, o.x + 30.0, o.y + 230.0, &dir_line, LIGHT_TEXT());
        winui::edit_light_on(
            ui,
            r(o, 31.0, 239.0, 281.0, 23.0),
            &mut self.cat_dir,
            true,
            false,
            LIGHT_PAGE(),
        );
        if winui::button_light(
            ui,
            r(o, 319.0, 240.0, 73.0, 21.0),
            // i18n: verb: choose a folder
            "Browse",
            ButtonKind::Normal,
            false,
        )
        .clicked()
            && let Some(dir) = rfd::FileDialog::new()
                .set_title(dir_line.clone())
                .set_directory(&self.cat_dir)
                .pick_folder()
        {
            self.cat_dir = display_dir(&dir);
        }
        self.commit_cat_dir(downloads);
        let line = with_category(crate::i18n::t(
            "Change folder for \"{category}\" category on last selected",
        ));
        winui::checkbox_light(
            ui,
            o + vec2(33.0, 271.0),
            &mut self.s.change_folder_on_last,
            &line,
            true,
        );
        winui::checkbox_light(
            ui,
            o + vec2(33.0, 306.0),
            &mut self.s.set_creation_date,
            "Set file creation date as provided by the server",
            true,
        );
        group(ui, o, 21.0, 334.0, 380.0, 118.0, "Temporary directory");
        // D-004: no temporary directory — parts are written next to the file, so there is
        // nothing to assemble at the end.
        let none = crate::i18n::t("Not used: file parts are written next to the file");
        readonly(ui, r(o, 30.0, 349.0, 281.0, 23.0), none);
        winui::button_light(
            ui,
            r(o, 317.0, 350.0, 73.0, 21.0),
            // i18n: verb: choose a folder
            "Browse",
            ButtonKind::Disabled,
            false,
        );
        winui::paragraph(
            &p,
            o.x + 30.0,
            o.y + 387.0,
            crate::i18n::t(
                "Mister Download Manager writes every connection straight into the final\nfile (a sparse part file next to it), so no temporary directory and no\nassembling step are needed.",
            ),
            LIGHT_TEXT(),
            o.x + 426.0,
            13.0,
        );
    }

    /// The typed folder for the selected category becomes its own (or the default again).
    fn commit_cat_dir(&mut self, downloads: &std::path::Path) {
        let cats = Category::all();
        let Some(&cat) = cats.get(self.category) else {
            return;
        };
        let default = display_dir(&cat.default_folder(downloads));
        let typed = self.cat_dir.trim();
        let own = (!typed.is_empty()
            && typed.trim_end_matches('\\') != default.trim_end_matches('\\'))
        .then(|| typed.to_owned());
        self.s.set_category_dir(cat, own);
    }

    fn downloads(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageDownloads,
            "Default download settings",
            true,
            222.0,
            58.0,
            398.0,
        );
        let s = &mut self.s;
        winui::text_right(
            &p,
            o.x + 306.0,
            o.y + 104.0,
            crate::i18n::t("Customize \"Download progress\" dialog"),
            LIGHT_DISABLED(),
        );
        // The progress dialog customisation dialog is not built yet.
        winui::button_light(
            ui,
            r(o, 319.0, 89.0, 73.0, 21.0),
            // i18n: verb: edit the item
            "Edit...",
            ButtonKind::Disabled,
            false,
        );
        winui::checkbox_light(
            ui,
            o + vec2(24.0, 120.0),
            &mut s.show_start_dialog,
            "Show start download dialog",
            true,
        );
        let on = s.show_start_dialog;
        winui::checkbox_light(
            ui,
            o + vec2(50.0, 141.0),
            &mut s.only_add_to_queue,
            "Do not start downloading, only add files to the queue",
            !on,
        );
        winui::checkbox_light(
            ui,
            o + vec2(24.0, 160.0),
            &mut s.show_complete,
            "Show download complete dialog",
            true,
        );
        text(
            &p,
            o.x + 31.0,
            o.y + 191.0,
            crate::i18n::t("Note: These settings don't relate to queue processing"),
            LIGHT_TEXT(),
        );
        winui::etched_h(&p, o.x + 21.0, o.y + 198.0, 381.0);
        winui::checkbox_light(
            ui,
            o + vec2(24.0, 212.0),
            &mut s.start_immediately,
            "Start downloading immediately while displaying \"Download File Info\" dialog",
            true,
        );
        winui::checkbox_light(
            ui,
            o + vec2(24.0, 241.0),
            &mut s.queue_panel_later,
            "Show queue selection panel on pressing \"Download Later\" button",
            true,
        );
        winui::checkbox_light(
            ui,
            o + vec2(24.0, 266.0),
            &mut s.queue_panel_batch,
            "Show queue selection panel on closing batch download dialogs",
            true,
        );
        winui::checkbox_light(
            ui,
            o + vec2(24.0, 290.0),
            &mut s.ignore_mtime,
            "Ignore file modification time changes when resuming a download",
            true,
        );
        winui::etched_h(&p, o.x + 21.0, o.y + 314.0, 381.0);
        text(
            &p,
            o.x + 21.0,
            o.y + 334.0,
            crate::i18n::t("If a duplicate download link is added:"),
            LIGHT_TEXT(),
        );
        if let Some(i) = winui::combo_light(
            ui,
            r(o, 21.0, 340.0, 380.0, 21.0),
            &DUPLICATE,
            s.duplicate_action as usize,
            true,
            "opt-duplicate",
        ) {
            s.duplicate_action = i as u8;
        }
        winui::etched_h(&p, o.x + 21.0, o.y + 367.0, 381.0);
        text(
            &p,
            o.x + 21.0,
            o.y + 388.0,
            crate::i18n::t("User-Agent for manually added downloads:"),
            LIGHT_TEXT(),
        );
        winui::edit_light(
            ui,
            r(o, 21.0, 398.0, 381.0, 23.0),
            &mut s.user_agent,
            true,
            false,
        );
        winui::etched_h(&p, o.x + 21.0, o.y + 426.0, 381.0);
        winui::text_right(
            &p,
            o.x + 302.0,
            o.y + 451.0,
            crate::i18n::t("Virus checking settings"),
            LIGHT_DISABLED(),
        );
        winui::button_light(
            ui,
            r(o, 319.0, 436.0, 73.0, 21.0),
            // i18n: verb: edit the item
            "Edit...",
            ButtonKind::Disabled,
            false,
        );
    }

    fn connection(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageConnection,
            "Connections and Limits",
            true,
            245.0,
            63.0,
            399.0,
        );
        group(ui, o, 21.0, 102.0, 380.0, 195.0, "Max. connections number");
        let label = crate::i18n::t("Default max. conn. number");
        let (rect, cut) = winui::label_into(
            &p,
            o.x + 130.0,
            o.y + 126.0,
            label,
            LIGHT_TEXT(),
            o.x + 268.0,
            o.x + 30.0,
        );
        if cut {
            winui::full_on_hover(ui, rect, "max-conns", label);
        }
        let labels: Vec<String> = CONNECTIONS.iter().map(|n| n.to_string()).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let cur = CONNECTIONS
            .iter()
            .position(|n| *n == self.s.max_connections)
            .unwrap_or(3);
        if let Some(i) = winui::combo_light(
            ui,
            r(o, 268.0, 112.0, 101.0, 21.0),
            &labels,
            cur,
            true,
            "opt-conns",
        ) {
            self.s.max_connections = CONNECTIONS[i];
        }
        winui::etched_h(&p, o.x + 30.0, o.y + 141.0, 359.0);
        // i18n: plural noun: label before a list of exceptions
        text(
            &p,
            o.x + 33.0,
            o.y + 158.0,
            crate::i18n::t("Exceptions:"),
            LIGHT_TEXT(),
        );
        // The user's exceptions, then the caps the engine learned (read-only "(auto)" rows).
        let rows: Vec<Vec<String>> = self
            .s
            .conn_exceptions
            .iter()
            .map(|(server, n)| vec![server.clone(), n.to_string()])
            .chain(
                self.learned
                    .iter()
                    .filter(|(h, _)| {
                        !self
                            .s
                            .conn_exceptions
                            .iter()
                            .any(|(s, _)| s.eq_ignore_ascii_case(h))
                    })
                    .map(|(h, n)| {
                        // i18n: a number of connections the app learned by itself for a server
                        let auto = crate::i18n::t("{n} (auto)");
                        vec![h.clone(), crate::i18n::fmt(auto, &[("n", &n.to_string())])]
                    }),
            )
            .collect();
        let (dbl, del_key) = report(
            ui,
            o,
            r(o, 30.0, 162.0, 281.0, 122.0),
            // i18n: column titles: the server, and the number of connections allowed to it
            &[
                (crate::i18n::mark("Server"), 241.0),
                (crate::i18n::mark("Number"), 291.0),
            ],
            &rows,
            &mut self.exc_rows,
            true,
            "opt-exceptions",
        );
        // Learned rows can't be edited or deleted (they fade by themselves).
        let sel = self
            .exc_rows
            .selected
            .filter(|i| *i < self.s.conn_exceptions.len());
        let new = winui::button_light(
            ui,
            r(o, 319.0, 173.0, 73.0, 21.0),
            // i18n: button: create a new item
            "New",
            ButtonKind::Normal,
            false,
        )
        .clicked();
        let kind = if sel.is_some() {
            ButtonKind::Normal
        } else {
            ButtonKind::Disabled
        };
        // i18n: verb: delete the item
        let del = winui::button_light(ui, r(o, 319.0, 202.0, 73.0, 21.0), "Delete", kind, false)
            .clicked()
            || del_key;
        let edit =
            // i18n: verb: edit the item
            winui::button_light(ui, r(o, 319.0, 232.0, 73.0, 21.0), "Edit", kind, false).clicked();
        if new {
            self.sub = Some(Sub::Exception(None, String::new(), "1".into()));
        }
        if let Some(i) = sel {
            if del {
                self.s.conn_exceptions.remove(i);
                self.exc_rows.selected = None;
            } else if edit || dbl {
                let (server, n) = self.s.conn_exceptions[i].clone();
                self.sub = Some(Sub::Exception(Some(i), server, n.to_string()));
            }
        }
        // Download limits (shared with the Scheduler's page).
        let gp = r(o, 21.0, 315.0, 380.0, 82.0);
        let gap = winui::caption_gap(&p, o.x + 37.0, o.x + 132.0, "Download limits", true);
        winui::group_box(&p, gp, Some(gap));
        winui::checkbox_light(
            ui,
            o + vec2(37.0, 310.0),
            &mut self.s.limits_on,
            "Download limits",
            true,
        );
        let on = self.s.limits_on;
        let line =
            |lead: f32, lead_baseline: f32, y: f32, gap: f32, baseline: f32| winui::SentenceLine {
                lead: winui::Lead::Right(o.x + lead),
                lead_baseline: o.y + lead_baseline,
                boxes: o + vec2(178.0, y),
                gap,
                baseline: o.y + baseline,
            };
        let boxes = winui::sentence(
            ui,
            &p,
            crate::i18n::t(crate::quota::LIMITS_SENTENCE),
            &[
                line(170.0, 345.0, 330.0, 8.0, 346.0),
                line(170.0, 366.0, 351.0, 11.0, 364.0),
            ],
            &[("mb", vec2(36.0, 20.0)), ("hours", vec2(36.0, 20.0))],
            |p, x, baseline, s| embossed(p, x, baseline, s, on),
        );
        winui::edit_light_on(ui, boxes[0], &mut self.mb, on, false, LIGHT_PAGE());
        winui::edit_light_on(ui, boxes[1], &mut self.hours, on, false, LIGHT_PAGE());
        winui::checkbox_light(
            ui,
            o + vec2(37.0, 376.0),
            &mut self.s.limits_warn,
            "Show warning before stopping downloads",
            on,
        );
        self.mb.retain(|c| c.is_ascii_digit());
        self.hours.retain(|c| c.is_ascii_digit());
    }

    fn proxy(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        Icon::PageProxy
            .image(true)
            .paint_at(ui, r(o, 21.0, 50.0, 32.0, 32.0));
        page_title(
            &p,
            winui::tahoma_bold(),
            o.x + 112.0,
            o.y + 69.0,
            "Proxy / socks configuration",
        );
        let s = &mut self.s;
        if winui::button_light(
            ui,
            r(o, 307.0, 61.0, 93.0, 21.0),
            "Get System",
            ButtonKind::Normal,
            false,
        )
        .clicked()
            && let Some((host, port)) = system_proxy()
        {
            s.proxy_mode = 3;
            s.proxy_host = host;
            s.proxy_port = port;
            s.proxy_http = true;
            s.proxy_https = true;
        }
        winui::checkbox_light(
            ui,
            o + vec2(30.0, 100.0),
            &mut s.proxy_from_browser,
            "",
            true,
        );
        // The checkbox's caption, in two lines as IDM has it (clicking it ticks the box).
        let lines = winui::paragraph(
            &p,
            o.x + 46.0,
            o.y + 104.0,
            crate::i18n::t(
                "Use proxy/socks from a browser's request if there are download errors /\nfor the downloads intercepted from the browser",
            ),
            LIGHT_TEXT(),
            o.x + 426.0,
            13.0,
        );
        winui::caption_click(
            ui,
            r(o, 46.0, 99.0, 380.0, 13.0 * lines as f32 + 2.0),
            "proxy-from-browser",
            &mut s.proxy_from_browser,
            true,
        );
        winui::etched_h(&p, o.x + 22.0, o.y + 128.0, 380.0);
        let modes = [
            (21.0, 137.0, crate::i18n::mark("No proxy/socks")),
            (21.0, 167.0, crate::i18n::mark("Use system settings")),
            (
                21.0,
                194.0,
                crate::i18n::mark("Use automatic configuration script"),
            ),
            (
                22.0,
                250.0,
                crate::i18n::mark("Manual proxy/socks configuration"),
            ),
        ];
        let mut group = Vec::new();
        for (i, (x, y, label)) in modes.iter().enumerate() {
            let resp = winui::radio_light(
                ui,
                o + vec2(*x, *y),
                s.proxy_mode as usize == i,
                label,
                true,
            );
            if resp.clicked() {
                s.proxy_mode = i as u8;
            }
            group.push(resp);
        }
        if let Some(i) = winui::radio_keys(ui, &group) {
            s.proxy_mode = i as u8;
        }
        winui::etched_h(&p, o.x + 22.0, o.y + 159.0, 380.0);
        winui::etched_h(&p, o.x + 21.0, o.y + 187.0, 380.0);
        winui::etched_h(&p, o.x + 21.0, o.y + 242.0, 380.0);
        // The script's address (http, https or file): MDM fetches and runs it (pacfetch.rs,
        // mdm_engine::pac).
        let script = s.proxy_mode == 2;
        // i18n: noun: the web address (URL)
        disabled_label(&p, o.x + 30.0, o.y + 228.0, "Address", script, false);
        winui::edit_light_on(
            ui,
            r(o, 90.0, 213.0, 305.0, 23.0),
            &mut s.proxy_script,
            script,
            false,
            LIGHT_PAGE(),
        );
        let manual = s.proxy_mode == 3;
        disabled_label(
            &p,
            o.x + 30.0,
            o.y + 280.0,
            "Proxy server address",
            manual,
            false,
        );
        disabled_label(&p, o.x + 172.0, o.y + 278.0, "Port", manual, false);
        disabled_label(&p, o.x + 229.0, o.y + 278.0, "UserName", manual, false);
        disabled_label(&p, o.x + 313.0, o.y + 278.0, "Password", manual, false);
        winui::edit_light_on(
            ui,
            r(o, 30.0, 286.0, 137.0, 24.0),
            &mut s.proxy_host,
            manual,
            false,
            LIGHT_PAGE(),
        );
        winui::edit_light_on(
            ui,
            r(o, 172.0, 286.0, 51.0, 24.0),
            &mut s.proxy_port,
            manual,
            false,
            LIGHT_PAGE(),
        );
        s.proxy_port.retain(|c| c.is_ascii_digit());
        winui::edit_light_on(
            ui,
            r(o, 229.0, 286.0, 78.0, 24.0),
            &mut s.proxy_user,
            manual,
            false,
            LIGHT_PAGE(),
        );
        password(
            ui,
            r(o, 312.0, 286.0, 83.0, 24.0),
            &mut s.proxy_pass,
            manual,
        );
        disabled_label(
            &p,
            o.x + 30.0,
            o.y + 326.0,
            "Use this proxy for the following protocols:",
            manual,
            false,
        );
        winui::checkbox_light(ui, o + vec2(30.0, 334.0), &mut s.proxy_http, "http", manual);
        winui::checkbox_light(
            ui,
            o + vec2(97.0, 334.0),
            &mut s.proxy_https,
            "https",
            manual,
        );
        // FTP connects to its servers directly, never through an HTTP proxy: the ftp box
        // stays greyed (D-021).
        winui::checkbox_light(ui, o + vec2(174.0, 334.0), &mut s.proxy_ftp, "ftp", false);
        // Socks settings: with the proxy work in M10.
        winui::button_light(
            ui,
            r(o, 235.0, 339.0, 159.0, 21.0),
            "Advanced / Socks...",
            ButtonKind::Disabled,
            false,
        );
        winui::etched_h(&p, o.x + 21.0, o.y + 439.0, 381.0);
        winui::checkbox_light(
            ui,
            o + vec2(27.0, 445.0),
            &mut s.ftp_passive,
            "Use FTP in PASV mode",
            true,
        );
    }

    fn logins(&mut self, ui: &mut Ui, o: Pos2) {
        header(
            ui,
            o,
            Icon::PageLogins,
            "User names and passwords for servers/sites",
            true,
            117.0,
            63.0,
            399.0,
        );
        let rows: Vec<Vec<String>> = self
            .s
            .site_logins
            .iter()
            .map(|(site, user, pass)| {
                vec![site.clone(), user.clone(), "*".repeat(pass.chars().count())]
            })
            .collect();
        let (dbl, del_key) = report(
            ui,
            o,
            r(o, 21.0, 93.0, 383.0, 280.0),
            &[
                // i18n: column title: the site (and path) a login is for
                (crate::i18n::mark("Site/path"), 262.0),
                // i18n: column title: the user name
                (crate::i18n::mark("User"), 332.0),
                (crate::i18n::mark("Password"), 387.0),
            ],
            &rows,
            &mut self.login_rows,
            false,
            "opt-logins",
        );
        let sel = self.login_rows.selected.filter(|i| *i < rows.len());
        let new = winui::button_light(
            ui,
            r(o, 31.0, 381.0, 73.0, 21.0),
            // i18n: button: create a new item
            "New",
            ButtonKind::Normal,
            false,
        )
        .clicked();
        let kind = if sel.is_some() {
            ButtonKind::Normal
        } else {
            ButtonKind::Disabled
        };
        let edit =
            // i18n: verb: edit the item
            winui::button_light(ui, r(o, 125.0, 381.0, 73.0, 21.0), "Edit", kind, false).clicked();
        // i18n: verb: remove the item
        let remove = winui::button_light(ui, r(o, 220.0, 381.0, 73.0, 21.0), "Remove", kind, false)
            .clicked()
            || del_key;
        if new {
            self.sub = Some(Sub::Login(
                None,
                String::new(),
                String::new(),
                String::new(),
            ));
        }
        if let Some(i) = sel {
            if remove {
                self.s.site_logins.remove(i);
                self.login_rows.selected = None;
            } else if edit || dbl {
                let (site, user, pass) = self.s.site_logins[i].clone();
                self.sub = Some(Sub::Login(Some(i), site, user, pass));
            }
        }
    }

    /// Regional (ours): how dates and times are written in the list and the Scheduler, as the
    /// system's region writes them or the user's own way.
    fn regional(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageRegional,
            "Dates and times",
            true,
            302.0,
            61.0,
            406.0,
        );
        let reg = &mut self.s.regional;
        group(ui, o, 16.0, 92.0, 392.0, 250.0, "Write dates and times");
        let system = winui::radio_light(
            ui,
            o + vec2(30.0, 112.0),
            !reg.custom,
            "As the system's region writes them",
            true,
        );
        let own = winui::radio_light(ui, o + vec2(30.0, 140.0), reg.custom, "My own way:", true);
        match winui::radio_keys(ui, &[system.clone(), own.clone()]) {
            Some(i) => reg.custom = i == 1,
            None if system.clicked() => reg.custom = false,
            None if own.clicked() => reg.custom = true,
            None => {}
        }
        let on = reg.custom;
        // Each choice: its label right-aligned before its box.
        let row = |ui: &mut Ui, y: f32, label: &str, items: &[&str], at: usize, id: &str| {
            let label = crate::i18n::t(label);
            let color = if on { LIGHT_TEXT() } else { LIGHT_DISABLED() };
            let (rect, cut) =
                winui::text_right_within(&p, o.x + 186.0, o.y + y + 15.0, label, color, o.x + 48.0);
            if cut {
                winui::full_on_hover(ui, rect, ("regional-label", y as i32), label);
            }
            winui::combo_light(ui, r(o, 192.0, y, 200.0, 21.0), items, at, on, id)
        };
        let orders = [
            // i18n: a date's order: day first (30/10/2026)
            crate::i18n::mark("Day, month, year"),
            // i18n: a date's order: month first (10/30/2026)
            crate::i18n::mark("Month, day, year"),
            // i18n: a date's order: year first (2026-10-30)
            crate::i18n::mark("Year, month, day"),
        ];
        let order_at = match reg.order {
            DateOrder::Dmy => 0,
            DateOrder::Mdy => 1,
            DateOrder::Ymd => 2,
        };
        // i18n: label before the choice of a date's order
        if let Some(i) = row(
            ui,
            168.0,
            crate::i18n::mark("Date order:"),
            &orders,
            order_at,
            "regional-order",
        ) {
            reg.order = [DateOrder::Dmy, DateOrder::Mdy, DateOrder::Ymd][i];
        }
        let clocks = [
            // i18n: a 12-hour clock, with an example
            crate::i18n::mark("12-hour (9:41 PM)"),
            // i18n: a 24-hour clock, with an example
            crate::i18n::mark("24-hour (21:41)"),
        ];
        // i18n: label before the choice of a 12- or 24-hour clock
        if let Some(i) = row(
            ui,
            198.0,
            crate::i18n::mark("Clock:"),
            &clocks,
            reg.clock24 as usize,
            "regional-clock",
        ) {
            reg.clock24 = i == 1;
        }
        let days = [
            crate::i18n::mark("Sunday"),
            crate::i18n::mark("Monday"),
            crate::i18n::mark("Saturday"),
        ];
        let day_at = match reg.week_start {
            1 => 1,
            6 => 2,
            _ => 0,
        };
        // i18n: label before the choice of the week's first day (in the Scheduler's calendar)
        if let Some(i) = row(
            ui,
            228.0,
            crate::i18n::mark("First day of the week:"),
            &days,
            day_at,
            "regional-week",
        ) {
            reg.week_start = [0, 1, 6][i];
        }
        // What it looks like, now.
        let now = chrono::Local::now().naive_local();
        let sample = crate::regional::format_with(crate::regional::effective(reg), now);
        // i18n: label before an example date and time written the chosen way
        let label = crate::i18n::t("Example:");
        text(&p, o.x + 48.0, o.y + 290.0, label, LIGHT_TEXT());
        let lw = winui::text_width(&p, label);
        winui::text_in_window(
            ui,
            &p,
            o.x + 48.0 + lw + 8.0,
            o.y + 290.0,
            &crate::i18n::ltr(&sample),
            LIGHT_TEXT(),
        );
        // i18n: a note under the date and time choices
        winui::text_in_window(
            ui,
            &p,
            o.x + 30.0,
            o.y + 320.0,
            crate::i18n::t("Used by the list of downloads and the Scheduler."),
            LIGHT_DISABLED(),
        );
    }

    fn sounds(&mut self, ui: &mut Ui, o: Pos2) {
        let p = ui.painter().clone();
        header(
            ui,
            o,
            Icon::PageSounds,
            "Sound settings",
            true,
            304.0,
            61.0,
            406.0,
        );
        group(
            ui,
            o,
            25.0,
            102.0,
            383.0,
            299.0,
            "Select sounds for Mister Download Manager events",
        );
        let list_on_screen = r(o, 33.0, 112.0, 366.0, 250.0);
        list_frame(&p, list_on_screen);
        let list = winui::rect(list_on_screen);
        let head_y = list.top() + 1.0;
        // i18n: column title: the event a sound plays for
        text(
            &p,
            list.left() + 8.0,
            head_y + 16.0,
            crate::i18n::t("Event"),
            LIGHT_TEXT(),
        );
        fill(
            &p,
            o.x + 224.0,
            head_y + 1.0,
            o.x + 224.0,
            head_y + 23.0,
            winui::HEADER_LINE(),
        );
        text(
            &p,
            o.x + 231.0,
            head_y + 16.0,
            crate::i18n::t("Sound file"),
            LIGHT_TEXT(),
        );
        let body_top = head_y + 25.0;
        let grid = winui::GRID();
        for k in 0..13 {
            let y = body_top + 17.0 * k as f32 + 16.0;
            if y < list.bottom() - 2.0 {
                fill(&p, list.left() + 2.0, y, list.right() - 3.0, y, grid);
            }
        }
        fill(
            &p,
            o.x + 225.0,
            body_top,
            o.x + 225.0,
            list.bottom() - 3.0,
            grid,
        );
        for (i, event) in SOUND_EVENTS.iter().enumerate() {
            let top = body_top + 17.0 * i as f32;
            let row =
                Rect::from_min_size(pos2(list.left() + 2.0, top), vec2(list.width() - 4.0, 16.0));
            let glyph = pos2(o.x + 39.0, top + 2.0);
            let (on, file) = &mut self.s.sounds[i];
            let box_hit = winui::rect(Rect::from_min_size(glyph, vec2(13.0, 13.0)));
            if ui
                .interact(
                    box_hit,
                    egui::Id::unique(("sound-check", i)),
                    Sense::click(),
                )
                .clicked()
            {
                *on = !*on;
            } else if ui
                .interact(
                    winui::rect(row),
                    egui::Id::unique(("sound-row", i)),
                    Sense::click(),
                )
                .clicked()
            {
                self.sound_sel = Some(i);
            }
            if self.sound_sel == Some(i) {
                fill(
                    &p,
                    glyph.x + 16.0,
                    top,
                    row.right() - 1.0,
                    top + 15.0,
                    winui::SELECTION,
                );
            }
            let c = if self.sound_sel == Some(i) {
                Color32::WHITE
            } else {
                LIGHT_TEXT()
            };
            check_glyph(&p, glyph, *on, true);
            let event = crate::i18n::t(event);
            let (rect, cut) = winui::text_fit(&p, o.x + 57.0, top + 12.0, event, c, o.x + 221.0);
            if cut {
                winui::full_on_hover(ui, rect, ("sound-event", i), event);
            }
            let name = std::path::Path::new(file.as_str())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            winui::fit(
                ui,
                &p,
                o.x + 231.0,
                top + 12.0,
                &name,
                c,
                list.right() - 4.0,
                ("sound-file", i),
            );
        }
        let sel = self.sound_sel;
        let kind = if sel.is_some() {
            ButtonKind::Normal
        } else {
            ButtonKind::Disabled
        };
        let browse =
            winui::button_light(ui, r(o, 106.0, 371.0, 73.0, 21.0), "Browse...", kind, false)
                .clicked();
        let has_file = sel.is_some_and(|i| !self.s.sounds[i].1.is_empty());
        let play_kind = if has_file {
            ButtonKind::Normal
        } else {
            ButtonKind::Disabled
        };
        let play =
            // i18n: verb: plays the sound
            winui::button_light(ui, r(o, 241.0, 371.0, 73.0, 21.0), "Play", play_kind, false)
                .clicked();
        if let Some(i) = sel {
            if browse
                && let Some(f) = rfd::FileDialog::new()
                    .set_title(crate::i18n::t(SOUND_EVENTS[i]))
                    // i18n: the kind of file the Open dialog lists (sound files)
                    .add_filter(crate::i18n::t("Sounds"), &["wav"])
                    .pick_file()
            {
                self.s.sounds[i] = (true, f.display().to_string());
            }
            if play && has_file {
                play_sound(&self.s.sounds[i].1);
            }
        }
    }

    // ---- entry dialogs --------------------------------------------------------------------

    fn sub_dialog(&mut self, ctx: &egui::Context) {
        let Some(sub) = self.sub.take() else { return };
        match sub {
            Sub::Login(row, mut site, mut user, mut pass) => {
                let title = if row.is_some() {
                    crate::i18n::mark("Edit login")
                } else {
                    crate::i18n::mark("New login")
                };
                let out = window(
                    ctx,
                    "opt-login",
                    title,
                    [300.0, 150.0],
                    false,
                    LIGHT_BG(),
                    |ui, o| {
                        let p = ui.painter().clone();
                        // i18n: label before a web address (site and folder)
                        winui::text_right(
                            &p,
                            o.x + 70.0,
                            o.y + 26.0,
                            crate::i18n::t("Site/path:"),
                            LIGHT_TEXT(),
                        );
                        winui::edit_light(
                            ui,
                            r(o, 78.0, 12.0, 210.0, 23.0),
                            &mut site,
                            true,
                            false,
                        );
                        // i18n: noun: label before the user name
                        winui::text_right(
                            &p,
                            o.x + 70.0,
                            o.y + 57.0,
                            crate::i18n::t("User:"),
                            LIGHT_TEXT(),
                        );
                        winui::edit_light(
                            ui,
                            r(o, 78.0, 43.0, 210.0, 23.0),
                            &mut user,
                            true,
                            false,
                        );
                        winui::text_right(
                            &p,
                            o.x + 70.0,
                            o.y + 88.0,
                            crate::i18n::t("Password:"),
                            LIGHT_TEXT(),
                        );
                        password(ui, r(o, 78.0, 74.0, 210.0, 23.0), &mut pass, true);
                        ok_cancel(ui, o, 136.0, 117.0, !site.trim().is_empty())
                    },
                );
                match out {
                    Outcome::Open => self.sub = Some(Sub::Login(row, site, user, pass)),
                    Outcome::Done(()) => {
                        let entry = (site.trim().to_owned(), user, pass);
                        match row {
                            Some(i) => self.s.site_logins[i] = entry,
                            None => self.s.site_logins.push(entry),
                        }
                    }
                    Outcome::Closed => {}
                }
            }
            Sub::Exception(row, mut server, mut number) => {
                let title = if row.is_some() {
                    crate::i18n::mark("Edit exception")
                } else {
                    crate::i18n::mark("New exception")
                };
                let out = window(
                    ctx,
                    "opt-exception",
                    title,
                    [300.0, 118.0],
                    false,
                    LIGHT_BG(),
                    |ui, o| {
                        let p = ui.painter().clone();
                        // i18n: noun: label before a server name
                        winui::text_right(
                            &p,
                            o.x + 70.0,
                            o.y + 26.0,
                            crate::i18n::t("Server:"),
                            LIGHT_TEXT(),
                        );
                        winui::edit_light(
                            ui,
                            r(o, 78.0, 12.0, 210.0, 23.0),
                            &mut server,
                            true,
                            false,
                        );
                        // i18n: label before the number of connections allowed
                        winui::text_right(
                            &p,
                            o.x + 70.0,
                            o.y + 57.0,
                            crate::i18n::t("Connections:"),
                            LIGHT_TEXT(),
                        );
                        winui::edit_light(
                            ui,
                            r(o, 78.0, 43.0, 50.0, 23.0),
                            &mut number,
                            true,
                            false,
                        );
                        number.retain(|c| c.is_ascii_digit());
                        ok_cancel(ui, o, 136.0, 85.0, !server.trim().is_empty())
                    },
                );
                match out {
                    Outcome::Open => self.sub = Some(Sub::Exception(row, server, number)),
                    Outcome::Done(()) => {
                        let entry = (
                            server.trim().to_owned(),
                            number.parse().unwrap_or(1).clamp(1, 32),
                        );
                        match row {
                            Some(i) => self.s.conn_exceptions[i] = entry,
                            None => self.s.conn_exceptions.push(entry),
                        }
                    }
                    Outcome::Closed => {}
                }
            }
            Sub::Panels(mut on, mut sites) => {
                let out = window(
                    ctx,
                    "opt-panels",
                    // i18n: window title: the download button the extension shows over videos
                    "Download panels",
                    [380.0, 190.0],
                    false,
                    LIGHT_BG(),
                    |ui, o| {
                        let p = ui.painter().clone();
                        winui::checkbox_light(
                            ui,
                            o + vec2(14.0, 16.0),
                            &mut on,
                            "Show a download button over videos in browsers",
                            true,
                        );
                        winui::text_in_window(
                            ui,
                            &p,
                            o.x + 14.0,
                            o.y + 55.0,
                            crate::i18n::t("Never show it on these sites:"),
                            if on { LIGHT_TEXT() } else { LIGHT_DISABLED() },
                        );
                        multiline(ui, r(o, 14.0, 62.0, 352.0, 55.0), &mut sites, "panel-sites");
                        winui::text_in_window(
                            ui,
                            &p,
                            o.x + 14.0,
                            o.y + 133.0,
                            crate::i18n::t("(separate names by spaces)"),
                            LIGHT_TEXT(),
                        );
                        ok_cancel(ui, o, 212.0, 157.0, true)
                    },
                );
                match out {
                    Outcome::Open => self.sub = Some(Sub::Panels(on, sites)),
                    Outcome::Done(()) => {
                        self.s.video_panel = on;
                        self.s.video_panel_sites =
                            sites.split_whitespace().collect::<Vec<_>>().join(" ");
                    }
                    Outcome::Closed => {}
                }
            }
        }
    }
}

// ---- pieces -----------------------------------------------------------------------------

/// Proxy / Socks → Get System: Windows' own proxy (host, port), if one is set.
fn system_proxy() -> Option<(String, String)> {
    #[cfg(windows)]
    return crate::win32::system_proxy();
    #[cfg(not(windows))]
    None
}

pub fn play_sound(file: &str) {
    if crate::offscreen() {
        return;
    }
    #[cfg(windows)]
    crate::win32::play_sound(file);
    #[cfg(unix)]
    crate::unix::play_sound(file);
}

/// A folder as the dialogs show it: the full path, as IDM's Options shows it, ending in the
/// system's separator (a backslash on Windows).
fn display_dir(dir: &std::path::Path) -> String {
    let s = dir.display().to_string();
    if s.ends_with(std::path::MAIN_SEPARATOR) {
        s
    } else {
        format!("{s}{}", std::path::MAIN_SEPARATOR)
    }
}

/// OK / Cancel at (x, y), OK only when `valid`.
fn ok_cancel(ui: &mut Ui, o: Pos2, x: f32, y: f32, valid: bool) -> Outcome<()> {
    let kind = if valid {
        ButtonKind::Default
    } else {
        ButtonKind::Disabled
    };
    let ok = winui::button_light(ui, r(o, x, y, 73.0, 21.0), "OK", kind, false).clicked();
    let cancel = winui::button_light(
        ui,
        r(o, x + 81.0, y, 73.0, 21.0),
        "Cancel",
        ButtonKind::Normal,
        false,
    )
    .clicked();
    if cancel || winui::escape_pressed(ui) {
        Outcome::Closed
    } else if valid && (ok || winui::enter_pressed(ui)) {
        Outcome::Done(())
    } else {
        Outcome::Open
    }
}

/// A row of tabs as shown: IDM's measured tabs in English; a translation's sized by its labels
/// (each label centred, the room left over shared out), so the row keeps its span (x 8..419)
/// and no label is cut.
fn tab_row(
    p: &egui::Painter,
    row: &[(usize, &'static str, f32, f32, f32)],
) -> Vec<(usize, &'static str, f32, f32, f32)> {
    if row.iter().all(|t| crate::i18n::t(t.1) == t.1) {
        return row.to_vec();
    }
    let (left, right) = (row[0].2, row[row.len() - 1].3);
    let widths: Vec<f32> = row
        .iter()
        .map(|t| winui::text_width(p, crate::i18n::t(t.1)).ceil())
        .collect();
    let total: f32 = widths.iter().map(|w| w + 16.0).sum();
    let spare = ((right - left - total) / row.len() as f32).floor();
    let padded: Vec<f32> = widths.iter().map(|w| w + 16.0).collect();
    let cap = winui::width_cap(&padded, right - left);
    let mut x0 = left;
    row.iter()
        .zip(&widths)
        .enumerate()
        .map(|(k, (t, w))| {
            let x1 = if k + 1 == row.len() {
                right
            } else if spare < 0.0 {
                // Too wide for the strip: the narrow tabs keep their width, the wide ones share
                // the rest (their labels cut where drawn).
                x0 + (w + 16.0).min(cap)
            } else {
                x0 + w + 16.0 + spare
            };
            let tab = (t.0, t.1, x0, x1, x0 + ((x1 - x0 - w) / 2.0).floor());
            x0 = x1;
            tab
        })
        .collect()
}

/// The two-row tab strip and the page frame; returns the page clicked.
fn tabs(ui: &mut Ui, o: Pos2, page: usize) -> Option<usize> {
    // (page, label, x0, x1, label x): the rows never move sideways, only swap front and back.
    const G: [(usize, &str, f32, f32, f32); 5] = [
        // i18n: tab title: general settings
        (0, crate::i18n::mark("General"), 8.0, 82.0, 26.0),
        // i18n: tab title
        (1, crate::i18n::mark("File types"), 82.0, 166.0, 101.0),
        // i18n: tab title: where downloads are saved
        (2, crate::i18n::mark("Save to"), 166.0, 241.0, 185.0),
        // i18n: tab title: download settings
        (3, crate::i18n::mark("Downloads"), 241.0, 330.0, 260.0),
        // i18n: tab title: connection settings
        (4, crate::i18n::mark("Connection"), 330.0, 419.0, 349.0),
    ];
    // IDM's Dial Up / VPN tab is gone (D-021: no modem support; it did nothing); ours, Regional
    // (how dates and times are written), shares the row.
    const P: [(usize, &str, f32, f32, f32); 4] = [
        // i18n: tab title
        (5, crate::i18n::mark("Proxy / Socks"), 8.0, 118.0, 30.0),
        // i18n: tab title: user names and passwords for sites
        (6, crate::i18n::mark("Sites Logins"), 118.0, 226.0, 142.0),
        // i18n: tab title: sound settings
        (8, crate::i18n::mark("Sounds"), 226.0, 316.0, 253.0),
        // i18n: tab title: how dates and times are written (date order, 12- or 24-hour clock)
        (9, crate::i18n::mark("Regional"), 316.0, 419.0, 343.0),
    ];
    let p = ui.painter().clone();
    let border = winui::HEADER_LINE();
    let (g, pr) = (tab_row(&p, &G), tab_row(&p, &P));
    let (front, back): (&[_], &[_]) = if page < 5 { (&g, &pr) } else { (&pr, &g) };
    let mut clicked = None;
    let mut tab = |ui: &mut Ui,
                   (i, label, x0, x1, lx): (usize, &str, f32, f32, f32),
                   top: f32,
                   base: f32,
                   sel: bool| {
        let (x0, x1) = if sel {
            (x0 - if x0 <= 8.0 { 2.0 } else { 1.0 }, x1 + 2.0)
        } else {
            (x0, x1)
        };
        let bottom = if sel { 45.0 } else { top + 17.0 };
        // From the leading edge: the first tab at the right in RTL.
        let rc = winui::rect(Rect::from_min_max(
            o + vec2(x0, top),
            o + vec2(x1 + 1.0, bottom + 1.0),
        ));
        let resp = ui.interact(rc, egui::Id::unique(("opt-tab", i)), Sense::click());
        if resp.clicked() && !sel {
            clicked = Some(i);
        }
        let face = if sel {
            LIGHT_PAGE()
        } else if resp.hovered() {
            winui::TAB_HOT()
        } else {
            winui::TAB_UNSELECTED()
        };
        fill(&p, o.x + x0, o.y + top, o.x + x1, o.y + bottom, face);
        fill(
            &p,
            o.x + x0 + 1.0,
            o.y + top,
            o.x + x1 - 1.0,
            o.y + top,
            border,
        );
        fill(
            &p,
            o.x + x0,
            o.y + top + 1.0,
            o.x + x0,
            o.y + bottom,
            border,
        );
        fill(
            &p,
            o.x + x1,
            o.y + top + 1.0,
            o.x + x1,
            o.y + bottom,
            border,
        );
        // A translated row too wide for the strip has narrower tabs (`tab_row`): their labels are
        // cut with "..." and shown whole on hover.
        let full = crate::i18n::t(label);
        let shown = winui::ellipsize(&p, &winui::tahoma(), full, x1 - x0 - 6.0);
        if shown == full {
            text(&p, o.x + lx, o.y + base, full, LIGHT_TEXT());
        } else {
            let w = winui::text_width(&p, &shown);
            let lx = x0 + ((x1 - x0 - w) / 2.0).floor();
            let rc = text(&p, o.x + lx, o.y + base, &shown, LIGHT_TEXT());
            winui::full_on_hover(ui, rc, ("opt-tab-label", i), full);
        }
    };
    for t in back {
        tab(ui, *t, 9.0, 22.0, false);
    }
    for t in front.iter().filter(|t| t.0 != page) {
        tab(ui, *t, 27.0, 40.0, false);
    }
    // Page frame (x 6..421, y 45..468) with its shadow, then the selected tab over its top.
    fill(
        &p,
        o.x + 6.0,
        o.y + 45.0,
        o.x + 421.0,
        o.y + 468.0,
        LIGHT_PAGE(),
    );
    fill(&p, o.x + 6.0, o.y + 45.0, o.x + 421.0, o.y + 45.0, border);
    fill(&p, o.x + 6.0, o.y + 468.0, o.x + 421.0, o.y + 468.0, border);
    fill(&p, o.x + 6.0, o.y + 45.0, o.x + 6.0, o.y + 468.0, border);
    fill(
        &p,
        o.x + 421.0,
        o.y + 45.0,
        o.x + 421.0,
        o.y + 468.0,
        border,
    );
    let shadow = winui::TAB_UNSELECTED();
    fill(
        &p,
        o.x + 422.0,
        o.y + 45.0,
        o.x + 423.0,
        o.y + 469.0,
        shadow,
    );
    fill(&p, o.x + 6.0, o.y + 469.0, o.x + 423.0, o.y + 469.0, shadow);
    if let Some(t) = front.iter().find(|t| t.0 == page) {
        tab(ui, *t, 25.0, 38.0, true);
    }
    clicked
}

/// Page header: the icon, the title (bold, right-aligned as IDM's) and the etched rule.
#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut Ui,
    o: Pos2,
    icon: Icon,
    title: &str,
    bold: bool,
    ink_x: f32,
    rule_x0: f32,
    rule_x1: f32,
) {
    let p = ui.painter().clone();
    icon.image(true).paint_at(ui, r(o, 21.0, 54.0, 32.0, 32.0));
    let font = if bold {
        winui::tahoma_bold()
    } else {
        winui::tahoma()
    };
    page_title(&p, font, o.x + ink_x, o.y + 69.0, title);
    winui::etched_h(&p, o.x + rule_x0, o.y + 78.0, rule_x1 - rule_x0 + 2.0);
}

/// A page title, right-aligned as IDM's: English at its measured pen `x`, a translation ending
/// where the English ends (in RTL: starting there, at the mirrored edge).
fn page_title(p: &egui::Painter, font: egui::FontId, x: f32, baseline: f32, english: &str) {
    let shown = crate::i18n::t(english);
    let x = if shown == english {
        x
    } else {
        let end = x + winui::text_width_font(p, font.clone(), english);
        end - winui::text_width_font(p, font.clone(), shown)
    };
    winui::text_font(p, font, x, baseline, shown, LIGHT_TEXT());
}

/// Group box (`#dcdcdc` frame) with its caption (translated here) at x + 9.
fn group(ui: &Ui, o: Pos2, x: f32, y: f32, w: f32, h: f32, caption: &str) {
    let p = ui.painter();
    let full = crate::i18n::t(caption);
    // A caption longer than the box is cut with "..." before its right edge (whole on hover).
    let caption = winui::ellipsize(p, &winui::tahoma(), full, w - 18.0);
    let cw = winui::text_width(p, &caption);
    winui::group_box(
        p,
        r(o, x, y, w, h),
        Some((o.x + x + 7.0, o.x + x + 9.0 + cw + 2.0)),
    );
    let rc = text(p, o.x + x + 9.0, o.y + y + 4.0, &caption, LIGHT_TEXT());
    if caption != full {
        winui::full_on_hover(ui, rc, ("group", caption.as_str()), full);
    }
}

/// A label (translated here) that IDM greys out with the classic embossed look when its
/// controls are off.
fn disabled_label(p: &egui::Painter, x: f32, base: f32, s: &str, enabled: bool, right: bool) {
    let s = crate::i18n::t(s);
    let x = if right {
        x - winui::text_width(p, s)
    } else {
        x
    };
    embossed(p, x, base, s, enabled);
}

/// [`disabled_label`]'s look for text already translated.
fn embossed(p: &egui::Painter, x: f32, base: f32, s: &str, enabled: bool) {
    if enabled {
        text(p, x, base, s, LIGHT_TEXT());
    } else {
        text(p, x + 1.0, base + 1.0, s, winui::ETCH_LIGHT());
        text(p, x, base, s, winui::TEXT_DISABLED());
    }
}

pub(crate) fn list_frame(p: &egui::Painter, rc: Rect) {
    p.rect_filled(rc, 0.0, winui::FIELD());
    p.rect_stroke(
        rc,
        0.0,
        egui::Stroke::new(1.0, winui::FIELD_EDGE()),
        egui::StrokeKind::Inside,
    );
}

/// A 13 × 13 check glyph (as in `checkbox_light`) without a label, its top-left at English `at`
/// (placed on screen; the check keeps its shape).
pub(crate) fn check_glyph(p: &egui::Painter, at: Pos2, on: bool, enabled: bool) {
    let rc = winui::rect(Rect::from_min_size(at, vec2(13.0, 13.0)));
    let state = if enabled {
        winui::State::Normal
    } else {
        winui::State::Disabled
    };
    winui::check_box_glyph(p, rc, on, state);
}

/// A report list (header 25 px, rows 17 px): `cols` are (title, right edge in client x).
/// Returns true on a double click of a row.
#[allow(clippy::too_many_arguments)]
fn report(
    ui: &mut Ui,
    o: Pos2,
    rc: Rect,
    cols: &[(&str, f32)],
    rows: &[Vec<String>],
    st: &mut winui::RowList,
    grid: bool,
    id: &str,
) -> (bool, bool) {
    let p = ui.painter().clone();
    list_frame(&p, rc);
    // Laid out in English, as `cols` are; each part made on screen.
    let rc = winui::rect(rc);
    let left = rc.left();
    let head = rc.top() + 1.0;
    let divider = winui::HEADER_LINE();
    let mut x0 = left;
    for (i, (title, right)) in cols.iter().enumerate() {
        let title = crate::i18n::t(title);
        let right = o.x + right;
        let tx = if i == 0 { x0 + 8.0 } else { x0 + 7.0 };
        let (rect, cut) = winui::text_fit(&p, tx, head + 16.0, title, LIGHT_TEXT(), right - 3.0);
        if cut {
            winui::full_on_hover(ui, rect, (id, "head", i), title);
        }
        fill(&p, right, head + 1.0, right, head + 23.0, divider);
        x0 = right;
    }
    // The rows: the shared row list under the header (its frame on the list's).
    let body = Rect::from_min_max(
        pos2(rc.left() + 1.0, head + 25.0),
        pos2(rc.right() - 1.0, rc.bottom() - 1.0),
    );
    let gridc = winui::GRID();
    let ev = winui::row_list(
        ui,
        (id, "rows"),
        winui::rect(body.expand(1.0)),
        rows.len(),
        17.0,
        st,
        |ui, clip, k, line, look| {
            let top = line.top();
            if grid {
                fill(
                    clip,
                    line.left(),
                    top + 16.0,
                    line.right(),
                    top + 16.0,
                    gridc,
                );
            }
            let mut cx = left;
            for (i, (_, right)) in cols.iter().enumerate() {
                let right = o.x + right;
                if grid {
                    fill(clip, right + 1.0, top, right + 1.0, top + 16.0, gridc);
                }
                if let Some(cell) = rows[k].get(i) {
                    winui::fit(
                        ui,
                        clip,
                        cx + 6.0,
                        top + 12.0,
                        cell,
                        look.ink,
                        right - 3.0,
                        (id, "cell", k, i),
                    );
                }
                cx = right;
            }
        },
    );
    // A double-click or Enter edits; Delete removes.
    (
        ev.double_clicked.or(ev.entered).is_some(),
        ev.delete.is_some(),
    )
}

/// A multi-line edit that wraps its text (the shared one).
fn multiline(ui: &mut Ui, rc: Rect, value: &mut String, id: &str) {
    winui::multiline_light(ui, rc, value, id);
}

/// A read-only field (the page colour, as IDM's): the shared one, so it can be copied.
fn readonly(ui: &mut Ui, rc: Rect, value: &str) {
    winui::readonly_field(
        ui,
        rc,
        value,
        LIGHT_PAGE(),
        false,
        ("options", rc.left() as i32, rc.top() as i32),
    );
}

/// A password edit (characters shown as bullets): the shared one, on the tab page.
fn password(ui: &mut Ui, rc: Rect, value: &mut String, enabled: bool) {
    winui::password_light_on(ui, rc, value, enabled, LIGHT_PAGE());
}

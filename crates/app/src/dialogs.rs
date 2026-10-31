//! IDM's small dialogs as separate OS windows, laid out to the pixel from
//! notes/ui-spec/dialogs.md: "Enter new address to download", "Download File Info",
//! "Download complete". All coordinates below are client pixels from the spec.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Pos2, Ui, ViewportBuilder, ViewportId, vec2};
use mdm_engine::{Options, ProbeInfo};
use url::Url;

use crate::model::{self, Category};
use crate::winui::{self, ButtonKind, r};

pub enum Outcome<T> {
    Open,
    Done(T),
    Closed,
}

const MAIN_RECT: &str = "mdm-main-rect";

/// The main window publishes its outer rect each frame so dialogs can centre on it.
pub fn publish_main_rect(ctx: &egui::Context) {
    if let Some(rc) = ctx.input(|i| i.viewport().outer_rect) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(MAIN_RECT), rc));
    }
}

/// Where a dialog opens: centred over the main window (as IDM's are), decided once when the
/// dialog first shows and kept until `forget_place` — so the user can still move it.
pub fn place(ctx: &egui::Context, key: egui::Id, client: [f32; 2]) -> Option<Pos2> {
    ctx.data_mut(|d| {
        if let Some(p) = d.get_temp::<Pos2>(key) {
            return Some(p);
        }
        let main = d.get_temp::<egui::Rect>(egui::Id::unique(MAIN_RECT))?;
        // A fixed-frame dialog's outer size is client + (6, 29) (spec "Conventions").
        let outer = vec2(client[0] + 6.0, client[1] + 29.0);
        let p = (main.center() - outer / 2.0).round();
        d.insert_temp(key, p);
        Some(p)
    })
}

pub fn forget_place(ctx: &egui::Context, key: egui::Id) {
    ctx.data_mut(|d| d.remove::<Pos2>(key));
}

/// Window chrome shared by every dialog and the progress window: the caption's theme, and when
/// the window may be seen. winit gives a new window the *system* theme, so windows are created
/// hidden (`hidden_builder`), get their theme on the first frame and are shown on the next,
/// already right — no dark flash. The theme is re-sent whenever the wish changes. Call every
/// frame with `open` = false once the window is gone.
pub fn keep_theme(
    ctx: &egui::Context,
    vid: ViewportId,
    key: egui::Id,
    title: &str,
    dark: bool,
    open: bool,
) {
    let theme_key = key.with("theme");
    let shown_key = key.with("shown");
    if !open {
        ctx.data_mut(|d| {
            d.remove::<bool>(theme_key);
            d.remove::<bool>(shown_key);
        });
        return;
    }
    let sent = ctx.data(|d| d.get_temp::<bool>(theme_key));
    if sent != Some(dark) {
        ctx.send_viewport_cmd_to(
            vid,
            egui::ViewportCommand::SetTheme(crate::theme::system_theme(dark)),
        );
        ctx.data_mut(|d| d.insert_temp(theme_key, dark));
        // The window is revealed on the next frame: make sure there is one.
        ctx.request_repaint();
    }
    // The theme went out in an earlier frame: show the window now.
    if sent.is_some() && !ctx.data(|d| d.get_temp::<bool>(shown_key).unwrap_or(false)) {
        ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Visible(true));
        ctx.data_mut(|d| d.insert_temp(shown_key, true));
    }
    // Belt and braces: a no-op when the caption is already right.
    #[cfg(windows)]
    crate::win32::caption_dark(title, dark);
    #[cfg(not(windows))]
    let _ = title;
}

/// A fixed-size dialog window whose client area is exactly `client` pixels; `body` gets the
/// client origin and paints at absolute spec coordinates.
pub(crate) fn window<T>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    client: [f32; 2],
    minimize: bool,
    bg: egui::Color32,
    body: impl FnMut(&mut Ui, Pos2) -> Outcome<T>,
) -> Outcome<T> {
    dialog_window(ctx, id, title, client, minimize, bg, false, body)
}

/// A question that must be answered (Yes / No, a countdown): a `window` that stays above the
/// app's other windows and takes the focus when it opens, so a click on the dialog that asked
/// it, or on the main window, never hides it behind them (on Windows it is also owned by the
/// main window; macOS and Linux have no owner to keep it in front).
pub(crate) fn question<T>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    client: [f32; 2],
    minimize: bool,
    bg: egui::Color32,
    body: impl FnMut(&mut Ui, Pos2) -> Outcome<T>,
) -> Outcome<T> {
    dialog_window(ctx, id, title, client, minimize, bg, true, body)
}

#[allow(clippy::too_many_arguments)]
fn dialog_window<T>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    client: [f32; 2],
    minimize: bool,
    bg: egui::Color32,
    modal: bool,
    mut body: impl FnMut(&mut Ui, Pos2) -> Outcome<T>,
) -> Outcome<T> {
    let key = egui::Id::unique(("dialog-pos", id));
    // The title in the interface language (Win32 finds the window by it, so it is used as is
    // from here on).
    let title = crate::i18n::t(title);
    // Hidden until `keep_theme` has given it its theme (see there).
    let mut builder = ViewportBuilder::default()
        .with_visible(false)
        .with_title(title)
        .with_inner_size(client)
        .with_resizable(false)
        .with_minimize_button(minimize)
        .with_maximize_button(false);
    // Every dialog carries the app icon: without one Windows draws its generic placeholder.
    builder = crate::quiet(builder.with_icon(crate::app_icon()));
    if modal {
        builder = builder.with_always_on_top();
    }
    if let Some(p) = place(ctx, key, client) {
        builder = builder.with_position(p);
    }
    let out = ctx.show_viewport_immediate(ViewportId::from_hash_of(id), builder, |ui, _| {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            return Outcome::Closed;
        }
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(bg))
            .show(ui, |ui| {
                let origin = ui.max_rect().min;
                // The client area, laid out from the right in a right-to-left interface.
                let _surface = winui::Surface::enter(origin.x, origin.x + client[0]);
                body(ui, origin)
            })
            .inner
    });
    // Give the window IDM's modal-dialog frame (3 px borders, 26 px caption) once it exists,
    // and again at a new size (a dialog that grows: the frame is sized from the client).
    #[cfg(windows)]
    {
        let framed = key.with("framed");
        let scale = ctx.input_for(ViewportId::from_hash_of(id), |i| i.pixels_per_point);
        if ctx.data(|d| d.get_temp::<[f32; 2]>(framed)) != Some(client)
            && crate::win32::dialog_frame(title, client, !crate::theme::dark(), scale)
        {
            ctx.data_mut(|d| d.insert_temp(framed, client));
        }
    }
    keep_theme(
        ctx,
        ViewportId::from_hash_of(id),
        key,
        title,
        // Every dialog follows the mode (the light dialogs too).
        crate::theme::dark(),
        matches!(out, Outcome::Open),
    );
    // A question takes the focus once it is shown (not while hidden for its theme).
    let focused = key.with("focus-sent");
    if modal && matches!(out, Outcome::Open) && !crate::offscreen() {
        let shown = ctx.data(|d| d.get_temp::<bool>(key.with("shown")).unwrap_or(false));
        if shown && !ctx.data(|d| d.get_temp::<bool>(focused).unwrap_or(false)) {
            ctx.send_viewport_cmd_to(ViewportId::from_hash_of(id), egui::ViewportCommand::Focus);
            ctx.data_mut(|d| d.insert_temp(focused, true));
        }
    }
    if !matches!(out, Outcome::Open) {
        ctx.data_mut(|d| d.remove::<bool>(focused));
        forget_place(ctx, key);
        #[cfg(windows)]
        ctx.data_mut(|d| d.remove::<[f32; 2]>(key.with("framed")));
    }
    out
}

/// IDM shows paths under the user profile relative to it: `Downloads\Compressed\a.zip`.
pub fn display_path(p: &Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_owned)) {
        Some(rel) => rel.display().to_string(),
        None => p.display().to_string(),
    }
}

/// The inverse of `display_path`: relative paths are under the user profile.
pub fn resolve_path(s: &str) -> PathBuf {
    let p = PathBuf::from(s.trim());
    if p.is_absolute() {
        p
    } else {
        dirs::home_dir().map_or(p.clone(), |h| h.join(p))
    }
}

/// IDM's dialog size text, e.g. `60.00  MB` (two spaces, as measured).
pub fn dialog_size(b: u64) -> String {
    model::size(b).replacen(' ', "  ", 1)
}

// ---- Enter new address to download (client 560 × 93) ---------------------------------------

#[derive(Default)]
pub struct AddUrl {
    pub address: String,
    pub auth: bool,
    pub login: String,
    pub password: String,
    error: Option<String>,
    /// The addresses entered before, newest first (the Address drop list).
    history: Vec<String>,
    list_open: bool,
}

pub struct NewUrl {
    pub url: Url,
    pub login: Option<(String, String)>,
}

impl AddUrl {
    /// Opens like IDM's: pre-filled with the clipboard's text when it is an http(s) URL (or a
    /// magnet link: OK then opens Add torrent).
    pub fn from_clipboard(history: &[String]) -> Self {
        let clip = arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .unwrap_or_default();
        let clip = clip.trim();
        let address = match Url::parse(clip) {
            Ok(u) if mdm_engine::supported(&u) || u.scheme() == "magnet" => clip.to_owned(),
            _ => String::new(),
        };
        AddUrl {
            address,
            history: history.to_vec(),
            ..AddUrl::default()
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<NewUrl> {
        window(
            ctx,
            "add-url",
            "Enter new address to download",
            [560.0, 93.0],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                // i18n: noun: the web address (URL)
                winui::text(
                    &p,
                    o.x + 12.0,
                    o.y + 25.0,
                    crate::i18n::t("Address"),
                    winui::TEXT(),
                );
                let addr_rc = r(o, 68.0, 11.0, 396.0, 21.0);
                let addr = winui::combo_edit(ui, addr_rc, &mut self.address);
                // An error about the address goes once it is edited.
                if addr.changed() {
                    self.error = None;
                }
                if ui.memory(|m| m.focused().is_none()) {
                    addr.request_focus();
                }
                // The arrow: the addresses entered before.
                if winui::combo_arrow(ui, addr_rc, "add-url", self.list_open).clicked()
                    || winui::combo_keys(ui, &addr)
                {
                    self.list_open = !self.list_open;
                }
                let list_was_open = self.list_open;
                if self.list_open
                    && let Some(i) = winui::drop_list(
                        ui,
                        "add-url-history",
                        addr_rc,
                        &self.history,
                        crate::i18n::t("No addresses yet"),
                        &mut self.list_open,
                    )
                {
                    self.address = self.history[i].clone();
                    addr.request_focus();
                }
                // Enter presses the default button (OK), as in IDM, wherever the focus is.
                let _ = &addr;
                let enter = winui::enter_pressed(ui);

                winui::group_box(
                    &p,
                    r(o, 11.0, 45.0, 453.0, 40.0),
                    Some(winui::caption_gap(
                        &p,
                        o.x + 20.0,
                        o.x + 124.0,
                        "Use authorization",
                        false,
                    )),
                );
                winui::checkbox(
                    ui,
                    o + vec2(20.0, 39.0),
                    &mut self.auth,
                    "Use authorization",
                    true,
                );
                let label = if self.auth {
                    winui::TEXT()
                } else {
                    winui::TEXT_DISABLED()
                };
                // i18n: noun: the user name and password section
                winui::text(&p, o.x + 84.0, o.y + 69.0, crate::i18n::t("Login"), label);
                winui::edit(
                    ui,
                    r(o, 116.0, 55.0, 117.0, 23.0),
                    &mut self.login,
                    self.auth,
                    false,
                );
                winui::text(
                    &p,
                    o.x + 289.0,
                    o.y + 69.0,
                    crate::i18n::t("Password"),
                    label,
                );
                winui::edit(
                    ui,
                    r(o, 342.0, 55.0, 113.0, 23.0),
                    &mut self.password,
                    self.auth,
                    true,
                );

                let ok = winui::button(
                    ui,
                    r(o, 475.0, 12.0, 79.0, 19.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                let cancel = winui::button(
                    ui,
                    r(o, 475.0, 40.0, 79.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                if let Some(e) = &self.error {
                    let red = winui::ERROR();
                    // Ends before the OK / Cancel buttons; whole on hover when cut.
                    // On the group box's top line, over a strip of the dialog's colour (as a
                    // caption sits on its line), not struck through by it.
                    let w = winui::text_width(&p, e).min(465.0 - 140.0) + 6.0;
                    winui::fill_l(
                        &p,
                        o.x + 137.0,
                        o.y + 39.0,
                        o.x + 137.0 + w,
                        o.y + 52.0,
                        winui::BG(),
                    );
                    winui::fit(
                        ui,
                        &p,
                        o.x + 140.0,
                        o.y + 49.0,
                        e,
                        red,
                        o.x + 465.0,
                        "add-url-error",
                    );
                }
                // Escape closes an open drop list first, then the dialog.
                if cancel || (!list_was_open && winui::escape_pressed(ui)) {
                    return Outcome::Closed;
                }
                if ok || (enter && !list_was_open) {
                    match Url::parse(self.address.trim()) {
                        Ok(url) if mdm_engine::supported(&url) || url.scheme() == "magnet" => {
                            return Outcome::Done(NewUrl {
                                url,
                                login: self
                                    .auth
                                    .then(|| (self.login.clone(), self.password.clone())),
                            });
                        }
                        _ => self.error = Some(crate::i18n::t("Invalid address").into()),
                    }
                }
                Outcome::Open
            },
        )
    }
}

// ---- Download File Info (client 570 × 208) -------------------------------------------------

pub struct FileInfo {
    pub url: Url,
    pub login: Option<(String, String)>,
    /// Shown relative to the user profile, like IDM (`Downloads\Video\a.mp4`).
    pub save_as: String,
    pub category: Category,
    pub description: String,
    /// "Remember this path for … category"; starts as Options → Save to → "Change folder for
    /// … category on last selected" says.
    pub remember: bool,
    url_text: String,
    /// The URL box holds no address MDM downloads: Start and Later wait.
    url_bad: bool,
    /// To look an edited address up again.
    rt: tokio::runtime::Handle,
    probe: Arc<Mutex<Option<Result<ProbeInfo, String>>>>,
    info: Option<Result<ProbeInfo, String>>,
    edited: bool,
    downloads: PathBuf,
    /// Options → Save to folders per category (label, folder).
    dirs: Vec<(String, String)>,
    category_open: bool,
    /// Browser request context for the download this dialog adds.
    pub context: mdm_ipc::Command,
    /// Asking to create a missing folder, the dialog's choice held meanwhile.
    asking: Option<(CreateFolder, Start)>,
    /// The download already running while the dialog shows (Options → Downloads → "Start
    /// downloading immediately while displaying Download File Info dialog").
    pub early: Option<u64>,
    /// The folders downloads were saved to before, newest first (the Save As drop list).
    pub folders: Vec<String>,
    folders_open: bool,
    /// "+" was clicked: the app opens the new-category window.
    pub new_category: bool,
}

pub enum Start {
    Now,
    Later,
}

impl FileInfo {
    /// Opens the dialog and looks the URL up in the background (name, size), like IDM.
    pub fn new(
        new: NewUrl,
        downloads: PathBuf,
        dirs: Vec<(String, String)>,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) -> Self {
        let name = mdm_engine::naming::file_name(None, &new.url);
        let category = Category::of(&name);
        let folder = crate::model::category_folder(category, &downloads, &dirs);
        let save_as = display_path(&folder.join(&name));
        let probe = start_probe(rt, ctx, &new.url, new.login.as_ref());
        FileInfo {
            url_text: new.url.to_string(),
            url_bad: false,
            rt: rt.clone(),
            url: new.url,
            login: new.login,
            save_as,
            category,
            description: String::new(),
            remember: false,
            probe,
            info: None,
            edited: false,
            downloads,
            dirs,
            category_open: false,
            context: Default::default(),
            asking: None,
            early: None,
            folders: Vec::new(),
            folders_open: false,
            new_category: false,
        }
    }

    /// The URL box changed: a new address MDM downloads is the one to download (looked up
    /// again; its file name too, unless Save As was edited); anything else holds Start and
    /// Later back.
    fn url_edited(&mut self, ctx: &egui::Context) {
        match Url::parse(self.url_text.trim()) {
            Ok(url) if mdm_engine::supported(&url) => {
                self.url_bad = false;
                if url == self.url {
                    return;
                }
                if !self.edited {
                    let name = mdm_engine::naming::file_name(None, &url);
                    self.category = Category::of(&name);
                    let dir =
                        crate::model::category_folder(self.category, &self.downloads, &self.dirs);
                    self.save_as = display_path(&dir.join(&name));
                }
                self.probe = start_probe(&self.rt, ctx, &url, self.login.as_ref());
                self.info = None;
                self.url = url;
            }
            _ => self.url_bad = true,
        }
    }

    /// Save As keeps its file name in `folder`.
    fn set_folder(&mut self, folder: &Path) {
        let name = Path::new(&self.save_as)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.save_as = display_path(&folder.join(name));
        self.edited = true;
    }

    /// "Remember this path for … category": the category and the folder to keep for it.
    pub fn remembered(&self) -> Option<(Category, String)> {
        self.remember.then(|| {
            let dir = Path::new(&self.save_as)
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            (self.category, dir)
        })
    }

    /// The address was looked up and answered (name and size known): the moment IDM starts an
    /// early download.
    pub fn probed(&self) -> bool {
        matches!(self.info, Some(Ok(_)))
    }

    pub fn size(&self) -> Option<u64> {
        self.info
            .as_ref()
            .and_then(|i| i.as_ref().ok())
            .and_then(|i| i.size)
    }

    /// The chosen destination as an absolute path.
    pub fn dest(&self) -> PathBuf {
        resolve_path(&self.save_as)
    }

    fn set_category(&mut self, c: Category) {
        self.category = c;
        let name = Path::new(&self.save_as)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.save_as =
            display_path(&crate::model::category_folder(c, &self.downloads, &self.dirs).join(name));
    }

    /// The dialog; Start (or Download Later) waits for a missing folder to be created.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        icons: &mut crate::chrome::ShellIcons,
    ) -> Outcome<Start> {
        let folder = self
            .dest()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let out = self.dialog(ctx, icons);
        ensure_folder(ctx, &mut self.asking, out, &folder)
    }

    fn dialog(
        &mut self,
        ctx: &egui::Context,
        icons: &mut crate::chrome::ShellIcons,
    ) -> Outcome<Start> {
        if self.info.is_none()
            && let Some(res) = self.probe.lock().unwrap().take()
        {
            if let (Ok(pi), false) = (&res, self.edited) {
                self.category = Category::of(&pi.file_name);
                let dir = crate::model::category_folder(self.category, &self.downloads, &self.dirs);
                self.save_as = display_path(&mdm_engine::naming::unique_path(&dir, &pi.file_name));
            }
            self.info = Some(res);
        }
        window(
            ctx,
            "file-info",
            "Download File Info",
            [570.0, 208.0],
            true,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                winui::text_right(
                    &p,
                    o.x + 88.0,
                    o.y + 18.0,
                    crate::i18n::t("URL"),
                    winui::TEXT(),
                );
                let url_rc = r(o, 96.0, 5.0, 366.0, 23.0);
                let url_box = winui::edit(ui, url_rc, &mut self.url_text, true, false);
                // An edited address is the one downloaded (it was ignored before).
                if url_box.changed() {
                    self.url_edited(ui.ctx());
                }
                if self.url_bad {
                    p.rect_stroke(
                        url_rc,
                        0.0,
                        egui::Stroke::new(1.0, winui::ERROR()),
                        egui::StrokeKind::Inside,
                    );
                }

                // i18n: noun: label before the list of categories
                winui::text_right(
                    &p,
                    o.x + 88.0,
                    o.y + 46.0,
                    crate::i18n::t("Category"),
                    winui::TEXT(),
                );
                let cat_rect = r(o, 96.0, 36.0, 174.0, 21.0);
                let shown = crate::i18n::t(&self.category.label()).to_owned();
                let cat = winui::combo(ui, cat_rect, &shown, "category");
                if cat.clicked() || winui::combo_keys(ui, &cat) {
                    self.category_open = !self.category_open;
                }
                let category_was_open = self.category_open;
                // "+": a new category (the app's category window).
                if winui::small_button(ui, r(o, 276.0, 35.0, 22.0, 21.0), "+").clicked() {
                    self.new_category = true;
                }

                winui::text_right(
                    &p,
                    o.x + 88.0,
                    o.y + 77.0,
                    crate::i18n::t("Save As"),
                    winui::TEXT(),
                );
                let save_rc = r(o, 96.0, 63.0, 335.0, 21.0);
                let save = winui::combo_edit(ui, save_rc, &mut self.save_as);
                if save.changed() {
                    self.edited = true;
                }
                // The arrow: the folders saved to before.
                if winui::combo_arrow(ui, save_rc, "save-as", self.folders_open).clicked()
                    || winui::combo_keys(ui, &save)
                {
                    self.folders_open = !self.folders_open;
                }
                let folders_were_open = self.folders_open;
                if self.folders_open {
                    let shown: Vec<String> = self
                        .folders
                        .iter()
                        .map(|f| display_path(Path::new(f)))
                        .collect();
                    if let Some(i) = winui::drop_list(
                        ui,
                        "save-as-history",
                        save_rc,
                        &shown,
                        crate::i18n::t("No folders yet"),
                        &mut self.folders_open,
                    ) {
                        let folder = PathBuf::from(&self.folders[i]);
                        self.set_folder(&folder);
                    }
                }
                // "...": choose the folder.
                if winui::small_button(ui, r(o, 439.0, 64.0, 22.0, 21.0), "...").clicked() {
                    let now = self
                        .dest()
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_default();
                    // i18n: title of the folder picker
                    let mut pick =
                        rfd::FileDialog::new().set_title(crate::i18n::t("Select a folder"));
                    if now.is_dir() {
                        pick = pick.set_directory(&now);
                    }
                    if let Some(dir) = pick.pick_folder() {
                        self.set_folder(&dir);
                    }
                }

                let caption = crate::i18n::fmt(
                    crate::i18n::t("Remember this path for \"{category}\" category"),
                    &[("category", crate::i18n::t(&self.category.label()))],
                );
                let gap_end = o.x + 122.0 + winui::text_width(&p, &caption) + 5.0;
                winui::group_box(
                    &p,
                    r(o, 96.0, 97.0, 366.0, 45.0),
                    Some((o.x + 104.0, gap_end)),
                );
                winui::checkbox(
                    ui,
                    o + vec2(104.0, 90.0),
                    &mut self.remember,
                    &caption,
                    true,
                );
                let mut folder = format!(
                    "{}\\",
                    display_path(&crate::model::category_folder(
                        self.category,
                        &self.downloads,
                        &self.dirs
                    ))
                    .trim_end_matches('\\')
                );
                let folder_rc = r(o, 104.0, 112.0, 348.0, 23.0);
                winui::edit(ui, folder_rc, &mut folder, false, false);
                // Greyed as IDM shows it, and still copyable.
                winui::copyable(ui, folder_rc, "remembered-folder", &folder, false);

                // i18n: noun: label before a free-text box describing the file
                winui::text_right(
                    &p,
                    o.x + 87.0,
                    o.y + 161.0,
                    crate::i18n::t("Description"),
                    winui::TEXT(),
                );
                winui::edit(
                    ui,
                    r(o, 96.0, 148.0, 366.0, 23.0),
                    &mut self.description,
                    true,
                    false,
                );

                let (later_kind, start_kind) = if self.url_bad {
                    (ButtonKind::Disabled, ButtonKind::Disabled)
                } else {
                    (ButtonKind::Normal, ButtonKind::Default)
                };
                let later = winui::button(
                    ui,
                    r(o, 97.0, 180.0, 109.0, 21.0),
                    "Download Later",
                    later_kind,
                    false,
                    Some(9),
                );
                let later = winui::why_disabled(later, self.url_bad, "Invalid address");
                let start = winui::button(
                    ui,
                    r(o, 225.0, 180.0, 109.0, 21.0),
                    "Start Download",
                    start_kind,
                    true,
                    Some(0),
                );
                let start = winui::why_disabled(start, self.url_bad, "Invalid address");
                let cancel = winui::button(
                    ui,
                    r(o, 352.0, 180.0, 109.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                    Some(0),
                );

                // Right column, centred on x ≈ 516: file icon, size, Preview.
                // The shell's own 32 px icon for this file type, as IDM shows.
                let ext = Path::new(&self.save_as)
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                icons.paint(
                    ui,
                    crate::chrome::IconKey::ExtLarge(ext),
                    o + vec2(500.0, 45.0),
                );
                let (size, reason) = match &self.info {
                    None => ("...".to_owned(), None),
                    Some(Ok(pi)) => (
                        // i18n: a download's size is not known (the server didn't say)
                        pi.size
                            .map_or(crate::i18n::t("unknown").to_owned(), dialog_size),
                        None,
                    ),
                    // i18n: in Download File Info, where the size goes: the address didn't answer
                    Some(Err(e)) => (crate::i18n::t("Error").to_owned(), Some(e.clone())),
                };
                let w = winui::text_width(&p, &size);
                let at = winui::text(&p, o.x + 515.0 - w / 2.0, o.y + 103.0, &size, winui::TEXT());
                // Why the address didn't answer (not found, sign-in needed…): on hover, copyable.
                if let Some(reason) = reason {
                    winui::copyable(ui, at, "file-info-error", &reason, true);
                }
                winui::button(
                    ui,
                    r(o, 480.0, 128.0, 73.0, 21.0),
                    // i18n: verb: open the part already downloaded
                    "Preview",
                    ButtonKind::Disabled,
                    false,
                    None,
                )
                .on_hover_text(crate::i18n::t(
                    // i18n: tooltip of the greyed Preview button in Download File Info
                    "Preview isn't available yet",
                ));

                // The categories in a list window of their own (the shared drop list: keys,
                // Escape, never cut off by this short window).
                if self.category_open {
                    let all = Category::all();
                    let labels: Vec<String> = all
                        .iter()
                        .map(|c| crate::i18n::t(&c.label()).to_owned())
                        .collect();
                    if let Some(i) = winui::drop_list(
                        ui,
                        "file-info-category",
                        cat_rect,
                        &labels,
                        "",
                        &mut self.category_open,
                    ) && let Some(c) = all.get(i)
                    {
                        self.set_category(*c);
                    }
                }

                // Escape closes an open drop list first, then the dialog.
                let key =
                    |k| !folders_were_open && !category_was_open && ui.input(|i| i.key_pressed(k));
                if cancel.clicked() || key(egui::Key::Escape) {
                    Outcome::Closed
                } else if self.url_bad {
                    Outcome::Open
                } else if start.clicked() || key(egui::Key::Enter) {
                    Outcome::Done(Start::Now)
                } else if later.clicked() {
                    Outcome::Done(Start::Later)
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// Look `url` up in the background (name, size), the answer landing in the slot returned.
fn start_probe(
    rt: &tokio::runtime::Handle,
    ctx: &egui::Context,
    url: &Url,
    login: Option<&(String, String)>,
) -> Arc<Mutex<Option<Result<ProbeInfo, String>>>> {
    let probe = Arc::new(Mutex::new(None));
    let (slot, url, ctx) = (probe.clone(), url.clone(), ctx.clone());
    let headers = crate::app::auth_headers(login);
    rt.spawn(async move {
        let res = mdm_engine::probe(url, headers, &Options::default())
            .await
            .map_err(|e| e.to_string());
        *slot.lock().unwrap() = Some(res);
        ctx.request_repaint();
    });
    probe
}

// ---- Download complete (client 455 × 215) --------------------------------------------------

pub struct Complete {
    pub id: u64,
    pub url: String,
    pub path: PathBuf,
    pub size: u64,
    pub dont_show: bool,
}

impl Complete {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        icons: &mut crate::chrome::ShellIcons,
    ) -> Outcome<()> {
        let id = format!("complete-{}", self.id);
        window(
            ctx,
            &id,
            "Download complete",
            [455.0, 215.0],
            true,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                // Windows' own open-folder icon (IDM shows an open folder here).
                icons.paint(
                    ui,
                    crate::chrome::IconKey::FolderOpenLarge,
                    o + vec2(10.0, 11.0),
                );
                winui::text(
                    &p,
                    o.x + 51.0,
                    o.y + 21.0,
                    crate::i18n::t("Download complete"),
                    winui::TEXT(),
                );
                let line = crate::i18n::fmt(
                    crate::i18n::t("Downloaded {size} ({bytes} Bytes)"),
                    &[
                        ("size", &dialog_size(self.size)),
                        ("bytes", &crate::i18n::integer(self.size)),
                    ],
                );
                winui::text(&p, o.x + 51.0, o.y + 34.0, &line, winui::TEXT());
                // i18n: noun: the web address (URL)
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 72.0,
                    crate::i18n::t("Address"),
                    winui::TEXT(),
                );
                // Read-only and copyable (an editable copy let typing seem to work, then reverted).
                winui::readonly_field(
                    ui,
                    r(o, 11.0, 78.0, 434.0, 23.0),
                    &self.url,
                    winui::BG(),
                    true,
                    "complete-url",
                );
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 117.0,
                    crate::i18n::t("The file saved as"),
                    winui::TEXT(),
                );
                winui::readonly_field(
                    ui,
                    r(o, 11.0, 124.0, 434.0, 23.0),
                    &display_path(&self.path),
                    winui::BG(),
                    true,
                    "complete-saved",
                );

                let btn = |ui: &mut Ui, x: f32, label: &str, kind| {
                    winui::button(ui, r(o, x, 157.0, 90.0, 21.0), label, kind, false, None)
                };
                // i18n: verb: opens the downloaded file
                let open = btn(ui, 12.0, "Open", ButtonKind::Default);
                let with = btn(ui, 115.0, "Open with...", ButtonKind::Normal);
                let folder = btn(ui, 219.0, "Open folder", ButtonKind::Normal);
                // i18n: verb: closes this window
                let close = btn(ui, 354.0, "Close", ButtonKind::Normal);
                winui::checkbox(
                    ui,
                    o + vec2(24.0, 191.0),
                    &mut self.dont_show,
                    "Don't show this dialog again",
                    true,
                );

                // Drag handle: white box with a 1 px #828790 border; drag it to drop the file into
                // Explorer, a mail or a chat (IDM 5.05).
                let handle = r(o, 414.0, 187.0, 27.0, 20.0);
                let grip = ui.interact(
                    handle,
                    egui::Id::unique(("drag-out", self.id)),
                    egui::Sense::drag(),
                );
                if grip.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                #[cfg(windows)]
                if grip.drag_started() && self.path.exists() {
                    let _ = crate::win32::drag_file_out("Download complete", &self.path);
                }
                p.rect_filled(handle, 0.0, winui::FIELD());
                p.rect_stroke(
                    handle,
                    0.0,
                    egui::Stroke::new(1.0, winui::FIELD_EDGE()),
                    egui::StrokeKind::Inside,
                );
                crate::icons::Icon::DragOut.image(true).paint_at(
                    ui,
                    egui::Rect::from_center_size(handle.center(), vec2(16.0, 16.0)),
                );

                // Enter: Open, the default button (as in the other dialogs).
                if open.clicked() || winui::enter_pressed(ui) {
                    let _ = open::that_detached(&self.path);
                    Outcome::Done(())
                } else if with.clicked() {
                    open_with(&self.path);
                    Outcome::Done(())
                } else if folder.clicked() {
                    show_in_folder(&self.path);
                    Outcome::Done(())
                } else if close.clicked() || winui::escape_pressed(ui) {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

// ---- Speed Limiter settings (client 453 × 145, light) --------------------------------------

/// Downloads → Speed Limiter → Settings... (notes/ui-spec/scheduler-and-misc.md §2).
pub struct SpeedSettings {
    pub kb: String,
    pub on_startup: bool,
    /// Beyond IDM (D-012): one limit for all downloads together.
    pub global_on: bool,
    pub global_kb: String,
    pub only_active: bool,
    /// How much taller the note is than IDM's three lines (a longer translation): the rows
    /// below it and the window move down by this much (from the frame before).
    note_extra: f32,
}

/// What Speed Limiter settings → OK sets.
pub struct SpeedChoice {
    pub kb: u64,
    pub on_startup: bool,
    pub global_on: bool,
    pub global_kb: u64,
    pub only_active: bool,
}

impl SpeedSettings {
    pub fn new(s: &crate::settings::Settings) -> Self {
        SpeedSettings {
            kb: s.speed_limit_kb.to_string(),
            on_startup: s.limiter_on_startup,
            global_on: s.global_limit_on,
            global_kb: s.global_limit_kb.to_string(),
            only_active: s.global_only_active,
            note_extra: 0.0,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<SpeedChoice> {
        let extra = self.note_extra;
        window(
            ctx,
            "speed-settings",
            "Speed Limiter settings",
            [453.0, 197.0 + extra],
            false,
            winui::LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let black = winui::LIGHT_TEXT();
                // The label ends where `text_right(265)` ended it: at 266.
                let kb_box = winui::sentence(
                    ui,
                    &p,
                    // i18n: {kb} is an input box (kilobytes per second), drawn where it stands in the translation
                    crate::i18n::t("Maximum download speed for one file {kb} KBytes/sec"),
                    &[winui::SentenceLine {
                        lead: winui::Lead::Right(o.x + 266.0),
                        lead_baseline: o.y + 21.0,
                        boxes: o + vec2(275.0, 7.0),
                        gap: 11.0,
                        baseline: o.y + 21.0,
                    }],
                    &[("kb", vec2(50.0, 23.0))],
                    |p, x, baseline, s| {
                        winui::text(p, x, baseline, s, black);
                    },
                );
                winui::edit_light(ui, kb_box[0], &mut self.kb, true, true);
                self.kb.retain(|c| c.is_ascii_digit());
                let note = crate::i18n::t(
                    "Note: Internet servers may break connection when the speed is too limited! Thus it's not\nrecommended to use Speed Limiter to download from servers that do not support\n\"resume\" feature.",
                );
                // Wrapped inside the window (IDM's English runs to x 440); a translation longer than three
                // lines pushes the rest down and the window grows.
                let lines =
                    winui::paragraph(&p, o.x + 11.0, o.y + 47.0, note, black, o.x + 449.0, 13.0);
                self.note_extra = lines.saturating_sub(3) as f32 * 13.0;
                let o = o + vec2(0.0, self.note_extra);
                winui::checkbox_light(
                    ui,
                    o + vec2(11.0, 86.0),
                    &mut self.on_startup,
                    "Always turn on Speed Limiter on Mister Download Manager startup",
                    true,
                );
                // Beyond IDM: a limit for all downloads together.
                winui::etched_h(&p, o.x + 11.0, o.y + 108.0, 431.0);
                winui::checkbox_light_until(
                    ui,
                    o + vec2(11.0, 121.0),
                    &mut self.global_on,
                    "Limit all downloads together to",
                    true,
                    Some(o.x + 196.0),
                );
                let on = self.global_on;
                winui::edit_light(
                    ui,
                    r(o, 196.0, 116.0, 60.0, 23.0),
                    &mut self.global_kb,
                    on,
                    true,
                );
                self.global_kb.retain(|c| c.is_ascii_digit());
                let c = if on { black } else { winui::LIGHT_DISABLED() };
                // i18n: unit after an input box: kilobytes per second
                winui::text(
                    &p,
                    o.x + 264.0,
                    o.y + 131.0,
                    crate::i18n::t("KBytes/sec"),
                    c,
                );
                winui::checkbox_light(
                    ui,
                    o + vec2(29.0, 143.0),
                    &mut self.only_active,
                    "Only while I'm using the computer",
                    on,
                );
                let ok = winui::button_light(
                    ui,
                    r(o, 285.0, 164.0, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    true,
                )
                .clicked();
                let cancel = winui::button_light(
                    ui,
                    r(o, 369.0, 164.0, 73.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                if cancel || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if ok || winui::enter_pressed(ui) {
                    Outcome::Done(SpeedChoice {
                        kb: self.kb.parse().unwrap_or(10).max(1),
                        on_startup: self.on_startup,
                        global_on: self.global_on,
                        global_kb: self.global_kb.parse().unwrap_or(500).max(1),
                        only_active: self.only_active,
                    })
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

// ---- Paragraphs that grow their dialog -------------------------------------------------------

/// The height a dialog adds for its paragraph `key` (13 px a line beyond the lines its layout
/// planned), as the paragraph measured it last frame ([`grow_to`]): a long translation or name
/// moves what is under it down and the window grows, instead of running over the controls.
pub fn grow_extra(ctx: &egui::Context, key: &str) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(egui::Id::unique(("grow", key))))
        .unwrap_or(0.0)
}

/// Records that paragraph `key` took `lines` lines where `planned` fit (see [`grow_extra`]); a
/// change draws again at once, at the new size.
pub fn grow_to(ctx: &egui::Context, key: &str, lines: usize, planned: usize) {
    let extra = lines.saturating_sub(planned) as f32 * 13.0;
    if extra != grow_extra(ctx, key) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("grow", key)), extra));
        ctx.request_repaint();
    }
}

// ---- Message (light) --------------------------------------------------------------------------

/// A notice with OK, sized to its text (crate::i18n::t("Cannot find …")): one line when it
/// fits, wrapped and taller when it doesn't (a long path or reason is never cut off).
pub fn message(ctx: &egui::Context, id: &str, title: &str, text: &str) -> Outcome<()> {
    // Tahoma −11 averages about 5.5 px a character; the box is kept between 260 and 480 px.
    let width = (text.chars().count() as f32 * 5.6 + 40.0).clamp(260.0, 480.0);
    let extra = grow_extra(ctx, id);
    window(
        ctx,
        id,
        title,
        [width, 80.0 + extra],
        false,
        winui::LIGHT_BG(),
        |ui, o| {
            let p = ui.painter().clone();
            let lines = winui::paragraph(
                &p,
                o.x + 16.0,
                o.y + 24.0,
                text,
                winui::LIGHT_TEXT(),
                o.x + width - 16.0,
                13.0,
            );
            grow_to(ui.ctx(), id, lines, 1);
            let ok = winui::button_light(
                ui,
                r(o, (width - 73.0) / 2.0, 47.0 + extra, 73.0, 21.0),
                "OK",
                ButtonKind::Default,
                false,
            )
            .clicked();
            let keys =
                ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
            if ok || keys {
                Outcome::Closed
            } else {
                Outcome::Open
            }
        },
    )
}

// ---- Download limit reached (light) ---------------------------------------------------------

/// "Show warning before stopping downloads" (Scheduler → Download limits): says the limit was
/// reached and when downloads go on. IDM's own wording is [unknown] (lab).
pub struct LimitNotice {
    lines: [String; 2],
}

impl LimitNotice {
    pub fn new(mb: u32, hours: u32, wait: std::time::Duration) -> Self {
        let at = chrono::Local::now()
            + chrono::Duration::from_std(wait).unwrap_or_else(|_| chrono::Duration::zero());
        LimitNotice {
            lines: [
                crate::i18n::fmt(
                    crate::i18n::t(
                        "The download limit ({mb} MBytes every {hours} hours) has been reached.",
                    ),
                    &[
                        ("mb", &crate::i18n::integer(u64::from(mb))),
                        ("hours", &crate::i18n::integer(u64::from(hours))),
                    ],
                ),
                crate::i18n::fmt(
                    crate::i18n::t("Downloads stopped and will resume automatically at {time}."),
                    &[(
                        "time",
                        &at.format(if crate::i18n::code() == "en" {
                            "%-I:%M %p"
                        } else {
                            "%H:%M"
                        })
                        .to_string(),
                    )],
                ),
            ],
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        window(
            ctx,
            "limit-notice",
            "Download limits",
            [420.0, 90.0],
            false,
            winui::LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                for (i, line) in self.lines.iter().enumerate() {
                    winui::fit(
                        ui,
                        &p,
                        o.x + 11.0,
                        o.y + 21.0 + 13.0 * i as f32,
                        line,
                        winui::LIGHT_TEXT(),
                        o.x + 409.0,
                        ("notice-line", i),
                    );
                }
                let ok = winui::button_light(
                    ui,
                    r(o, 173.0, 57.0, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                )
                .clicked();
                let keys = ui
                    .input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
                if ok || keys {
                    Outcome::Closed
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

// ---- Turn off computer countdown (light) ---------------------------------------------------

/// After a queue with "Turn off computer when done": a 30 s countdown that can be cancelled
/// (or skipped) before the computer shuts down, sleeps or hibernates. IDM's own prompt is
/// [unknown] (lab); turning a computer off unannounced is not acceptable.
pub struct TurnOffCountdown {
    mode: crate::queue::TurnOff,
    force: bool,
    deadline: std::time::Instant,
}

impl TurnOffCountdown {
    pub const SECONDS: u64 = 30;

    pub fn new(mode: crate::queue::TurnOff, force: bool) -> Self {
        TurnOffCountdown {
            mode,
            force,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(Self::SECONDS),
        }
    }

    /// `Done` when the time is up or "Turn off now" was pressed; `Closed` on Cancel.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        use crate::queue::TurnOff;
        let left = self
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        // Whole sentences: the verb's form and place differ between languages.
        let sentence = match self.mode {
            TurnOff::ShutDown => crate::i18n::t("The computer will shut down in {n} seconds."),
            TurnOff::Sleep => crate::i18n::t("The computer will go to sleep in {n} seconds."),
            TurnOff::Hibernate => crate::i18n::t("The computer will hibernate in {n} seconds."),
        };
        question(
            ctx,
            "turn-off",
            "Mister Download Manager",
            [374.0, 100.0],
            false,
            winui::LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let black = winui::LIGHT_TEXT();
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 21.0,
                    crate::i18n::t("The download queue is done."),
                    black,
                );
                let line = crate::i18n::fmt(
                    sentence,
                    &[(
                        "n",
                        &crate::i18n::integer(left.as_secs() + u64::from(left.subsec_millis() > 0)),
                    )],
                );
                winui::text(&p, o.x + 11.0, o.y + 34.0, &line, black);
                let now = winui::button_light(
                    ui,
                    r(o, 94.0, 63.0, 90.0, 21.0),
                    "Turn off now",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                let cancel = winui::button_light(
                    ui,
                    r(o, 194.0, 63.0, 73.0, 21.0),
                    "Cancel",
                    ButtonKind::Default,
                    false,
                )
                .clicked();
                // Enter: Cancel, the default button.
                if cancel || winui::escape_pressed(ui) || winui::enter_pressed(ui) {
                    Outcome::Closed
                } else if now || left.is_zero() {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        )
    }

    /// Shut down, sleep or hibernate now.
    pub fn execute(&self) {
        use crate::queue::TurnOff;
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut cmd = std::process::Command::new("shutdown");
            match self.mode {
                TurnOff::ShutDown => cmd.args(["/s", "/t", "0"]),
                TurnOff::Hibernate => cmd.arg("/h"),
                TurnOff::Sleep => {
                    crate::win32::sleep(self.force);
                    return;
                }
            };
            // "Force processes to terminate": don't wait for programs that hold it up.
            if self.force {
                cmd.arg("/f");
            }
            // No console window flashing up (CREATE_NO_WINDOW).
            let _ = cmd.creation_flags(0x0800_0000).spawn();
        }
        // Linux: systemd, as the desktop's own menu does; forcing ignores the programs that ask
        // to delay it (inhibitors).
        #[cfg(target_os = "linux")]
        {
            let verb = match self.mode {
                TurnOff::ShutDown => "poweroff",
                TurnOff::Hibernate => "hibernate",
                TurnOff::Sleep => "suspend",
            };
            let mut cmd = std::process::Command::new("systemctl");
            cmd.arg(verb);
            if self.force {
                cmd.arg("--ignore-inhibitors");
            }
            let _ = cmd.spawn();
        }
        // macOS: through System Events, as the Apple menu does (macOS has no separate
        // hibernate: its sleep writes memory to disk as set up); apps can still object.
        #[cfg(target_os = "macos")]
        {
            let script = match self.mode {
                TurnOff::ShutDown => "tell application \"System Events\" to shut down",
                TurnOff::Hibernate | TurnOff::Sleep => {
                    "tell application \"System Events\" to sleep"
                }
            };
            let _ = std::process::Command::new("osascript")
                .args(["-e", script])
                .spawn();
            let _ = self.force;
        }
    }
}

// ---- Confirm deletion of downloads (client 374 × 120, light) ----------------------------------

/// "Confirm deletion of downloads": Delete / Remove / Delete All Completed. Asks whether the
/// files go too (user, 2026-10-29: IDM's checkbox; for an unfinished download its data so far),
/// for one download or several. `Done((files, dont_show_again))` on Yes.
pub struct ConfirmDelete {
    pub ids: Vec<u64>,
    /// The file name when there is one download.
    name: String,
    /// One download, not complete (IDM's wording).
    unfinished: bool,
    files: bool,
    dont_show: bool,
    /// The files box's answer becomes the next one's default (not after Shift+Delete).
    pub remember: bool,
}

impl ConfirmDelete {
    /// `files`: the checkbox's first state (the last answer, or Shift+Delete).
    pub fn new(ids: Vec<u64>, name: String, unfinished: bool, files: bool) -> Self {
        ConfirmDelete {
            ids,
            name,
            unfinished,
            files,
            dont_show: false,
            remember: true,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<(bool, bool)> {
        let n = self.ids.len() as u64;
        let extra = grow_extra(ctx, "confirm-delete");
        question(
            ctx,
            "confirm-delete",
            "Confirm deletion of downloads",
            [374.0, 146.0 + extra],
            false,
            winui::LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let black = winui::LIGHT_TEXT();
                let text = if n == 1 && self.unfinished {
                    crate::i18n::t(
                        "The selected download is not complete. Are you sure you want to delete
it from Mister Download Manager list of downloads?",
                    )
                    .to_owned()
                } else if n == 1 {
                    // The whole name (two similar names must stay apart): the box grows.
                    let name = self.name.clone();
                    crate::i18n::fmt(
                        crate::i18n::t(
                            "Are you sure you want to delete \"{name}\" from Mister Download Manager list of downloads?",
                        ),
                        // A file name keeps its own direction (an Arabic one reads right to left).
                        &[("name", &crate::i18n::isolate(&name))],
                    )
                } else {
                    // Two or more (one download has its own sentences above); the singular form
                    // is for languages whose "one" form also counts 21, 31…
                    crate::i18n::fmt(
                        // i18n: never shown for a single download
                        &crate::i18n::tn(
                            n,
                            "Are you sure you want to delete {n} download from Mister Download Manager list of downloads?",
                            "Are you sure you want to delete these {n} downloads from Mister Download Manager list of downloads?",
                        ),
                        &[("n", &crate::i18n::integer(n))],
                    )
                };
                let lines =
                    winui::paragraph(&p, o.x + 11.0, o.y + 21.0, &text, black, o.x + 362.0, 13.0);
                grow_to(ui.ctx(), "confirm-delete", lines, 3);
                let o = o + vec2(0.0, extra);
                let files = if n == 1 {
                    crate::i18n::t("Also delete the file from disk").to_owned()
                } else {
                    crate::i18n::fmt(
                        // i18n: never shown for a single file
                        &crate::i18n::tn(
                            n,
                            "Also delete the {n} file from disk",
                            "Also delete the {n} files from disk",
                        ),
                        &[("n", &crate::i18n::integer(n))],
                    )
                };
                winui::checkbox_light(ui, o + vec2(24.0, 60.0), &mut self.files, &files, true);
                let yes = winui::button_light(
                    ui,
                    r(o, 94.0, 86.0, 73.0, 21.0),
                    "Yes",
                    ButtonKind::Default,
                    false,
                )
                .clicked();
                let no = winui::button_light(
                    ui,
                    r(o, 207.0, 86.0, 73.0, 21.0),
                    "No",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                winui::checkbox_light(
                    ui,
                    o + vec2(24.0, 120.0),
                    &mut self.dont_show,
                    "Don't show this dialog again",
                    true,
                );
                if no || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if yes || winui::enter_pressed(ui) {
                    Outcome::Done((self.files, self.dont_show))
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// Select the file in Explorer (Finder, the Linux file manager). A file that is gone: its
/// folder, if that is still there.
pub fn show_in_folder(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if !path.exists() {
            if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
                let _ = open::that_detached(dir);
            }
            return;
        }
        // Explorer reads `/select,"C:\a b\c.mp4"` — quoted after the comma. Rust's own quoting
        // would wrap the whole argument ("/select,C:\a b\c.mp4"), which Explorer doesn't
        // understand: it opened its default folder for any path with a space (user report
        // 2026-10-29, a user folder with a space in its name). Windows paths can't contain a
        // quote.
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(unix)]
    crate::unix::show_in_folder(path);
}

pub fn open_with(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("rundll32")
            .arg("shell32.dll,OpenAs_RunDLL")
            .arg(path)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = open::that_detached(path);
    }
}

/// "Duplicate download link": the address is already in the list (Options → Downloads → "If a
/// duplicate download link is added: Show a dialog and ask what to do"). `Done` with the
/// choice, numbered as Options lists them (1 numbered name, 2 overwrite, 3 open or resume).
pub struct Duplicate {
    name: String,
    choice: usize,
}

impl Duplicate {
    pub fn new(name: String) -> Self {
        Duplicate { name, choice: 1 }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<usize> {
        question(
            ctx,
            "duplicate",
            "Duplicate download link",
            [460.0, 156.0],
            false,
            winui::LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let ink = winui::LIGHT_TEXT();
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 21.0,
                    crate::i18n::t("This address is already in the list of downloads:"),
                    ink,
                );
                let name = crate::i18n::isolate(&self.name);
                let (rect, cut) =
                    winui::text_fit(&p, o.x + 11.0, o.y + 39.0, &name, ink, o.x + 449.0);
                if cut {
                    winui::full_on_hover(ui, rect, "duplicate-name", &self.name);
                }
                let mut group = Vec::new();
                for (k, label) in crate::options::DUPLICATE.iter().enumerate().skip(1) {
                    let y = 52.0 + 20.0 * (k - 1) as f32;
                    let resp =
                        winui::radio_light(ui, o + vec2(14.0, y), self.choice == k, label, true);
                    if resp.clicked() {
                        self.choice = k;
                    }
                    group.push(resp);
                }
                if let Some(j) = winui::radio_keys(ui, &group) {
                    self.choice = j + 1;
                }
                let ok = winui::button_light(
                    ui,
                    r(o, 295.0, 124.0, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                )
                .clicked();
                let cancel = winui::button_light(
                    ui,
                    r(o, 376.0, 124.0, 73.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                if cancel || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if ok || winui::enter_pressed(ui) {
                    Outcome::Done(self.choice)
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// The queue selection panel (Options → Downloads → "Show queue selection panel on pressing
/// Download Later" / "on closing batch download dialogs"): which queue the downloads join, and
/// whether it starts now. `Done((queue, start))`; Cancel leaves them in the list, in no queue.
pub struct QueuePick {
    names: Vec<String>,
    chosen: usize,
    start: bool,
}

impl QueuePick {
    pub fn new(names: Vec<String>) -> Self {
        QueuePick {
            names,
            chosen: 0,
            start: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<(usize, bool)> {
        window(
            ctx,
            "queue-pick",
            "Download later",
            [340.0, 110.0],
            false,
            winui::LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                winui::text_right(
                    &p,
                    o.x + 92.0,
                    o.y + 26.0,
                    crate::i18n::t("Add to queue:"),
                    winui::LIGHT_TEXT(),
                );
                let names: Vec<&str> = self.names.iter().map(String::as_str).collect();
                if let Some(i) = winui::combo_light(
                    ui,
                    r(o, 100.0, 12.0, 228.0, 21.0),
                    &names,
                    self.chosen,
                    true,
                    "queue-pick",
                ) {
                    self.chosen = i;
                }
                winui::checkbox_light(
                    ui,
                    o + vec2(100.0, 44.0),
                    &mut self.start,
                    "Start the queue now",
                    true,
                );
                let ok = winui::button_light(
                    ui,
                    r(o, 174.0, 78.0, 73.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                )
                .clicked();
                let cancel = winui::button_light(
                    ui,
                    r(o, 255.0, 78.0, 73.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                if cancel || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if ok || winui::enter_pressed(ui) {
                    Outcome::Done((self.chosen, self.start))
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// A question with Yes / No (deleting a queue…). `Done` on Yes.
pub struct Confirm {
    title: &'static str,
    text: String,
}

impl Confirm {
    pub fn new(title: &'static str, text: String) -> Self {
        Confirm { title, text }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        let text = self.text.clone();
        let key = format!("confirm-{}", self.title);
        let extra = grow_extra(ctx, &key);
        question(
            ctx,
            &key,
            self.title,
            [400.0, 96.0 + extra],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let lines = winui::paragraph(
                    &p,
                    o.x + 11.0,
                    o.y + 22.0,
                    &text,
                    winui::TEXT(),
                    o.x + 389.0,
                    13.0,
                );
                grow_to(ui.ctx(), &key, lines, 3);
                let o = o + vec2(0.0, extra);
                let yes = winui::button(
                    ui,
                    r(o, 234.0, 64.0, 73.0, 21.0),
                    "Yes",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                let no = winui::button(
                    ui,
                    r(o, 316.0, 64.0, 73.0, 21.0),
                    "No",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                if no || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if yes || winui::enter_pressed(ui) {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// "This folder does not exist … Do you want to create it?" — asked when a download is about to
/// be added to a folder that isn't there (Windows' question, Yes / No). Yes creates it.
pub struct CreateFolder {
    folder: PathBuf,
    error: Option<String>,
}

impl CreateFolder {
    pub fn new(folder: PathBuf) -> Self {
        CreateFolder {
            folder,
            error: None,
        }
    }

    /// `Done` once the folder exists (created on Yes); `Closed` on No, Escape or the close box.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<()> {
        let folder = self.folder.display().to_string();
        question(
            ctx,
            "create-folder",
            "Create folder",
            [420.0, 128.0],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let ink = winui::TEXT();
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 21.0,
                    crate::i18n::t("This folder does not exist:"),
                    ink,
                );
                let (rect, cut) =
                    winui::text_fit(&p, o.x + 11.0, o.y + 39.0, &folder, ink, o.x + 409.0);
                if cut {
                    winui::full_on_hover(ui, rect, "create-folder-path", &folder);
                }
                winui::text(
                    &p,
                    o.x + 11.0,
                    o.y + 57.0,
                    crate::i18n::t("Do you want to create it?"),
                    ink,
                );
                if let Some(e) = &self.error {
                    let (rect, cut) =
                        winui::text_fit(&p, o.x + 11.0, o.y + 75.0, e, winui::ERROR(), o.x + 409.0);
                    if cut {
                        winui::full_on_hover(ui, rect, "create-folder-error", e);
                    }
                }
                let yes = winui::button(
                    ui,
                    r(o, 254.0, 94.0, 73.0, 21.0),
                    "Yes",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                let no = winui::button(
                    ui,
                    r(o, 336.0, 94.0, 73.0, 21.0),
                    "No",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                if no || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if yes || winui::enter_pressed(ui) {
                    match std::fs::create_dir_all(&self.folder) {
                        Ok(()) => Outcome::Done(()),
                        Err(e) => {
                            self.error = Some(crate::i18n::fmt(
                                crate::i18n::t("The folder could not be created: {error}"),
                                &[("error", &e.to_string())],
                            ));
                            Outcome::Open
                        }
                    }
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// Holds a dialog's "start" (`out`) until `folder` exists: when it is missing, asks to create
/// it ([`CreateFolder`]) and keeps the dialog's result meanwhile; No goes back to the dialog.
/// `asking` lives in the dialog, across frames.
pub fn ensure_folder<T>(
    ctx: &egui::Context,
    asking: &mut Option<(CreateFolder, T)>,
    out: Outcome<T>,
    folder: &Path,
) -> Outcome<T> {
    match out {
        Outcome::Closed => {
            *asking = None;
            return Outcome::Closed;
        }
        Outcome::Done(v) if asking.is_none() => {
            if folder.as_os_str().is_empty() || folder.is_dir() {
                return Outcome::Done(v);
            }
            *asking = Some((CreateFolder::new(folder.to_owned()), v));
        }
        // While the question is up the dialog's own "start" waits for the answer.
        _ => {}
    }
    let Some((ask, _)) = asking.as_mut() else {
        return Outcome::Open;
    };
    match ask.show(ctx) {
        Outcome::Open => Outcome::Open,
        Outcome::Closed => {
            *asking = None;
            Outcome::Open
        }
        Outcome::Done(()) => asking
            .take()
            .map_or(Outcome::Open, |(_, v)| Outcome::Done(v)),
    }
}

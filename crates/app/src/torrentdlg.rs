//! Add torrent (M15, D-014; ours — IDM has no torrents). A magnet link or a .torrent file, its
//! file list once known (a magnet asks the swarm for it), which files to download, and where.
//! It follows the app's mode (dark or light) like Add URL: glyphs are rendered for the main
//! window's mode, so a light dialog in dark mode would get heavy text.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Color32, Pos2, Rect, Stroke, Ui, pos2, vec2};
use mdm_torrent::{Meta, Source};

use crate::dialogs::{Outcome, window};
use crate::model::{Category, category_folder};
use crate::torrents::Host;
// `fill` takes English pixels (made where they are on screen), `text` an English pen.
use crate::winui::{self, ButtonKind, fill_l as fill, r, text};

/// How long a magnet may take to get its file list from peers.
const METADATA_WAIT: Duration = Duration::from_secs(180);
const ROW: f32 = 17.0;

type Slot = Arc<Mutex<Option<Result<Meta, String>>>>;

crate::theme::themed! {
    LIST_HEAD = rgb(0x19, 0x19, 0x19), rgb(0xf3, 0xf3, 0xf3);
    LIST_LINE = rgb(0x4a, 0x4a, 0x4a), rgb(0xe5, 0xe5, 0xe5);
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// What Download Now / Later adds.
pub struct TorrentAdd {
    pub meta: Meta,
    /// The folder chosen; a torrent of several files gets its own folder inside.
    pub dir: PathBuf,
    /// Chosen files (indexes into `meta.files`); None: all.
    pub files: Option<Vec<usize>>,
    pub category: Category,
    pub start_now: bool,
    pub from_magnet: bool,
}

pub struct TorrentDialog {
    source: String,
    folder: String,
    folder_edited: bool,
    downloads: PathBuf,
    category_dirs: Vec<(String, String)>,
    /// The torrents the list already has (and the session must keep).
    known: HashSet<String>,
    host: Host,
    lookup: Option<Slot>,
    /// The look-up running (Stop ends it).
    lookup_task: Option<tokio::task::JoinHandle<()>>,
    /// When the lookup started (the loading state counts the seconds).
    started: Option<Instant>,
    meta: Option<Meta>,
    chosen: Vec<bool>,
    /// The file tree's scroll and selection.
    rows: winui::RowList,
    /// The file tree (built when the list arrives) and which folders are open.
    tree: Vec<Node>,
    open: Vec<bool>,
    error: Option<String>,
    from_magnet: bool,
    focused: bool,
    start_now: Option<bool>,
    /// Asking to create a missing folder.
    asking: Option<(crate::dialogs::CreateFolder, ())>,
}

/// The category of a torrent: its largest file's.
/// Every torrent goes to the Torrents category and its folder (D-021), whatever it holds.
fn category_of(_meta: &Meta) -> Category {
    Category::Torrents
}

/// A turning ring: the loading sign while something is fetched (also the video dialog's).
pub(crate) fn spinner(ui: &Ui, center: Pos2, radius: f32) {
    let t = ui.input(|i| i.time) as f32;
    let start = t * 5.0;
    let n = 24;
    let points: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a = start + k as f32 / n as f32 * std::f32::consts::PI * 1.5;
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    let p = ui.painter();
    p.circle_stroke(center, radius, Stroke::new(2.0, LIST_LINE()));
    p.line(points, Stroke::new(2.0, winui::SELECTION));
    ui.ctx().request_repaint();
}

impl TorrentDialog {
    /// `source`: a magnet link or a .torrent path to look up at once, or None (the clipboard's
    /// magnet, if any, is offered and looked up).
    pub fn new(
        source: Option<String>,
        host: Host,
        known: HashSet<String>,
        downloads: PathBuf,
        category_dirs: Vec<(String, String)>,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) -> Self {
        let source = source.unwrap_or_else(|| {
            arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
                .filter(|t| Source::magnet(t).is_some())
                .map(|t| t.trim().to_owned())
                .unwrap_or_default()
        });
        let folder = category_folder(Category::General, &downloads, &category_dirs);
        let mut d = TorrentDialog {
            source,
            folder: folder.display().to_string(),
            folder_edited: false,
            downloads,
            category_dirs,
            known,
            host,
            lookup: None,
            lookup_task: None,
            started: None,
            meta: None,
            chosen: Vec::new(),
            rows: Default::default(),
            tree: Vec::new(),
            open: Vec::new(),
            error: None,
            from_magnet: false,
            focused: false,
            start_now: None,
            asking: None,
        };
        if !d.source.is_empty() {
            d.look_up(rt, ctx);
        }
        d
    }

    fn look_up(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context) {
        let text = self.source.trim().to_owned();
        let magnet = Source::magnet(&text);
        self.from_magnet = magnet.is_some();
        // A .torrent file is read with the rest of the look-up, off the window's thread (a
        // path on a slow share froze every window).
        let path = text.trim_matches('"').to_owned();
        let not_found = crate::i18n::t("Enter a magnet link or choose a .torrent file").to_owned();
        let slot: Slot = Arc::new(Mutex::new(None));
        let (out, host, known, ctx) = (
            slot.clone(),
            self.host.clone(),
            self.known.clone(),
            ctx.clone(),
        );
        let task = rt.spawn(async move {
            let src = match magnet {
                Some(m) => m,
                None => match tokio::fs::read(&path).await {
                    Ok(bytes) => Source::File(bytes),
                    Err(_) => {
                        *out.lock().unwrap() = Some(Err(not_found));
                        ctx.request_repaint();
                        return;
                    }
                },
            };
            let res = match host.session(known).await {
                Ok(t) => t
                    .metadata(&src, METADATA_WAIT)
                    .await
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            *out.lock().unwrap() = Some(res);
            ctx.request_repaint();
        });
        self.lookup = Some(slot);
        self.lookup_task = Some(task);
        self.started = Some(Instant::now());
        self.meta = None;
        self.rows = Default::default();
        self.error = None;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        rt: &tokio::runtime::Handle,
    ) -> Outcome<TorrentAdd> {
        let looked = self.lookup.as_ref().and_then(|s| s.lock().unwrap().take());
        if let Some(res) = looked {
            self.lookup = None;
            self.started = None;
            match res {
                Ok(meta) if self.known.contains(&meta.info_hash) => {
                    self.error =
                        Some(crate::i18n::t("This torrent is already in the download list").into());
                }
                Ok(meta) => {
                    self.chosen = meta.files.iter().map(|f| !f.padding).collect();
                    self.tree = build_tree(&meta);
                    // Open the first two levels; deeper folders start closed.
                    self.open = self.tree.iter().map(|n| n.depth < 2).collect();
                    if !self.folder_edited {
                        let dir = category_folder(
                            category_of(&meta),
                            &self.downloads,
                            &self.category_dirs,
                        );
                        self.folder = dir.display().to_string();
                    }
                    self.meta = Some(meta);
                }
                Err(e) => self.error = Some(e),
            }
        }
        let out = window(
            ctx,
            "add-torrent",
            "Add torrent",
            [600.0, 460.0],
            false,
            winui::BG(),
            |ui, o| self.body(ui, o, rt),
        );
        let folder = PathBuf::from(self.folder.trim());
        let out = crate::dialogs::ensure_folder(ctx, &mut self.asking, out, &folder);
        match out {
            Outcome::Open => Outcome::Open,
            Outcome::Closed => Outcome::Closed,
            Outcome::Done(()) => {
                let Some(meta) = self.meta.take() else {
                    return Outcome::Closed;
                };
                let all = self
                    .chosen
                    .iter()
                    .zip(&meta.files)
                    .all(|(c, f)| *c || f.padding);
                let files = (!all).then(|| {
                    self.chosen
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| **c)
                        .map(|(i, _)| i)
                        .collect()
                });
                Outcome::Done(TorrentAdd {
                    category: category_of(&meta),
                    meta,
                    dir: PathBuf::from(self.folder.trim()),
                    files,
                    start_now: self.start_now.unwrap_or(true),
                    from_magnet: self.from_magnet,
                })
            }
        }
    }

    fn body(&mut self, ui: &mut Ui, o: Pos2, rt: &tokio::runtime::Handle) -> Outcome<()> {
        let p = ui.painter().clone();
        let busy = self.lookup.is_some();
        let kind = |on: bool, default: bool| match (on, default) {
            (false, _) => ButtonKind::Disabled,
            (true, true) => ButtonKind::Default,
            (true, false) => ButtonKind::Normal,
        };

        // The source: a magnet link, or a .torrent file chosen with "...".
        // i18n: noun: label before the torrent file or magnet link
        text(
            &p,
            o.x + 11.0,
            o.y + 23.0,
            crate::i18n::t("Torrent"),
            winui::TEXT(),
        );
        let edit = winui::edit(
            ui,
            r(o, 70.0, 9.0, 410.0, 21.0),
            &mut self.source,
            !busy,
            false,
        );
        if !self.focused {
            edit.request_focus();
            self.focused = true;
        }
        let browse = winui::button(
            ui,
            r(o, 486.0, 9.0, 22.0, 21.0),
            "...",
            kind(!busy, false),
            false,
            None,
        )
        .clicked();
        if browse
            && !busy
            && let Some(f) = rfd::FileDialog::new()
                // i18n: the kind of file the Open dialog lists
                .add_filter(crate::i18n::t("Torrent files"), &["torrent"])
                .set_title(crate::i18n::t("Open torrent file"))
                .pick_file()
        {
            self.source = f.display().to_string();
            self.look_up(rt, ui.ctx());
        }
        let can_look = !busy && !self.source.trim().is_empty();
        // While it looks up, the button stops it (a magnet with no peers took up to 3 minutes).
        let look = winui::button(
            ui,
            r(o, 514.0, 9.0, 75.0, 21.0),
            // i18n: button: stop looking the torrent up
            if busy {
                crate::i18n::mark("Stop")
            } else {
                crate::i18n::mark("Look up")
            },
            kind(can_look || busy, self.meta.is_none()),
            false,
            None,
        )
        .clicked()
            || (edit.has_focus()
                && can_look
                && winui::enter_pressed(ui)
                && winui::take_key(ui, egui::Key::Enter));
        if look && busy {
            if let Some(t) = self.lookup_task.take() {
                t.abort();
            }
            self.lookup = None;
            self.started = None;
        } else if look && can_look {
            self.look_up(rt, ui.ctx());
        }

        // Status: looking up (with a spinner and the seconds) / name and size / what went wrong.
        let line = o.y + 50.0;
        if busy {
            spinner(ui, pos2(o.x + 18.0, line - 4.0), 6.0);
            let secs = self.started.map_or(0, |s| s.elapsed().as_secs());
            let msg = if self.from_magnet {
                crate::i18n::fmt(
                    // i18n: still going on; {n} = the seconds it has taken so far ("s" = seconds)
                    crate::i18n::t("Getting the file list from peers...  {n} s"),
                    &[("n", &crate::i18n::integer(secs))],
                )
            } else {
                crate::i18n::t("Reading the torrent...").to_owned()
            };
            text(&p, o.x + 31.0, line, &msg, winui::TEXT());
        } else if let Some(e) = &self.error {
            let (rect, cut) = winui::text_fit(&p, o.x + 11.0, line, e, winui::ERROR(), o.x + 589.0);
            if cut {
                winui::full_on_hover(ui, rect, "torrent-error", e);
            }
        } else if let Some(m) = &self.meta {
            let shown = m.files.iter().filter(|f| !f.padding).count();
            let summary = crate::i18n::fmt(
                // i18n: the torrent's name, then {files} = "3 files" and {size} = its total size
                crate::i18n::t("{name}  ({files}, {size})"),
                &[
                    ("name", &m.name),
                    (
                        "files",
                        &crate::i18n::tn(shown as u64, "{n} file", "{n} files"),
                    ),
                    ("size", &crate::dialogs::dialog_size(m.total())),
                ],
            );
            let (rect, cut) =
                winui::text_fit(&p, o.x + 11.0, line, &summary, winui::TEXT(), o.x + 589.0);
            if cut {
                winui::full_on_hover(ui, rect, "torrent-name", &summary);
            }
        }

        self.file_list(ui, r(o, 11.0, 58.0, 578.0, 262.0));

        // Select all / none, and what is chosen.
        let has = self.meta.is_some();
        if winui::button(
            ui,
            r(o, 11.0, 328.0, 90.0, 21.0),
            "Select all",
            kind(has, false),
            false,
            None,
        )
        .clicked()
            && let Some(m) = &self.meta
        {
            self.chosen = m.files.iter().map(|f| !f.padding).collect();
        }
        if winui::button(
            ui,
            r(o, 107.0, 328.0, 90.0, 21.0),
            "Select none",
            kind(has, false),
            false,
            None,
        )
        .clicked()
        {
            self.chosen.iter_mut().for_each(|c| *c = false);
        }
        let (count, bytes) = self.selection();
        if has {
            winui::text_right(
                &p,
                o.x + 589.0,
                o.y + 343.0,
                &crate::i18n::fmt(
                    // i18n: the files chosen: {files} = "3 files", {size} = their total size
                    crate::i18n::t("Selected: {files}, {size}"),
                    &[
                        (
                            "files",
                            &crate::i18n::tn(count as u64, "{n} file", "{n} files"),
                        ),
                        ("size", &crate::dialogs::dialog_size(bytes)),
                    ],
                ),
                winui::TEXT(),
            );
        }

        // Where.
        text(
            &p,
            o.x + 11.0,
            o.y + 375.0,
            crate::i18n::t("Save to"),
            winui::TEXT(),
        );
        if winui::edit(
            ui,
            r(o, 70.0, 361.0, 491.0, 21.0),
            &mut self.folder,
            true,
            false,
        )
        .changed()
        {
            self.folder_edited = true;
        }
        if winui::button(
            ui,
            r(o, 567.0, 361.0, 22.0, 21.0),
            "...",
            ButtonKind::Normal,
            false,
            None,
        )
        .clicked()
            && let Some(d) = rfd::FileDialog::new()
                .set_directory(&self.folder)
                .pick_folder()
        {
            self.folder = d.display().to_string();
            self.folder_edited = true;
        }
        if let Some(m) = &self.meta {
            let goes = m.folder_in(&PathBuf::from(self.folder.trim()));
            let s = crate::i18n::fmt(
                // i18n: {folder} = the folder the torrent's files are saved in
                crate::i18n::t("Files go to: {folder}"),
                &[("folder", &crate::i18n::ltr(&goes.display().to_string()))],
            );
            let (rect, cut) =
                winui::text_fit(&p, o.x + 70.0, o.y + 402.0, &s, winui::MUTED(), o.x + 589.0);
            if cut {
                winui::full_on_hover(ui, rect, "torrent-goes", &s);
            }
        }

        let ready = has && count > 0 && !self.folder.trim().is_empty() && !busy;
        let now = winui::button(
            ui,
            r(o, 290.0, 428.0, 105.0, 21.0),
            "Download Now",
            kind(ready, true),
            false,
            None,
        )
        .clicked()
            || (ready && winui::enter_pressed(ui));
        let later = winui::button(
            ui,
            r(o, 401.0, 428.0, 91.0, 21.0),
            // i18n: button: add it now, start downloading later
            "Later",
            kind(ready, false),
            false,
            None,
        )
        .clicked();
        let cancel = winui::button(
            ui,
            r(o, 498.0, 428.0, 91.0, 21.0),
            "Cancel",
            ButtonKind::Normal,
            false,
            None,
        )
        .clicked();
        if cancel || winui::escape_pressed(ui) {
            return Outcome::Closed;
        }
        if ready && (now || later) {
            self.start_now = Some(now);
            return Outcome::Done(());
        }
        Outcome::Open
    }

    fn selection(&self) -> (usize, u64) {
        let Some(m) = &self.meta else { return (0, 0) };
        m.files
            .iter()
            .zip(&self.chosen)
            .filter(|(f, c)| **c && !f.padding)
            .fold((0, 0), |(n, b), (f, _)| (n + 1, b + f.size))
    }

    /// The file tree: folders open and close, every level has a checkbox (a folder's is mixed
    /// when only some files inside are chosen; ticking it chooses or clears them all); sizes
    /// on the right; scrolled by the wheel a row at a time. While looking up: a spinner.
    fn file_list(&mut self, ui: &mut Ui, list: Rect) {
        let p = ui.painter().clone();
        winui::solid(&p, list, rgb(0x82, 0x87, 0x90));
        // Laid out in English (`list` comes on screen); each part made where it is on screen:
        // names from the leading edge, sizes at the trailing one, folders indenting from the
        // leading edge.
        let list = winui::rect(list);
        let inner = list.shrink(1.0);
        p.rect_filled(winui::rect(inner), 0.0, winui::FIELD());
        let head = Rect::from_min_size(inner.min, vec2(inner.width(), 20.0));
        p.rect_filled(winui::rect(head), 0.0, LIST_HEAD());
        fill(
            &p,
            head.left(),
            head.bottom() - 1.0,
            head.right() - 1.0,
            head.bottom() - 1.0,
            LIST_LINE(),
        );
        text(
            &p,
            head.left() + 25.0,
            head.top() + 14.0,
            // i18n: column title: file name
            crate::i18n::t("Name"),
            winui::TEXT(),
        );
        winui::text_right(
            &p,
            head.right() - 24.0,
            head.top() + 14.0,
            // i18n: column title: file size
            crate::i18n::t("Size"),
            winui::TEXT(),
        );
        let body = Rect::from_min_max(pos2(inner.left(), head.bottom()), inner.max);

        if self.lookup.is_some() {
            let c = body.center();
            spinner(ui, pos2(c.x, c.y - 14.0), 12.0);
            let note = if self.from_magnet {
                crate::i18n::t(
                    "Asking peers for the file list. Magnet links can take a minute or two.",
                )
            } else {
                crate::i18n::t("Reading the file list...")
            };
            let w = winui::text_width(&p, note);
            text(&p, c.x - w / 2.0, c.y + 22.0, note, winui::MUTED());
            return;
        }
        if self.meta.is_none() {
            // i18n: "Look up" is the button that reads the torrent: use its translation
            let note =
                crate::i18n::t("Paste a magnet link or choose a .torrent file, then Look up.");
            let w = winui::text_width(&p, note);
            text(
                &p,
                body.center().x - w / 2.0,
                body.center().y + 4.0,
                note,
                winui::MUTED(),
            );
            return;
        }
        let rows = visible_rows(&self.tree, &self.open);
        // The shared row list over the body (its frame on the list's): a click selects, the
        // box, Space or a double-click on a file ticks, a double-click or the arrow opens a
        // folder, Right / Left open and close the selected one.
        let (tree, open, chosen) = (&self.tree, &self.open, &self.chosen);
        let mut toggle = None;
        let mut flip_open = None;
        let ev = winui::row_list(
            ui,
            "torrent-tree",
            winui::rect(body.expand(1.0)),
            rows.len(),
            ROW,
            &mut self.rows,
            |ui, clip, k, row, look| {
                let n = rows[k];
                let node = &tree[n];
                let top = row.top();
                let x = row.left() + 6.0 + 16.0 * node.depth as f32;
                let is_dir = node.file.is_none();
                if is_dir {
                    let arrow = Rect::from_min_size(pos2(x, top), vec2(14.0, ROW));
                    let ar = ui.interact(
                        winui::rect(arrow),
                        egui::Id::unique(("torrent-arrow", n)),
                        winui::pointer_only(),
                    );
                    if ar.clicked() {
                        flip_open = Some(n);
                    }
                    expander(clip, pos2(x + 4.0, top + 5.0), open[n]);
                }
                let state = tri_state(node, chosen);
                let glyph = pos2(x + 16.0, top + 2.0);
                let mut on = state == Tri::All;
                if winui::row_check(ui, clip, glyph, &mut on, ("torrent-node", n)) {
                    toggle = Some(n);
                }
                if state == Tri::Some {
                    mixed_mark(clip, glyph);
                }
                let size = crate::dialogs::dialog_size(node.size);
                let size_w = winui::text_width(clip, &size);
                let left = glyph.x + 19.0;
                // A name keeps its own direction (cut inside its isolate), whole on hover.
                let name = crate::i18n::isolate(&node.name);
                let max = row.right() - 8.0 - size_w - 12.0;
                winui::fit(
                    ui,
                    clip,
                    left,
                    top + 12.0,
                    &name,
                    look.ink,
                    max,
                    ("torrent-name", n),
                );
                let size_color = if is_dir && !look.selected {
                    winui::MUTED()
                } else {
                    look.ink
                };
                winui::text_right(clip, row.right() - 8.0, top + 12.0, &size, size_color);
            },
        );
        let is_dir = |k: usize| self.tree[rows[k]].file.is_none();
        if let Some(k) = ev.double_clicked {
            if is_dir(k) {
                flip_open = Some(rows[k]);
            } else {
                toggle = Some(rows[k]);
            }
        }
        if let Some(k) = ev.space {
            toggle = Some(rows[k]);
        }
        if ev.focused {
            let tree = &self.tree;
            self.rows.type_to(ui, "torrent-tree", rows.len(), |k| {
                tree[rows[k]].name.clone()
            });
        }
        // Right opens the selected folder, Left closes it.
        if ev.focused
            && let Some(k) = self.rows.selected.filter(|k| *k < rows.len() && is_dir(*k))
        {
            let n = rows[k];
            if (winui::take_key(ui, egui::Key::ArrowRight) && !self.open[n])
                || (winui::take_key(ui, egui::Key::ArrowLeft) && self.open[n])
            {
                flip_open = Some(n);
            }
        }
        if let Some(n) = flip_open {
            self.open[n] = !self.open[n];
        }
        if let Some(n) = toggle {
            // A folder (or file): all of it on, unless it already is all on.
            let on = tri_state(&self.tree[n], &self.chosen) != Tri::All;
            for &f in &self.tree[n].files {
                self.chosen[f] = on;
            }
        }
    }
}

/// Torrent settings (ours, D-018): sharing after a torrent finishes, and whether Windows opens
/// magnet links and .torrent files with MDM. Both off by default.
pub struct TorrentSettings {
    share: bool,
    assoc: bool,
}

impl TorrentSettings {
    pub fn new(s: &crate::settings::Settings) -> Self {
        TorrentSettings {
            share: s.torrent_keep_sharing,
            assoc: s.torrent_associations,
        }
    }

    /// OK gives (keep sharing, associations).
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<(bool, bool)> {
        window(
            ctx,
            "torrent-settings",
            "Torrent settings",
            [440.0, 150.0],
            false,
            winui::BG(),
            |ui, o| {
                let p = ui.painter().clone();
                winui::checkbox(
                    ui,
                    pos2(o.x + 14.0, o.y + 16.0),
                    &mut self.share,
                    "Keep sharing torrents after they finish",
                    true,
                );
                text(
                    &p,
                    o.x + 32.0,
                    o.y + 44.0,
                    crate::i18n::t("Off: a finished torrent stops, like any download."),
                    winui::MUTED(),
                );
                winui::checkbox(
                    ui,
                    pos2(o.x + 14.0, o.y + 60.0),
                    &mut self.assoc,
                    "Open magnet links and .torrent files with Mister Download Manager",
                    true,
                );
                text(
                    &p,
                    o.x + 32.0,
                    o.y + 88.0,
                    // What the switch does is the system's: Windows asks per account, macOS and
                    // Linux register MDM as the handler.
                    if cfg!(windows) {
                        crate::i18n::t(
                            "For your Windows account only. Another app chosen in \"Open with\" stays first.",
                        )
                    } else {
                        // i18n: Torrent settings, under the switch that makes MDM open magnet links and .torrent files (macOS, Linux)
                        crate::i18n::t(
                            "For your account only. Another app set as the default stays first.",
                        )
                    },
                    winui::MUTED(),
                );
                let ok = winui::button(
                    ui,
                    r(o, 268.0, 118.0, 77.0, 21.0),
                    "OK",
                    ButtonKind::Default,
                    false,
                    None,
                )
                .clicked();
                let cancel = winui::button(
                    ui,
                    r(o, 351.0, 118.0, 77.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                if cancel || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if ok || winui::enter_pressed(ui) {
                    Outcome::Done((self.share, self.assoc))
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

/// A row of the file tree: a folder or a file.
struct Node {
    name: String,
    depth: usize,
    /// The file's index in `Meta::files`; None for a folder.
    file: Option<usize>,
    children: Vec<usize>,
    /// Every file at or under this node (padding files left out).
    files: Vec<usize>,
    size: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tri {
    None,
    Some,
    All,
}

fn tri_state(node: &Node, chosen: &[bool]) -> Tri {
    let on = node.files.iter().filter(|f| chosen[**f]).count();
    match on {
        0 => Tri::None,
        n if n == node.files.len() => Tri::All,
        _ => Tri::Some,
    }
}

/// The tree of a torrent's files: folders first, then files, each by name. A torrent of several
/// files has its own folder (named after it) at the top, as on disk (`Meta::folder_in`).
fn build_tree(meta: &Meta) -> Vec<Node> {
    let multi = meta.files.iter().filter(|f| !f.padding).count() > 1;
    let mut nodes = vec![Node {
        name: meta.name.clone(),
        depth: 0,
        file: None,
        children: Vec::new(),
        files: Vec::new(),
        size: 0,
    }];
    for (i, f) in meta.files.iter().enumerate().filter(|(_, f)| !f.padding) {
        let parts: Vec<String> = f
            .path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let mut at = 0;
        nodes[0].files.push(i);
        nodes[0].size += f.size;
        for (k, part) in parts.iter().enumerate() {
            let last = k + 1 == parts.len();
            let found = nodes[at]
                .children
                .iter()
                .copied()
                .find(|c| !last && nodes[*c].file.is_none() && nodes[*c].name == *part);
            let next = match found {
                Some(c) => c,
                None => {
                    nodes.push(Node {
                        name: part.clone(),
                        depth: nodes[at].depth + 1,
                        file: last.then_some(i),
                        children: Vec::new(),
                        files: Vec::new(),
                        size: 0,
                    });
                    let id = nodes.len() - 1;
                    nodes[at].children.push(id);
                    id
                }
            };
            nodes[next].files.push(i);
            nodes[next].size += f.size;
            at = next;
        }
    }
    // Folders first, then by name (case-insensitive), at every level.
    for n in 0..nodes.len() {
        let mut kids = std::mem::take(&mut nodes[n].children);
        kids.sort_by(|a, b| {
            let (x, y) = (&nodes[*a], &nodes[*b]);
            x.file
                .is_some()
                .cmp(&y.file.is_some())
                .then_with(|| x.name.to_lowercase().cmp(&y.name.to_lowercase()))
        });
        nodes[n].children = kids;
    }
    if !multi {
        // One file: just its row, no folder above it.
        let only = nodes[0].children.first().copied();
        if let Some(c) = only {
            nodes[c].depth = 0;
            nodes[0].children.clear();
            nodes.swap(0, c);
            // The old root, now at `c`, is unreachable.
            nodes[c].files.clear();
        }
    }
    nodes
}

/// The rows shown: the root, and the children of every open folder, depth first.
fn visible_rows(tree: &[Node], open: &[bool]) -> Vec<usize> {
    let mut out = Vec::new();
    if tree.is_empty() {
        return out;
    }
    let mut stack = vec![0usize];
    while let Some(n) = stack.pop() {
        out.push(n);
        if open[n] {
            stack.extend(tree[n].children.iter().rev().copied());
        }
    }
    out
}

/// A tree expander at English point `at`: ▸ when closed (pointing into the row: ◂ in RTL), ▾
/// when open.
fn expander(p: &egui::Painter, at: Pos2, open: bool) {
    let c = winui::MUTED();
    let pts = if open {
        vec![at, pos2(at.x + 8.0, at.y), pos2(at.x + 4.0, at.y + 5.0)]
    } else {
        vec![
            pos2(at.x + 2.0, at.y - 1.0),
            pos2(at.x + 7.0, at.y + 3.5),
            pos2(at.x + 2.0, at.y + 8.0),
        ]
    };
    let pts = pts.into_iter().map(|q| pos2(winui::mx(q.x), q.y)).collect();
    p.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
}

/// The "some chosen" mark over an unticked checkbox glyph.
fn mixed_mark(p: &egui::Painter, glyph: Pos2) {
    // The accent in both modes (black on the dark unticked box could not be seen).
    let c = winui::accent();
    // `glyph` is English, as the check box's.
    p.rect_filled(
        winui::rect(Rect::from_min_size(
            pos2(glyph.x + 3.0, glyph.y + 3.0),
            vec2(7.0, 7.0),
        )),
        1.0,
        c,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_torrent::FileEntry;

    fn meta(paths: &[(&str, u64)]) -> Meta {
        Meta {
            name: "Set".into(),
            info_hash: "0".repeat(40),
            files: paths
                .iter()
                .map(|(p, s)| FileEntry {
                    path: PathBuf::from(p),
                    size: *s,
                    padding: false,
                })
                .collect(),
            piece_length: 16384,
            torrent: Vec::new(),
            peers: Vec::new(),
        }
    }

    #[test]
    fn folders_first_with_sizes_and_mixed_state() {
        let m = meta(&[
            ("readme.txt", 1),
            ("disc 2/b.iso", 20),
            ("disc 1/a.iso", 10),
            ("disc 1/extras/c.txt", 3),
        ]);
        let t = build_tree(&m);
        let open = vec![true; t.len()];
        let names: Vec<(usize, &str)> = visible_rows(&t, &open)
            .into_iter()
            .map(|n| (t[n].depth, t[n].name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                (0, "Set"),
                (1, "disc 1"),
                (2, "extras"),
                (3, "c.txt"),
                (2, "a.iso"),
                (1, "disc 2"),
                (2, "b.iso"),
                (1, "readme.txt"),
            ]
        );
        assert_eq!(t[0].size, 34);
        let disc1 = t.iter().position(|n| n.name == "disc 1").unwrap();
        assert_eq!(t[disc1].size, 13);
        let mut chosen = vec![true; 4];
        assert_eq!(tri_state(&t[0], &chosen), Tri::All);
        chosen[3] = false; // c.txt
        assert_eq!(tri_state(&t[disc1], &chosen), Tri::Some);
        assert_eq!(tri_state(&t[0], &chosen), Tri::Some);
        // A closed folder hides what is inside.
        let mut open = open;
        open[disc1] = false;
        assert_eq!(visible_rows(&t, &open).len(), 5);
    }

    #[test]
    fn one_file_is_one_row() {
        let t = build_tree(&meta(&[("debian.iso", 5)]));
        let open = vec![true; t.len()];
        let rows = visible_rows(&t, &open);
        assert_eq!(rows.len(), 1);
        assert_eq!(t[rows[0]].name, "debian.iso");
        assert_eq!(t[rows[0]].files, [0]);
    }
}

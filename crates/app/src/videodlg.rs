//! Tasks → Download video from a page… (M9, ours: IDM's video panel is fed by its browser
//! module, which comes with M8). Paste a page, look it up with yt-dlp, pick a quality, choose
//! where it goes. yt-dlp is installed (verified, D-011) from here on first use, and ffmpeg (the
//! LGPL build) when the best qualities need merging. It follows the app's mode, like Add torrent.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{Color32, Pos2, Rect, Sense, Ui, pos2};
use mdm_tools::resolve::{Choice, Video};

use crate::dialogs::{Outcome, window};
use crate::winui::{self, ButtonKind, r, text};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

crate::theme::themed! {}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// What OK adds.
pub struct VideoAdd {
    pub page: url::Url,
    pub selector: String,
    pub dest: PathBuf,
    pub size: Option<u64>,
    pub start_now: bool,
}

/// Which helper is being installed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    YtDlp,
    Ffmpeg,
}

pub struct VideoDialog {
    page: String,
    folder: String,
    name: String,
    lookup: Option<Slot<Video>>,
    install: Option<(Tool, Slot<mdm_tools::Installed>)>,
    video: Option<(Video, Vec<Choice>)>,
    chosen: usize,
    /// First choice shown (the list scrolls by whole rows).
    first: usize,
    error: Option<String>,
    focused: bool,
    /// Download Now (true) or Later, once chosen.
    start_now: Option<bool>,
    /// Look the page up as soon as yt-dlp is there (opened from Add URL).
    auto_look: bool,
    /// Asking to create a missing folder.
    asking: Option<(crate::dialogs::CreateFolder, ())>,
}

/// What is installed in the tools folder: (folder, yt-dlp, ffmpeg).
type Tools = (
    Option<PathBuf>,
    Option<mdm_tools::Installed>,
    Option<mdm_tools::Installed>,
);

/// Keep yt-dlp current: video sites change often and an old yt-dlp is the usual reason a page
/// stops working. Once per session, when the video window opens and yt-dlp is installed, the
/// newest release is fetched in the background (mdm_tools::install_latest: signed by yt-dlp's
/// developers, never older, switched atomically, the previous version kept). Quiet: a failure
/// leaves the installed version in use.
pub fn refresh_ytdlp(rt: &tokio::runtime::Handle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    let Some(dir) = mdm_tools::tools_dir() else {
        return;
    };
    if mdm_tools::installed(&dir).is_none() || DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    rt.spawn(async move {
        if let Err(e) = mdm_tools::install_latest(&dir).await {
            eprintln!("yt-dlp update check: {e}"); // not translated: a log line
        }
    });
}

fn tools() -> Tools {
    let dir = mdm_tools::tools_dir();
    let ytdlp = dir.as_deref().and_then(mdm_tools::installed);
    let ffmpeg = dir.as_deref().and_then(mdm_tools::ffmpeg::installed);
    (dir, ytdlp, ffmpeg)
}

impl VideoDialog {
    pub fn new(folder: PathBuf) -> Self {
        let page = arboard::Clipboard::new()
            .ok()
            .and_then(|mut c| c.get_text().ok())
            .map(|t| t.trim().to_owned())
            .filter(|t| url::Url::parse(t).is_ok_and(|u| matches!(u.scheme(), "http" | "https")))
            .unwrap_or_default();
        VideoDialog {
            page,
            folder: folder.display().to_string(),
            name: String::new(),
            lookup: None,
            install: None,
            video: None,
            chosen: 0,
            first: 0,
            error: None,
            focused: false,
            start_now: None,
            auto_look: false,
            asking: None,
        }
    }

    /// Opened for `page` (Add URL was given a video's page): looks it up at once.
    pub fn for_page(folder: PathBuf, page: &str) -> Self {
        VideoDialog {
            page: page.to_owned(),
            auto_look: true,
            ..Self::new(folder)
        }
    }

    /// The choices for a looked-up video: merged qualities when ffmpeg is there.
    fn set_video(&mut self, v: Video) {
        let merge = tools().2.is_some();
        let choices = mdm_tools::resolve::choices(&v, merge);
        self.chosen = 0;
        self.first = 0;
        self.error = choices
            .is_empty()
            .then(|| crate::i18n::t("No video this can download on that page").into());
        self.video = Some((v, choices));
        self.name_from_choice();
    }

    pub fn show(&mut self, ctx: &egui::Context, rt: &tokio::runtime::Handle) -> Outcome<VideoAdd> {
        // Results from the background work.
        let looked = self.lookup.as_ref().and_then(|s| s.lock().unwrap().take());
        if let Some(res) = looked {
            self.lookup = None;
            match res {
                Ok(v) => self.set_video(v),
                Err(e) => self.error = Some(e),
            }
        }
        let installed = self
            .install
            .as_ref()
            .and_then(|(t, s)| s.lock().unwrap().take().map(|r| (*t, r)));
        if let Some((tool, res)) = installed {
            self.install = None;
            match res {
                Err(e) => {
                    let what = if tool == Tool::YtDlp {
                        "yt-dlp"
                    } else {
                        "ffmpeg"
                    };
                    self.error = Some(crate::i18n::fmt(
                        // i18n: an error: {what} = a helper program (yt-dlp, ffmpeg), {error} = why
                        crate::i18n::t("{what} could not be installed: {error}"),
                        &[("what", what), ("error", &e)],
                    ));
                }
                // With ffmpeg the qualities that need merging appear.
                Ok(_) if tool == Tool::Ffmpeg => {
                    if let Some((v, _)) = self.video.take() {
                        self.set_video(v);
                    }
                }
                Ok(_) => {}
            }
        }
        let out = window(
            ctx,
            "video-page",
            "Download video from a page",
            [500.0, 360.0],
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
                let (Some((_, choices)), Ok(page)) =
                    (&self.video, url::Url::parse(self.page.trim()))
                else {
                    return Outcome::Closed;
                };
                let c = &choices[self.chosen.min(choices.len() - 1)];
                let name = mdm_engine::naming::sanitize(self.name.trim());
                Outcome::Done(VideoAdd {
                    page,
                    selector: c.selector.clone(),
                    dest: PathBuf::from(self.folder.trim()).join(name),
                    size: c.size,
                    start_now: self.start_now.unwrap_or(true),
                })
            }
        }
    }

    fn name_from_choice(&mut self) {
        if let Some((v, choices)) = &self.video
            && let Some(c) = choices.get(self.chosen)
        {
            let stem = mdm_engine::naming::sanitize(&v.title);
            self.name = format!("{stem}.{}", c.ext);
        }
    }

    /// Install a helper in the background.
    fn install(
        &mut self,
        tool: Tool,
        dir: PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) {
        let slot: Slot<mdm_tools::Installed> = Arc::new(Mutex::new(None));
        let (out, ctx) = (slot.clone(), ctx.clone());
        rt.spawn(async move {
            let res = match tool {
                Tool::YtDlp => mdm_tools::install_latest(&dir).await,
                Tool::Ffmpeg => mdm_tools::ffmpeg::install_latest(&dir).await,
            };
            *out.lock().unwrap() = Some(res.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.install = Some((tool, slot));
        self.error = None;
    }

    fn body(&mut self, ui: &mut Ui, o: Pos2, rt: &tokio::runtime::Handle) -> Outcome<()> {
        let p = ui.painter().clone();
        let (dir, ytdlp, ffmpeg) = tools();
        text(
            &p,
            o.x + 11.0,
            o.y + 21.0,
            crate::i18n::t("Page address"),
            winui::TEXT(),
        );
        let edit = winui::edit(
            ui,
            r(o, 11.0, 28.0, 380.0, 21.0),
            &mut self.page,
            true,
            false,
        );
        if !self.focused {
            edit.request_focus();
            self.focused = true;
        }
        let busy = self.lookup.is_some() || self.install.is_some();
        let page_ok =
            url::Url::parse(self.page.trim()).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
        let can_look = page_ok && !busy && ytdlp.is_some();
        let look = winui::button(
            ui,
            r(o, 399.0, 28.0, 90.0, 21.0),
            "Look up",
            if can_look {
                ButtonKind::Default
            } else {
                ButtonKind::Disabled
            },
            false,
            None,
        )
        .clicked()
            || (edit.lost_focus() && winui::enter_pressed(ui) && can_look);
        // Opened for a page: look it up as soon as yt-dlp is there.
        let look = look || (can_look && std::mem::take(&mut self.auto_look));
        if look
            && can_look
            && let Some(tool) = &ytdlp
        {
            let slot: Slot<Video> = Arc::new(Mutex::new(None));
            let (out, exe, page, ctx) = (
                slot.clone(),
                tool.exe.clone(),
                self.page.trim().to_owned(),
                ui.ctx().clone(),
            );
            std::thread::spawn(move || {
                let res = mdm_tools::resolve::resolve(&exe, &page).map_err(|e| e.to_string());
                *out.lock().unwrap() = Some(res);
                ctx.request_repaint();
            });
            self.lookup = Some(slot);
            self.video = None;
            self.error = None;
        }

        // The status line: a helper missing or installing / looking up / the title.
        let line_y = o.y + 70.0;
        let installing = self.install.as_ref().map(|(t, _)| *t);
        let offer = match (&ytdlp, &ffmpeg) {
            (None, _) => Some((
                Tool::YtDlp,
                // i18n: button: download and install the yt-dlp program
                crate::i18n::mark("Install yt-dlp"),
                crate::i18n::t(
                    "Video lookups use yt-dlp (about 23 MB, checked against its signature).",
                ),
            )),
            (Some(_), None) => Some((
                Tool::Ffmpeg,
                // i18n: button: download and install the ffmpeg program
                crate::i18n::mark("Install ffmpeg"),
                crate::i18n::t(
                    "The best qualities need ffmpeg to merge video and audio (about 80 MB).",
                ),
            )),
            _ => None,
        };
        if let Some(t) = installing {
            crate::torrentdlg::spinner(ui, pos2(o.x + 18.0, line_y - 4.0), 6.0);
            let msg = if t == Tool::YtDlp {
                crate::i18n::t("Downloading and verifying yt-dlp...")
            } else {
                crate::i18n::t("Downloading and verifying ffmpeg (about 80 MB)...")
            };
            text(&p, o.x + 31.0, line_y, msg, winui::TEXT());
        } else if self.lookup.is_some() {
            crate::torrentdlg::spinner(ui, pos2(o.x + 18.0, line_y - 4.0), 6.0);
            text(
                &p,
                o.x + 31.0,
                line_y,
                crate::i18n::t("Looking the page up..."),
                winui::TEXT(),
            );
        } else if let Some((v, _)) = &self.video {
            let (rect, cut) =
                winui::text_fit(&p, o.x + 11.0, line_y, &v.title, winui::TEXT(), o.x + 489.0);
            if cut {
                winui::full_on_hover(ui, rect, "video-title", &v.title);
            }
        } else if let Some((_, _, msg)) = offer {
            winui::fit(
                ui,
                &p,
                o.x + 11.0,
                line_y,
                msg,
                winui::MUTED(),
                o.x + 390.0,
                "video-offer",
            );
        }
        if let Some((tool, label, _)) = offer
            && installing.is_none()
            && winui::button(
                ui,
                r(o, 399.0, 57.0, 90.0, 21.0),
                label,
                ButtonKind::Normal,
                false,
                None,
            )
            .clicked()
            && let Some(dir) = dir.clone()
        {
            self.install(tool, dir, rt, ui.ctx());
        }
        if let Some(e) = &self.error {
            let (rect, cut) =
                winui::text_fit(&p, o.x + 11.0, o.y + 86.0, e, winui::ERROR(), o.x + 489.0);
            if cut {
                winui::full_on_hover(ui, rect, "video-error", e);
            }
        }

        // The choices.
        let list = r(o, 11.0, 94.0, 478.0, 150.0);
        winui::solid(&p, list, rgb(0x82, 0x87, 0x90));
        p.rect_filled(list.shrink(1.0), 0.0, winui::FIELD());
        let mut picked = None;
        if let Some((_, choices)) = &self.video {
            // Whole rows only, scrolled by the wheel; a thin bar when they don't all fit.
            let page = ((list.height() - 4.0) / 17.0).floor() as usize;
            let max_first = choices.len().saturating_sub(page);
            if ui.rect_contains_pointer(list) {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                if dy < 0.0 {
                    self.first = (self.first + 1).min(max_first);
                } else if dy > 0.0 {
                    self.first = self.first.saturating_sub(1);
                }
            }
            self.first = self.first.min(max_first);
            let clip = p.with_clip_rect(list.shrink(1.0));
            // Rows laid out in English, made where they are on screen: the choice from the
            // leading edge, its size at the trailing one, the bar after them.
            let list = winui::rect(list);
            let right = list.right() - if max_first > 0 { 17.0 } else { 1.0 };
            for (k, (i, c)) in choices
                .iter()
                .enumerate()
                .skip(self.first)
                .take(page)
                .enumerate()
            {
                let top = list.top() + 2.0 + 17.0 * k as f32;
                let row = Rect::from_min_max(pos2(list.left() + 1.0, top), pos2(right, top + 17.0));
                if ui
                    .interact(
                        winui::rect(row),
                        egui::Id::unique(("video-choice", i)),
                        Sense::click(),
                    )
                    .clicked()
                {
                    picked = Some(i);
                }
                let sel = i == self.chosen;
                if sel {
                    clip.rect_filled(winui::rect(row), 0.0, winui::SELECTION);
                }
                let color = if sel { Color32::WHITE } else { winui::TEXT() };
                // A format ("1080p mp4, 60 fps") reads left to right.
                let label = crate::i18n::ltr(&c.label);
                text(&clip, row.left() + 6.0, top + 12.0, &label, color);
                let size = c
                    .size
                    // i18n: the video's file size is not known
                    .map_or(
                        crate::i18n::t("size unknown").into(),
                        crate::dialogs::dialog_size,
                    );
                winui::text_right(&clip, row.right() - 6.0, top + 12.0, &size, color);
            }
            if max_first > 0 {
                let (t, b) = (list.top() + 2.0, list.bottom() - 2.0);
                let len = ((b - t) * page as f32 / choices.len() as f32).max(12.0);
                let y0 = t + (b - t - len) * self.first as f32 / max_first as f32;
                let x = list.right() - 9.0;
                winui::fill_l(&p, x - 1.0, y0, x, y0 + len, winui::MUTED());
            }
        }
        if let Some(i) = picked {
            self.chosen = i;
            self.name_from_choice();
        }

        text(
            &p,
            o.x + 11.0,
            o.y + 266.0,
            crate::i18n::t("Save to"),
            winui::TEXT(),
        );
        winui::edit(
            ui,
            r(o, 70.0, 253.0, 389.0, 21.0),
            &mut self.folder,
            true,
            false,
        );
        if winui::button(
            ui,
            r(o, 467.0, 253.0, 22.0, 21.0),
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
        }
        text(
            &p,
            o.x + 11.0,
            o.y + 297.0,
            crate::i18n::t("File name"),
            winui::TEXT(),
        );
        winui::edit(
            ui,
            r(o, 70.0, 284.0, 419.0, 21.0),
            &mut self.name,
            true,
            false,
        );

        let ready = self.video.as_ref().is_some_and(|(_, c)| !c.is_empty())
            && !self.name.trim().is_empty()
            && !busy;
        let kind = |default: bool| match (ready, default) {
            (false, _) => ButtonKind::Disabled,
            (true, true) => ButtonKind::Default,
            (true, false) => ButtonKind::Normal,
        };
        let now = winui::button(
            ui,
            r(o, 222.0, 327.0, 105.0, 21.0),
            "Download Now",
            kind(true),
            false,
            None,
        )
        .clicked();
        let later = winui::button(
            ui,
            r(o, 335.0, 327.0, 73.0, 21.0),
            // i18n: button: add it now, start downloading later
            "Later",
            kind(false),
            false,
            None,
        )
        .clicked();
        let cancel = winui::button(
            ui,
            r(o, 416.0, 327.0, 73.0, 21.0),
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
}

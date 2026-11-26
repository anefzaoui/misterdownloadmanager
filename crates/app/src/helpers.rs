//! The first start's offer of the video helpers (user request 2026-11-01), a step of the setup
//! window since #1839 (setup.rs): yt-dlp finds a page's video, ffmpeg joins its picture and
//! sound, and, only where the computer has no JavaScript runtime, Node.js solves YouTube's
//! challenges (#1570; YairDies' idea, #1541). Each is explained, says whether it is there, and has
//! its own Install button and progress; nothing is downloaded until one is pressed (through MDM's
//! own engine, checked against its publisher's signed checksums: see mdm_tools). Next leaves them
//! to Tasks → Download video from a page, which offers them again.

use std::sync::Arc;
use std::sync::Mutex;

use egui::{Pos2, Ui};

use crate::dialogs::{Outcome, window};
use crate::icons::Icon;
use crate::winui::{self, ButtonKind, r};

/// The setup's page of the video helpers.
pub struct Offer {
    dir: Option<std::path::PathBuf>,
    /// Which helpers are there (yt-dlp, ffmpeg, Node.js): read when the page is made and again
    /// after each install here, not every frame (it reads files).
    have: Have,
    /// Node.js is offered too: the computer has no JavaScript runtime yt-dlp can use, and MDM
    /// has a Node.js for it. What can be told without running anything when the page is made
    /// (`jsruntime::current_now`); when a runtime on the computer must first be asked for its
    /// version, that is done off the window's thread ([`RuntimeProbe`]) and the row comes (or not)
    /// when it answers, unless an install already started.
    node: bool,
    /// The runtime lookup waiting for an answer.
    probe: RuntimeProbe,
    /// The installs started here, one per helper, each with its answer once it ends.
    installs: Vec<(Tool, Install)>,
    /// The last install that failed: the helper, why, and what to do about it (#816).
    failed: Option<(Tool, String, Option<String>)>,
    /// An install started here ended well.
    installed_here: bool,
}

/// An install's answer, once it ends: Err holds why, in words, and what to do about it.
type Install = Arc<Mutex<Option<Result<(), (String, Option<String>)>>>>;

/// Which helpers are there ([`Offer`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Have {
    ytdlp: bool,
    ffmpeg: bool,
    node: bool,
}

impl Have {
    /// What the tools folder `dir` (and the system) has.
    fn read(dir: &std::path::Path) -> Have {
        Have {
            ytdlp: mdm_tools::installed(dir).is_some(),
            ffmpeg: mdm_tools::ffmpeg::installed(dir).is_some(),
            node: mdm_tools::node::installed(dir).is_some(),
        }
    }

    fn of(&self, t: Tool) -> bool {
        match t {
            Tool::YtDlp => self.ytdlp,
            Tool::Ffmpeg => self.ffmpeg,
            Tool::Node => self.node,
            Tool::GalleryDl => false,
        }
    }
}

/// A look for the JavaScript runtime yt-dlp would use (`jsruntime::current`), which may run the
/// computer's runtimes (`--version`, up to 20 s each): on a thread of its own, started from a
/// window's `show` (never when the window is made), its answer read back on a later frame.
#[derive(Default)]
struct RuntimeProbe {
    /// It needs doing.
    wanted: bool,
    /// Its answer, once there.
    slot: Option<Arc<Mutex<Option<Option<mdm_tools::jsruntime::Runtime>>>>>,
}

impl RuntimeProbe {
    /// Start it, if wanted and not started yet.
    fn start(
        &mut self,
        dir: Option<std::path::PathBuf>,
        ytdlp: Option<std::path::PathBuf>,
        ctx: &egui::Context,
    ) {
        if !self.wanted || self.slot.is_some() {
            return;
        }
        let slot = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        std::thread::spawn(move || {
            let found = mdm_tools::jsruntime::current(dir.as_deref(), ytdlp.as_deref());
            *out.lock().unwrap() = Some(found);
            wake.request_repaint();
        });
        self.slot = Some(slot);
    }

    /// Its answer, once (then it is done).
    fn answer(&mut self) -> Option<Option<mdm_tools::jsruntime::Runtime>> {
        let found = self.slot.as_ref()?.lock().unwrap().take()?;
        self.wanted = false;
        self.slot = None;
        Some(found)
    }
}

/// Whether to offer the helpers at this start: never asked, and one of them is missing (ffmpeg
/// counts only where MDM can install one, Windows; elsewhere the system's is used).
pub fn worth_asking() -> bool {
    let Some(dir) = mdm_tools::tools_dir() else {
        return false;
    };
    mdm_tools::installed(&dir).is_none()
        || (cfg!(windows) && mdm_tools::ffmpeg::installed(&dir).is_none())
}

/// The first row of the page's rows: where they start under the window's banner and steps.
const ROWS_TOP: f32 = 12.0;
/// A row's least height (a name and three lines of what it does).
const ROW: f32 = 72.0;

impl Offer {
    /// Runs no program: a runtime that must be asked is asked from `page`, off its thread.
    pub fn new() -> Self {
        Self::with_dir(mdm_tools::tools_dir())
    }

    /// [`Offer::new`] for the tools folder `dir`.
    fn with_dir(dir: Option<std::path::PathBuf>) -> Self {
        let mut probe = RuntimeProbe::default();
        let node = mdm_tools::jsruntime::available()
            && dir.as_deref().is_some_and(|d| {
                let ytdlp = mdm_tools::installed(d).map(|i| i.exe);
                match mdm_tools::jsruntime::current_now(Some(d), ytdlp.as_deref()) {
                    Some(found) => found.is_none(),
                    // A runtime is there, to be asked: none offered until it answers.
                    None => {
                        probe.wanted = true;
                        false
                    }
                }
            });
        Offer {
            have: dir.as_deref().map(Have::read).unwrap_or_default(),
            dir,
            node,
            probe,
            installs: Vec::new(),
            failed: None,
            installed_here: false,
        }
    }

    /// The helpers the page shows, in order: yt-dlp, ffmpeg, and Node.js where it is offered.
    fn rows(&self) -> Vec<Tool> {
        let mut rows = vec![Tool::YtDlp, Tool::Ffmpeg];
        if self.node {
            rows.push(Tool::Node);
        }
        rows
    }

    /// `t`'s install started here and hasn't ended.
    fn installing(&self, t: Tool) -> bool {
        self.installs
            .iter()
            .any(|(i, answer)| *i == t && answer.lock().unwrap().is_none())
    }

    /// Whether `t` can be installed from here now: missing, not under way, and MDM has a copy of
    /// it for this system (ffmpeg only on Windows: elsewhere the system's is used).
    fn installable(&self, t: Tool) -> bool {
        self.dir.is_some()
            && !self.have.of(t)
            && !self.installing(t)
            && (t != Tool::Ffmpeg || cfg!(windows))
    }

    /// Install MDM's own copy of `t` in the background (`mdm_tools::autoupdate::install_own`:
    /// checked against its publisher's signed checksums, then kept up to date as Helpers' own
    /// copies are). It goes on when the window closes.
    fn install(&mut self, t: Tool, rt: &tokio::runtime::Handle, ctx: &egui::Context) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let slot: Install = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let now = mdm_tools::autoupdate::unix_now();
            let res = mdm_tools::autoupdate::install_own(&dir, t, &net, now).await;
            if let Err(e) = &res {
                tracing::warn!("helper download failed: {}: {e}", t.name());
            }
            *out.lock().unwrap() = Some(res.map(|_| ()).map_err(|e| {
                (
                    crate::errors::tools(&e),
                    crate::errors::tools_advice(&e, display_name(t)),
                )
            }));
            wake.request_repaint();
        });
        self.installs.retain(|(i, _)| *i != t);
        self.installs.push((t, slot));
        if self.failed.as_ref().is_some_and(|(f, ..)| *f == t) {
            self.failed = None;
        }
    }

    /// The installs that ended since the last frame: what is there read again, a failure kept to
    /// be said.
    fn follow(&mut self) {
        let (ended, going): (Vec<_>, Vec<_>) = std::mem::take(&mut self.installs)
            .into_iter()
            .partition(|(_, answer)| answer.lock().unwrap().is_some());
        self.installs = going;
        if ended.is_empty() {
            return;
        }
        for (t, answer) in ended {
            match answer.lock().unwrap().take() {
                Some(Ok(())) => self.installed_here = true,
                Some(Err((why, advice))) => self.failed = Some((t, why, advice)),
                None => {}
            }
        }
        if let Some(dir) = self.dir.as_deref() {
            self.have = Have::read(dir);
        }
    }

    /// The page's headline, for the setup window's banner.
    pub fn headline(&self) -> &'static str {
        // i18n: the headline of the first start's helpers window
        crate::i18n::t("Download videos from YouTube and 1,000+ sites")
    }

    /// The page's subline, for the setup window's banner.
    pub fn subline(&self) -> &'static str {
        if self.node {
            crate::i18n::t(
                "Three free helpers make it work. Get them now, and saving a video is one click away when you find one.",
            )
        } else {
            crate::i18n::t(
                "Two free helpers make it work. Get them now, and saving a video is one click away when you find one.",
            )
        }
    }

    /// The page, `w` wide, under English origin `o`: a row per helper (what it does, and whether
    /// it is there, its Install button or its progress), the promise that they are official and
    /// checked, and how it stands. Its height.
    pub fn page(&mut self, ui: &mut Ui, o: Pos2, w: f32, rt: &tokio::runtime::Handle) -> f32 {
        // The computer's runtime, asked off this thread: Node.js offered if none will do (only
        // before an install starts, so the rows stay as they were when it was pressed).
        let ytdlp = self
            .dir
            .as_deref()
            .and_then(mdm_tools::installed)
            .map(|i| i.exe);
        self.probe.start(self.dir.clone(), ytdlp, ui.ctx());
        if let Some(found) = self.probe.answer()
            && self.installs.is_empty()
        {
            self.node = mdm_tools::jsruntime::available() && found.is_none();
        }
        self.follow();
        let running = self.rows().into_iter().any(|t| self.installing(t));
        if running {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        }
        let p = ui.painter().clone();
        let (ink, muted) = (winui::TEXT(), winui::MUTED());
        // The column of the rows' buttons and states, as wide as the widest Install in this
        // language (#192).
        let rows = self.rows();
        let labels: Vec<&str> = rows.iter().map(|t| install_label(*t)).collect();
        let trail = labels
            .iter()
            .map(|l| winui::button_fit_width(&p, l, 110.0, 200.0))
            .fold(110.0, f32::max);
        let trail_x = w - 18.0 - trail;
        let mut pressed = None;
        let mut y = ROWS_TOP;
        for (k, t) in rows.iter().copied().enumerate() {
            let (icon, name, what) = row_text(t, self.dir.as_deref());
            icon.image(true)
                .paint_at(ui, r(o, 22.0, y + 2.0, 32.0, 32.0));
            let name_rc = winui::text_font(
                &p,
                winui::tahoma_bold(),
                o.x + 68.0,
                o.y + y + 11.0,
                crate::i18n::t(name),
                ink,
            );
            // Painted text a screen reader couldn't see before (#1407).
            winui::readable(ui, name_rc, ("helpers-name", k), crate::i18n::t(name));
            let lines = winui::readable_paragraph(
                ui,
                &p,
                o.x + 68.0,
                o.y + y + 29.0,
                &what,
                muted,
                o.x + trail_x - 12.0,
                14.0,
                ("helpers-what", k),
            );
            // Its state, at the row's end: there, downloading, or its Install button.
            if self.have.of(t) {
                let there = format!(
                    "\u{2714} {}",
                    // i18n: the first start's video helpers: beside a helper that is there already (MDM's own copy or the system's)
                    crate::i18n::t("Installed")
                );
                let (rc, _) =
                    winui::text_right(&p, o.x + w - 18.0, o.y + y + 17.0, &there, winui::SUCCESS());
                winui::readable(ui, rc, ("helpers-there", k), &there);
            } else if self.installing(t) {
                let progress = self
                    .dir
                    .as_deref()
                    .and_then(|d| mdm_tools::autoupdate::checking(d, t));
                let f = progress.as_ref().and_then(|p| p.fraction());
                winui::meter(&p, r(o, trail_x, y + 8.0, trail, 8.0), f.unwrap_or(0.0));
                let line = match f {
                    // The last byte is in: its signature and checksum are being checked.
                    Some(f) if f >= 1.0 => crate::i18n::t("Checking...").to_owned(),
                    Some(f) => format!("{}%", (f * 100.0).floor()),
                    // i18n: the first start's video helpers: a helper's download has started, its size not known yet
                    None => crate::i18n::t("Starting...").to_owned(),
                };
                let (rc, _) = winui::text_right(&p, o.x + w - 18.0, o.y + y + 34.0, &line, muted);
                winui::readable(ui, rc, ("helpers-progress", k), &line);
            } else if self.installable(t) {
                let label = install_label(t);
                let b = winui::button(
                    ui,
                    r(o, trail_x, y + 2.0, trail, 25.0),
                    label,
                    ButtonKind::Normal,
                    false,
                    None,
                );
                if b.clicked() {
                    pressed = Some(vec![t]);
                }
            }
            y += row_height(lines);
        }
        // The promise, across the whole width.
        let (icon, name, what) = promise(self.node);
        icon.image(true)
            .paint_at(ui, r(o, 22.0, y + 2.0, 32.0, 32.0));
        let name_rc = winui::text_font(
            &p,
            winui::tahoma_bold(),
            o.x + 68.0,
            o.y + y + 11.0,
            crate::i18n::t(name),
            ink,
        );
        winui::readable(
            ui,
            name_rc,
            ("helpers-name", "promise"),
            crate::i18n::t(name),
        );
        let lines = winui::readable_paragraph(
            ui,
            &p,
            o.x + 68.0,
            o.y + y + 29.0,
            what,
            muted,
            o.x + w - 18.0,
            14.0,
            ("helpers-what", "promise"),
        );
        y += row_height(lines) - 8.0;

        // How it stands, and the one button that gets every missing helper (#1839: each has its
        // own too), only while more than one is missing.
        winui::etched_h(&p, o.x + 16.0, o.y + y, w - 32.0);
        y += 12.0;
        let missing: Vec<Tool> = rows
            .iter()
            .copied()
            .filter(|t| self.installable(*t))
            .collect();
        let all_at = (missing.len() > 1).then(|| {
            winui::buttons_to_right(
                &p,
                w - 18.0,
                8.0,
                &[("Yes, download them (recommended)", 212.0)],
                300.0,
            )[0]
        });
        let text_end = all_at.map_or(w - 18.0, |(x, _)| x - 12.0);
        let (line, color) = if let Some((t, why, _)) = &self.failed {
            (
                crate::i18n::fmt(
                    // i18n: {name} = yt-dlp, ffmpeg or Node.js; {error} = why
                    crate::i18n::t("{name} could not be downloaded: {error}"),
                    &[("name", display_name(*t)), ("error", why)],
                ),
                winui::ERROR(),
            )
        } else if running {
            (
                crate::i18n::t("They keep downloading if you close this window.").to_owned(),
                muted,
            )
        } else if self.installed_here && missing.is_empty() {
            (
                crate::i18n::t(
                    "All set: on a page with a video, use Tasks → Download video from a page.",
                )
                .to_owned(),
                winui::SUCCESS(),
            )
        } else {
            (
                crate::i18n::t("Not now? Get them later from Tasks → Download video from a page.")
                    .to_owned(),
                muted,
            )
        };
        // A failure is a polite live region: a screen reader speaks it as it comes, without the
        // focus; the other lines never are (#1407, #1978).
        let at = (o.x + 18.0, o.y + y + 13.0, o.x + text_end);
        let lines = if self.failed.is_some() {
            winui::readable_paragraph_live(
                ui,
                &p,
                at.0,
                at.1,
                &line,
                color,
                at.2,
                16.0,
                ("helpers", "status"),
            )
        } else {
            winui::readable_paragraph(
                ui,
                &p,
                at.0,
                at.1,
                &line,
                color,
                at.2,
                16.0,
                ("helpers", "status"),
            )
        };
        // What to do about the failure (#816), under it.
        let mut more = 16.0 * lines.saturating_sub(1) as f32;
        if let Some((_, _, advice)) = &self.failed {
            let advice = advice.as_deref().unwrap_or(crate::i18n::t(
                "Check your connection, then try again, or get them later from the video window.",
            ));
            let n = winui::readable_paragraph(
                ui,
                &p,
                o.x + 18.0,
                o.y + y + 13.0 + more + 18.0,
                advice,
                muted,
                o.x + text_end,
                16.0,
                ("helpers", "advice"),
            );
            more += 18.0 + 16.0 * n.saturating_sub(1) as f32;
        }
        if let Some((x, bw)) = all_at {
            // i18n: the first start's helpers: download every missing helper (yt-dlp, ffmpeg, Node.js) at once (the recommended answer)
            let all = winui::button(
                ui,
                r(o, x, y, bw, 25.0),
                "Yes, download them (recommended)",
                ButtonKind::Accent,
                false,
                None,
            );
            if all.clicked() {
                pressed = Some(missing.clone());
            }
        }
        if let Some(tools) = pressed {
            for t in tools {
                self.install(t, rt, ui.ctx());
            }
        }
        y + 30.0 + more
    }
}

/// A row's height for `lines` lines of what its helper does: at least [`ROW`].
fn row_height(lines: usize) -> f32 {
    (29.0 + 14.0 * lines.saturating_sub(1) as f32 + 22.0).max(ROW)
}

/// A helper's name as its messages say it.
fn display_name(t: Tool) -> &'static str {
    match t {
        Tool::Node => "Node.js",
        t => t.name(),
    }
}

/// A helper's Install button: "Install yt-dlp" (each one's own, as in Helpers).
fn install_label(t: Tool) -> &'static str {
    OwnButton::Install { first: true }.label(t)
}

/// A helper row's icon, name (translated where shown) and what it does, in the interface
/// language.
fn row_text(t: Tool, dir: Option<&std::path::Path>) -> (Icon, &'static str, String) {
    match t {
        Tool::YtDlp => (
            Icon::HelperVideo,
            // i18n: helper name and what it does (yt-dlp is a program's name)
            "yt-dlp: finds the video",
            crate::i18n::t(
                "Reads a page and lists every quality the site offers, up to 4K, from YouTube, Vimeo and over a thousand other sites.",
            )
            .to_owned(),
        ),
        Tool::Ffmpeg => {
            let ffmpeg_here = dir.and_then(mdm_tools::ffmpeg::installed).is_some();
            (
                Icon::HelperMerge,
                // i18n: helper name and what it does (ffmpeg is a program's name)
                "ffmpeg: joins picture and sound",
                if cfg!(windows) || ffmpeg_here {
                    crate::i18n::t(
                        "Sites send their best quality as separate video and audio. ffmpeg puts them together into one file that plays anywhere.",
                    )
                    .to_owned()
                } else {
                    crate::i18n::t(
                        "Sites send their best quality as separate video and audio, and ffmpeg joins them. On this system MDM uses yours: install ffmpeg with your package manager.",
                    )
                    .to_owned()
                },
            )
        }
        Tool::Node | Tool::GalleryDl => (
            Icon::HelperScript,
            // i18n: helper name and what it does (Node.js and YouTube are names)
            "Node.js: unlocks YouTube's qualities",
            crate::i18n::t(
                "YouTube makes yt-dlp run a piece of JavaScript before it lists its videos, and this computer has nothing to run it with. Node.js does.",
            )
            .to_owned(),
        ),
    }
}

/// The promise under the helpers: free, official and checked; three of them with Node.js.
fn promise(node: bool) -> (Icon, &'static str, &'static str) {
    (
        Icon::HelperChecked,
        // i18n: a promise in the helpers window: the helpers are free, official and checked
        "Free, official and checked",
        if node {
            crate::i18n::t(
                "All three are open source. MDM gets them from their official releases with its own downloader and checks each file's signature before it runs. About 140 MB, once.",
            )
        } else {
            crate::i18n::t(
                "Both are open source. MDM gets them from their official releases with its own downloader and checks each file's signature before it runs. About 100 MB, once.",
            )
        },
    )
}

// ---- Video helpers: MDM's own copies or the person's (#51) ------------------------------------

use mdm_tools::autoupdate::{Settled, Tool};
use mdm_tools::chosen::Helper;

/// Helpers (Download video from a page or a gallery → Helpers...): for yt-dlp, ffmpeg and
/// gallery-dl (#58), where MDM takes it from (its own copy, or a program of the person's), and the
/// way to choose another: one copy of each for every program that uses them (#51). Then the
/// JavaScript runtime yt-dlp needs for YouTube (#1448, #1470, #1570): one the person chose, else
/// the computer's own (Deno, Node.js, QuickJS, Bun), else Node.js, MDM's own copy, downloaded and
/// checked here (`mdm_tools::jsruntime`). MDM's own copies are kept up to date
/// (#1544, `mdm_tools::autoupdate`): each row says how its copy stands, with Update and Go back to
/// the previous version; Check for updates checks them all, and the switch below turns the
/// automatic updates off.
pub struct Programs {
    dir: Option<std::path::PathBuf>,
    /// The helpers as found (read again after each change, not every frame: it reads files).
    found: [Option<mdm_tools::Installed>; 3],
    /// The JavaScript runtime yt-dlp uses (`jsruntime::current`), read with `found`: at once when
    /// that runs nothing (`current_now`), else from `probe`, off the window's thread.
    runtime: Option<mdm_tools::jsruntime::Runtime>,
    probe: RuntimeProbe,
    /// MDM's own copy of each tool (`Tool::ALL`'s order) as its updates stand, read with `found`.
    own: [Own; 4],
    /// The update state (`<tools>/updates.json`), read with `found`.
    updates: mdm_tools::autoupdate::State,
    /// `autoupdate::generation()` when they were read: read again when it moves.
    seen: u64,
    /// A chosen program being checked (`--version`), and what it answered.
    checking: Option<(Helper, Answer)>,
    /// MDM's own yt-dlp or gallery-dl being downloaded in place of the system's (#452, #571),
    /// and how that went.
    installing: Option<(Helper, Answer)>,
    /// MDM's own Node.js being downloaded, and how that went.
    installing_runtime: Option<Answer>,
    /// A runtime just chosen being checked, and what it answered.
    checking_runtime: Option<Answer>,
    /// Check for updates, an Update or a Go back under way, and how it went.
    updating: Option<Answer>,
    error: Option<String>,
}

/// MDM's own copy of a tool, as its updates stand.
#[derive(Default)]
struct Own {
    /// The version in use, the one kept to go back to (older), and a newer one waiting for the
    /// downloads using the tool to end.
    current: Option<mdm_tools::Installed>,
    previous: Option<mdm_tools::Installed>,
    staged: Option<mdm_tools::Installed>,
}

const HELPERS: [Helper; 3] = [Helper::YtDlp, Helper::Ffmpeg, Helper::GalleryDl];

/// The tool a helper row is.
fn tool_of(h: Helper) -> Tool {
    match h {
        Helper::YtDlp => Tool::YtDlp,
        Helper::Ffmpeg => Tool::Ffmpeg,
        Helper::GalleryDl => Tool::GalleryDl,
    }
}

/// What a row's own-copy button does (#1801), from where its program comes from now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnButton {
    /// Forget the program the person chose: MDM's own copy is used again (installed first when
    /// there is none).
    Forget,
    /// Download and install MDM's own copy: `first` where there is none yet ("Install …"), else
    /// in place of the system's (#452, #571).
    Install { first: bool },
    /// Greyed: MDM's own copy is already the one used, or MDM has none for this system (the
    /// row's status says so).
    Off,
}

impl OwnButton {
    /// For a row whose program is one the person chose (`chosen`), else the one found
    /// (`found`: whether it is the system's, None when there is none), where MDM has (`has_own`)
    /// or hasn't a copy of its own for this system and processor.
    fn of(chosen: bool, found: Option<bool>, has_own: bool) -> OwnButton {
        match found {
            _ if chosen => OwnButton::Forget,
            _ if !has_own => OwnButton::Off,
            None => OwnButton::Install { first: true },
            Some(true) => OwnButton::Install { first: false },
            Some(false) => OwnButton::Off,
        }
    }

    /// Its label (translated by the button): "Install yt-dlp" (or its tool's) where there is
    /// nothing yet, else "Use MDM's own copy".
    fn label(self, t: Tool) -> &'static str {
        if self != (OwnButton::Install { first: true }) {
            // i18n: Video helpers: button: go back to the copy MDM downloads and checks itself
            return crate::i18n::mark("Use MDM's own copy");
        }
        match t {
            // i18n: button: download and install the yt-dlp program
            Tool::YtDlp => crate::i18n::mark("Install yt-dlp"),
            // i18n: button: download and install the ffmpeg program
            Tool::Ffmpeg => crate::i18n::mark("Install ffmpeg"),
            // i18n: button: download and install the gallery-dl program
            Tool::GalleryDl => crate::i18n::mark("Install gallery-dl"),
            // i18n: button in Download video: download and install Node.js, the JavaScript runtime YouTube needs (Node.js is a program's name)
            Tool::Node => crate::i18n::mark("Install Node.js"),
        }
    }
}

/// A row's status while MDM's own copy of `t` downloads (#1801): how far, once its size is known,
/// as an update says it.
fn downloading_own(dir: Option<&std::path::Path>, tool: Tool) -> String {
    use crate::i18n::{fmt, t};
    match dir
        .and_then(|d| mdm_tools::autoupdate::checking(d, tool))
        .and_then(|p| p.fraction())
    {
        Some(f) => fmt(
            // i18n: Helpers: MDM's own copy of a helper is being downloaded (where there was none, or in place of the system's); {percent} = how far, a number
            t("Downloading MDM's own copy... {percent}%"),
            &[("percent", &format!("{:.0}", f * 100.0))],
        ),
        // i18n: Video helpers: MDM's own yt-dlp is being downloaded
        None => t("Downloading MDM's own copy...").to_owned(),
    }
}

/// How long "Updated to …" is said after an update.
const UPDATED_SHOWN: i64 = 3 * 24 * 3600;

/// A check's answer, once it has one (Err: why the program wasn't taken, in words).
type Answer = Arc<Mutex<Option<Result<(), String>>>>;

impl Programs {
    pub fn new() -> Self {
        Self::with_dir(mdm_tools::tools_dir())
    }

    /// [`Programs::new`] for the tools folder `dir`; runs no program (the runtime, when the
    /// computer's must be asked, is asked from `show`, off the window's thread).
    fn with_dir(dir: Option<std::path::PathBuf>) -> Self {
        let mut p = Programs {
            dir,
            found: [None, None, None],
            runtime: None,
            own: Default::default(),
            updates: Default::default(),
            seen: 0,
            checking: None,
            installing: None,
            installing_runtime: None,
            checking_runtime: None,
            updating: None,
            error: None,
            probe: RuntimeProbe::default(),
        };
        p.look();
        p
    }

    fn look(&mut self) {
        let Some(dir) = self.dir.as_deref() else {
            return;
        };
        self.seen = mdm_tools::autoupdate::generation();
        self.found = [
            mdm_tools::installed(dir),
            mdm_tools::ffmpeg::installed(dir),
            mdm_tools::gallery::installed(dir),
        ];
        let ytdlp = self.found[0].as_ref().map(|i| i.exe.as_path());
        match mdm_tools::jsruntime::current_now(Some(dir), ytdlp) {
            Some(found) => {
                self.runtime = found;
                self.probe = RuntimeProbe::default();
            }
            // Asked from `show`, off the window's thread; the row says it is looking meanwhile.
            None => {
                self.probe = RuntimeProbe {
                    wanted: true,
                    slot: None,
                };
            }
        }
        self.updates = mdm_tools::autoupdate::read(dir);
        for t in Tool::ALL {
            use mdm_tools::autoupdate as au;
            let root = t.root(dir);
            let current = au::own(dir, t);
            let newer_than_current = |i: &mdm_tools::Installed, older: bool| {
                current.as_ref().is_some_and(|c| {
                    if older {
                        mdm_tools::autoupdate::is_newer(&c.version, &i.version)
                    } else {
                        mdm_tools::autoupdate::is_newer(&i.version, &c.version)
                    }
                })
            };
            self.own[t as usize] = Own {
                previous: au::previous_in(&root).filter(|p| newer_than_current(p, true)),
                staged: au::staged_in(&root).filter(|s| newer_than_current(s, false)),
                current,
            };
        }
    }

    /// MDM's own Node.js (#1570): downloaded and checked against its release's signed checksums,
    /// from then on the runtime yt-dlp is told of when the computer has none (`mdm_tools::jsruntime`).
    /// It goes on if the window is closed.
    fn install_runtime(
        &mut self,
        dir: std::path::PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) {
        self.installing_runtime = Some(Self::start_install(Tool::Node, dir, rt, ctx));
        self.error = None;
    }

    /// MDM's own yt-dlp, ffmpeg or gallery-dl, where there is none yet (#1801) or in place of
    /// the system's (#452, #571): downloaded and checked as usual, from then on the one used
    /// (`mdm_tools::installed` and the like) and kept up to date. It goes on if the window is
    /// closed.
    fn install_own(
        &mut self,
        h: Helper,
        dir: std::path::PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) {
        self.installing = Some((h, Self::start_install(tool_of(h), dir, rt, ctx)));
        self.error = None;
    }

    /// Every row's install of MDM's own copy of `t`, in the background
    /// (`mdm_tools::autoupdate::install_own`: its progress shown as an update's, and the
    /// automatic checks going on from it).
    fn start_install(
        t: Tool,
        dir: std::path::PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) -> Answer {
        let slot: Answer = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let now = mdm_tools::autoupdate::unix_now();
            let res = mdm_tools::autoupdate::install_own(&dir, t, &net, now).await;
            *out.lock().unwrap() = Some(res.map(|_| ()).map_err(|e| crate::errors::tools(&e)));
            wake.request_repaint();
        });
        slot
    }

    /// Check for updates (`tools`: every own copy) or one row's Update, now, in the background:
    /// each newer version checked as its first install was. It goes on if the window is closed.
    fn update(
        &mut self,
        tools: Vec<Tool>,
        dir: std::path::PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) {
        let slot: Answer = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let mut failed = None;
            for t in tools {
                let now = mdm_tools::autoupdate::unix_now();
                let res = mdm_tools::autoupdate::check(
                    &dir,
                    t,
                    &net,
                    mdm_tools::autoupdate::Why::Asked,
                    now,
                )
                .await;
                wake.request_repaint();
                if let Err(e) = res {
                    failed.get_or_insert_with(|| {
                        // i18n: Helpers: an update check failed; {name} = yt-dlp, ffmpeg, gallery-dl or node, {error} = why
                        crate::i18n::fmt(
                            crate::i18n::t("{name}: {error}"),
                            &[("name", t.name()), ("error", &crate::errors::tools(&e))],
                        )
                    });
                }
            }
            *out.lock().unwrap() = Some(failed.map_or(Ok(()), Err));
            wake.request_repaint();
        });
        self.updating = Some(slot);
        self.error = None;
    }

    /// Go back to the previous version of `t`, off the window's thread (a switch removes older
    /// folders).
    fn go_back(&mut self, t: Tool, dir: std::path::PathBuf, ctx: &egui::Context) {
        let slot: Answer = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        std::thread::spawn(move || {
            let res = mdm_tools::autoupdate::go_back(&dir, t);
            match &res {
                Ok(Settled::Switched(i)) => tracing::info!("{} back to {}", t.name(), i.version),
                Ok(Settled::Staged(i)) => {
                    tracing::info!("{} back to {} once its downloads end", t.name(), i.version)
                }
                Err(e) => tracing::warn!("{}: going back failed: {e}", t.name()),
            }
            *out.lock().unwrap() = Some(res.map(|_| ()).map_err(|e| crate::errors::tools(&e)));
            wake.request_repaint();
        });
        self.updating = Some(slot);
        self.error = None;
    }

    /// Open until Close.
    pub fn show(&mut self, ctx: &egui::Context, rt: &tokio::runtime::Handle) -> Outcome<()> {
        // The runtime, when the computer's must be asked: off this thread.
        let ytdlp = self.found[0].as_ref().map(|i| i.exe.clone());
        self.probe.start(self.dir.clone(), ytdlp, ctx);
        if let Some(found) = self.probe.answer() {
            self.runtime = found;
        }
        let answered = self
            .checking
            .as_ref()
            .and_then(|(_, slot)| slot.lock().unwrap().take());
        if let Some(res) = answered {
            self.error = res.err();
            self.checking = None;
            self.look();
        }
        let installed = self
            .installing
            .as_ref()
            .and_then(|(_, slot)| slot.lock().unwrap().take());
        if let Some(res) = installed {
            self.error = res.err();
            self.installing = None;
            self.look();
        }
        let mut runtime_done = None;
        for slot in [&mut self.installing_runtime, &mut self.checking_runtime] {
            let res = slot.as_ref().and_then(|a| a.lock().unwrap().take());
            if let Some(res) = res {
                *slot = None;
                runtime_done = Some(res);
            }
        }
        if let Some(res) = runtime_done {
            self.error = res.err();
            self.look();
        }
        if let Some(path) = winui::picked(ctx, "helper-js-runtime")
            && let Some(dir) = self.dir.clone()
        {
            let slot: Answer = Arc::new(Mutex::new(None));
            let (out, wake) = (slot.clone(), ctx.clone());
            std::thread::spawn(move || {
                let res = mdm_tools::jsruntime::choose(&dir, &path)
                    .map(|_| ())
                    .map_err(|e| crate::errors::tools(&e));
                *out.lock().unwrap() = Some(res);
                wake.request_repaint();
            });
            self.checking_runtime = Some(slot);
            self.error = None;
        }
        let updated = self
            .updating
            .as_ref()
            .and_then(|slot| slot.lock().unwrap().take());
        if let Some(res) = updated {
            self.error = res.err();
            self.updating = None;
            self.look();
        }
        // An update under way (here or by itself), or done since: the rows follow it.
        if mdm_tools::autoupdate::generation() != self.seen {
            self.look();
        }
        if self.any_update_running() || self.updating.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        for h in HELPERS {
            if let Some(path) = winui::picked(ctx, &format!("helper-{}", h.name()))
                && let Some(dir) = self.dir.clone()
            {
                let slot = Arc::new(Mutex::new(None));
                let (out, wake) = (slot.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let res = mdm_tools::chosen::choose(&dir, h, &path)
                        .map(|_| ())
                        .map_err(|e| crate::errors::tools(&e));
                    *out.lock().unwrap() = Some(res);
                    wake.request_repaint();
                });
                self.checking = Some((h, slot));
                self.error = None;
            }
        }
        window(
            ctx,
            "video-helpers",
            // i18n: window title: where MDM takes its helper programs (yt-dlp, ffmpeg, gallery-dl) from
            "Helpers",
            [500.0, 402.0],
            false,
            winui::BG(),
            |ui, o| self.body(ui, o, rt),
        )
    }

    /// How MDM's own copy of `t` stands, for its row's first line: an update under way, one
    /// waiting, one done lately, one found, up to date. None before its first check.
    fn update_status(&self, tool: Tool) -> Option<String> {
        use crate::i18n::{fmt, t};
        let own = &self.own[tool as usize];
        let current = own.current.as_ref()?;
        if let Some(p) = self
            .dir
            .as_deref()
            .and_then(|d| mdm_tools::autoupdate::checking(d, tool))
        {
            return Some(match p.fraction() {
                Some(f) => fmt(
                    // i18n: Helpers: a newer version of a helper is being downloaded; {percent} = how far, a number
                    t("Downloading the update... {percent}%"),
                    &[("percent", &format!("{:.0}", f * 100.0))],
                ),
                // i18n: Helpers: MDM is asking whether a newer version of a helper exists
                None => t("Checking for updates...").to_owned(),
            });
        }
        if let Some(s) = &own.staged {
            return Some(fmt(
                // i18n: Helpers: a newer version is downloaded and checked, and is used as soon as the downloads using this helper end; {version} = its version
                t("Version {version} is ready: used once its downloads end"),
                &[("version", &s.version)],
            ));
        }
        let state = self.updates.of(tool);
        let now = mdm_tools::autoupdate::unix_now();
        if let Some(u) = &state.updated
            && u.to == current.version
            && (0..UPDATED_SHOWN).contains(&(now - u.at))
        {
            return Some(fmt(
                // i18n: Helpers: MDM updated its own copy of a helper lately; {version} = the new version
                t("Updated to {version}"),
                &[("version", &u.to)],
            ));
        }
        if state.failed.is_some() {
            // i18n: Helpers: the last update check failed (no network...); MDM checks again later by itself
            return Some(t("Couldn't check for updates: MDM tries again later").to_owned());
        }
        match &state.latest {
            Some(latest) if mdm_tools::autoupdate::is_newer(latest, &current.version) => {
                Some(fmt(
                    // i18n: Helpers: a newer version of a helper exists ("Update" downloads it); {version} = its version
                    t("Version {version} available"),
                    &[("version", latest)],
                ))
            }
            // i18n: Helpers: MDM's own copy of a helper is the newest version
            Some(_) => Some(t("Up to date").to_owned()),
            None => None,
        }
    }

    fn body(&mut self, ui: &mut Ui, o: Pos2, rt: &tokio::runtime::Handle) -> Outcome<()> {
        let p = ui.painter().clone();
        for (k, h) in HELPERS.into_iter().enumerate() {
            let y = o.y + 8.0 + k as f32 * 78.0;
            let name_rc = winui::text_font(
                &p,
                winui::tahoma_bold(),
                o.x + 14.0,
                y + 16.0,
                h.name(),
                winui::TEXT(),
            );
            // Painted text a screen reader couldn't see before (#1407).
            winui::readable(ui, name_rc, ("helper-name", k), h.name());
            let found = self.found[k].as_ref();
            let chosen = self
                .dir
                .as_deref()
                .and_then(|d| mdm_tools::chosen::chosen(d, h));
            // One of the person's, maybe passed over (a gallery-dl too old, #236).
            let mine = chosen.is_some();
            let too_old = chosen
                .as_ref()
                .filter(|c| h == Helper::GalleryDl && mdm_tools::gallery::too_old(&c.version));
            let checking = self.checking.as_ref().is_some_and(|(c, _)| *c == h);
            let downloading = self.installing.as_ref().is_some_and(|(i, _)| *i == h);
            // Whether MDM downloads a copy of its own on this system and processor: yt-dlp
            // almost everywhere, gallery-dl not on macOS nor Linux on ARM, ffmpeg on 64-bit
            // Windows only (BtbN builds none for 32-bit Windows).
            let has_own = match h {
                Helper::YtDlp => mdm_tools::ytdlp_asset().is_some(),
                Helper::GalleryDl => mdm_tools::gallery::asset().is_some(),
                Helper::Ffmpeg => mdm_tools::ffmpeg::available(),
            };
            // MDM's own copy, where MDM has one for this system and processor: back from the
            // person's own program, in place of the system's (which may be the person's to keep
            // up to date, #452, #571), or installed where there is none yet (#1801).
            let own_button =
                OwnButton::of(mine, found.map(mdm_tools::Installed::is_system), has_own);
            let status = match (too_old, found) {
                _ if downloading => downloading_own(self.dir.as_deref(), tool_of(h)),
                _ if checking => {
                    // i18n: Video helpers: a program just chosen is being checked
                    crate::i18n::t("Checking the program...").to_owned()
                }
                (Some(old), _) => crate::i18n::fmt(
                    // i18n: Video helpers: the person's own gallery-dl is too old and not used; {version} = its version, {oldest} = the oldest MDM can use
                    crate::i18n::t(
                        "Your own program ({version}) is too old: MDM needs {oldest} or newer",
                    ),
                    &[
                        ("version", &old.version),
                        ("oldest", mdm_tools::gallery::OLDEST),
                    ],
                ),
                (None, Some(i)) if mine => crate::i18n::fmt(
                    // i18n: Video helpers: {path} = the program the person chose, {version} = its version
                    crate::i18n::t("Your own program: {path} ({version})"),
                    &[
                        ("path", &crate::i18n::ltr(&i.exe.display().to_string())),
                        ("version", &i.version),
                    ],
                ),
                (None, Some(i)) if i.is_system() => crate::i18n::fmt(
                    // i18n: Video helpers: {path} = the system's yt-dlp, ffmpeg or gallery-dl
                    crate::i18n::t("Your system's: {path}"),
                    &[("path", &crate::i18n::ltr(&i.exe.display().to_string()))],
                ),
                (None, Some(i)) => crate::i18n::fmt(
                    // i18n: Video helpers: {version} = the version of MDM's own copy
                    crate::i18n::t("Mister Download Manager's own copy, version {version}"),
                    &[("version", &i.version)],
                ),
                // Linux and macOS as before: there the system's package manager (or Homebrew)
                // installs what MDM has no copy of, and MDM finds it.
                // i18n: Video helpers: no copy of this helper yet
                (None, None) if has_own || !cfg!(windows) => {
                    crate::i18n::t("Not installed yet").to_owned()
                }
                // i18n: Video helpers: MDM can't download this helper on this Windows (ffmpeg on 32-bit Windows); "Use another program" is the button below
                (None, None) => crate::i18n::t(
                    "MDM has no copy for this system: install it, or use another program",
                )
                .to_owned(),
            };
            let status_rc = winui::fit(
                ui,
                &p,
                o.x + 14.0,
                y + 36.0,
                &status,
                winui::MUTED(),
                o.x + 486.0,
                ("helper", k),
            );
            winui::readable(ui, status_rc, ("helper-status", k), &status);
            // Its updates: MDM's own copy's, or the person's to make (#1544).
            let name_end =
                o.x + 14.0 + winui::text_width_font(&p, winui::tahoma_bold(), h.name()) + 10.0;
            let updates = if found.is_some_and(|i| mine || i.is_system()) {
                // i18n: Helpers: a program the person chose, or the system's copy: MDM doesn't update it, the person (or their package manager) does
                Some(crate::i18n::t("Updated by you").to_owned())
            } else {
                self.update_status(tool_of(h))
            };
            self.update_row(ui, &p, o, y, name_end, tool_of(h), updates, rt);
            let busy = self.busy();
            let own_label = own_button.label(tool_of(h));
            // As wide as their labels in this language (#192), side by side from the left.
            let [other_at, own_at] = winui::button_row_from_left(
                &p,
                &[
                    ("Use another program...", 14.0, 180.0),
                    (own_label, 202.0, 180.0),
                ],
                8.0,
                486.0,
                260.0,
            )[..] else {
                unreachable!("two buttons")
            };
            // i18n: file picker title; {name} = yt-dlp or ffmpeg
            let choose = crate::i18n::fmt(crate::i18n::t("Choose {name}"), &[("name", h.name())]);
            let other = winui::button(
                ui,
                r(o, other_at.0, y - o.y + 46.0, other_at.1, 21.0),
                // i18n: Video helpers: button: pick a program (yt-dlp or ffmpeg) already on the computer
                "Use another program...",
                if busy {
                    ButtonKind::Disabled
                } else {
                    ButtonKind::Normal
                },
                false,
                None,
            );
            // The same label on every row: said with its helper's name (#1407), the visible
            // label first so voice control finds it by what it shows.
            winui::name(
                &other,
                &crate::i18n::fmt(
                    // i18n: Helpers: the "Use another program..." button's name for a screen reader; {name} = the helper (yt-dlp, ffmpeg, gallery-dl)
                    crate::i18n::t("Use another program for {name}..."),
                    &[("name", h.name())],
                ),
            );
            if other.clicked() {
                winui::start_pick(ui.ctx(), &format!("helper-{}", h.name()), move || {
                    crate::winui::file_dialog(&choose).pick_file()
                });
            }
            let own_b = winui::button(
                ui,
                r(o, own_at.0, y - o.y + 46.0, own_at.1, 21.0),
                own_label,
                if own_button != OwnButton::Off && !busy {
                    ButtonKind::Normal
                } else {
                    ButtonKind::Disabled
                },
                false,
                None,
            );
            // "Use MDM's own copy" on every row: said with its helper's name (#1407); the
            // Install labels already carry it.
            if own_label == "Use MDM's own copy" {
                winui::name(
                    &own_b,
                    &crate::i18n::fmt(
                        // i18n: Helpers: the "Use MDM's own copy" button's name for a screen reader; {name} = the helper (yt-dlp, ffmpeg, gallery-dl)
                        crate::i18n::t("Use MDM's own copy of {name}"),
                        &[("name", h.name())],
                    ),
                );
            }
            if own_b.clicked()
                && let Some(dir) = self.dir.clone()
            {
                match own_button {
                    OwnButton::Forget => {
                        self.error = mdm_tools::chosen::forget(&dir, h)
                            .err()
                            .map(|e| crate::errors::tools(&e));
                        self.look();
                        // Nothing left once the choice is gone: MDM's own comes, as the
                        // button said.
                        if self.error.is_none() && self.found[k].is_none() && has_own {
                            self.install_own(h, dir, rt, ui.ctx());
                        }
                    }
                    OwnButton::Install { .. } => self.install_own(h, dir, rt, ui.ctx()),
                    OwnButton::Off => {}
                }
            }
            // A line under each, the JavaScript runtime's row below them.
            winui::etched_h(&p, o.x + 14.0, o.y + 82.0 + k as f32 * 78.0, 472.0);
        }
        self.runtime_row(ui, &p, o, HELPERS.len(), rt);
        winui::etched_h(&p, o.x + 14.0, o.y + 8.0 + 4.0 * 78.0 + 4.0, 472.0);
        // Automatic updates, on unless turned off (#1544).
        let mut auto = self.updates.auto;
        let enabled = self.dir.is_some();
        winui::checkbox(
            ui,
            o + egui::vec2(14.0, 332.0),
            &mut auto,
            // i18n: Helpers: checkbox: MDM checks for and installs newer versions of its own yt-dlp, ffmpeg, gallery-dl and Node.js by itself
            "Keep MDM's helpers up to date automatically",
            enabled,
        );
        if auto != self.updates.auto
            && let Some(dir) = self.dir.as_deref()
        {
            mdm_tools::autoupdate::set_auto(dir, auto);
            self.updates.auto = auto;
        }
        let [check_at, close_at] = winui::button_row(
            &p,
            &[("Check for updates", 284.0, 123.0), ("Close", 413.0, 77.0)],
            6.0,
            14.0,
            240.0,
        )[..] else {
            unreachable!("two buttons")
        };
        // MDM's earlier Deno counts as its runtime's own copy: its update brings Node.js (#1570).
        let earlier_deno = self.runtime.as_ref().is_some_and(|r| {
            r.from == mdm_tools::jsruntime::Origin::Own
                && r.kind == mdm_tools::jsruntime::Kind::Deno
        });
        let own: Vec<Tool> = Tool::ALL
            .into_iter()
            .filter(|t| {
                self.own[*t as usize].current.is_some() || (*t == Tool::Node && earlier_deno)
            })
            .collect();
        let y = 370.0;
        // What failed, left of the buttons.
        if let Some(e) = &self.error {
            let rc = winui::fit(
                ui,
                &p,
                o.x + 14.0,
                o.y + y + 15.0,
                e,
                winui::ERROR(),
                o.x + check_at.0 - 8.0,
                "helper-error",
            );
            winui::readable(ui, rc, ("helpers", "error"), e);
        }
        if winui::button(
            ui,
            r(o, check_at.0, y, check_at.1, 21.0),
            // i18n: Helpers: button: look for newer versions of MDM's own helpers now, and install them
            "Check for updates",
            if own.is_empty() || self.busy() || self.any_update_running() {
                ButtonKind::Disabled
            } else {
                ButtonKind::Normal
            },
            false,
            None,
        )
        .clicked()
            && let Some(dir) = self.dir.clone()
        {
            self.update(own, dir, rt, ui.ctx());
        }
        let close = winui::button(
            ui,
            r(o, close_at.0, y, close_at.1, 21.0),
            "Close",
            ButtonKind::Default,
            false,
            None,
        );
        if close.clicked() || winui::escape_pressed(ui) || winui::enter_pressed(ui) {
            Outcome::Done(())
        } else {
            Outcome::Open
        }
    }

    /// A row's update status (`status`, after its name, from `from`) and, for MDM's own copy,
    /// Update and Go back to its previous version, at the row's right end.
    #[allow(clippy::too_many_arguments)]
    fn update_row(
        &mut self,
        ui: &mut Ui,
        p: &egui::Painter,
        o: Pos2,
        y: f32,
        from: f32,
        t: Tool,
        status: Option<String>,
        rt: &tokio::runtime::Handle,
    ) {
        let own = &self.own[t as usize];
        let mut right = o.x + 486.0;
        let mut back = None;
        if own.current.is_some() {
            let back_label = own.previous.as_ref().map(|prev| {
                crate::i18n::fmt(
                    // i18n: Helpers: button: use the previous version of a helper again (if the new one misbehaves); {version} = that version
                    crate::i18n::t("Go back to {version}"),
                    &[("version", &prev.version)],
                )
            });
            // i18n: Helpers: button: check for a newer version of this helper now and install it
            let update_label = crate::i18n::t("Update");
            // "Update" and "Go back to ..." repeat on every row: a screen reader says each
            // with its helper's name (#1407); the runtime's is "Node.js", not "node".
            let for_tool = match t {
                Tool::Node => mdm_tools::jsruntime::Kind::Node.label(),
                _ => t.name(),
            };
            let mut row: Vec<(&str, f32, f32)> = Vec::new();
            if let Some(l) = &back_label {
                row.push((l.as_str(), 270.0, 130.0));
            }
            row.push((update_label, 406.0, 80.0));
            let placed = winui::button_row(p, &row, 6.0, from - o.x, 200.0);
            let busy = self.busy()
                || self
                    .dir
                    .as_deref()
                    .is_some_and(|d| mdm_tools::autoupdate::checking(d, t).is_some());
            let kind = if busy {
                ButtonKind::Disabled
            } else {
                ButtonKind::Normal
            };
            let mut at = placed.iter();
            if let Some(l) = &back_label
                && let Some(&(x, w)) = at.next()
            {
                right = right.min(o.x + x - 8.0);
                let resp = winui::button(ui, r(o, x, y - o.y + 4.0, w, 21.0), l, kind, false, None);
                if let Some(prev) = &own.previous {
                    winui::name(
                        &resp,
                        &crate::i18n::fmt(
                            // i18n: Helpers: the "Go back" button's name for a screen reader; {version} = the earlier version, {name} = the helper (yt-dlp, ffmpeg, gallery-dl, Node.js)
                            crate::i18n::t("Go back to {version} of {name}"),
                            &[("version", &prev.version), ("name", for_tool)],
                        ),
                    );
                }
                if resp.clicked() {
                    back = Some(t);
                }
            }
            if let Some(&(x, w)) = at.next() {
                right = right.min(o.x + x - 8.0);
                let resp = winui::button(
                    ui,
                    r(o, x, y - o.y + 4.0, w, 21.0),
                    update_label,
                    kind,
                    false,
                    None,
                );
                winui::name(
                    &resp,
                    &crate::i18n::fmt(
                        // i18n: Helpers: the Update button's name for a screen reader; {name} = the helper (yt-dlp, ffmpeg, gallery-dl, Node.js)
                        crate::i18n::t("Update {name}"),
                        &[("name", for_tool)],
                    ),
                );
                if resp.clicked()
                    && let Some(dir) = self.dir.clone()
                {
                    self.update(vec![t], dir, rt, ui.ctx());
                }
            }
        }
        if let Some(s) = &status {
            let rc = winui::fit(
                ui,
                p,
                from,
                y + 16.0,
                s,
                winui::MUTED(),
                right,
                ("helper-update", t as usize),
            );
            winui::readable(ui, rc, ("helper-update", t as usize), s);
        }
        if let (Some(t), Some(dir)) = (back, self.dir.clone()) {
            self.go_back(t, dir, ui.ctx());
        }
    }

    /// A check or a download is under way: the buttons wait.
    fn busy(&self) -> bool {
        self.checking.is_some()
            || self.installing.is_some()
            || self.installing_runtime.is_some()
            || self.checking_runtime.is_some()
            || self.updating.is_some()
    }

    /// An update check runs in this copy of MDM (the window's, or MDM's own by itself).
    fn any_update_running(&self) -> bool {
        self.dir.as_deref().is_some_and(|d| {
            Tool::ALL
                .iter()
                .any(|t| mdm_tools::autoupdate::checking(d, *t).is_some())
        })
    }

    /// The JavaScript runtime's row, the `k`th (#1448, #1470, #1570): YouTube makes yt-dlp solve a
    /// JavaScript challenge, and without a runtime its look-ups are slow, list few qualities or
    /// fail. It says which runtime yt-dlp uses and where it comes from: one the person chose
    /// (**Use another program...**, any runtime yt-dlp supports: YairDies' idea, #1541), the
    /// computer's own, or MDM's own Node.js, which **Use MDM's own copy** downloads where the
    /// computer has none (and which replaces the Deno earlier versions of MDM installed).
    fn runtime_row(
        &mut self,
        ui: &mut Ui,
        p: &egui::Painter,
        o: Pos2,
        k: usize,
        rt: &tokio::runtime::Handle,
    ) {
        use mdm_tools::jsruntime::{Kind, Origin};
        let y = o.y + 8.0 + k as f32 * 78.0;
        // i18n: Video helpers: the row of the JavaScript runtime (Node.js, Deno...) yt-dlp needs for YouTube's videos
        let title = crate::i18n::t("JavaScript runtime (for YouTube)");
        let title_rc = winui::text_font(
            p,
            winui::tahoma_bold(),
            o.x + 14.0,
            y + 16.0,
            title,
            winui::TEXT(),
        );
        // Painted text a screen reader couldn't see before (#1407).
        winui::readable(ui, title_rc, ("helper-name", k), title);
        let runtime = self.runtime.clone();
        // MDM's own Node.js, the one in use.
        let own_node = runtime
            .as_ref()
            .is_some_and(|r| r.from == Origin::Own && r.kind == Kind::Node);
        // What it is for, in one line after its name; for MDM's own copy, Update and Go back there
        // instead, and how its updates stand after its version below (the title is long in some
        // languages, the room beside it short).
        let after = winui::text_width_font(p, winui::tahoma_bold(), title) + 10.0;
        let updates = own_node.then(|| self.update_status(Tool::Node)).flatten();
        if own_node {
            self.update_row(ui, p, o, y, o.x + 14.0 + after, Tool::Node, None, rt);
        } else {
            // i18n: Video helpers: what the JavaScript runtime is for, after its name (yt-dlp is a program's name)
            let what = crate::i18n::t("lets yt-dlp list all of YouTube's qualities");
            let what_rc = winui::fit(
                ui,
                p,
                o.x + 14.0 + after,
                y + 16.0,
                what,
                winui::MUTED(),
                o.x + 486.0,
                ("helper-what", k),
            );
            winui::readable(ui, what_rc, ("helper-what", k), what);
        }
        let available = mdm_tools::jsruntime::available();
        let path_of =
            |r: &mdm_tools::jsruntime::Runtime| crate::i18n::ltr(&r.exe.display().to_string());
        let named = |r: &mdm_tools::jsruntime::Runtime| match &r.version {
            Some(v) => format!("{} {v}", r.kind.label()),
            None => r.kind.label().to_owned(),
        };
        let looking = self.probe.wanted;
        let status = if self.installing_runtime.is_some() {
            downloading_own(self.dir.as_deref(), Tool::Node)
        } else if looking {
            // i18n: Video helpers: MDM is asking the JavaScript runtimes on the computer (Node.js, Deno...) for their versions, to see which one yt-dlp can use
            crate::i18n::t("Looking for a JavaScript runtime on this computer...").to_owned()
        } else if self.checking_runtime.is_some() {
            crate::i18n::t("Checking the program...").to_owned()
        } else if let Some(r) = &runtime {
            match (r.from, r.kind) {
                (Origin::Chosen, _) => crate::i18n::fmt(
                    // i18n: Video helpers: the JavaScript runtime the person chose; {runtime} = its name and version ("Node.js 24.11.0"), {path} = where it is
                    crate::i18n::t("{runtime}, your own program: {path}"),
                    &[("runtime", &named(r)), ("path", &path_of(r))],
                ),
                (Origin::System, _) => crate::i18n::fmt(
                    // i18n: Video helpers: the computer's own JavaScript runtime, used as it is (MDM needn't download one); {runtime} = its name and version ("Node.js 24.11.0", "Deno 2.5.6"), {path} = where it is
                    crate::i18n::t("{runtime}, your system's, nothing to download: {path}"),
                    &[("runtime", &named(r)), ("path", &path_of(r))],
                ),
                (Origin::Own, Kind::Node) => {
                    let own = crate::i18n::fmt(
                        // i18n: Video helpers: MDM's own Node.js is the JavaScript runtime; {runtime} = "Node.js" and its version
                        crate::i18n::t("{runtime}, Mister Download Manager's own copy"),
                        &[("runtime", &named(r))],
                    );
                    match &updates {
                        Some(u) => format!("{own} · {u}"),
                        None => own,
                    }
                }
                (Origin::Own, _) => crate::i18n::fmt(
                    // i18n: Video helpers: the Deno an earlier version of MDM installed, still used until MDM's own Node.js replaces it (by the next update, or Use MDM's own copy); {runtime} = "Deno" and its version
                    crate::i18n::t(
                        "{runtime}, MDM's earlier copy: Node.js takes its place at the next update",
                    ),
                    &[("runtime", &named(r))],
                ),
            }
        } else if available {
            crate::i18n::t("Not installed yet").to_owned()
        } else {
            // i18n: Video helpers: MDM can't download Node.js on this system (an unusual processor); Node.js and Deno are program names; "Use another program" is the button below
            crate::i18n::t(
                "MDM has no copy for this system: install Node.js or Deno, or use another program",
            )
            .to_owned()
        };
        let status_rc = winui::fit(
            ui,
            p,
            o.x + 14.0,
            y + 36.0,
            &status,
            winui::MUTED(),
            o.x + 486.0,
            ("helper", k),
        );
        winui::readable(ui, status_rc, ("helper-status", k), &status);
        let chosen = runtime.as_ref().is_some_and(|r| r.from == Origin::Chosen);
        // MDM's own copy, as the helpers' rows bring it: back from a chosen runtime, or
        // downloaded where the computer has none ("Install Node.js") or only MDM's earlier Deno
        // (which it replaces, as the system's copy of a helper); never in place of the
        // computer's own runtime.
        let own_button = OwnButton::of(
            chosen,
            runtime
                .as_ref()
                .map(|r| r.from == Origin::Own && r.kind != Kind::Node),
            available,
        );
        let own_possible = own_button != OwnButton::Off;
        let busy = self.busy() || looking;
        // While the computer's runtimes are asked, not yet "Install".
        let own_label = if looking { OwnButton::Off } else { own_button }.label(Tool::Node);
        let [other_at, own_at] = winui::button_row_from_left(
            p,
            &[
                ("Use another program...", 14.0, 180.0),
                (own_label, 202.0, 180.0),
            ],
            8.0,
            486.0,
            260.0,
        )[..] else {
            unreachable!("two buttons")
        };
        // i18n: file picker title: choose a JavaScript runtime's program (node.exe, deno.exe, qjs.exe, bun.exe)
        let choose = crate::i18n::t("Choose a JavaScript runtime (Node.js, Deno, QuickJS or Bun)")
            .to_owned();
        let other = winui::button(
            ui,
            r(o, other_at.0, y - o.y + 46.0, other_at.1, 21.0),
            "Use another program...",
            if busy {
                ButtonKind::Disabled
            } else {
                ButtonKind::Normal
            },
            false,
            None,
        );
        // The same label on every row: said with what it chooses (#1407), the visible label
        // first so voice control finds it by what it shows.
        winui::name(
            &other,
            // i18n: Helpers: the "Use another program..." button's name for a screen reader, on the JavaScript runtime's row (Node.js, Deno, QuickJS or Bun)
            crate::i18n::t("Use another program as the JavaScript runtime..."),
        );
        if other.clicked() {
            winui::start_pick(ui.ctx(), "helper-js-runtime", move || {
                crate::winui::file_dialog(&choose).pick_file()
            });
        }
        let own_b = winui::button(
            ui,
            r(o, own_at.0, y - o.y + 46.0, own_at.1, 21.0),
            own_label,
            if own_possible && !busy {
                ButtonKind::Normal
            } else {
                ButtonKind::Disabled
            },
            false,
            None,
        );
        // "Use MDM's own copy" on every row: said with its helper's name (#1407); the
        // Install labels already carry it.
        if own_label == "Use MDM's own copy" {
            winui::name(
                &own_b,
                &crate::i18n::fmt(
                    // i18n: Helpers: the "Use MDM's own copy" button's name for a screen reader; {name} = the helper (Node.js)
                    crate::i18n::t("Use MDM's own copy of {name}"),
                    &[("name", Kind::Node.label())],
                ),
            );
        }
        if own_b.clicked()
            && let Some(dir) = self.dir.clone()
        {
            if chosen {
                self.error = mdm_tools::jsruntime::forget(&dir)
                    .err()
                    .map(|e| crate::errors::tools(&e));
                self.look();
                // Nothing found once the choice is gone: MDM's own is downloaded (once the
                // computer's runtimes, if they must be asked, have answered: Use MDM's own copy
                // again then).
                if self.runtime.is_none() && !self.probe.wanted && available {
                    self.install_runtime(dir, rt, ui.ctx());
                }
            } else {
                self.install_runtime(dir, rt, ui.ctx());
            }
        }
    }
}

/// MDM's own helpers kept up to date while it runs (#1544): a few seconds after the start, then
/// every [`Keeper::EVERY`], the checks that are due (`mdm_tools::autoupdate::run_due`: each tool
/// at most as often as its own interval, remembered across starts), off the window's thread.
#[derive(Default)]
pub struct Keeper {
    /// When the last round started.
    last: Option<std::time::Instant>,
    /// A round is running.
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Keeper {
    /// How often what is due is looked at.
    pub const EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

    /// Start a round when one is due: `started` is when MDM started. Off-screen test runs (and
    /// tests) only when MDM_HELPER_UPDATES=1 says so: their tools folder is a test's.
    pub fn tick(
        &mut self,
        rt: &tokio::runtime::Handle,
        started: std::time::Instant,
        ctx: Option<&egui::Context>,
    ) {
        use std::sync::atomic::Ordering;
        let asked = std::env::var_os("MDM_HELPER_UPDATES").is_some_and(|v| v == "1");
        if (cfg!(test) || crate::offscreen()) && !asked {
            return;
        }
        let first_at = crate::help::CHECK_AFTER_START;
        let wait = match self.last {
            None => first_at.saturating_sub(started.elapsed()),
            Some(at) => Self::EVERY.saturating_sub(at.elapsed()),
        };
        if !wait.is_zero() || self.running.load(Ordering::Acquire) {
            if let Some(ctx) = ctx
                && !wait.is_zero()
            {
                ctx.request_repaint_after(wait);
            }
            return;
        }
        let Some(dir) = mdm_tools::tools_dir() else {
            return;
        };
        self.last = Some(std::time::Instant::now());
        self.running.store(true, Ordering::Release);
        let running = self.running.clone();
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let now = mdm_tools::autoupdate::unix_now();
            for (tool, found) in mdm_tools::autoupdate::run_due(&dir, &net, now).await {
                tracing::info!("helper update check: {}: {found:?}", tool.name());
            }
            running.store(false, Ordering::Release);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opening the first start's page or Helpers runs no program on the window's thread: a
    /// runtime on the computer that must be asked for its version is asked from `show`, on a
    /// thread of its own. (A runtime chosen in the tools folder, the one case told at once, is
    /// read from its file.)
    #[test]
    fn opening_the_helpers_windows_runs_no_program() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let before = mdm_tools::jsruntime::probes();
        let first = Offer::with_dir(Some(tools.clone()));
        let helpers = Programs::with_dir(Some(tools.clone()));
        assert_eq!(mdm_tools::jsruntime::probes(), before);
        // Whatever this computer has, it is either known now or left to the probe.
        assert!(first.probe.slot.is_none() && helpers.probe.slot.is_none());
        if helpers.probe.wanted {
            assert_eq!(helpers.runtime, None);
        }
    }

    /// #1801: a helper not installed yet gets "Install <it>" where MDM has a copy for this
    /// system, greyed where it hasn't; the system's copy is replaced, a chosen program forgotten,
    /// MDM's own copy in use leaves nothing to do.
    #[test]
    fn the_own_copy_button_installs_what_is_missing() {
        use OwnButton::*;
        // Nothing yet.
        assert_eq!(OwnButton::of(false, None, true), Install { first: true });
        assert_eq!(OwnButton::of(false, None, false), Off);
        // The system's copy.
        assert_eq!(
            OwnButton::of(false, Some(true), true),
            Install { first: false }
        );
        assert_eq!(OwnButton::of(false, Some(true), false), Off);
        // MDM's own copy in use.
        assert_eq!(OwnButton::of(false, Some(false), true), Off);
        // The person's own program, wherever MDM has a copy or not.
        for found in [None, Some(true), Some(false)] {
            assert_eq!(OwnButton::of(true, found, true), Forget);
            assert_eq!(OwnButton::of(true, found, false), Forget);
        }
        // Labels: each tool's own Install, already in the catalogs (gallery-dl's window, Download
        // video), else Use MDM's own copy.
        let install = Install { first: true };
        assert_eq!(install.label(Tool::GalleryDl), "Install gallery-dl");
        assert_eq!(install.label(Tool::YtDlp), "Install yt-dlp");
        assert_eq!(install.label(Tool::Ffmpeg), "Install ffmpeg");
        assert_eq!(install.label(Tool::Node), "Install Node.js");
        for b in [Install { first: false }, Forget, Off] {
            assert_eq!(b.label(Tool::GalleryDl), "Use MDM's own copy");
        }
    }

    /// #1839: the page downloads nothing by itself: made and read, every missing helper only
    /// offers its Install button, and an empty tools folder has yt-dlp to install (ffmpeg too on
    /// Windows, where MDM has its own).
    #[test]
    fn the_helpers_page_downloads_nothing_unasked() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let mut offer = Offer::with_dir(Some(tools.clone()));
        offer.follow();
        assert!(offer.installs.is_empty());
        for t in offer.rows() {
            assert!(!offer.installing(t));
        }
        if !offer.have.ytdlp {
            assert!(offer.installable(Tool::YtDlp));
        }
        assert_eq!(
            offer.installable(Tool::Ffmpeg),
            cfg!(windows) && !offer.have.ffmpeg
        );
        // Nothing was written in the tools folder.
        assert_eq!(std::fs::read_dir(&tools).unwrap().count(), 0);
    }

    /// #1839: a helper that is there already (here a yt-dlp chosen in Helpers) says so and has no
    /// Install button: only what is missing is offered.
    #[test]
    fn a_helper_already_there_is_not_offered_again() {
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let mut offer = Offer::with_dir(Some(tools));
        offer.have.ytdlp = true;
        assert!(!offer.installable(Tool::YtDlp));
        // No tools folder (no data folder): nothing can be installed.
        let none = Offer::with_dir(None);
        assert!(none.rows().iter().all(|t| !none.installable(*t)));
    }

    /// Before its size is known (or with no install under way here), the download's line says
    /// only that it downloads.
    #[test]
    fn a_download_without_its_size_says_no_percent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            downloading_own(Some(dir.path()), Tool::GalleryDl),
            "Downloading MDM's own copy..."
        );
        assert_eq!(
            downloading_own(None, Tool::Node),
            "Downloading MDM's own copy..."
        );
    }
}

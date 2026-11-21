//! The first start's offer of the video helpers (user request 2026-11-01): yt-dlp finds a page's
//! video, ffmpeg joins its picture and sound. Both are asked for once, explained, and downloaded
//! on Yes (through MDM's own engine, checked against their publishers' signed checksums: see
//! mdm_tools); Not now leaves them to Tasks → Download video from a page, which offers them again.

use std::sync::Arc;
use std::sync::Mutex;

use egui::{Pos2, Ui};

use crate::dialogs::{Outcome, window};
use crate::icons::Icon;
use crate::winui::{self, ButtonKind, r};

/// The helpers' download, one after the other (user decision 2026-11-01: one at a time, one
/// progress bar for both).
#[derive(Default)]
struct State {
    /// The helper downloading now: its name, its place (1-based) and how many there are.
    current: Option<(&'static str, usize, usize)>,
    /// The helpers done: name and version.
    ready: Vec<(&'static str, String)>,
    /// The helper that failed, why, and what to do about it (#816; the ones after it are not
    /// tried).
    error: Option<(&'static str, String, Option<String>)>,
    done: bool,
}

struct Job {
    /// The current helper's bytes.
    progress: Arc<mdm_tools::update::Progress>,
    state: Arc<Mutex<State>>,
}

pub struct Helpers {
    dir: Option<std::path::PathBuf>,
    /// Once Yes was pressed.
    job: Option<Job>,
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

impl Helpers {
    pub fn new() -> Self {
        Helpers {
            dir: mdm_tools::tools_dir(),
            job: None,
        }
    }

    /// Download the missing helpers one after the other, in the background: they go on when
    /// the window is closed. Try again starts over with the ones still missing.
    fn start(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let mut list = Vec::new();
        if mdm_tools::installed(&dir).is_none() {
            list.push("yt-dlp");
        }
        // ffmpeg: MDM's own on Windows (it has no build elsewhere, where the system's is used).
        if cfg!(windows) && mdm_tools::ffmpeg::installed(&dir).is_none() {
            list.push("ffmpeg");
        }
        let job = Job {
            progress: Arc::default(),
            state: Arc::default(),
        };
        let (progress, state, ctx) = (job.progress.clone(), job.state.clone(), ctx.clone());
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let total = list.len();
            for (k, name) in list.into_iter().enumerate() {
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    progress.got.store(0, Relaxed);
                    progress.total.store(0, Relaxed);
                }
                state.lock().unwrap().current = Some((name, k + 1, total));
                let res = if name == "ffmpeg" {
                    mdm_tools::ffmpeg::install_latest(&dir, &net, Some(&progress)).await
                } else {
                    mdm_tools::install_latest(&dir, &net, Some(&progress)).await
                };
                let mut s = state.lock().unwrap();
                match res {
                    Ok(installed) => s.ready.push((name, installed.version)),
                    Err(e) => {
                        tracing::warn!("helper download failed: {name}: {e}");
                        s.error = Some((
                            name,
                            crate::errors::tools(&e),
                            crate::errors::tools_advice(&e, name),
                        ));
                        break;
                    }
                }
            }
            let mut s = state.lock().unwrap();
            s.current = None;
            s.done = true;
            drop(s);
            ctx.request_repaint();
        });
        self.job = Some(job);
    }

    /// Open while shown (after Yes too, with the downloads' progress); Closed on Not now, Close
    /// or the window's X.
    pub fn show(&mut self, ctx: &egui::Context, rt: &tokio::runtime::Handle) -> Outcome<()> {
        let (running, failed) = match &self.job {
            Some(job) => {
                let s = job.state.lock().unwrap();
                (!s.done, s.error.is_some())
            }
            None => (false, false),
        };
        if running {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        let mut answer = None;
        // A long translation's subline makes the banner taller (winui::banner).
        let extra = crate::dialogs::grow_extra(ctx, "helpers-banner")
            + crate::dialogs::grow_extra(ctx, "helpers-error")
            + crate::dialogs::grow_extra(ctx, "helpers-advice");
        window(
            ctx,
            "helpers",
            // i18n: title of the first start's window that offers yt-dlp and ffmpeg
            "Get ready for video downloads",
            // Taller once Yes is pressed: the progress under the last section.
            [
                520.0,
                if self.job.is_some() { 418.0 } else { 382.0 } + extra,
            ],
            false,
            winui::BG(),
            |ui, o| {
                self.body(ui, o, running, failed, &mut answer);
                match answer.take() {
                    Some(true) => {
                        self.start(rt, ui.ctx());
                        Outcome::Open
                    }
                    Some(false) => Outcome::Closed,
                    None if winui::escape_pressed(ui) => Outcome::Closed,
                    None => Outcome::Open,
                }
            },
        )
    }

    fn body(
        &mut self,
        ui: &mut Ui,
        o: Pos2,
        running: bool,
        failed: bool,
        answer: &mut Option<bool>,
    ) {
        let p = ui.painter().clone();
        let (ink, muted) = (winui::TEXT(), winui::MUTED());
        let w = 520.0;
        // The banner: MDM's own logo, what this is for, in one line, and why now.
        let extra = winui::banner(
            ui,
            &p,
            o,
            w,
            "helpers-banner",
            // i18n: the headline of the first start's helpers window
            crate::i18n::t("Download videos from YouTube and 1,000+ sites"),
            crate::i18n::t(
                "Two free helpers make it work. Get them now, and saving a video is one click away when you find one.",
            ),
        );
        // Everything below it moves down with a taller banner.
        let o = o + egui::vec2(0.0, extra);

        let ffmpeg_here = self
            .dir
            .as_deref()
            .and_then(mdm_tools::ffmpeg::installed)
            .is_some();
        let rows: [(Icon, &str, String); 3] = [
            (
                Icon::HelperVideo,
                // i18n: helper name and what it does (yt-dlp is a program's name)
                "yt-dlp: finds the video",
                crate::i18n::t(
                    "Reads a page and lists every quality the site offers, up to 4K, from YouTube, Vimeo and over a thousand other sites.",
                )
                .to_owned(),
            ),
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
            ),
            (
                Icon::HelperChecked,
                // i18n: a promise in the helpers window: the helpers are free, official and checked
                "Free, official and checked",
                crate::i18n::t(
                    "Both are open source. MDM gets them from their official releases with its own downloader and checks each file's signature before it runs. About 100 MB, once.",
                )
                .to_owned(),
            ),
        ];
        for (k, (icon, name, what)) in rows.iter().enumerate() {
            let y = 100.0 + k as f32 * 72.0;
            icon.image(true)
                .paint_at(ui, r(o, 22.0, y + 2.0, 32.0, 32.0));
            winui::text_font(
                &p,
                winui::tahoma_bold(),
                o.x + 68.0,
                o.y + y + 11.0,
                crate::i18n::t(name),
                ink,
            );
            winui::paragraph(
                &p,
                o.x + 68.0,
                o.y + y + 29.0,
                what,
                muted,
                o.x + w - 18.0,
                14.0,
            );
        }

        // After Yes: one bar for both, centred under the last section, and what it is doing.
        let mut y = 322.0;
        if let Some(job) = &self.job {
            let st = job.state.lock().unwrap();
            let f = job.progress.fraction().unwrap_or(0.0);
            let (fraction, line, color) = if let Some((name, error, _)) = &st.error {
                (
                    st.ready.len() as f32 / (st.ready.len() + 1) as f32,
                    crate::i18n::fmt(
                        // i18n: {name} = yt-dlp or ffmpeg; {error} = why
                        crate::i18n::t("{name} could not be downloaded: {error}"),
                        &[("name", name), ("error", error)],
                    ),
                    winui::ERROR(),
                )
            } else if st.done {
                let list = st
                    .ready
                    .iter()
                    .map(|(n, v)| format!("{n} {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                (
                    1.0,
                    crate::i18n::fmt(
                        // i18n: the helpers are downloaded, checked and ready; {list} = names and versions, e.g. "yt-dlp 2026.10.26, ffmpeg 9.0"
                        crate::i18n::t("Ready: {list}"),
                        &[("list", &list)],
                    ),
                    winui::SUCCESS(),
                )
            } else if let Some((name, k, total)) = st.current
                && f >= 1.0
            {
                (
                    k as f32 / total as f32,
                    crate::i18n::fmt(
                        // i18n: a downloaded helper's signature and checksum are being checked; {name} = yt-dlp or ffmpeg
                        crate::i18n::t("Checking {name} ({n} of {total})..."),
                        &[
                            ("name", name),
                            ("n", &crate::i18n::integer(k as u64)),
                            ("total", &crate::i18n::integer(total as u64)),
                        ],
                    ),
                    ink,
                )
            } else if let Some((name, k, total)) = st.current {
                (
                    ((k - 1) as f32 + f) / total as f32,
                    crate::i18n::fmt(
                        // i18n: {name} = yt-dlp or ffmpeg; {n} of {total} = which helper of how many; {percent} = how far, e.g. 45%
                        crate::i18n::t("Downloading {name} ({n} of {total})... {percent}"),
                        &[
                            ("name", name),
                            ("n", &crate::i18n::integer(k as u64)),
                            ("total", &crate::i18n::integer(total as u64)),
                            ("percent", &format!("{}%", (f * 100.0).floor())),
                        ],
                    ),
                    ink,
                )
            } else {
                (0.0, crate::i18n::t("Starting...").to_owned(), ink)
            };
            drop(st);
            let bar_w = 320.0;
            winui::meter(&p, r(o, (w - bar_w) / 2.0, 312.0, bar_w, 8.0), fraction);
            let room = w - 40.0;
            let mut more = 0.0;
            if failed && winui::text_width(&p, &line) > room {
                // A failure is read whole (#816): wrapped, the window growing by its lines past
                // the first.
                let n = winui::paragraph(
                    &p,
                    o.x + 20.0,
                    o.y + 340.0,
                    &line,
                    color,
                    o.x + w - 20.0,
                    16.0,
                );
                more = 16.0 * n.saturating_sub(1) as f32;
            } else {
                // Centred on screen (the same place in both directions); cut to the window.
                let shown = winui::ellipsize(&p, &winui::tahoma(), &line, room);
                let tw = winui::text_width(&p, &shown);
                let rc = winui::text_at(
                    &p,
                    winui::tahoma(),
                    (o.x + (w - tw) / 2.0).round(),
                    o.y + 340.0,
                    &shown,
                    color,
                );
                if shown != line {
                    winui::full_on_hover(ui, rc, "helpers-status", &line);
                }
            }
            crate::dialogs::grow_px(ui.ctx(), "helpers-error", more);
            y += 36.0 + more;
        }

        // The way back later, then the answer.
        winui::etched_h(&p, o.x + 16.0, o.y + y, w - 32.0);
        // What to do about the failure (#816), when it says.
        let advice = self.job.as_ref().and_then(|job| {
            let st = job.state.lock().unwrap();
            st.error.as_ref().and_then(|(_, _, a)| a.clone())
        });
        let later = if self.job.is_some() && running {
            crate::i18n::t("They keep downloading if you close this window.")
        } else if self.job.is_some() && !failed {
            crate::i18n::t(
                "All set: on a page with a video, use Tasks → Download video from a page.",
            )
        } else if let Some(advice) = &advice {
            advice.as_str()
        } else if self.job.is_some() {
            crate::i18n::t(
                "Check your connection, then try again, or get them later from the video window.",
            )
        } else {
            crate::i18n::t("Not now? Get them later from Tasks → Download video from a page.")
        };
        // Wrapped: the window grows by the lines past the first.
        let lines = winui::paragraph(
            &p,
            o.x + 16.0,
            o.y + y + 20.0,
            later,
            muted,
            o.x + w - 16.0,
            16.0,
        );
        crate::dialogs::grow_px(
            ui.ctx(),
            "helpers-advice",
            16.0 * lines.saturating_sub(1) as f32,
        );
        let y = y + 28.0 + 16.0 * lines.saturating_sub(1) as f32;
        if self.job.is_none() {
            // As wide as their labels in this language (#192), the row ending where it ends in
            // English.
            let [not_now_at, yes_at] = winui::button_row(
                &p,
                &[
                    ("Not now", 196.0, 90.0),
                    ("Yes, download them (recommended)", 292.0, 212.0),
                ],
                6.0,
                16.0,
                360.0,
            )[..] else {
                unreachable!("two buttons")
            };
            let not_now = winui::button(
                ui,
                r(o, not_now_at.0, y, not_now_at.1, 25.0),
                "Not now",
                ButtonKind::Normal,
                false,
                None,
            );
            // i18n: the first start's helpers window: download yt-dlp and ffmpeg (the recommended answer)
            let yes = winui::button(
                ui,
                r(o, yes_at.0, y, yes_at.1, 25.0),
                "Yes, download them (recommended)",
                ButtonKind::Accent,
                false,
                None,
            );
            if yes.clicked() || winui::enter_pressed(ui) {
                *answer = Some(true);
            } else if not_now.clicked() {
                *answer = Some(false);
            }
        } else {
            let [again_at, close_at] = winui::button_row(
                &p,
                &[("Try again", 318.0, 90.0), ("Close", 414.0, 90.0)],
                6.0,
                16.0,
                200.0,
            )[..] else {
                unreachable!("two buttons")
            };
            if failed && !running {
                let again = winui::button(
                    ui,
                    r(o, again_at.0, y, again_at.1, 25.0),
                    "Try again",
                    ButtonKind::Normal,
                    false,
                    None,
                );
                if again.clicked() {
                    *answer = Some(true);
                }
            }
            let close = winui::button(
                ui,
                r(o, close_at.0, y, close_at.1, 25.0),
                "Close",
                ButtonKind::Default,
                false,
                None,
            );
            if close.clicked() || winui::enter_pressed(ui) {
                *answer = Some(false);
            }
        }
    }
}

// ---- Video helpers: MDM's own copies or the person's (#51) ------------------------------------

use mdm_tools::autoupdate::{Settled, Tool};
use mdm_tools::chosen::Helper;

/// Helpers (Download video from a page or a gallery → Helpers...): for yt-dlp, ffmpeg and
/// gallery-dl (#58), where MDM takes it from (its own copy, or a program of the person's), and the
/// way to choose another: one copy of each for every program that uses them (#51). Then the
/// JavaScript runtime yt-dlp needs for YouTube (#1448, #1470): the computer's own (Node.js, Deno),
/// else Deno, MDM's own copy, downloaded and checked here. MDM's own copies are kept up to date
/// (#1544, `mdm_tools::autoupdate`): each row says how its copy stands, with Update and Go back to
/// the previous version; Check for updates checks them all, and the switch below turns the
/// automatic updates off.
pub struct Programs {
    dir: Option<std::path::PathBuf>,
    /// The helpers as found (read again after each change, not every frame: it reads files).
    found: [Option<mdm_tools::Installed>; 3],
    /// MDM's own Deno, and the computer's own runtime (`proc::system_js_runtime`), read with
    /// `found`.
    deno: Option<mdm_tools::Installed>,
    runtime: Option<std::path::PathBuf>,
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
    /// MDM's own Deno being downloaded, and how that went.
    installing_deno: Option<Answer>,
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

/// How long "Updated to …" is said after an update.
const UPDATED_SHOWN: i64 = 3 * 24 * 3600;

/// A check's answer, once it has one (Err: why the program wasn't taken, in words).
type Answer = Arc<Mutex<Option<Result<(), String>>>>;

impl Programs {
    pub fn new() -> Self {
        let mut p = Programs {
            dir: mdm_tools::tools_dir(),
            found: [None, None, None],
            deno: None,
            runtime: None,
            own: Default::default(),
            updates: Default::default(),
            seen: 0,
            checking: None,
            installing: None,
            installing_deno: None,
            updating: None,
            error: None,
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
        self.deno = mdm_tools::deno::installed(dir);
        self.runtime = mdm_tools::proc::system_js_runtime();
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

    /// MDM's own Deno (#1448): downloaded and checked against its release's checksums, from then
    /// on the runtime yt-dlp is told of (`mdm_tools::proc`). It goes on if the window is closed.
    fn install_deno(
        &mut self,
        dir: std::path::PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) {
        let slot: Answer = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let res = mdm_tools::deno::install_latest(&dir, &net, None).await;
            if let Err(e) = &res {
                tracing::warn!("Deno download failed: {e}");
            }
            *out.lock().unwrap() = Some(res.map(|_| ()).map_err(|e| crate::errors::tools(&e)));
            wake.request_repaint();
        });
        self.installing_deno = Some(slot);
        self.error = None;
    }

    /// MDM's own yt-dlp or gallery-dl in place of the system's (#452, #571): downloaded and
    /// checked as usual, from then on the one used (`mdm_tools::installed`,
    /// `mdm_tools::gallery::installed`). It goes on if the window is closed.
    fn install_own(
        &mut self,
        h: Helper,
        dir: std::path::PathBuf,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) {
        let slot: Answer = Arc::new(Mutex::new(None));
        let (out, wake) = (slot.clone(), ctx.clone());
        let net = crate::settings::own_net();
        rt.spawn(async move {
            let res = if h == Helper::GalleryDl {
                mdm_tools::gallery::install_latest(&dir, &net, None).await
            } else if h == Helper::Ffmpeg {
                mdm_tools::ffmpeg::install_latest(&dir, &net, None).await
            } else {
                mdm_tools::install_latest(&dir, &net, None).await
            };
            *out.lock().unwrap() = Some(res.map(|_| ()).map_err(|e| crate::errors::tools(&e)));
            wake.request_repaint();
        });
        self.installing = Some((h, slot));
        self.error = None;
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
                        // i18n: Helpers: an update check failed; {name} = yt-dlp, ffmpeg, gallery-dl or deno, {error} = why
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
        let deno = self
            .installing_deno
            .as_ref()
            .and_then(|slot| slot.lock().unwrap().take());
        if let Some(res) = deno {
            self.error = res.err();
            self.installing_deno = None;
            self.look();
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
            winui::text_font(
                &p,
                winui::tahoma_bold(),
                o.x + 14.0,
                y + 16.0,
                h.name(),
                winui::TEXT(),
            );
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
            // The system's copy may be the person's to keep up to date: MDM's own can replace it
            // (#452, #571), where MDM has one for this system and processor.
            let replaceable = has_own;
            let system = replaceable && found.is_some_and(mdm_tools::Installed::is_system);
            let status = match (too_old, found) {
                _ if downloading => {
                    // i18n: Video helpers: MDM's own yt-dlp is being downloaded
                    crate::i18n::t("Downloading MDM's own copy...").to_owned()
                }
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
            winui::fit(
                ui,
                &p,
                o.x + 14.0,
                y + 36.0,
                &status,
                winui::MUTED(),
                o.x + 486.0,
                ("helper", k),
            );
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
            // As wide as their labels in this language (#192), side by side from the left.
            let [other_at, own_at] = winui::button_row_from_left(
                &p,
                &[
                    ("Use another program...", 14.0, 180.0),
                    ("Use MDM's own copy", 202.0, 180.0),
                ],
                8.0,
                486.0,
                260.0,
            )[..] else {
                unreachable!("two buttons")
            };
            if winui::button(
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
            )
            .clicked()
            {
                // i18n: file picker title; {name} = yt-dlp or ffmpeg
                let title =
                    crate::i18n::fmt(crate::i18n::t("Choose {name}"), &[("name", h.name())]);
                winui::start_pick(ui.ctx(), &format!("helper-{}", h.name()), move || {
                    crate::winui::file_dialog(&title).pick_file()
                });
            }
            if winui::button(
                ui,
                r(o, own_at.0, y - o.y + 46.0, own_at.1, 21.0),
                // i18n: Video helpers: button: go back to the copy MDM downloads and checks itself
                "Use MDM's own copy",
                if (mine || system) && !busy {
                    ButtonKind::Normal
                } else {
                    ButtonKind::Disabled
                },
                false,
                None,
            )
            .clicked()
                && let Some(dir) = self.dir.clone()
            {
                if mine {
                    self.error = mdm_tools::chosen::forget(&dir, h)
                        .err()
                        .map(|e| crate::errors::tools(&e));
                    self.look();
                } else {
                    self.install_own(h, dir, rt, ui.ctx());
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
            // i18n: Helpers: checkbox: MDM checks for and installs newer versions of its own yt-dlp, ffmpeg, gallery-dl and Deno by itself
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
        let own: Vec<Tool> = Tool::ALL
            .into_iter()
            .filter(|t| self.own[*t as usize].current.is_some())
            .collect();
        let y = 370.0;
        // What failed, left of the buttons.
        if let Some(e) = &self.error {
            winui::fit(
                ui,
                &p,
                o.x + 14.0,
                o.y + y + 15.0,
                e,
                winui::ERROR(),
                o.x + check_at.0 - 8.0,
                "helper-error",
            );
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
                if winui::button(ui, r(o, x, y - o.y + 4.0, w, 21.0), l, kind, false, None)
                    .clicked()
                {
                    back = Some(t);
                }
            }
            if let Some(&(x, w)) = at.next() {
                right = right.min(o.x + x - 8.0);
                if winui::button(
                    ui,
                    r(o, x, y - o.y + 4.0, w, 21.0),
                    update_label,
                    kind,
                    false,
                    None,
                )
                .clicked()
                    && let Some(dir) = self.dir.clone()
                {
                    self.update(vec![t], dir, rt, ui.ctx());
                }
            }
        }
        if let Some(s) = &status {
            winui::fit(
                ui,
                p,
                from,
                y + 16.0,
                s,
                winui::MUTED(),
                right,
                ("helper-update", t as usize),
            );
        }
        if let (Some(t), Some(dir)) = (back, self.dir.clone()) {
            self.go_back(t, dir, ui.ctx());
        }
    }

    /// A check or a download is under way: the buttons wait.
    fn busy(&self) -> bool {
        self.checking.is_some()
            || self.installing.is_some()
            || self.installing_deno.is_some()
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

    /// The JavaScript runtime's row, the `k`th (#1448, #1470): YouTube makes yt-dlp solve a
    /// JavaScript challenge, and without a runtime its look-ups are slow, list few qualities or
    /// fail. The computer's own (Node.js, Deno) is said and used; else MDM's own Deno, which
    /// "Use MDM's own copy" downloads (where Deno builds one for this system and processor).
    fn runtime_row(
        &mut self,
        ui: &mut Ui,
        p: &egui::Painter,
        o: Pos2,
        k: usize,
        rt: &tokio::runtime::Handle,
    ) {
        let y = o.y + 8.0 + k as f32 * 78.0;
        // i18n: Video helpers: the row of the JavaScript runtime yt-dlp needs for YouTube (Deno is a program's name)
        let title = crate::i18n::t("JavaScript runtime (Deno)");
        winui::text_font(
            p,
            winui::tahoma_bold(),
            o.x + 14.0,
            y + 16.0,
            title,
            winui::TEXT(),
        );
        // What it is for, in one line after its name; for MDM's own copy, Update and Go back there
        // instead, and how its updates stand after its version below (the title is long in some
        // languages, the room beside it short).
        let after = winui::text_width_font(p, winui::tahoma_bold(), title) + 10.0;
        let updates = self
            .deno
            .is_some()
            .then(|| self.update_status(Tool::Deno))
            .flatten();
        if self.deno.is_some() {
            self.update_row(ui, p, o, y, o.x + 14.0 + after, Tool::Deno, None, rt);
        } else {
            winui::fit(
                ui,
                p,
                o.x + 14.0 + after,
                y + 16.0,
                // i18n: Video helpers: what the JavaScript runtime is for, after its name (yt-dlp is a program's name)
                crate::i18n::t("lets yt-dlp list all of YouTube's qualities"),
                winui::MUTED(),
                o.x + 486.0,
                ("helper-what", k),
            );
        }
        let available = mdm_tools::deno::available();
        let status = if self.installing_deno.is_some() {
            crate::i18n::t("Downloading MDM's own copy...").to_owned()
        } else if let Some(own) = &self.deno {
            let own = crate::i18n::fmt(
                crate::i18n::t("Mister Download Manager's own copy, version {version}"),
                &[("version", &own.version)],
            );
            match &updates {
                Some(u) => format!("{own} · {u}"),
                None => own,
            }
        } else if let Some(path) = &self.runtime {
            crate::i18n::fmt(
                // i18n: Video helpers: {path} = the computer's own JavaScript runtime (node.exe, deno.exe), used as it is: MDM needn't download one
                crate::i18n::t("Your system's, nothing to download: {path}"),
                &[("path", &crate::i18n::ltr(&path.display().to_string()))],
            )
        } else if available {
            crate::i18n::t("Not installed yet").to_owned()
        } else {
            // i18n: Video helpers: MDM can't download Deno on this system (32-bit Windows, an unusual processor); Deno and Node.js are program names
            crate::i18n::t("MDM has no copy for this system: install Deno or Node.js").to_owned()
        };
        winui::fit(
            ui,
            p,
            o.x + 14.0,
            y + 36.0,
            &status,
            winui::MUTED(),
            o.x + 486.0,
            ("helper", k),
        );
        let [own_at] = winui::button_row_from_left(
            p,
            &[("Use MDM's own copy", 14.0, 180.0)],
            8.0,
            486.0,
            260.0,
        )[..] else {
            unreachable!("one button")
        };
        if winui::button(
            ui,
            r(o, own_at.0, y - o.y + 46.0, own_at.1, 21.0),
            "Use MDM's own copy",
            if available && self.deno.is_none() && !self.busy() {
                ButtonKind::Normal
            } else {
                ButtonKind::Disabled
            },
            false,
            None,
        )
        .clicked()
            && let Some(dir) = self.dir.clone()
        {
            self.install_deno(dir, rt, ui.ctx());
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

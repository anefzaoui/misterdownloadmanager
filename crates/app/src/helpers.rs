//! The first start's offer of the video helpers (user request 2026-11-01): yt-dlp finds a page's
//! video, ffmpeg joins its picture and sound. Both are asked for once, explained, and downloaded
//! on Yes (through MDM's own engine, checked against their publishers' signed checksums: see
//! mdm_tools); Not now leaves them to Tasks → Download video from a page, which offers them again.

use std::sync::Arc;
use std::sync::Mutex;

use egui::{FontFamily, FontId, Pos2, Ui};

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
    /// The helper that failed, and why (the ones after it are not tried).
    error: Option<(&'static str, String)>,
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
/// counts only where MDM installs it; elsewhere the system's is used).
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
        // ffmpeg: MDM's own on Windows; elsewhere the system's.
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
                        s.error = Some((name, e.to_string()));
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
        window(
            ctx,
            "helpers",
            // i18n: title of the first start's window that offers yt-dlp and ffmpeg
            "Get ready for video downloads",
            // Taller once Yes is pressed: the progress under the last section.
            [520.0, if self.job.is_some() { 418.0 } else { 382.0 }],
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
        p.rect_filled(winui::rect(r(o, 0.0, 0.0, w, 84.0)), 0.0, winui::FIELD());
        winui::fill_l(
            &p,
            o.x,
            o.y + 84.0,
            o.x + w - 1.0,
            o.y + 84.0,
            winui::FIELD_EDGE(),
        );
        egui::Image::new(egui::include_image!("../assets/icons/app/mdm-48.png"))
            .paint_at(ui, winui::rect(r(o, 18.0, 18.0, 48.0, 48.0)));
        let title = FontId::new(16.0, FontFamily::Name("tahoma-bold".into()));
        winui::text_font(
            &p,
            title,
            o.x + 80.0,
            o.y + 36.0,
            // i18n: the headline of the first start's helpers window
            crate::i18n::t("Download videos from YouTube and 1,000+ sites"),
            ink,
        );
        winui::paragraph(
            &p,
            o.x + 80.0,
            o.y + 57.0,
            crate::i18n::t(
                "Two free helpers make it work. Get them now, and saving a video is one click away when you find one.",
            ),
            muted,
            o.x + w - 18.0,
            14.0,
        );

        let ffmpeg_here = !cfg!(windows)
            && self
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
                .paint_at(ui, winui::rect(r(o, 22.0, y + 2.0, 32.0, 32.0)));
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
            let (fraction, line, color) = if let Some((name, error)) = &st.error {
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
                            ("n", &k.to_string()),
                            ("total", &total.to_string()),
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
                            ("n", &k.to_string()),
                            ("total", &total.to_string()),
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
            winui::meter(
                &p,
                winui::rect(r(o, (w - bar_w) / 2.0, 312.0, bar_w, 8.0)),
                fraction,
            );
            // Centred on screen (the same place in both directions); cut to the window.
            let room = w - 40.0;
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
            y += 36.0;
        }

        // The way back later, then the answer.
        winui::etched_h(&p, o.x + 16.0, o.y + y, w - 32.0);
        let later = if self.job.is_some() && running {
            crate::i18n::t("They keep downloading if you close this window.")
        } else if self.job.is_some() && !failed {
            crate::i18n::t(
                "All set: on a page with a video, use Tasks → Download video from a page.",
            )
        } else if self.job.is_some() {
            crate::i18n::t(
                "Check your connection, then try again, or get them later from the video window.",
            )
        } else {
            crate::i18n::t("Not now? Get them later from Tasks → Download video from a page.")
        };
        winui::text(&p, o.x + 16.0, o.y + y + 20.0, later, muted);
        let y = y + 28.0;
        if self.job.is_none() {
            let not_now = winui::button(
                ui,
                r(o, 196.0, y, 90.0, 25.0),
                "Not now",
                ButtonKind::Normal,
                false,
                None,
            );
            // i18n: the first start's helpers window: download yt-dlp and ffmpeg (the recommended answer)
            let yes = winui::button(
                ui,
                r(o, 292.0, y, 212.0, 25.0),
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
            if failed && !running {
                let again = winui::button(
                    ui,
                    r(o, 318.0, y, 90.0, 25.0),
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
                r(o, 414.0, y, 90.0, 25.0),
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

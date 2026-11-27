//! The offer of Mesa's software renderer, llvmpipe (#476), on Windows: where no graphics card has
//! a driver MDM can draw with, Windows' own software renderer (WARP) draws, and keeps the
//! processor far busier than llvmpipe does for the same windows. MDM doesn't ship llvmpipe; this
//! window offers it, with its size, and downloads it only on Download (the owner's rule: nothing
//! downloads without a click). It comes once at a start that draws with WARP, and from View →
//! Draw with at any time; it is installed by `mdm_tools::mesa` (checked against MDM's signed
//! list), and draws from MDM's next start (renderer.rs).
//!
//! The start's offer first reads that signed list (two small files, for the download's size), and
//! only while the automatic update check is on, as the update check reads MDM's releases; with it
//! off, only View → Draw with offers it. Linux and macOS have their own software OpenGL: this
//! module is Windows' only.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use egui::Pos2;

use crate::dialogs::{Outcome, question};
use crate::i18n::{fmt, t};
use crate::winui::{self, ButtonKind, r};

type Slot<T> = Arc<Mutex<Option<T>>>;

/// The window's client width; its height follows its text.
const W: f32 = 460.0;

/// Windows' software renderer draws MDM now: Direct3D 12 on WARP.
pub fn warp_draws() -> bool {
    crate::renderer::active() == Some(crate::renderer::Api::Direct3D) && crate::renderer::software()
}

/// Is the start's offer worth making: WARP draws, llvmpipe isn't downloaded, the offer wasn't
/// made before, and the automatic update check may read MDM's releases.
pub fn worth_offering(settings: &crate::settings::Settings) -> bool {
    warp_draws()
        && crate::renderer::mesa_dll().is_none()
        && !settings.software_renderer_asked
        && settings.update_check
}

/// What MDM's signed list offers for this computer: Mesa's version and the download's size.
#[derive(Clone, Debug)]
pub struct Offered {
    version: String,
    size: u64,
}

/// The list being read in the background: then the offer, or why there is none (in the
/// interface's words).
pub type Lookup = Slot<Result<Offered, String>>;

/// The list read in the background ([`Lookup`]).
pub fn look_up(rt: &tokio::runtime::Handle, ctx: &egui::Context) -> Lookup {
    let slot: Slot<Result<Offered, String>> = Arc::new(Mutex::new(None));
    let (out, ctx) = (slot.clone(), ctx.clone());
    let net = crate::settings::own_net();
    rt.spawn(async move {
        let got = match mdm_tools::tools_dir() {
            Some(dir) => mdm_tools::mesa::offer(&dir, &net)
                .await
                .map(|b| Offered {
                    version: b.version.clone(),
                    size: b.size,
                })
                .map_err(|e| {
                    tracing::info!("Mesa's software renderer isn't offered: {e}");
                    crate::errors::tools(&e)
                }),
            None => Err(t("MDM has no data folder").to_owned()),
        };
        if let Ok(mut s) = out.lock() {
            *s = Some(got);
        }
        ctx.request_repaint();
    });
    slot
}

enum State {
    /// Reading the list (View → Draw with's offer).
    Looking(Lookup),
    Offered(Offered),
    Downloading {
        offered: Offered,
        progress: Arc<mdm_tools::update::Progress>,
        slot: Slot<Result<(), String>>,
        task: tokio::task::JoinHandle<()>,
    },
    Done,
    Failed(String),
}

/// The window.
pub struct Offer {
    state: State,
    rt: tokio::runtime::Handle,
}

impl Drop for Offer {
    /// Closed while downloading (Cancel, or MDM closing): the download stops; its part stays
    /// beside the install, and a later Download goes on from it.
    fn drop(&mut self) {
        if let State::Downloading { task, .. } = &self.state {
            task.abort();
        }
    }
}

impl Offer {
    /// From View → Draw with: the list is read first.
    pub fn looking(rt: &tokio::runtime::Handle, ctx: &egui::Context) -> Self {
        Offer {
            state: State::Looking(look_up(rt, ctx)),
            rt: rt.clone(),
        }
    }

    /// At a start: the list was read already.
    pub fn offered(rt: &tokio::runtime::Handle, offered: Offered) -> Self {
        Offer {
            state: State::Offered(offered),
            rt: rt.clone(),
        }
    }

    fn download(&mut self, offered: Offered, ctx: &egui::Context) {
        let progress = Arc::new(mdm_tools::update::Progress::default());
        let slot: Slot<Result<(), String>> = Arc::new(Mutex::new(None));
        let (out, counted, ctx2) = (slot.clone(), progress.clone(), ctx.clone());
        let net = crate::settings::own_net();
        tracing::info!(
            "Mesa {} ({} bytes): downloading, as asked",
            offered.version,
            offered.size
        );
        let task = self.rt.spawn(async move {
            let got = match mdm_tools::tools_dir() {
                Some(dir) => mdm_tools::mesa::install(&dir, &net, Some(&*counted))
                    .await
                    .map(|i| {
                        tracing::info!("Mesa {} installed: {}", i.version, i.exe.display());
                    })
                    .map_err(|e| {
                        tracing::warn!("Mesa's software renderer wasn't installed: {e}");
                        crate::errors::tools(&e)
                    }),
                None => Err(t("MDM has no data folder").to_owned()),
            };
            if let Ok(mut s) = out.lock() {
                *s = Some(got);
            }
            ctx2.request_repaint();
        });
        self.state = State::Downloading {
            offered,
            progress,
            slot,
            task,
        };
    }

    /// `Done(true)`: installed (MDM draws with it from its next start); `Closed`: not now.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<bool> {
        // What came back from the background since the last frame.
        let next = match &self.state {
            State::Looking(slot) => {
                slot.lock()
                    .ok()
                    .and_then(|mut s| s.take())
                    .map(|got| match got {
                        Ok(o) => State::Offered(o),
                        Err(why) => State::Failed(why),
                    })
            }
            State::Downloading { slot, .. } => {
                slot.lock()
                    .ok()
                    .and_then(|mut s| s.take())
                    .map(|got| match got {
                        Ok(()) => {
                            // Another llvmpipe's failure here (if any) is forgotten: this one is tried.
                            crate::renderer::forget_llvmpipe_failure();
                            State::Done
                        }
                        Err(why) => State::Failed(why),
                    })
            }
            _ => None,
        };
        if let Some(n) = next {
            self.state = n;
        }
        let size = |b: u64| crate::i18n::ltr(&crate::model::size(b));
        let (text, busy) = match &self.state {
            // i18n: the software renderer's window: while MDM reads its signed list of the software renderers it may download (Mesa's llvmpipe: it draws MDM's windows where there is no graphics card)
            State::Looking(_) => (t("Looking up Mesa's software renderer...").to_owned(), None),
            State::Offered(o) => (
                fmt(
                    // i18n: the software renderer's window, offered once at a start where Windows' own software renderer (WARP, keep the name) draws MDM because no graphics card has a driver for it; llvmpipe (keep the name) is Mesa's software renderer; {size} = its download's size (e.g. 66.3 MB)
                    t(
                        "No graphics card here has a driver MDM can draw with, so Windows' software renderer (WARP) draws its windows and keeps the processor busy. Mesa's software renderer (llvmpipe) draws them with far less. Download it once ({size}, from Mesa's builds on GitHub)? MDM draws with it from its next start.",
                    ),
                    &[("size", &size(o.size))],
                ),
                None,
            ),
            State::Downloading {
                offered, progress, ..
            } => {
                let got = progress.got.load(Ordering::Relaxed);
                let total = progress.total.load(Ordering::Relaxed).max(offered.size);
                let text = if got >= total && total > 0 {
                    // i18n: the software renderer's window: the download is done, and its files are being checked and unpacked
                    t("Checking and unpacking Mesa's software renderer...").to_owned()
                } else {
                    fmt(
                        // i18n: the software renderer's window, while it downloads; {done} and {size} = sizes (12.0 MB of 66.3 MB)
                        t("Downloading Mesa's software renderer: {done} of {size}"),
                        &[("done", &size(got)), ("size", &size(total))],
                    )
                };
                (text, Some(progress.fraction()))
            }
            State::Done => (
                // i18n: the software renderer's window, once Mesa's llvmpipe is downloaded and checked
                t("Mesa's software renderer is installed. MDM draws with it from its next start.")
                    .to_owned(),
                None,
            ),
            State::Failed(why) => (
                fmt(
                    // i18n: the software renderer's window: the download or its check failed; {reason} = why, a sentence's part
                    t("Mesa's software renderer couldn't be installed: {reason}"),
                    &[("reason", why)],
                ),
                None,
            ),
        };
        let id = "software-renderer";
        let extra = crate::dialogs::grow_extra(ctx, id);
        let bar = if busy.is_some() { 22.0 } else { 0.0 };
        let state = &self.state;
        let mut pressed = None;
        let out: Outcome<()> = question(
            ctx,
            id,
            // i18n: the window's title: Mesa's software renderer, offered where no graphics card draws MDM's windows
            "Software renderer",
            [W, 96.0 + bar + extra],
            false,
            winui::BG(),
            |ui, o: Pos2| {
                let p = ui.painter().clone();
                let lines = winui::paragraph(
                    &p,
                    o.x + 12.0,
                    o.y + 22.0,
                    &text,
                    winui::TEXT(),
                    o.x + W - 12.0,
                    13.0,
                );
                crate::dialogs::grow_to(ui.ctx(), id, lines, 3);
                let o = o + egui::vec2(0.0, extra);
                if let Some(fraction) = busy {
                    let time = ui.input(|i| i.time);
                    winui::progress_bar(&p, r(o, 12.0, 62.0, W - 24.0, 14.0), fraction, time);
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));
                }
                let y = 64.0 + bar;
                let right = W - 12.0;
                match state {
                    State::Offered(_) => {
                        let at = winui::buttons_to_right(
                            &p,
                            right,
                            8.0,
                            &[("Download", 80.0), ("Not now", 80.0)],
                            200.0,
                        );
                        if winui::button(
                            ui,
                            r(o, at[0].0, y, at[0].1, 23.0),
                            "Download",
                            ButtonKind::Default,
                            false,
                            None,
                        )
                        .clicked()
                            || winui::enter_pressed(ui)
                        {
                            pressed = Some(true);
                        }
                        if winui::button(
                            ui,
                            r(o, at[1].0, y, at[1].1, 23.0),
                            "Not now",
                            ButtonKind::Normal,
                            false,
                            None,
                        )
                        .clicked()
                            || winui::escape_pressed(ui)
                        {
                            pressed = Some(false);
                        }
                    }
                    State::Looking(_) | State::Downloading { .. } => {
                        let at =
                            winui::buttons_to_right(&p, right, 8.0, &[("Cancel", 80.0)], 200.0);
                        if winui::button(
                            ui,
                            r(o, at[0].0, y, at[0].1, 23.0),
                            "Cancel",
                            ButtonKind::Normal,
                            false,
                            None,
                        )
                        .clicked()
                            || winui::escape_pressed(ui)
                        {
                            pressed = Some(false);
                        }
                    }
                    State::Done | State::Failed(_) => {
                        let at = winui::buttons_to_right(&p, right, 8.0, &[("OK", 80.0)], 200.0);
                        if winui::button(
                            ui,
                            r(o, at[0].0, y, at[0].1, 23.0),
                            "OK",
                            ButtonKind::Default,
                            false,
                            None,
                        )
                        .clicked()
                            || winui::enter_pressed(ui)
                            || winui::escape_pressed(ui)
                        {
                            pressed = Some(false);
                        }
                    }
                }
                Outcome::Open
            },
        );
        if !matches!(out, Outcome::Open) {
            // Closed by its title bar's button.
            return Outcome::Closed;
        }
        match (pressed, &self.state) {
            (Some(true), State::Offered(o)) => {
                let o = o.clone();
                self.download(o, ctx);
                Outcome::Open
            }
            (Some(_), State::Done) => Outcome::Done(true),
            (Some(_), _) => Outcome::Closed,
            (None, _) => Outcome::Open,
        }
    }
}

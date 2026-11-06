//! What draws the windows (#89). On Windows, Direct3D 12 (eframe's wgpu with wgpu's Direct3D 12
//! backend only) by default, or OpenGL (eframe's glow) when chosen in View → Draw with, from the
//! next start. Linux and macOS draw with OpenGL only: wgpu isn't built there (its Vulkan and
//! Metal backends would add size for windows OpenGL already draws well).
//!
//! The windows always open: when the chosen way can't start (no Direct3D 12 driver, a virtual
//! machine, Remote Desktop), the other is tried, and the reason is logged. A start with Direct3D
//! that ends before its first frame (a crash or a kill inside the driver) is remembered, and so
//! is one whose first frame doesn't come within [`FIRST_FRAME_WITHIN`] (MDM starts again, with
//! OpenGL): the next starts draw with OpenGL until Direct3D is chosen again. A graphics card
//! lost while MDM runs (a driver update or reset) starts MDM again; a second loss soon after
//! counts as Direct3D failing.
//!
//! Testing overrides, in this order: `/renderer <name>` on the command line, then
//! `MDM_RENDERER=<name>`. Names: `opengl` (`gl`), `direct3d` (`d3d`), `warp` (Direct3D drawn by
//! Microsoft's software renderer, as on a machine without a graphics driver).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// A choice of View → Draw with, as saved in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Api {
    #[serde(rename = "opengl")]
    OpenGl,
    #[serde(rename = "direct3d")]
    Direct3D,
}

/// What draws when the user chose nothing (the settings' `renderer: None`): Direct3D on Windows
/// (#89), OpenGL elsewhere (the only one built there).
pub const DEFAULT: Api = if cfg!(windows) {
    Api::Direct3D
} else {
    Api::OpenGl
};

/// How long a watched Direct3D start may take to its first frame before MDM starts again with
/// OpenGL (a driver stuck in its first present, a window that never shows). Generous: a slow
/// computer just after it starts (#57) can take seconds.
#[cfg(windows)]
const FIRST_FRAME_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// Which graphics adapters a Direct3D start may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapters {
    /// A graphics card's driver only.
    Hardware,
    /// Microsoft's software renderer (WARP) only.
    Software,
    /// A card first, else WARP: the last way to open the windows at all.
    Any,
    /// None at all: Direct3D fails to start (`MDM_RENDERER=direct3d-none` in off-screen test
    /// copies, to check the way back to OpenGL).
    Nothing,
}

/// One way to start drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attempt {
    OpenGl,
    Direct3D(Adapters),
}

impl Attempt {
    pub fn api(self) -> Api {
        match self {
            Attempt::OpenGl => Api::OpenGl,
            Attempt::Direct3D(_) => Api::Direct3D,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Attempt::OpenGl => "OpenGL",
            Attempt::Direct3D(Adapters::Software) => "Direct3D 12 (WARP)",
            Attempt::Direct3D(_) => "Direct3D 12",
        }
    }
}

/// A testing override's name.
fn parse(name: &str) -> Option<Attempt> {
    match name.trim().to_ascii_lowercase().as_str() {
        "opengl" | "gl" | "glow" => Some(Attempt::OpenGl),
        "direct3d" | "d3d" | "d3d12" | "dx12" | "wgpu" => {
            Some(Attempt::Direct3D(Adapters::Hardware))
        }
        "warp" => Some(Attempt::Direct3D(Adapters::Software)),
        "direct3d-none" if crate::offscreen() => Some(Attempt::Direct3D(Adapters::Nothing)),
        _ => None,
    }
}

/// The ways to try, in order, and whether the first is the user's Direct3D (whose start is
/// watched: see [`begin`]).
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub attempts: Vec<Attempt>,
    pub watched: bool,
}

/// What to try: the override if any, else the setting (or [`DEFAULT`]) unless Direct3D failed
/// here before; then the other way. A graphics card's Direct3D falls back to OpenGL, then to
/// Direct3D on Microsoft's software renderer (Remote Desktop, a virtual machine: no card's
/// driver, and Windows' own OpenGL 1.1 can't draw MDM). Only OpenGL where Direct3D isn't built.
pub fn plan(setting: Option<Api>, overridden: Option<&str>, failed_before: bool) -> Plan {
    let forced = overridden.and_then(|o| {
        let a = parse(o);
        if a.is_none() {
            tracing::warn!("unknown renderer {o:?} (opengl, direct3d, warp): not used");
        }
        a
    });
    let chosen = setting.unwrap_or(DEFAULT);
    let first = match forced {
        Some(a) => a,
        None if chosen == Api::Direct3D && !failed_before => Attempt::Direct3D(Adapters::Hardware),
        None => Attempt::OpenGl,
    };
    let then: &[Attempt] = match first {
        Attempt::OpenGl => &[Attempt::Direct3D(Adapters::Any)],
        Attempt::Direct3D(Adapters::Hardware) if forced.is_none() => {
            &[Attempt::OpenGl, Attempt::Direct3D(Adapters::Software)]
        }
        Attempt::Direct3D(_) => &[Attempt::OpenGl],
    };
    let attempts: Vec<Attempt> = std::iter::once(first)
        .chain(then.iter().copied())
        .filter(|a| cfg!(windows) || *a == Attempt::OpenGl)
        .collect();
    Plan {
        watched: forced.is_none()
            && attempts.first() == Some(&Attempt::Direct3D(Adapters::Hardware)),
        attempts,
    }
}

/// The testing override: `/renderer <name>` (or `--renderer`) among the arguments, else
/// `MDM_RENDERER`.
pub fn overridden(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| {
            a.trim_start_matches(['/', '-'])
                .eq_ignore_ascii_case("renderer")
        })
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("MDM_RENDERER").ok())
        .filter(|s| !s.trim().is_empty())
}

// ---- Remembering a Direct3D start that never drew ------------------------------------------

/// Written before a watched Direct3D start, removed once its first frame is drawn.
const STARTING: &str = "direct3d-starting";
/// Why Direct3D couldn't draw here; while it is there, OpenGL draws.
const FAILED: &str = "direct3d-failed.txt";

/// Direct3D failed here (before this start, or in it) and wasn't chosen again since.
static DIRECT3D_FAILED: AtomicBool = AtomicBool::new(false);

/// For View → Draw with's note (Windows: the menu is there only).
#[cfg(windows)]
pub fn direct3d_failed() -> bool {
    DIRECT3D_FAILED.load(Ordering::Relaxed)
}

fn file(name: &str) -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join(name))
}

/// Why Direct3D failed here before, if it did. A start that left its marker behind ended before
/// its first frame: that becomes the reason.
pub fn failed_before() -> Option<String> {
    let (starting, failed) = (file(STARTING)?, file(FAILED)?);
    if starting.exists() {
        let why = "the last start with Direct3D ended before its first frame";
        tracing::warn!("Direct3D: {why}; drawing with OpenGL until it is chosen again");
        let _ = std::fs::write(&failed, why);
        let _ = std::fs::remove_file(&starting);
    }
    let why = std::fs::read_to_string(&failed)
        .ok()
        .map(|s| s.trim().to_owned());
    if let Some(why) = &why {
        tracing::info!("Direct3D failed here before ({why})");
        DIRECT3D_FAILED.store(true, Ordering::Relaxed);
    }
    why
}

/// Has Direct3D failed here, or is a start with it not yet drawn? Only looks (the crash monitor:
/// its report may be about that start).
pub fn suspect() -> bool {
    [STARTING, FAILED]
        .into_iter()
        .filter_map(file)
        .any(|f| f.exists())
}

/// Direct3D could not start: remembered with its reason.
fn remember_failure(why: &str) {
    if let Some(f) = file(FAILED) {
        let _ = std::fs::write(f, why);
    }
}

/// Direct3D chosen again (View → Draw with): its next start is tried.
#[cfg(windows)]
pub fn forget_failure() {
    DIRECT3D_FAILED.store(false, Ordering::Relaxed);
    for name in [FAILED, STARTING] {
        if let Some(f) = file(name) {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// A test's failure asked for (`name`=1), honoured only by off-screen test copies.
fn testing(name: &str) -> bool {
    crate::offscreen() && std::env::var_os(name).is_some_and(|v| v == "1")
}

/// The start of every main-window pass: Direct3D's device checked (see `direct3d::poll`).
pub fn frame_begin() {
    #[cfg(windows)]
    direct3d::poll();
}

/// The first frame was drawn (presented).
static DRAWN: AtomicBool = AtomicBool::new(false);

/// The end of every main-window pass. Once a frame has been completed before this one (egui's
/// frame count, not its passes: a first frame can run two), the first was drawn: a watched
/// Direct3D start made it.
pub fn frame_done(ctx: &egui::Context) {
    if DRAWN.load(Ordering::Relaxed) || ctx.cumulative_frame_nr() == 0 {
        return;
    }
    // MDM_TEST_NO_FIRST_FRAME=1 (off-screen test copies): a first frame that never comes (the
    // watchdog's check).
    if testing("MDM_TEST_NO_FIRST_FRAME") {
        return;
    }
    DRAWN.store(true, Ordering::Relaxed);
    tracing::info!("first frame drawn, {} ms after start", since_start());
    if let Some(f) = file(STARTING) {
        let _ = std::fs::remove_file(f);
    }
}

// ---- What draws now ---------------------------------------------------------------------------

static START: OnceLock<std::time::Instant> = OnceLock::new();

/// The program started (first thing in `main`).
pub fn mark_start() {
    let _ = START.set(std::time::Instant::now());
}

fn since_start() -> u128 {
    START.get().map_or(0, |s| s.elapsed().as_millis())
}

/// What draws now (set once the window started), for View → Draw with.
static ACTIVE: OnceLock<Api> = OnceLock::new();

#[cfg(windows)]
pub fn active() -> Option<Api> {
    ACTIVE.get().copied()
}

/// Run eframe's window with each way of the plan until one starts. `options` makes the window's
/// options (each try needs its own); `creator` makes the app, once (a try that fails before
/// that leaves it for the next). Returns the last try's result.
pub fn run<A: eframe::App + 'static>(
    name: &str,
    plan: &Plan,
    options: impl Fn() -> eframe::NativeOptions,
    creator: impl FnOnce(&eframe::CreationContext<'_>) -> A,
) -> eframe::Result {
    let creator = std::rc::Rc::new(std::cell::RefCell::new(Some(creator)));
    let mut result = Ok(());
    let mut instead: Option<String> = None;
    for (n, &attempt) in plan.attempts.iter().enumerate() {
        if n == 0
            && plan.watched
            && let Some(f) = file(STARTING)
        {
            let _ = std::fs::write(f, "");
        }
        // A watched Direct3D start must show its first frame in time, or MDM starts again.
        #[cfg(windows)]
        if n == 0 && plan.watched {
            direct3d::watch_first_frame();
        }
        #[cfg(windows)]
        direct3d::TRYING.store(attempt.api() == Api::Direct3D, Ordering::Relaxed);
        let mut opts = options();
        configure(&mut opts, attempt);
        tracing::info!("drawing with {}", attempt.name());
        let make = creator.clone();
        let why_not = instead.clone();
        result = eframe::run_native(
            name,
            opts,
            Box::new(move |cc| {
                let make = make.borrow_mut().take().ok_or("the app was already made")?;
                started(cc, attempt, why_not);
                Ok(Box::new(make(cc)))
            }),
        );
        // A failure after the app was made is the app's own (or a later one): no retry.
        let Err(e) = &result else { break };
        if creator.borrow().is_none() {
            break;
        }
        let why = format!("{} didn't start: {e}", attempt.name());
        tracing::warn!("{why}");
        if attempt.api() == Api::Direct3D {
            DIRECT3D_FAILED.store(true, Ordering::Relaxed);
        }
        if attempt.api() == Api::Direct3D && n == 0 && plan.watched {
            remember_failure(&format!("{e}"));
            if let Some(f) = file(STARTING) {
                let _ = std::fs::remove_file(f);
            }
        }
        instead = Some(why);
    }
    result
}

/// The window's options for one way of drawing.
fn configure(opts: &mut eframe::NativeOptions, attempt: Attempt) {
    match attempt {
        Attempt::OpenGl => {
            opts.renderer = eframe::Renderer::Glow;
            // MDM_TEST_NO_OPENGL=1 (off-screen test copies): a pixel format no OpenGL has, as on
            // a computer whose only OpenGL is Windows' own 1.1 (Remote Desktop, a VM).
            if testing("MDM_TEST_NO_OPENGL") {
                opts.multisampling = 255;
                opts.depth_buffer = 255;
                opts.stencil_buffer = 255;
            }
        }
        #[cfg(windows)]
        Attempt::Direct3D(adapters) => {
            opts.renderer = eframe::Renderer::Wgpu;
            opts.wgpu_options = direct3d::options(adapters);
        }
        // Not built here (plan() never asks for it).
        #[cfg(not(windows))]
        Attempt::Direct3D(_) => opts.renderer = eframe::Renderer::Glow,
    }
}

/// The app is being made: what draws is known.
fn started(cc: &eframe::CreationContext<'_>, attempt: Attempt, instead: Option<String>) {
    #[cfg(windows)]
    let detail = match &cc.wgpu_render_state {
        Some(rs) => direct3d::describe(&rs.adapter.get_info()),
        None => attempt.name().to_owned(),
    };
    #[cfg(not(windows))]
    let detail = {
        let _ = cc;
        attempt.name().to_owned()
    };
    tracing::info!(
        "drawing with {detail}, {} ms after start{}",
        since_start(),
        instead
            .as_deref()
            .map(|w| format!(" ({w})"))
            .unwrap_or_default()
    );
    // The crash reports say it ("Direct3D 12 on Intel(R) UHD Graphics 620 (driver …)").
    crate::crash::renderer(&detail);
    // Errors are logged, not panics, and a lost graphics card starts MDM again.
    #[cfg(windows)]
    if let Some(rs) = &cc.wgpu_render_state {
        direct3d::watch_device(&rs.device);
    }
    let _ = ACTIVE.set(attempt.api());
}

/// `/show renderer-lost` (off-screen test copies): Direct3D's device lost on purpose (MDM starts
/// again).
#[cfg(windows)]
pub fn test_device_lost() {
    direct3d::lose_device();
}

/// eframe's wgpu set up for Direct3D 12 only.
#[cfg(windows)]
mod direct3d {
    use super::{Adapters, DRAWN, FIRST_FRAME_WITHIN, file, remember_failure};
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant, SystemTime};

    /// The try under way is Direct3D's (a failed one goes on to OpenGL in the same process).
    pub static TRYING: AtomicBool = AtomicBool::new(false);

    /// The first frame of this Direct3D start must come within [`FIRST_FRAME_WITHIN`]; if it
    /// doesn't (a driver stuck in its first present, a window left black), Direct3D is
    /// remembered as failed and MDM starts again, drawing with OpenGL.
    pub fn watch_first_frame() {
        let _ = std::thread::Builder::new()
            .name("first frame watch".into())
            .spawn(|| {
                let until = Instant::now() + FIRST_FRAME_WITHIN;
                while Instant::now() < until {
                    if DRAWN.load(Ordering::Relaxed) || !TRYING.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                if DRAWN.load(Ordering::Relaxed) || !TRYING.load(Ordering::Relaxed) {
                    return;
                }
                let why = format!(
                    "Direct3D drew no first frame within {} s",
                    FIRST_FRAME_WITHIN.as_secs()
                );
                remember_failure(&why);
                // Remembered as this reason, not as a start that ended before its first frame.
                if let Some(f) = file(super::STARTING) {
                    let _ = std::fs::remove_file(f);
                }
                start_again(&why);
            });
    }

    /// When the graphics card is lost twice this close together, Direct3D counts as failing.
    const LOST_AGAIN_WITHIN: Duration = Duration::from_secs(600);
    /// When the card was last lost (its time inside).
    const LOST: &str = "direct3d-lost";

    /// wgpu's errors go to the log instead of ending MDM (its default handler panics), and a
    /// device lost while MDM runs (a driver update or reset: Direct3D 12 doesn't recover) starts
    /// MDM again, with Direct3D, or with OpenGL after a second loss within 10 minutes.
    pub fn watch_device(device: &wgpu::Device) {
        device.on_uncaptured_error(Arc::new(|e| {
            tracing::error!("Direct3D: {e}");
        }));
        device.set_device_lost_callback(|reason, message| {
            let testing = TEST_LOSS.load(Ordering::Relaxed);
            if reason != wgpu::DeviceLostReason::Unknown && !testing {
                return;
            }
            let why = format!("the graphics card was lost ({reason:?}: {message})");
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let last = file(LOST)
                .and_then(|f| std::fs::read_to_string(f).ok())
                .and_then(|t| t.trim().parse::<u64>().ok());
            if last.is_some_and(|t| now.saturating_sub(t) < LOST_AGAIN_WITHIN.as_secs()) {
                remember_failure(&format!("{why}, twice within 10 minutes"));
            } else if let Some(f) = file(LOST) {
                let _ = std::fs::write(f, now.to_string());
            }
            start_again(&why);
        });
        let _ = DEVICE.set(device.clone());
    }

    static DEVICE: std::sync::OnceLock<wgpu::Device> = std::sync::OnceLock::new();

    /// Each main-window pass, before its frame is painted: wgpu notices a lost device here and
    /// calls [`watch_device`]'s callback, before painting with it would fail.
    pub fn poll() {
        if let Some(d) = DEVICE.get() {
            let _ = d.poll(wgpu::PollType::Poll);
        }
    }
    static TEST_LOSS: AtomicBool = AtomicBool::new(false);

    /// `/show renderer-lost` (off-screen test copies): lose the device on purpose.
    pub fn lose_device() {
        if let Some(d) = DEVICE.get() {
            TEST_LOSS.store(true, Ordering::Relaxed);
            d.destroy();
            poll();
        }
    }

    /// End this copy and start MDM again: the new one waits for this one to be gone (as after
    /// an update), and draws as the failure markers now say.
    fn start_again(why: &str) {
        tracing::warn!("{why}: starting MDM again");
        let started = std::env::current_exe().ok().and_then(|exe| {
            std::process::Command::new(exe)
                .arg(crate::update::UPDATED)
                // An override would bring the same failure back.
                .env_remove("MDM_RENDERER")
                .spawn()
                .ok()
        });
        if started.is_none() {
            tracing::error!("MDM couldn't start again");
        }
        crate::crash::clean_exit();
        // The window thread may be stuck in the driver: end at once, without unwinding.
        crate::win32::end_process(std::process::id());
        std::process::exit(1);
    }

    pub fn options(adapters: Adapters) -> WgpuConfiguration {
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        let desc = &mut setup.instance_descriptor;
        // Direct3D 12 only, and nothing from WGPU_* variables: what draws is MDM's choice.
        desc.backends = wgpu::Backends::DX12;
        desc.flags = wgpu::InstanceFlags::from_build_config();
        desc.backend_options = wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                // Windows' own compiler (d3dcompiler_47.dll): no dxcompiler.dll looked for on
                // the PATH, where any program could have put one.
                shader_compiler: wgpu::Dx12Compiler::Fxc,
                ..Default::default()
            },
            ..Default::default()
        };
        setup.power_preference = wgpu::PowerPreference::LowPower;
        setup.native_adapter_selector = Some(Arc::new(move |list, surface| {
            let kinds: Vec<(wgpu::DeviceType, bool)> = list
                .iter()
                .map(|a| {
                    let fits = surface.is_none_or(|s| a.is_surface_supported(s));
                    (a.get_info().device_type, fits)
                })
                .collect();
            match super::choose(&kinds, adapters) {
                Some(i) => Ok(list[i].clone()),
                None => Err(format!(
                    "no {} Direct3D 12 adapter among [{}]",
                    match adapters {
                        Adapters::Hardware => "hardware",
                        Adapters::Software => "software (WARP)",
                        Adapters::Any => "usable",
                        Adapters::Nothing => "(none wanted)",
                    },
                    list.iter()
                        .map(|a| describe(&a.get_info()))
                        .collect::<Vec<_>>()
                        .join("; ")
                )),
            }
        }));
        setup.device_descriptor = Arc::new(|_adapter| wgpu::DeviceDescriptor {
            label: Some("MDM"),
            required_limits: wgpu::Limits {
                // As eframe asks: windows as large as 4K and more.
                max_texture_dimension_2d: 8192,
                // Direct3D 12 keeps one descriptor heap this large, in the graphics card's memory
                // and a copy in MDM's: wgpu's default (a million, 32 MB each) is for games. egui
                // uses one per texture (icons, fonts, file icons): a few hundred.
                max_non_sampler_bindings: 65_536,
                ..wgpu::Limits::default()
            },
            // Small memory blocks: a download manager's windows need little, and it often sits
            // in the tray.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        });
        WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..Default::default()
        }
    }

    pub fn describe(info: &wgpu::AdapterInfo) -> String {
        let mut s = format!("Direct3D 12 on {}", info.name);
        let driver = [&info.driver, &info.driver_info]
            .into_iter()
            .filter(|d| !d.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if !driver.is_empty() {
            s.push_str(&format!(" (driver {driver})"));
        }
        if info.device_type == wgpu::DeviceType::Cpu {
            s.push_str(" [software]");
        }
        s
    }
}

/// The adapter to draw with, among (kind, can draw to the window) in the system's order (the
/// one showing the main screen first): the first fitting one of the kinds `adapters` allows.
#[cfg(windows)]
fn choose(list: &[(eframe::wgpu::DeviceType, bool)], adapters: Adapters) -> Option<usize> {
    use eframe::wgpu::DeviceType;
    // MDM_TEST_NO_HARDWARE=1 (off-screen test copies): no graphics card's Direct3D 12, only
    // Microsoft's software renderer, as over Remote Desktop.
    let no_card = testing("MDM_TEST_NO_HARDWARE");
    let hardware = |t: DeviceType| t != DeviceType::Cpu && !no_card;
    let software = |t: DeviceType| t == DeviceType::Cpu;
    let first =
        |want: &dyn Fn(DeviceType) -> bool| list.iter().position(|&(t, fits)| fits && want(t));
    match adapters {
        Adapters::Hardware => first(&hardware),
        Adapters::Software => first(&software),
        Adapters::Any => first(&hardware).or_else(|| first(&software)),
        Adapters::Nothing => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_or_an_override_then_the_other_way() {
        let d3d = Attempt::Direct3D(Adapters::Hardware);
        let any = Attempt::Direct3D(Adapters::Any);
        if cfg!(windows) {
            // Nothing chosen: Direct3D (#89), its start watched; OpenGL behind it, then WARP.
            let warp = Attempt::Direct3D(Adapters::Software);
            let p = plan(None, None, false);
            assert_eq!(p.attempts, [d3d, Attempt::OpenGl, warp]);
            assert!(p.watched);
            // Direct3D chosen: the same.
            let p = plan(Some(Api::Direct3D), None, false);
            assert_eq!(p.attempts, [d3d, Attempt::OpenGl, warp]);
            assert!(p.watched);
            // ... unless it failed here before, chosen or by default.
            for setting in [None, Some(Api::Direct3D)] {
                let p = plan(setting, None, true);
                assert_eq!(p.attempts, [Attempt::OpenGl, any]);
                assert!(!p.watched);
            }
            // OpenGL chosen: kept, Direct3D only if OpenGL can't start.
            let p = plan(Some(Api::OpenGl), None, false);
            assert_eq!(p.attempts, [Attempt::OpenGl, any]);
            assert!(!p.watched);
            // Overrides win, failure or not, and aren't watched.
            let p = plan(Some(Api::OpenGl), Some("D3D"), true);
            assert_eq!(p.attempts, [d3d, Attempt::OpenGl]);
            assert!(!p.watched);
            let p = plan(Some(Api::Direct3D), Some("gl"), false);
            assert_eq!(p.attempts, [Attempt::OpenGl, any]);
            let p = plan(None, Some("warp"), false);
            assert_eq!(
                p.attempts,
                [Attempt::Direct3D(Adapters::Software), Attempt::OpenGl]
            );
            // An unknown name is ignored.
            assert_eq!(plan(None, Some("vulkan"), false).attempts[0], d3d);
        } else {
            // Direct3D isn't built: OpenGL whatever was asked.
            for p in [
                plan(None, None, false),
                plan(Some(Api::Direct3D), None, false),
                plan(None, Some("d3d"), false),
            ] {
                assert_eq!(p.attempts, [Attempt::OpenGl]);
                assert!(!p.watched);
            }
        }
    }

    #[test]
    fn override_from_the_command_line() {
        let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(overridden(&a("/renderer d3d")).as_deref(), Some("d3d"));
        assert_eq!(
            overridden(&a("--renderer warp /n")).as_deref(),
            Some("warp")
        );
    }

    #[test]
    fn settings_names() {
        assert_eq!(
            serde_json::to_string(&Api::Direct3D).unwrap(),
            "\"direct3d\""
        );
        assert_eq!(
            serde_json::from_str::<Api>("\"opengl\"").unwrap(),
            Api::OpenGl
        );
    }

    #[cfg(windows)]
    #[test]
    fn adapters_in_the_systems_order() {
        use eframe::wgpu::DeviceType::*;
        // The main screen's card first; one that can't draw to the window is passed over.
        let l = [(IntegratedGpu, false), (DiscreteGpu, true), (Cpu, true)];
        assert_eq!(choose(&l, Adapters::Hardware), Some(1));
        assert_eq!(choose(&l, Adapters::Software), Some(2));
        assert_eq!(choose(&l, Adapters::Any), Some(1));
        // No card: WARP only when allowed.
        let warp = [(Cpu, true)];
        assert_eq!(choose(&warp, Adapters::Hardware), None);
        assert_eq!(choose(&warp, Adapters::Any), Some(0));
        assert_eq!(choose(&l, Adapters::Nothing), None);
    }
}

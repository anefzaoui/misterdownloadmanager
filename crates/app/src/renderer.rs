//! What draws the windows (#89). OpenGL (eframe's glow) everywhere; on Windows also Direct3D 12
//! (eframe's wgpu with wgpu's Direct3D 12 backend only), chosen in View → Draw with and used
//! from the next start. Linux and macOS draw with OpenGL only: wgpu isn't built there (its
//! Vulkan and Metal backends would add size for windows OpenGL already draws well).
//!
//! The windows always open: when the chosen way can't start (no Direct3D 12 driver, a virtual
//! machine, Remote Desktop), the other is tried, and the reason is logged. A start with Direct3D
//! that ends before its first frame (a crash or a kill inside the driver) is remembered, and the
//! next starts draw with OpenGL until Direct3D is chosen again.
//!
//! Testing overrides, in this order: `/renderer <name>` on the command line, then
//! `MDM_RENDERER=<name>`. Names: `opengl` (`gl`), `direct3d` (`d3d`), `warp` (Direct3D drawn by
//! Microsoft's software renderer, as on a machine without a graphics driver).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

/// A choice of View → Draw with, as saved in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Api {
    #[serde(rename = "opengl")]
    OpenGl,
    #[serde(rename = "direct3d")]
    Direct3D,
}

/// What draws when the user chose nothing (the settings' `renderer: None`). OpenGL until the
/// comparison in #89 says otherwise.
pub const DEFAULT: Api = Api::OpenGl;

/// Which graphics adapters a Direct3D start may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapters {
    /// A graphics card's driver only.
    Hardware,
    /// Microsoft's software renderer (WARP) only.
    Software,
    /// A card first, else WARP: the last way to open the windows at all.
    Any,
    /// None at all: Direct3D fails to start (debug builds' `MDM_RENDERER=direct3d-none`, to
    /// check the way back to OpenGL).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
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
        #[cfg(debug_assertions)]
        "direct3d-none" => Some(Attempt::Direct3D(Adapters::Nothing)),
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
/// here before; then the other way. Only OpenGL where Direct3D isn't built.
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
    let then = match first {
        Attempt::OpenGl => Attempt::Direct3D(Adapters::Any),
        Attempt::Direct3D(_) => Attempt::OpenGl,
    };
    let attempts: Vec<Attempt> = [first, then]
        .into_iter()
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

/// Frames drawn since the start (counted to two).
static FRAMES: AtomicU32 = AtomicU32::new(0);

/// The end of every main-window frame. The second one means the first was drawn: a watched
/// Direct3D start made it.
pub fn frame_done() {
    if FRAMES.load(Ordering::Relaxed) >= 2 {
        return;
    }
    if FRAMES.fetch_add(1, Ordering::Relaxed) == 1 {
        tracing::info!("first frame drawn, {} ms after start", since_start());
        if let Some(f) = file(STARTING) {
            let _ = std::fs::remove_file(f);
        }
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
        Attempt::OpenGl => opts.renderer = eframe::Renderer::Glow,
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
    let _ = ACTIVE.set(attempt.api());
}

/// eframe's wgpu set up for Direct3D 12 only.
#[cfg(windows)]
mod direct3d {
    use super::Adapters;
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;
    use std::sync::Arc;

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
    let hardware = |t: DeviceType| t != DeviceType::Cpu;
    let first =
        |want: &dyn Fn(DeviceType) -> bool| list.iter().position(|&(t, fits)| fits && want(t));
    match adapters {
        Adapters::Hardware => first(&hardware),
        Adapters::Software => first(&|t| !hardware(t)),
        Adapters::Any => first(&hardware).or_else(|| first(&|t| !hardware(t))),
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
            // Nothing chosen: OpenGL, and Direct3D if OpenGL can't start.
            let p = plan(None, None, false);
            assert_eq!(p.attempts, [Attempt::OpenGl, any]);
            assert!(!p.watched);
            // Direct3D chosen: tried first, its start watched; OpenGL behind it.
            let p = plan(Some(Api::Direct3D), None, false);
            assert_eq!(p.attempts, [d3d, Attempt::OpenGl]);
            assert!(p.watched);
            // ... unless it failed here before.
            let p = plan(Some(Api::Direct3D), None, true);
            assert_eq!(p.attempts, [Attempt::OpenGl, any]);
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
            assert_eq!(
                plan(None, Some("vulkan"), false).attempts[0],
                Attempt::OpenGl
            );
        } else {
            // Direct3D isn't built: OpenGL whatever was asked.
            for p in [
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

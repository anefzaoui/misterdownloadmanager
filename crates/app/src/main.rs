#![windows_subsystem = "windows"]

mod app;
mod batch;
mod categories;
mod checksum;
mod chrome;
mod dialogs;
mod droptarget;
mod errors;
mod find;
mod help;
mod i18n;
mod icons;
mod langpick;
mod list;
mod model;
mod options;
mod pacfetch;
mod progress;
mod properties;
mod queue;
mod quota;
mod regional;
mod scheduler;
mod scrollbar;
mod selection;
mod settings;
mod store;
mod theme;
mod torrentdlg;
mod torrents;
mod transfer;
mod tray;
#[cfg(unix)]
mod unix;
mod videodlg;
#[cfg(windows)]
mod win32;
mod winui;

/// The main window's title (dialogs find their owner by it).
pub const MAIN_TITLE: &str = concat!("Mister Download Manager ", env!("CARGO_PKG_VERSION"));

/// The app icon (art/, made with ChatGPT; sizes by tools/icon/build_icon.py), for windows that
/// show one as IDM's do.
pub fn app_icon() -> std::sync::Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<std::sync::Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let png = include_bytes!("../assets/icons/app/mdm-32.png");
        std::sync::Arc::new(eframe::icon_data::from_png_bytes(png).unwrap_or_default())
    })
    .clone()
}

/// MDM_OFFSCREEN=1, for screenshots and checks (tools/lab/docs-shots.ps1): every window opens
/// far off the screens, never takes the focus and has no taskbar button, and there is no tray
/// icon and no sound, so nothing shows while it runs. The windows still draw, and PrintWindow
/// captures them.
pub fn offscreen() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MDM_OFFSCREEN").is_some_and(|v| v == "1"))
}

/// A window's builder as [`offscreen`] wants it; unchanged otherwise. Dialogs open centred on
/// the main window, so they are off the screens with it.
pub fn quiet(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if offscreen() {
        builder.with_active(false).with_taskbar(false)
    } else {
        builder
    }
}

fn main() -> eframe::Result {
    // A windowed app has no console: record panics in %APPDATA%\misterdownloadmanager\crash.log.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = mdm_ipc::data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let bt = std::backtrace::Backtrace::force_capture();
            let _ = std::fs::write(dir.join("crash.log"), format!("{info}\n\n{bt}"));
        }
        default_hook(info);
    }));

    // IDM-style command line; hand it to a running instance if there is one.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = mdm_ipc::parse_args(&args);
    if mdm_ipc::send(&cmd.clone().unwrap_or_default()).is_ok()
        || cmd.as_ref().is_some_and(|c| c.exit)
    {
        return Ok(());
    }
    // No instance answered. Only one launch may become the instance (two started together
    // would both get here); the other forwards its command once the winner listens.
    let Some(_instance) = mdm_ipc::instance_lock() else {
        let _ = mdm_ipc::send_waiting(&cmd.unwrap_or_default(), std::time::Duration::from_secs(10));
        return Ok(());
    };
    #[cfg(windows)]
    win32::mark_running();

    // Downloads run on a Tokio runtime; the UI thread only reads snapshots.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rt.handle().clone();
    // MDM_ZOOM=2: everything drawn twice as large (sharp 2× screenshots; checks the windows'
    // sizing at high display scaling).
    let zoom = std::env::var("MDM_ZOOM")
        .ok()
        .and_then(|z| z.parse::<f32>().ok())
        .filter(|z| (0.5..=4.0).contains(z));
    let z = zoom.unwrap_or(1.0);
    let mut viewport = quiet(
        egui::ViewportBuilder::default()
            .with_title(MAIN_TITLE)
            .with_inner_size([930.0 * z, 470.0 * z])
            .with_min_inner_size([600.0 * z, 300.0 * z])
            .with_icon(app_icon()),
    );
    if offscreen() {
        viewport = viewport.with_position([-20000.0, -20000.0]);
    }
    let opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "misterdownloadmanager",
        opts,
        Box::new(move |cc| {
            if let Some(z) = zoom {
                cc.egui_ctx.set_zoom_factor(z);
            }
            Ok(Box::new(app::App::new(cc, handle, cmd)))
        }),
    )
}

#![windows_subsystem = "windows"]

mod app;
mod batch;
mod categories;
mod checksum;
mod chrome;
mod crash;
mod crashui;
mod dialogs;
mod droptarget;
mod errors;
mod find;
mod help;
mod helpers;
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
mod update;
mod videodlg;
#[cfg(windows)]
mod win32;
mod winui;

/// The main window's title (dialogs find their owner by it).
pub const MAIN_TITLE: &str = concat!("Mister Download Manager ", env!("CARGO_PKG_VERSION"));

/// The app icon (art/, made with ChatGPT; sizes by tools/icon/build_icon.py), for windows that
/// show one as IDM's do: the 256 px size, which the system scales down for the title bar, the
/// taskbar and Alt+Tab at any display scaling. On macOS none: the Dock keeps the app bundle's own
/// icon (AppIcon.icns, every size, tools/installer/build-macos.sh), which an icon set at run
/// time would replace.
pub fn app_icon() -> std::sync::Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<std::sync::Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        if cfg!(target_os = "macos") {
            // eframe takes an empty icon as "none" (AppTitleIconSetter).
            return std::sync::Arc::new(egui::IconData::default());
        }
        let png = include_bytes!("../assets/icons/app/mdm-256.png");
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
    // MDM's crash monitor (crash.rs started it): the report window when the app crashes or
    // freezes; it never becomes MDM's instance.
    let all: Vec<String> = std::env::args().collect();
    if let Some(m) = mdm_crash::MonitorArgs::parse(&all) {
        return crashui::monitor_main(m);
    }

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
    // Started by an update (update.rs): the old copy may still be closing; this one waits for
    // it rather than handing it anything.
    let after_update = args.iter().any(|a| a.eq_ignore_ascii_case(update::UPDATED));
    if !after_update
        && (mdm_ipc::send(&cmd.clone().unwrap_or_default()).is_ok()
            || cmd.as_ref().is_some_and(|c| c.exit))
    {
        return Ok(());
    }
    // No instance answered. Only one launch may become the instance (two started together
    // would both get here); the other forwards its command once the winner listens.
    let lock = if after_update {
        update::lock_after_update()
    } else {
        mdm_ipc::instance_lock()
    };
    let Some(_instance) = lock else {
        let _ = mdm_ipc::send_waiting(&cmd.unwrap_or_default(), std::time::Duration::from_secs(10));
        return Ok(());
    };
    #[cfg(windows)]
    win32::mark_running();
    // What an earlier update set aside (old programs, a Mac's old app, downloaded files).
    update::tidy();

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
    // The list and settings, read before the window opens: it comes back where it was left.
    let loaded = store::load();
    // The log (release builds too) and the crash monitor.
    if let Some(data) = mdm_ipc::data_dir() {
        mdm_crash::logs::init(&data, mdm_crash::logs::APP);
        tracing::info!(
            "Mister Download Manager {} ({}) starting",
            help::VERSION,
            help::COMMIT
        );
        crash::start(
            &data,
            crash::session(
                loaded.settings.language.as_deref().unwrap_or("system"),
                loaded.settings.dark_mode_support,
            ),
        );
    }
    #[cfg(unix)]
    use unix::on_a_screen;
    #[cfg(windows)]
    use win32::on_a_screen;
    let saved = loaded
        .settings
        .window
        .filter(|_| zoom.is_none() && !offscreen())
        .filter(|[x, y, w, h]| *w >= 600 && *h >= 300 && on_a_screen(x + 40, y + 10));
    let size = saved.map_or([930.0 * z, 470.0 * z], |[_, _, w, h]| [w as f32, h as f32]);
    let mut viewport = quiet(
        egui::ViewportBuilder::default()
            .with_title(MAIN_TITLE)
            .with_inner_size(size)
            .with_min_inner_size([600.0 * z, 300.0 * z])
            .with_maximized(loaded.settings.window_maximized && !offscreen())
            .with_icon(app_icon()),
    );
    if let Some([x, y, ..]) = saved {
        viewport = viewport.with_position([x as f32, y as f32]);
    }
    if offscreen() {
        viewport = viewport.with_position([-20000.0, -20000.0]);
    }
    let opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "misterdownloadmanager",
        opts,
        Box::new(move |cc| {
            if let Some(z) = zoom {
                cc.egui_ctx.set_zoom_factor(z);
            }
            Ok(Box::new(app::App::new(cc, handle, cmd, loaded)))
        }),
    );
    crash::clean_exit();
    result
}

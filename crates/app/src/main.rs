#![windows_subsystem = "windows"]

mod app;
mod batch;
mod categories;
mod checksum;
mod chrome;
mod crash;
mod crashui;
mod debrid;
mod dialogs;
mod droptarget;
mod errors;
mod extension;
mod extractq;
mod fileicons;
mod find;
mod gallerydlg;
mod grabber;
mod help;
mod helpers;
mod i18n;
mod icons;
mod langpick;
mod list;
mod model;
mod netwatch;
mod options;
mod pacfetch;
mod playlistdlg;
mod progress;
mod properties;
mod queue;
mod quota;
mod regional;
mod renderer;
mod rss;
mod rssdlg;
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
#[cfg(any(target_os = "linux", test))]
mod xdgicons;

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

/// The clipboard's text, if any. Never read in an off-screen run ([`offscreen`]): its windows
/// open from scripts, and the clipboard belongs to the person at the computer (Add URL's
/// address came from it, and was fetched).
pub fn clipboard_text() -> Option<String> {
    read_clipboard(offscreen(), || {
        arboard::Clipboard::new().ok()?.get_text().ok()
    })
}

fn read_clipboard(off: bool, read: impl FnOnce() -> Option<String>) -> Option<String> {
    if off { None } else { read() }
}

/// Put `text` on the clipboard (egui hands it to the system after the frame). An off-screen
/// run leaves the clipboard alone and logs what it would have copied, for a check to read.
pub fn copy_text(ctx: &egui::Context, text: String) {
    if let Some(text) = write_clipboard(offscreen(), text) {
        ctx.copy_text(text);
    }
}

/// [`copy_text`] at once, outside a frame; whether the clipboard took it (off-screen: logged,
/// and true).
pub fn set_clipboard(text: &str) -> bool {
    match write_clipboard(offscreen(), text.to_owned()) {
        Some(text) => arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .is_ok(),
        None => true,
    }
}

fn write_clipboard(off: bool, text: String) -> Option<String> {
    if off {
        tracing::info!("clipboard (off-screen, not copied): {text}");
        None
    } else {
        Some(text)
    }
}

/// Where [`offscreen`] puts the main window: far off the screens, or at `MDM_OFFSCREEN_AT=x,y`.
/// That is for a machine whose screen nobody watches (the docs screenshots on GitHub's runner,
/// .github/workflows/docs-shots.yml): there the software OpenGL (Mesa's llvmpipe) presents
/// through GDI, which draws nothing off the screens.
fn offscreen_at() -> [f32; 2] {
    std::env::var("MDM_OFFSCREEN_AT")
        .ok()
        .and_then(|v| {
            let (x, y) = v.split_once(',')?;
            Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
        })
        .unwrap_or([-20000.0, -20000.0])
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

fn main() -> std::process::ExitCode {
    renderer::mark_start();
    // PAC's myIpAddress from the routing table, not a UDP socket (a firewall prompt, #35).
    #[cfg(windows)]
    mdm_engine::net::set_source_lookup(win32::source_address);
    // misterdownloadmanager-cli's commands (crates/cli): its launcher starts this program for
    // them. Nothing of the app's runs: no window, instance, crash monitor or crash log.
    if let Some(cli) = mdm_cli::launched(std::env::args_os().collect()) {
        // A windowed program has no console: the launcher's, unless the output goes to a file
        // or a pipe. On Linux and macOS the launcher became this process, terminal and all.
        #[cfg(windows)]
        win32::cli_console(cli.launcher);
        return mdm_cli::main(cli.args);
    }
    match app() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // As `main` returning the error would.
        Err(e) => {
            eprintln!("Error: {e:?}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn app() -> eframe::Result {
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
    // The command channel opens now, before the window: the browser's host that started MDM
    // waits for it, and what arrives meanwhile is handled at the first frame. Without it the
    // app still works; it just can't receive commands.
    let (tx, commands) = std::sync::mpsc::channel();
    if let Some(cmd) = cmd {
        let _ = tx.send(mdm_ipc::Incoming { cmd, reply: None });
    }
    let window: std::sync::Arc<std::sync::OnceLock<egui::Context>> = Default::default();
    let wake = window.clone();
    let _ = mdm_ipc::serve(tx, move || {
        if let Some(ctx) = wake.get() {
            ctx.request_repaint();
        }
    });

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
        viewport = viewport.with_position(offscreen_at());
    }
    // OpenGL or Direct3D (View → Draw with, #89), the other one when it can't start.
    let plan = renderer::plan(
        loaded.settings.renderer,
        renderer::overridden(&args).as_deref(),
        renderer::failed_before().is_some(),
    );
    let options = || eframe::NativeOptions {
        viewport: viewport.clone(),
        // Linux: X11 (XWayland) over Wayland when both are there, so menus open where clicked.
        #[cfg(target_os = "linux")]
        event_loop_builder: unix::prefer_x11(),
        ..Default::default()
    };
    let result = renderer::run("misterdownloadmanager", &plan, options, move |cc| {
        if let Some(z) = zoom {
            cc.egui_ctx.set_zoom_factor(z);
        }
        let _ = window.set(cc.egui_ctx.clone());
        app::App::new(cc, handle, commands, loaded)
    });
    crash::clean_exit();
    result
}

#[cfg(test)]
mod tests {
    /// An off-screen run neither reads the clipboard (Add URL, Batch, the clipboard monitor…)
    /// nor writes it (Copy magnet link, Copy…): it logs what it would have copied.
    #[test]
    fn offscreen_runs_leave_the_clipboard_alone() {
        let mut asked = false;
        let got = super::read_clipboard(true, || {
            asked = true;
            Some("https://meet.example/abc".into())
        });
        assert_eq!(got, None);
        assert!(!asked, "the clipboard isn't even opened");
        assert_eq!(
            super::read_clipboard(false, || Some("x".into())),
            Some("x".into())
        );
        assert_eq!(
            super::write_clipboard(true, "magnet:?xt=urn:btih:ab".into()),
            None
        );
        assert_eq!(
            super::write_clipboard(false, "magnet:?xt=urn:btih:ab".into()),
            Some("magnet:?xt=urn:btih:ab".into())
        );
    }
}

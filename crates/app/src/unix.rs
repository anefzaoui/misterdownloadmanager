//! Linux and macOS counterparts of what `win32` does through Windows (experimental builds):
//! starting with the session, the browsers' host registration, sounds, showing a file in its
//! folder, and the torrent link handler. Each runs the system's own tool, so nothing here needs
//! a library; the exceptions are the desktop's dark mode on Linux (D-Bus, through zbus) and the
//! crash reports (at the end): signals and Mach exceptions through crash-handler, thread and
//! process ids through libc.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The program to start again later: the AppImage file itself when running from one (its
/// programs sit in a temporary mount), else this executable.
fn program() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

/// Run a tool quietly, without waiting for it.
fn spawn(cmd: &mut Command) {
    let _ = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Where "start with the session" is kept: an XDG autostart entry on Linux, a LaunchAgent on
/// macOS.
fn startup_file() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        Some(home.join("Library/LaunchAgents/io.github.anefzaoui.misterdownloadmanager.plist"))
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Some(config.join("autostart/misterdownloadmanager.desktop"))
    }
}

pub fn run_at_startup() -> bool {
    startup_file().is_some_and(|f| f.is_file())
}

pub fn set_run_at_startup(on: bool) {
    let (Some(file), Some(exe)) = (startup_file(), program()) else {
        return;
    };
    if !on {
        let _ = std::fs::remove_file(&file);
        return;
    }
    let exe = exe.display().to_string();
    let text = if cfg!(target_os = "macos") {
        let exe = exe
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n\
             \x20 <key>Label</key><string>io.github.anefzaoui.misterdownloadmanager</string>\n\
             \x20 <key>ProgramArguments</key><array><string>{exe}</string></array>\n\
             \x20 <key>RunAtLoad</key><true/>\n\
             </dict>\n</plist>\n"
        )
    } else {
        // Desktop Entry quoting: the path in double quotes, with \ " ` $ escaped.
        let quoted: String = exe
            .chars()
            .flat_map(|c| match c {
                '\\' | '"' | '`' | '$' => vec!['\\', c],
                c => vec![c],
            })
            .collect();
        format!(
            "[Desktop Entry]\nType=Application\nName=Mister Download Manager\n\
             Exec=\"{quoted}\"\nIcon=misterdownloadmanager\nTerminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
        )
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, text);
}

/// Browser integration: the host's manifests in every installed browser's folder. There is no
/// installer step on Linux and macOS, so this runs at each start (a moved app, or a browser
/// installed since, is picked up).
///
/// From an AppImage, whose programs sit in a mount that is gone once it quits, the host is first
/// copied to the data folder with the AppImage's path beside it (`app-path`: the host starts
/// that when MDM isn't running), and registered from there.
pub fn register_browsers() {
    if let Some(mut cmd) = register_command() {
        spawn(&mut cmd);
    }
}

/// `register_browsers`, waiting for it to finish (Test browser integration's Set it up).
pub fn register_browsers_now() {
    if let Some(mut cmd) = register_command() {
        let _ = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The host's `--register`, from a copy outside an AppImage's mount when it runs from one.
fn register_command() -> Option<Command> {
    let Ok(exe) = std::env::current_exe() else {
        return None;
    };
    let host = exe.with_file_name("misterdownloadmanager-nmhost");
    let host = match (std::env::var_os("APPIMAGE"), mdm_ipc::data_dir()) {
        (Some(image), Some(data)) => {
            let dir = data.join("browser-host");
            let copy = dir.join("misterdownloadmanager-nmhost");
            let made = std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::copy(&host, &copy).map(|_| ()))
                .and_then(|()| std::fs::write(dir.join("app-path"), image.as_encoded_bytes()));
            if made.is_err() {
                return None;
            }
            copy
        }
        _ => host,
    };
    let mut cmd = Command::new(host);
    cmd.arg("--register");
    Some(cmd)
}

/// Browser integration is registered: a host manifest in one of the browsers' folders, for this
/// user or for everyone (Options → General → Test browser integration).
pub fn host_registered() -> bool {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let mac = cfg!(target_os = "macos");
    // The folders the native host writes (mdm_ipc::browsers), for this user and for everyone.
    [false, true]
        .into_iter()
        .flat_map(|all| mdm_ipc::browsers::host_dirs(&home, &config, mac, all))
        .any(|(d, _)| d.join("com.misterdownloadmanager.host.json").is_file())
}

/// Options → Sounds: play a sound file (macOS: afplay; Linux: PulseAudio / PipeWire's paplay,
/// else ALSA's aplay).
pub fn play_sound(file: &str) {
    if cfg!(target_os = "macos") {
        spawn(Command::new("afplay").arg(file));
    } else {
        let file = file.to_owned();
        std::thread::spawn(move || {
            let played = Command::new("paplay")
                .arg(&file)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !played {
                spawn(Command::new("aplay").arg("-q").arg(&file));
            }
        });
    }
}

/// The system and its version, for problem reports: "macOS 15.1" (sw_vers), or the Linux
/// distribution's name from /etc/os-release ("Ubuntu 24.04.1 LTS").
pub fn os_version() -> String {
    if cfg!(target_os = "macos") {
        let v = Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default();
        return format!("macOS {v}").trim_end().to_owned();
    }
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| "Linux".into())
}

/// Show the file selected in the file manager (macOS: Finder; Linux: the desktop's file
/// manager through D-Bus, else its folder opened).
pub fn show_in_folder(path: &Path) {
    if !path.exists() {
        if let Some(dir) = path.parent().filter(|d| d.is_dir()) {
            let _ = open::that_detached(dir);
        }
        return;
    }
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg("-R").arg(path));
        return;
    }
    let Ok(url) = url::Url::from_file_path(path) else {
        return;
    };
    let shown = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("['{}']", url.as_str().replace('\'', "%27")),
            "",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !shown && let Some(dir) = path.parent() {
        let _ = open::that_detached(dir);
    }
}

/// Options → Downloads → open magnet links and .torrent files with Mister Download Manager:
/// Linux records it with xdg-mime for the desktop entry the packages install (turning it off
/// leaves the choice to the desktop's settings). Not on macOS yet: it hands links to an app as
/// open-URL events, which the app doesn't receive.
pub fn set_torrent_handlers(on: bool) {
    if on && !cfg!(target_os = "macos") {
        spawn(Command::new("xdg-mime").args([
            "default",
            "misterdownloadmanager.desktop",
            "x-scheme-handler/magnet",
            "application/x-bittorrent",
        ]));
    }
}

/// `win32::assistive_technology_listening`: Linux's AT-SPI adapter answers a screen reader with
/// the first tree a frame produces (no placeholder to replace), and macOS has no such cheap
/// question; there, as on Windows, a window's tree is ready at once for any window read after
/// the first one (egui keeps AccessKit on from the first request).
pub fn assistive_technology_listening() -> bool {
    false
}

/// `win32::wheel_lines`: Linux and macOS have no such setting shared by every toolkit; three
/// rows a notch, as GTK, Qt and Windows' default do.
pub fn wheel_lines() -> Option<f32> {
    Some(3.0)
}

/// The desktop's light or dark preference on Linux, as the freedesktop portal tells it
/// (`org.freedesktop.appearance` `color-scheme`, which KDE, GNOME, Cinnamon, COSMIC and the
/// others set): `Some(true)` dark, `Some(false)` light, `None` when no portal answers or the
/// desktop has no preference (then the window system's answer is used). X11 reports no theme to
/// winit at all, so without this "Dark Mode support" never turns dark there. The first call
/// starts a thread that reads it and follows its changes (D-Bus can take seconds to answer, or
/// start the portal); `ctx` is repainted when it changes.
#[cfg(target_os = "linux")]
pub fn desktop_dark(ctx: &egui::Context) -> Option<bool> {
    use std::sync::atomic::{AtomicU8, Ordering};
    // 0: not known, 1: dark, 2: light.
    static SCHEME: AtomicU8 = AtomicU8::new(0);
    static WATCH: std::sync::Once = std::sync::Once::new();
    WATCH.call_once(|| {
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("desktop-theme".into())
            .spawn(move || {
                let set = |v: &zbus::zvariant::Value| {
                    let scheme = match color_scheme(v) {
                        Some(1) => 1,
                        Some(2) => 2,
                        _ => 0,
                    };
                    if SCHEME.swap(scheme, Ordering::Relaxed) != scheme {
                        ctx.request_repaint();
                    }
                };
                let watch = || -> zbus::Result<()> {
                    let conn = zbus::blocking::Connection::session()?;
                    let portal = zbus::blocking::Proxy::new(
                        &conn,
                        "org.freedesktop.portal.Desktop",
                        "/org/freedesktop/portal/desktop",
                        "org.freedesktop.portal.Settings",
                    )?;
                    let changes = portal.receive_signal("SettingChanged")?;
                    let key = ("org.freedesktop.appearance", "color-scheme");
                    // ReadOne since version 2 of the portal; Read (the value wrapped once more)
                    // before it.
                    let now: zbus::zvariant::OwnedValue = portal
                        .call("ReadOne", &key)
                        .or_else(|_| portal.call("Read", &key))?;
                    set(&now);
                    for change in changes {
                        let (namespace, name, value): (String, String, zbus::zvariant::OwnedValue) =
                            change.body().deserialize()?;
                        if (namespace.as_str(), name.as_str()) == key {
                            set(&value);
                        }
                    }
                    Ok(())
                };
                if let Err(e) = watch() {
                    tracing::info!("desktop theme: no answer from the portal ({e})");
                }
            });
    });
    match SCHEME.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// The portal's `color-scheme` number (0 no preference, 1 dark, 2 light), unwrapped from the
/// variants `Read` adds around it.
#[cfg(target_os = "linux")]
fn color_scheme(v: &zbus::zvariant::Value) -> Option<u32> {
    match v {
        zbus::zvariant::Value::U32(n) => Some(*n),
        zbus::zvariant::Value::Value(inner) => color_scheme(inner),
        _ => None,
    }
}

/// Linux: the windows run through X11 (XWayland, in a Wayland session) when there is one, unless
/// `MDM_WAYLAND=1` (#50). Wayland lets no program place its own windows, and MDM's menus and drop
/// lists are windows it places under what was clicked: Wayland put them in the middle of the
/// screen. The hook for `NativeOptions::event_loop_builder`; None: winit's own choice.
#[cfg(target_os = "linux")]
pub fn prefer_x11() -> Option<eframe::EventLoopBuilderHook> {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    let keep = std::env::var_os("MDM_WAYLAND").is_some_and(|v| v == "1");
    let x11 = set("WAYLAND_DISPLAY") && set("DISPLAY") && !keep;
    let _ = ON_WAYLAND.set(set("WAYLAND_DISPLAY") && !x11);
    x11.then(|| -> eframe::EventLoopBuilderHook {
        Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        })
    })
}

#[cfg(target_os = "linux")]
static ON_WAYLAND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Linux: MDM's windows are Wayland's (not X11's or XWayland's), as `prefer_x11` decided.
#[cfg(target_os = "linux")]
pub fn on_wayland() -> bool {
    *ON_WAYLAND.get_or_init(|| std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()))
}

/// Linux: a tray host is there to show MDM's icon (a StatusNotifierWatcher on the session bus:
/// KDE, waybar's tray, GNOME's AppIndicator extension), #59. Without one (MangoWC, plain
/// window managers), closing the window would hide it where nothing can bring it back.
#[cfg(target_os = "linux")]
pub fn tray_host() -> bool {
    let has = || -> zbus::Result<bool> {
        let bus = zbus::blocking::Connection::session()?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&bus)?;
        let name = zbus::names::BusName::try_from("org.kde.StatusNotifierWatcher")?;
        Ok(dbus.name_has_owner(name)?)
    };
    has().unwrap_or(false)
}

/// `win32::on_a_screen`: winit has no monitor list before the window exists, so a saved
/// position is trusted (the window manager keeps windows on screen on Linux and macOS).
pub fn on_a_screen(_x: i32, _y: i32) -> bool {
    true
}

// ---- File icons (fileicons.rs) -----------------------------------------------------------------

/// `win32::file_icon` on Linux: the desktop icon theme's file for a type, as its file manager
/// shows it (xdgicons.rs): the type's MIME type from shared-mime-info, its icon names, looked up
/// in the desktop's theme (read once: KDE's kdeglobals, GNOME's gsettings, GTK's settings.ini),
/// the themes it inherits, Adwaita and Breeze when installed, then hicolor. None when no theme
/// has one (fileicons.rs then draws Tango's).
#[cfg(target_os = "linux")]
pub fn file_icon(what: &crate::fileicons::What, px: u32) -> Option<crate::fileicons::Native> {
    use crate::fileicons::{Native, What};
    use crate::xdgicons::{Dirs, Icons, MimeDb};
    static LOOKUP: std::sync::OnceLock<(MimeDb, Icons)> = std::sync::OnceLock::new();
    let (mime, icons) = LOOKUP.get_or_init(|| {
        let var = |n: &str| std::env::var(n).ok();
        let dirs = Dirs::new(
            var("HOME").as_deref(),
            var("XDG_DATA_HOME").as_deref(),
            var("XDG_DATA_DIRS").as_deref(),
        );
        let kde = var("XDG_CURRENT_DESKTOP").is_some_and(|d| d.to_uppercase().contains("KDE"));
        let theme = icon_theme(kde);
        tracing::info!("file icons: the {:?} icon theme", theme);
        let default = if kde { "breeze" } else { "Adwaita" };
        let themes = [theme.as_deref().unwrap_or(default), "Adwaita", "breeze"];
        (MimeDb::load(&dirs.data), Icons::new(&dirs.icons, &themes))
    });
    let names = match what {
        What::Folder => vec!["folder".to_string()],
        What::FolderOpen => vec!["folder-open".to_string(), "folder".to_string()],
        What::File(ext) => {
            let found = ext
                .as_ref()
                .and_then(|e| mime.mime_for(&format!("file.{e}")));
            mime.icon_names(found.unwrap_or("application/octet-stream"))
        }
    };
    icons.find(&names, px).map(Native::File)
}

/// The desktop's icon theme name: KDE's (`kdeglobals` [Icons] Theme) first on KDE, else GNOME's
/// (`gsettings`), then GTK's `settings.ini`.
#[cfg(target_os = "linux")]
fn icon_theme(kde: bool) -> Option<String> {
    use crate::xdgicons::ini_value;
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let read = |rel: &str| std::fs::read_to_string(config.join(rel)).ok();
    let kdeglobals = || ini_value(&read("kdeglobals")?, "Icons", "Theme");
    let gnome = || {
        let out = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let name = String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_matches('\'')
            .to_string();
        (out.status.success() && !name.is_empty()).then_some(name)
    };
    let gtk = || {
        ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"]
            .iter()
            .find_map(|f| ini_value(&read(f)?, "Settings", "gtk-icon-theme-name"))
    };
    if kde {
        kdeglobals().or_else(gnome).or_else(gtk)
    } else {
        gnome().or_else(gtk).or_else(kdeglobals)
    }
}

/// `win32::file_icon` on macOS: Finder's icon for a type (NSWorkspace), drawn at `px` pixels
/// into a bitmap of our own (AppKit picks the icon's picture made for that size).
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn file_icon(what: &crate::fileicons::What, px: u32) -> Option<crate::fileicons::Native> {
    use crate::fileicons::{Native, What};
    use objc2::AllocAnyThread as _;
    use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSWorkspace};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    // An extension, or a type identifier where there is none.
    let ty = match what {
        // Finder has no open folder of its own: its folder.
        What::Folder | What::FolderOpen => "public.folder".to_string(),
        What::File(Some(ext)) => ext.clone(),
        What::File(None) => "public.data".to_string(),
    };
    let side = px.clamp(1, 1024) as usize;
    objc2::rc::autoreleasepool(|_| {
        #[allow(deprecated)] // iconForContentType needs a UTType; this takes the extension as is.
        let image = NSWorkspace::sharedWorkspace().iconForFileType(&NSString::from_str(&ty));
        // SAFETY: no planes, so the bitmap allocates and owns its pixels: 8-bit RGBA, packed rows
        // (0 lets it choose the row length and bits per pixel). The colour space name is AppKit's
        // own constant.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                side as isize,
                side as isize,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        let gc = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&gc));
        let edge = side as f64;
        image.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(edge, edge)));
        gc.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        let row = rep.bytesPerRow() as usize;
        let data = rep.bitmapData();
        if data.is_null() || row < side * 4 {
            return None;
        }
        let mut rgba = Vec::with_capacity(side * side * 4);
        for y in 0..side {
            // SAFETY: the bitmap holds `side` rows of `row` bytes, alive while `rep` is.
            rgba.extend_from_slice(unsafe {
                std::slice::from_raw_parts(data.add(y * row), side * 4)
            });
        }
        Some(Native::Rgba {
            side: side as u32,
            rgba,
            premultiplied: true,
        })
    })
}

// ---- Crash reports (crash.rs, mdm-crash) -------------------------------------------------------

static CRASH_HANDLER: std::sync::OnceLock<mdm_crash::crash_handler::CrashHandler> =
    std::sync::OnceLock::new();

/// Catch native crashes (SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGABRT on Linux; Mach exceptions on
/// macOS) and have `reporter`'s monitor dump this process when one comes. On Linux only the
/// monitor may read this process (ptrace).
#[allow(unsafe_code)]
pub fn attach_crash_handler(reporter: std::sync::Arc<mdm_crash::client::Reporter>) -> bool {
    use mdm_crash::crash_handler::{CrashEventResult, CrashHandler, make_crash_event};
    #[cfg(target_os = "linux")]
    let monitor = reporter.monitor_pid();
    // SAFETY: the closure runs in the signal / exception handler; it only sends the dump request
    // over the already-open socket.
    let event = unsafe { make_crash_event(move |cc| CrashEventResult::Handled(reporter.dump(cc))) };
    match CrashHandler::attach(event) {
        Ok(h) => {
            #[cfg(target_os = "linux")]
            h.set_ptracer(monitor);
            CRASH_HANDLER.set(h).is_ok()
        }
        Err(e) => {
            tracing::warn!("crash handler not attached: {e}");
            false
        }
    }
}

/// Have the monitor dump this process now (a panic, a freeze): every thread, this one asking.
/// The process goes on.
pub fn dump_now() -> bool {
    use mdm_crash::crash_handler::CrashEventResult;
    CRASH_HANDLER.get().is_some_and(|h| {
        #[cfg(target_os = "linux")]
        return matches!(
            h.simulate_signal(libc::SIGUSR2 as u32),
            CrashEventResult::Handled(true)
        );
        #[cfg(target_os = "macos")]
        return h.simulate_exception(None);
        #[allow(unreachable_code)]
        {
            let _ = (h, CrashEventResult::Handled(false));
            false
        }
    })
}

/// This thread's id as the dump knows it: the tid on Linux; on macOS none (the dump's ids are
/// Mach port names of the monitor's own; the window thread is found as the main thread).
#[allow(unsafe_code)]
pub fn thread_id() -> u32 {
    #[cfg(target_os = "linux")]
    // SAFETY: a plain query.
    return unsafe { libc::gettid() } as u32;
    #[allow(unreachable_code)]
    0
}

/// Thread `tid`'s processor time and its page faults from disk so far, from /proc (crash.rs's
/// freeze notes).
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub fn thread_usage(tid: u32) -> Option<crate::crash::Usage> {
    let stat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).ok()?;
    // The fields after the name (in parentheses, and it may hold spaces), field 3 on.
    let fields: Vec<&str> = stat[stat.rfind(')')? + 1..].split_whitespace().collect();
    let num = |field: usize| fields.get(field - 3)?.parse::<u64>().ok();
    let (majflt, utime, stime) = (num(12)?, num(14)?, num(15)?);
    // SAFETY: a plain query.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = if hz > 0 { hz as u64 } else { 100 };
    Some(crate::crash::Usage {
        cpu_ms: (utime + stime) * 1000 / hz,
        faults: Some(majflt),
    })
}

/// macOS: not measured, since the window thread has no id there (`thread_id` is 0).
#[cfg(not(target_os = "linux"))]
pub fn thread_usage(_tid: u32) -> Option<crate::crash::Usage> {
    None
}

/// While the window is hidden (#72): glibc hands the heap's free pages back to the system
/// (`malloc_trim`); it keeps them otherwise. Other systems: nothing to do here, since macOS and
/// non-glibc allocators return free pages by themselves, and neither has a working set to trim
/// as Windows does.
#[allow(unsafe_code)]
pub fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: a plain call; it only releases memory the allocator already holds as free.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Move `files` to the Trash (an archive and its volumes after extracting, #37), with the
/// system's own tool: `gio trash` (GLib's, on most Linux desktops) or `trash-put`; on macOS
/// `trash` (macOS 14 and later), else the Finder. False when none could: the files are kept.
pub fn to_trash(files: &[PathBuf]) -> bool {
    if files.is_empty() {
        return true;
    }
    let run = |cmd: &mut Command| {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if cfg!(target_os = "macos") {
        if run(Command::new("trash").args(files)) {
            return true;
        }
        // The Finder moves them as a person would (it may ask once to let MDM drive it).
        let list: Vec<String> = files
            .iter()
            .map(|f| {
                let p = f
                    .display()
                    .to_string()
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                format!("POSIX file \"{p}\"")
            })
            .collect();
        let script = format!(
            "tell application \"Finder\" to delete {{{}}}",
            list.join(", ")
        );
        return run(Command::new("osascript").arg("-e").arg(script));
    }
    run(Command::new("gio").arg("trash").arg("--").args(files))
        || run(Command::new("trash-put").arg("--").args(files))
}

/// Is process `pid` still running?
#[allow(unsafe_code)]
pub fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// End process `pid` (a frozen MDM the user chose to close from the crash report).
#[allow(unsafe_code)]
pub fn end_process(pid: u32) -> bool {
    // SAFETY: a signal to a process id we were given.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) == 0 }
}

/// The crash test's segmentation fault (MDM_CRASH_TEST=1, `/show crash-test:crash`).
#[allow(unsafe_code)]
pub fn crash_now() {
    // SAFETY: none: a write through a null pointer, on purpose, to test the crash reports.
    unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u32>(), 1) };
}

/// The browsers installed, by name, with their program: the app bundle on macOS (in
/// /Applications or ~/Applications), the command on the PATH on Linux. The browser extension's
/// offer (extension.rs) opens its page in each.
pub fn browsers() -> Vec<(String, PathBuf)> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let found = |names: &[&str], dirs: &[PathBuf]| {
        names
            .iter()
            .flat_map(|n| dirs.iter().map(move |d| d.join(n)))
            .find(|p| p.exists())
    };
    let mac = cfg!(target_os = "macos");
    let dirs: Vec<PathBuf> = if mac {
        vec![PathBuf::from("/Applications"), home.join("Applications")]
    } else {
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default()
    };
    // A Flatpak browser (`app.zen_browser.zen` …) isn't listed: its sandbox can't start the
    // native host, so the extension couldn't reach MDM (docs/browser-help.md).
    mdm_ipc::browsers::BROWSERS
        .iter()
        .filter_map(|b| {
            let names = if mac { b.mac_apps } else { b.linux_cmds };
            found(names, &dirs).map(|p| (b.name.to_owned(), p))
        })
        .collect()
}

/// Open `url` in the browser whose program is `program` (from `browsers`).
pub fn open_in(program: &Path, url: &str) {
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg("-a").arg(program).arg(url));
    } else {
        spawn(Command::new(program).arg(url));
    }
}

/// The counterpart of win32's Mark-of-the-Web on a file being downloaded from `url`: macOS'
/// quarantine attribute (Gatekeeper checks a program before its first start, as for a browser's
/// download), and on Linux the freedesktop.org origin attributes file managers show.
pub fn mark_downloaded(path: &Path, url: &str, referrer: Option<&str>) {
    #[cfg(target_os = "macos")]
    {
        let _ = referrer;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        set_xattr(
            path,
            "com.apple.quarantine",
            &format!("0081;{now:x};Mister Download Manager;"),
        );
        let _ = url;
    }
    #[cfg(not(target_os = "macos"))]
    {
        set_xattr(path, "user.xdg.origin.url", url);
        if let Some(r) = referrer.filter(|r| r.starts_with("http")) {
            set_xattr(path, "user.xdg.referrer.url", r);
        }
    }
}

/// One extended attribute; a file system without them is left as it is.
#[allow(unsafe_code)]
fn set_xattr(path: &Path, name: &str, value: &str) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let (Ok(p), Ok(n)) = (
        CString::new(path.as_os_str().as_bytes()),
        CString::new(name),
    ) else {
        return;
    };
    // SAFETY: both strings are NUL-terminated and live through the call; the value is a byte
    // slice with its length.
    #[cfg(target_os = "macos")]
    let _ = unsafe {
        libc::setxattr(
            p.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    #[cfg(not(target_os = "macos"))]
    let _ = unsafe {
        libc::setxattr(
            p.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use zbus::zvariant::Value;

    /// `ReadOne` answers the number, `Read` the number wrapped in one more variant.
    #[test]
    fn color_scheme_unwraps_the_portals_answers() {
        assert_eq!(super::color_scheme(&Value::U32(1)), Some(1));
        assert_eq!(
            super::color_scheme(&Value::Value(Box::new(Value::U32(2)))),
            Some(2)
        );
        assert_eq!(super::color_scheme(&Value::Str("dark".into())), None);
    }
}

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
pub fn program() -> Option<PathBuf> {
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

/// `register_browsers`, waiting for it to finish, up to `limit` (Test browser integration's Set
/// it up, on a thread of its own: extension::set_up).
pub fn register_browsers_now(limit: std::time::Duration) {
    if let Some(mut cmd) = register_command() {
        crate::launch::run_within(&mut cmd, limit);
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
        let v = crate::launch::output_within(
            Command::new("sw_vers").arg("-productVersion"),
            std::time::Duration::from_secs(2),
        )
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

/// How long the file manager has to answer ShowItems before its folder is opened instead: one
/// started for it (D-Bus activation) answers in a second or two.
const SHOW_ITEMS_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Show the file selected in the file manager (macOS: Finder; Linux: the desktop's file
/// manager through D-Bus, else its folder opened). Returns at once: the file manager is asked
/// on a thread of its own (crate::launch), as one slow to start, or that never answers, froze
/// the window for as long as `gdbus` waited (#222); when it doesn't answer within
/// [`SHOW_ITEMS_LIMIT`], the folder is opened instead.
///
/// `gdbus` rather than a zbus call: the same tool and call the desktops answered before, ended
/// at the limit by crate::launch (zbus's blocking calls have no time limit of their own, and a
/// connection to the session bus would be made for this one call).
pub fn show_in_folder(path: &Path) {
    let dir = path.parent().map(Path::to_path_buf);
    if !path.exists() {
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            crate::launch::open(dir);
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
    let mut gdbus = Command::new("gdbus");
    gdbus.args([
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
    ]);
    crate::launch::run_detached_with_fallback(gdbus, SHOW_ITEMS_LIMIT, move || {
        if let Some(dir) = dir {
            let _ = open::that_detached(dir);
        }
    });
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

/// The desktop's accent colour and whether it asks for higher contrast (the freedesktop portal's
/// `accent-color` and `contrast`, GNOME 47+, KDE Plasma), read on a thread of their own that
/// follows their changes; `ctx` is repainted when one changes. None / false until the portal
/// answers, or when it has neither.
#[cfg(target_os = "linux")]
fn desktop_look(ctx: &egui::Context) -> (Option<egui::Color32>, bool) {
    use std::sync::Mutex;
    static LOOK: Mutex<(Option<egui::Color32>, bool)> = Mutex::new((None, false));
    static WATCH: std::sync::Once = std::sync::Once::new();
    WATCH.call_once(|| {
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("desktop-look".into())
            .spawn(move || {
                let set = |name: &str, v: &zbus::zvariant::Value| {
                    let mut look = LOOK.lock().unwrap_or_else(|e| e.into_inner());
                    let before = *look;
                    match name {
                        "accent-color" => look.0 = accent_color(v),
                        "contrast" => look.1 = color_scheme(v) == Some(1),
                        _ => {}
                    }
                    if *look != before {
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
                    for name in ["accent-color", "contrast"] {
                        let key = ("org.freedesktop.appearance", name);
                        let now: zbus::Result<zbus::zvariant::OwnedValue> = portal
                            .call("ReadOne", &key)
                            .or_else(|_| portal.call("Read", &key));
                        if let Ok(now) = now {
                            set(name, &now);
                        }
                    }
                    for change in changes {
                        let (namespace, name, value): (String, String, zbus::zvariant::OwnedValue) =
                            change.body().deserialize()?;
                        if namespace == "org.freedesktop.appearance" {
                            set(&name, &value);
                        }
                    }
                    Ok(())
                };
                if let Err(e) = watch() {
                    tracing::info!("desktop accent: no answer from the portal ({e})");
                }
            });
    });
    *LOOK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The desktop's accent colour (the Native theme's), if it has one.
#[cfg(target_os = "linux")]
pub fn desktop_accent(ctx: &egui::Context) -> Option<egui::Color32> {
    desktop_look(ctx).0
}

/// The desktop asks for higher contrast.
#[cfg(target_os = "linux")]
pub fn desktop_contrast(ctx: &egui::Context) -> bool {
    desktop_look(ctx).1
}

/// The portal's `accent-color`: sRGB, each part from 0 to 1, unwrapped from `Read`'s variants;
/// a part out of range means none is set.
#[cfg(target_os = "linux")]
fn accent_color(v: &zbus::zvariant::Value) -> Option<egui::Color32> {
    use zbus::zvariant::Value;
    match v {
        Value::Value(inner) => accent_color(inner),
        Value::Structure(s) => {
            let parts: Vec<f64> = s
                .fields()
                .iter()
                .filter_map(|f| match f {
                    Value::F64(x) => Some(*x),
                    _ => None,
                })
                .collect();
            accent_parts(&parts)
        }
        _ => None,
    }
}

/// Three parts from 0 to 1 as a colour; None for anything else.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn accent_parts(parts: &[f64]) -> Option<egui::Color32> {
    if parts.len() != 3 || parts.iter().any(|p| !(0.0..=1.0).contains(p)) {
        return None;
    }
    let b = |p: f64| (p * 255.0).round() as u8;
    Some(egui::Color32::from_rgb(
        b(parts[0]),
        b(parts[1]),
        b(parts[2]),
    ))
}

/// macOS's accent colour (System Settings → Appearance), from `defaults read -g
/// AppleAccentColor`: -1 graphite, 0 red, 1 orange, 2 yellow, 3 green, 4 blue, 5 purple, 6 pink;
/// no value is the default (multicolour: blue). Mapped to Apple's published system colours.
///
/// The Native theme asks every 2 s, from the window's thread, so `defaults` runs on a thread of
/// its own (#222) and this answers what it said last, one check late; only the first answer is
/// waited for, at most half a second, so Native starts in the right colour.
#[cfg(target_os = "macos")]
pub fn mac_accent() -> Option<egui::Color32> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    /// What `defaults` said last (None: not yet; Some(None): it couldn't be asked).
    static SAID: Mutex<Option<Option<egui::Color32>>> = Mutex::new(None);
    static ASKING: AtomicBool = AtomicBool::new(false);
    let said = || *SAID.lock().unwrap_or_else(|e| e.into_inner());
    if !ASKING.swap(true, Ordering::AcqRel) {
        let asked = std::thread::Builder::new()
            .name("mac-accent".into())
            .spawn(|| {
                let out = crate::launch::output_within(
                    Command::new("defaults").args(["read", "-g", "AppleAccentColor"]),
                    Duration::from_secs(2),
                );
                let color = out.map(|out| {
                    let n = String::from_utf8_lossy(&out.stdout)
                        .trim()
                        .parse::<i32>()
                        .ok();
                    mac_accent_color(n)
                });
                *SAID.lock().unwrap_or_else(|e| e.into_inner()) = Some(color);
                ASKING.store(false, Ordering::Release);
            });
        if asked.is_err() {
            ASKING.store(false, Ordering::Release);
        }
    }
    let start = Instant::now();
    loop {
        if let Some(color) = said() {
            return color;
        }
        if start.elapsed() > Duration::from_millis(500) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// AppleAccentColor's number (None: not set) as the colour it stands for.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_accent_color(n: Option<i32>) -> egui::Color32 {
    let rgb = egui::Color32::from_rgb;
    match n {
        Some(-1) => rgb(0x8c, 0x8c, 0x8c),
        Some(0) => rgb(0xff, 0x3b, 0x30),
        Some(1) => rgb(0xff, 0x95, 0x00),
        Some(2) => rgb(0xff, 0xcc, 0x00),
        Some(3) => rgb(0x28, 0xcd, 0x41),
        Some(5) => rgb(0xaf, 0x52, 0xde),
        Some(6) => rgb(0xff, 0x2d, 0x55),
        _ => rgb(0x00, 0x7a, 0xff),
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
    let (mime, icons) = icon_lookup();
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

/// The MIME database and the desktop's icon themes, read once: started at start-up by
/// [`prepare_file_icons`], off the window's thread (it asks `gsettings`, #222).
#[cfg(target_os = "linux")]
fn icon_lookup() -> &'static (crate::xdgicons::MimeDb, crate::xdgicons::Icons) {
    use crate::xdgicons::{Dirs, Icons, MimeDb};
    static LOOKUP: std::sync::OnceLock<(MimeDb, Icons)> = std::sync::OnceLock::new();
    LOOKUP.get_or_init(|| {
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
    })
}

/// Read the file icons' themes while the window opens ([`icon_lookup`]); the list's first
/// icons only wait for what is still being read.
#[cfg(target_os = "linux")]
pub fn prepare_file_icons() {
    let _ = std::thread::Builder::new()
        .name("file-icons".into())
        .spawn(|| {
            icon_lookup();
        });
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
        let out = crate::launch::output_within(
            Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "icon-theme"]),
            std::time::Duration::from_secs(2),
        )?;
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
    // native host, so the extension couldn't reach MDM (docs/browser-help.md). Nor is a Chromium
    // browser that runs as a Snap (Ubuntu's Chromium): it starts the host inside its sandbox,
    // which can't read MDM's data folder (~/.local/share is hidden from Snaps) and so started a
    // second, sandboxed MDM. The Snap Firefox is listed: it starts the host through the desktop
    // portal, outside its sandbox.
    mdm_ipc::browsers::BROWSERS
        .iter()
        .filter_map(|b| {
            let names = if mac { b.mac_apps } else { b.linux_cmds };
            found(names, &dirs).map(|p| (b, p))
        })
        .filter(|(b, p)| mac || b.family == mdm_ipc::browsers::Family::Gecko || !is_snap(p))
        .map(|(b, p)| (b.name.to_owned(), p))
        .collect()
}

/// Linux: whether the command `p` starts a Snap: /snap/bin's links to `snap`, or the short
/// scripts some distributions put in /usr/bin to start the Snap in a package's place.
fn is_snap(p: &Path) -> bool {
    if std::fs::canonicalize(p).is_ok_and(|real| real.ends_with("usr/bin/snap")) {
        return true;
    }
    std::fs::metadata(p).is_ok_and(|m| m.len() < 16 * 1024)
        && std::fs::read(p)
            .is_ok_and(|b| b.starts_with(b"#!") && b.windows(10).any(|w| w == b"/snap/bin/"))
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

    /// The child of [`show_in_folder_never_waits_for_the_file_manager`]: the file it shows.
    const SHOW_CHILD: &str = "MDM_TEST_SHOW_IN_FOLDER_CHILD";

    /// #222: a file manager that never answers ShowItems (here a `gdbus` that hangs for 30 s)
    /// leaves `show_in_folder` returning at once, and the folder is opened instead after
    /// SHOW_ITEMS_LIMIT (here an `xdg-open` that notes what it was given). PATH is the test's
    /// own, so the test runs itself again as a child with it (no `set_var` in a test).
    #[test]
    fn show_in_folder_never_waits_for_the_file_manager() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};
        if let Some(file) = std::env::var_os(SHOW_CHILD) {
            let file = std::path::PathBuf::from(file);
            let start = Instant::now();
            super::show_in_folder(&file);
            println!(
                "show_in_folder returned after {} ms",
                start.elapsed().as_millis()
            );
            // The fallback runs on its thread: wait for it before the child ends.
            let opened = file.with_file_name("opened.txt");
            let until = Instant::now() + Duration::from_secs(15);
            while !opened.exists() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
            return;
        }
        let root = std::env::temp_dir().join(format!("mdm-show-222-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (bin, folder) = (root.join("bin"), root.join("Downloads"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("done.zip");
        std::fs::write(&file, b"test").unwrap();
        let opened = folder.join("opened.txt");
        let tool = |name: &str, body: String| {
            let p = bin.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        tool("gdbus", "exec sleep 30".into());
        tool("xdg-open", format!("echo \"$1\" > '{}'", opened.display()));
        // Only these tools and the system's (no Windows programs under WSL, no real opener).
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let start = Instant::now();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "unix::tests::show_in_folder_never_waits_for_the_file_manager",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(SHOW_CHILD, &file)
            .env("PATH", path)
            .output()
            .unwrap();
        let took = start.elapsed();
        let text = String::from_utf8_lossy(&out.stdout);
        // libtest prints the test's name on the same line, before it.
        let returned = text
            .split("show_in_folder returned after ")
            .nth(1)
            .and_then(|l| l.split(" ms").next()?.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("the child said nothing: {text}"));
        assert!(
            returned < 500,
            "show_in_folder waited {returned} ms for gdbus"
        );
        let shown = std::fs::read_to_string(&opened).unwrap_or_default();
        assert_eq!(
            shown.trim(),
            folder.display().to_string(),
            "the folder wasn't opened"
        );
        // The folder opened after the limit, not after gdbus's 30 s.
        assert!(took < Duration::from_secs(20), "took {took:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

// Sign-in secrets in the system's keychain (#44, secrets.rs). Linux: the Secret Service
// (GNOME Keyring, KeePassXC, KDE's wallet in its Secret Service mode) over D-Bus, its default
// collection only, and only while it is unlocked: MDM never unlocks it nor answers a prompt
// (desktops that log in on their own often have a locked keyring, #164), so a locked or missing
// keyring leaves the secrets in the owner-only settings. macOS: the login keychain's generic
// passwords (SecItemAdd / SecItemCopyMatching, through security-framework).

#[cfg(target_os = "linux")]
pub use secret_service::{
    delete as keychain_delete, list as keychain_list, probe as keychain_probe,
    read as keychain_read, write as keychain_write,
};

#[cfg(target_os = "macos")]
pub use mac_keychain::{
    delete as keychain_delete, list as keychain_list, probe as keychain_probe,
    read as keychain_read, write as keychain_write,
};

#[cfg(target_os = "linux")]
mod secret_service {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

    use crate::secrets::Fault;

    const BUS: &str = "org.freedesktop.secrets";
    const ROOT: &str = "/org/freedesktop/secrets";
    const SERVICE: &str = "org.freedesktop.Secret.Service";
    const COLLECTION: &str = "org.freedesktop.Secret.Collection";
    const ITEM: &str = "org.freedesktop.Secret.Item";
    const SESSION: &str = "org.freedesktop.Secret.Session";
    /// Groups MDM's entries in Seahorse and the like.
    const SCHEMA: &str = "io.github.anefzaoui.misterdownloadmanager.Secret";

    /// The D-Bus error's kind; its text (the service's own words) is never kept.
    fn fault(e: zbus::Error) -> Fault {
        let name = match &e {
            zbus::Error::MethodError(name, _, _) => name.as_str().to_owned(),
            zbus::Error::FDO(f) => zbus::DBusError::name(&**f).as_str().to_owned(),
            zbus::Error::InputOutput(_)
            | zbus::Error::Address(_)
            | zbus::Error::Connection(..)
            | zbus::Error::Handshake(_) => return Fault::Missing,
            _ => return Fault::Failed("D-Bus call failed".into()),
        };
        if name == "org.freedesktop.Secret.Error.IsLocked" {
            Fault::Locked
        } else if name.starts_with("org.freedesktop.DBus.Error.") {
            // No such service, none to start, no such object or method: no keyring.
            Fault::Missing
        } else {
            Fault::Failed(name)
        }
    }

    /// The session bus, once (a call answers within 5 s or is taken as no keyring).
    fn connection() -> Result<Connection, Fault> {
        static CONN: Mutex<Option<Connection>> = Mutex::new(None);
        let mut held = CONN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = &*held {
            return Ok(c.clone());
        }
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.method_timeout(std::time::Duration::from_secs(5)).build())
            .map_err(|_| Fault::Missing)?;
        *held = Some(conn.clone());
        Ok(conn)
    }

    fn proxy<'a>(conn: &Connection, path: &'a str, iface: &'a str) -> Result<Proxy<'a>, Fault> {
        zbus::blocking::proxy::Builder::<Proxy<'a>>::new(conn)
            .destination(BUS)
            .and_then(|b| b.path(path))
            .and_then(|b| b.interface(iface))
            .map(|b| b.cache_properties(zbus::proxy::CacheProperties::No))
            .and_then(|b| b.build())
            .map_err(fault)
    }

    /// A session with the service ("plain": the secret crosses only the user's own session bus),
    /// closed when dropped.
    struct Session {
        conn: Connection,
        path: OwnedObjectPath,
    }

    impl Session {
        fn open() -> Result<Session, Fault> {
            let conn = connection()?;
            let (_, path): (OwnedValue, OwnedObjectPath) = proxy(&conn, ROOT, SERVICE)?
                .call("OpenSession", &("plain", Value::from("")))
                .map_err(fault)?;
            Ok(Session { conn, path })
        }

        fn path(&self) -> ObjectPath<'_> {
            self.path.as_ref()
        }

        /// The default collection, only while it is unlocked (never unlocked from here).
        fn collection(&self) -> Result<OwnedObjectPath, Fault> {
            let path: OwnedObjectPath = proxy(&self.conn, ROOT, SERVICE)?
                .call("ReadAlias", &("default",))
                .map_err(fault)?;
            if path.as_str() == "/" {
                return Err(Fault::Missing);
            }
            let locked: bool = proxy(&self.conn, path.as_str(), COLLECTION)?
                .get_property("Locked")
                .map_err(fault)?;
            if locked {
                return Err(Fault::Locked);
            }
            Ok(path)
        }

        /// MDM's items of `service` (one account's, or all): (unlocked, locked).
        fn find(
            &self,
            service: &str,
            account: Option<&str>,
        ) -> Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>), Fault> {
            let mut attrs = HashMap::from([("service", service)]);
            if let Some(account) = account {
                attrs.insert("account", account);
            }
            proxy(&self.conn, ROOT, SERVICE)?
                .call("SearchItems", &(attrs,))
                .map_err(fault)
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            if let Ok(p) = proxy(&self.conn, self.path.as_str(), SESSION) {
                let _ = p.call::<_, _, ()>("Close", &());
            }
        }
    }

    pub fn probe(_service: &str) -> Result<(), Fault> {
        Session::open()?.collection().map(|_| ())
    }

    pub fn read(service: &str, account: &str) -> Result<Option<String>, Fault> {
        let s = Session::open()?;
        let (unlocked, locked) = s.find(service, Some(account))?;
        let Some(item) = unlocked.first() else {
            return if locked.is_empty() {
                Ok(None)
            } else {
                Err(Fault::Locked)
            };
        };
        let (_, _, value, _): (OwnedObjectPath, Vec<u8>, Vec<u8>, String) =
            proxy(&s.conn, item.as_str(), ITEM)?
                .call("GetSecret", &(s.path(),))
                .map_err(fault)?;
        String::from_utf8(value)
            .map(Some)
            .map_err(|_| Fault::Failed("not UTF-8".into()))
    }

    pub fn write(service: &str, account: &str, label: &str, secret: &str) -> Result<(), Fault> {
        let s = Session::open()?;
        let collection = s.collection()?;
        let attrs = HashMap::from([
            ("service", service),
            ("account", account),
            ("xdg:schema", SCHEMA),
        ]);
        let props = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Value::from(label)),
            ("org.freedesktop.Secret.Item.Attributes", Value::from(attrs)),
        ]);
        let value = (
            s.path(),
            Vec::<u8>::new(),
            secret.as_bytes(),
            "text/plain; charset=utf8",
        );
        let (_, prompt): (OwnedObjectPath, OwnedObjectPath) =
            proxy(&s.conn, collection.as_str(), COLLECTION)?
                .call("CreateItem", &(props, value, true))
                .map_err(fault)?;
        // It wants to ask the user (to unlock): never done from here.
        if prompt.as_str() != "/" {
            return Err(Fault::Locked);
        }
        Ok(())
    }

    pub fn delete(service: &str, account: &str) -> Result<(), Fault> {
        let s = Session::open()?;
        let (unlocked, locked) = s.find(service, Some(account))?;
        if unlocked.is_empty() && !locked.is_empty() {
            return Err(Fault::Locked);
        }
        for item in unlocked {
            let prompt: OwnedObjectPath = proxy(&s.conn, item.as_str(), ITEM)?
                .call("Delete", &())
                .map_err(fault)?;
            if prompt.as_str() != "/" {
                return Err(Fault::Locked);
            }
        }
        Ok(())
    }

    /// The accounts of `service`'s unlocked items.
    pub fn list(service: &str) -> Result<Vec<String>, Fault> {
        let s = Session::open()?;
        let (unlocked, _) = s.find(service, None)?;
        let mut accounts = Vec::new();
        for item in unlocked {
            let attrs: OwnedValue = proxy(&s.conn, item.as_str(), ITEM)?
                .get_property("Attributes")
                .map_err(fault)?;
            let attrs = HashMap::<String, String>::try_from(attrs)
                .map_err(|_| Fault::Failed("attributes".into()))?;
            if attrs.get("service").is_some_and(|s| s == service)
                && let Some(account) = attrs.get("account")
            {
                accounts.push(account.clone());
            }
        }
        Ok(accounts)
    }
}

#[cfg(target_os = "macos")]
mod mac_keychain {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    use security_framework::passwords::{
        PasswordOptions, delete_generic_password, generic_password, set_generic_password_options,
    };

    use crate::secrets::Fault;

    const ITEM_NOT_FOUND: i32 = -25300;
    const INTERACTION_NOT_ALLOWED: i32 = -25308;
    const NO_SUCH_KEYCHAIN: i32 = -25294;
    const NOT_AVAILABLE: i32 = -25291;

    fn fault(e: security_framework::base::Error) -> Fault {
        match e.code() {
            INTERACTION_NOT_ALLOWED => Fault::Locked,
            NO_SUCH_KEYCHAIN | NOT_AVAILABLE => Fault::Missing,
            code => Fault::Failed(format!("Security error {code}")),
        }
    }

    pub fn probe(service: &str) -> Result<(), Fault> {
        list(service).map(|_| ())
    }

    pub fn read(service: &str, account: &str) -> Result<Option<String>, Fault> {
        match generic_password(PasswordOptions::new_generic_password(service, account)) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Fault::Failed("not UTF-8".into())),
            Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(fault(e)),
        }
    }

    pub fn write(service: &str, account: &str, label: &str, secret: &str) -> Result<(), Fault> {
        let mut options = PasswordOptions::new_generic_password(service, account);
        options.set_label(label);
        set_generic_password_options(secret.as_bytes(), options).map_err(fault)
    }

    pub fn delete(service: &str, account: &str) -> Result<(), Fault> {
        match delete_generic_password(service, account) {
            Err(e) if e.code() != ITEM_NOT_FOUND => Err(fault(e)),
            _ => Ok(()),
        }
    }

    /// The accounts of `service`'s generic passwords (their attributes only, not their data).
    pub fn list(service: &str) -> Result<Vec<String>, Fault> {
        let found = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service)
            .load_attributes(true)
            .limit(Limit::All)
            .search();
        match found {
            Ok(items) => Ok(items
                .iter()
                .filter_map(|i| i.simplify_dict()?.get("acct").cloned())
                .collect()),
            Err(e) if e.code() == ITEM_NOT_FOUND => Ok(Vec::new()),
            Err(e) => Err(fault(e)),
        }
    }
}

// ---- waking the computer for a queue (#142) -----------------------------------------------

/// Linux: the clock chip's wake alarm, in seconds since 1970. Root's alone unless an
/// administrator lets users write it (docs/queues-and-scheduler.md shows a tmpfiles.d line).
/// Without it nothing else wakes a sleeping Linux for a program: a systemd user timer's
/// `WakeSystem=` needs the system manager, rtcwake needs root, alarm timers CAP_WAKE_ALARM.
#[cfg(target_os = "linux")]
const WAKEALARM: &str = "/sys/class/rtc/rtc0/wakealarm";

/// Whether MDM may wake this computer.
pub fn wake_support() -> crate::wake::Support {
    #[cfg(target_os = "linux")]
    {
        use crate::wake::Support;
        let path = Path::new(WAKEALARM);
        if !path.exists() {
            return Support::NoAlarm;
        }
        // Opening it for writing changes nothing: it only asks whether we may.
        if std::fs::OpenOptions::new().write(true).open(path).is_ok() {
            Support::Ready
        } else {
            Support::NeedsAdmin
        }
    }
    // macOS: `pmset schedule wake` and IOPMSchedulePowerEvent need an administrator, and MDM
    // never asks for one by itself.
    #[cfg(target_os = "macos")]
    {
        crate::wake::Support::MacAdmin
    }
}

/// Seconds to add to a system time to have the clock chip's: the chip may keep local time
/// (/etc/adjtime: LOCAL), or have drifted from the system's clock (a virtual machine's was a
/// minute and a half behind). The alarm is set in the chip's time, as rtcwake does.
#[cfg(target_os = "linux")]
fn rtc_offset() -> i64 {
    let chip = std::fs::read_to_string("/sys/class/rtc/rtc0/since_epoch")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok());
    match chip {
        Some(chip) => chip - chrono::Utc::now().timestamp(),
        None => 0,
    }
}

/// Wake the computer at `at` (seconds since 1970, UTC), or cancel MDM's wake (None).
#[cfg(target_os = "linux")]
pub fn wake_at(at: Option<i64>) -> Result<(), String> {
    // The alarm MDM set (in the chip's time), to tell it from another program's: kept in the
    // data folder, so an alarm left by an MDM that was killed (or signed out of) is still ours.
    let file = mdm_ipc::data_dir().map(|d| d.join("wake-alarm"));
    let mut ours = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| s.trim().parse::<i64>().ok());
    let keep = |v: Option<i64>| {
        if let Some(f) = &file {
            let _ = match v {
                Some(v) => std::fs::write(f, v.to_string()),
                None => std::fs::remove_file(f),
            };
        }
    };
    let set_now = std::fs::read_to_string(WAKEALARM)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok());
    let want = at.map(|t| t + rtc_offset());
    // The same alarm within a couple of seconds (the two clocks are read a moment apart).
    let close = |a: i64, b: i64| (a - b).abs() <= 2;
    match (set_now, want) {
        (Some(now), Some(w)) if close(now, w) => {
            if ours != Some(now) {
                keep(Some(now));
            }
            return Ok(());
        }
        (None, None) => {
            if ours.is_some() {
                keep(None);
            }
            return Ok(());
        }
        _ => {}
    }
    // There is one alarm: one that another program (rtcwake, a script) set stays theirs.
    if let Some(theirs) = set_now
        && ours.is_none_or(|o| !close(o, theirs))
    {
        if want.is_none() {
            return Ok(());
        }
        return Err(crate::i18n::t(
            // i18n: the Scheduler, under "Wake the computer to run this queue" (Linux): the computer's one wake alarm is already set by something else
            "Another program has set the computer's wake alarm",
        )
        .to_owned());
    }
    let write = |v: &str| std::fs::write(WAKEALARM, v).map_err(|e| e.to_string());
    // An alarm is replaced by clearing it first (writing over it is refused).
    if set_now.is_some() {
        write("0")?;
    }
    ours = None;
    if let Some(v) = want {
        write(&v.to_string())?;
        ours = Some(v);
    }
    keep(ours);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn wake_at(at: Option<i64>) -> Result<(), String> {
    match at {
        None => Ok(()),
        Some(_) => Err("macOS lets only an administrator schedule a wake".to_owned()),
    }
}

/// Keep the computer from sleeping while `reason` is given; None lets it sleep again as the
/// system's settings say. Linux: logind's sleep and idle inhibitor, held by its file
/// descriptor (`systemd-inhibit --list` shows it with its reason); a user in the active
/// session may take it without a password. False when logind refused.
#[cfg(target_os = "linux")]
pub fn keep_awake(reason: Option<&str>) -> bool {
    use std::sync::Mutex;
    static LOCK: Mutex<Option<zbus::zvariant::OwnedFd>> = Mutex::new(None);
    let mut held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Closing the descriptor ends the inhibitor.
    *held = None;
    let Some(reason) = reason else {
        return true;
    };
    let take = || -> zbus::Result<zbus::zvariant::OwnedFd> {
        let conn = zbus::blocking::Connection::system()?;
        let logind = zbus::blocking::Proxy::new(
            &conn,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )?;
        logind.call(
            "Inhibit",
            &("sleep:idle", "Mister Download Manager", reason, "block"),
        )
    };
    match take() {
        Ok(fd) => {
            *held = Some(fd);
            true
        }
        Err(e) => {
            tracing::warn!("wake: logind's inhibitor refused ({e})");
            false
        }
    }
}

/// macOS: `caffeinate -i` (no idle sleep) for as long as MDM runs, ended when not needed.
#[cfg(target_os = "macos")]
pub fn keep_awake(reason: Option<&str>) -> bool {
    use std::sync::Mutex;
    static CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);
    let mut held = CHILD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut c) = held.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    if reason.is_some() {
        *held = Command::new("caffeinate")
            .args(["-i", "-w", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        return held.is_some();
    }
    true
}

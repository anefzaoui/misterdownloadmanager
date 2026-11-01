//! Linux and macOS counterparts of what `win32` does through Windows (experimental builds):
//! starting with the session, the browsers' host registration, sounds, showing a file in its
//! folder, and the torrent link handler. Each runs the system's own tool, so nothing here needs
//! a library.

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
pub fn register_browsers() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    spawn(Command::new(exe.with_file_name("misterdownloadmanager-nmhost")).arg("--register"));
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

/// `win32::wheel_lines`: Linux and macOS have no such setting shared by every toolkit; three
/// rows a notch, as GTK, Qt and Windows' default do.
pub fn wheel_lines() -> Option<f32> {
    Some(3.0)
}

/// `win32::on_a_screen`: winit has no monitor list before the window exists, so a saved
/// position is trusted (the window manager keeps windows on screen on Linux and macOS).
pub fn on_a_screen(_x: i32, _y: i32) -> bool {
    true
}

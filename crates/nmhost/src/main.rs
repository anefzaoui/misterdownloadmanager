//! Mister Download Manager's browser native messaging host (Chrome, Edge, Brave, Firefox).
//!
//! - Started by the browser: reads native messages (u32 LE length + JSON) from stdin, hands each
//!   download to the running Mister Download Manager over mdm-ipc (starting Mister Download Manager if needed), answers
//!   `{"ok": …}`. A download Options says to leave to the browser (file type, excluded site or
//!   address, browser, integration off: `mdm_ipc::capture`) is answered `{"ok": false}` without
//!   starting anything, and the extension lets the browser have it. `{"rules": true}` asks for
//!   the file types to offer: `{"ok": true, "types": "EXE ZIP …"}`.
//! - The download button over videos: `{"panel": true, "url": page}` asks whether it may show on
//!   that page (`{"ok": …}`, Options' rules); `{"video": true, "url": page}` opens the Download
//!   video window for it; `{"hideSite": host}` keeps it off that site from now on;
//!   `{"openOptions": true}` opens Options where it is set up.
//! - `misterdownloadmanager-nmhost --register` / `--unregister`: writes / removes the host
//!   manifests (Chromium's names the extension's origin, Firefox's its add-on ID). Windows: beside
//!   this exe, with the per-user (HKCU) registry entries the browsers look up; with `--all-users`
//!   (the installer, installing for everyone, elevated) under HKLM instead. Linux and macOS: in
//!   each installed browser's own host folder (the app does it at every start, as there is no
//!   installer step), or the system-wide ones with `--all-users`. Nothing else.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const HOST_NAME: &str = "com.misterdownloadmanager.host";
/// The extension's fixed ID (from the public key in extension/manifest.json).
const EXTENSION_ID: &str = "oogbclbbninliongijffeeggalppliok";
/// The Firefox add-on's ID (extension/manifest.firefox.json).
const FIREFOX_ID: &str = "integration@misterdownloadmanager.com";
/// Where each Chromium browser looks for native messaging hosts on Windows, under HKCU (one
/// user) or HKLM (everyone).
#[cfg(windows)]
const REGISTRY_KEYS: [&str; 3] = [
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
    r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
];
/// Where Firefox looks.
#[cfg(windows)]
const FIREFOX_KEY: &str = r"Software\Mozilla\NativeMessagingHosts";

/// A message from the extension.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
struct Message {
    /// Only asking for the capture rules (the file types to offer).
    rules: bool,
    url: String,
    referrer: Option<String>,
    cookies: Option<String>,
    user_agent: Option<String>,
    filename: Option<String>,
    /// The user asked for this download (right-click → Download with Mister Download Manager):
    /// Options' automatic-capture rules don't apply, as IDM's "Download with IDM" takes any link.
    explicit: bool,
    /// May the download button show over videos on page `url`?
    panel: bool,
    /// The download button was clicked: `url` is the video's page.
    video: bool,
    /// The button's menu: keep it off this site (a host name).
    hide_site: Option<String>,
    /// The button's menu: open its settings in Options.
    open_options: bool,
}

#[derive(Serialize)]
struct Reply {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// For `rules`: Options → File types.
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<String>,
}

/// The name a download will have: the browser's, else the address's last part.
fn file_name_of(m: &Message) -> String {
    let from_browser = m
        .filename
        .as_deref()
        .and_then(|f| f.rsplit(['\\', '/']).next())
        .filter(|f| !f.is_empty());
    match from_browser {
        Some(f) => f.to_owned(),
        None => m
            .url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned(),
    }
}

/// The answer to one message, under `rules` (the app's saved Options).
fn answer(m: Message, rules: &mdm_ipc::capture::CaptureRules) -> Reply {
    let reply = |ok: bool, error: Option<String>| Reply {
        ok,
        error,
        types: None,
    };
    if m.rules {
        return Reply {
            ok: true,
            error: None,
            types: Some(rules.auto_types.clone()),
        };
    }
    let ua = m.user_agent.clone().unwrap_or_default();
    // The video button: asked about, clicked, its menu. What it sends goes to the app (started
    // if need be), which opens its window or keeps the setting.
    if m.panel {
        return reply(rules.panel_on(&m.url, &ua), None);
    }
    let hide = m
        .hide_site
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty());
    let panel_cmd = if let Some(host) = hide {
        Some(mdm_ipc::Command {
            panel_hide_site: Some(host.to_owned()),
            ..Default::default()
        })
    } else if m.open_options {
        Some(mdm_ipc::Command {
            show: Some("options:general".into()),
            ..Default::default()
        })
    } else if m.video {
        if !(m.url.starts_with("http://") || m.url.starts_with("https://")) {
            return reply(false, Some("not an http(s) page".into()));
        }
        if !rules.panel_on(&m.url, &ua) {
            return reply(false, Some("the video button is off here".into()));
        }
        Some(mdm_ipc::Command {
            video_page: Some(m.url.clone()),
            referer: m.referrer.clone().filter(|r| !r.is_empty()),
            cookies: m.cookies.clone().filter(|c| !c.is_empty()),
            user_agent: m.user_agent.clone(),
            ..Default::default()
        })
    } else {
        None
    };
    if let Some(cmd) = panel_cmd {
        return match deliver(&cmd) {
            Ok(()) => reply(true, None),
            Err(e) => reply(false, Some(e)),
        };
    }
    if !downloadable(&m.url) {
        return reply(false, Some("not an http(s) or ftp(s) URL".into()));
    }
    if let Err(why) = verdict(&m, rules) {
        return reply(
            false,
            Some(format!("left to the browser by Options ({why:?})")),
        );
    }
    match deliver(&to_command(m)) {
        Ok(()) => reply(true, None),
        Err(e) => reply(false, Some(e)),
    }
}

/// Whether Options let the browser hand `m` over; an explicit request always may.
fn verdict(
    m: &Message,
    rules: &mdm_ipc::capture::CaptureRules,
) -> Result<(), mdm_ipc::capture::Declined> {
    if m.explicit {
        return Ok(());
    }
    let ua = m.user_agent.clone().unwrap_or_default();
    rules.accepts(&m.url, &file_name_of(m), &ua)
}

/// Read one native message; None at end of input.
fn read_message(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    // Browsers send at most 4 GB, but a download request is tiny: refuse anything silly.
    if len > 1 << 20 {
        return Err(io::Error::other("message too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

fn write_message(w: &mut impl Write, json: &[u8]) -> io::Result<()> {
    w.write_all(&(json.len() as u32).to_le_bytes())?;
    w.write_all(json)?;
    w.flush()
}

fn to_command(m: Message) -> mdm_ipc::Command {
    mdm_ipc::Command {
        url: Some(m.url),
        name: m.filename.filter(|f| !f.is_empty()),
        referer: m.referrer.filter(|r| !r.is_empty()),
        cookies: m.cookies.filter(|c| !c.is_empty()),
        user_agent: m.user_agent,
        ..Default::default()
    }
}

/// Hand `cmd` to Mister Download Manager, starting it if it isn't running.
fn deliver(cmd: &mdm_ipc::Command) -> Result<(), String> {
    if mdm_ipc::send(cmd).is_ok() {
        return Ok(());
    }
    let app = exe_dir()?.join(if cfg!(windows) {
        "misterdownloadmanager.exe"
    } else {
        "misterdownloadmanager"
    });
    std::process::Command::new(&app)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", app.display()))?;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if mdm_ipc::send(cmd).is_ok() {
            return Ok(());
        }
    }
    Err("Mister Download Manager did not start".into())
}

fn exe_dir() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "no exe dir".into())
}

fn serve() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    while let Some(raw) = read_message(&mut stdin)? {
        let reply = match serde_json::from_slice::<Message>(&raw) {
            // The rules as saved now: a change in Options applies to the next download.
            Ok(m) => answer(m, &mdm_ipc::capture::CaptureRules::load()),
            Err(e) => Reply {
                ok: false,
                error: Some(e.to_string()),
                types: None,
            },
        };
        write_message(
            &mut stdout,
            &serde_json::to_vec(&reply).map_err(io::Error::other)?,
        )?;
    }
    Ok(())
}

/// The manifest Chromium browsers read: the host may serve our extension's origin only.
fn chromium_manifest() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "name": HOST_NAME,
        "description": "Mister Download Manager",
        "path": std::env::current_exe().map_err(|e| e.to_string())?,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    }))
}

/// Firefox's manifest: the same host, allowed for our add-on ID instead of an origin.
fn firefox_manifest() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "name": HOST_NAME,
        "description": "Mister Download Manager",
        "path": std::env::current_exe().map_err(|e| e.to_string())?,
        "type": "stdio",
        "allowed_extensions": [FIREFOX_ID],
    }))
}

fn write_json(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn portable_refused() -> Result<(), String> {
    if mdm_ipc::portable_dir().is_some() {
        return Err(
            "portable mode: browser integration is not registered (nothing is written outside \
             its folder); install Mister Download Manager to use it"
                .into(),
        );
    }
    Ok(())
}

/// Is this the caller the browsers start us for? Chromium passes the extension's origin first;
/// Firefox passes the manifest's path, then the add-on ID.
fn our_caller(args: &[String]) -> bool {
    let chrome = format!("chrome-extension://{EXTENSION_ID}/");
    args.first().is_some_and(|a| *a == chrome) || args.get(1).is_some_and(|a| a == FIREFOX_ID)
}

/// Windows: the manifests beside this exe, and the registry entries naming them, per user
/// (HKCU) or for everyone (HKLM).
#[cfg(windows)]
fn register(all_users: bool) -> Result<(), String> {
    portable_refused()?;
    let root = if all_users { "HKLM" } else { "HKCU" };
    let path = exe_dir()?.join(format!("{HOST_NAME}.json"));
    write_json(&path, &chromium_manifest()?)?;
    for key in REGISTRY_KEYS {
        reg(&[
            "add",
            &format!(r"{root}\{key}\{HOST_NAME}"),
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            &path.display().to_string(),
            "/f",
        ])?;
    }
    let ff_path = exe_dir()?.join(format!("{HOST_NAME}.firefox.json"));
    write_json(&ff_path, &firefox_manifest()?)?;
    reg(&[
        "add",
        &format!(r"{root}\{FIREFOX_KEY}\{HOST_NAME}"),
        "/ve",
        "/t",
        "REG_SZ",
        "/d",
        &ff_path.display().to_string(),
        "/f",
    ])?;
    println!(
        "registered {HOST_NAME} for Chrome, Edge, Brave ({}) and Firefox ({})",
        path.display(),
        ff_path.display()
    );
    Ok(())
}

#[cfg(windows)]
fn unregister(all_users: bool) -> Result<(), String> {
    let root = if all_users { "HKLM" } else { "HKCU" };
    for key in REGISTRY_KEYS.into_iter().chain([FIREFOX_KEY]) {
        let _ = reg(&["delete", &format!(r"{root}\{key}\{HOST_NAME}"), "/f"]);
    }
    let _ = std::fs::remove_file(exe_dir()?.join(format!("{HOST_NAME}.json")));
    let _ = std::fs::remove_file(exe_dir()?.join(format!("{HOST_NAME}.firefox.json")));
    println!("unregistered {HOST_NAME}");
    Ok(())
}

#[cfg(windows)]
fn reg(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("reg")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Linux and macOS: the folders each browser reads host manifests from, for one user (in the
/// home folder) or for everyone, each with whether it is Firefox's. Only browsers that are there
/// (the folder above it exists) get one.
#[cfg(not(windows))]
fn host_dirs(all_users: bool) -> Vec<(PathBuf, bool)> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut dirs: Vec<(PathBuf, bool)> = Vec::new();
    if cfg!(target_os = "macos") {
        let support = home.join("Library/Application Support");
        if all_users {
            for d in [
                "/Library/Google/Chrome/NativeMessagingHosts",
                "/Library/Application Support/Chromium/NativeMessagingHosts",
                "/Library/Microsoft/Edge/NativeMessagingHosts",
            ] {
                dirs.push((PathBuf::from(d), false));
            }
            dirs.push((
                PathBuf::from("/Library/Application Support/Mozilla/NativeMessagingHosts"),
                true,
            ));
        } else {
            for d in [
                "Google/Chrome",
                "Chromium",
                "Microsoft Edge",
                "BraveSoftware/Brave-Browser",
                "Vivaldi",
            ] {
                dirs.push((support.join(d).join("NativeMessagingHosts"), false));
            }
            dirs.push((support.join("Mozilla/NativeMessagingHosts"), true));
        }
    } else if all_users {
        for d in [
            "/etc/opt/chrome/native-messaging-hosts",
            "/etc/chromium/native-messaging-hosts",
            "/etc/opt/edge/native-messaging-hosts",
        ] {
            dirs.push((PathBuf::from(d), false));
        }
        dirs.push((
            PathBuf::from("/usr/lib/mozilla/native-messaging-hosts"),
            true,
        ));
    } else {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        for d in [
            "google-chrome",
            "chromium",
            "microsoft-edge",
            "BraveSoftware/Brave-Browser",
            "vivaldi",
        ] {
            dirs.push((config.join(d).join("NativeMessagingHosts"), false));
        }
        dirs.push((home.join(".mozilla/native-messaging-hosts"), true));
    }
    dirs.retain(|(d, _)| d.parent().is_some_and(|p| p.is_dir()));
    dirs
}

#[cfg(not(windows))]
fn register(all_users: bool) -> Result<(), String> {
    portable_refused()?;
    let (chromium, firefox) = (chromium_manifest()?, firefox_manifest()?);
    let dirs = host_dirs(all_users);
    for (dir, is_firefox) in &dirs {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let manifest = if *is_firefox { &firefox } else { &chromium };
        write_json(&dir.join(format!("{HOST_NAME}.json")), manifest)?;
        println!("registered {HOST_NAME} in {}", dir.display());
    }
    if dirs.is_empty() {
        println!("registered {HOST_NAME} for no browser: none found");
    }
    Ok(())
}

#[cfg(not(windows))]
fn unregister(all_users: bool) -> Result<(), String> {
    for (dir, _) in host_dirs(all_users) {
        let _ = std::fs::remove_file(dir.join(format!("{HOST_NAME}.json")));
    }
    println!("unregistered {HOST_NAME}");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all_users = args.iter().any(|a| a == "--all-users");
    let result = match args.first().map(String::as_str).unwrap_or_default() {
        "--register" => register(all_users),
        "--unregister" => unregister(all_users),
        // Browsers start the host naming the caller: serve only our own extension (the
        // manifests' allowed_origins / allowed_extensions say the same; this is a second check).
        _ if our_caller(&args) => serve().map_err(|e| e.to_string()),
        _ => Err(format!(
            "refusing caller {args:?}: not the Mister Download Manager extension"
        )),
    };
    if let Err(e) = result {
        eprintln!("misterdownloadmanager-nmhost: {e}");
        std::process::exit(1);
    }
}

/// The addresses MDM downloads (mdm_engine::supported): http(s), and ftp links sent from a page
/// (browsers no longer download ftp themselves).
fn downloadable(url: &str) -> bool {
    let scheme = url.split_once("://").map_or("", |(s, _)| s);
    ["http", "https", "ftp", "ftps", "ftpes"]
        .iter()
        .any(|k| scheme.eq_ignore_ascii_case(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_and_ftp_links_are_downloadable() {
        assert!(downloadable("https://x.org/a.zip") && downloadable("FTP://x.org/a.iso"));
        assert!(downloadable("ftps://x/a") && downloadable("ftpes://x/a"));
        assert!(!downloadable("file:///C:/a") && !downloadable("javascript:alert(1)"));
    }

    #[test]
    fn only_our_extension_in_either_browser() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert!(our_caller(&s(&[&format!(
            "chrome-extension://{EXTENSION_ID}/"
        )])));
        assert!(our_caller(&s(&[r"C:\x\host.firefox.json", FIREFOX_ID])));
        assert!(!our_caller(&s(&["chrome-extension://someoneelse/"])));
        assert!(!our_caller(&s(&[r"C:\x\host.json", "evil@example.com"])));
        assert!(!our_caller(&[]));
    }

    #[test]
    fn options_decide_what_is_taken() {
        let mut rules = mdm_ipc::capture::CaptureRules::default();
        let ua = "Mozilla/5.0 (Windows NT 10.0) Chrome/141.0.0.0 Safari/537.36";
        let msg = |url: &str, name: &str| Message {
            url: url.into(),
            filename: Some(name.into()),
            user_agent: Some(ua.into()),
            ..Default::default()
        };
        // Not a listed type: left to the browser, nothing delivered.
        let r = answer(msg("https://example.com/page.html", "page.html"), &rules);
        assert!(!r.ok && r.error.unwrap().contains("FileType"));
        // An excluded site.
        let r = answer(
            msg("https://dl.update.microsoft.com/kb.exe", "kb.exe"),
            &rules,
        );
        assert!(!r.ok && r.error.unwrap().contains("Site"));
        // The browser off in Options.
        rules.browsers = vec![("Google Chrome".into(), false)];
        let r = answer(
            msg(
                "https://example.com/a.zip",
                "C:\\Users\\u\\Downloads\\a.zip",
            ),
            &rules,
        );
        assert!(!r.ok && r.error.unwrap().contains("Browser"));
        // The rules themselves.
        let r = answer(
            Message {
                rules: true,
                ..Default::default()
            },
            &rules,
        );
        assert!(r.ok && r.types.unwrap().contains("ZIP"));
        assert_eq!(file_name_of(&msg("https://x.org/f/b.rar?x=1", "")), "b.rar");
        // Right-click → Download with Mister Download Manager: any link, whatever Options say.
        let mut page = msg("https://example.com/page.html", "");
        assert!(verdict(&page, &rules).is_err());
        page.explicit = true;
        assert!(verdict(&page, &rules).is_ok());
        rules.advanced_integration = false;
        assert!(verdict(&page, &rules).is_ok());
    }

    #[test]
    fn the_video_button_asks_the_rules() {
        let ua = "Mozilla/5.0 (Windows NT 10.0) Chrome/141.0.0.0 Safari/537.36";
        let mut rules = mdm_ipc::capture::CaptureRules {
            video_panel_sites: "*.quiet.test".into(),
            ..Default::default()
        };
        let ask = |page: &str| Message {
            panel: true,
            url: page.into(),
            user_agent: Some(ua.into()),
            ..Default::default()
        };
        assert!(answer(ask("https://videos.test/watch"), &rules).ok);
        assert!(!answer(ask("https://www.quiet.test/watch"), &rules).ok);
        rules.video_panel = false;
        assert!(!answer(ask("https://videos.test/watch"), &rules).ok);
        // Clicked where it's off (the page was open before the change): refused, nothing sent.
        let click = Message {
            video: true,
            url: "https://videos.test/watch".into(),
            user_agent: Some(ua.into()),
            ..Default::default()
        };
        let r = answer(click, &rules);
        assert!(!r.ok && r.error.unwrap().contains("off"));
        // From the wire: the extension's camelCase names.
        let m: Message =
            serde_json::from_str(r#"{"hideSite":"tube.test","openOptions":false}"#).unwrap();
        assert_eq!(m.hide_site.as_deref(), Some("tube.test"));
    }

    #[test]
    fn framing_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, br#"{"url":"https://x/a.zip"}"#).unwrap();
        assert_eq!(&buf[..4], &25u32.to_le_bytes());
        let mut r = &buf[..];
        assert_eq!(
            read_message(&mut r).unwrap().unwrap(),
            br#"{"url":"https://x/a.zip"}"#
        );
        assert!(
            read_message(&mut r).unwrap().is_none(),
            "clean end of input"
        );
    }

    #[test]
    fn refuses_huge_messages() {
        let mut r: &[u8] = &(2u32 << 20).to_le_bytes();
        assert!(read_message(&mut r).is_err());
    }

    #[test]
    fn message_to_command() {
        let m: Message = serde_json::from_str(
            r#"{"url":"https://x/a.zip","referrer":"https://x/","cookies":"a=1","userAgent":"UA","filename":""}"#,
        )
        .unwrap();
        let c = to_command(m);
        assert_eq!(c.url.as_deref(), Some("https://x/a.zip"));
        assert_eq!(c.referer.as_deref(), Some("https://x/"));
        assert_eq!(c.cookies.as_deref(), Some("a=1"));
        assert_eq!(c.user_agent.as_deref(), Some("UA"));
        assert_eq!(
            c.name, None,
            "empty filename means: let Mister Download Manager decide"
        );
        assert!(!c.later && !c.silent);
    }
}

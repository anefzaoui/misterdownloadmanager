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
use mdm_ipc::extension::{FIREFOX_ID, STORE_ID, STORE_UPDATE_URL, chromium_ids, store_id};
/// Where each Chromium browser looks for native messaging hosts on Windows, under HKCU (one
/// user) or HKLM (everyone). Chromium-branded builds (Chromium, Helium, Thorium) read
/// Chromium's first, then Chrome's, as most other forks do (Chromium's launch_context_win.cc).
#[cfg(windows)]
const REGISTRY_KEYS: [&str; 4] = [
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Chromium\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
    r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
];
/// Where Firefox looks.
#[cfg(windows)]
const FIREFOX_KEY: &str = r"Software\Mozilla\NativeMessagingHosts";
/// Chrome's external extensions: an entry naming the store's extension makes Chrome offer it at
/// its next start ("New extension added"), the user turning it on there. HKLM's is read from the
/// 32-bit view on 64-bit Windows, hence /reg:32 (HKCU has one view).
#[cfg(windows)]
const CHROME_EXTERNAL_KEY: &str = r"Software\Google\Chrome\Extensions";

/// A message from the extension.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
struct Message {
    /// Only asking for the capture rules (the file types to offer). `hello` (the extension
    /// installed, or the browser started) asks the same and tells the app it is there.
    rules: bool,
    hello: bool,
    /// The extension's version.
    v: Option<String>,
    url: String,
    referrer: Option<String>,
    cookies: Option<String>,
    user_agent: Option<String>,
    /// `navigator.userAgentData` brands: Brave and Opera name themselves there, not in the
    /// User-Agent; Firefox's forks add the name `runtime.getBrowserInfo` gives ("Zen").
    brands: Vec<String>,
    filename: Option<String>,
    /// Where the browser's redirects from `url` ended.
    final_url: Option<String>,
    /// The browser's own download: its size (bytes, when known) and type.
    size: Option<u64>,
    mime: Option<String>,
    /// A form's download: the body to post again, and its Content-Type.
    post: Option<String>,
    post_type: Option<String>,
    /// From a private window.
    private: bool,
    /// The user asked for this download (right-click → Download with Mister Download Manager):
    /// Options' automatic-capture rules don't apply, as IDM's "Download with IDM" takes any link.
    explicit: bool,
    /// May the download button show over videos on page `url`?
    panel: bool,
    /// The download button was clicked: `url` is the video's page, `src` the video's own
    /// address when the page plays a plain file (not a stream assembled in the page).
    video: bool,
    src: Option<String>,
    /// The button's menu: keep it off this site (a host name).
    hide_site: Option<String>,
    /// The button's menu: open its settings in Options (`page`: the toolbar button's popup asks
    /// for `general`).
    open_options: bool,
    page: Option<String>,
    /// The right-click menu: these links of a page or its selection.
    links: Vec<String>,
    /// The toolbar button: leave this site (a host name) to the browser.
    exclude_site: Option<String>,
    /// The toolbar button: bring Mister Download Manager's window up.
    open: bool,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Why it failed, for the extension to say and to link the help page's section
    /// (docs/browser-help.md): `not-running`, `too-large`, `refused`, `options`.
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<&'static str>,
    /// Mister Download Manager's version (this host comes with it).
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'static str>,
    /// For `rules`: Options → File types.
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<String>,
    /// For `rules`: whether this browser's downloads are taken at all (integration on, the
    /// browser captured), and the sites and addresses left to the browser; the extension leaves
    /// those alone without asking (the host checks again at each hand-off).
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    excluded_sites: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    excluded_addresses: Option<Vec<String>>,
    /// For `rules`: also take listed types the browser would open itself (a PDF, an MP3).
    #[serde(skip_serializing_if = "Option::is_none")]
    opened: Option<bool>,
    /// For `rules`: files smaller than this (KB) stay with the browser.
    #[serde(skip_serializing_if = "Option::is_none")]
    min_kb: Option<u64>,
}

impl Reply {
    fn done() -> Reply {
        Reply {
            ok: true,
            ..Default::default()
        }
    }

    fn failed(why: &'static str, error: impl Into<String>) -> Reply {
        Reply {
            why: Some(why),
            error: Some(error.into()),
            ..Default::default()
        }
    }
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

/// The browser `m` comes from, by the names Options lists.
fn browser(m: &Message) -> &'static str {
    mdm_ipc::capture::browser_named(m.user_agent.as_deref().unwrap_or_default(), &m.brands)
}

/// The answer to one message, under `rules` (the app's saved Options).
fn answer(m: Message, rules: &mdm_ipc::capture::CaptureRules) -> Reply {
    let browser = browser(&m);
    if m.rules || m.hello {
        return Reply {
            ok: true,
            version: Some(env!("CARGO_PKG_VERSION")),
            types: Some(rules.auto_types.clone()),
            capture: Some(rules.captures(browser)),
            excluded_sites: Some(rules.excluded_sites.clone()),
            excluded_addresses: Some(rules.excluded_addresses.clone()),
            opened: Some(rules.capture_opened),
            min_kb: Some(rules.capture_min_kb),
            ..Default::default()
        };
    }
    // The video button: asked about, clicked, its menu. What it sends goes to the app (started
    // if need be), which opens its window or keeps the setting.
    if m.panel {
        return Reply {
            ok: rules.panel_on(&m.url, browser),
            ..Default::default()
        };
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
        let page = match m.page.as_deref() {
            Some("general") => "options:general",
            _ => "options:panels",
        };
        Some(mdm_ipc::Command {
            show: Some(page.into()),
            ..Default::default()
        })
    } else if m.open {
        // An empty command: the running app comes to the front (a second launch's).
        Some(mdm_ipc::Command::default())
    } else if let Some(host) = m
        .exclude_site
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        Some(mdm_ipc::Command {
            exclude_site: Some(host.to_owned()),
            ..Default::default()
        })
    } else if !m.links.is_empty() {
        let links: Vec<String> = m
            .links
            .iter()
            .filter(|l| downloadable(l))
            .cloned()
            .collect();
        if links.is_empty() {
            return Reply::failed("refused", "no link MDM downloads");
        }
        Some(mdm_ipc::Command {
            links,
            referer: m.referrer.clone().filter(|r| !r.is_empty()),
            cookies: m.cookies.clone().filter(|c| !c.is_empty()),
            user_agent: m.user_agent.clone(),
            private: m.private,
            ..Default::default()
        })
    } else if m.video {
        if !(m.url.starts_with("http://") || m.url.starts_with("https://")) {
            return Reply::failed("refused", "not an http(s) page");
        }
        if !rules.panel_on(&m.url, browser) {
            return Reply::failed("options", "the video button is off here");
        }
        // The video clicked, not the page's first one (a feed plays many): its own file when
        // there is one, fetched as from its page.
        let src = m
            .src
            .clone()
            .filter(|s| s.starts_with("http://") || s.starts_with("https://"));
        let referer = match &src {
            Some(_) => Some(m.url.clone()),
            None => m.referrer.clone().filter(|r| !r.is_empty()),
        };
        Some(mdm_ipc::Command {
            video_page: Some(src.unwrap_or_else(|| m.url.clone())),
            referer,
            cookies: m.cookies.clone().filter(|c| !c.is_empty()),
            user_agent: m.user_agent.clone(),
            private: m.private,
            ..Default::default()
        })
    } else {
        None
    };
    if let Some(cmd) = panel_cmd {
        return delivered(&cmd);
    }
    if !downloadable(&m.url) {
        return Reply::failed("refused", "not an http(s) or ftp(s) URL");
    }
    if let Err(why) = verdict(&m, rules) {
        return Reply::failed(
            "options",
            format!("left to the browser by Options ({why:?})"),
        );
    }
    delivered(&to_command(m))
}

/// `cmd` handed to the app: the reply saying so, or why not.
fn delivered(cmd: &mdm_ipc::Command) -> Reply {
    match deliver(cmd) {
        Ok(()) => Reply::done(),
        Err(Undelivered::NotRunning(e)) => Reply::failed("not-running", e),
        Err(Undelivered::Refused(e)) if e.starts_with("too-large") => Reply::failed("too-large", e),
        Err(Undelivered::Refused(e)) => Reply::failed("refused", e),
    }
}

/// Whether Options let the browser hand `m` over; an explicit request always may. Both
/// addresses must pass when redirects led elsewhere (an excluded site sending to a CDN).
fn verdict(
    m: &Message,
    rules: &mdm_ipc::capture::CaptureRules,
) -> Result<(), mdm_ipc::capture::Declined> {
    if m.explicit {
        return Ok(());
    }
    let (name, browser) = (file_name_of(m), browser(m));
    rules.accepts(&m.url, &name, browser)?;
    if rules.too_small(m.size) {
        return Err(mdm_ipc::capture::Declined::Small);
    }
    match m.final_url.as_deref().filter(|f| *f != m.url) {
        Some(f) => rules.accepts(f, &name, browser),
        None => Ok(()),
    }
}

/// Read one native message; None at end of input.
/// Read one native message; None at end of input. `Err` inside: a message over the limit (its
/// size), read past and dropped, to be answered "too-large".
fn read_message(r: &mut impl Read) -> io::Result<Option<Result<Vec<u8>, usize>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    // Browsers send at most 4 GB, but a download request is small: refuse anything silly (the
    // app's channel takes this much, so whatever is read here reaches it). Read past it, so
    // the extension hears why instead of a closed host.
    if len > mdm_ipc::MAX_MESSAGE {
        io::copy(&mut r.take(len as u64), &mut io::sink())?;
        return Ok(Some(Err(len)));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(Ok(buf)))
}

fn write_message(w: &mut impl Write, json: &[u8]) -> io::Result<()> {
    w.write_all(&(json.len() as u32).to_le_bytes())?;
    w.write_all(json)?;
    w.flush()
}

fn to_command(m: Message) -> mdm_ipc::Command {
    let some = |s: Option<String>| s.filter(|s| !s.is_empty());
    // The browser's file name, without its folder.
    let name = some(m.filename).and_then(|f| {
        f.rsplit(['\\', '/'])
            .next()
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
    });
    mdm_ipc::Command {
        final_url: some(m.final_url).filter(|f| *f != m.url),
        url: Some(m.url),
        name,
        referer: some(m.referrer),
        cookies: some(m.cookies),
        user_agent: m.user_agent,
        size: m.size.filter(|s| *s > 0),
        mime: some(m.mime),
        post_type: m.post.as_ref().and(some(m.post_type)),
        post: m.post,
        private: m.private,
        ..Default::default()
    }
}

/// Why a command didn't reach the app.
enum Undelivered {
    /// It isn't running and couldn't be started (in time).
    NotRunning(String),
    /// It is running and said no (or stopped answering): starting another can't help.
    Refused(String),
}

/// How long a Mister Download Manager just started gets to open its channel (a first start, a
/// slow disk or an antivirus scan can take a while); the browser's download waits meanwhile.
const START_WAIT: Duration = Duration::from_secs(20);

/// Hand `cmd` to Mister Download Manager, starting it only if none is running.
fn deliver(cmd: &mdm_ipc::Command) -> Result<(), Undelivered> {
    match mdm_ipc::send(cmd) {
        Ok(()) => return Ok(()),
        Err(mdm_ipc::SendError::Refused(e)) => return Err(Undelivered::Refused(e)),
        Err(mdm_ipc::SendError::NotRunning(_)) => {}
    }
    let dir = exe_dir().map_err(Undelivered::NotRunning)?;
    // Beside the app, or (an AppImage's host, copied out of its mount: unix.rs) the path
    // written next to this copy.
    let app = match std::fs::read_to_string(dir.join("app-path")) {
        Ok(path) if !path.trim().is_empty() => PathBuf::from(path.trim()),
        _ => dir.join(if cfg!(windows) {
            "misterdownloadmanager.exe"
        } else {
            "misterdownloadmanager"
        }),
    };
    let mut child = start_detached(&app)
        .map_err(|e| Undelivered::NotRunning(format!("cannot start {}: {e}", app.display())))?;
    let started = std::time::Instant::now();
    while started.elapsed() < START_WAIT {
        std::thread::sleep(Duration::from_millis(100));
        match mdm_ipc::send(cmd) {
            Ok(()) => return Ok(()),
            Err(mdm_ipc::SendError::Refused(e)) => return Err(Undelivered::Refused(e)),
            Err(mdm_ipc::SendError::NotRunning(_)) => {}
        }
        // It quit (another copy took over, or it failed to start): that one may answer still.
        if child.try_wait().is_ok_and(|s| s.is_some()) && started.elapsed() > START_WAIT / 4 {
            break;
        }
    }
    Err(Undelivered::NotRunning(
        "Mister Download Manager did not start".into(),
    ))
}

/// Start the app outside the browser's reach: browsers end a host's whole process tree (on
/// Windows a job object, elsewhere its process group) once it has answered, which would end an
/// app this host started along with it (MDN, "Native messaging": closing the native app).
fn start_detached(app: &std::path::Path) -> io::Result<std::process::Child> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(app);
    // The browser's message pipes are this host's standard streams: never the app's.
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // Leaving the job is refused when the job doesn't allow it: then without.
        if let Ok(child) = cmd
            .creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
        {
            return Ok(child);
        }
        cmd.creation_flags(flags);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
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
        let raw = match raw {
            Ok(raw) => raw,
            Err(len) => {
                let reply = Reply::failed("too-large", format!("a message of {len} bytes"));
                write_message(
                    &mut stdout,
                    &serde_json::to_vec(&reply).map_err(io::Error::other)?,
                )?;
                continue;
            }
        };
        let reply = match serde_json::from_slice::<Message>(&raw) {
            // The rules as saved now: a change in Options applies to the next download.
            Ok(m) => {
                // This browser has the extension: the app stops offering it (extension.rs).
                mdm_ipc::extension::mark_seen(browser(&m), m.v.as_deref().unwrap_or_default());
                answer(m, &mdm_ipc::capture::CaptureRules::load())
            }
            Err(e) => Reply::failed("refused", e.to_string()),
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
        "allowed_origins": chromium_ids().into_iter().map(origin).collect::<Vec<_>>(),
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

/// A Chromium extension's origin, as browsers name the caller.
fn origin(id: &str) -> String {
    format!("chrome-extension://{id}/")
}

/// Is this the caller the browsers start us for? Chromium passes the extension's origin first
/// (unpacked or from the store); Firefox passes the manifest's path, then the add-on ID.
fn our_caller(args: &[String]) -> bool {
    args.first()
        .is_some_and(|a| chromium_ids().iter().any(|id| *a == origin(id)))
        || args.get(1).is_some_and(|a| a == FIREFOX_ID)
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
    // Chrome offers the store's extension once it is listed (never a placeholder ID).
    if let Some(id) = store_id() {
        reg(&[
            "add",
            &format!(r"{root}\{CHROME_EXTERNAL_KEY}\{id}"),
            "/v",
            "update_url",
            "/t",
            "REG_SZ",
            "/d",
            STORE_UPDATE_URL,
            "/f",
            "/reg:32",
        ])?;
    }
    println!(
        "registered {HOST_NAME} for Chrome, Edge, Brave, Helium and the other Chromium browsers \
         ({}), and for Firefox, Zen and its other forks ({})",
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
    let _ = reg(&[
        "delete",
        &format!(r"{root}\{CHROME_EXTERNAL_KEY}\{STORE_ID}"),
        "/f",
        "/reg:32",
    ]);
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

/// Linux and macOS: the folders the browsers there read host manifests from, for one user (in
/// the home folder) or for everyone, each with whether it is Firefox's (`mdm_ipc::browsers`).
#[cfg(not(windows))]
fn host_dirs(all_users: bool) -> Vec<(PathBuf, bool)> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    mdm_ipc::browsers::host_dirs(&home, &config, cfg!(target_os = "macos"), all_users)
        .into_iter()
        .map(|(d, f)| (d, f == mdm_ipc::browsers::Family::Gecko))
        .collect()
}

/// Linux and macOS: the folder Chrome reads external extensions from (an `<id>.json` naming
/// the store's update service makes Chrome offer that extension at its next start), when
/// Chrome is there. macOS has one per user and one for everyone; Linux Chrome reads only the
/// system's, so a per-user registration there leaves it to the app's own offer (extension.rs).
#[cfg(not(windows))]
fn chrome_external_dir(all_users: bool) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let (dir, chrome) = if cfg!(target_os = "macos") {
        if all_users {
            (
                PathBuf::from("/Library/Application Support/Google/Chrome/External Extensions"),
                PathBuf::from("/Applications/Google Chrome.app"),
            )
        } else {
            let chrome = home.join("Library/Application Support/Google/Chrome");
            (chrome.join("External Extensions"), chrome)
        }
    } else if all_users {
        (
            PathBuf::from("/usr/share/google-chrome/extensions"),
            PathBuf::from("/opt/google/chrome"),
        )
    } else {
        return None;
    };
    chrome.exists().then_some(dir)
}

#[cfg(not(windows))]
fn register(all_users: bool) -> Result<(), String> {
    portable_refused()?;
    let (chromium, firefox) = (chromium_manifest()?, firefox_manifest()?);
    let dirs = host_dirs(all_users);
    // One browser's folder that can't be written doesn't keep the others from theirs.
    let mut failed = Vec::new();
    for (dir, is_firefox) in &dirs {
        let manifest = if *is_firefox { &firefox } else { &chromium };
        let written = std::fs::create_dir_all(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))
            .and_then(|()| write_json(&dir.join(format!("{HOST_NAME}.json")), manifest));
        match written {
            Ok(()) => println!("registered {HOST_NAME} in {}", dir.display()),
            Err(e) => failed.push(e),
        }
    }
    if dirs.is_empty() {
        println!("registered {HOST_NAME} for no browser: none found");
    }
    if !failed.is_empty() {
        return Err(failed.join("; "));
    }
    if let (Some(dir), Some(id)) = (chrome_external_dir(all_users), store_id()) {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write_json(
            &dir.join(format!("{id}.json")),
            &serde_json::json!({ "external_update_url": STORE_UPDATE_URL }),
        )?;
        println!("offered the extension to Chrome in {}", dir.display());
    }
    Ok(())
}

#[cfg(not(windows))]
fn unregister(all_users: bool) -> Result<(), String> {
    for (dir, _) in host_dirs(all_users) {
        let _ = std::fs::remove_file(dir.join(format!("{HOST_NAME}.json")));
    }
    if let Some(dir) = chrome_external_dir(all_users) {
        let _ = std::fs::remove_file(dir.join(format!("{STORE_ID}.json")));
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
        for id in chromium_ids() {
            assert!(our_caller(&s(&[&format!("chrome-extension://{id}/")])));
        }
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
    fn the_browsers_whole_context_reaches_the_app() {
        let m: Message = serde_json::from_str(
            r#"{"url":"https://x.test/get?id=1","finalUrl":"https://cdn.test/f/abc","filename":"C:\\dl\\report.pdf",
                "size":12345,"mime":"application/pdf","post":"token=abc","postType":"application/x-www-form-urlencoded",
                "private":true,"userAgent":"UA","brands":["Brave"],"v":"0.2.5"}"#,
        )
        .unwrap();
        assert_eq!(browser(&m), "Brave Browser");
        let c = to_command(m);
        assert_eq!(
            c.url.as_deref(),
            Some("https://x.test/get?id=1"),
            "the address asked for"
        );
        assert_eq!(c.final_url.as_deref(), Some("https://cdn.test/f/abc"));
        assert_eq!(
            c.name.as_deref(),
            Some("report.pdf"),
            "without the browser's folder"
        );
        assert_eq!(
            (c.size, c.mime.as_deref()),
            (Some(12345), Some("application/pdf"))
        );
        assert_eq!(c.post.as_deref(), Some("token=abc"));
        assert_eq!(
            c.post_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
        assert!(c.private);
        // No form: no Content-Type for one.
        let m: Message = serde_json::from_str(
            r#"{"url":"https://x.test/a.zip","postType":"text/plain","size":0}"#,
        )
        .unwrap();
        let c = to_command(m);
        assert_eq!((c.post_type, c.size, c.final_url), (None, None, None));
    }

    #[test]
    fn redirects_are_checked_at_both_ends() {
        let rules = mdm_ipc::capture::CaptureRules::default();
        let ua = "Mozilla/5.0 (Windows NT 10.0) Chrome/141.0.0.0 Safari/537.36";
        let mut m = Message {
            url: "https://example.com/get/a.zip".into(),
            final_url: Some("https://download.windowsupdate.com/a.zip".into()),
            filename: Some("a.zip".into()),
            user_agent: Some(ua.into()),
            ..Default::default()
        };
        assert_eq!(verdict(&m, &rules), Err(mdm_ipc::capture::Declined::Site));
        m.final_url = Some("https://cdn.example.com/a.zip".into());
        assert_eq!(verdict(&m, &rules), Ok(()));
    }

    #[test]
    fn rules_say_everything_the_extension_needs() {
        let mut rules = mdm_ipc::capture::CaptureRules::default();
        let hello = || Message {
            hello: true,
            user_agent: Some("Mozilla/5.0 Firefox/145.0".into()),
            ..Default::default()
        };
        let r = answer(hello(), &rules);
        assert!(r.ok && r.capture == Some(true) && r.opened == Some(true));
        assert_eq!(r.version, Some(env!("CARGO_PKG_VERSION")));
        assert!(r.excluded_sites.unwrap().contains("windowsupdate"));
        rules.browsers = vec![("Mozilla Firefox".into(), false)];
        assert_eq!(
            answer(hello(), &rules).capture,
            Some(false),
            "this browser off"
        );
        // The wire names.
        let json = serde_json::to_string(&answer(hello(), &rules)).unwrap();
        assert!(json.contains("\"excludedSites\"") && json.contains("\"capture\":false"));
        // Links that aren't downloads are refused without anything sent.
        let r = answer(
            Message {
                links: vec!["javascript:void(0)".into(), "mailto:a@b".into()],
                ..Default::default()
            },
            &rules,
        );
        assert!(!r.ok && r.why == Some("refused"));
    }

    #[test]
    fn framing_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, br#"{"url":"https://x/a.zip"}"#).unwrap();
        assert_eq!(&buf[..4], &25u32.to_le_bytes());
        let mut r = &buf[..];
        assert_eq!(
            read_message(&mut r).unwrap().unwrap().unwrap(),
            br#"{"url":"https://x/a.zip"}"#
        );
        assert!(
            read_message(&mut r).unwrap().is_none(),
            "clean end of input"
        );
    }

    #[test]
    fn huge_messages_are_read_past_and_said() {
        let big = 2usize << 20;
        let mut input = (big as u32).to_le_bytes().to_vec();
        input.extend(vec![b'x'; big]);
        write_message(&mut input, br#"{"rules":true}"#).unwrap();
        let mut r = &input[..];
        assert_eq!(read_message(&mut r).unwrap(), Some(Err(big)));
        assert!(
            matches!(read_message(&mut r).unwrap(), Some(Ok(_))),
            "the next one reads"
        );
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

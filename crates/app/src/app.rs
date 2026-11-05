//! The main window (notes/ui-spec/main-window.md): menu bar, toolbar, Categories tree and the
//! download list at IDM 6.42's measured positions, plus the download actions behind them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use base64::Engine as _;
use egui::{Pos2, Ui, pos2};
use mdm_engine::state::state_path;
use mdm_engine::{Destination, Download, Options, Request};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use crate::chrome::{
    self, IconKey, MenuEntry, PopupResult, ShellIcons, ToolbarEvent, TreeEvent, TreeRow,
};
use crate::dialogs::{self, AddUrl, Complete, FileInfo, Outcome, Start};
use crate::icons::Icon;
use crate::list;
use crate::model::{self, Category, Item, Live, Status};
use crate::progress::{self, Progress};
use crate::queue::{self, Member, Queue};
use crate::settings::Settings;
use crate::store;
use crate::winui;
use mdm_ipc as ipc;

/// A node of the Categories tree, what the list shows (saved: the last one comes back).
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum View {
    All,
    Cat(Category),
    Unfinished(Option<Category>),
    Finished(Option<Category>),
    /// A queue (index into `App::queues`).
    Queue(usize),
}

/// What was being added when its address turned out to be in the list already.
enum Pending {
    /// From the browser or the command line.
    Command(Box<ipc::Command>),
    /// From Add URL.
    Url(dialogs::NewUrl),
}

/// Menu, toolbar and context-menu commands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cmd {
    AddUrl,
    /// Help → a page of the website (`""` = home).
    HelpPage(&'static str),
    Shortcuts,
    ReportProblem,
    CheckUpdates,
    About,
    TellFriend,
    Exit,
    Stop,
    Remove,
    /// Shift+Delete: Remove with "Also delete the files from disk" ticked.
    RemoveWithFiles,
    Resume,
    StopAll,
    DeleteCompleted,
    /// Start / stop a queue (index into `App::queues`).
    StartQueue(usize),
    StopQueue(usize),
    /// Queue right-click → Edit queue (its files) / Schedule; Delete; Create new queue.
    /// Tasks → Show drop target (shown / hidden); the drop target's Show main window.
    ToggleDropTarget,
    ShowMain,
    /// Tasks → Export / Import (D-021): IDM's export file (.ef2) or a text file.
    Export {
        ef2: bool,
    },
    Import {
        ef2: bool,
    },
    /// Download again from the start (File menu, right-click), after asking.
    Redownload,
    /// Move the file or rename it (right-click, Ctrl+M).
    MoveRename,
    /// Categories right-click (D-021): Add category, Properties, Delete (the user's only).
    AddCategory,
    CategoryProps(Category),
    DeleteCategory(Category),
    EditQueue(usize),
    ScheduleQueue(usize),
    DeleteQueue(usize),
    NewQueue,
    /// Categories right-click → Browse: open the category's folder.
    Browse(Category),
    /// List right-click → On Double click → Open (true) / Properties.
    DoubleClickOpen(bool),
    /// Right-click → Add to queue → queue index; Delete from queue.
    AddToQueue(usize),
    RemoveFromQueue,
    ToggleCategories,
    /// View → Dark Mode support.
    ToggleDarkMode,
    /// View → Always dark (#60).
    ToggleAlwaysDark,
    /// View → Language: a catalog's code, or follow Windows (`None`).
    Language(Option<&'static str>),
    /// View → Language...: the language picker.
    ChooseLanguage,
    Open,
    OpenWith,
    OpenFolder,
    Properties,
    /// Show or hide a list column (data slot).
    ToggleColumn(usize),
    /// Downloads → Speed Limiter.
    LimiterOn,
    LimiterOff,
    LimiterSettings,
    /// Downloads → Scheduler (also the toolbar button).
    Scheduler,
    /// Downloads → Options (also the toolbar button).
    Options,
    /// File → Add batch download (true: from the clipboard).
    Batch(bool),
    /// Tasks → Download video from a page… (yt-dlp, M9).
    VideoPage,
    /// Add torrent → Add magnet link… (M15): the Add torrent dialog, empty or with the
    /// clipboard's magnet.
    AddMagnet,
    /// Add torrent → Open torrent file…: pick a .torrent, then the Add torrent dialog.
    OpenTorrentFile,
    /// Add torrent → Torrent settings… (sharing after finishing, Windows associations).
    TorrentSettings,
    /// Right-click → Refresh download address: open the source page, edit the address.
    RefreshAddress,
    /// Right-click → Verify checksum… (beyond IDM).
    VerifyChecksum,
    /// Downloads → Find (Ctrl-F) / Find Next (F3).
    Find,
    FindNext,
    /// View → Arrange files: sort by a column (data slot), or by order of addition.
    Arrange(Option<usize>),
    /// Header right-click → Clear all filters.
    ClearFilters,
    /// Every download the list shows (Ctrl+A).
    SelectAll,
    /// The All Downloads view (an empty list's link).
    ShowAll,
    /// Header right-click → Reset columns.
    ResetColumns,
    None,
}

/// A download's standing (a gone one counts as complete: nothing to start).
fn member_in(members: &std::collections::HashMap<u64, Member>, id: u64) -> Member {
    members.get(&id).copied().unwrap_or(Member::Complete)
}

/// A file move's answer, from its thread.
type MoveDone = std::sync::mpsc::Receiver<Result<(), String>>;

/// Move a file: a rename, else a copy and the original deleted (a copy whose original can't go,
/// in use, is taken back: the file stays in one place).
fn move_file(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| e.to_string())?;
    std::fs::remove_file(from).map_err(|e| {
        let _ = std::fs::remove_file(to);
        e.to_string()
    })
}

/// A download's new place (its name from it). A name the user gave is no longer the address's
/// guess (`Item::name_from_url`); another folder alone keeps the server's say.
fn take_place(item: &mut Item, target: PathBuf) {
    if let Some(name) = target.file_name() {
        let name = name.to_string_lossy().into_owned();
        if name != item.file_name {
            item.name_from_url = false;
        }
        item.file_name = name;
    }
    item.dest = target;
}

/// The status strip's height, under the list and the Categories pane.
const STRIP_H: f32 = 22.0;

impl Cmd {
    /// The key that runs this command anywhere in the main window, as the shortcuts list writes
    /// it (Help → Keyboard shortcuts); menus show it beside the item.
    fn hint(&self) -> Option<&'static str> {
        Some(match self {
            Cmd::AddUrl => "Ctrl+N",
            Cmd::Find => "Ctrl+F",
            Cmd::FindNext => "F3",
            Cmd::MoveRename => "Ctrl+M",
            Cmd::Remove => "Delete",
            Cmd::RemoveWithFiles => "Shift+Delete",
            Cmd::Open => "Enter",
            Cmd::SelectAll => "Ctrl+A",
            Cmd::Properties => "Alt+Enter",
            _ => return None,
        })
    }
}

/// The open popup menu (other than the menu bar's); notes/ui-spec/menus.md "Context menus".
#[derive(Clone, Copy, PartialEq, Debug)]
enum Popup {
    /// List header right-click: the column chooser.
    Columns,
    /// List right-click on a row, or on blank space (None).
    Item(Option<u64>),
    /// The Start Queue (true) / Stop Queue drop-down arrows.
    QueueDrop(bool),
    /// The Add Torrent button's drop-down arrow.
    TorrentDrop,
    /// Right-click on the drop target.
    DropTarget,
    /// Categories right-click: the node's view (None: the "Queues" root).
    Node(Option<View>),
}

/// Hidden to the tray, the window's memory is given back this long after it hid (its last frames
/// done), then again this often while it stays hidden (the clock's ticks page some back in).
const TRIM_AFTER_HIDE: Duration = Duration::from_secs(2);
const TRIM_EVERY_HIDDEN: Duration = Duration::from_secs(600);

/// Hand the memory the process isn't using now back to the system (#72).
fn trim_memory() {
    #[cfg(windows)]
    crate::win32::trim_working_set();
    #[cfg(unix)]
    crate::unix::trim_heap();
}

/// Tree rows carry what they select and which branch they toggle.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Branch {
    Leaf,
    All,
    Unfinished,
    Finished,
    Queues,
}

pub struct App {
    rt: tokio::runtime::Handle,
    items: Vec<Item>,
    next_id: u64,
    /// The list's selection (several downloads, Ctrl / Shift).
    selection: crate::selection::Selection,
    view: View,
    show_categories: bool,
    /// The node the list was last drawn for (a new one starts at the top).
    list_view: View,
    /// The download Move/Rename's dialog is for (winui::start_pick answers later).
    move_rename_for: Option<u64>,
    /// The Categories tree has the keys (else the list; Tab moves between them).
    tree_keys: bool,
    /// The app's context, to reach its windows from anywhere (bring one to the front).
    ui_ctx: egui::Context,
    /// The language screen readers were last told.
    a11y_lang: &'static str,
    /// Maximized as last seen (the saved size is the restored window's, not the screen's).
    maximized: Option<bool>,
    all_open: bool,
    unfinished_open: bool,
    finished_open: bool,
    queues_open: bool,
    add_url: Option<AddUrl>,
    file_info: Option<FileInfo>,
    /// Addresses (and their browser context) waiting for Download File Info to close.
    file_info_waiting: std::collections::VecDeque<(dialogs::NewUrl, ipc::Command)>,
    /// Download File Info's open answers, for the download each was given to (by id) to start
    /// from (#28: a one-time link refuses a second request).
    kept_answers: std::collections::HashMap<u64, mdm_engine::KeptAnswer>,
    /// Help menu windows.
    about: Option<crate::help::About>,
    updates: Option<crate::help::Updates>,
    shortcuts: Option<crate::help::Shortcuts>,
    tell_friend: Option<crate::help::TellFriend>,
    /// The language picker (first start, View → Language...).
    lang_picker: Option<crate::langpick::Picker>,
    /// The video helpers' offer (the first start's, after the language).
    helpers: Option<crate::helpers::Helpers>,
    /// "Files already there", opened on its own by `/show existing-files` (screenshots and the
    /// translations' fit check); Add torrent asks it itself.
    shown_existing: Option<crate::dialogs::ExistingFiles>,
    /// The browser extension's offer (extension.rs), and whether this run has weighed it.
    extension: Option<crate::extension::Extension>,
    extension_weighed: bool,
    /// The weekly update check, while it runs.
    auto_update: Option<crate::help::Slot>,
    /// The automatic check found a new version: its window comes to the front once shown.
    raise_updates: bool,
    /// Options was asked for from outside (the extension's Button settings..., `/show`): it comes
    /// to the front once shown, or it would open behind the browser (#77).
    raise_options: bool,
    /// When MDM started (the update check waits a few seconds after it).
    started: Instant,
    /// When this run last checked for updates by itself (seconds since the epoch).
    update_checked_this_run: Option<i64>,
    progress: Vec<Progress>,
    complete: Vec<Complete>,
    settings: Settings,
    speed_settings: Option<dialogs::SpeedSettings>,
    scheduler: Option<crate::scheduler::Scheduler>,
    options: Option<crate::options::OptionsDialog>,
    /// Downloads to finish off once stopped: Download File Info was cancelled after an early
    /// start (`None`: the download and its data go), or its file was given another place
    /// (`Some((dest, start))`: the data so far goes, it starts over there when `start`).
    after_stop: Vec<(u64, Option<(PathBuf, bool)>)>,
    /// Tasks → Show drop target: the floating box links are dragged onto.
    drop_target: Option<crate::droptarget::DropTarget>,
    /// Links and files dropped on the main window.
    drops: crate::droptarget::Drops,
    /// The queue selection panel after Download Later / a batch: the downloads waiting for it.
    queue_pick: Option<(Vec<u64>, dialogs::QueuePick)>,
    /// Tasks → Import: the addresses found, to check before adding.
    import: Option<crate::transfer::ImportDialog>,
    /// Add category / Properties from the Categories tree.
    category_dlg: Option<crate::categories::CategoryDialog>,
    /// The category dialog was opened by Download File Info's "+" (its result is picked there).
    category_for_file_info: bool,
    /// Delete a category of the user's, asking first.
    confirm_category_delete: Option<dialogs::DeleteCategory>,
    /// File Properties windows, one per download.
    properties: Vec<crate::properties::Properties>,
    /// Verify checksum windows, one per download.
    checksums: Vec<crate::checksum::ChecksumDialog>,
    batch: Option<crate::batch::Batch>,
    video_dlg: Option<crate::videodlg::VideoDialog>,
    /// Download a gallery (#58).
    gallery_dlg: Option<crate::gallerydlg::GalleryDialog>,
    torrent_dlg: Option<crate::torrentdlg::TorrentDialog>,
    torrent_settings: Option<crate::torrentdlg::TorrentSettings>,
    /// Since when no torrent has needed the torrent engine (`torrent_tick`).
    torrent_idle_since: Option<Instant>,
    /// What each torrent sent, run after run (`torrent_accounts`), when last counted (whole
    /// seconds), and when its totals last made the list be saved.
    accountant: crate::torrents::Accountant,
    account_at: Instant,
    account_saved: Instant,
    /// Each running torrent's bytes from peers as last seen (by item id), and the torrents'
    /// smoothed speed with when it was measured (`torrent_speed`, for the global cap).
    torrent_fetched: HashMap<u64, u64>,
    torrent_speed: (f64, Instant),
    /// The torrents' share of the global cap as last set, and when (`apply_global_limit`).
    torrent_cap: (Option<u64>, Instant),
    /// File downloads' smoothed speed, when measured, and the global limiter's byte count
    /// then (`file_speed`).
    file_speed: (f64, Instant, u64),
    /// A blocklist is being fetched (torrents::refresh_blocklist).
    blocklist_busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Startup work for torrents done (associations refreshed, sharing resumed).
    torrents_started: bool,
    /// The torrent session, started on first use (D-014).
    torrent_host: crate::torrents::Host,
    /// Options → Proxy / Socks → Network adapter (#35): the watch, the route seen last frame, and
    /// the downloads stopped because it changed, to start again when it allows.
    netwatch: crate::netwatch::NetWatch,
    route: mdm_engine::Route,
    adapter_paused: Vec<u64>,
    find: Option<crate::find::Find>,
    /// The last search, for Find Next.
    last_find: Option<crate::find::Query>,
    /// A message with OK: its window title and message: "Cannot find …", an export that failed…
    notice: Option<(&'static str, String)>,
    /// Why the last save of the download list failed (told once per reason).
    save_failed: Option<String>,
    /// Finished files being moved (Move/Rename, Properties → Move): where to, and the answer.
    moving: Vec<(u64, PathBuf, MoveDone)>,
    /// A finished queue asked to turn the computer off: the countdown, cancellable.
    turn_off: Option<dialogs::TurnOffCountdown>,
    /// A finished queue asked to exit.
    exit_requested: bool,
    /// The global speed cap shared by every download (Speed Limiter settings).
    global: std::sync::Arc<mdm_engine::limiter::Limiter>,
    /// Connection caps learned per host, shared by every download.
    host_limits: std::sync::Arc<mdm_engine::hosts::HostLimits>,
    /// Scheduler → Download limits: traffic in the window, and what the limit holds back.
    quota: crate::quota::Quota,
    /// "Show warning before stopping downloads": the notice, while open.
    limit_notice: Option<dialogs::LimitNotice>,
    /// The notification-area icon (created on the first frame), the window hidden in it, and
    /// a real exit under way (closing the window otherwise only hides it, as IDM does).
    tray: Option<crate::tray::Tray>,
    tray_limiter: Option<bool>,
    hidden: bool,
    /// Hidden to the tray: when to hand the window's memory back to the system next (#72).
    trim_when_hidden: Option<Instant>,
    exiting: bool,
    /// Clipboard watching (Options → General): the text last seen (outer None: not looked
    /// yet) and when.
    clip_seen: Option<Option<String>>,
    clip_checked: Option<Instant>,
    confirm_delete: Option<dialogs::ConfirmDelete>,
    /// Queue right-click → Delete, asking first: the queue and the question.
    confirm_queue_delete: Option<(usize, dialogs::Confirm)>,
    /// Redownload, asking first: the download and the question.
    confirm_redownload: Option<(u64, dialogs::Confirm)>,
    /// A duplicate address asking what to do: the download already there, what was being added.
    duplicate: Option<(u64, Pending, dialogs::Duplicate)>,
    /// Duplicates that came while one was being asked about, next in turn.
    duplicates_waiting: std::collections::VecDeque<(u64, Pending, dialogs::Duplicate)>,
    /// The next duplicate check is passed (the user chose to add it anyway).
    duplicate_cleared: bool,
    downloads: PathBuf,
    dirty: bool,
    last_save: Instant,
    /// Writes the list off the window's thread.
    saver: store::Saver,
    /// When the light/dark mode was last checked against Windows' app mode.
    theme_checked: Option<Instant>,
    /// The main window's caption matches the mode.
    caption_set: bool,
    list: list::ListState,
    icons: ShellIcons,
    /// Open top-level menu (index into `chrome::menus()`).
    open_menu: Option<usize>,
    /// Alt is down, and was pressed alone so far (releasing it opens the menu bar).
    alt_down: bool,
    alt_alone: bool,
    /// The open context / drop-down menu, and its top-left.
    popup: Option<(Popup, Pos2, u64)>,
    /// Counts menu opens: each open is a new menu window, even at the same place.
    popup_serial: u64,
    /// The column layout last seen, to notice changes that need saving.
    last_layout: Option<list::Layout>,
    /// Main download queue (0), synchronization queue (1).
    queues: Vec<Queue>,
    /// Failure time and count per download, for queue retries.
    failures: HashMap<u64, (Instant, u32)>,
    /// Seconds since local midnight at the last scheduler tick.
    clock: u32,
    /// Commands from ipc.rs (command line, second launch, browser).
    commands: std::sync::mpsc::Receiver<ipc::Incoming>,
}

/// The login written in an address (`ftp://user:pass@host/…`), decoded.
fn url_login(url: &url::Url) -> Option<(String, String)> {
    let password = url.password()?;
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    Some((decode(url.username()), decode(password)))
}

/// `Authorization: Basic …` for "Use authorization".
/// What a download's requests carry: its headers (a login, the browser's Referer and Cookie, a
/// form's Content-Type) and the options its requests go with (User-Agent, proxy, a form's body).
/// Download File Info's look-up asks with the same as the download, so both get one answer.
#[derive(Clone)]
pub struct RequestParts {
    pub headers: HeaderMap,
    pub opts: Options,
}

/// The Referer a browser would send for `url` from page `referrer` (its default policy,
/// strict-origin-when-cross-origin): the whole address on the same site, only the site's origin
/// to another, nothing from https to http.
fn sent_referrer(url: &url::Url, referrer: &str) -> Option<String> {
    let from = url::Url::parse(referrer).ok()?;
    if !matches!(from.scheme(), "http" | "https") {
        return None;
    }
    if from.scheme() == "https" && url.scheme() == "http" {
        return None;
    }
    if from.origin() == url.origin() {
        let mut full = from.clone();
        full.set_fragment(None);
        let _ = full.set_username("");
        let _ = full.set_password(None);
        return Some(full.to_string());
    }
    Some(format!("{}/", from.origin().ascii_serialization()))
}

/// Marks each file the engine starts writing as downloaded from the web (Mark-of-the-Web, macOS'
/// quarantine: win32 / unix `mark_downloaded`), as browsers mark theirs. Only http(s) addresses
/// (an FTP address may carry its login).
fn mark_hook(url: &url::Url, referrer: Option<&str>) -> mdm_engine::FileHook {
    let web = matches!(url.scheme(), "http" | "https");
    let (url, referrer) = (url.to_string(), referrer.map(str::to_owned));
    mdm_engine::FileHook::new(move |path: &std::path::Path| {
        if !web {
            return;
        }
        #[cfg(windows)]
        crate::win32::mark_downloaded(path, &url, referrer.as_deref());
        #[cfg(unix)]
        crate::unix::mark_downloaded(path, &url, referrer.as_deref());
    })
}

/// The Google Drive file whose player the browser's video button was clicked over (#34): the
/// page itself when it is a Drive file's (`video_page`), or, when the button sent the player's
/// own stream (on Google's hosts), the page it came from (`referer`). None for any other video.
fn drive_file_behind_player(cmd: &ipc::Command) -> Option<String> {
    use mdm_engine::sites::gdrive;
    let drive_file = |s: &str| {
        url::Url::parse(s)
            .ok()
            .and_then(|u| gdrive::parse(&u))
            .is_some_and(|l| l.kind == gdrive::Kind::File)
    };
    let page = cmd.video_page.as_deref()?;
    if drive_file(page) {
        return Some(page.to_owned());
    }
    let googles = url::Url::parse(page).ok().and_then(|u| {
        u.host_str().map(|h| {
            let h = h.to_ascii_lowercase();
            [
                ".google.com",
                ".googleapis.com",
                ".googlevideo.com",
                ".googleusercontent.com",
            ]
            .iter()
            .any(|d| h.ends_with(d))
        })
    });
    cmd.referer
        .as_deref()
        .filter(|r| googles == Some(true) && drive_file(r))
        .map(str::to_owned)
}

pub fn auth_headers(login: Option<&(String, String)>) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Some((user, pass)) = login {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        if let Ok(v) = HeaderValue::from_str(&format!("Basic {token}")) {
            h.insert(AUTHORIZATION, v);
        }
    }
    h
}

/// Seconds since local midnight (for queue schedules).
fn seconds_now() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().num_seconds_from_midnight()
}

/// The shell icon for a file name's extension.
fn file_icon(name: &str) -> IconKey {
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => IconKey::Ext(ext.to_ascii_lowercase()),
        _ => IconKey::Ext("unknown".into()),
    }
}

/// Sort key for list column col: numbers where IDM sorts numerically, else text.
/// `queued`: the download is in a queue (the Q column's mark).
fn sort_key(it: &Item, col: usize, queued: bool) -> (f64, String) {
    let frac = it
        .size
        .filter(|s| *s > 0)
        .map_or(0.0, |s| it.downloaded as f64 / s as f64);
    match col {
        0 => (0.0, it.file_name.to_lowercase()),
        // As the column shows it: in a queue or not.
        1 => (f64::from(u8::from(queued)), String::new()),
        2 => (it.size.unwrap_or(0) as f64, String::new()),
        3 => (
            if it.status == Status::Complete {
                2.0
            } else {
                frac
            },
            String::new(),
        ),
        4 => (
            it.size.map_or(f64::MAX, |s| {
                (s.saturating_sub(it.downloaded)) as f64 / it.rate().max(1.0)
            }),
            String::new(),
        ),
        5 => (it.rate(), String::new()),
        // By time, not by the text ("Apr" would come before "Oct").
        6 => (
            it.last_try
                .as_deref()
                .and_then(model::stamp_time)
                .map_or(0.0, |t| t.and_utc().timestamp() as f64),
            String::new(),
        ),
        // Added: the order of addition itself (ids only grow), exact even without a time.
        list::ADDED => (it.id as f64, String::new()),
        list::SOURCE => (f64::from(u8::from(it.torrent.is_some())), String::new()),
        _ => (0.0, it.description.to_lowercase()),
    }
}

/// Silk icons standing in for IDM's XP-era category icons.
fn category_icon(c: Category) -> IconKey {
    IconKey::Silk(match c {
        Category::Compressed => "page_white_zip",
        Category::Documents => "page_white",
        Category::Music => "music",
        Category::Programs => "application_xp",
        Category::Video => "film",
        Category::General => "page_white",
        Category::Torrents => "folder_link",
        Category::Custom(_) => "folder",
    })
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rt: tokio::runtime::Handle,
        commands: std::sync::mpsc::Receiver<ipc::Incoming>,
        loaded: store::Loaded,
    ) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let mut settings = loaded.settings;
        if settings.limiter_on_startup {
            settings.speed_limit_on = true;
        }
        // The language (D-019): the installer's wizard says which on its first start (it always
        // leaves the value; take it so an upgrade's can't override a later choice); a portable
        // copy writes nothing to the registry. With neither, the first start asks.
        #[cfg(windows)]
        let installer = mdm_ipc::portable_dir()
            .is_none()
            .then(crate::win32::take_install_language)
            .flatten();
        #[cfg(not(windows))]
        let installer: Option<String> = None;
        if !settings.language_asked
            && let Some(code) = installer.filter(|c| crate::i18n::i18n().from_code(c).is_some())
        {
            // Windows' own language is kept as "follow Windows".
            settings.language = (code != crate::i18n::system_code()).then_some(code);
            settings.language_asked = true;
        }
        let ask_language = !settings.language_asked;
        crate::i18n::set_lang(settings.language.as_deref());
        crate::theme::language_fonts(&cc.egui_ctx);
        crate::regional::set(&settings.regional);
        if settings.proxy_mode == 2 {
            crate::pacfetch::refresh(&settings.proxy_script, &rt);
        }
        // "Launch on startup" is whatever the Run key says (the installer may have set it); on
        // Linux and macOS, the autostart entry / LaunchAgent.
        #[cfg(windows)]
        {
            settings.launch_on_startup = crate::win32::run_at_startup();
        }
        #[cfg(unix)]
        {
            settings.launch_on_startup = crate::unix::run_at_startup();
            if mdm_ipc::portable_dir().is_none() && !crate::offscreen() {
                crate::unix::register_browsers();
            }
        }
        let mut queues = loaded.queues;
        if queues.len() < 2 {
            // The built-in queues' names are saved in English and translated where shown
            // (`t(&queue.name)`; a name the user typed is not in the catalog and stays).
            queues = vec![
                Queue::new(crate::i18n::mark("Main download queue"), false),
                Queue::new(crate::i18n::mark("Synchronization queue"), true),
            ];
        }
        for q in &mut queues {
            q.running = q.schedule.start_on_startup;
        }
        let (next_id, mut items) = (loaded.next_id, loaded.items);
        settings.publish_categories();
        // D-021: torrents are a category of their own (lists from before had them by file
        // type); their files stay where they are.
        for i in &mut items {
            if i.torrent.is_some() {
                i.category = Category::Torrents;
            }
            // A user's category that is gone: General.
            if let Category::Custom(id) = i.category
                && crate::model::custom(id).is_none()
            {
                i.category = Category::General;
            }
        }
        let mut list = list::ListState::default();
        if let Some(cols) = loaded.columns {
            list.apply(cols);
        }
        let downloads = dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
            .unwrap_or_else(|| PathBuf::from("."));
        // The Categories pane as it was left, and the node shown (if it still exists).
        let [all_open, unfinished_open, finished_open, queues_open] = settings.branches;
        let known = |c: Option<Category>| c.is_none_or(|c| Category::all().contains(&c));
        let view = match settings.view {
            Some(View::Queue(q)) if q < queues.len() => View::Queue(q),
            Some(v @ View::Cat(c)) if known(Some(c)) => v,
            Some(v @ (View::Unfinished(c) | View::Finished(c))) if known(c) => v,
            _ => View::All,
        };
        let show_categories = settings.show_categories;
        App {
            rt,
            items,
            next_id,
            selection: Default::default(),
            view,
            show_categories,
            all_open,
            unfinished_open,
            finished_open,
            queues_open,
            maximized: None,
            a11y_lang: "",
            ui_ctx: cc.egui_ctx.clone(),
            tree_keys: false,
            move_rename_for: None,
            list_view: view,
            add_url: None,
            about: None,
            updates: None,
            shortcuts: None,
            tell_friend: None,
            lang_picker: ask_language.then(|| crate::langpick::Picker::new(true, None)),
            helpers: None,
            shown_existing: None,
            extension: None,
            extension_weighed: false,
            auto_update: None,
            raise_updates: false,
            raise_options: false,
            started: Instant::now(),
            update_checked_this_run: None,
            file_info: None,
            file_info_waiting: Default::default(),
            kept_answers: Default::default(),
            progress: Vec::new(),
            complete: Vec::new(),
            settings,
            speed_settings: None,
            scheduler: None,
            options: None,
            category_dlg: None,
            category_for_file_info: false,
            import: None,
            queue_pick: None,
            drop_target: None,
            drops: Default::default(),
            after_stop: Vec::new(),
            confirm_category_delete: None,
            properties: Vec::new(),
            checksums: Vec::new(),
            batch: None,
            video_dlg: None,
            gallery_dlg: None,
            torrent_dlg: None,
            torrent_settings: None,
            torrent_idle_since: None,
            accountant: Default::default(),
            account_at: Instant::now(),
            account_saved: Instant::now(),
            torrent_fetched: HashMap::new(),
            torrent_speed: (0.0, Instant::now()),
            torrent_cap: (None, Instant::now()),
            file_speed: (0.0, Instant::now(), 0),
            blocklist_busy: Default::default(),
            torrents_started: false,
            torrent_host: crate::torrents::Host::new(),
            netwatch: Default::default(),
            route: mdm_engine::Route::Any,
            adapter_paused: Vec::new(),
            find: None,
            last_find: None,
            notice: None,
            save_failed: None,
            moving: Vec::new(),
            turn_off: None,
            exit_requested: false,
            global: std::sync::Arc::new(mdm_engine::limiter::Limiter::new(None)),
            host_limits: Default::default(),
            quota: Default::default(),
            limit_notice: None,
            clip_seen: None,
            clip_checked: None,
            tray: None,
            tray_limiter: None,
            hidden: false,
            trim_when_hidden: None,
            exiting: false,
            confirm_delete: None,
            confirm_queue_delete: None,
            confirm_redownload: None,
            duplicate: None,
            duplicates_waiting: Default::default(),
            duplicate_cleared: false,
            downloads,
            dirty: false,
            last_save: Instant::now(),
            saver: store::Saver::new(),
            theme_checked: None,
            caption_set: false,
            list,
            icons: ShellIcons::default(),
            open_menu: None,
            alt_down: false,
            alt_alone: false,
            popup: None,
            popup_serial: 0,
            last_layout: None,
            queues,
            failures: HashMap::new(),
            clock: seconds_now(),
            commands,
        }
    }

    fn item(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// The one download single-download commands act on (Properties, Open…).
    fn selected_item(&self) -> Option<&Item> {
        self.selection
            .one()
            .and_then(|id| self.items.iter().find(|i| i.id == id))
    }

    /// The downloads a command acts on: those selected, in list order; a right-clicked one
    /// outside the selection alone.
    fn targets(&self, target: Option<u64>) -> Vec<u64> {
        match target {
            Some(t) if !self.selection.contains(t) => vec![t],
            _ => self.selection.ids().to_vec(),
        }
    }

    /// The downloads the selection-wide commands would act on.
    fn chosen(&self, target: Option<u64>) -> Vec<&Item> {
        self.targets(target)
            .into_iter()
            .filter_map(|id| self.items.iter().find(|i| i.id == id))
            .collect()
    }

    /// An empty list's message, centred in its rows' area `body` (English): an icon, what is
    /// empty, why, and the one thing to do about it (clear the filters, show every download,
    /// add one). Returns that command when it is clicked.
    fn empty_state(&self, ui: &Ui, body: egui::Rect) -> Option<Cmd> {
        let p = ui.painter();
        let filtered = self.list.filters.iter().any(Option::is_some) && !self.view_ids().is_empty();
        let (title, why, action, cmd) = if filtered {
            (
                // i18n: title in the empty download list, when the column filters hide every download
                "No downloads match the filters",
                // i18n: in the empty download list, under its title
                "The column filters hide every download in this view.",
                "Clear all filters",
                Cmd::ClearFilters,
            )
        } else if !self.items.is_empty() {
            (
                // i18n: title in the empty download list (this category or queue has no downloads)
                "No downloads here",
                // i18n: in the empty download list, under its title
                "This category or queue holds no downloads yet.",
                // i18n: link in the empty download list: go to the All Downloads view
                "Show all downloads",
                Cmd::ShowAll,
            )
        } else {
            (
                // i18n: title in the empty download list when there are no downloads at all
                "No downloads yet",
                // i18n: in the empty download list, under its title
                "The downloads you add, or drop on this window, show here.",
                // i18n: link in the empty download list: opens Add URL
                "Add new download",
                Cmd::AddUrl,
            )
        };
        let (title, why) = (crate::i18n::t(title), crate::i18n::t(why));
        let title_font = winui::tahoma_bold();
        let tw = winui::text_width_font(p, title_font.clone(), title);
        let ww = winui::text_width(p, why);
        let aw = winui::text_width(p, crate::i18n::t(action));
        // The block: icon 40, title, reason, link; in the middle, or under the header when
        // the list is short.
        let block_h = 40.0 + 14.0 + 16.0 + 8.0 + 13.0 + 14.0 + 13.0;
        let top = (body.center().y - block_h / 2.0).max(body.top() + 16.0);
        let cx = body.center().x;
        // A download glyph in the muted ink (outlined, as the toolbar's icons are).
        let ink = egui::Stroke::new(1.5, winui::MUTED());
        let c = winui::rect(egui::Rect::from_center_size(
            pos2(cx, top + 20.0),
            egui::vec2(1.0, 1.0),
        ))
        .center();
        p.circle_stroke(c, 19.0, ink);
        p.line_segment([c + egui::vec2(0.0, -10.0), c + egui::vec2(0.0, 9.0)], ink);
        p.line_segment([c + egui::vec2(-7.0, 2.0), c + egui::vec2(0.0, 9.0)], ink);
        p.line_segment([c + egui::vec2(7.0, 2.0), c + egui::vec2(0.0, 9.0)], ink);
        let y = top + 40.0 + 14.0 + 12.0;
        winui::text_font(p, title_font, cx - tw / 2.0, y, title, winui::TEXT());
        let y = y + 8.0 + 13.0;
        winui::text(p, cx - ww / 2.0, y, why, winui::MUTED());
        let y = y + 14.0 + 13.0;
        winui::link(ui, p, cx - aw / 2.0, y, action, "empty-action").then_some(cmd)
    }

    /// The strip under the list: how many downloads are shown and selected, how many run and
    /// how fast in all (beyond IDM, which has no status bar).
    fn status_strip(&self, ui: &Ui, o: Pos2, w: f32, h: f32, shown: usize) {
        let p = ui.painter();
        let top = o.y + h - STRIP_H;
        // Centred in the band it reads as: the strip and the 5 px gap above it, under the panes.
        let baseline = winui::baseline_in(top - 5.0, STRIP_H + 5.0);
        let n = shown as u64;
        let mut left = crate::i18n::tn(n, "{n} download", "{n} downloads");
        let selected = self.selection.ids().len() as u64;
        if selected > 1 {
            left += "   ";
            left += &crate::i18n::tn(selected, "{n} selected", "{n} selected");
        }
        winui::fit(
            ui,
            p,
            o.x + 8.0,
            baseline,
            &left,
            winui::MUTED(),
            o.x + w / 2.0,
            "strip-left",
        );
        if let Some(right) = self.activity() {
            winui::text_right(p, o.x + w - 10.0, baseline, &right, winui::MUTED());
        }
    }

    /// "2 downloading, 1.25 MB/sec" while downloads run (the status strip, the tray's tip).
    fn activity(&self) -> Option<String> {
        let running: Vec<&Item> = self.items.iter().filter(|i| i.live.is_some()).collect();
        if running.is_empty() {
            return None;
        }
        let speed: f64 = running.iter().map(|i| i.rate()).sum();
        Some(crate::i18n::fmt(
            // i18n: {count} = "2 downloading"; {speed} = all of them together, like "1.25 MB/sec"
            crate::i18n::t("{count}, {speed}"),
            &[
                (
                    "count",
                    &crate::i18n::tn(running.len() as u64, "{n} downloading", "{n} downloading"),
                ),
                ("speed", &model::rate(speed)),
            ],
        ))
    }

    /// The list's rows as drawn: the view's downloads in the list's order that pass the column
    /// filters (Find, Select all and scrolling count these, so they match what is shown).
    fn shown_ids(&self) -> Vec<u64> {
        if self.list.filters.iter().all(Option::is_none) {
            return self.view_ids();
        }
        self.list_rows().0
    }

    /// The rows the list draws ([`Self::shown_ids`]) with their cells.
    fn list_rows(&self) -> (Vec<u64>, Vec<list::Row>) {
        let queued: std::collections::HashSet<u64> = self
            .queues
            .iter()
            .flat_map(|q| q.items.iter().copied())
            .collect();
        self.view_items()
            .into_iter()
            .map(|it| (it.id, self.row(it, &queued)))
            .filter(|(_, r)| list::matches(&self.list, r))
            .unzip()
    }

    /// One download's row in the list.
    /// `queued`: the downloads in a queue (worked out once for all the rows).
    fn row(&self, it: &Item, queued: &std::collections::HashSet<u64>) -> list::Row {
        list::Row {
            status_group: match it.status {
                // i18n: a download's status (state it is in, not a command): being downloaded now
                Status::Downloading => "Downloading",
                // i18n: a download's status (state it is in, not a command): stopped by the user
                Status::Stopped => "Stopped",
                // i18n: a download's status (state it is in, not a command): waiting in a queue for its turn
                Status::Queued => "Waiting",
                // i18n: a download's status (state it is in, not a command): finished, the whole file is there
                Status::Complete => "Complete",
                // i18n: a download's status (state it is in, not a command): it failed
                Status::Error => "Error",
            },
            icon: file_icon(&it.file_name),
            queued: queued.contains(&it.id),
            cells: [
                it.file_name.clone(),
                String::new(),
                it.size.map_or(String::new(), model::size_list),
                it.status_text(),
                it.time_left(),
                if it.live.is_some() {
                    model::rate(it.rate())
                } else {
                    String::new()
                },
                it.last_try
                    .as_deref()
                    .map_or(String::new(), model::date_text),
                it.error.clone().unwrap_or_else(|| it.description.clone()),
                it.added.as_deref().map_or(String::new(), model::date_text),
                if it.torrent.is_some() {
                    // i18n: where a download comes from: BitTorrent (shown on a small colored label)
                    "Torrent".into()
                } else {
                    // i18n: where a download comes from: a web or FTP server (shown on a small colored label)
                    "Web".into()
                },
            ],
        }
    }

    /// The view's downloads in the list's order, before the column filters.
    fn view_ids(&self) -> Vec<u64> {
        self.view_items().iter().map(|i| i.id).collect()
    }

    /// [`Self::view_ids`]'s downloads themselves (each sort key worked out once, not at
    /// every comparison).
    fn view_items(&self) -> Vec<&Item> {
        let shown = self.items.iter().filter(|i| self.in_view(self.view, i));
        let Some((col, asc)) = self.list.sort else {
            return shown.collect();
        };
        let queued: std::collections::HashSet<u64> = self
            .queues
            .iter()
            .flat_map(|q| q.items.iter().copied())
            .collect();
        let mut keyed: Vec<((f64, String), &Item)> = shown
            .map(|i| (sort_key(i, col, queued.contains(&i.id)), i))
            .collect();
        keyed.sort_by(|(ka, _), (kb, _)| {
            let ord = ka.0.total_cmp(&kb.0).then_with(|| ka.1.cmp(&kb.1));
            if asc { ord } else { ord.reverse() }
        });
        keyed.into_iter().map(|(_, i)| i).collect()
    }

    /// Does `view` (a node of the Categories tree) hold this download?
    fn in_view(&self, view: View, i: &Item) -> bool {
        let in_cat = |c: Option<Category>| c.is_none_or(|c| i.category == c);
        match view {
            View::All => true,
            View::Cat(c) => i.category == c,
            View::Unfinished(c) => i.status != Status::Complete && in_cat(c),
            View::Finished(c) => i.status == Status::Complete && in_cat(c),
            View::Queue(q) => self.queues.get(q).is_some_and(|q| q.items.contains(&i.id)),
        }
    }

    // ---- actions ---------------------------------------------------------------------------

    fn add(&mut self, fi: &mut FileInfo, start: Start) {
        // Only a folder other than the category's becomes its new one (an unchanged folder
        // stays "the default", following Downloads).
        if let Some((cat, dir)) = fi.remembered()
            && std::path::Path::new(&dir) != self.settings.category_dir(cat, &self.downloads)
        {
            self.settings.set_category_dir(cat, Some(dir));
        }
        let id = self.push_item(
            fi.url.clone(),
            fi.dest(),
            fi.category,
            fi.description.clone(),
            fi.size(),
            fi.login.clone(),
            fi.context.clone(),
            if fi.context.referer.is_some() || fi.context.cookies.is_some() {
                crate::i18n::mark("from the browser")
            } else {
                crate::i18n::mark("with Add URL")
            },
        );
        self.name_guessed(id, fi.name_guessed());
        self.selection.set(id);
        if matches!(start, Start::Now) {
            // Later: the answer is closed (its download starts some other time).
            if let Some(k) = fi.take_kept() {
                self.kept_answers.insert(id, k);
            }
            self.start(id, true);
        } else {
            // "Download Later": the main download queue (IDM's default), or the one picked.
            self.queue_later(vec![id], self.settings.queue_panel_later);
        }
        self.dirty = true;
    }

    /// Options → Downloads → "Start downloading immediately while displaying Download File
    /// Info": once the address answered (name and size known) and its folder exists, the
    /// download starts behind the dialog; the dialog's answer then keeps, moves or drops it.
    fn start_early(&mut self, fi: &mut FileInfo) {
        if !self.settings.start_immediately || fi.early.is_some() || !fi.probed() {
            return;
        }
        let dest = fi.dest();
        if !dest.parent().is_some_and(std::path::Path::is_dir) {
            return; // a folder still to be created waits for the dialog's question
        }
        let id = self.push_item(
            fi.url.clone(),
            dest,
            fi.category,
            fi.description.clone(),
            fi.size(),
            fi.login.clone(),
            fi.context.clone(),
            if fi.context.referer.is_some() || fi.context.cookies.is_some() {
                crate::i18n::mark("from the browser")
            } else {
                crate::i18n::mark("with Add URL")
            },
        );
        fi.early = Some(id);
        if let Some(k) = fi.take_kept() {
            self.kept_answers.insert(id, k);
        }
        self.start(id, false);
    }

    /// Download File Info closed with Start / Later on a download that started early.
    fn adopt_early(&mut self, fi: &FileInfo, id: u64, start: Start) {
        if let Some((cat, dir)) = fi.remembered()
            && std::path::Path::new(&dir) != self.settings.category_dir(cat, &self.downloads)
        {
            self.settings.set_category_dir(cat, Some(dir));
        }
        let dest = fi.dest();
        let now = matches!(start, Start::Now);
        let Some(item) = self.item(id) else { return };
        item.category = fi.category;
        item.description = fi.description.clone();
        let moved = item.dest != dest;
        self.selection.set(id);
        if moved {
            // Another name or folder than the one it started at: the data so far goes and it
            // starts over there.
            self.stop(id);
            self.after_stop.push((id, Some((dest, now))));
        } else if now {
            let (live, complete) = self
                .items
                .iter()
                .find(|i| i.id == id)
                .map_or((false, false), |i| {
                    (i.live.is_some(), i.status == Status::Complete)
                });
            if complete {
                // It finished behind the dialog: its box now.
                if self.settings.show_complete
                    && let Some(i) = self.items.iter().find(|i| i.id == id)
                {
                    self.complete.push(Complete {
                        id,
                        url: i.url.to_string(),
                        path: i.dest.clone(),
                        size: i.size.unwrap_or(i.downloaded),
                        dont_show: false,
                        notices: i.notices.clone(),
                        seen: false,
                    });
                }
            } else if !live {
                self.start(id, true);
            } else if !self.progress.iter().any(|p| p.id == id) {
                self.progress
                    .push(Progress::new(id, self.settings.show_complete));
            }
        } else {
            // Later: paused, its data kept for the queue to resume.
            self.stop(id);
        }
        if !now {
            self.queue_later(vec![id], self.settings.queue_panel_later);
        }
        self.dirty = true;
    }

    /// Carry out `after_stop` for downloads that have stopped.
    fn finish_stopped(&mut self) {
        if self.after_stop.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.after_stop);
        for (id, then) in pending {
            if self.item(id).is_some_and(|i| i.live.is_some()) {
                self.after_stop.push((id, then));
                continue;
            }
            let Some(item) = self.item(id) else { continue };
            let old = item.dest.clone();
            let _ = std::fs::remove_file(state_path(&old));
            let _ = std::fs::remove_file(mdm_engine::naming::part_path(&old));
            match then {
                None => {
                    // Cancelled: a file that finished meanwhile goes too, with its windows.
                    if item.status == Status::Complete {
                        let _ = std::fs::remove_file(&old);
                    }
                    self.complete.retain(|c| c.id != id);
                    self.delete(id);
                }
                Some((dest, start)) => {
                    if item.status == Status::Complete {
                        let _ = std::fs::remove_file(&old);
                    }
                    if let Some(name) = dest.file_name() {
                        item.file_name = name.to_string_lossy().into_owned();
                    }
                    item.dest = dest;
                    item.downloaded = 0;
                    item.status = Status::Stopped;
                    if start {
                        self.start(id, true);
                    }
                }
            }
            self.dirty = true;
        }
    }

    /// Downloads added with Later (or by a batch): into the main queue, or the queue chosen in
    /// the selection panel when `ask` (Options → Downloads).
    fn queue_later(&mut self, ids: Vec<u64>, ask: bool) {
        if ids.is_empty() {
            return;
        }
        if ask && self.queue_pick.is_none() {
            let names = self.queues.iter().map(|q| q.name.clone()).collect();
            self.queue_pick = Some((ids, dialogs::QueuePick::new(names)));
        } else {
            self.queues[0].items.extend(ids);
        }
        self.dirty = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn push_item(
        &mut self,
        url: url::Url,
        dest: PathBuf,
        category: Category,
        description: String,
        size: Option<u64>,
        login: Option<(String, String)>,
        context: ipc::Command,
        source: &str,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut item = Item::new(id, url, dest, category);
        item.note(crate::i18n::fmt(
            crate::i18n::t("Added {source}"),
            &[("source", crate::i18n::t(source))],
        ));
        item.size = size;
        item.description = description;
        item.login = login;
        item.context = context;
        self.items.push(item);
        self.dirty = true;
        id
    }

    /// Download `id`'s name is only its address's guess (`guessed`): the server names it when
    /// it starts (`Item::name_from_url`).
    fn name_guessed(&mut self, id: u64, guessed: bool) {
        if guessed && let Some(item) = self.item(id) {
            item.name_from_url = true;
        }
    }

    /// Each frame (#35): the route connections take, from the adapter watch. When it changes,
    /// everything running stops (its connections were held to the route before, or to none;
    /// sockets that are bound can't leak, this is so they don't wait for a timeout) and the
    /// torrent session with it; what was stopped so starts again once the route lets it.
    fn network_adapter_tick(&mut self, ctx: &egui::Context) {
        let route = self.netwatch.route(&self.settings.net_adapter, ctx);
        crate::settings::share_route(route.clone());
        if route != self.route {
            let before = std::mem::replace(&mut self.route, route.clone());
            self.route_changed(&before);
        }
        if matches!(self.route, mdm_engine::Route::Down { .. }) || self.adapter_paused.is_empty() {
            return;
        }
        // Started again once their jobs have ended.
        let ready: Vec<u64> = self
            .adapter_paused
            .iter()
            .copied()
            .filter(|id| self.items.iter().any(|i| i.id == *id && i.live.is_none()))
            .collect();
        self.adapter_paused
            .retain(|id| self.items.iter().any(|i| i.id == *id) && !ready.contains(id));
        for id in ready {
            self.start(id, false);
        }
    }

    fn route_changed(&mut self, before: &mdm_engine::Route) {
        let note = match &self.route {
            mdm_engine::Route::Down { label } => crate::i18n::fmt(
                // i18n: event log entry: the download stopped because the chosen network adapter (a VPN's, usually) went away; {adapter} its name
                crate::i18n::t("Stopped: {adapter} is not connected; it goes on when it is back"),
                &[("adapter", label)],
            ),
            // i18n: event log entry: the download stopped to start again through the network adapter as it is now (a VPN reconnected, another adapter chosen)
            _ => crate::i18n::t("Stopped: the network adapter changed; starting again").to_owned(),
        };
        let mut stopped = Vec::new();
        for i in &mut self.items {
            if let Some(l) = &i.live {
                l.dl.cancel();
                i.note(note.clone());
                stopped.push(i.id);
            }
        }
        for id in stopped {
            if !self.adapter_paused.contains(&id) {
                self.adapter_paused.push(id);
            }
        }
        // The torrent session's sockets were bound as things were.
        if !before.is_any() || !self.route.is_any() {
            self.torrent_host.stop_now(&self.rt);
        }
        self.dirty = true;
    }

    /// Every 2 s (#40): Torrent settings for the next torrent session, and the running one stopped
    /// once no torrent has needed it for a minute (none running or sharing, no Add torrent open),
    /// when Torrent settings say so or their network settings changed.
    /// What each torrent sent and how long it shared, added to its `TorrentSpec` (librqbit
    /// counts only the current run); and the own upload limit of those sharing without a `Run`
    /// (a `Run` sets its own). Saved with the list at most every 10 s.
    fn torrent_accounts(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.account_at).as_secs();
        // Whole seconds only: the rest carries to the next tick.
        if dt > 0 {
            self.account_at += Duration::from_secs(dt);
        }
        let Some(t) = self.torrent_host.get() else {
            return;
        };
        let limits = self.settings.sharing_limits();
        let mut changed = false;
        let mut hashes = std::collections::HashSet::new();
        for i in self.items.iter_mut() {
            let Some(spec) = i.torrent.as_mut() else {
                continue;
            };
            hashes.insert(spec.hash.clone());
            let Ok(st) = t.stats(&spec.hash) else {
                continue;
            };
            let sent = self.accountant.sent(&spec.hash, st.uploaded);
            if sent > 0 {
                spec.uploaded += sent;
                changed = true;
            }
            if st.phase == mdm_torrent::Phase::Finished && dt > 0 {
                spec.seeded_secs += dt;
                changed = true;
            }
            // Torrent settings' seeding limits: a finished torrent over one stops being shared
            // (paused; it stays so after a restart, `complete_torrents`), with or without a Run.
            let mut stopped = None;
            if st.phase == mdm_torrent::Phase::Finished
                && let Some(why) = crate::torrents::sharing_limit_reached(spec, st.done, limits)
            {
                let (t2, hash) = (t.clone(), spec.hash.clone());
                self.rt.spawn(async move {
                    let _ = t2.pause(&hash).await;
                });
                stopped = Some(why);
                changed = true;
            }
            let (hash, up) = (spec.hash.clone(), spec.upload_limit);
            if i.live.is_none() {
                let _ = t.set_torrent_limits(&hash, None, up);
            }
            if let Some(why) = stopped {
                i.note(sharing_stopped_note(why));
            }
        }
        self.accountant.keep(&hashes);
        if changed && self.account_saved.elapsed() >= Duration::from_secs(10) {
            self.account_saved = now;
            self.dirty = true;
        }
    }

    fn torrent_tick(&mut self) {
        let cfg = crate::torrents::state_dir().and_then(|d| self.settings.torrent_config(d));
        self.torrent_host.set_config(cfg);
        self.torrent_host
            .set_upload(self.settings.torrent_upload_limit());
        self.torrent_accounts();
        if !self.settings.torrent_blocklist.trim().is_empty() {
            crate::torrents::refresh_blocklist(
                &self.rt,
                &self.settings.torrent_blocklist,
                &self.blocklist_busy,
            );
        }
        let sharing = self.settings.torrent_keep_sharing
            && self
                .items
                .iter()
                .any(|i| i.torrent.is_some() && i.status == Status::Complete);
        let busy = sharing
            || self.torrent_dlg.is_some()
            || self
                .items
                .iter()
                .any(|i| i.torrent.is_some() && i.live.is_some());
        if busy {
            self.torrent_idle_since = None;
            return;
        }
        let since = *self.torrent_idle_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(60) {
            self.torrent_host
                .stop_if_idle(&self.rt, true, self.settings.torrent_stop_idle);
        }
    }

    /// `/show <window>` (mdm_ipc::Command::show): open one of the windows by name, as its menu
    /// command does. Unknown names do nothing.
    fn show_named(&mut self, name: &str, ctx: &egui::Context) {
        self.show_window(ctx);
        let first = self.items.first().map(|i| i.id);
        let last = self.items.last().map(|i| i.id);
        let (window, tab) = name.split_once(':').unwrap_or((name, ""));
        match window {
            "addurl" => self.run(Cmd::AddUrl, None, ctx),
            // MDM_CRASH_TEST=1 only (crash.rs): the crash reports' own check.
            "crash-test" => crate::crash::test(tab),
            "batch" => self.run(Cmd::Batch(false), None, ctx),
            "find" => self.run(Cmd::Find, None, ctx),
            "scheduler" => self.run(Cmd::Scheduler, None, ctx),
            "about" => self.run(Cmd::About, None, ctx),
            // The main window into the tray, as closing it does (tests and #72's measurements).
            "tray" => self.hide_to_tray(ctx),
            "helpers" => self.helpers = Some(crate::helpers::Helpers::new()),
            "torrent-settings" => self.run(Cmd::TorrentSettings, None, ctx),
            // category:Music / category-delete:Music — the Categories right-click menu's
            // Properties and Delete for a category by name (#93; the docs' shots and checks);
            // category-remove:Music opens Delete with "Remove its downloads" chosen.
            "category" | "category-delete" | "category-remove" => {
                if let Some(c) = Category::all()
                    .into_iter()
                    // `/show` arrives in lower case (mdm_ipc::parse_args).
                    .find(|c| c.key().eq_ignore_ascii_case(tab) || c.shown().to_lowercase() == tab)
                {
                    let cmd = if window == "category" {
                        Cmd::CategoryProps(c)
                    } else {
                        Cmd::DeleteCategory(c)
                    };
                    self.run(cmd, None, ctx);
                    if window == "category-remove" {
                        self.confirm_category_delete =
                            self.confirm_category_delete.take().map(|d| d.removing());
                    }
                }
            }
            "video-page" => self.run(Cmd::VideoPage, None, ctx),
            // Download video from a page → Helpers... (#51).
            "video-helpers" => {
                self.run(Cmd::VideoPage, None, ctx);
                if let Some(v) = &mut self.video_dlg {
                    v.open_helpers();
                }
            }
            // Download video from a page with a sample video's subtitles (#68).
            // `video-subs:flac`: with that choice picked (screenshots of the lower choices).
            "video-subs" => {
                self.run(Cmd::VideoPage, None, ctx);
                if let Some(v) = &mut self.video_dlg {
                    v.show_sample();
                    if !tab.is_empty() {
                        v.pick(tab);
                    }
                }
            }
            "existing-files" => {
                let dir = self.downloads.join("Big Buck Bunny");
                self.shown_existing = Some(crate::dialogs::ExistingFiles::new(&[
                    dir.join("big_buck_bunny_1080p.mkv"),
                    dir.join("subtitles.srt"),
                    dir.join("poster.jpg"),
                ]));
            }
            "extension" => self.extension = Some(crate::extension::Extension::new()),
            "language" => self.run(Cmd::ChooseLanguage, None, ctx),
            "shortcuts" => self.run(Cmd::Shortcuts, None, ctx),
            "speedlimit" => self.run(Cmd::LimiterSettings, None, ctx),
            "properties" if first.is_some() => self.run(Cmd::Properties, first, ctx),
            "delete" if first.is_some() => self.run(Cmd::Remove, first, ctx),
            "progress" => {
                if let Some(id) = last
                    && !self.progress.iter().any(|p| p.id == id)
                {
                    self.progress
                        .push(Progress::new(id, self.settings.show_complete));
                }
                // progress:peers and so on: a torrent's details tab (the docs' screenshots).
                if let Some(id) = last
                    && let Some(p) = self.progress.iter_mut().find(|p| p.id == id)
                {
                    p.show_tab(tab);
                }
            }
            "options" => {
                let page = match tab {
                    "types" => 1,
                    "saveto" => 2,
                    "downloads" => 3,
                    "connection" => 4,
                    "proxy" => 5,
                    "logins" => 6,
                    "sounds" => 8,
                    "regional" => 9,
                    _ => 0,
                };
                self.run(Cmd::Options, None, ctx);
                self.options = self.options.take().map(|o| o.on_page(page));
                self.raise_options = true;
                // General → Download panels → Edit... (the video button's settings).
                if tab == "panels" {
                    self.options = self.options.take().map(|o| o.with_panels());
                }
            }
            _ => {}
        }
    }

    /// A command from the command line, a second launch, or the browser (ipc.rs).
    fn handle_command(&mut self, cmd: ipc::Command, ctx: &egui::Context) {
        if cmd.exit {
            self.run(Cmd::Exit, None, ctx);
            return;
        }
        if let Some(name) = cmd.show.as_deref() {
            self.show_window(ctx);
            self.show_named(name, ctx);
            return;
        }
        // The download button over a Google Drive file's player: the file itself, as it was
        // uploaded (segmented and resumable), not the player's re-encoded streams (#34).
        let mut cmd = cmd;
        if let Some(file) = drive_file_behind_player(&cmd) {
            cmd.video_page = None;
            cmd.url = Some(file);
        }
        // The browser's download button over a video: the Download video window, looking the
        // page up at once.
        if let Some(page) = cmd.video_page.as_deref() {
            self.show_window(ctx);
            let dir = self.settings.category_dir(Category::Video, &self.downloads);
            crate::videodlg::refresh_ytdlp(&self.rt);
            let page = page.to_owned();
            self.video_dlg = Some(
                crate::videodlg::VideoDialog::for_page(dir, &page, cmd)
                    .with_subs(self.settings.video_subs.clone()),
            );
            return;
        }
        // Its menu: never on this site.
        if let Some(host) = cmd.panel_hide_site.as_deref() {
            self.settings.video_panel_sites =
                mdm_ipc::capture::add_site(&self.settings.video_panel_sites, host);
            self.dirty = true;
            return;
        }
        // The browser's right-click menu: a page's (or a selection's) links, to tick in Import.
        if !cmd.links.is_empty() {
            self.show_window(ctx);
            let found = cmd
                .links
                .iter()
                .map(|url| crate::transfer::Imported {
                    url: url.clone(),
                    referer: cmd.referer.clone(),
                    cookies: cmd.cookies.clone(),
                    user_agent: cmd.user_agent.clone(),
                    private: cmd.private,
                })
                .collect();
            // i18n: in Import, where the addresses came from: a page in the browser
            let source = crate::i18n::t("Links from the browser").to_owned();
            self.show_import(found, source);
            return;
        }
        // The browser's toolbar button: this site's downloads stay with the browser.
        if let Some(host) = cmd.exclude_site.as_deref() {
            self.settings.excluded_sites =
                mdm_ipc::capture::add_site(&self.settings.excluded_sites, host);
            self.dirty = true;
            return;
        }
        if cmd.start_queue {
            self.start_queue(0);
        }
        // A magnet link or a .torrent file (browser, command line, file association): Add
        // torrent shows what it holds.
        if let Some(m) = cmd.url.as_deref().filter(|u| {
            mdm_torrent::Source::magnet(u).is_some()
                || (u.to_ascii_lowercase().ends_with(".torrent")
                    && std::path::Path::new(u.trim_matches('"')).is_file())
        }) {
            let m = m.trim().to_owned();
            self.show_window(ctx);
            self.open_torrent_dialog(Some(m), ctx);
            return;
        }
        let url = cmd
            .url
            .as_deref()
            .and_then(|u| url::Url::parse(u.trim()).ok())
            .filter(mdm_engine::supported);
        let Some(url) = url else {
            if !cmd.start_queue {
                // A plain second launch brings the window to the front, as IDM does.
                self.show_window(ctx);
            }
            return;
        };
        if !self.duplicate_gate(&url, || Pending::Command(Box::new(cmd.clone()))) {
            return;
        }
        // Options → Downloads → "Show start download dialog" off: start (or only queue) at once.
        let skip_dialog = !self.settings.show_start_dialog;
        if cmd.later || cmd.silent || skip_dialog {
            let guessed = cmd.name.is_none();
            let name = cmd
                .name
                .clone()
                .unwrap_or_else(|| mdm_engine::naming::file_name(None, &url));
            let name = mdm_engine::naming::sanitize(&name);
            let category = Category::of(&name);
            let dir = cmd
                .dir
                .clone()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.settings.category_dir(category, &self.downloads));
            let dest = mdm_engine::naming::destination(&dir, &name);
            let later = cmd.later || (skip_dialog && self.settings.only_add_to_queue);
            let source = if cmd.referer.is_some() || cmd.cookies.is_some() {
                crate::i18n::mark("from the browser")
            } else {
                crate::i18n::mark("from the command line")
            };
            let size = cmd.size;
            let id = self.push_item(url, dest, category, String::new(), size, None, cmd, source);
            self.name_guessed(id, guessed);
            if later {
                self.queues[0].items.push(id);
            } else {
                self.start(id, true);
            }
        } else {
            self.open_file_info(dialogs::NewUrl { url, login: None }, cmd, ctx);
        }
    }

    /// The headers and options `url`'s requests go with: the download's own `login`, else the
    /// one Options → Sites Logins has for its address, else an FTP address's own (its resume
    /// state keeps the address without the password); the browser's Referer, Cookie,
    /// User-Agent and form (`context`); Options' connections, proxy and User-Agent.
    pub fn request_parts(
        &self,
        url: &url::Url,
        login: Option<&(String, String)>,
        context: &ipc::Command,
    ) -> RequestParts {
        let site_login = self.settings.site_login(url);
        let url_login = mdm_engine::ftp::is_ftp(url)
            .then(|| url_login(url))
            .flatten();
        let mut headers = auth_headers(login.or(site_login.as_ref()).or(url_login.as_ref()));
        let post_type = context.post.as_ref().and(context.post_type.as_ref());
        let referer = context
            .referer
            .as_deref()
            .and_then(|r| sent_referrer(url, r));
        for (name, v) in [
            (reqwest::header::REFERER, &referer),
            (reqwest::header::COOKIE, &context.cookies),
            (reqwest::header::CONTENT_TYPE, &post_type.cloned()),
        ] {
            if let Some(v) = v.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
                headers.insert(name, v);
            }
        }
        let ua = self.settings.user_agent.trim();
        let opts = Options {
            max_connections: self.settings.connections_for(url),
            proxy: self.settings.proxy_for(context),
            dns: self.settings.dns(),
            route: self.route.clone(),
            global: Some(self.global.clone()),
            host_limits: Some(self.host_limits.clone()),
            ignore_mtime: self.settings.ignore_mtime,
            server_time: self.settings.set_creation_date,
            ftp_passive: self.settings.ftp_passive,
            user_agent: match context.user_agent.as_deref() {
                Some(browser) => browser.to_owned(),
                None if ua.is_empty() => Options::default().user_agent,
                None => ua.to_owned(),
            },
            post: context.post.clone().map(|body| body.into_bytes().into()),
            on_file_created: Some(mark_hook(url, context.referer.as_deref())),
            ..Options::default()
        };
        RequestParts { headers, opts }
    }

    /// Start or resume a download; `show` opens its progress dialog (IDM's default).
    fn start(&mut self, id: u64, show: bool) {
        let rt = self.rt.clone();
        let Some(item) = self.item(id) else { return };
        // A finished gallery may start again: it fetches the files not there yet (#58).
        if item.live.is_some() || (item.status == Status::Complete && item.gallery.is_none()) {
            return;
        }
        // A video from a page: yt-dlp, not the engine.
        if self.item(id).is_some_and(|i| i.video.is_some()) {
            self.start_video(id, show);
            return;
        }
        // An image gallery: gallery-dl.
        if self.item(id).is_some_and(|i| i.gallery.is_some()) {
            self.start_gallery(id, show);
            return;
        }
        // The download limit is reached: it starts when the window has room again.
        if self.settings.limits_on && self.quota.blocked {
            if !self.quota.held.contains(&id) {
                self.quota.held.push(id);
            }
            if let Some(item) = self.item(id) {
                item.status = Status::Queued;
                item.note(crate::i18n::t(
                    "Waiting: the download limit is reached (Scheduler)",
                ));
            }
            self.dirty = true;
            return;
        }
        if self.item(id).is_some_and(|i| i.torrent.is_some()) {
            self.start_torrent(id, show);
            return;
        }
        let Some((url, login, context, own_limit, checksum)) = self.item(id).map(|i| {
            (
                i.url.clone(),
                i.login.clone(),
                i.context.clone(),
                i.own_limit,
                i.checksum.clone(),
            )
        }) else {
            return;
        };
        // The browser's proxy needs its VPN extension's own login (#42): said, never bypassed.
        if let Some(why) = self.settings.browser_proxy_refused(&context) {
            if let Some(item) = self.item(id) {
                item.status = Status::Error;
                item.error = Some(why);
                item.last_try = Some(model::now_stamp());
            }
            self.dirty = true;
            return;
        }
        let RequestParts { headers, mut opts } = self.request_parts(&url, login.as_ref(), &context);
        // Download File Info's open answer for it, if it has one (#28); used once.
        let kept = self.kept_answers.remove(&id);
        // The file's own remembered limit, else the Speed Limiter's.
        opts.speed_limit = own_limit.or(self.settings.speed_limit());
        opts.checksum = checksum;
        let Some(item) = self.item(id) else { return };
        let _guard = rt.enter();
        // Resume whenever there is something to resume. If that fails, say why and keep the
        // data: starting over would overwrite it, and that is only ever the user's choice.
        let state = state_path(&item.dest);
        let dl = if state.exists() {
            match Download::resume(state, headers, opts) {
                Ok(dl) => dl,
                Err(e) => {
                    item.status = Status::Error;
                    item.error = Some(crate::errors::describe(&e));
                    item.last_try = Some(model::now_stamp());
                    self.dirty = true;
                    return;
                }
            }
        } else {
            // Nothing to resume. A missing folder is created by the engine; a removed drive is
            // reported without touching the progress shown for the data on it.
            if item.dest.parent().is_none_or(std::path::Path::is_dir) {
                item.downloaded = 0;
            }
            // A name nobody gave (the address's guess) is the server's to give: into the folder,
            // under its Content-Disposition name (#28); the row follows the file made.
            let dest = match item.dest.parent() {
                Some(dir) if item.name_from_url => Destination::Dir(dir.to_owned()),
                _ => Destination::File(item.dest.clone()),
            };
            Download::start_with(
                Request {
                    url: item.url.clone(),
                    headers,
                    dest,
                },
                opts,
                kept,
            )
        };
        item.note(if item.downloaded > 0 {
            // i18n: event log entry, past tense: the download resumed (just now)
            crate::i18n::t("Resumed")
        } else {
            // i18n: event log entry, past tense: the download started (just now)
            crate::i18n::t("Started")
        });
        item.live = Some(Live::new(model::Job::File(dl), item.downloaded));
        item.status = Status::Downloading;
        item.error = None;
        item.last_try = Some(model::now_stamp());
        self.dirty = true;
        if show && !self.progress.iter().any(|p| p.id == id) {
            self.progress
                .push(Progress::new(id, self.settings.show_complete));
        }
    }

    /// Apply the Speed Limiter's "for one file" setting to every running download, file or
    /// torrent; one with a remembered limit of its own keeps it, as when it starts.
    fn apply_speed_limit(&self) {
        let limit = self.settings.speed_limit();
        for i in &self.items {
            if let Some(l) = &i.live {
                l.dl.set_speed_limit(i.own_limit.or(limit));
            }
        }
    }

    fn stop(&mut self, id: u64) {
        if let Some(l) = self.item(id).and_then(|i| i.live.as_ref()) {
            l.dl.cancel();
        }
    }

    fn stop_all(&mut self) {
        for i in &self.items {
            if let Some(l) = &i.live {
                l.dl.cancel();
            }
        }
    }

    /// Take a download off the list; its files stay.
    fn delete(&mut self, id: u64) {
        self.remove_downloads(&[id], false);
    }

    /// Take downloads off the list (not while they run; see [`Self::remove_download`]). When
    /// the selection goes with them, the row that comes after them is selected, as in Explorer,
    /// so Delete can be pressed again.
    fn remove_downloads(&mut self, ids: &[u64], files: bool) {
        let before = self.shown_ids();
        for id in ids {
            self.remove_download(*id, files);
        }
        let after = self.shown_ids();
        self.selection.after_removal(&before, &after);
    }

    /// Take a download off the list (not while it runs); with `files`, its file goes from disk
    /// too: a finished file, or the data so far (a torrent's files when its session runs).
    fn remove_download(&mut self, id: u64, files: bool) {
        if self.item(id).is_some_and(|i| i.live.is_none()) {
            if let Some(spec) = self.item(id).and_then(|i| i.torrent.clone()) {
                self.torrent_host.forget(&self.rt, &spec.hash, files);
            } else if files
                && let Some(i) = self.items.iter().find(|i| i.id == id)
                // A gallery's folder stays (it may hold other files; #58).
                && i.gallery.is_none()
            {
                // The file itself: one that can't go (open in a player) is said, not skipped
                // silently; the row still leaves the list.
                if i.status == Status::Complete
                    && let Err(e) = std::fs::remove_file(&i.dest)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    dialogs::tell(
                        crate::i18n::mark("Delete"),
                        crate::i18n::fmt(
                            crate::i18n::t("{file} could not be deleted from the disk: {error}"),
                            &[
                                ("file", &crate::i18n::isolate(&i.file_name)),
                                ("error", &e.to_string()),
                            ],
                        ),
                    );
                }
                let _ = std::fs::remove_file(mdm_engine::naming::part_path(&i.dest));
                let _ = std::fs::remove_file(state_path(&i.dest));
            }
            self.items.retain(|i| i.id != id);
            for q in &mut self.queues {
                q.items.retain(|i| *i != id);
            }
            self.progress.retain(|p| p.id != id);
            self.selection.forget(id);
            self.dirty = true;
        }
    }

    /// Delete / Remove / Delete All Completed: asks first (unless "Don't show this dialog
    /// again" was ticked) whether only the list entries go or the files too; `files` ticks that
    /// box (Shift+Delete). Running downloads are left alone.
    fn ask_delete(&mut self, ids: Vec<u64>, files: bool) {
        let gone: Vec<&Item> = ids
            .iter()
            .filter_map(|id| self.items.iter().find(|i| i.id == *id && i.live.is_none()))
            .collect();
        let Some(first) = gone.first() else { return };
        // Shift+Delete ticks "also the files" this once; only a plain Delete's answer is
        // remembered (else one Shift+Delete made every later Delete erase files).
        let shift = files;
        let files = files || self.settings.delete_files_too;
        if self.settings.confirm_delete {
            let one = gone.len() == 1;
            let mut ask = dialogs::ConfirmDelete::new(
                gone.iter().map(|i| i.id).collect(),
                first.file_name.clone(),
                one && first.status != Status::Complete,
                files,
            );
            ask.remember = !shift;
            self.confirm_delete = Some(ask);
        } else {
            let ids: Vec<u64> = gone.into_iter().map(|i| i.id).collect();
            self.remove_downloads(&ids, files);
        }
    }

    fn delete_completed(&mut self) {
        let done = self
            .items
            .iter()
            .filter(|i| i.status == Status::Complete)
            .map(|i| i.id)
            .collect();
        self.ask_delete(done, false);
    }

    /// The global cap's rate now: on, and (if asked) only while the user is active (input in
    /// the last minute).
    ///
    /// Torrents share it with file downloads (D-012, "one limit for all downloads together"),
    /// evenly: each side may take half of it, and more when the other uses less (`share`).
    /// Both sides' speeds are measured each frame: the torrents' as the torrent session's
    /// download limit, file downloads' as the global limiter they wait on.
    fn apply_global_limit(&mut self) {
        let s = &self.settings;
        #[cfg(windows)]
        let active = crate::win32::idle_seconds() < 60;
        #[cfg(not(windows))]
        let active = true;
        let rate = (s.global_limit_on && (!s.global_only_active || active))
            .then(|| s.global_limit_kb.max(1) * 1024);
        let torrents = self.torrent_speed();
        let files = self.file_speed();
        // The torrents' limit moves in eighths of the cap, at most every 3 s: librqbit starts
        // each new limit with a second's burst, so following the other side's speed closely
        // let torrents run far past their share (measured: 773 KB/s for a 400 KB/s half).
        let want = rate.map(|cap| {
            let step = (cap / 8).max(1);
            (share(cap, files) / step * step).max(1024)
        });
        let (had, when) = self.torrent_cap;
        // Down at once (the other side has started: give way now), up only 3 s after the last
        // change.
        let moved = match (had, want) {
            (Some(a), Some(b)) => b < a || (b > a && when.elapsed() >= Duration::from_secs(3)),
            (a, b) => a != b,
        };
        if moved {
            self.torrent_cap = (want, Instant::now());
        }
        self.torrent_host.set_cap(self.torrent_cap.0);
        let for_files = rate.map(|cap| share(cap, torrents));
        if self.global.get() != for_files {
            self.global.set(for_files);
        }
    }

    /// File downloads' speed together, bytes/s, smoothed over about two seconds: the bytes that
    /// went through the global limiter, which every file download waits on. (Their progress
    /// rates read high and low by a quarter, which tipped the split.)
    fn file_speed(&mut self) -> f64 {
        let now = Instant::now();
        let taken = self.global.taken();
        let (speed, at, before) = self.file_speed;
        let dt = now.duration_since(at).as_secs_f64();
        if dt <= 0.0 {
            return speed;
        }
        let new = taken.saturating_sub(before) as f64;
        let k = 1.0 - (-dt / 2.0).exp();
        let mut speed = speed + (new / dt - speed) * k;
        // None running: 0 at once, so the torrents take the whole cap back in one step.
        if !self
            .items
            .iter()
            .any(|i| matches!(i.live.as_ref().map(|l| &l.dl), Some(model::Job::File(_))))
        {
            speed = 0.0;
        }
        self.file_speed = (speed, now, taken);
        speed
    }

    /// The torrents' download speed together, bytes/s, smoothed over about two seconds (from
    /// the bytes librqbit says each fetched; it counts from 0 again each time a torrent
    /// resumes, so a smaller count is all new).
    fn torrent_speed(&mut self) -> f64 {
        let now = Instant::now();
        let mut seen = HashMap::new();
        let mut new = 0u64;
        for i in &self.items {
            let Some(model::Job::Torrent(run)) = i.live.as_ref().map(|l| &l.dl) else {
                continue;
            };
            let Some(fetched) = run.fetched() else {
                continue;
            };
            if let Some(&before) = self.torrent_fetched.get(&i.id) {
                new += fetched.checked_sub(before).unwrap_or(fetched);
            }
            seen.insert(i.id, fetched);
        }
        self.torrent_fetched = seen;
        let dt = now.duration_since(self.torrent_speed.1).as_secs_f64();
        self.torrent_speed.1 = now;
        if dt > 0.0 {
            let k = 1.0 - (-dt / 2.0).exp();
            self.torrent_speed.0 += (new as f64 / dt - self.torrent_speed.0) * k;
        }
        if self.torrent_fetched.is_empty() {
            self.torrent_speed.0 = 0.0;
        }
        self.torrent_speed.0
    }

    /// Progress of all running downloads together (bytes done / bytes in all), None when
    /// nothing runs or no size is known. For the taskbar button's progress, which only Windows
    /// has (macOS and Linux docks have no common way to show one).
    #[cfg_attr(not(windows), allow(dead_code))]
    fn overall_progress(&self) -> Option<f64> {
        let (done, total) = self
            .items
            .iter()
            .filter(|i| i.live.is_some())
            .filter_map(|i| i.size.map(|s| (i.downloaded, s)))
            .fold((0u64, 0u64), |(d, t), (di, si)| (d + di.min(si), t + si));
        (total > 0).then(|| done as f64 / total as f64)
    }

    /// The main window into the tray: downloads, queues and schedules go on. Wayland can't hide a
    /// window, so there it goes to the dock / task list instead.
    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "linux")]
        let wayland = crate::unix::on_wayland();
        #[cfg(not(target_os = "linux"))]
        let wayland = false;
        if wayland {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.hidden = true;
        // Hidden, the window's memory sits unused until it shows again: give it back to the
        // system (#72). Windows pages it back in from RAM when it's touched again.
        self.trim_when_hidden = Some(Instant::now() + TRIM_AFTER_HIDE);
    }

    /// The window back from the tray (or from minimized), in front.
    fn show_window(&mut self, ctx: &egui::Context) {
        self.trim_when_hidden = None;
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        if !crate::offscreen() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Whether the tray icon is shown somewhere (Linux: a tray host is there; asked when the
    /// window is closed, the host may come and go).
    fn tray_shown(&self) -> bool {
        crate::tray::available()
    }

    /// The tray icon: created once, its commands run, and closing the window hides it there.
    fn tray(&mut self, ctx: &egui::Context) {
        if self.tray.is_none() && self.tray_limiter.is_none() && !crate::offscreen() {
            let icon = crate::app_icon();
            self.tray = crate::tray::Tray::new(
                ctx,
                icon.rgba.clone(),
                icon.width,
                self.settings.speed_limit_on,
            );
            self.tray_limiter = Some(self.settings.speed_limit_on);
        }
        if let Some(t) = &self.tray
            && self.tray_limiter != Some(self.settings.speed_limit_on)
        {
            t.set_limiter(self.settings.speed_limit_on);
            self.tray_limiter = Some(self.settings.speed_limit_on);
        }
        if let (Some(t), Some(q)) = (&self.tray, self.queues.first()) {
            t.set_queue(q.running);
        }
        if let Some(t) = &self.tray {
            let tip = match self.activity() {
                Some(a) => format!("{}\n{a}", crate::MAIN_TITLE),
                None => crate::MAIN_TITLE.to_owned(),
            };
            t.set_tooltip(&tip);
            // "Downloads in progress": each running download, to show its progress window (#63).
            let running: Vec<(u64, String)> = self
                .items
                .iter()
                .filter(|i| i.live.is_some())
                .map(|i| (i.id, i.file_name.clone()))
                .collect();
            t.set_downloads(&running);
        }
        let cmds = self.tray.as_ref().map(|t| t.poll()).unwrap_or_default();
        for c in cmds {
            use crate::tray::TrayCmd;
            tracing::info!("tray: {c:?}");
            let cmd = match c {
                TrayCmd::Show => {
                    self.show_window(ctx);
                    continue;
                }
                TrayCmd::AddUrl => Cmd::AddUrl,
                TrayCmd::Batch => Cmd::Batch(false),
                TrayCmd::LimiterOn => Cmd::LimiterOn,
                TrayCmd::LimiterOff => Cmd::LimiterOff,
                TrayCmd::Scheduler => Cmd::Scheduler,
                TrayCmd::StartQueue => Cmd::StartQueue(0),
                TrayCmd::StopQueue => Cmd::StopQueue(0),
                TrayCmd::Options => Cmd::Options,
                TrayCmd::Exit => Cmd::Exit,
                TrayCmd::ShowDownload(id) => {
                    self.open_item(id);
                    continue;
                }
            };
            self.run(cmd, None, ctx);
        }
        // Closing hides the window in the tray; downloads and queues go on. On Linux only when a
        // tray host shows the icon (#59: without one the window couldn't come back, nor close):
        // otherwise closing closes, as any window's does.
        if self.tray.is_some()
            && !self.exiting
            && ctx.input(|i| i.viewport().close_requested())
            && self.tray_shown()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
        }
        // Options → General → "Minimize to the notification area" (#63): Minimize hides the
        // window there too, where a tray shows the icon. Wayland can't hide a window: there it
        // stays minimized, in the dock, as it was.
        #[cfg(target_os = "linux")]
        let can_hide = !crate::unix::on_wayland();
        #[cfg(not(target_os = "linux"))]
        let can_hide = true;
        let wanted = self.tray.is_some()
            && self.settings.minimize_to_tray
            && can_hide
            && !self.exiting
            && self.tray_shown();
        // Windows: the minimize command itself is caught (win32), so the window goes straight to
        // the tray, never minimized (an idle app runs no frame while minimized: it wouldn't see).
        #[cfg(windows)]
        let minimize = crate::win32::main_minimize_to_tray(ctx, wanted);
        // Elsewhere: on the change to minimized (brought back, the window may still read
        // minimized for a frame or two). It is seen at the next frame the app runs; macOS doesn't
        // report it at all while running (egui#3494), so there Minimize stays in the Dock.
        #[cfg(not(windows))]
        let minimize = {
            let minimized = ctx.input(|i| i.viewport().minimized == Some(true));
            let was = egui::Id::unique("main-was-minimized");
            let was_minimized = ctx.data(|d| d.get_temp::<bool>(was)).unwrap_or(false);
            ctx.data_mut(|d| d.insert_temp(was, minimized));
            wanted && minimized && !was_minimized
        };
        if minimize && !self.hidden {
            tracing::info!("minimized to the notification area");
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
        }
        // Hidden, the window gets no paint messages: keep the clock (queues, scheduler,
        // download limits) running.
        if self.hidden {
            ctx.request_repaint_after(Duration::from_secs(1));
            if self.trim_when_hidden.is_some_and(|t| Instant::now() >= t) {
                trim_memory();
                self.trim_when_hidden = Some(Instant::now() + TRIM_EVERY_HIDDEN);
            }
        }
    }

    /// Options → General → "Offer to download links copied to the clipboard": a newly copied
    /// link to a listed file type (Options → File types) is handled as if a browser had sent
    /// it, so Download File Info opens with it (unless that window is turned off). Only read,
    /// never written; on by default.
    fn watch_clipboard(&mut self, ctx: &egui::Context) {
        if !self.settings.clipboard_autostart {
            self.clip_seen = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        if self
            .clip_checked
            .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.clip_checked = Some(Instant::now());
        let text = arboard::Clipboard::new()
            .ok()
            .and_then(|mut c| c.get_text().ok())
            .map(|t| t.trim().to_owned());
        // The first look only notes what is there: turning it on doesn't grab an old link.
        let Some(before) = self.clip_seen.replace(text.clone()) else {
            return;
        };
        let Some(text) = text.filter(|t| Some(t) != before.as_ref()) else {
            return;
        };
        let Ok(url) = url::Url::parse(&text) else {
            return;
        };
        let name = mdm_engine::naming::file_name(None, &url);
        if mdm_engine::supported(&url) && self.settings.auto_type(&name) {
            let cmd = ipc::Command {
                url: Some(text),
                ..Default::default()
            };
            self.handle_command(cmd, ctx);
        }
    }

    /// Open (or bring back) the File Properties of a download.
    fn properties(&mut self, id: u64) {
        if self.properties.iter().any(|p| p.id == id) {
            dialogs::raise(
                &self.ui_ctx,
                dialogs::dialog_viewport(&format!("properties-{id}")),
            );
            return;
        }
        if let Some(item) = self.items.iter().find(|i| i.id == id) {
            self.properties
                .push(crate::properties::Properties::new(item));
        }
    }

    /// File Properties → OK.
    fn apply_properties(&mut self, c: crate::properties::Changes) {
        let Some(item) = self.item(c.id) else { return };
        item.description = c.description;
        item.login = c.login;
        item.context.referer = c.referer;
        if let Some(url) = c.address {
            // Refresh download address: the resume state follows, so the download continues
            // from the new address (its size and validators still have to match).
            let state = mdm_engine::state::state_path(&item.dest);
            if let Ok(mut saved) = mdm_engine::state::ResumeState::load(&state) {
                saved.url = url.clone();
                saved.final_url = url.clone();
                let _ = saved.write(&state);
            }
            item.note(crate::i18n::fmt(
                crate::i18n::t("Address changed to {host}"),
                &[("host", &crate::i18n::ltr(url.host_str().unwrap_or("?")))],
            ));
            item.url = url;
            item.error = None;
            if item.status == Status::Error {
                item.status = Status::Stopped;
            }
        }
        // Moved into the folder it is already in: nothing to do (it became "a (1).zip").
        let move_to = c
            .move_to
            .filter(|dir| item.dest.parent() != Some(dir.as_path()))
            .map(|dir| mdm_engine::naming::destination(&dir, &item.file_name));
        self.dirty = true;
        if let Some(target) = move_to {
            self.start_move(c.id, target);
        }
    }

    /// Move a download to `target` (its name too): a finished file moves on a thread of its
    /// own (a copy to another drive takes a while), one not started just takes the new place.
    /// A failure says why.
    fn start_move(&mut self, id: u64, target: PathBuf) {
        let Some(item) = self.item(id) else { return };
        if target == item.dest {
            return;
        }
        if item.status != Status::Complete {
            if item.downloaded == 0 {
                take_place(item, target);
                self.dirty = true;
            }
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let from = item.dest.clone();
        let to = target.clone();
        item.note(crate::i18n::t("Moving the file..."));
        std::thread::spawn(move || {
            let _ = tx.send(move_file(&from, &to));
        });
        self.moving.push((id, target, rx));
    }

    /// Moves that finished: the download takes its new place, or the user is told why not.
    fn poll_moves(&mut self) {
        let mut done = Vec::new();
        self.moving.retain(|(id, target, rx)| match rx.try_recv() {
            Ok(r) => {
                done.push((*id, target.clone(), r));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for (id, target, r) in done {
            match r {
                Ok(()) => {
                    if let Some(item) = self.item(id) {
                        take_place(item, target);
                    }
                    self.dirty = true;
                }
                Err(e) => {
                    if let Some(item) = self.item(id) {
                        item.note(crate::i18n::t("The file could not be moved"));
                    }
                    self.notice = Some((
                        crate::i18n::mark("Move/Rename"),
                        crate::i18n::fmt(
                            crate::i18n::t("The file could not be moved: {error}"),
                            &[("error", &e)],
                        ),
                    ));
                }
            }
        }
    }

    /// Select the next download matching `q` after the selection: in the list as shown, else
    /// in all downloads (switching the view to All Downloads).
    fn find_next(&mut self, q: &crate::find::Query) {
        let shown = self.shown_ids();
        let found = q
            .next(&shown, &self.items, self.selection.focus())
            .or_else(|| {
                self.view = View::All;
                q.next(&self.shown_ids(), &self.items, self.selection.focus())
            });
        match found {
            Some(id) => {
                self.selection.set(id);
                self.list.scroll_to = self.shown_ids().iter().position(|x| *x == id);
            }
            None => {
                self.notice = Some((
                    crate::i18n::mark("Find file"),
                    crate::i18n::fmt(
                        crate::i18n::t("Cannot find \"{text}\""),
                        &[("text", q.text.trim())],
                    ),
                ))
            }
        }
    }

    /// Download video from a page → Download Now / Later.
    fn add_video(&mut self, add: crate::videodlg::VideoAdd) {
        let folder = add
            .dest
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.downloads.clone());
        let name = add
            .dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".into());
        // yt-dlp takes any `<stem>.*` there as its own: a stem no file and no other download
        // in the folder has, whatever the extension (#32).
        let items = &self.items;
        let dest = mdm_engine::naming::stem_destination(&folder, &name, |stem| {
            items.iter().any(|i| {
                i.dest.parent() == Some(folder.as_path())
                    && i.dest
                        .file_stem()
                        .is_some_and(|s| s.to_string_lossy().to_lowercase() == stem.to_lowercase())
            })
        });
        let id = self.push_item(
            add.page,
            dest,
            Category::Video,
            String::new(),
            add.size,
            None,
            add.context,
            crate::i18n::mark("from a page (yt-dlp)"),
        );
        if let Some(item) = self.item(id) {
            item.video = Some(model::VideoSpec {
                selector: add.selector,
                subs: add.subs,
            });
        }
        // The Subtitles choice is offered again for the next video (#68).
        if let Some(subs) = add.remember {
            self.settings.video_subs = subs;
        }
        self.selection.set(id);
        if add.start_now {
            self.start(id, true);
        } else {
            self.queue_later(vec![id], self.settings.queue_panel_later);
        }
        self.dirty = true;
    }

    /// A playlist's videos (#69), one download each, into its folder: names picked with the
    /// folder read once (naming::Stems), added at once and saved once, then queued (Download
    /// Now starts the main queue, so its "N at the same time" applies).
    fn add_playlist(&mut self, add: crate::playlistdlg::PlaylistAdd) {
        let started = Instant::now();
        let folder = add.folder;
        let in_use = self
            .items
            .iter()
            .filter(|i| i.dest.parent() == Some(folder.as_path()))
            .filter_map(|i| i.dest.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut stems = mdm_engine::naming::Stems::new(&folder, in_use);
        self.items.reserve(add.entries.len());
        let mut ids = Vec::with_capacity(add.entries.len());
        for (page, title) in add.entries {
            let stem = mdm_engine::naming::sanitize(&title);
            let dest = stems.pick(&format!("{stem}.{}", add.ext));
            let id = self.push_item(
                page,
                dest,
                Category::Video,
                String::new(),
                None,
                None,
                add.context.clone(),
                // i18n: where a download came from, in its log: a playlist or channel, by yt-dlp
                crate::i18n::mark("from a playlist (yt-dlp)"),
            );
            // push_item adds at the end: no search through the list for each one.
            if let Some(item) = self.items.last_mut() {
                item.video = Some(model::VideoSpec {
                    selector: add.selector.clone(),
                    subs: Default::default(),
                });
            }
            ids.push(id);
        }
        let Some(&first) = ids.first() else { return };
        self.selection.set(first);
        if add.start_now {
            self.queues[0].items.extend(ids.iter().copied());
            self.start_queue(0);
        } else {
            self.queue_later(ids.clone(), self.settings.queue_panel_later);
        }
        self.dirty = true;
        eprintln!(
            "playlist: {} downloads added in {} ms",
            ids.len(),
            started.elapsed().as_millis()
        ); // not translated: a log line
    }

    /// Start a video item: yt-dlp (installed and verified) downloads it.
    fn start_video(&mut self, id: u64, show: bool) {
        let tools = mdm_tools::tools_dir();
        let tool = tools.as_deref().and_then(mdm_tools::installed);
        let ffmpeg = tools.as_deref().and_then(mdm_tools::ffmpeg::installed);
        let rate = self.settings.speed_limit();
        let Some(item) = self.item(id) else { return };
        let Some(spec) = item.video.clone() else {
            return;
        };
        let Some(tool) = tool else {
            item.status = Status::Error;
            item.error = Some(
                crate::i18n::t(
                    "yt-dlp is not installed (Tasks → Download video from a page installs it)",
                )
                .into(),
            );
            self.dirty = true;
            return;
        };
        let folder = item.dest.parent().map(PathBuf::from).unwrap_or_default();
        let stem = item
            .dest
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "video".into());
        let _ = std::fs::create_dir_all(&folder);
        let started = mdm_tools::video::VideoJob::start(
            &tool.exe,
            item.url.as_str(),
            &spec.selector,
            &folder,
            &stem,
            rate,
            ffmpeg.as_ref().map(|f| f.exe.as_path()),
            item.size,
            &crate::videodlg::from_browser(&item.context),
            &spec.subs,
        );
        match started {
            Ok(job) => {
                item.note(crate::i18n::fmt(
                    crate::i18n::t("Started with yt-dlp {version} ({format})"),
                    &[("version", &tool.version), ("format", &spec.selector)],
                ));
                let url = item.url.clone();
                item.live = Some(Live::new(model::Job::Video { job, url }, item.downloaded));
                item.status = Status::Downloading;
                item.error = None;
                item.last_try = Some(model::now_stamp());
            }
            Err(e) => {
                item.status = Status::Error;
                item.error = Some(e.to_string());
            }
        }
        self.dirty = true;
        if show && !self.progress.iter().any(|p| p.id == id) {
            self.progress
                .push(Progress::new(id, self.settings.show_complete));
        }
    }

    /// Download a gallery → Download Now / Later (#58): one row for the gallery, its folder its
    /// own.
    fn add_gallery(&mut self, add: crate::gallerydlg::GalleryAdd) {
        let id = self.push_item(
            add.page,
            add.folder,
            Category::General,
            String::new(),
            None,
            None,
            Default::default(),
            // i18n: where a download came from, in its log: an image gallery, by gallery-dl
            crate::i18n::mark("from a gallery (gallery-dl)"),
        );
        if let Some(item) = self.items.last_mut() {
            item.file_name = add.name;
            item.gallery = Some(model::GallerySpec { limit: add.limit });
        }
        self.selection.set(id);
        if add.start_now {
            self.start(id, true);
        } else {
            self.queue_later(vec![id], self.settings.queue_panel_later);
        }
        self.dirty = true;
    }

    /// Start (or start again) a gallery: gallery-dl, installed and verified, or the person's.
    fn start_gallery(&mut self, id: u64, show: bool) {
        let tool = mdm_tools::tools_dir()
            .as_deref()
            .and_then(mdm_tools::gallery::installed);
        let rate = self.settings.speed_limit();
        let Some(item) = self.item(id) else { return };
        let Some(spec) = item.gallery.clone() else {
            return;
        };
        let Some(tool) = tool else {
            item.status = Status::Error;
            item.error = Some(
                crate::i18n::t(
                    // i18n: a gallery download's error: the helper program it needs is missing
                    "gallery-dl is not installed (Add URL with a gallery's address installs it)",
                )
                .into(),
            );
            self.dirty = true;
            return;
        };
        let ask = mdm_tools::gallery::Ask {
            limit: spec.limit,
            rate,
        };
        let started = mdm_tools::gallery::GalleryJob::start(
            &tool.exe,
            item.url.as_str(),
            &item.dest,
            &ask,
            &crate::videodlg::from_browser(&item.context),
        );
        match started {
            Ok(job) => {
                item.note(crate::i18n::fmt(
                    // i18n: a gallery download's log: {version} = gallery-dl's version
                    crate::i18n::t("Started with gallery-dl {version}"),
                    &[("version", &tool.version)],
                ));
                let url = item.url.clone();
                // A gallery counts what it saves this run.
                item.downloaded = 0;
                item.size = None;
                item.live = Some(Live::new(model::Job::Gallery { job, url }, 0));
                item.status = Status::Downloading;
                item.error = None;
                item.last_try = Some(model::now_stamp());
            }
            Err(e) => {
                item.status = Status::Error;
                item.error = Some(e.to_string());
            }
        }
        self.dirty = true;
        if show && !self.progress.iter().any(|p| p.id == id) {
            self.progress
                .push(Progress::new(id, self.settings.show_complete));
        }
    }

    /// The info hashes of the torrents in the list (the torrent session keeps only these).
    fn torrent_hashes(&self) -> std::collections::HashSet<String> {
        self.items
            .iter()
            .filter_map(|i| i.torrent.as_ref().map(|t| t.hash.clone()))
            .collect()
    }

    /// The list's finished torrents (what "keep sharing" shares).
    /// The finished torrents to share (Torrent settings' "Keep sharing"): those that haven't
    /// reached a seeding limit.
    fn complete_torrents(&self) -> Vec<String> {
        let limits = self.settings.sharing_limits();
        self.items
            .iter()
            .filter(|i| i.status == Status::Complete)
            .filter_map(|i| i.torrent.as_ref().map(|t| (t, i.downloaded)))
            .filter(|(t, done)| crate::torrents::sharing_limit_reached(t, *done, limits).is_none())
            .map(|(t, _)| t.hash.clone())
            .collect()
    }

    fn open_torrent_dialog(&mut self, source: Option<String>, ctx: &egui::Context) {
        if self.torrent_dlg.is_some() {
            return;
        }
        self.torrent_dlg = Some(crate::torrentdlg::TorrentDialog::new(
            source,
            self.torrent_host.clone(),
            self.torrent_hashes(),
            self.downloads.clone(),
            self.settings.category_dirs.clone(),
            &self.rt,
            ctx,
        ));
    }

    /// Add torrent → Download Now / Later.
    fn add_torrent(&mut self, add: crate::torrentdlg::TorrentAdd) {
        let meta = &add.meta;
        if let Err(e) = crate::torrents::save_torrent(&meta.info_hash, &meta.torrent) {
            self.notice = Some((
                crate::i18n::mark("Add torrent"),
                crate::i18n::fmt(
                    crate::i18n::t("The torrent could not be saved: {error}"),
                    &[("error", &e.to_string())],
                ),
            ));
            return;
        }
        let folder = meta.folder_in(&add.dir);
        let dest = crate::torrents::dest_for(meta, &add.dir, add.files.as_deref());
        let size = meta
            .files
            .iter()
            .enumerate()
            .filter(|(i, f)| !f.padding && add.files.as_ref().is_none_or(|c| c.contains(i)))
            .map(|(_, f)| f.size)
            .sum();
        let id = self.push_item(
            crate::torrents::magnet_url(&meta.info_hash, &meta.name),
            dest,
            add.category,
            String::new(),
            Some(size),
            None,
            Default::default(),
            if add.from_magnet {
                crate::i18n::mark("from a magnet link")
            } else {
                crate::i18n::mark("from a torrent file")
            },
        );
        if let Some(item) = self.item(id) {
            item.torrent = Some(crate::torrents::TorrentSpec {
                hash: meta.info_hash.clone(),
                files: add.files.clone(),
                folder,
                uploaded: 0,
                seeded_secs: 0,
                upload_limit: None,
            });
        }
        self.selection.set(id);
        if add.start_now {
            self.start(id, true);
        } else {
            self.queue_later(vec![id], self.settings.queue_panel_later);
        }
        self.dirty = true;
    }

    /// Start a torrent: the session adds or resumes it.
    fn start_torrent(&mut self, id: u64, show: bool) {
        let known = self.torrent_hashes();
        let share = self.settings.torrent_keep_sharing;
        let limit = self.settings.speed_limit();
        let (host, rt) = (self.torrent_host.clone(), self.rt.clone());
        let Some(item) = self.item(id) else { return };
        // Its own remembered limit first, as a file download's (#39).
        let limit = item.own_limit.or(limit);
        let Some(spec) = item.torrent.clone() else {
            return;
        };
        let run =
            crate::torrents::Run::start(&host, &rt, &spec, item.url.clone(), known, limit, share);
        item.note(if item.downloaded > 0 {
            // i18n: event log entry, past tense: the download resumed (just now)
            crate::i18n::t("Resumed")
        } else {
            // i18n: event log entry, past tense: the download started (just now)
            crate::i18n::t("Started")
        });
        item.live = Some(Live::new(model::Job::Torrent(run), item.downloaded));
        item.status = Status::Downloading;
        item.error = None;
        item.last_try = Some(model::now_stamp());
        self.dirty = true;
        if show && !self.progress.iter().any(|p| p.id == id) {
            self.progress
                .push(Progress::new(id, self.settings.show_complete));
        }
    }

    /// Batch download → OK: every address goes to the main download queue (IDM's default; its
    /// queue selection panel is not built yet).
    fn add_batch(&mut self, added: crate::batch::Added) {
        let mut ids = Vec::new();
        for url in added.urls {
            let name = mdm_engine::naming::file_name(None, &url);
            let category = Category::of(&name);
            let dir = self.settings.category_dir(category, &self.downloads);
            let dest = mdm_engine::naming::destination(&dir, &name);
            let id = self.push_item(
                url,
                dest,
                category,
                String::new(),
                None,
                added.login.clone(),
                Default::default(),
                crate::i18n::mark("by batch download"),
            );
            self.name_guessed(id, true);
            ids.push(id);
        }
        self.queue_later(ids, self.settings.queue_panel_batch);
        self.dirty = true;
    }

    /// The automation API's answer (a JSON line, shaped by `ipc::api`): `api` turns it on or
    /// off; `query: "list"` gives every download and the total speed, `action` acts on
    /// downloads ([`App::act`]) — only while it is on.
    fn answer(&mut self, cmd: &ipc::Command) -> String {
        use ipc::api::{ApiAnswer, DownloadInfo, ErrorAnswer, ListAnswer, line};
        if let Some(on) = cmd.api {
            self.settings.automation_api = on;
            self.dirty = true;
            return line(&ApiAnswer { api: on });
        }
        if !self.settings.automation_api {
            return line(&ErrorAnswer {
                error: "the automation API is off (misterdownloadmanager-cli api on)".into(),
                code: Some(ipc::api::API_OFF.into()),
            });
        }
        if let Some(action) = cmd.action.as_deref() {
            return match self.act(action, &cmd.ids) {
                Ok(done) => line(&done),
                Err(e) => line(&e),
            };
        }
        match cmd.query.as_deref() {
            Some("list") => {
                let downloads = self
                    .items
                    .iter()
                    .map(|i| DownloadInfo {
                        id: i.id,
                        file: i.file_name.clone(),
                        url: i.url.to_string(),
                        path: i.dest.to_string_lossy().into_owned(),
                        status: format!("{:?}", i.status),
                        size: i.size,
                        downloaded: i.downloaded,
                        speed: if i.live.is_some() { i.rate() as u64 } else { 0 },
                        added: i.added.clone(),
                        error: i.error.clone().filter(|_| i.status == Status::Error),
                    })
                    .collect();
                let speed: f64 = self
                    .items
                    .iter()
                    .filter(|i| i.live.is_some())
                    .map(|i| i.rate())
                    .sum();
                line(&ListAnswer {
                    downloads,
                    speed: speed as u64,
                })
            }
            other => line(&ErrorAnswer {
                error: format!("unknown query {other:?}"),
                code: None,
            }),
        }
    }

    /// The automation API's actions on downloads by id: pause one running, start one stopped,
    /// queued or failed (or a finished gallery), remove one (its file kept) or delete it with its file; one that runs
    /// is paused first by the caller.
    fn act(
        &mut self,
        action: &str,
        ids: &[u64],
    ) -> Result<ipc::api::ActAnswer, ipc::api::ErrorAnswer> {
        if !matches!(action, "pause" | "start" | "remove" | "delete") {
            return Err(ipc::api::ErrorAnswer {
                error: format!("unknown action {action:?}"),
                code: None,
            });
        }
        let mut answer = ipc::api::ActAnswer::default();
        for &id in ids {
            let Some(item) = self.item(id) else {
                answer.skipped.push(ipc::api::Skipped {
                    id,
                    why: "no such download".into(),
                });
                continue;
            };
            let running = item.live.is_some();
            // As Resume: a finished gallery may start again (#58).
            let startable = running || item.can_start();
            let why = match action {
                "pause" if !running => Some("not running"),
                "pause" => {
                    self.stop(id);
                    None
                }
                "start" if running => Some("already running"),
                "start" if !startable => Some("complete"),
                "start" => {
                    self.start(id, false);
                    None
                }
                _ if running => Some("running: pause it first"),
                _ => {
                    self.remove_download(id, action == "delete");
                    None
                }
            };
            match why {
                Some(why) => answer.skipped.push(ipc::api::Skipped {
                    id,
                    why: why.into(),
                }),
                None => answer.done.push(id),
            }
        }
        Ok(answer)
    }

    /// Options → OK: keep the new settings and act on what changed.
    fn apply_options(&mut self, new: Settings) {
        new.publish_categories();
        // Portable mode writes nothing outside its folder (the option is off there).
        if new.launch_on_startup != self.settings.launch_on_startup
            && mdm_ipc::portable_dir().is_none()
        {
            #[cfg(windows)]
            crate::win32::set_run_at_startup(new.launch_on_startup);
            #[cfg(unix)]
            crate::unix::set_run_at_startup(new.launch_on_startup);
        }
        self.settings = new;
        crate::regional::set(&self.settings.regional);
        if self.settings.proxy_mode == 2 {
            crate::pacfetch::refresh(&self.settings.proxy_script, &self.rt);
        }
        self.apply_speed_limit();
        self.dirty = true;
    }

    /// Options → Sounds: play the sound for event `i` (complete, failed, queue started,
    /// queue stopped/finished) when it is on.
    fn sound(&self, i: usize) {
        if let Some((true, file)) = self.settings.sounds.get(i)
            && !file.is_empty()
        {
            crate::options::play_sound(file);
        }
    }

    /// Apply what the Scheduler asks for.
    fn scheduler_action(&mut self, a: crate::scheduler::Action) {
        use crate::scheduler::Action;
        let index = |queues: &[Queue], name: &str| queues.iter().position(|q| q.name == name);
        match a {
            Action::Apply { queues, limits } => {
                // A running queue the Scheduler deleted stops first (its downloads stay).
                for q in 0..self.queues.len() {
                    if self.queues[q].running
                        && !queues.iter().any(|n| n.name == self.queues[q].name)
                    {
                        self.stop_queue(q);
                    }
                }
                // Running state belongs to the app, not the draft.
                let running: Vec<String> = self
                    .queues
                    .iter()
                    .filter(|q| q.running)
                    .map(|q| q.name.clone())
                    .collect();
                // The list showing a queue follows it to its new place (All Downloads when
                // it is gone).
                if let View::Queue(i) = self.view {
                    let name = self.queues.get(i).map(|q| q.name.clone());
                    self.view = name
                        .and_then(|n| queues.iter().position(|q| q.name == n))
                        .map_or(View::All, View::Queue);
                }
                self.queues = queues;
                for q in &mut self.queues {
                    q.running = running.contains(&q.name);
                }
                self.settings.limits_on = limits.on;
                self.settings.limits_mb = limits.mb;
                self.settings.limits_hours = limits.hours;
                self.settings.limits_warn = limits.warn;
                self.dirty = true;
            }
            Action::Start(name) => {
                if let Some(q) = index(&self.queues, &name) {
                    self.start_queue(q);
                }
            }
            Action::Stop(name) => {
                if let Some(q) = index(&self.queues, &name) {
                    self.stop_queue(q);
                }
            }
        }
    }

    fn start_queue(&mut self, q: usize) {
        if !self.queues[q].running {
            self.sound(2);
        }
        self.queues[q].running = true;
        self.tick_queues();
    }

    fn stop_queue(&mut self, q: usize) {
        self.queues[q].running = false;
        for id in self.queues[q].items.clone() {
            self.stop(id);
        }
    }

    /// How a queue sees one of its downloads.
    /// Every download's standing for the queues, by id.
    fn members(&self, now: Instant) -> std::collections::HashMap<u64, Member> {
        self.items
            .iter()
            .map(|i| (i.id, self.member_of(i, now)))
            .collect()
    }

    fn member_of(&self, i: &Item, now: Instant) -> Member {
        let id = i.id;
        match i.status {
            Status::Queued | Status::Stopped => Member::Waiting,
            Status::Downloading => Member::Running,
            Status::Complete => Member::Complete,
            Status::Error => {
                let long_ago = now.checked_sub(queue::RETRY_DELAY).unwrap_or(now);
                let (at, tries) = self.failures.get(&id).copied().unwrap_or((long_ago, 0));
                Member::Failed { at, tries }
            }
        }
    }

    /// Scheduler times, then start what running queues allow (notes/ENGINE.md §5).
    fn tick_queues(&mut self) {
        use chrono::Datelike;
        let now_s = seconds_now();
        let prev = std::mem::replace(&mut self.clock, now_s);
        let today = chrono::Local::now().date_naive();
        let weekday = today.weekday().num_days_from_sunday() as usize;
        let date = (today.year(), today.month(), today.day());
        for q in 0..self.queues.len() {
            let sch = self.queues[q].schedule.clone();
            if let Some(t) = sch.start_at
                && sch.starts_on(date, weekday)
                && queue::crossed(t, prev, now_s)
            {
                self.queues[q].running = true;
            }
            if let Some(t) = sch.stop_at
                && queue::crossed(t, prev, now_s)
            {
                self.stop_queue(q);
            }
        }
        let now = Instant::now();
        for q in 0..self.queues.len() {
            if !self.queues[q].running {
                continue;
            }
            // Each download's standing, looked up once (a linear search per queued id before).
            let members = self.members(now);
            let starts = queue::to_start(&self.queues[q], |id| member_in(&members, id), now);
            for id in starts {
                self.start(id, false);
            }
            let members = self.members(now);
            if queue::finished(&self.queues[q], |id| member_in(&members, id)) {
                self.queues[q].running = false;
                self.queue_done(q);
            }
        }
    }

    /// Scheduler → Download limits: stop at the limit, go on when the window has room.
    fn check_limits(&mut self, now: Instant) {
        self.quota
            .record(now, self.items.iter().map(|i| (i.id, i.downloaded)));
        let s = &self.settings;
        let limit = u64::from(s.limits_mb.max(1)) << 20;
        let window = Duration::from_secs(u64::from(s.limits_hours.max(1)) * 3600);
        let change = if s.limits_on {
            self.quota.check(now, limit, window)
        } else if self.quota.blocked {
            self.quota.blocked = false;
            Some(false)
        } else {
            None
        };
        match change {
            Some(true) => {
                let running: Vec<u64> = self
                    .items
                    .iter()
                    .filter(|i| i.live.is_some())
                    .map(|i| i.id)
                    .collect();
                for id in running {
                    self.stop(id);
                    self.quota.held.push(id);
                }
                for id in self.quota.held.clone() {
                    if let Some(item) = self.item(id) {
                        item.status = Status::Queued;
                    }
                }
                if self.settings.limits_warn {
                    let wait = self.quota.room_in(now, limit, window);
                    self.limit_notice = Some(dialogs::LimitNotice::new(
                        self.settings.limits_mb,
                        self.settings.limits_hours,
                        wait,
                    ));
                }
                self.dirty = true;
            }
            Some(false) => {
                for id in std::mem::take(&mut self.quota.held) {
                    if let Some(item) = self.item(id) {
                        item.status = Status::Stopped;
                    }
                    self.start(id, false);
                }
                self.dirty = true;
            }
            None => {}
        }
    }

    /// A queue ran out of work: its "when done" actions (Scheduler).
    fn queue_done(&mut self, q: usize) {
        self.sound(3);
        let sch = self.queues[q].schedule.clone();
        if let Some(path) = &sch.open_when_done {
            let _ = open::that_detached(path);
        }
        self.when_done(sch.when_done());
    }

    /// A queue's, or a download's (its progress window's Options on completion), "when done":
    /// hang up, turn the computer off (a countdown first), or exit.
    fn when_done(&mut self, w: crate::queue::WhenDone) {
        // Ends dial-up (RAS) connections; nothing happens without one. Windows only: macOS and
        // Linux have no dial-up command to call.
        #[cfg(windows)]
        if w.hang_up {
            let _ = std::process::Command::new("rasdial")
                .arg("/disconnect")
                .spawn();
        }
        if let Some(mode) = w.turn_off {
            self.turn_off = Some(dialogs::TurnOffCountdown::new(mode, w.force));
        } else if w.exit {
            self.exit_requested = true;
        }
    }

    fn open_item(&mut self, id: u64) {
        let Some(i) = self.items.iter().find(|i| i.id == id) else {
            return;
        };
        if i.status == Status::Complete {
            crate::dialogs::open_file(&i.dest);
        } else if !self.progress.iter().any(|p| p.id == id) {
            self.progress
                .push(Progress::new(id, self.settings.show_complete));
        } else if let Some(p) = self.progress.iter_mut().find(|p| p.id == id && p.hidden) {
            // Hidden in the tray (#63): back, as it was.
            p.hidden = false;
        } else {
            // Open already, behind: to the front.
            dialogs::raise(
                &self.ui_ctx,
                egui::ViewportId::from_hash_of(("progress", id)),
            );
        }
    }

    /// `target`: the right-clicked download, if any. Stop, Resume, Remove and the queue
    /// commands act on every selected download (or the right-clicked one outside the
    /// selection); the others on one download.
    fn run(&mut self, cmd: Cmd, target: Option<u64>, ctx: &egui::Context) {
        // The log a crash report quotes: what was asked for (commands carry no addresses).
        tracing::info!("command {cmd:?}");
        let many = self.targets(target);
        let target = target.or(self.selection.one());
        let dest = target
            .and_then(|id| self.items.iter().find(|i| i.id == id))
            .map(|i| i.dest.clone());
        match cmd {
            Cmd::AddUrl => self.add_url = Some(AddUrl::from_clipboard(&self.settings.url_history)),
            Cmd::Exit => {
                self.exiting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Cmd::Stop => {
                for id in many {
                    if self.item(id).is_some_and(|i| i.live.is_some()) {
                        self.stop(id);
                    }
                }
            }
            Cmd::Remove => self.ask_delete(many, false),
            Cmd::RemoveWithFiles => self.ask_delete(many, true),
            Cmd::Resume => {
                for id in many {
                    if self.item(id).is_some_and(|i| i.can_start()) {
                        self.start(id, true);
                    }
                }
            }
            Cmd::StopAll => self.stop_all(),
            Cmd::DeleteCompleted => self.delete_completed(),
            Cmd::StartQueue(q) => self.start_queue(q),
            Cmd::StopQueue(q) => self.stop_queue(q),
            Cmd::Browse(c) => {
                let dir = self.settings.category_dir(c, &self.downloads);
                let _ = std::fs::create_dir_all(&dir);
                let _ = open::that_detached(dir);
            }
            Cmd::DoubleClickOpen(open) => {
                self.settings.double_click_open = open;
                self.dirty = true;
            }
            Cmd::AddToQueue(q) => {
                // "Move to queue" for a download already in one: it leaves that queue.
                let done: Vec<u64> = many
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.items
                            .iter()
                            .any(|i| i.id == *id && i.status == Status::Complete)
                    })
                    .collect();
                for id in many {
                    // Finished files go only to synchronization queues.
                    if done.contains(&id) && !self.queues[q].sync {
                        continue;
                    }
                    for (k, other) in self.queues.iter_mut().enumerate() {
                        if k != q {
                            other.items.retain(|x| *x != id);
                        }
                    }
                    if !self.queues[q].items.contains(&id) {
                        self.queues[q].items.push(id);
                    }
                }
                self.dirty = true;
            }
            Cmd::RemoveFromQueue => {
                for id in many {
                    for q in &mut self.queues {
                        q.items.retain(|x| *x != id);
                    }
                }
                self.dirty = true;
            }
            Cmd::ToggleCategories => self.show_categories = !self.show_categories,
            Cmd::ChooseLanguage => {
                if self.lang_picker.is_none() {
                    self.lang_picker = Some(crate::langpick::Picker::new(
                        false,
                        self.settings.language.as_deref(),
                    ));
                }
            }
            Cmd::Language(code) => {
                self.settings.language = code.map(str::to_owned);
                self.settings.language_asked = true;
                crate::i18n::set_lang(code);
                crate::theme::language_fonts(ctx);
                if let Some(tray) = &self.tray {
                    tray.relabel();
                }
                self.dirty = true;
                ctx.request_repaint();
            }
            Cmd::ToggleDarkMode => {
                self.settings.dark_mode_support = !self.settings.dark_mode_support;
                self.theme_checked = None;
                self.dirty = true;
            }
            Cmd::ToggleAlwaysDark => {
                self.settings.always_dark = !self.settings.always_dark;
                self.theme_checked = None;
                self.dirty = true;
            }
            Cmd::Open => {
                if let Some(id) = target {
                    self.open_item(id);
                }
            }
            Cmd::Properties => {
                if let Some(id) = target {
                    self.properties(id);
                }
            }
            Cmd::VerifyChecksum => {
                if let Some(id) = target
                    && !self.checksums.iter().any(|c| c.id == id)
                    && let Some(i) = self.items.iter().find(|i| i.id == id)
                {
                    let file = (i.status == Status::Complete).then(|| i.dest.clone());
                    self.checksums.push(crate::checksum::ChecksumDialog::new(
                        id,
                        i.checksum.as_ref(),
                        file,
                    ));
                }
            }
            Cmd::RefreshAddress => {
                // As IDM: the page the link came from opens, so a fresh link can be taken
                // there; Properties takes the new address and the download resumes from it.
                if let Some(id) = target {
                    let page = self
                        .items
                        .iter()
                        .find(|i| i.id == id)
                        .and_then(|i| i.context.referer.clone());
                    if let Some(page) = page {
                        let _ = open::that_detached(page);
                    }
                    self.properties(id);
                }
            }
            Cmd::OpenWith => {
                if let Some(d) = dest {
                    dialogs::open_with(&d);
                }
            }
            Cmd::OpenFolder => {
                if let Some(d) = dest {
                    dialogs::show_in_folder(&d);
                }
            }
            Cmd::ToggleColumn(i) => self.list.visible[i] = !self.list.visible[i],
            Cmd::LimiterOn | Cmd::LimiterOff => {
                self.settings.speed_limit_on = cmd == Cmd::LimiterOn;
                self.apply_speed_limit();
                self.dirty = true;
            }
            Cmd::LimiterSettings => {
                self.speed_settings = Some(dialogs::SpeedSettings::new(&self.settings));
            }
            Cmd::Find => {
                if self.find.is_none() {
                    self.find = Some(crate::find::Find::new(self.last_find.as_ref()));
                } else {
                    dialogs::raise(ctx, dialogs::dialog_viewport("find"));
                }
            }
            Cmd::FindNext => match self.last_find.clone() {
                Some(q) => self.find_next(&q),
                None => self.find = Some(crate::find::Find::new(None)),
            },
            Cmd::VideoPage => {
                if self.video_dlg.is_none() {
                    let dir = self.settings.category_dir(Category::Video, &self.downloads);
                    crate::videodlg::refresh_ytdlp(&self.rt);
                    self.video_dlg = Some(
                        crate::videodlg::VideoDialog::new(dir)
                            .with_subs(self.settings.video_subs.clone()),
                    );
                }
            }
            Cmd::AddMagnet => self.open_torrent_dialog(None, ctx),
            Cmd::HelpPage(page) => crate::help::open_page(page),
            Cmd::Shortcuts => self.shortcuts = Some(crate::help::Shortcuts),
            Cmd::ReportProblem => crate::help::report_problem(),
            Cmd::CheckUpdates => {
                if self.updates.is_none() {
                    self.updates = Some(crate::help::Updates::check(&self.rt, ctx));
                }
            }
            Cmd::About => self.about = Some(crate::help::About),
            Cmd::TellFriend => {
                if self.tell_friend.is_none() {
                    self.tell_friend = Some(crate::help::TellFriend::new());
                }
            }
            Cmd::TorrentSettings => {
                if self.torrent_settings.is_none() {
                    self.torrent_settings =
                        Some(crate::torrentdlg::TorrentSettings::new(&self.settings));
                }
            }
            Cmd::OpenTorrentFile => {
                let kind = crate::i18n::t("Torrent files").to_owned();
                let title = crate::i18n::t("Open torrent file").to_owned();
                winui::start_pick(ctx, "app-torrent-file", move || {
                    rfd::FileDialog::new()
                        .add_filter(kind, &["torrent"])
                        .set_title(title)
                        .pick_file()
                });
            }
            Cmd::Batch(clipboard) => {
                if self.batch.is_none() {
                    self.batch = Some(crate::batch::Batch::new(clipboard));
                }
            }
            Cmd::Options => {
                if self.options.is_none() {
                    self.options = Some(crate::options::OptionsDialog::new(
                        &self.settings,
                        &self.downloads,
                        self.host_limits.learned(),
                    ));
                } else {
                    dialogs::raise(ctx, dialogs::dialog_viewport("options"));
                }
            }
            Cmd::Scheduler => {
                if self.scheduler.is_none() {
                    self.scheduler = Some(crate::scheduler::Scheduler::new(
                        &self.queues,
                        &self.settings,
                    ));
                } else {
                    dialogs::raise(ctx, dialogs::dialog_viewport("scheduler"));
                }
            }
            Cmd::EditQueue(_) | Cmd::ScheduleQueue(_) | Cmd::NewQueue => {
                let s = self.scheduler.get_or_insert_with(|| {
                    crate::scheduler::Scheduler::new(&self.queues, &self.settings)
                });
                match cmd {
                    // Edit queue: its files; Schedule: when it runs.
                    Cmd::EditQueue(q) => s.show_queue(q, true),
                    Cmd::ScheduleQueue(q) => s.show_queue(q, false),
                    _ => s.new_queue(),
                }
            }
            // A dialog already open (half filled in) stays.
            Cmd::AddCategory => {
                if self.category_dlg.is_none() {
                    self.category_dlg = Some(crate::categories::CategoryDialog::new());
                }
            }
            Cmd::CategoryProps(c) => {
                let folder = self.settings.category_dir(c, &self.downloads);
                self.category_dlg = Some(crate::categories::CategoryDialog::edit(c, &folder));
            }
            // The user's categories and the built-in ones by file type (#93); not General nor
            // Torrents.
            Cmd::DeleteCategory(c) if c.editable() => {
                self.confirm_category_delete = Some(dialogs::DeleteCategory::new(c));
            }
            Cmd::DeleteCategory(_) => {}
            Cmd::ToggleDropTarget => {
                self.settings.drop_target = !self.settings.drop_target;
                self.dirty = true;
            }
            Cmd::ShowMain => self.show_window(ctx),
            Cmd::Export { ef2 } => self.export(ef2),
            Cmd::Import { ef2 } => self.import(ef2),
            Cmd::Redownload => {
                if let Some(id) = target
                    && self.can_redownload(id)
                    && let Some(item) = self.items.iter().find(|i| i.id == id)
                {
                    let text = crate::i18n::fmt(
                        crate::i18n::t(
                            "Download \"{name}\" again from the start? A finished file is replaced when the new download completes.",
                        ),
                        &[("name", &item.file_name)],
                    );
                    // i18n: window title, verb: download the file again from the start
                    let title = crate::i18n::mark("Redownload");
                    self.confirm_redownload = Some((id, dialogs::Confirm::new(title, text)));
                }
            }
            Cmd::MoveRename => {
                if let Some(id) = target {
                    self.move_rename(id);
                }
            }
            Cmd::DeleteQueue(q) => {
                if q >= 2 && q < self.queues.len() {
                    let name = self.queues[q].name.clone();
                    self.confirm_queue_delete = Some((
                        q,
                        dialogs::Confirm::new(
                            crate::i18n::mark("Delete queue"),
                            crate::i18n::fmt(
                                crate::i18n::t(
                                    "Delete the queue \"{queue}\"? Its downloads stay in the list.",
                                ),
                                &[("queue", &name)],
                            ),
                        ),
                    ));
                }
            }
            Cmd::Arrange(col) => self.list.sort = col.map(|c| (c, true)),
            Cmd::ClearFilters => self.list.filters = vec![None; list::COLS],
            Cmd::ResetColumns => self.list.reset_columns(),
            Cmd::ShowAll => self.view = View::All,
            Cmd::SelectAll => {
                let ids = self.shown_ids();
                self.selection.apply(crate::selection::Pick::All, &ids);
            }
            Cmd::None => {}
        }
    }

    /// Pull engine progress into the rows; open "Download complete" when one finishes.
    fn poll(&mut self, ctx: &egui::Context) {
        self.poll_moves();
        if !self.moving.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let mut finished = Vec::new();
        let mut failed = Vec::new();
        let mut any_live = false;
        for i in &mut self.items {
            if i.poll() {
                self.dirty = true;
                match i.status {
                    Status::Complete => finished.push(i.id),
                    Status::Error => failed.push(i.id),
                    _ => {}
                }
            }
            any_live |= i.live.is_some();
        }
        let now = Instant::now();
        self.check_limits(now);
        if !finished.is_empty() {
            self.sound(0);
        }
        if !failed.is_empty() {
            self.sound(1);
        }
        for id in failed {
            let e = self.failures.entry(id).or_insert((now, 0));
            *e = (now, e.1 + 1);
            for q in &mut self.queues {
                queue::requeue_failed(q, id);
            }
        }
        for id in &finished {
            self.failures.remove(id);
            // Finished files leave download queues (synchronization queues keep them).
            for q in self.queues.iter_mut().filter(|q| !q.sync) {
                q.items.retain(|x| x != id);
            }
        }
        self.tick_queues();
        // A download started early behind Download File Info stays quiet until the dialog's
        // answer (Start shows its box then; Cancel drops it).
        let early = self.file_info.as_ref().and_then(|f| f.early);
        for id in finished {
            // This file's "Show download complete dialog" (its progress window's Options on
            // completion tab), read before the window closes.
            let window = self.progress.iter().find(|p| p.id == id);
            let this_file = window.is_none_or(|p| p.shows_complete());
            if let Some(w) = window.and_then(Progress::when_done) {
                self.when_done(w);
            }
            self.progress.retain(|p| p.id != id);
            if Some(id) == early {
                continue;
            }
            if self.settings.show_complete
                && this_file
                && let Some(i) = self.items.iter().find(|i| i.id == id)
            {
                self.complete.push(Complete {
                    id,
                    url: i.url.to_string(),
                    path: i.dest.clone(),
                    size: i.size.unwrap_or(i.downloaded),
                    dont_show: false,
                    notices: i.notices.clone(),
                    seen: false,
                });
            }
        }
        if any_live {
            ctx.request_repaint_after(Duration::from_millis(250));
            // The list's progress figures, every 30 s while something runs: after a crash the
            // rows show about where each download was (torrents keep their own resume data,
            // files their state file; this is only what the list shows).
            if self.last_save.elapsed() > Duration::from_secs(30) {
                self.dirty = true;
            }
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(1) {
            // Written on the saver's thread (store::Saver); refused at once when the list
            // couldn't be read at start.
            if let Err(e) = self.saver.save(
                self.next_id,
                &self.items,
                &self.list.layout(),
                &self.settings,
                &self.queues,
            ) {
                self.report_save(Err(e.to_string()));
            }
            self.dirty = false;
            self.last_save = Instant::now();
        }
        if let Some(outcome) = self.saver.outcome() {
            self.report_save(outcome);
        }
    }

    /// A list that can't be saved says so, once for each reason (not every second); it was
    /// silent and a restart lost the changes.
    fn report_save(&mut self, outcome: Result<(), String>) {
        {
            match outcome {
                Ok(()) => self.save_failed = None,
                Err(e) => {
                    if self.save_failed.as_ref() != Some(&e) {
                        self.notice = Some((
                            crate::i18n::mark("Mister Download Manager"),
                            crate::i18n::fmt(
                                crate::i18n::t(
                                    "The download list could not be saved: {error}. Changes are kept while MDM runs.",
                                ),
                                &[("error", &e)],
                            ),
                        ));
                    }
                    self.save_failed = Some(e);
                }
            }
        }
    }

    // ---- menus -----------------------------------------------------------------------------

    /// IDM 6.42's menus (notes/ui-spec/menus.md); unbuilt features are greyed out.
    fn menu_entries(&self, index: usize) -> Vec<MenuEntry<Cmd>> {
        use MenuEntry::{Item as I, Sep, Sub};
        let sel = self.selected_item();
        let picked = self.chosen(None);
        let running = picked.iter().any(|i| i.live.is_some());
        let resumable = picked.iter().any(|i| i.can_start());
        let deletable = picked.iter().any(|i| i.live.is_none());
        let any_running = self.items.iter().any(|i| i.live.is_some());
        let off = |label| I(label, Cmd::None, false, false);
        match index {
            0 => vec![
                I("&Add new download", Cmd::AddUrl, true, false),
                I("Open &torrent file...", Cmd::OpenTorrentFile, true, false),
                I("Add &magnet link...", Cmd::AddMagnet, true, false),
                I("Torrent setti&ngs...", Cmd::TorrentSettings, true, false),
                I("Add &batch download", Cmd::Batch(false), true, false),
                I(
                    "Add batch download from &clipboard",
                    Cmd::Batch(true),
                    true,
                    false,
                ),
                I(
                    "Download &video from a page...",
                    Cmd::VideoPage,
                    true,
                    false,
                ),
                Sep,
                I(
                    "Show &drop target",
                    Cmd::ToggleDropTarget,
                    true,
                    self.settings.drop_target,
                ),
                Sep,
                // i18n: verb: save the download list to a file
                Sub(
                    "E&xport",
                    !self.items.is_empty(),
                    vec![
                        I(
                            "To &IDM export file...",
                            Cmd::Export { ef2: true },
                            true,
                            false,
                        ),
                        I("To &text file...", Cmd::Export { ef2: false }, true, false),
                    ],
                ),
                // i18n: verb: load downloads from a file
                Sub(
                    "&Import",
                    true,
                    vec![
                        I(
                            "From &IDM export file...",
                            Cmd::Import { ef2: true },
                            true,
                            false,
                        ),
                        I(
                            "From &text file...",
                            Cmd::Import { ef2: false },
                            true,
                            false,
                        ),
                    ],
                ),
                Sep,
                I("&Exit", Cmd::Exit, true, false),
            ],
            1 => vec![
                I("&Stop Download", Cmd::Stop, running, false),
                // i18n: verb: removes the selected download from the list
                I("&Remove", Cmd::Remove, deletable, false),
                // i18n: verb: removes the selected download from the list and its file from the disk
                I("Remove with &files", Cmd::RemoveWithFiles, deletable, false),
                I("&Download Now", Cmd::Resume, resumable, false),
                // i18n: verb: download the file again from the start
                I(
                    "R&edownload",
                    Cmd::Redownload,
                    sel.is_some_and(|i| self.can_redownload(i.id)),
                    false,
                ),
            ],
            2 => vec![
                I("&Pause All", Cmd::StopAll, any_running, false),
                I("&Stop All", Cmd::StopAll, any_running, false),
                Sep,
                I(
                    "&Delete All Completed",
                    Cmd::DeleteCompleted,
                    self.items.iter().any(|i| i.status == Status::Complete),
                    false,
                ),
                Sep,
                I("Find (Ctrl-F)", Cmd::Find, true, false),
                I("Find Next (F3)", Cmd::FindNext, true, false),
                I(
                    "Select &all",
                    Cmd::SelectAll,
                    !self.shown_ids().is_empty(),
                    false,
                ),
                Sep,
                I("S&cheduler", Cmd::Scheduler, true, false),
                Sub("Start &queue", true, self.queue_drop(true)),
                Sub("S&top queue", true, self.queue_drop(false)),
                Sep,
                Sub(
                    "Speed &Limiter",
                    true,
                    vec![
                        I(
                            "Turn o&n",
                            Cmd::LimiterOn,
                            true,
                            self.settings.speed_limit_on,
                        ),
                        I(
                            "Turn of&f",
                            Cmd::LimiterOff,
                            true,
                            !self.settings.speed_limit_on,
                        ),
                        Sep,
                        I("S&ettings...", Cmd::LimiterSettings, true, false),
                    ],
                ),
                Sep,
                I("&Options", Cmd::Options, true, false),
            ],
            3 => vec![
                I(
                    if self.show_categories {
                        crate::i18n::mark("Hide &categories")
                    } else {
                        crate::i18n::mark("Show &categories")
                    },
                    Cmd::ToggleCategories,
                    true,
                    false,
                ),
                self.arrange_menu(),
                // i18n: noun: the toolbar (submenu of View with its settings)
                Sub("Toolbar", false, vec![]),
                Sub("&Tray icon", false, vec![]),
                off("C&ustomize URL List..."),
                I(
                    "Dark Mode support",
                    Cmd::ToggleDarkMode,
                    true,
                    self.settings.dark_mode_support,
                ),
                // MDM's (#60): dark whatever the system says (a desktop that reports nothing).
                I(
                    // i18n: View menu: a check item; dark mode on, whatever the system's setting
                    "Always dark",
                    Cmd::ToggleAlwaysDark,
                    true,
                    self.settings.always_dark,
                ),
                // i18n: noun: the list's font (submenu)
                Sub("Font", false, vec![]),
                Sep,
                // IDM's place for it; a window rather than IDM's submenu, which 86 languages
                // would make taller than the screen (langpick.rs).
                I("&Language...", Cmd::ChooseLanguage, true, false),
            ],
            // IDM's Help menu, ours (D-020): pages of the user guide (docs/); "Video downloads" stands
            // where IDM has "Grabber Help"; "Report a problem" where "Contact IDM Support";
            // Keyboard shortcuts added; no Tip of the Day.
            _ => vec![
                I("H&elp contents", Cmd::HelpPage("README.md"), true, false),
                I(
                    "&Tutorials",
                    Cmd::HelpPage("README.md#tutorials"),
                    true,
                    false,
                ),
                I(
                    "&Scheduler and queues",
                    Cmd::HelpPage("queues-and-scheduler.md"),
                    true,
                    false,
                ),
                I("&Video downloads", Cmd::HelpPage("video.md"), true, false),
                I("&Keyboard shortcuts", Cmd::Shortcuts, true, false),
                Sep,
                I(
                    "Mister Download Manager &Home Page",
                    Cmd::HelpPage(""),
                    true,
                    false,
                ),
                I("&Report a problem", Cmd::ReportProblem, true, false),
                Sep,
                I("Check for &updates...", Cmd::CheckUpdates, true, false),
                Sep,
                I("&About Mister Download Manager", Cmd::About, true, false),
                I("Tell a &friend", Cmd::TellFriend, true, false),
            ],
        }
    }

    /// Start Queue / Stop Queue drop-downs (and the Downloads menu's submenus).
    fn queue_drop(&self, start: bool) -> Vec<MenuEntry<Cmd>> {
        let label = |q: usize| {
            let template = if start {
                crate::i18n::t("Start {queue}")
            } else {
                crate::i18n::t("Stop {queue}")
            };
            crate::chrome::label(&crate::i18n::fmt(
                template,
                // A name's "&" is text, not an access key (Tom & Jerry lost it).
                &[(
                    "queue",
                    &crate::i18n::t(&self.queues[q].name).replace('&', "&&"),
                )],
            ))
        };
        (0..self.queues.len())
            .map(|q| {
                let running = self.queues[q].running;
                if start {
                    MenuEntry::Item(label(q), Cmd::StartQueue(q), !running, false)
                } else {
                    MenuEntry::Item(label(q), Cmd::StopQueue(q), running, false)
                }
            })
            .collect()
    }

    /// IDM 6.42's list menu (notes/ui-spec/menus.md "Context menus"): on a row, or on blank
    /// space (`None`: nothing selected, almost everything greyed).
    /// The list's right-click menu. With several downloads selected it acts on all of them
    /// (Resume, Stop, Remove, queues); commands for one download need one.
    /// View → Arrange files, also on the list's blank space.
    fn arrange_menu(&self) -> MenuEntry<Cmd> {
        use MenuEntry::{Item as I, Sub};
        let off = |label| I(label, Cmd::None, false, false);
        Sub("Arrange files", true, {
            let sorted = self.list.sort.map(|s| s.0);
            let by = |label, col: Option<usize>| I(label, Cmd::Arrange(col), true, sorted == col);
            vec![
                by("By &Order Of Addition", None),
                by("By &File Name", Some(0)),
                by("By &Size", Some(2)),
                by("By St&atus", Some(3)),
                by("By &Time Left", Some(4)),
                by("By Transfer &Rate", Some(5)),
                by("By &Last Try Date", Some(6)),
                by("By Date &Added", Some(list::ADDED)),
                by("By &Description", Some(7)),
                off("By Save &Path"),
                off("By R&eferer"),
                off("By Parent &Web Page"),
            ]
        })
    }

    /// Right-click on the list's blank space: what applies to the whole list (IDM greys the
    /// download menu there).
    fn blank_entries(&self) -> Vec<MenuEntry<Cmd>> {
        use MenuEntry::{Item as I, Sep};
        vec![
            I("&Add new download...", Cmd::AddUrl, true, false),
            Sep,
            I(
                "Select &all",
                Cmd::SelectAll,
                !self.shown_ids().is_empty(),
                false,
            ),
            self.arrange_menu(),
            I(
                "Clear all &filters",
                Cmd::ClearFilters,
                self.list.filters.iter().any(Option::is_some),
                false,
            ),
            Sep,
            I(
                "&Delete All Completed",
                Cmd::DeleteCompleted,
                self.items.iter().any(|i| i.status == Status::Complete),
                false,
            ),
        ]
    }

    fn item_entries(&self, id: Option<u64>) -> Vec<MenuEntry<Cmd>> {
        use MenuEntry::{Item as I, Radio, Sep, Sub};
        let chosen = if id.is_some() {
            self.chosen(id)
        } else {
            Vec::new()
        };
        let it = (chosen.len() == 1).then(|| chosen[0]);
        let sel = it.is_some();
        let done = it.is_some_and(|i| i.status == Status::Complete);
        let live = it.is_some_and(|i| i.live.is_some());
        let any = !chosen.is_empty();
        let any_live = chosen.iter().any(|i| i.live.is_some());
        let any_resumable = chosen.iter().any(|i| i.can_start());
        let any_idle = chosen.iter().any(|i| i.live.is_none());
        let all_done = any && chosen.iter().all(|i| i.status == Status::Complete);
        let queue_of = |q: &Queue| chosen.iter().all(|i| q.items.contains(&i.id)) && any;
        let in_queue = self
            .queues
            .iter()
            .any(|q| chosen.iter().any(|i| q.items.contains(&i.id)));
        // The other queues they can go to; finished files only to synchronization queues.
        let targets: Vec<MenuEntry<Cmd>> = self
            .queues
            .iter()
            .enumerate()
            .filter(|(_, q)| !queue_of(q) && (!all_done || q.sync))
            .map(|(q, queue)| {
                I(
                    crate::chrome::label(&crate::i18n::t(&queue.name).replace('&', "&&")),
                    Cmd::AddToQueue(q),
                    true,
                    false,
                )
            })
            .chain([I("Create new queue", Cmd::NewQueue, true, false)])
            .collect();
        let open = self.settings.double_click_open;
        let movable = it.is_some_and(|i| !live && (done || i.downloaded == 0));
        let redo = it.is_some_and(|i| self.can_redownload(i.id));
        vec![
            // i18n: verb: opens the downloaded file
            I("Open", Cmd::Open, done, false),
            I("Open with...", Cmd::OpenWith, done, false),
            I("Open folder", Cmd::OpenFolder, done, false),
            Sep,
            I("Move/Rename (Ctrl-M)", Cmd::MoveRename, movable, false),
            Sep,
            // i18n: verb: download the file again from the start
            I("Redownload", Cmd::Redownload, redo, false),
            Sep,
            I("Resume Download", Cmd::Resume, any_resumable, false),
            I("Stop Download", Cmd::Stop, any_live, false),
            Sep,
            I(
                "Refresh download address",
                Cmd::RefreshAddress,
                sel && !live && !done,
                false,
            ),
            I("Verify checksum...", Cmd::VerifyChecksum, sel, false),
            Sep,
            // i18n: verb: removes the download from the list
            I("Remove", Cmd::Remove, any_idle, false),
            // i18n: verb: removes the download from the list and its file from the disk
            I("Remove with files", Cmd::RemoveWithFiles, any_idle, false),
            Sep,
            Sub(
                if in_queue {
                    crate::i18n::mark("Move to queue")
                } else {
                    crate::i18n::mark("Add to queue")
                },
                any,
                targets,
            ),
            I("Delete from queue", Cmd::RemoveFromQueue, in_queue, false),
            Sep,
            Sub(
                "On Double click",
                true,
                vec![
                    // i18n: what double-clicking a download does — verb: open the file
                    Radio("Open", Cmd::DoubleClickOpen(true), true, open),
                    // i18n: what double-clicking a download does — noun: show its properties
                    Radio("Properties", Cmd::DoubleClickOpen(false), true, !open),
                ],
            ),
            Sep,
            // i18n: noun: opens the download's Properties window
            I("Properties", Cmd::Properties, sel, false),
        ]
    }

    /// Categories right-click: categories (Browse, Properties, Add category, Delete; greyed
    /// on the built-in nodes) and queues (Start now, Stop, Edit queue, Schedule, Delete,
    /// Create new queue).
    fn node_entries(&self, node: Option<View>) -> Vec<MenuEntry<Cmd>> {
        use MenuEntry::{Item as I, Sep};
        let off = |label| I(label, Cmd::None, false, false);
        let queue = match node {
            Some(View::Queue(q)) => Some(q),
            None => None,
            Some(v) => {
                let cat = match v {
                    View::Cat(c) | View::Unfinished(Some(c)) | View::Finished(Some(c)) => Some(c),
                    _ => None,
                };
                return vec![
                    match cat {
                        // i18n: verb: opens this category's download folder
                        Some(c) => I("Browse", Cmd::Browse(c), true, false),
                        // i18n: verb: opens this category's download folder
                        None => off("Browse"),
                    },
                    match cat {
                        // i18n: noun: opens the category's properties
                        Some(c) => I("Properties", Cmd::CategoryProps(c), true, false),
                        // i18n: noun: opens the category's properties
                        None => off("Properties"),
                    },
                    Sep,
                    I("Add category", Cmd::AddCategory, true, false),
                    match cat {
                        // i18n: verb: deletes the category
                        Some(c) if c.editable() => I("Delete", Cmd::DeleteCategory(c), true, false),
                        // i18n: verb: deletes the category
                        _ => off("Delete"),
                    },
                ];
            }
        };
        let running = queue.is_some_and(|q| self.queues[q].running);
        vec![
            match queue {
                Some(q) => I("Start now", Cmd::StartQueue(q), !running, false),
                None => off("Start now"),
            },
            match queue {
                // i18n: verb: stops the queue
                Some(q) => I("Stop", Cmd::StopQueue(q), running, false),
                // i18n: verb: stops the queue
                None => off("Stop"),
            },
            Sep,
            match queue {
                Some(q) => I("Edit queue", Cmd::EditQueue(q), true, false),
                None => off("Edit queue"),
            },
            match queue {
                // i18n: command: opens this queue's schedule (when it starts and stops)
                Some(q) => I("Schedule", Cmd::ScheduleQueue(q), true, false),
                // i18n: command: opens this queue's schedule (when it starts and stops)
                None => off("Schedule"),
            },
            Sep,
            // The two built-in queues cannot be deleted.
            match queue {
                // i18n: verb: deletes the queue
                Some(q) if q >= 2 => I("Delete", Cmd::DeleteQueue(q), !running, false),
                // i18n: verb: deletes the queue
                _ => off("Delete"),
            },
            Sep,
            I("Create new queue", Cmd::NewQueue, true, false),
        ]
    }

    // ---- painting --------------------------------------------------------------------------

    /// The Categories tree: each row, the view it selects (None: just toggles) and its branch.
    fn tree_rows(&self) -> Vec<(TreeRow<'static>, Option<View>, Branch)> {
        let cur = self.view;
        let row = |level, label: &'static str, expander, icon, badge, view: Option<View>| TreeRow {
            level,
            label: label.into(),
            expander,
            selected: view == Some(cur),
            icon,
            badge,
            count: None,
        };
        let mut rows = Vec::new();
        // Silk icons in IDM's roles: folder, folder + green arrow, folder + tick, page stacks.
        let mut branch =
            |label, open: bool, icon, badge, view: View, b: Branch, child: fn(Category) -> View| {
                rows.push((
                    row(0, label, Some(open), IconKey::Silk(icon), badge, Some(view)),
                    Some(view),
                    b,
                ));
                if open {
                    for c in Category::named() {
                        let v = child(c);
                        rows.push((
                            TreeRow {
                                level: 1,
                                label: c.label(),
                                expander: None,
                                selected: v == cur,
                                icon: category_icon(c),
                                badge: None,
                                count: None,
                            },
                            Some(v),
                            Branch::Leaf,
                        ));
                    }
                }
            };
        branch(
            "All Downloads",
            self.all_open,
            "folder",
            None,
            View::All,
            Branch::All,
            View::Cat,
        );
        branch(
            // i18n: adjective: the downloads that are not complete yet
            "Unfinished",
            self.unfinished_open,
            "folder_go",
            None,
            View::Unfinished(None),
            Branch::Unfinished,
            |c| View::Unfinished(Some(c)),
        );
        branch(
            // i18n: adjective: the downloads that are complete
            "Finished",
            self.finished_open,
            "folder",
            Some("tick"),
            View::Finished(None),
            Branch::Finished,
            |c| View::Finished(Some(c)),
        );
        let stack = IconKey::Silk("page_white_stack");
        rows.push((
            row(
                0,
                // i18n: plural noun: the download queues
                "Queues",
                Some(self.queues_open),
                IconKey::Silk("folder"),
                Some("page_white_stack"),
                None,
            ),
            None,
            Branch::Queues,
        ));
        if self.queues_open {
            // Every queue, the user's too (IDM lists them all here).
            for (q, queue) in self.queues.iter().enumerate() {
                let view = Some(View::Queue(q));
                rows.push((
                    TreeRow {
                        level: 1,
                        label: queue.name.clone().into(),
                        expander: None,
                        selected: view == Some(cur),
                        icon: stack.clone(),
                        badge: queue.sync.then_some("tick"),
                        count: None,
                    },
                    view,
                    Branch::Leaf,
                ));
            }
        }
        // Each top node's downloads, as its list would show them (before column filters):
        // the pane is narrow, its children's names fill it already.
        for (row, view, _) in &mut rows {
            if row.level == 0
                && let Some(v) = view
            {
                let n = self.items.iter().filter(|i| self.in_view(*v, i)).count();
                row.count = (n > 0).then_some(n);
            }
        }
        rows
    }

    fn open_popup(&mut self, kind: Popup, at: Pos2) {
        self.popup_serial += 1;
        self.popup = Some((kind, at, self.popup_serial));
    }

    fn toggle(&mut self, b: Branch) {
        match b {
            Branch::All => self.all_open = !self.all_open,
            Branch::Unfinished => self.unfinished_open = !self.unfinished_open,
            Branch::Finished => self.finished_open = !self.finished_open,
            Branch::Queues => self.queues_open = !self.queues_open,
            Branch::Leaf => {}
        }
    }

    fn paint(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let o = full.min;
        let (w, h) = (full.width(), full.height());
        let mut chosen: Option<(Cmd, Option<u64>)> = None;
        // The client area, laid out from the right in a right-to-left interface (winui).
        let _surface = winui::Surface::enter(o.x, o.x + w);

        // The menu bar from the keyboard: F10 or Alt pressed alone opens its first menu, Alt and a
        // menu's underlined letter that menu (Windows and Linux); on macOS, whose apps have no
        // Alt keys, Ctrl+F2 (the system's key for the menu bar).
        if self.open_menu.is_none() && self.popup.is_none() && ctx.memory(|m| m.focused().is_none())
        {
            let labels: Vec<String> = chrome::menus(ui.painter())
                .iter()
                .map(|(l, ..)| chrome::menu_letter(l))
                .collect();
            let (alt, other, f10, ctrl_f2, letter) = ctx.input(|i| {
                let mut letter = None;
                let mut other = false;
                for e in &i.events {
                    match e {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } => {
                            other = true;
                            if modifiers.alt && !modifiers.ctrl && !modifiers.command {
                                letter = Some(key.name().to_lowercase());
                            }
                        }
                        egui::Event::PointerButton { pressed: true, .. } => other = true,
                        // A letter of another script (Cyrillic, Arabic...) typed with Alt.
                        egui::Event::Text(t) if i.modifiers.alt && !i.modifiers.ctrl => {
                            letter = Some(t.to_lowercase());
                        }
                        _ => {}
                    }
                }
                (
                    i.modifiers.alt,
                    other,
                    i.key_pressed(egui::Key::F10) && i.modifiers.is_none(),
                    i.key_pressed(egui::Key::F2) && i.modifiers.ctrl,
                    letter,
                )
            });
            let mut open = None;
            if cfg!(target_os = "macos") {
                if ctrl_f2 {
                    open = Some(0);
                }
            } else {
                if alt && !self.alt_down {
                    self.alt_alone = true;
                    // The keyboard is in use: access letters show (light mode too).
                    crate::chrome::show_cues(&ctx);
                }
                if alt && other {
                    self.alt_alone = false;
                }
                if !alt && self.alt_down && self.alt_alone {
                    open = Some(0);
                }
                if f10 {
                    open = Some(0);
                }
                if let Some(l) = letter {
                    open = labels.iter().position(|m| *m == l).or(open);
                }
            }
            self.alt_down = alt;
            if let Some(i) = open {
                self.alt_alone = false;
                self.open_menu = Some(i);
                chrome::start_keyboard(&ctx, &format!("menu-{i}"));
            }
        } else {
            self.alt_down = false;
            self.alt_alone = false;
        }

        // Menu bar and its popups.
        if let Some(i) = chrome::menu_bar(ui, o, w, self.open_menu) {
            let clicked = ui.input(|x| x.pointer.any_released()) || chrome::menubar_collapse(&ctx);
            self.open_menu = if self.open_menu == Some(i) && clicked {
                None
            } else {
                Some(i)
            };
        }
        if let Some(i) = self.open_menu {
            let x = chrome::menus(ui.painter())[i].1;
            let entries = self.menu_entries(i);
            // Under the item's leading edge (the menu's top-right in RTL).
            let at = pos2(winui::mx(o.x + x), o.y + chrome::MENU_H - 1.0);
            match chrome::popup(&ctx, &format!("menu-{i}"), at, &entries, &|c: &Cmd| {
                c.hint()
            }) {
                PopupResult::Open => {}
                PopupResult::Neighbor(d) => {
                    let n = chrome::menus(ui.painter()).len() as i32;
                    let j = (i as i32 + d).rem_euclid(n) as usize;
                    self.open_menu = Some(j);
                    chrome::start_keyboard(&ctx, &format!("menu-{j}"));
                }
                PopupResult::Chosen(c) => {
                    chosen = Some((c, None));
                    self.open_menu = None;
                }
                PopupResult::Dismissed => {
                    // Clicks on the bar itself are handled by `menu_bar`.
                    let on_bar = ctx
                        .pointer_interact_pos()
                        .is_some_and(|p| p.y < o.y + chrome::MENU_H);
                    if !on_bar {
                        self.open_menu = None;
                    }
                }
            }
        }

        // Toolbar — full labels (IDM truncates "Delete Co...", "Start Qu...": user decision).
        let picked = self.chosen(None);
        let running = picked.iter().any(|i| i.live.is_some());
        let resumable = picked.iter().any(|i| i.can_start());
        let deletable = picked.iter().any(|i| i.live.is_none());
        let any_running = self.items.iter().any(|i| i.live.is_some());
        let any_complete = self.items.iter().any(|i| i.status == Status::Complete);
        let buttons = [
            (Icon::AddUrl, "Add URL", true, Cmd::AddUrl),
            (Icon::AddTorrent, "Add Torrent", true, Cmd::OpenTorrentFile),
            // i18n: verb: continue the selected downloads
            (Icon::Resume, "Resume", resumable, Cmd::Resume),
            // i18n: verb: stop the selected downloads
            (Icon::Stop, "Stop", running, Cmd::Stop),
            (Icon::StopAll, "Stop All", any_running, Cmd::StopAll),
            // i18n: verb: delete the selected downloads
            (Icon::Delete, "Delete", deletable, Cmd::Remove),
            (
                Icon::DeleteCompleted,
                "Delete Completed",
                any_complete,
                Cmd::DeleteCompleted,
            ),
            // i18n: plural noun: opens the Options window (settings)
            (Icon::Options, "Options", true, Cmd::Options),
            // i18n: noun: the tool that starts and stops queues on time
            (Icon::Scheduler, "Scheduler", true, Cmd::Scheduler),
            (
                Icon::StartQueue,
                "Start Queue",
                // The button acts on the main queue (the others: its arrow, the menus).
                self.queues.first().is_some_and(|q| !q.running),
                Cmd::StartQueue(0),
            ),
            (
                Icon::StopQueue,
                "Stop Queue",
                self.queues.first().is_some_and(|q| q.running),
                Cmd::StopQueue(0),
            ),
        ];
        let tb: Vec<(Icon, &str, bool)> = buttons.iter().map(|(i, l, e, _)| (*i, *l, *e)).collect();
        match chrome::toolbar(ui, o, w, &tb) {
            Some(ToolbarEvent::Click(k)) => chosen = Some((buttons[k].3, None)),
            Some(ToolbarEvent::Drop(k, at)) => match buttons[k].0 {
                Icon::AddTorrent => self.open_popup(Popup::TorrentDrop, at),
                icon => self.open_popup(Popup::QueueDrop(matches!(icon, Icon::StartQueue)), at),
            },
            None => {}
        }

        // Categories pane.
        let list_left = if self.show_categories {
            let rows = self.tree_rows();
            let tree: Vec<TreeRow<'_>> = rows.iter().map(|(r, ..)| r.clone()).collect();
            // The splitter: the 4 px gap between the pane and the list, dragged sideways.
            let max_w = (w * 0.6).max(100.0);
            let cat_w = (self.settings.categories_width as f32).clamp(100.0, max_w);
            let right = o.x + 6.0 + cat_w;
            let split = winui::rect(egui::Rect::from_min_max(
                pos2(right, o.y + chrome::MENU_H + chrome::pane_top()),
                pos2(right + 4.0, o.y + h - STRIP_H - 5.0),
            ));
            let sr = ui.interact(
                split,
                egui::Id::unique("cat-splitter"),
                winui::pointer_drag(),
            );
            if sr.hovered() || sr.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if sr.dragged()
                && let Some(p) = sr.interact_pointer_pos()
            {
                // The pointer stays on the splitter's middle (the pane's width from its leading
                // edge: the right in RTL).
                let nw = (winui::mx(p.x) - o.x - 6.0 - 2.0)
                    .round()
                    .clamp(100.0, max_w);
                self.settings.categories_width = nw as u32;
            }
            if sr.drag_stopped() {
                self.dirty = true;
            }
            // The tree's keys while it has them (Tab moves between it and the list): Up / Down
            // through the nodes, Left / Right close and open a branch, Home / End.
            let free = self.popup.is_none()
                && self.open_menu.is_none()
                && ctx.memory(|m| m.focused().is_none());
            if free
                && ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                        || i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab)
                })
            {
                self.tree_keys = !self.tree_keys;
                self.list.set_keys(!self.tree_keys);
                crate::chrome::show_cues(&ctx);
            }
            let mut tree_event = None;
            if free && self.tree_keys {
                let at = rows
                    .iter()
                    .position(|(_, v, _)| *v == Some(self.view))
                    .unwrap_or(0);
                let with_view: Vec<usize> =
                    (0..rows.len()).filter(|k| rows[*k].1.is_some()).collect();
                let pos = with_view.iter().position(|k| *k == at).unwrap_or(0);
                let key = |k| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
                let to = if key(egui::Key::ArrowDown) {
                    with_view.get(pos + 1).copied()
                } else if key(egui::Key::ArrowUp) {
                    pos.checked_sub(1).and_then(|p| with_view.get(p).copied())
                } else if key(egui::Key::Home) {
                    with_view.first().copied()
                } else if key(egui::Key::End) {
                    with_view.last().copied()
                } else {
                    None
                };
                if let Some(k) = to {
                    tree_event = Some(TreeEvent::Select(k));
                }
                let open = rows[at].0.expander;
                let (left, right) = if winui::rtl() {
                    (egui::Key::ArrowRight, egui::Key::ArrowLeft)
                } else {
                    (egui::Key::ArrowLeft, egui::Key::ArrowRight)
                };
                if (open == Some(true) && key(left)) || (open == Some(false) && key(right)) {
                    tree_event = Some(TreeEvent::Toggle(at));
                }
            }
            if let Some(ev) = chrome::categories(
                ui,
                &mut self.icons,
                o,
                o.y + h - STRIP_H,
                right,
                &tree,
                self.tree_keys && free,
            )
            .or(tree_event)
            {
                // A click in the tree gives it the keys.
                self.tree_keys = true;
                self.list.set_keys(false);
                match ev {
                    TreeEvent::Close => self.show_categories = false,
                    TreeEvent::Select(k) => match rows[k].1 {
                        Some(v) => self.view = v,
                        None => self.toggle(rows[k].2),
                    },
                    TreeEvent::Toggle(k) => self.toggle(rows[k].2),
                    TreeEvent::Context(k, at) => {
                        // Leaves and nodes with a view; "Queues" (a branch without one) has
                        // the queue menu, all greyed.
                        let node = rows[k].1;
                        if node.is_some() || rows[k].2 == Branch::Queues {
                            self.open_popup(Popup::Node(node), at);
                        }
                    }
                }
            }
            6.0 + cat_w + 4.0
        } else {
            6.0
        };

        // Download list.
        let (ids, rows) = self.list_rows();
        self.selection.retain(&ids);
        let (selected, focus) = self.selection.rows(&ids);
        let outer = egui::Rect::from_min_max(
            pos2(o.x + list_left, o.y + chrome::MENU_H + chrome::pane_top()),
            pos2(o.x + w - 7.0, o.y + h - STRIP_H - 4.0),
        );
        self.list.menu_open = self.popup.is_some() || self.open_menu.is_some();
        // Another node shows from its top (its rows aren't the last one's).
        if self.list_view != self.view {
            self.list.scroll = 0.0;
            self.list_view = self.view;
        }
        // The Status cell's tip, the download's recent events: made for the row under the
        // pointer only (twelve dated lines for every row, every frame, before).
        let items = &self.items;
        let tip = |i: usize| {
            ids.get(i)
                .and_then(|id| items.iter().find(|it| it.id == *id))
                .map_or(String::new(), |it| it.log_lines(12))
        };
        let r = list::show(
            ui,
            outer,
            &mut self.list,
            &rows,
            &selected,
            focus,
            &mut self.icons,
            &tip,
        );
        if self.list.has_keys() {
            self.tree_keys = false;
        }
        if let Some(pick) = r.pick {
            self.selection.apply(pick, &ids);
        }
        if let Some(i) = r.properties {
            self.properties(ids[i]);
        }
        // A finished download: Open or Properties (IDM's default), as the context menu sets.
        if let Some(i) = r.double_clicked {
            let id = ids[i];
            let complete = self
                .items
                .iter()
                .any(|it| it.id == id && it.status == Status::Complete);
            if complete && !self.settings.double_click_open {
                self.properties(id);
            } else {
                self.open_item(id);
            }
        }
        if let Some(i) = r.enter {
            self.open_item(ids[i]);
        }
        // Something droppable over the window: the list is where it goes.
        let dragging = self.drops.over() || ctx.input(|i| !i.raw.hovered_files.is_empty());
        if dragging {
            ui.painter().rect_stroke(
                winui::rect(outer),
                0.0,
                egui::Stroke::new(2.0, winui::accent()),
                egui::StrokeKind::Inside,
            );
        }
        if ids.is_empty()
            && let Some(cmd) = self.empty_state(ui, r.body.unwrap_or(outer))
        {
            chosen = Some((cmd, None));
        }
        self.status_strip(ui, o, w, h, ids.len());
        if let Some((i, pos)) = r.context {
            self.open_popup(Popup::Item(i.map(|i| ids[i])), pos);
        }
        let layout = self.list.layout();
        if self.last_layout.as_ref() != Some(&layout) {
            self.dirty |= self.last_layout.is_some();
            self.last_layout = Some(layout);
        }
        if let Some(pos) = r.header_context {
            self.open_popup(Popup::Columns, pos);
        }
        if let Some((kind, pos, serial)) = self.popup {
            let (entries, target) = match kind {
                // Column chooser (IDM: right-click on the header). File Name always stays.
                Popup::Columns => {
                    let mut e: Vec<MenuEntry<Cmd>> = list::TITLES
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            // As the header names them (its own words, not the menus'), and
                            // the narrow Q column by its whole name.
                            let name = if *t == "Q" {
                                crate::i18n::tc("column", "Queue")
                            } else {
                                crate::i18n::tc("column", t)
                            };
                            let name = crate::chrome::label(&name.replace('&', "&&"));
                            MenuEntry::Item(
                                name,
                                Cmd::ToggleColumn(i),
                                i != 0,
                                self.list.visible[i],
                            )
                        })
                        .collect();
                    e.push(MenuEntry::Sep);
                    e.push(MenuEntry::Item(
                        "Clear all &filters",
                        Cmd::ClearFilters,
                        self.list.filters.iter().any(Option::is_some),
                        false,
                    ));
                    // i18n: header menu: put the columns back as they first were (order, widths, all shown)
                    e.push(MenuEntry::Item(
                        "&Reset columns",
                        Cmd::ResetColumns,
                        true,
                        false,
                    ));
                    (e, None)
                }
                Popup::Item(None) => (self.blank_entries(), None),
                Popup::Item(id) => (self.item_entries(id), id),
                Popup::QueueDrop(start) => (self.queue_drop(start), None),
                Popup::TorrentDrop => (
                    vec![
                        MenuEntry::Item("Open &torrent file...", Cmd::OpenTorrentFile, true, false),
                        MenuEntry::Item("Add &magnet link...", Cmd::AddMagnet, true, false),
                        MenuEntry::Sep,
                        MenuEntry::Item("Torrent setti&ngs...", Cmd::TorrentSettings, true, false),
                    ],
                    None,
                ),
                Popup::Node(node) => (self.node_entries(node), None),
                Popup::DropTarget => (
                    vec![
                        MenuEntry::Item("Show Mister Download Manager", Cmd::ShowMain, true, false),
                        MenuEntry::Item("&Add new download", Cmd::AddUrl, true, false),
                        MenuEntry::Sep,
                        MenuEntry::Item("Hide drop target", Cmd::ToggleDropTarget, true, false),
                    ],
                    None,
                ),
            };
            match chrome::popup(
                &ctx,
                &format!("popup-{serial}"),
                pos,
                &entries,
                &|c: &Cmd| c.hint(),
            ) {
                PopupResult::Open | PopupResult::Neighbor(_) => {}
                PopupResult::Chosen(c) => {
                    chosen = Some((c, target));
                    self.popup = None;
                }
                PopupResult::Dismissed => self.popup = None,
            }
        }
        // Ctrl+F finds, F3 finds the next match (Downloads menu); Ctrl+N adds a download (not
        // in IDM). The full list: Help → Keyboard shortcuts (`shortcuts.rs`).
        if chosen.is_none() && self.popup.is_none() && self.open_menu.is_none() {
            let (find, next, add) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::F),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::F3),
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::N),
                )
            });
            // Ctrl+M as IDM; F2 as renaming in Explorer.
            if ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::M)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::F2)
            }) {
                chosen = Some((Cmd::MoveRename, None));
            }
            if find {
                chosen = Some((Cmd::Find, None));
            } else if next {
                chosen = Some((Cmd::FindNext, None));
            } else if add {
                chosen = Some((Cmd::AddUrl, None));
            }
        }
        // Delete removes the selected downloads, Shift+Delete with their files (both ask first);
        // not while a text box (Find, a filter) has the keys.
        if chosen.is_none()
            && self.popup.is_none()
            && self.open_menu.is_none()
            && ctx.memory(|m| m.focused().is_none())
        {
            // On Windows egui reports Shift+Delete as Cut (Ctrl+X is Cut too, without Shift).
            let (del, shift_del) = ctx.input(|i| {
                (
                    i.key_pressed(egui::Key::Delete) && !i.modifiers.shift,
                    // Windows reports it as Cut; macOS and Linux as the key.
                    i.modifiers.shift
                        && !i.modifiers.command
                        && (i.events.iter().any(|e| matches!(e, egui::Event::Cut))
                            || i.key_pressed(egui::Key::Delete)),
                )
            });
            if shift_del {
                chosen = Some((Cmd::RemoveWithFiles, None));
            } else if del {
                chosen = Some((Cmd::Remove, None));
            }
        }
        if let Some((c, target)) = chosen {
            self.run(c, target, &ctx);
        }
    }

    /// Download File Info for an address from Add URL, the browser (`context`) or the command
    /// line. One at a time: another that arrives meanwhile waits its turn (the browser has
    /// already given its own copy up, so none may be dropped).
    fn open_file_info(&mut self, new: dialogs::NewUrl, context: ipc::Command, ctx: &egui::Context) {
        if self.file_info.is_some() {
            self.file_info_waiting.push_back((new, context));
            return;
        }
        let request = self.request_parts(&new.url, new.login.as_ref(), &context);
        let mut fi = FileInfo::new(
            new,
            context,
            request,
            self.downloads.clone(),
            self.settings.category_dirs.clone(),
            &self.rt,
            ctx,
        );
        fi.remember = self.settings.change_folder_on_last;
        fi.folders = self.settings.folder_history.clone();
        self.file_info = Some(fi);
    }

    /// Options → Downloads → "If a duplicate download link is added": true when adding `url`
    /// goes on now. 0 asks (`pending` waits), 1 adds it with a numbered name, 2 replaces the
    /// download already there, 3 opens that one (finished) or resumes it.
    fn duplicate_gate(&mut self, url: &url::Url, pending: impl FnOnce() -> Pending) -> bool {
        if std::mem::take(&mut self.duplicate_cleared) {
            return true;
        }
        let Some((dup, name)) = self
            .items
            .iter()
            .find(|i| i.url == *url)
            .map(|i| (i.id, i.file_name.clone()))
        else {
            return true;
        };
        match self.settings.duplicate_action {
            1 => true,
            2 => {
                self.overwrite(dup);
                true
            }
            3 => {
                self.open_or_resume(dup);
                false
            }
            _ => {
                // One question at a time; the others wait their turn (a second link while the
                // first was asked was dropped).
                let ask = (dup, pending(), dialogs::Duplicate::new(name));
                if self.duplicate.is_none() {
                    self.duplicate = Some(ask);
                } else {
                    self.duplicates_waiting.push_back(ask);
                }
                false
            }
        }
    }

    /// The user's answer to "Duplicate download link".
    fn resolve_duplicate(
        &mut self,
        dup: u64,
        pending: Pending,
        choice: usize,
        ctx: &egui::Context,
    ) {
        match choice {
            3 => {
                self.open_or_resume(dup);
                return;
            }
            2 => self.overwrite(dup),
            _ => {}
        }
        match pending {
            Pending::Command(cmd) => {
                self.duplicate_cleared = true;
                self.handle_command(*cmd, ctx);
                self.duplicate_cleared = false;
            }
            Pending::Url(new) => self.open_file_info(new, Default::default(), ctx),
        }
    }

    /// The new copy replaces the download already there: its file and its row go (a running
    /// one is left alone; the new copy then gets a numbered name).
    fn overwrite(&mut self, id: u64) {
        let Some(item) = self.items.iter().find(|i| i.id == id) else {
            return;
        };
        if item.live.is_some() {
            return;
        }
        let dest = item.dest.clone();
        if item.status == Status::Complete {
            let _ = std::fs::remove_file(&dest);
        }
        let _ = std::fs::remove_file(state_path(&dest));
        let _ = std::fs::remove_file(mdm_engine::naming::part_path(&dest));
        self.delete(id);
    }

    /// The download already there: its "Download complete" box when finished, else resumed.
    fn open_or_resume(&mut self, id: u64) {
        let Some(i) = self.items.iter().find(|i| i.id == id) else {
            return;
        };
        self.selection.set(id);
        if i.status == Status::Complete {
            self.complete.push(Complete {
                id,
                url: i.url.to_string(),
                path: i.dest.clone(),
                size: i.size.unwrap_or(i.downloaded),
                dont_show: false,
                notices: i.notices.clone(),
                seen: false,
            });
        } else if i.live.is_none() {
            self.start(id, true);
        } else if !self.progress.iter().any(|p| p.id == id) {
            self.progress
                .push(Progress::new(id, self.settings.show_complete));
        }
    }

    /// Tasks → Show drop target: the box while it's on; what is dropped on it is added as if
    /// given on the command line (an address, a magnet link, a .torrent file).
    fn drop_target(&mut self, ctx: &egui::Context) {
        if !self.settings.drop_target {
            self.drop_target = None;
            return;
        }
        // First shown: beside the main window's top-right corner, on the same screen.
        let pos = self
            .settings
            .drop_target_pos
            .map(|(x, y)| pos2(x as f32, y as f32))
            .or_else(|| {
                ctx.input(|i| i.viewport().outer_rect)
                    .map(|r| pos2(r.right() - 90.0, r.top() + 90.0))
            });
        let dt = self
            .drop_target
            .get_or_insert_with(|| crate::droptarget::DropTarget::new(pos));
        let icon = egui::include_image!("../assets/icons/app/mdm-48.png");
        for ev in dt.show(ctx, &icon) {
            use crate::droptarget::Event;
            match ev {
                Event::Drop(s) => self.dropped(vec![s], ctx),
                Event::ShowMain => self.show_window(ctx),
                Event::Context(screen) => {
                    // Popups take a point in the main window.
                    let origin = ctx
                        .input(|i| i.viewport().inner_rect)
                        .map_or(Pos2::ZERO, |r| r.min);
                    self.open_popup(Popup::DropTarget, screen - origin.to_vec2());
                }
                Event::Moved(p) => {
                    self.settings.drop_target_pos = Some((p.x.round() as i32, p.y.round() as i32));
                    self.dirty = true;
                }
            }
        }
    }

    /// Tasks → Export: every download's address, as an IDM export file or plain text.
    fn export(&mut self, ef2: bool) {
        let (ext, name) = if ef2 {
            ("ef2", "downloads.ef2")
        } else {
            ("txt", "downloads.txt")
        };
        let title = crate::i18n::t("Export").to_owned();
        let key = if ef2 {
            "app-export-ef2"
        } else {
            "app-export-txt"
        };
        winui::start_pick(&self.ui_ctx, key, move || {
            rfd::FileDialog::new()
                .set_title(title)
                .add_filter(ext.to_ascii_uppercase(), &[ext])
                .set_file_name(name)
                .save_file()
        });
    }

    /// Tasks → Export, the file chosen: the addresses written to it.
    fn export_to(&mut self, path: &std::path::Path, ef2: bool) {
        let text = if ef2 {
            crate::transfer::export_ef2(&self.items)
        } else {
            crate::transfer::export_text(&self.items)
        };
        if let Err(e) = std::fs::write(path, text) {
            self.notice = Some((
                crate::i18n::mark("Export"),
                crate::i18n::fmt(
                    crate::i18n::t("The list could not be saved: {error}"),
                    &[("error", &e.to_string())],
                ),
            ));
        }
    }

    /// The window's place and size, the Categories pane and the node shown, kept in the
    /// settings as they change (so they come back at the next start).
    fn remember_layout(&mut self, ctx: &egui::Context) {
        // Screen readers speak in the interface's language (its voice for Arabic, Japanese...).
        let lang = crate::i18n::code();
        if self.a11y_lang != lang {
            ctx.set_language(Some(lang.to_owned()));
            self.a11y_lang = lang;
        }
        let s = &mut self.settings;
        let pane = (
            self.show_categories,
            [
                self.all_open,
                self.unfinished_open,
                self.finished_open,
                self.queues_open,
            ],
            Some(self.view),
        );
        if (s.show_categories, s.branches, s.view) != pane {
            (s.show_categories, s.branches, s.view) = pane;
            self.dirty = true;
        }
        // Not a test instance's off-screen window, nor a zoomed one (screenshots).
        if crate::offscreen() || ctx.zoom_factor() != 1.0 {
            return;
        }
        let (outer, inner, maximized, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.maximized, v.minimized)
        });
        if minimized == Some(true) {
            return;
        }
        if let Some(m) = maximized {
            self.maximized = Some(m);
            if s.window_maximized != m {
                s.window_maximized = m;
                self.dirty = true;
            }
        }
        if self.maximized == Some(false)
            && let (Some(outer), Some(inner)) = (outer, inner)
        {
            let rect = [
                outer.min.x.round() as i32,
                outer.min.y.round() as i32,
                inner.width().round() as i32,
                inner.height().round() as i32,
            ];
            if s.window != Some(rect) {
                s.window = Some(rect);
                self.dirty = true;
            }
        }
    }

    /// What was dropped on the main window (its OLE target on Windows; winit's dropped files on
    /// macOS and Linux, which hand over files only).
    fn main_drops(&mut self, ctx: &egui::Context) {
        #[cfg_attr(windows, allow(unused_mut))]
        let mut got = self.drops.poll(ctx, crate::MAIN_TITLE);
        #[cfg(not(windows))]
        got.extend(ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().display().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
        }));
        if !got.is_empty() {
            self.dropped(got, ctx);
        }
    }

    /// Links or files dropped (on the main window, on the drop target): one address, a magnet
    /// link or a .torrent file goes the usual way (Download File Info, Add torrent); several
    /// addresses, or a list file (.txt, .ef2, .htm, .csv), show in Import to tick first.
    fn dropped(&mut self, items: Vec<String>, ctx: &egui::Context) {
        let list_file = |s: &str| {
            let p = std::path::Path::new(s);
            p.is_file()
                && p.extension().is_some_and(|e| {
                    ["txt", "ef2", "htm", "html", "csv"]
                        .contains(&e.to_string_lossy().to_ascii_lowercase().as_str())
                })
        };
        let (lists, items): (Vec<String>, Vec<String>) =
            items.into_iter().partition(|s| list_file(s));
        for file in lists {
            self.import_file(std::path::Path::new(&file));
        }
        let (links, other): (Vec<String>, Vec<String>) = items
            .into_iter()
            .partition(|s| url::Url::parse(s.trim()).is_ok_and(|u| mdm_engine::supported(&u)));
        if links.len() > 1 {
            self.show_window(ctx);
            let found = links
                .into_iter()
                .map(|url| crate::transfer::Imported {
                    url,
                    referer: None,
                    cookies: None,
                    user_agent: None,
                    private: false,
                })
                .collect();
            // i18n: in Import, where the addresses came from: dragged onto the window
            let source = crate::i18n::t("Dropped links").to_owned();
            self.show_import(found, source);
        } else {
            for url in links {
                self.handle_command(
                    ipc::Command {
                        url: Some(url),
                        ..Default::default()
                    },
                    ctx,
                );
            }
        }
        for s in other {
            let torrent = mdm_torrent::Source::magnet(&s).is_some()
                || (s.to_ascii_lowercase().ends_with(".torrent")
                    && std::path::Path::new(&s).is_file());
            if torrent {
                self.handle_command(
                    ipc::Command {
                        url: Some(s),
                        ..Default::default()
                    },
                    ctx,
                );
            } else {
                // A file MDM can't download or import (a picture, a program).
                self.notice = Some((
                    crate::i18n::mark("Mister Download Manager"),
                    crate::i18n::fmt(
                        crate::i18n::t(
                            "Nothing to download in {item}: drop links, a .torrent file, or a list of addresses (.txt, .ef2).",
                        ),
                        &[("item", &crate::i18n::isolate(&s))],
                    ),
                ));
            }
        }
    }

    /// Tasks → Import: read a file and show the addresses found, to check before adding.
    fn import(&mut self, ef2: bool) {
        let title = crate::i18n::t("Import").to_owned();
        // i18n: the kind of file the Import dialog lists (text files with addresses)
        let lists = crate::i18n::t("Lists of addresses").to_owned();
        let all = crate::i18n::t("All files").to_owned();
        winui::start_pick(&self.ui_ctx, "app-import", move || {
            let dialog = rfd::FileDialog::new().set_title(title);
            let dialog = if ef2 {
                dialog.add_filter("EF2", &["ef2"])
            } else {
                dialog.add_filter(lists, &["txt", "htm", "html", "csv"])
            };
            dialog.add_filter(all, &["*"]).pick_file()
        });
    }

    /// The system dialogs opened from the main window, answered (winui::start_pick): their
    /// work goes on.
    fn poll_picks(&mut self, ctx: &egui::Context) {
        if let Some(f) = winui::picked(ctx, "app-torrent-file") {
            self.open_torrent_dialog(Some(f.display().to_string()), ctx);
        }
        if let Some(path) = winui::picked(ctx, "app-export-ef2") {
            self.export_to(&path, true);
        }
        if let Some(path) = winui::picked(ctx, "app-export-txt") {
            self.export_to(&path, false);
        }
        if let Some(path) = winui::picked(ctx, "app-import") {
            self.import_file(&path);
        }
        if let Some(target) = winui::picked(ctx, "app-move-rename")
            && let Some(id) = self.move_rename_for.take()
        {
            self.start_move(id, target);
        }
    }

    /// The addresses in a list file, shown in Import to tick before adding.
    fn import_file(&mut self, path: &std::path::Path) {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let found = match std::fs::read(path) {
            Ok(bytes) => crate::transfer::import(&String::from_utf8_lossy(&bytes)),
            Err(e) => {
                self.notice = Some((
                    crate::i18n::mark("Import"),
                    crate::i18n::fmt(
                        crate::i18n::t("The file could not be read: {error}"),
                        &[("error", &e.to_string())],
                    ),
                ));
                return;
            }
        };
        if found.is_empty() {
            self.notice = Some((
                crate::i18n::mark("Import"),
                crate::i18n::fmt(
                    crate::i18n::t("No web addresses were found in {file}."),
                    &[("file", &file)],
                ),
            ));
            return;
        }
        self.show_import(found, file);
    }

    /// The checked addresses go to the list and the main queue, not started (IDM's import).
    /// Addresses to check before adding (Import, dropped links): into the Import window open
    /// already (several dropped lists, a list and links), else a new one; those in the list
    /// already are marked and unticked (imported twice, they were doubled).
    fn show_import(&mut self, found: Vec<crate::transfer::Imported>, source: String) {
        let known: std::collections::HashSet<String> =
            self.items.iter().map(|i| i.url.to_string()).collect();
        let dialog = self
            .import
            .get_or_insert_with(crate::transfer::ImportDialog::default);
        dialog.add(found, source, |u| {
            url::Url::parse(u).is_ok_and(|u| known.contains(u.as_str()))
        });
    }

    fn add_imported(&mut self, found: Vec<crate::transfer::Imported>) {
        for d in found {
            let Ok(url) = url::Url::parse(&d.url) else {
                continue;
            };
            let name = mdm_engine::naming::sanitize(&mdm_engine::naming::file_name(None, &url));
            let category = Category::of(&name);
            let dir = self.settings.category_dir(category, &self.downloads);
            let dest = mdm_engine::naming::destination(&dir, &name);
            let context = ipc::Command {
                referer: d.referer,
                cookies: d.cookies,
                user_agent: d.user_agent,
                private: d.private,
                ..Default::default()
            };
            let id = self.push_item(
                url,
                dest,
                category,
                String::new(),
                None,
                None,
                context,
                crate::i18n::mark("by import"),
            );
            self.name_guessed(id, true);
            self.queues[0].items.push(id);
        }
        self.dirty = true;
    }

    /// Can this download be fetched again from the start? A file download that isn't running
    /// (torrents re-check their data instead; a video's file is yt-dlp's to replace).
    fn can_redownload(&self, id: u64) -> bool {
        self.items
            .iter()
            .find(|i| i.id == id)
            .is_some_and(|i| i.live.is_none() && i.torrent.is_none() && i.video.is_none())
    }

    /// Download again from the start: the partial state goes; a finished file is replaced when
    /// the new download completes.
    fn redownload(&mut self, id: u64) {
        if !self.can_redownload(id) {
            return;
        }
        let Some(item) = self.item(id) else { return };
        let _ = std::fs::remove_file(state_path(&item.dest));
        let _ = std::fs::remove_file(mdm_engine::naming::part_path(&item.dest));
        item.downloaded = 0;
        item.status = Status::Stopped;
        item.error = None;
        item.note(crate::i18n::t("Downloading again from the start"));
        self.dirty = true;
        self.start(id, true);
    }

    /// Move/Rename: a Save As box at the file's place; a finished file is moved there, one
    /// that hasn't started gets the new name and folder.
    fn move_rename(&mut self, id: u64) {
        let Some(item) = self.items.iter().find(|i| i.id == id) else {
            return;
        };
        let complete = item.status == Status::Complete;
        if item.live.is_some() || !(complete || item.downloaded == 0) {
            // Said, not silently ignored (F2, Ctrl+M on a download that can't move).
            self.notice = Some((
                crate::i18n::mark("Move/Rename"),
                crate::i18n::t(
                    "Only a finished download, or one not started yet, can be moved. Stop it and finish it first.",
                )
                .to_owned(),
            ));
            return;
        }
        let title = crate::i18n::t("Move/Rename").to_owned();
        let from = item
            .dest
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        let name = item.file_name.clone();
        self.move_rename_for = Some(id);
        winui::start_pick(&self.ui_ctx, "app-move-rename", move || {
            rfd::FileDialog::new()
                .set_title(title)
                .set_directory(from)
                .set_file_name(name)
                .save_file()
        });
    }

    /// Delete a queue of the user's (not the two built-in ones); its downloads stay in the list.
    fn delete_queue(&mut self, q: usize) {
        if q < 2 || q >= self.queues.len() {
            return;
        }
        if self.queues[q].running {
            self.stop_queue(q);
        }
        self.queues.remove(q);
        // The tree's selection follows the queues after it, and leaves the deleted one.
        self.view = match self.view {
            View::Queue(v) if v == q => View::All,
            View::Queue(v) if v > q => View::Queue(v - 1),
            v => v,
        };
        self.dirty = true;
    }

    /// IDM looks for a new version once a week; so do we (off: the switch in Check for updates). Found: the
    /// update window says so; otherwise nothing shows.
    /// The automatic update check: a few seconds after every start, then once a day while MDM
    /// runs (it was at most once a week). A new version's window comes to the front, as apps
    /// announce theirs. Not in off-screen test runs (their windows stay as asked).
    fn auto_update_check(&mut self, ctx: &egui::Context) {
        let now = chrono::Utc::now().timestamp();
        let first = self.update_checked_this_run.is_none()
            && self.started.elapsed() >= crate::help::CHECK_AFTER_START;
        let again = self
            .update_checked_this_run
            .is_some_and(|t| now - t >= crate::help::CHECK_EVERY);
        if self.auto_update.is_none()
            && self.settings.update_check
            // Off-screen test runs check only when told where (MDM_UPDATE_FEED: their own feed).
            && (!crate::offscreen() || std::env::var_os("MDM_UPDATE_FEED").is_some())
            && (first || again)
        {
            self.update_checked_this_run = Some(now);
            self.settings.update_checked = now;
            self.dirty = true;
            self.auto_update = Some(crate::help::check(&self.rt, ctx));
        }
        if self.updates.is_some() {
            if self.raise_updates && crate::dialogs::focus_when_shown(ctx, "updates") {
                self.raise_updates = false;
            }
        } else {
            self.raise_updates = false;
        }
        if self.options.is_some() {
            if self.raise_options && crate::dialogs::focus_when_shown(ctx, "options") {
                self.raise_options = false;
            }
        } else {
            self.raise_options = false;
        }
        // Until the check at start is due.
        if self.update_checked_this_run.is_none() && self.settings.update_check {
            ctx.request_repaint_after(crate::help::CHECK_AFTER_START);
        }
        let done = self
            .auto_update
            .as_ref()
            .and_then(|s| s.lock().unwrap().take());
        if let Some(res) = done {
            self.auto_update = None;
            if let (Ok(crate::help::Found::Newer(rel)), None) = (res, &self.updates) {
                tracing::info!("update available: {}", rel.version);
                self.updates = Some(crate::help::Updates::found(rel));
                self.raise_updates = true;
            }
        }
    }

    fn help_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(d) = &mut self.lang_picker {
            let first = !self.settings.language_asked;
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Done(code) => {
                    // On the first start, keeping the preselected Windows language follows
                    // Windows from then on.
                    let code = code.filter(|c| !(first && *c == crate::i18n::system_code()));
                    self.lang_picker = None;
                    self.run(Cmd::Language(code), None, ctx);
                    crate::theme::interface_fonts(ctx);
                }
                Outcome::Closed => {
                    self.lang_picker = None;
                    crate::theme::interface_fonts(ctx);
                    // Cancelled on the first start: Windows' language, and don't ask again.
                    if first {
                        self.settings.language_asked = true;
                        self.dirty = true;
                    }
                }
            }
        }
        // Once the language is settled: the video helpers, offered once (helpers.rs).
        if self.lang_picker.is_none()
            && self.settings.language_asked
            && !self.settings.helpers_asked
        {
            self.settings.helpers_asked = true;
            self.dirty = true;
            if crate::helpers::worth_asking() {
                self.helpers = Some(crate::helpers::Helpers::new());
            }
        }
        if let Some(d) = &mut self.helpers
            && !matches!(d.show(ctx, &self.rt), Outcome::Open)
        {
            self.helpers = None;
        }
        if let Some(q) = &mut self.shown_existing
            && !matches!(q.show(ctx), Outcome::Open)
        {
            self.shown_existing = None;
        }
        // Then the browser extension, once per start until a browser has it (extension.rs).
        if self.lang_picker.is_none()
            && self.helpers.is_none()
            && self.settings.language_asked
            && !self.extension_weighed
        {
            self.extension_weighed = true;
            if crate::extension::worth_asking(&self.settings) {
                self.extension = Some(crate::extension::Extension::new());
            }
        }
        if let Some(d) = &mut self.extension {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Done(()) => {
                    self.extension = None;
                    self.settings.extension_never_ask = true;
                    self.dirty = true;
                }
                Outcome::Closed => self.extension = None,
            }
        }
        if let Some(d) = &mut self.about
            && !matches!(d.show(ctx), Outcome::Open)
        {
            self.about = None;
        }
        let mut weekly = self.settings.update_check;
        let mut close_for_update = false;
        if let Some(d) = &mut self.updates {
            match d.show(ctx, &self.rt, &mut weekly) {
                Outcome::Open => {}
                // Update now: the new copy is going in place; MDM saves and closes for it.
                Outcome::Done(true) => {
                    self.updates = None;
                    close_for_update = true;
                }
                _ => self.updates = None,
            }
        }
        if close_for_update {
            self.run(Cmd::Exit, None, ctx);
        }
        if weekly != self.settings.update_check {
            self.settings.update_check = weekly;
            self.dirty = true;
        }
        if let Some(d) = &mut self.shortcuts
            && !matches!(d.show(ctx), Outcome::Open)
        {
            self.shortcuts = None;
        }
        if let Some(d) = &mut self.tell_friend
            && !matches!(d.show(ctx), Outcome::Open)
        {
            self.tell_friend = None;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.help_dialogs(ctx);
        if self.duplicate.is_none() {
            self.duplicate = self.duplicates_waiting.pop_front();
        }
        if let Some((_, _, d)) = &mut self.duplicate {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.duplicate = None,
                Outcome::Done(choice) => {
                    if let Some((dup, pending, _)) = self.duplicate.take() {
                        self.resolve_duplicate(dup, pending, choice, ctx);
                    }
                }
            }
        }
        if let Some((_, d)) = &mut self.queue_pick {
            match d.show(ctx) {
                Outcome::Open => {}
                // Cancel: the downloads stay in the list, in no queue.
                Outcome::Closed => self.queue_pick = None,
                Outcome::Done((q, start)) => {
                    if let Some((ids, _)) = self.queue_pick.take()
                        && q < self.queues.len()
                    {
                        for id in ids {
                            if !self.queues[q].items.contains(&id) {
                                self.queues[q].items.push(id);
                            }
                        }
                        if start {
                            self.start_queue(q);
                        }
                        self.dirty = true;
                    }
                }
            }
        }
        if let Some(d) = &mut self.import {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.import = None,
                Outcome::Done(found) => {
                    self.import = None;
                    self.add_imported(found);
                }
            }
        }
        if let Some(d) = &mut self.category_dlg {
            match d.show(ctx, &crate::categories::names()) {
                Outcome::Open => {}
                Outcome::Closed => self.category_dlg = None,
                Outcome::Done(e) => {
                    self.category_dlg = None;
                    let made = crate::categories::apply(&mut self.settings, e, &self.downloads);
                    // Made with Download File Info's "+": that download goes in it.
                    if std::mem::take(&mut self.category_for_file_info)
                        && let Some(fi) = &mut self.file_info
                    {
                        fi.set_category(made);
                    }
                    self.dirty = true;
                }
            }
        }
        if let Some(d) = &mut self.confirm_category_delete {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.confirm_category_delete = None,
                Outcome::Done(remove) => {
                    let cat = d.category;
                    self.confirm_category_delete = None;
                    // Off the list first (not their files); the running ones can't go, so they
                    // move to General with the rest.
                    if remove {
                        let ids: Vec<u64> = self
                            .items
                            .iter()
                            .filter(|i| i.category == cat && i.live.is_none())
                            .map(|i| i.id)
                            .collect();
                        self.remove_downloads(&ids, false);
                    }
                    crate::categories::delete(&mut self.settings, cat, &mut self.items);
                    let gone = Some(cat);
                    if matches!(self.view, View::Cat(c) if Some(c) == gone)
                        || matches!(self.view, View::Unfinished(c) | View::Finished(c) if c == gone)
                    {
                        self.view = View::All;
                    }
                    self.dirty = true;
                }
            }
        }
        if let Some((id, d)) = &mut self.confirm_redownload {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.confirm_redownload = None,
                Outcome::Done(()) => {
                    let id = *id;
                    self.confirm_redownload = None;
                    self.redownload(id);
                }
            }
        }
        if let Some((q, d)) = &mut self.confirm_queue_delete {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.confirm_queue_delete = None,
                Outcome::Done(()) => {
                    let q = *q;
                    self.confirm_queue_delete = None;
                    self.delete_queue(q);
                }
            }
        }
        if let Some(d) = &mut self.add_url {
            let out = d.show(ctx);
            // Every address that goes on is remembered for the Address drop list.
            if let Outcome::Done(new) = &out {
                crate::settings::remember(&mut self.settings.url_history, new.url.as_str());
                self.dirty = true;
            }
            match out {
                Outcome::Open => {}
                Outcome::Closed => self.add_url = None,
                Outcome::Done(new) if new.url.scheme() == "magnet" => {
                    self.add_url = None;
                    self.open_torrent_dialog(Some(new.url.to_string()), ctx);
                }
                // A video's page, not a file (YouTube, Vimeo…): the video window looks it up.
                Outcome::Done(new) if mdm_tools::pages::is_video_page(&new.url) => {
                    self.add_url = None;
                    let dir = self.settings.category_dir(Category::Video, &self.downloads);
                    crate::videodlg::refresh_ytdlp(&self.rt);
                    self.video_dlg = Some(
                        crate::videodlg::VideoDialog::for_page(
                            dir,
                            new.url.as_str(),
                            Default::default(),
                        )
                        .with_subs(self.settings.video_subs.clone()),
                    );
                }
                // An image gallery's page (Wikimedia Commons, Flickr…): gallery-dl (#58).
                Outcome::Done(new) if mdm_tools::pages::is_gallery_page(&new.url) => {
                    self.add_url = None;
                    let dir = self
                        .settings
                        .category_dir(Category::General, &self.downloads);
                    self.gallery_dlg =
                        Some(crate::gallerydlg::GalleryDialog::new(dir, new.url.as_str()));
                }
                Outcome::Done(new) => {
                    self.add_url = None;
                    let url = new.url.clone();
                    if self.duplicate_gate(&url, || {
                        Pending::Url(dialogs::NewUrl {
                            url: new.url.clone(),
                            login: new.login.clone(),
                        })
                    }) {
                        self.open_file_info(new, Default::default(), ctx);
                    }
                }
            }
        }
        if let Some(d) = &mut self.confirm_delete {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.confirm_delete = None,
                Outcome::Done((files, dont_show)) => {
                    let ids = std::mem::take(&mut d.ids);
                    let remember = d.remember;
                    self.confirm_delete = None;
                    self.remove_downloads(&ids, files);
                    if remember {
                        self.settings.delete_files_too = files;
                    }
                    if dont_show {
                        self.settings.confirm_delete = false;
                    }
                    self.dirty = true;
                }
            }
        }
        if let Some(n) = &mut self.limit_notice
            && !matches!(n.show(ctx), Outcome::Open)
        {
            self.limit_notice = None;
        }
        if std::mem::take(&mut self.exit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(t) = &mut self.turn_off {
            match t.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.turn_off = None,
                Outcome::Done(()) => {
                    t.execute();
                    self.turn_off = None;
                }
            }
        }
        let mut changes = Vec::new();
        let items = &self.items;
        let icons = &mut self.icons;
        self.properties.retain_mut(|pr| {
            let Some(item) = items.iter().find(|i| i.id == pr.id) else {
                return false;
            };
            match pr.show(ctx, icons, item) {
                Outcome::Open => true,
                Outcome::Closed => false,
                Outcome::Done(c) => {
                    changes.push(c);
                    false
                }
            }
        });
        for c in changes {
            self.apply_properties(c);
        }
        let mut hashes = Vec::new();
        self.checksums.retain_mut(|c| match c.show(ctx) {
            Outcome::Open => true,
            Outcome::Closed => false,
            Outcome::Done(h) => {
                hashes.push((c.id, h));
                false
            }
        });
        for (id, h) in hashes {
            if let Some(item) = self.item(id) {
                item.note(crate::i18n::fmt(
                    // i18n: event log entry: the user gave the checksum the file must have
                    crate::i18n::t("Expected checksum set ({algorithm})"),
                    &[("algorithm", h.algo.name())],
                ));
                item.checksum = Some(h);
                self.dirty = true;
            }
        }
        if let Some(f) = &mut self.find {
            match f.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.find = None,
                Outcome::Done(()) => {
                    let q = f.query.clone();
                    self.find = None;
                    self.find_next(&q);
                    self.last_find = Some(q);
                }
            }
        }
        if let Some((title, text)) = &self.notice
            && !matches!(dialogs::message(ctx, "notice", title, text), Outcome::Open)
        {
            self.notice = None;
        }
        if !self.torrents_started {
            self.torrents_started = true;
            // The handlers follow the exe (a portable copy may have moved).
            if self.settings.torrent_associations {
                #[cfg(windows)]
                crate::win32::set_torrent_handlers(true);
                #[cfg(unix)]
                crate::unix::set_torrent_handlers(true);
            }
            if self.settings.torrent_keep_sharing {
                self.torrent_host.set_sharing(
                    &self.rt,
                    self.torrent_hashes(),
                    self.complete_torrents(),
                    true,
                );
            }
        }
        if let Some(t) = &mut self.torrent_settings {
            match t.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.torrent_settings = None,
                Outcome::Done(crate::torrentdlg::TorrentChoice {
                    share,
                    assoc,
                    net,
                    upload,
                    seeding,
                }) => {
                    self.torrent_settings = None;
                    // The network: the next torrent session takes it (torrent_tick).
                    net.apply(&mut self.settings);
                    // The upload limit: at once (torrent_tick sets it on the session).
                    (
                        self.settings.torrent_upload_on,
                        self.settings.torrent_upload_kb,
                    ) = upload;
                    // Seeding limits: the tick stops those over them; raised, the finished
                    // torrents now under them are shared again (when sharing stays on).
                    let s = &mut self.settings;
                    let before = s.sharing_limits();
                    s.torrent_stop_ratio_on = seeding.ratio_on;
                    s.torrent_stop_ratio = seeding.ratio;
                    s.torrent_stop_minutes_on = seeding.minutes_on;
                    s.torrent_stop_minutes = seeding.minutes;
                    if share
                        && share == self.settings.torrent_keep_sharing
                        && before != self.settings.sharing_limits()
                    {
                        self.torrent_host.set_sharing(
                            &self.rt,
                            self.torrent_hashes(),
                            self.complete_torrents(),
                            true,
                        );
                    }
                    if share != self.settings.torrent_keep_sharing {
                        self.settings.torrent_keep_sharing = share;
                        self.torrent_host.set_sharing(
                            &self.rt,
                            self.torrent_hashes(),
                            self.complete_torrents(),
                            share,
                        );
                    }
                    if assoc != self.settings.torrent_associations {
                        self.settings.torrent_associations = assoc;
                        #[cfg(windows)]
                        crate::win32::set_torrent_handlers(assoc);
                        #[cfg(unix)]
                        crate::unix::set_torrent_handlers(assoc);
                    }
                    self.dirty = true;
                }
            }
        }
        if let Some(t) = &mut self.torrent_dlg {
            match t.show(ctx, &self.rt) {
                Outcome::Open => {}
                Outcome::Closed => self.torrent_dlg = None,
                Outcome::Done(add) => {
                    self.torrent_dlg = None;
                    self.add_torrent(add);
                }
            }
        }
        if let Some(g) = &mut self.gallery_dlg {
            match g.show(ctx, &self.rt) {
                Outcome::Open => {}
                Outcome::Closed => self.gallery_dlg = None,
                Outcome::Done(add) => {
                    self.gallery_dlg = None;
                    self.add_gallery(add);
                }
            }
        }
        if let Some(v) = &mut self.video_dlg {
            match v.show(ctx, &self.rt) {
                Outcome::Open => {}
                Outcome::Closed => self.video_dlg = None,
                Outcome::Done(out) => {
                    self.video_dlg = None;
                    match out {
                        crate::videodlg::VideoOut::One(add) => self.add_video(add),
                        crate::videodlg::VideoOut::Many(add) => self.add_playlist(add),
                    }
                }
            }
        }
        if let Some(b) = &mut self.batch {
            match b.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.batch = None,
                Outcome::Done(added) => {
                    self.batch = None;
                    self.add_batch(added);
                }
            }
        }
        if let Some(d) = &mut self.options {
            match d.show(ctx, &self.downloads) {
                Outcome::Open => {}
                Outcome::Closed => {
                    // Cancel: categories the dialog added, changed or restored are undone.
                    self.settings.publish_categories();
                    self.options = None;
                }
                Outcome::Done(new) => {
                    let new = Settings::merge_changes(&self.settings, &d.from, &new);
                    self.options = None;
                    self.apply_options(new);
                }
            }
        }
        if let Some(mut s) = self.scheduler.take() {
            let mut actions = Vec::new();
            let open = s.show(
                ctx,
                &mut self.icons,
                &self.items,
                &self.queues,
                &mut actions,
            );
            for a in actions {
                self.scheduler_action(a);
            }
            if open {
                self.scheduler = Some(s);
            }
        }
        if let Some(d) = &mut self.speed_settings {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.speed_settings = None,
                Outcome::Done(c) => {
                    self.settings.speed_limit_kb = c.kb;
                    self.settings.limiter_on_startup = c.on_startup;
                    self.settings.global_limit_on = c.global_on;
                    self.settings.global_limit_kb = c.global_kb;
                    self.settings.global_only_active = c.only_active;
                    self.speed_settings = None;
                    self.apply_speed_limit();
                    self.dirty = true;
                }
            }
        }
        if let Some(mut fi) = self.file_info.take() {
            self.start_early(&mut fi);
            let out = fi.show(ctx, &mut self.icons);
            if std::mem::take(&mut fi.new_category) {
                self.category_for_file_info = true;
                self.run(Cmd::AddCategory, None, ctx);
            }
            // The folder a download goes to is remembered for the Save As drop list.
            if matches!(out, Outcome::Done(_))
                && let Some(dir) = fi.dest().parent()
            {
                crate::settings::remember(
                    &mut self.settings.folder_history,
                    &dir.display().to_string(),
                );
                self.dirty = true;
            }
            // The address was edited after the early start: that download is the old address's.
            // It goes; the new one starts early in its turn (start_early).
            if let Some(id) = fi.early
                && self.item(id).is_some_and(|i| i.url != fi.url)
            {
                self.stop(id);
                self.after_stop.push((id, None));
                fi.early = None;
            }
            match out {
                Outcome::Open => self.file_info = Some(fi),
                Outcome::Closed => {
                    if let Some(id) = fi.early {
                        self.stop(id);
                        self.after_stop.push((id, None));
                    }
                }
                Outcome::Done(start) => match fi.early {
                    Some(id) => self.adopt_early(&fi, id, start),
                    None => self.add(&mut fi, start),
                },
            }
        }
        // The next download that waited for Download File Info.
        if self.file_info.is_none()
            && let Some((new, context)) = self.file_info_waiting.pop_front()
        {
            self.open_file_info(new, context, ctx);
        }
        self.finish_stopped();
        let mut actions = Vec::new();
        // A progress window can be hidden in the tray only when there is one to bring it back.
        // Off-screen test copies have no tray icon: their windows are drawn as if they had one
        // (the captures and the accessibility checks see the hide button).
        let tray = (self.tray.is_some() || crate::offscreen()) && crate::tray::available();
        for p in &mut self.progress {
            p.keep_sharing = self.settings.torrent_keep_sharing;
            if p.host.is_none() {
                p.host = Some(self.torrent_host.clone());
            }
            p.tray = tray;
            p.minimize_to_tray = self.settings.minimize_to_tray;
            // Hidden: not shown (its window is gone) until the tray or the list brings it back.
            if p.hidden && tray {
                continue;
            }
            p.hidden = false;
            if let Some(item) = self.items.iter().find(|i| i.id == p.id) {
                actions.push((p.id, p.show(ctx, item, item.snap.as_ref())));
            }
        }
        for (id, a) in actions {
            match a {
                progress::Action::None => {}
                progress::Action::Pause => self.stop(id),
                progress::Action::Resume => self.start(id, true),
                progress::Action::Cancel => {
                    self.stop(id);
                    self.progress.retain(|p| p.id != id);
                }
                progress::Action::Close => self.progress.retain(|p| p.id != id),
                progress::Action::Hide => {
                    if let Some(p) = self.progress.iter_mut().find(|p| p.id == id) {
                        p.hidden = true;
                    }
                }
                progress::Action::Limit(limit, remember) => {
                    if let Some(item) = self.item(id) {
                        if let Some(l) = item.live.as_ref() {
                            l.dl.set_speed_limit(limit);
                        }
                        // Remembered, it applies again after a stop or a restart.
                        item.own_limit = if remember { limit } else { None };
                        self.dirty = true;
                    }
                }
                progress::Action::UploadLimit(limit) => {
                    if let Some(item) = self.item(id)
                        && let Some(spec) = item.torrent.as_mut()
                    {
                        // Kept with the torrent; a Run sets it now, a torrent sharing without
                        // one gets it at the next torrent_tick (torrent_accounts).
                        spec.upload_limit = limit;
                        if let Some(model::Job::Torrent(run)) = item.live.as_ref().map(|l| &l.dl) {
                            run.set_upload_limit(limit);
                        }
                        self.dirty = true;
                    }
                }
            }
        }
        let mut keep = Vec::new();
        for mut c in std::mem::take(&mut self.complete) {
            match c.show(ctx, &mut self.icons) {
                Outcome::Open => keep.push(c),
                Outcome::Done(()) | Outcome::Closed => {
                    if c.dont_show {
                        self.settings.show_complete = false;
                        self.dirty = true;
                    }
                }
            }
        }
        self.complete = keep;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // The freeze watchdog times every frame (crash.rs).
        crate::crash::frame_begin();
        // Off-screen test runs: no window of ours may become the foreground one (win32.rs). Linux
        // and macOS need no such pass: their off-screen windows open inactive (main.rs `quiet`)
        // and nothing raises them then (dialogs::raise).
        #[cfg(windows)]
        if crate::offscreen() {
            crate::win32::keep_from_foreground();
        }
        crate::settings::share_proxy(self.settings.proxy());
        crate::settings::share_dns(self.settings.dns());
        crate::crash::window("main");
        let ctx = ui.ctx().clone();
        self.network_adapter_tick(&ctx);
        dialogs::publish_main_rect(&ctx);
        while let Ok(inc) = self.commands.try_recv() {
            match inc.reply {
                Some(reply) => {
                    let _ = reply.send(self.answer(&inc.cmd));
                }
                None => self.handle_command(inc.cmd, &ctx),
            }
        }
        self.poll(&ctx);
        // What failed in any window, told here (dialogs::tell).
        if let Some((title, text)) = dialogs::told().pop() {
            self.notice = Some((title, text));
        }
        self.poll_picks(&ctx);
        self.drop_target(&ctx);
        self.main_drops(&ctx);
        self.remember_layout(&ctx);
        self.auto_update_check(&ctx);
        self.apply_global_limit();
        self.watch_clipboard(&ctx);
        self.tray(&ctx);
        // The taskbar button's green bar: everything running, as one.
        #[cfg(windows)]
        crate::win32::taskbar_progress(crate::MAIN_TITLE, self.overall_progress());
        // Light or dark: IDM follows Windows' app mode (checked every 2 s) unless View → Dark
        // Mode support is off. Switches live (IDM asks to restart).
        if self
            .theme_checked
            .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.theme_checked = Some(Instant::now());
            let changed = crate::theme::update(
                self.settings.dark_mode_support,
                self.settings.always_dark,
                &ctx,
            );
            // The caption follows (first check: the window may not exist yet; retried).
            if changed || !self.caption_set {
                crate::theme::apply_mode(&ctx);
            }
            // Re-asserted on every check (a no-op when already right).
            {
                #[cfg(windows)]
                {
                    self.caption_set =
                        crate::win32::caption_dark(crate::MAIN_TITLE, crate::theme::dark());
                }
                #[cfg(not(windows))]
                {
                    self.caption_set = true;
                }
            }
            if changed {
                ctx.request_repaint();
            }
            // For a crash report: what MDM has open and running (sent when it changes).
            crate::crash::state(
                self.items.len(),
                self.items.iter().filter(|i| i.live.is_some()).count(),
                self.queues.len(),
                ctx.pixels_per_point(),
            );
            self.torrent_tick();
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(chrome::WINDOW_BG()))
            .show(ui, |ui| self.paint(ui));
        self.dialogs(&ctx);
        crate::crash::frame_end();
    }
}

/// A download no Download File Info answered: its address, login and browser context.
type Unseen = (url::Url, Option<(String, String)>, ipc::Command);

impl App {
    /// Quitting: downloads the browser handed over that never got their Download File Info
    /// (waiting their turn, or arriving at the last moment) go to the main queue as Download
    /// Later, not lost (the browser gave its own copy up). Commands after this get "gone".
    fn keep_unseen(&mut self) {
        let mut unseen: Vec<Unseen> = self
            .file_info_waiting
            .drain(..)
            .map(|(new, context)| (new.url, new.login, context))
            .collect();
        // The one Download File Info shows, unanswered (an early download is in the list
        // already).
        if let Some(fi) = self.file_info.take()
            && fi.early.is_none()
        {
            unseen.insert(0, (fi.url.clone(), fi.login.clone(), fi.context.clone()));
        }
        while let Ok(inc) = self.commands.try_recv() {
            if inc.reply.is_none()
                && let Some(url) = inc
                    .cmd
                    .url
                    .as_deref()
                    .and_then(|u| url::Url::parse(u.trim()).ok())
                    .filter(mdm_engine::supported)
            {
                unseen.push((url, None, inc.cmd));
            }
        }
        for (url, login, context) in unseen {
            let guessed = context.name.is_none();
            let name = context
                .name
                .clone()
                .unwrap_or_else(|| mdm_engine::naming::file_name(None, &url));
            let name = mdm_engine::naming::sanitize(&name);
            let category = Category::of(&name);
            let dir = self.settings.category_dir(category, &self.downloads);
            let dest = mdm_engine::naming::destination(&dir, &name);
            let size = context.size;
            let id = self.push_item(
                url,
                dest,
                category,
                String::new(),
                size,
                login,
                context,
                crate::i18n::mark("from the browser"),
            );
            self.name_guessed(id, guessed);
            self.queues[0].items.push(id);
        }
        // The channel's receiver goes with it: later commands are answered "gone".
        self.commands = std::sync::mpsc::channel().1;
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.keep_unseen();
        // What the saver has in hand first, then the last state, written here and now.
        self.saver.flush();
        let _ = store::save(
            self.next_id,
            &self.items,
            &self.list.layout(),
            &self.settings,
            &self.queues,
        );
    }
}

/// The event log's line when a seeding limit stops a torrent's sharing.
fn sharing_stopped_note(why: crate::torrents::Reached) -> String {
    match why {
        crate::torrents::Reached::Ratio(r) => crate::i18n::fmt(
            // i18n: event log entry, past tense: a finished torrent stopped being shared to others because its share ratio (sent ÷ received) reached the limit set in Torrent settings; {ratio} = e.g. 2.00
            crate::i18n::t("Sharing stopped: the share ratio reached {ratio}"),
            &[("ratio", &format!("{r:.2}"))],
        ),
        crate::torrents::Reached::Time(s) => crate::i18n::fmt(
            // i18n: event log entry, past tense: a finished torrent stopped being shared to others after the time set in Torrent settings; {time} = e.g. 1 hour(s) 15 min
            crate::i18n::t("Sharing stopped after sharing for {time}"),
            &[("time", &model::time(s as f64))],
        ),
    }
}

/// What one side (torrents, or file downloads) may take of the global cap (bytes/s) while the
/// other uses `other`: half of it at least, and what the other leaves when that is more; never
/// under 1 KB/s (the limiter takes 0 as no limit).
///
/// An other side within 15% of its half counts as using its half: measured speeds wobble, and
/// taking every dip as room to grow kept both sides over their halves (measured: 463 + 398
/// KB/s under an 800 KB/s cap).
fn share(cap: u64, other: f64) -> u64 {
    let half = cap / 2;
    let other = other.max(0.0);
    let rest = if other >= half as f64 * 0.85 {
        half
    } else {
        cap.saturating_sub(other as u64)
    };
    rest.max(half).max(1024)
}

#[cfg(test)]
mod global_cap_tests {
    use super::share;

    #[test]
    fn each_side_gets_half_and_what_the_other_leaves() {
        let cap = 800 * 1024;
        assert_eq!(share(cap, 0.0), cap, "the other idle: all of it");
        assert_eq!(share(cap, 100.0 * 1024.0), 700 * 1024, "a slow other side");
        assert_eq!(share(cap, 300.0 * 1024.0), 500 * 1024);
        assert_eq!(share(cap, 400.0 * 1024.0), 400 * 1024);
        assert_eq!(
            share(cap, 350.0 * 1024.0),
            400 * 1024,
            "a wobble under its half"
        );
        assert_eq!(share(cap, 800.0 * 1024.0), 400 * 1024, "half at least");
        assert_eq!(
            share(cap, 5e9),
            400 * 1024,
            "the other measured over the cap"
        );
        assert_eq!(share(1024, 0.0), 1024);
        assert_eq!(share(1500, 1500.0), 1024, "at least 1 KB/s");
    }
}

#[cfg(test)]
mod referrer_tests {
    use super::sent_referrer;

    #[test]
    fn referrers_as_browsers_send_them() {
        let url = |u: &str| url::Url::parse(u).unwrap();
        let page = "https://shop.test/orders/123?tab=2#top";
        assert_eq!(
            sent_referrer(&url("https://shop.test/invoice.pdf"), page).as_deref(),
            Some("https://shop.test/orders/123?tab=2"),
            "same site: the whole address, no fragment"
        );
        assert_eq!(
            sent_referrer(&url("https://cdn.test/a.zip"), page).as_deref(),
            Some("https://shop.test/"),
            "another site: the origin only"
        );
        assert_eq!(
            sent_referrer(&url("http://plain.test/a.zip"), page),
            None,
            "https to http: none"
        );
        assert_eq!(
            sent_referrer(&url("https://x.test/a"), "javascript:void(0)"),
            None
        );
    }

    #[test]
    fn a_drive_players_button_downloads_the_file() {
        use super::{drive_file_behind_player, ipc};
        let page = "https://drive.google.com/file/d/1s52ek_4YTDRt_EOkx1FS53u-vJa0c4nu/view";
        let cmd = |video: &str, referer: Option<&str>| ipc::Command {
            video_page: Some(video.to_owned()),
            referer: referer.map(str::to_owned),
            ..Default::default()
        };
        assert_eq!(
            drive_file_behind_player(&cmd(page, None)).as_deref(),
            Some(page)
        );
        let stream = "https://abc.c.drive.google.com/videoplayback?id=1";
        assert_eq!(
            drive_file_behind_player(&cmd(stream, Some(page))).as_deref(),
            Some(page)
        );
        // Another site's video, even when reached from a Drive page, stays a video.
        assert_eq!(
            drive_file_behind_player(&cmd("https://www.youtube.com/watch?v=x", Some(page))),
            None
        );
        let folder = "https://drive.google.com/drive/folders/1s52ek_4YTDRt_EOkx1FS53u-vJa0c4nu";
        assert_eq!(drive_file_behind_player(&cmd(folder, None)), None);
    }
}

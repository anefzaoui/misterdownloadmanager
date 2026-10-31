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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
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
    Command(ipc::Command),
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
    /// Header right-click → Reset columns.
    ResetColumns,
    None,
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
    /// Right-click on a toolbar button.
    Toolbar,
    /// The Start Queue (true) / Stop Queue drop-down arrows.
    QueueDrop(bool),
    /// The Add Torrent button's drop-down arrow.
    TorrentDrop,
    /// Right-click on the drop target.
    DropTarget,
    /// Categories right-click: the node's view (None: the "Queues" root).
    Node(Option<View>),
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
    all_open: bool,
    unfinished_open: bool,
    finished_open: bool,
    queues_open: bool,
    add_url: Option<AddUrl>,
    file_info: Option<FileInfo>,
    /// Help menu windows.
    about: Option<crate::help::About>,
    updates: Option<crate::help::Updates>,
    shortcuts: Option<crate::help::Shortcuts>,
    tell_friend: Option<crate::help::TellFriend>,
    /// The language picker (first start, View → Language...).
    lang_picker: Option<crate::langpick::Picker>,
    /// The weekly update check, while it runs.
    auto_update: Option<crate::help::Slot>,
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
    /// Delete a category of the user's, asking first.
    confirm_category_delete: Option<(u32, dialogs::Confirm)>,
    /// File Properties windows, one per download.
    properties: Vec<crate::properties::Properties>,
    /// Verify checksum windows, one per download.
    checksums: Vec<crate::checksum::ChecksumDialog>,
    batch: Option<crate::batch::Batch>,
    video_dlg: Option<crate::videodlg::VideoDialog>,
    torrent_dlg: Option<crate::torrentdlg::TorrentDialog>,
    torrent_settings: Option<crate::torrentdlg::TorrentSettings>,
    /// Startup work for torrents done (associations refreshed, sharing resumed).
    torrents_started: bool,
    /// The torrent session, started on first use (D-014).
    torrent_host: crate::torrents::Host,
    find: Option<crate::find::Find>,
    /// The last search, for Find Next.
    last_find: Option<crate::find::Query>,
    /// A message with OK: its window title and message: "Cannot find …", an export that failed…
    notice: Option<(&'static str, String)>,
    /// Why the last save of the download list failed (told once per reason).
    save_failed: Option<String>,
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
    /// The next duplicate check is passed (the user chose to add it anyway).
    duplicate_cleared: bool,
    downloads: PathBuf,
    dirty: bool,
    last_save: Instant,
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
fn sort_key(it: &Item, col: usize) -> (f64, String) {
    let frac = it
        .size
        .filter(|s| *s > 0)
        .map_or(0.0, |s| it.downloaded as f64 / s as f64);
    match col {
        0 => (0.0, it.file_name.to_lowercase()),
        1 => (
            f64::from(u8::from(it.status == Status::Queued)),
            String::new(),
        ),
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
        initial: Option<ipc::Command>,
    ) -> Self {
        let (tx, commands) = std::sync::mpsc::channel();
        if let Some(cmd) = initial {
            let _ = tx.send(ipc::Incoming { cmd, reply: None });
        }
        let wake = cc.egui_ctx.clone();
        // Without the channel the app still works; it just can't receive commands.
        let _ = ipc::serve(tx, move || wake.request_repaint());
        crate::theme::apply(&cc.egui_ctx);
        let loaded = store::load();
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
        crate::model::set_custom(&settings.custom_categories);
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
        App {
            rt,
            items,
            next_id,
            selection: Default::default(),
            view: View::All,
            show_categories: true,
            all_open: true,
            unfinished_open: false,
            finished_open: false,
            queues_open: false,
            add_url: None,
            about: None,
            updates: None,
            shortcuts: None,
            tell_friend: None,
            lang_picker: ask_language.then(|| crate::langpick::Picker::new(true, None)),
            auto_update: None,
            file_info: None,
            progress: Vec::new(),
            complete: Vec::new(),
            settings,
            speed_settings: None,
            scheduler: None,
            options: None,
            category_dlg: None,
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
            torrent_dlg: None,
            torrent_settings: None,
            torrents_started: false,
            torrent_host: crate::torrents::Host::new(),
            find: None,
            last_find: None,
            notice: None,
            save_failed: None,
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
            exiting: false,
            confirm_delete: None,
            confirm_queue_delete: None,
            confirm_redownload: None,
            duplicate: None,
            duplicate_cleared: false,
            downloads,
            dirty: false,
            last_save: Instant::now(),
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

    /// Under an empty list: why (nothing in this view, or the column filters hide it all),
    /// with Clear filters when they do. True when Clear filters is clicked.
    fn empty_note(&self, ui: &Ui, outer: egui::Rect) -> bool {
        let p = ui.painter();
        let x = outer.left() + 12.0;
        let y = outer.top() + list::HEADER_H + 24.0;
        let filtered = self.list.filters.iter().any(Option::is_some) && !self.view_ids().is_empty();
        let note = if filtered {
            // i18n: in the empty download list, when the column filters hide every download
            crate::i18n::t("The column filters hide every download here.")
        } else {
            // i18n: in the empty download list (this category or queue has no downloads)
            crate::i18n::t("No downloads here.")
        };
        winui::text(p, x, y, note, winui::MUTED());
        filtered && winui::link(ui, p, x, y + 18.0, "Clear all filters", "empty-clear")
    }

    /// The strip under the list: how many downloads are shown and selected, how many run and
    /// how fast in all (beyond IDM, which has no status bar).
    fn status_strip(&self, ui: &Ui, o: Pos2, w: f32, h: f32, shown: usize) {
        let p = ui.painter();
        let top = o.y + h - STRIP_H;
        let baseline = top + 14.0;
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
        self.view_ids()
            .into_iter()
            .filter_map(|id| self.items.iter().find(|i| i.id == id))
            .map(|it| (it.id, self.row(it)))
            .filter(|(_, r)| list::matches(&self.list, r))
            .unzip()
    }

    /// One download's row in the list.
    fn row(&self, it: &Item) -> list::Row {
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
            queued: self.queues.iter().any(|q| q.items.contains(&it.id)),
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
            status_tip: it.log_lines(12),
        }
    }

    /// The view's downloads in the list's order, before the column filters.
    fn view_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .items
            .iter()
            .filter(|i| self.in_view(self.view, i))
            .map(|i| i.id)
            .collect();
        if let Some((col, asc)) = self.list.sort {
            let key = |id: &u64| {
                let it = self.items.iter().find(|i| i.id == *id);
                it.map_or((0.0, String::new()), |it| sort_key(it, col))
            };
            ids.sort_by(|a, b| {
                let (ka, kb) = (key(a), key(b));
                let ord = ka.0.total_cmp(&kb.0).then_with(|| ka.1.cmp(&kb.1));
                if asc { ord } else { ord.reverse() }
            });
        }
        ids
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

    fn add(&mut self, fi: &FileInfo, start: Start) {
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
        self.selection.set(id);
        if matches!(start, Start::Now) {
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
                    });
                }
            } else if !live {
                self.start(id, true);
            } else if !self.progress.iter().any(|p| p.id == id) {
                self.progress.push(Progress::new(id));
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

    /// `/show <window>` (mdm_ipc::Command::show): open one of the windows by name, as its menu
    /// command does. Unknown names do nothing.
    fn show_named(&mut self, name: &str, ctx: &egui::Context) {
        self.show_window(ctx);
        let first = self.items.first().map(|i| i.id);
        let last = self.items.last().map(|i| i.id);
        let (window, tab) = name.split_once(':').unwrap_or((name, ""));
        match window {
            "addurl" => self.run(Cmd::AddUrl, None, ctx),
            "batch" => self.run(Cmd::Batch(false), None, ctx),
            "find" => self.run(Cmd::Find, None, ctx),
            "scheduler" => self.run(Cmd::Scheduler, None, ctx),
            "about" => self.run(Cmd::About, None, ctx),
            "language" => self.run(Cmd::ChooseLanguage, None, ctx),
            "shortcuts" => self.run(Cmd::Shortcuts, None, ctx),
            "speedlimit" => self.run(Cmd::LimiterSettings, None, ctx),
            "properties" if first.is_some() => self.run(Cmd::Properties, first, ctx),
            "delete" if first.is_some() => self.run(Cmd::Remove, first, ctx),
            "progress" => {
                if let Some(id) = last
                    && !self.progress.iter().any(|p| p.id == id)
                {
                    self.progress.push(Progress::new(id));
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
        // The browser's download button over a video: the Download video window, looking the
        // page up at once.
        if let Some(page) = cmd.video_page.as_deref() {
            self.show_window(ctx);
            let dir = self.settings.category_dir(Category::Video, &self.downloads);
            crate::videodlg::refresh_ytdlp(&self.rt);
            self.video_dlg = Some(crate::videodlg::VideoDialog::for_page(dir, page));
            return;
        }
        // Its menu: never on this site.
        if let Some(host) = cmd.panel_hide_site.as_deref() {
            self.settings.video_panel_sites =
                mdm_ipc::capture::add_site(&self.settings.video_panel_sites, host);
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
        if !self.duplicate_gate(&url, || Pending::Command(cmd.clone())) {
            return;
        }
        // Options → Downloads → "Show start download dialog" off: start (or only queue) at once.
        let skip_dialog = !self.settings.show_start_dialog;
        if cmd.later || cmd.silent || skip_dialog {
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
            let dest = mdm_engine::naming::unique_path(&dir, &name);
            let later = cmd.later || (skip_dialog && self.settings.only_add_to_queue);
            let source = if cmd.referer.is_some() || cmd.cookies.is_some() {
                crate::i18n::mark("from the browser")
            } else {
                crate::i18n::mark("from the command line")
            };
            let id = self.push_item(url, dest, category, String::new(), None, None, cmd, source);
            if later {
                self.queues[0].items.push(id);
            } else {
                self.start(id, true);
            }
        } else {
            let mut fi = FileInfo::new(
                dialogs::NewUrl { url, login: None },
                self.downloads.clone(),
                self.settings.category_dirs.clone(),
                &self.rt,
                ctx,
            );
            fi.context = cmd;
            fi.remember = self.settings.change_folder_on_last;
            fi.folders = self.settings.folder_history.clone();
            self.file_info = Some(fi);
        }
    }

    /// Start or resume a download; `show` opens its progress dialog (IDM's default).
    fn start(&mut self, id: u64, show: bool) {
        let rt = self.rt.clone();
        let Some(item) = self.item(id) else { return };
        if item.live.is_some() || item.status == Status::Complete {
            return;
        }
        // A video from a page: yt-dlp, not the engine.
        if self.item(id).is_some_and(|i| i.video.is_some()) {
            self.start_video(id, show);
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
        let Some(url) = self.item(id).map(|i| i.url.clone()) else {
            return;
        };
        let ua = self.settings.user_agent.trim();
        let opts = Options {
            max_connections: self.settings.connections_for(&url),
            // The file's own remembered limit, else the Speed Limiter's.
            speed_limit: self
                .items
                .iter()
                .find(|i| i.id == id)
                .and_then(|i| i.own_limit)
                .or(self.settings.speed_limit()),
            proxy: self.settings.proxy(),
            global: Some(self.global.clone()),
            host_limits: Some(self.host_limits.clone()),
            ignore_mtime: self.settings.ignore_mtime,
            server_time: self.settings.set_creation_date,
            ftp_passive: self.settings.ftp_passive,
            checksum: self
                .items
                .iter()
                .find(|i| i.id == id)
                .and_then(|i| i.checksum.clone()),
            user_agent: if ua.is_empty() {
                Options::default().user_agent
            } else {
                ua.to_owned()
            },
            ..Options::default()
        };
        let site_login = self.settings.site_login(&url);
        let Some(item) = self.item(id) else { return };
        let _guard = rt.enter();
        // The download's own login, else the one Options → Sites Logins has for its address,
        // else an FTP address's own (its resume state keeps the address without the password).
        let url_login = mdm_engine::ftp::is_ftp(&item.url)
            .then(|| url_login(&item.url))
            .flatten();
        let mut headers = auth_headers(
            item.login
                .as_ref()
                .or(site_login.as_ref())
                .or(url_login.as_ref()),
        );
        let context = item.context.clone();
        for (name, v) in [("referer", &context.referer), ("cookie", &context.cookies)] {
            if let Some(v) = v.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
                headers.insert(name, v);
            }
        }
        let mut opts = opts;
        if let Some(ua) = context.user_agent.clone() {
            opts.user_agent = ua;
        }
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
            Download::start(
                Request {
                    url: item.url.clone(),
                    headers,
                    dest: Destination::File(item.dest.clone()),
                },
                opts,
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
            self.progress.push(Progress::new(id));
        }
    }

    /// Apply the global speed limiter setting to every running download.
    fn apply_speed_limit(&self) {
        let limit = self.settings.speed_limit();
        for i in &self.items {
            if let Some(l) = &i.live {
                l.dl.set_speed_limit(limit);
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
            } else if files && let Some(i) = self.items.iter().find(|i| i.id == id) {
                if i.status == Status::Complete {
                    let _ = std::fs::remove_file(&i.dest);
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
        let files = files || self.settings.delete_files_too;
        if self.settings.confirm_delete {
            let one = gone.len() == 1;
            self.confirm_delete = Some(dialogs::ConfirmDelete::new(
                gone.iter().map(|i| i.id).collect(),
                first.file_name.clone(),
                one && first.status != Status::Complete,
                files,
            ));
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
    fn apply_global_limit(&self) {
        let s = &self.settings;
        #[cfg(windows)]
        let active = crate::win32::idle_seconds() < 60;
        #[cfg(not(windows))]
        let active = true;
        let rate = (s.global_limit_on && (!s.global_only_active || active))
            .then(|| s.global_limit_kb.max(1) * 1024);
        if self.global.get() != rate {
            self.global.set(rate);
        }
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

    /// The window back from the tray (or from minimized), in front.
    fn show_window(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        if !crate::offscreen() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
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
        }
        let cmds = self.tray.as_ref().map(|t| t.poll()).unwrap_or_default();
        for c in cmds {
            use crate::tray::TrayCmd;
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
            };
            self.run(cmd, None, ctx);
        }
        // Closing hides the window in the tray; downloads and queues go on.
        if self.tray.is_some() && !self.exiting && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            // Wayland can't hide a window: it goes to the dock / task list instead.
            if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.hidden = true;
            }
        }
        // Hidden, the window gets no paint messages: keep the clock (queues, scheduler,
        // download limits) running.
        if self.hidden {
            ctx.request_repaint_after(Duration::from_secs(1));
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
        if let Some(dir) = c.move_to {
            let target = mdm_engine::naming::unique_path(&dir, &item.file_name);
            if item.status == Status::Complete {
                let moved = std::fs::rename(&item.dest, &target).is_ok()
                    || (std::fs::copy(&item.dest, &target).is_ok()
                        && std::fs::remove_file(&item.dest).is_ok());
                if moved {
                    item.dest = target;
                }
            } else if item.downloaded == 0 {
                item.dest = target;
            }
        }
        self.dirty = true;
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
        let dest = mdm_engine::naming::unique_path(&folder, &name);
        let id = self.push_item(
            add.page,
            dest,
            Category::Video,
            String::new(),
            add.size,
            None,
            Default::default(),
            crate::i18n::mark("from a page (yt-dlp)"),
        );
        if let Some(item) = self.item(id) {
            item.video = Some(model::VideoSpec {
                selector: add.selector,
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
            self.progress.push(Progress::new(id));
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
    fn complete_torrents(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.status == Status::Complete)
            .filter_map(|i| i.torrent.as_ref().map(|t| t.hash.clone()))
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
            self.progress.push(Progress::new(id));
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
            let dest = mdm_engine::naming::unique_path(&dir, &name);
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
    /// queued or failed, remove one (its file kept) or delete it with its file; one that runs
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
            let startable = matches!(
                item.status,
                Status::Queued | Status::Stopped | Status::Error
            );
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
        crate::model::set_custom(&new.custom_categories);
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
                // Running state belongs to the app, not the draft.
                let running: Vec<String> = self
                    .queues
                    .iter()
                    .filter(|q| q.running)
                    .map(|q| q.name.clone())
                    .collect();
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
    fn member(&self, id: u64, now: Instant) -> Member {
        let Some(i) = self.items.iter().find(|i| i.id == id) else {
            return Member::Complete;
        };
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
            let starts = queue::to_start(&self.queues[q], |id| self.member(id, now), now);
            for id in starts {
                self.start(id, false);
            }
            if queue::finished(&self.queues[q], |id| self.member(id, now)) {
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
            let _ = open::that_detached(&i.dest);
        } else if !self.progress.iter().any(|p| p.id == id) {
            self.progress.push(Progress::new(id));
        }
    }

    /// `target`: the right-clicked download, if any. Stop, Resume, Remove and the queue
    /// commands act on every selected download (or the right-clicked one outside the
    /// selection); the others on one download.
    fn run(&mut self, cmd: Cmd, target: Option<u64>, ctx: &egui::Context) {
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
                    if self.item(id).is_some_and(|i| {
                        i.live.is_none()
                            && matches!(i.status, Status::Queued | Status::Stopped | Status::Error)
                    }) {
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
                    self.video_dlg = Some(crate::videodlg::VideoDialog::new(dir));
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
                if let Some(f) = rfd::FileDialog::new()
                    .add_filter("Torrent files", &["torrent"])
                    .set_title(crate::i18n::t("Open torrent file"))
                    .pick_file()
                {
                    self.open_torrent_dialog(Some(f.display().to_string()), ctx);
                }
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
                }
            }
            Cmd::Scheduler => {
                if self.scheduler.is_none() {
                    self.scheduler = Some(crate::scheduler::Scheduler::new(
                        &self.queues,
                        &self.settings,
                    ));
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
            Cmd::AddCategory => {
                self.category_dlg = Some(crate::categories::CategoryDialog::new());
            }
            Cmd::CategoryProps(c) => {
                let folder = self.settings.category_dir(c, &self.downloads);
                self.category_dlg = Some(crate::categories::CategoryDialog::edit(c, &folder));
            }
            Cmd::DeleteCategory(Category::Custom(id)) => {
                let text = crate::i18n::fmt(
                    crate::i18n::t(
                        "Delete the category \"{category}\"? Its downloads become General; their files stay where they are.",
                    ),
                    &[("category", &Category::Custom(id).label())],
                );
                self.confirm_category_delete = Some((
                    id,
                    dialogs::Confirm::new(crate::i18n::mark("Delete category"), text),
                ));
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
            Cmd::SelectAll => {
                let ids = self.shown_ids();
                self.selection.apply(crate::selection::Pick::All, &ids);
            }
            Cmd::None => {}
        }
    }

    /// Pull engine progress into the rows; open "Download complete" when one finishes.
    fn poll(&mut self, ctx: &egui::Context) {
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
            let saved = store::save(
                self.next_id,
                &self.items,
                &self.list.layout(),
                &self.settings,
                &self.queues,
            );
            // A list that can't be saved says so, once for each reason (not every second);
            // it was silent and a restart lost the changes.
            match saved {
                Ok(()) => self.save_failed = None,
                Err(e) => {
                    let e = e.to_string();
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
            self.dirty = false;
            self.last_save = Instant::now();
        }
    }

    // ---- menus -----------------------------------------------------------------------------

    /// IDM 6.42's menus (notes/ui-spec/menus.md); unbuilt features are greyed out.
    fn menu_entries(&self, index: usize) -> Vec<MenuEntry<Cmd>> {
        use MenuEntry::{Item as I, Sep, Sub};
        let sel = self.selected_item();
        let picked = self.chosen(None);
        let running = picked.iter().any(|i| i.live.is_some());
        let resumable = picked.iter().any(|i| {
            i.live.is_none() && matches!(i.status, Status::Queued | Status::Stopped | Status::Error)
        });
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
                &[("queue", crate::i18n::t(&self.queues[q].name))],
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
        let any_resumable = chosen.iter().any(|i| {
            i.live.is_none() && matches!(i.status, Status::Queued | Status::Stopped | Status::Error)
        });
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
                    crate::chrome::label(&queue.name),
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

    /// Right-click on a toolbar button. Only the current look (large Neon buttons) is built.
    fn toolbar_entries() -> Vec<MenuEntry<Cmd>> {
        use MenuEntry::{Item as I, Radio, Sep};
        vec![
            I("Customize...", Cmd::None, false, false),
            Sep,
            Radio("Small Buttons", Cmd::None, false, false),
            Radio("Large Buttons", Cmd::None, true, true),
            Sep,
            I("Look for new toolbars", Cmd::None, false, false),
            Sep,
            // i18n: name of the toolbar's icon style (a proper name; may stay as is)
            Radio("Neon", Cmd::None, true, true),
            Radio("3D Style", Cmd::None, false, false),
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
                        Some(c) if c.is_custom() => {
                            I("Delete", Cmd::DeleteCategory(c), true, false)
                        }
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
        // Each node's downloads, as its list would show them (before column filters).
        for (row, view, _) in &mut rows {
            if let Some(v) = view {
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
                .map(|(l, ..)| {
                    l.chars()
                        .next()
                        .map(|c| c.to_lowercase().to_string())
                        .unwrap_or_default()
                })
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
            let clicked = ui.input(|x| x.pointer.any_released());
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
        let resumable = picked.iter().any(|i| {
            i.live.is_none() && matches!(i.status, Status::Queued | Status::Stopped | Status::Error)
        });
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
                self.queues.iter().any(|q| !q.running),
                Cmd::StartQueue(0),
            ),
            (
                Icon::StopQueue,
                "Stop Queue",
                self.queues.iter().any(|q| q.running),
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
            Some(ToolbarEvent::Context(at)) => self.open_popup(Popup::Toolbar, at),
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
            let sr = ui.interact(split, egui::Id::unique("cat-splitter"), egui::Sense::drag());
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
            if let Some(ev) =
                chrome::categories(ui, &mut self.icons, o, o.y + h - STRIP_H, right, &tree)
            {
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
        let r = list::show(
            ui,
            outer,
            &mut self.list,
            &rows,
            &selected,
            focus,
            &mut self.icons,
        );
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
        if ids.is_empty() && self.empty_note(ui, outer) {
            chosen = Some((Cmd::ClearFilters, None));
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
                            MenuEntry::Item(t, Cmd::ToggleColumn(i), i != 0, self.list.visible[i])
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
                Popup::Toolbar => (Self::toolbar_entries(), None),
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
                    i.modifiers.shift
                        && !i.modifiers.command
                        && i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
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

    /// Download File Info for an address from Add URL.
    fn open_file_info(&mut self, new: dialogs::NewUrl, ctx: &egui::Context) {
        let mut fi = FileInfo::new(
            new,
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
                if self.duplicate.is_none() {
                    self.duplicate = Some((dup, pending(), dialogs::Duplicate::new(name)));
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
                self.handle_command(cmd, ctx);
                self.duplicate_cleared = false;
            }
            Pending::Url(new) => self.open_file_info(new, ctx),
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
            });
        } else if i.live.is_none() {
            self.start(id, true);
        } else if !self.progress.iter().any(|p| p.id == id) {
            self.progress.push(Progress::new(id));
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
        let Some(path) = rfd::FileDialog::new()
            .set_title(crate::i18n::t("Export"))
            .add_filter(ext.to_ascii_uppercase(), &[ext])
            .set_file_name(name)
            .save_file()
        else {
            return;
        };
        let text = if ef2 {
            crate::transfer::export_ef2(&self.items)
        } else {
            crate::transfer::export_text(&self.items)
        };
        if let Err(e) = std::fs::write(&path, text) {
            self.notice = Some((
                crate::i18n::mark("Export"),
                crate::i18n::fmt(
                    crate::i18n::t("The list could not be saved: {error}"),
                    &[("error", &e.to_string())],
                ),
            ));
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
                .filter_map(|f| f.path.as_ref().map(|p| p.display().to_string()))
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
                })
                .collect();
            // i18n: in Import, where the addresses came from: dragged onto the window
            let source = crate::i18n::t("Dropped links").to_owned();
            self.import = Some(crate::transfer::ImportDialog::new(found, source));
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
        let mut dialog = rfd::FileDialog::new().set_title(crate::i18n::t("Import"));
        dialog = if ef2 {
            dialog.add_filter("EF2", &["ef2"])
        } else {
            dialog.add_filter("TXT", &["txt", "htm", "html", "csv"])
        };
        let Some(path) = dialog
            .add_filter(crate::i18n::t("All files"), &["*"])
            .pick_file()
        else {
            return;
        };
        self.import_file(&path);
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
        self.import = Some(crate::transfer::ImportDialog::new(found, file));
    }

    /// The checked addresses go to the list and the main queue, not started (IDM's import).
    fn add_imported(&mut self, found: Vec<crate::transfer::Imported>) {
        for d in found {
            let Ok(url) = url::Url::parse(&d.url) else {
                continue;
            };
            let name = mdm_engine::naming::sanitize(&mdm_engine::naming::file_name(None, &url));
            let category = Category::of(&name);
            let dir = self.settings.category_dir(category, &self.downloads);
            let dest = mdm_engine::naming::unique_path(&dir, &name);
            let context = ipc::Command {
                referer: d.referer,
                cookies: d.cookies,
                user_agent: d.user_agent,
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
            return;
        }
        let Some(target) = rfd::FileDialog::new()
            .set_title(crate::i18n::t("Move/Rename"))
            .set_directory(item.dest.parent().unwrap_or(std::path::Path::new(".")))
            .set_file_name(&item.file_name)
            .save_file()
        else {
            return;
        };
        let Some(item) = self.item(id) else { return };
        if target == item.dest {
            return;
        }
        if complete {
            let moved = std::fs::rename(&item.dest, &target).is_ok()
                || (std::fs::copy(&item.dest, &target).is_ok()
                    && std::fs::remove_file(&item.dest).is_ok());
            if !moved {
                item.note(crate::i18n::t("The file could not be moved"));
                return;
            }
        }
        if let Some(name) = target.file_name() {
            item.file_name = name.to_string_lossy().into_owned();
        }
        item.dest = target;
        self.dirty = true;
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
    fn weekly_update_check(&mut self, ctx: &egui::Context) {
        let now = chrono::Utc::now().timestamp();
        if self.auto_update.is_none()
            && self.settings.update_check
            && now - self.settings.update_checked >= crate::help::CHECK_EVERY
        {
            self.settings.update_checked = now;
            self.dirty = true;
            self.auto_update = Some(crate::help::check(&self.rt, ctx));
        }
        let done = self
            .auto_update
            .as_ref()
            .and_then(|s| s.lock().unwrap().take());
        if let Some(res) = done {
            self.auto_update = None;
            if let (Ok(crate::help::Found::Newer(rel)), None) = (res, &self.updates) {
                self.updates = Some(crate::help::Updates::found(rel));
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
                }
                Outcome::Closed => {
                    self.lang_picker = None;
                    // Cancelled on the first start: Windows' language, and don't ask again.
                    if first {
                        self.settings.language_asked = true;
                        self.dirty = true;
                    }
                }
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
                    crate::categories::apply(&mut self.settings, e, &self.downloads);
                    self.dirty = true;
                }
            }
        }
        if let Some((id, d)) = &mut self.confirm_category_delete {
            match d.show(ctx) {
                Outcome::Open => {}
                Outcome::Closed => self.confirm_category_delete = None,
                Outcome::Done(()) => {
                    let id = *id;
                    self.confirm_category_delete = None;
                    crate::categories::delete(&mut self.settings, id, &mut self.items);
                    let gone = Some(Category::Custom(id));
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
                    self.video_dlg = Some(crate::videodlg::VideoDialog::for_page(
                        dir,
                        new.url.as_str(),
                    ));
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
                        self.open_file_info(new, ctx);
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
                    self.confirm_delete = None;
                    self.remove_downloads(&ids, files);
                    self.settings.delete_files_too = files;
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
                Outcome::Done((share, assoc)) => {
                    self.torrent_settings = None;
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
        if let Some(v) = &mut self.video_dlg {
            match v.show(ctx, &self.rt) {
                Outcome::Open => {}
                Outcome::Closed => self.video_dlg = None,
                Outcome::Done(add) => {
                    self.video_dlg = None;
                    self.add_video(add);
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
                    // Cancel: categories the dialog added or changed are undone.
                    crate::model::set_custom(&self.settings.custom_categories);
                    self.options = None;
                }
                Outcome::Done(new) => {
                    self.options = None;
                    self.apply_options(new);
                }
            }
        }
        if let Some(mut s) = self.scheduler.take() {
            let running: Vec<String> = self
                .queues
                .iter()
                .filter(|q| q.running)
                .map(|q| q.name.clone())
                .collect();
            let mut actions = Vec::new();
            let open = s.show(ctx, &mut self.icons, &self.items, &running, &mut actions);
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
                    None => self.add(&fi, start),
                },
            }
        }
        self.finish_stopped();
        let mut actions = Vec::new();
        for p in &mut self.progress {
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
        let ctx = ui.ctx().clone();
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
        self.drop_target(&ctx);
        self.main_drops(&ctx);
        self.weekly_update_check(&ctx);
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
            let changed = crate::theme::update(self.settings.dark_mode_support, &ctx);
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
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(chrome::WINDOW_BG()))
            .show(ui, |ui| self.paint(ui));
        self.dialogs(&ctx);
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = store::save(
            self.next_id,
            &self.items,
            &self.list.layout(),
            &self.settings,
            &self.queues,
        );
    }
}

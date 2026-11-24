//! No notification-area icon (Android): the app's ongoing notification stands for it there. The
//! same names as `tray.rs`, so the app's code is the same on every system; no icon is ever made.

mod watch;

pub use watch::Icon;

/// What the tray asks the app to do (never sent here).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCmd {
    Show,
    Click(Option<(i32, i32)>),
    DoubleClick,
    AddUrl,
    Batch,
    BatchClipboard,
    LimiterOn,
    LimiterOff,
    Scheduler,
    StartQueue,
    StopQueue,
    Options,
    Exit,
    ShowDownload(u64),
}

/// No tray here.
pub fn available() -> bool {
    false
}

/// What closing the main window does (there is no window to close on Android).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnClose {
    #[allow(dead_code)]
    Hide,
    #[allow(dead_code)]
    Minimize,
    Quit,
}

/// With no tray, closing quits (`tray.rs` has the full rule).
pub fn on_close(
    _quitting: bool,
    _keep_running: bool,
    _tray: bool,
    _icon: Icon,
    _taskbar: bool,
    _dock: bool,
) -> OnClose {
    OnClose::Quit
}

/// A tray icon: none can be made, so no value of this type exists.
pub enum Tray {}

impl Tray {
    pub fn new(_ctx: &egui::Context, _limiter: bool) -> Option<Tray> {
        None
    }
    pub fn poll(&self) -> Vec<TrayCmd> {
        match *self {}
    }
    pub fn relabel(&self) {
        match *self {}
    }
    pub fn set_queue(&self, _running: bool) {
        match *self {}
    }
    pub fn set_tooltip(&self, _tip: &str) {
        match *self {}
    }
    pub fn set_downloads(&self, _running: &[(u64, String)]) {
        match *self {}
    }
    pub fn set_limiter(&self, _on: bool) {
        match *self {}
    }
}

/// Linux's icon on its way (`tray.rs`): never started here.
#[allow(dead_code)]
pub enum Setup {}

#[allow(dead_code)]
pub enum Came {
    Waiting,
    Up(Tray),
    Failed,
}

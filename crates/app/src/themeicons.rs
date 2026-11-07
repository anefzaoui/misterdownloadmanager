//! A theme's icon set (#45): files named by what MDM needs them for (`tb-add-url`, `cat-music`,
//! `ft-pdf`…, the list in [`NEEDS`]), at 16 pixels and at 24 (drawn at 24 and up). A theme that
//! has a set draws its icons where the classic look draws its own (the toolbar's outlines, the
//! categories' Silk icons, the file types' system icons); a need the set lacks keeps the classic
//! icon.
//!
//! Each file is an SVG drawing or a PNG picture, and says by its name how it is inked:
//! `<need>.png` and `<need>.svg` keep their colours, `<need>-symbolic.svg` is one colour (as the
//! freedesktop icon themes name theirs): its greys are drawn in the theme's text colour (greyed
//! when disabled), its colours kept, as GNOME and KDE recolour theirs. A set that is all one
//! colour says so once (`icon-mono = true`).
//!
//! Built-in sets are carried in the program (`assets/themes/<set>/icons/{16,24}/`). A theme file
//! in a folder of its own (`themes/<id>/theme.toml`) can bring one beside it
//! (`themes/<id>/icons/16/…`), read when the theme is chosen.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use egui::{Color32, ImageSource};

/// Every icon MDM can take from a theme, with what it stands for (the Themes help page lists
/// them).
pub const NEEDS: &[(&str, &str)] = &[
    ("tb-add-url", "Toolbar: Add URL"),
    ("tb-add-torrent", "Toolbar: Add torrent"),
    ("tb-resume", "Toolbar: Resume"),
    ("tb-stop", "Toolbar: Stop"),
    ("tb-stop-all", "Toolbar: Stop all"),
    ("tb-delete", "Toolbar: Delete"),
    ("tb-delete-completed", "Toolbar: Delete completed"),
    ("tb-options", "Toolbar: Options"),
    ("tb-scheduler", "Toolbar: Scheduler"),
    ("tb-start-queue", "Toolbar: Start queue"),
    ("tb-stop-queue", "Toolbar: Stop queue"),
    ("cat-all", "Categories: All downloads"),
    ("cat-compressed", "Categories: Compressed"),
    ("cat-documents", "Categories: Documents (and General)"),
    ("cat-music", "Categories: Music"),
    ("cat-programs", "Categories: Programs"),
    ("cat-video", "Categories: Video"),
    ("cat-torrents", "Categories: Torrents"),
    ("cat-unfinished", "Categories: Unfinished"),
    ("cat-finished", "Categories: Finished"),
    ("cat-queues", "Categories: Queues"),
    (
        "cat-folder",
        "A folder (a category of your own, a torrent's folder)",
    ),
    ("cat-folder-open", "An open folder"),
    (
        "misc-emblem-ok",
        "The check badge (Finished, a synchronized queue)",
    ),
    ("misc-up", "Scheduler: Move up"),
    ("misc-down", "Scheduler: Move down"),
    ("misc-remove", "Scheduler: Remove"),
    ("ft-generic", "A file of no known type"),
    ("ft-archive", "Archives (zip, 7z, rar…)"),
    ("ft-audio", "Sound files"),
    ("ft-video", "Video files"),
    ("ft-image", "Pictures and drawings"),
    ("ft-document", "Documents (doc, odt, epub…)"),
    ("ft-pdf", "PDF documents"),
    ("ft-spreadsheet", "Spreadsheets"),
    ("ft-presentation", "Presentations"),
    ("ft-text", "Plain text"),
    ("ft-code", "Web pages and scripts"),
    ("ft-executable", "Programs and installers"),
    ("ft-disk-image", "Disk images (iso, dmg…)"),
    ("ft-font", "Fonts"),
    ("ft-torrent", "Torrent files"),
    ("st-complete", "Status: complete"),
    ("st-downloading", "Status: downloading"),
    ("st-paused", "Status: paused"),
    ("st-queued", "Status: queued"),
    ("st-error", "Status: failed"),
    ("st-seeding", "Status: seeding"),
    ("ui-minimize", "Title bar: Minimize"),
    ("ui-maximize", "Title bar: Maximize"),
    ("ui-restore", "Title bar: Restore"),
    ("ui-close", "Title bar: Close"),
    ("ui-to-tray", "Title bar: Hide to the notification area"),
];

/// One icon file of a set: its bytes, whether it is a picture (PNG) rather than a drawing (SVG),
/// and whether it is drawn in one colour (white, tinted with the theme's text colour).
#[derive(Clone)]
struct File {
    bytes: Arc<[u8]>,
    png: bool,
    mono: bool,
}

/// One set: each need's icon at the sizes it has (16 pixels, and 24, drawn at 24 and up).
#[derive(Default)]
pub struct Set {
    files: HashMap<(String, u32), File>,
}

impl Set {
    #[cfg(test)]
    fn has(&self, need: &str) -> bool {
        self.files.contains_key(&(need.to_owned(), 16))
            || self.files.contains_key(&(need.to_owned(), 24))
    }

    /// The file nearest `px` (the screen's pixels): 16 up to 20 pixels, 24 above; the other size
    /// when one is missing. A 16-pixel picture is drawn at about its size only: larger, the 24's
    /// drawing is sharper where there is one.
    fn file(&self, need: &str, px: f32) -> Option<(u32, File)> {
        let get = |s: u32| self.files.get(&(need.to_owned(), s)).cloned();
        get(16)
            .filter(|f| px <= 20.0 && !(f.png && px > 17.0))
            .map(|f| (16, f))
            .or_else(|| get(24).map(|f| (24, f)))
            .or_else(|| get(16).map(|f| (16, f)))
    }

    /// A set from its files: each `(size, file name, bytes)`, the file named after its need
    /// (`<need>.svg`, `<need>.png`, or `<need>-symbolic.svg`). An SVG is one colour when the set
    /// says so (`mono`) or its name ends in `-symbolic` (the freedesktop icon themes' one-colour
    /// icons, GNOME's and KDE's): its greys take the theme's ink when drawn. A PNG keeps its
    /// colours.
    /// Files of no known need are left out.
    fn from_files<'a>(
        files: impl IntoIterator<Item = (u32, &'a str, Arc<[u8]>)>,
        mono: bool,
    ) -> Self {
        let mut set = Set::default();
        for (px, name, bytes) in files {
            let Some((stem, ext)) = name.rsplit_once('.') else {
                continue;
            };
            let (need, symbolic) = match stem.strip_suffix("-symbolic") {
                Some(n) => (n, true),
                None => (stem, false),
            };
            if !NEEDS.iter().any(|(n, _)| *n == need) {
                continue;
            }
            let file = match ext.to_ascii_lowercase().as_str() {
                "png" if !symbolic => File {
                    bytes,
                    png: true,
                    mono: false,
                },
                "svg" if symbolic => File {
                    bytes,
                    png: false,
                    mono: true,
                },
                "svg" => File {
                    bytes,
                    png: false,
                    mono,
                },
                _ => continue,
            };
            // One file per need and size: a one-colour icon before a picture, as a desktop's
            // icon theme prefers its symbolic icons.
            let key = (need.to_owned(), px);
            if set.files.get(&key).is_some_and(|f| f.mono && !file.mono) {
                continue;
            }
            set.files.insert(key, file);
        }
        set
    }
}

/// A one-colour SVG in `ink` (`#rrggbb`), as GNOME and KDE recolour theirs: each grey it names
/// (`fill`, `stroke`, `color`, in attributes or styles: the icon's ink, whatever shade its
/// author drew it in) becomes the ink, and so does the drawing's default. Its colours (a
/// warning's orange, KDE's highlight blue) stay.
fn inked(svg: &[u8], ink: &str) -> Arc<[u8]> {
    let Ok(text) = std::str::from_utf8(svg) else {
        return Arc::from(svg);
    };
    const KEYS: [&str; 7] = [
        "fill=\"",
        "stroke=\"",
        "fill:",
        "stroke:",
        "stop-color=\"",
        "color=\"",
        "color:",
    ];
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    'scan: while let Some(c) = rest.chars().next() {
        for key in KEYS {
            if let Some(after) = rest.strip_prefix(key)
                && let Some((len, grey)) = colour(after)
            {
                out.push_str(key);
                out.push_str(if grey { ink } else { &after[..len] });
                rest = &after[len..];
                continue 'scan;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    // A shape that names no fill is drawn black: the ink is the drawing's default instead.
    if let Some(at) = out.find("<svg")
        && let Some(end) = out[at..].find('>')
        && !out[at..at + end].contains(" fill=")
    {
        out.insert_str(at + 4, &format!(" fill=\"{ink}\""));
    }
    Arc::from(out.into_bytes())
}

/// The colour `s` starts with (`#rgb`, `#rrggbb`, `#rrggbbaa` or a few named inks): its length,
/// and whether it is a grey (its channels within 24 of one another). None for anything else
/// (`none`, `url(#…)`, `currentColor`).
fn colour(s: &str) -> Option<(usize, bool)> {
    if let Some(hex) = s.strip_prefix('#') {
        let n = hex.bytes().take_while(u8::is_ascii_hexdigit).count();
        let ch = |i: usize, w: usize| u8::from_str_radix(&hex[i * w..i * w + w], 16).ok();
        let rgb = match n {
            3 => [ch(0, 1)? * 17, ch(1, 1)? * 17, ch(2, 1)? * 17],
            6 | 8 => [ch(0, 2)?, ch(1, 2)?, ch(2, 2)?],
            _ => return None,
        };
        let spread = rgb.iter().max()? - rgb.iter().min()?;
        return Some((n + 1, spread <= 24));
    }
    ["black", "white", "gray", "grey"]
        .iter()
        .find(|w| s.starts_with(**w))
        .map(|w| (w.len(), true))
}

/// A built-in set's files, by size: their names as in its folder
/// (`assets/themes/<set>/icons/<size>/<name>`).
macro_rules! builtin_files {
    ($dir:literal; $($px:literal: [$($f:literal),* $(,)?]);* $(;)?) => {{
        let mut files: Vec<(u32, &'static str, Arc<[u8]>)> = Vec::new();
        $($(files.push((
            $px,
            $f,
            Arc::from(&include_bytes!(concat!(
                "../assets/themes/", $dir, "/icons/", stringify!($px), "/", $f
            ))[..]),
        ));)*)*
        files
    }};
}

/// The sets loaded so far, by name: the built-in ones, and the theme folders' as their themes
/// are chosen.
static SETS: LazyLock<Mutex<HashMap<String, Arc<Set>>>> = LazyLock::new(|| {
    let mut sets = HashMap::new();
    // Microsoft's Fluent UI System Icons (MIT, assets/themes/metro/icons/LICENSE), recoloured
    // white to be tinted; the magnet and the torrent file are MDM's own at their weight.
    let metro = builtin_files!("metro";
        16: ["cat-all.svg", "cat-compressed.svg", "cat-documents.svg", "cat-finished.svg",
            "cat-folder-open.svg", "cat-folder.svg", "cat-music.svg", "cat-programs.svg",
            "cat-queues.svg", "cat-torrents.svg", "cat-unfinished.svg", "cat-video.svg",
            "ft-archive.svg", "ft-audio.svg", "ft-code.svg", "ft-disk-image.svg",
            "ft-document.svg", "ft-executable.svg", "ft-font.svg", "ft-generic.svg",
            "ft-image.svg", "ft-pdf.svg", "ft-presentation.svg", "ft-spreadsheet.svg",
            "ft-text.svg", "ft-torrent.svg", "ft-video.svg", "misc-down.svg",
            "misc-emblem-ok.svg", "misc-remove.svg", "misc-up.svg", "st-complete.svg",
            "st-downloading.svg", "st-error.svg", "st-paused.svg", "st-queued.svg",
            "st-seeding.svg", "tb-add-torrent.svg", "tb-add-url.svg", "tb-delete-completed.svg",
            "tb-delete.svg", "tb-options.svg", "tb-resume.svg", "tb-scheduler.svg",
            "tb-start-queue.svg", "tb-stop-all.svg", "tb-stop-queue.svg", "tb-stop.svg",
            "ui-close.svg", "ui-maximize.svg", "ui-minimize.svg", "ui-restore.svg",
            "ui-to-tray.svg"];
        24: ["cat-all.svg", "cat-compressed.svg", "cat-documents.svg", "cat-finished.svg",
            "cat-folder-open.svg", "cat-folder.svg", "cat-music.svg", "cat-programs.svg",
            "cat-queues.svg", "cat-torrents.svg", "cat-unfinished.svg", "cat-video.svg",
            "ft-archive.svg", "ft-audio.svg", "ft-code.svg", "ft-document.svg",
            "ft-executable.svg", "ft-font.svg", "ft-generic.svg", "ft-image.svg", "ft-pdf.svg",
            "ft-presentation.svg", "ft-spreadsheet.svg", "ft-text.svg", "ft-torrent.svg",
            "ft-video.svg", "misc-down.svg", "misc-emblem-ok.svg", "misc-remove.svg",
            "misc-up.svg", "st-complete.svg", "st-downloading.svg", "st-error.svg",
            "st-paused.svg", "st-queued.svg", "st-seeding.svg", "tb-add-torrent.svg",
            "tb-add-url.svg", "tb-delete-completed.svg", "tb-delete.svg", "tb-options.svg",
            "tb-resume.svg", "tb-scheduler.svg", "tb-stop-all.svg", "tb-stop-queue.svg",
            "tb-stop.svg", "ui-close.svg", "ui-maximize.svg", "ui-minimize.svg",
            "ui-restore.svg", "ui-to-tray.svg"]);
    sets.insert("metro".to_owned(), Arc::new(Set::from_files(metro, true)));
    // GNOME's adwaita-icon-theme 51.0 (LGPL-3, assets/themes/adwaita/icons/LICENSE), its files
    // as GNOME ships them: colour pictures at 16 pixels and drawings above, one-colour symbolic
    // icons tinted. The PDF and torrent-file icons are MDM's own, in its style.
    let adwaita = builtin_files!("adwaita";
        16: ["cat-all.png", "cat-compressed.png", "cat-documents.png", "cat-finished.png",
            "cat-folder-open-symbolic.svg", "cat-folder.png", "cat-music.png", "cat-programs.png",
            "cat-queues-symbolic.svg", "cat-torrents.png", "cat-unfinished.png", "cat-video.png",
            "ft-archive.png", "ft-audio.png", "ft-code.png", "ft-disk-image-symbolic.svg",
            "ft-document.png", "ft-executable.png", "ft-font.png", "ft-generic.png",
            "ft-image.png", "ft-pdf.svg", "ft-presentation.png", "ft-spreadsheet.png",
            "ft-text.png", "ft-torrent.svg", "ft-video.png", "misc-down-symbolic.svg",
            "misc-emblem-ok-symbolic.svg", "misc-remove-symbolic.svg", "misc-up-symbolic.svg",
            "st-complete-symbolic.svg", "st-downloading-symbolic.svg", "st-error-symbolic.svg",
            "st-paused-symbolic.svg", "st-queued-symbolic.svg", "st-seeding-symbolic.svg",
            "tb-add-torrent-symbolic.svg", "tb-add-url-symbolic.svg",
            "tb-delete-completed-symbolic.svg", "tb-delete-symbolic.svg",
            "tb-options-symbolic.svg", "tb-resume-symbolic.svg", "tb-scheduler-symbolic.svg",
            "tb-start-queue-symbolic.svg", "tb-stop-all-symbolic.svg",
            "tb-stop-queue-symbolic.svg", "tb-stop-symbolic.svg", "ui-close-symbolic.svg",
            "ui-maximize-symbolic.svg", "ui-minimize-symbolic.svg", "ui-restore-symbolic.svg",
            "ui-to-tray-symbolic.svg"];
        24: ["cat-all.svg", "cat-compressed.svg", "cat-documents.svg", "cat-finished.svg",
            "cat-folder-open-symbolic.svg", "cat-folder.svg", "cat-music.svg", "cat-programs.svg",
            "cat-queues-symbolic.svg", "cat-torrents.svg", "cat-unfinished.svg", "cat-video.svg",
            "ft-archive.svg", "ft-audio.svg", "ft-code.svg", "ft-disk-image.svg",
            "ft-document.svg", "ft-executable.svg", "ft-font.svg", "ft-generic.svg",
            "ft-image.svg", "ft-pdf.svg", "ft-presentation.svg", "ft-spreadsheet.svg",
            "ft-text.svg", "ft-torrent.svg", "ft-video.svg", "misc-down-symbolic.svg",
            "misc-emblem-ok-symbolic.svg", "misc-remove-symbolic.svg", "misc-up-symbolic.svg",
            "st-complete-symbolic.svg", "st-downloading-symbolic.svg", "st-error-symbolic.svg",
            "st-paused-symbolic.svg", "st-queued-symbolic.svg", "st-seeding-symbolic.svg",
            "tb-add-torrent-symbolic.svg", "tb-add-url-symbolic.svg",
            "tb-delete-completed-symbolic.svg", "tb-delete-symbolic.svg",
            "tb-options-symbolic.svg", "tb-resume-symbolic.svg", "tb-scheduler-symbolic.svg",
            "tb-start-queue-symbolic.svg", "tb-stop-all-symbolic.svg",
            "tb-stop-queue-symbolic.svg", "tb-stop-symbolic.svg", "ui-close-symbolic.svg",
            "ui-maximize-symbolic.svg", "ui-minimize-symbolic.svg", "ui-restore-symbolic.svg",
            "ui-to-tray-symbolic.svg"]);
    sets.insert(
        "adwaita".to_owned(),
        Arc::new(Set::from_files(adwaita, false)),
    );
    // KDE's breeze-icons 6.31 (LGPL-3.0-or-later, assets/themes/breeze/icons/LICENSE), its files
    // as KDE ships them: colour drawings for files and categories, one-colour icons (its
    // ColorScheme-Text ones) in the theme's ink, their highlight and warning colours kept. The
    // PDF icon is MDM's own, in its style.
    let breeze = builtin_files!("breeze";
        16: ["cat-all-symbolic.svg", "cat-compressed.svg", "cat-documents.svg",
            "cat-finished-symbolic.svg", "cat-folder-open-symbolic.svg",
            "cat-folder-symbolic.svg", "cat-music.svg", "cat-programs-symbolic.svg",
            "cat-queues-symbolic.svg", "cat-torrents.svg", "cat-unfinished-symbolic.svg",
            "cat-video.svg", "ft-archive.svg", "ft-audio.svg", "ft-code.svg", "ft-disk-image-symbolic.svg",
            "ft-document.svg", "ft-executable-symbolic.svg", "ft-font.svg", "ft-generic.svg",
            "ft-image.svg", "ft-pdf.svg", "ft-presentation.svg", "ft-spreadsheet.svg",
            "ft-text.svg", "ft-torrent.svg", "ft-video.svg", "misc-down-symbolic.svg",
            "misc-emblem-ok-symbolic.svg", "misc-remove.svg", "misc-up-symbolic.svg",
            "st-complete-symbolic.svg", "st-downloading-symbolic.svg", "st-error-symbolic.svg",
            "st-paused-symbolic.svg", "st-queued-symbolic.svg", "st-seeding-symbolic.svg",
            "tb-add-url-symbolic.svg", "tb-delete-completed-symbolic.svg", "tb-delete.svg",
            "tb-options-symbolic.svg", "tb-resume-symbolic.svg", "tb-scheduler-symbolic.svg",
            "tb-stop-symbolic.svg", "ui-close-symbolic.svg", "ui-maximize-symbolic.svg",
            "ui-minimize-symbolic.svg", "ui-restore-symbolic.svg", "ui-to-tray-symbolic.svg"];
        24: ["cat-all-symbolic.svg", "cat-compressed.svg", "cat-documents.svg",
            "cat-finished-symbolic.svg", "cat-folder-open-symbolic.svg",
            "cat-folder-symbolic.svg", "cat-music.svg", "cat-programs.svg",
            "cat-queues-symbolic.svg", "cat-torrents.svg", "cat-unfinished-symbolic.svg",
            "cat-video.svg", "ft-archive.svg", "ft-audio.svg", "ft-code.svg", "ft-disk-image.svg",
            "ft-document.svg", "ft-executable.svg", "ft-font.svg", "ft-generic.svg",
            "ft-image.svg", "ft-pdf.svg", "ft-presentation.svg", "ft-spreadsheet.svg",
            "ft-text.svg", "ft-torrent.svg", "ft-video.svg", "misc-down-symbolic.svg",
            "misc-emblem-ok-symbolic.svg", "misc-remove.svg", "misc-up-symbolic.svg",
            "st-complete-symbolic.svg", "st-downloading-symbolic.svg", "st-error-symbolic.svg",
            "st-paused-symbolic.svg", "st-queued-symbolic.svg", "st-seeding-symbolic.svg",
            "tb-add-torrent-symbolic.svg", "tb-add-url-symbolic.svg",
            "tb-delete-completed-symbolic.svg", "tb-delete.svg", "tb-options-symbolic.svg",
            "tb-resume-symbolic.svg", "tb-scheduler-symbolic.svg", "tb-start-queue-symbolic.svg",
            "tb-stop-all-symbolic.svg", "tb-stop-queue-symbolic.svg", "tb-stop-symbolic.svg",
            "ui-close-symbolic.svg", "ui-maximize-symbolic.svg", "ui-minimize-symbolic.svg",
            "ui-restore-symbolic.svg", "ui-to-tray-symbolic.svg"]);
    sets.insert(
        "breeze".to_owned(),
        Arc::new(Set::from_files(breeze, false)),
    );
    Mutex::new(sets)
});

/// Reads the set in `dir` (`16/<need>.svg`, `.png` or `-symbolic.svg`, the same in `24`) under
/// `name`, for a theme file's own folder. Only the known needs are read; other files are left
/// alone. False when the folder holds none.
pub fn load_folder(name: &str, dir: &Path, mono: bool) -> bool {
    let mut files = Vec::new();
    for px in [16u32, 24] {
        for (need, _) in NEEDS {
            for f in [
                format!("{need}-symbolic.svg"),
                format!("{need}.svg"),
                format!("{need}.png"),
            ] {
                let path = dir.join(px.to_string()).join(&f);
                // An icon file is small: anything over 256 KB isn't one.
                if let Ok(meta) = std::fs::metadata(&path)
                    && meta.len() <= 256 * 1024
                    && let Ok(bytes) = std::fs::read(&path)
                {
                    files.push((px, f, Arc::<[u8]>::from(bytes)));
                }
            }
        }
    }
    if files.is_empty() {
        return false;
    }
    let set = Set::from_files(
        files.iter().map(|(px, f, b)| (*px, f.as_str(), b.clone())),
        mono,
    );
    if let Ok(mut sets) = SETS.lock() {
        sets.insert(name.to_owned(), Arc::new(set));
    }
    true
}

fn set(name: &str) -> Option<Arc<Set>> {
    SETS.lock().ok()?.get(name).cloned()
}

/// The ink a one-colour icon is drawn in: the text colour, greyed when disabled.
pub fn ink(enabled: bool) -> Color32 {
    let c = crate::theme::colors();
    if enabled { c.text } else { c.icon_disabled }
}

/// The active theme's icon for `need` at about `px` pixels on screen, a one-colour icon in
/// `ink`, and the tint to draw it with (the ink's opacity; a coloured icon its own colours,
/// faded when not `enabled`). None when the theme has no set, or its set lacks this need. Made
/// once per set, need, size and ink, then reused: drawing it each frame allocates nothing.
pub fn source(
    need: &str,
    px: f32,
    ink: Color32,
    enabled: bool,
) -> Option<(ImageSource<'static>, Color32)> {
    use std::cell::RefCell;
    type Key = (Cow<'static, str>, String, u32, [u8; 3]);
    thread_local! {
        static MADE: RefCell<HashMap<Key, Option<(ImageSource<'static>, bool)>>> = RefCell::default();
    }
    let name = crate::theme::current().icon_set.clone()?;
    let [r, g, b, a] = ink.to_srgba_unmultiplied();
    let (src, mono) = MADE.with(|made| {
        made.borrow_mut()
            .entry((name.clone(), need.to_owned(), px.round() as u32, [r, g, b]))
            .or_insert_with(|| {
                let set = set(&name)?;
                let (size, file) = set.file(need, px)?;
                let ext = if file.png { "png" } else { "svg" };
                let (ink_name, bytes) = if file.mono {
                    let hex = format!("#{r:02x}{g:02x}{b:02x}");
                    let bytes = inked(&file.bytes, &hex);
                    (hex[1..].to_owned(), bytes)
                } else {
                    ("color".to_owned(), file.bytes)
                };
                // Kept for the program's life, as its texture is.
                let uri: &'static str = Box::leak(
                    format!("bytes://mdm/icon-set/{name}/{size}/{ink_name}/{need}.{ext}")
                        .into_boxed_str(),
                );
                Some((
                    ImageSource::Bytes {
                        uri: uri.into(),
                        bytes: egui::load::Bytes::Shared(bytes),
                    },
                    file.mono,
                ))
            })
            .clone()
    })?;
    let tint = match (mono, enabled) {
        (true, _) => Color32::WHITE.gamma_multiply(f32::from(a) / 255.0),
        (false, true) => Color32::WHITE,
        (false, false) => Color32::WHITE.gamma_multiply(0.45),
    };
    Some((src, tint))
}

/// Paints the active theme's icon for `need` into `rc` (enabled or greyed); false when the theme
/// has none for it (the caller draws the classic icon).
pub fn paint(
    ui: &egui::Ui,
    painter: &egui::Painter,
    need: &str,
    rc: egui::Rect,
    enabled: bool,
) -> bool {
    let px = rc.width() * ui.ctx().pixels_per_point();
    let Some((src, tint)) = source(need, px, ink(enabled), enabled) else {
        return false;
    };
    let img = egui::Image::new(src);
    if let Ok(egui::load::TexturePoll::Ready { texture }) = img.load_for_size(ui.ctx(), rc.size()) {
        painter.image(
            texture.id,
            rc,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    }
    true
}

/// A file type's need, from the classic fallback icon's kind (fileicons.rs).
pub fn file_need(tango: &str, ext: Option<&str>) -> &'static str {
    match ext {
        Some("torrent") => return "ft-torrent",
        Some("pdf") => return "ft-pdf",
        _ => {}
    }
    match tango {
        "folder" => "cat-folder",
        "folder-open" => "cat-folder-open",
        "video-x-generic" => "ft-video",
        "audio-x-generic" => "ft-audio",
        "image-x-generic" | "x-office-drawing" | "x-office-drawing-template" => "ft-image",
        "package-x-generic" => "ft-archive",
        "application-x-executable" => "ft-executable",
        "media-optical" => "ft-disk-image",
        "x-office-document" | "x-office-document-template" => "ft-document",
        "x-office-spreadsheet" | "x-office-spreadsheet-template" => "ft-spreadsheet",
        "x-office-presentation" | "x-office-presentation-template" => "ft-presentation",
        "text-html" | "text-x-script" => "ft-code",
        "font-x-generic" => "ft-font",
        "text-x-generic" => "ft-text",
        _ => "ft-generic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_need_has_one_id_and_metro_has_them_all() {
        let mut ids: Vec<&str> = NEEDS.iter().map(|(n, _)| *n).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n);
        let metro = set("metro").unwrap();
        for (need, _) in NEEDS {
            assert!(metro.has(need), "metro lacks {need}");
            let (_, f) = metro.file(need, 16.0).unwrap();
            let text = std::str::from_utf8(&f.bytes).unwrap();
            // Drawn white, to be tinted: no other colour left in it.
            assert!(f.mono && !f.png, "{need}");
            assert!(!text.contains("#212121"), "{need} keeps Fluent's ink");
        }
    }

    /// Adwaita has every need at both sizes: colour pictures at 16 pixels and drawings above,
    /// its one-colour (symbolic) icons made white to be tinted, the others keeping their colours.
    #[test]
    fn adwaita_mixes_pictures_and_symbolic_icons() {
        let adw = set("adwaita").unwrap();
        for (need, _) in NEEDS {
            for px in [16u32, 24] {
                let f = adw.files.get(&((*need).to_owned(), px));
                assert!(f.is_some(), "adwaita lacks {need} at {px}");
                let f = f.unwrap();
                if f.png {
                    assert!(!f.mono && f.bytes.starts_with(b"\x89PNG"), "{need} {px}");
                    continue;
                }
                let text = std::str::from_utf8(&f.bytes).unwrap();
                assert!(text.contains("<svg"), "{need} {px}");
                if f.mono {
                    let inked = inked(&f.bytes, "#123456");
                    let text = std::str::from_utf8(&inked).unwrap();
                    assert!(!text.contains("#2e3436"), "{need} {px} keeps GNOME's ink");
                    assert!(text.contains("#123456"), "{need} {px}");
                }
            }
        }
        let (_, f) = adw.file("tb-resume", 16.0).unwrap();
        assert!(f.mono);
        let (_, f) = adw.file("cat-music", 16.0).unwrap();
        assert!(f.png && !f.mono);
        // A 16-pixel picture at 16 pixels; at 32 (twice the size), the drawing.
        let (_, f) = adw.file("cat-music", 32.0).unwrap();
        assert!(!f.png && !f.mono);
        // The scale between: at 18 pixels the drawing too, sharper than the picture stretched.
        assert!(!adw.file("ft-audio", 18.0).unwrap().1.png);
        // Our own PDF and torrent file keep their colours.
        assert!(!adw.file("ft-pdf", 16.0).unwrap().1.mono);
        assert!(!adw.file("ft-torrent", 32.0).unwrap().1.mono);
    }

    /// Breeze has every need: KDE's one-colour icons (ColorScheme-Text) take the ink and keep
    /// their highlight and warning colours; its colour icons and our PDF keep theirs.
    #[test]
    fn breeze_has_every_need_and_keeps_its_colours() {
        let set = set("breeze").unwrap();
        for (need, _) in NEEDS {
            assert!(set.has(need), "breeze lacks {need}");
            for px in [16.0, 32.0] {
                let (_, f) = set.file(need, px).unwrap();
                let text = std::str::from_utf8(&f.bytes).unwrap();
                assert!(!f.png && text.contains("<svg"), "{need}");
                assert_eq!(f.mono, text.contains("ColorScheme-Text"), "{need} {px}");
                if f.mono {
                    let inked = inked(&f.bytes, "#123456");
                    let text = std::str::from_utf8(&inked).unwrap();
                    assert!(!text.contains("#232629"), "{need} keeps KDE's ink");
                }
            }
        }
        // st-error's red stays red.
        let (_, f) = set.file("st-error", 16.0).unwrap();
        let text = String::from_utf8(inked(&f.bytes, "#123456").to_vec()).unwrap();
        assert!(text.contains("#da4453"), "{text}");
        assert!(!set.file("ft-pdf", 16.0).unwrap().1.mono);
    }

    /// A one-colour icon takes the ink in its greys (GNOME's #2e3436, KDE's ColorScheme-Text)
    /// and by default; its colours (a warning's orange, KDE's highlight) stay.
    #[test]
    fn a_one_colour_svg_takes_the_ink() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg"><style>.ColorScheme-Text { color:#232629; }</style><path fill="#2e3436" d="M0 0"/><path style="fill:#ff7800;stroke:#aaa" class="warning"/><path fill="url(#a)" stroke="none"/><path/></svg>"##;
        let out = String::from_utf8(inked(svg, "#123456").to_vec()).unwrap();
        assert!(out.starts_with(r##"<svg fill="#123456" "##), "{out}");
        assert!(out.contains("{ color:#123456; }"), "{out}");
        assert!(out.contains(r##"fill="#123456" d="##), "{out}");
        assert!(out.contains("fill:#ff7800;stroke:#123456"), "{out}");
        assert!(out.contains(r##"fill="url(#a)" stroke="none""##), "{out}");
        assert!(!out.contains("2e3436") && !out.contains("232629"), "{out}");
        // A drawing that names its own default keeps one attribute.
        let out = inked(br##"<svg fill="#000"><path/></svg>"##, "#ffffff");
        assert_eq!(&out[..], br##"<svg fill="#ffffff"><path/></svg>"##);
    }

    #[test]
    fn a_folder_set_reads_only_known_needs() {
        let dir = std::env::temp_dir().join(format!("mdm-iconset-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("16")).unwrap();
        std::fs::write(dir.join("16").join("tb-resume.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("16").join("not-a-need.svg"), "<svg/>").unwrap();
        std::fs::write(
            dir.join("16").join("tb-stop-symbolic.svg"),
            r##"<svg><path fill="#2e3436"/></svg>"##,
        )
        .unwrap();
        std::fs::write(dir.join("16").join("cat-music.png"), b"\x89PNG").unwrap();
        assert!(load_folder("test-folder-set", &dir, false));
        let s = set("test-folder-set").unwrap();
        assert!(s.has("tb-resume") && !s.has("not-a-need") && !s.has("tb-delete"));
        assert_eq!(s.file("tb-resume", 32.0).unwrap().0, 16);
        // By its name: a symbolic icon is one colour, a picture and a plain drawing keep theirs.
        assert!(s.file("tb-stop", 16.0).unwrap().1.mono);
        assert!(!s.file("tb-resume", 16.0).unwrap().1.mono);
        assert!(s.file("cat-music", 16.0).unwrap().1.png);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!load_folder(
            "empty",
            &std::env::temp_dir().join("mdm-no-such-set"),
            false
        ));
    }

    #[test]
    fn file_types_map_to_needs() {
        assert_eq!(file_need("x-office-document", Some("pdf")), "ft-pdf");
        assert_eq!(file_need("text-x-generic", Some("torrent")), "ft-torrent");
        assert_eq!(file_need("package-x-generic", Some("zip")), "ft-archive");
        assert_eq!(file_need("folder", None), "cat-folder");
    }
}

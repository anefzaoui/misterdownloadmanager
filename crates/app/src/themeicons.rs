//! A theme's icon set (#45): SVG files named by what MDM needs them for (`tb-add-url`,
//! `cat-music`, `ft-pdf`…, the list in [`NEEDS`]), at 16 and 24 pixels. A theme that has a set
//! draws its icons where the classic look draws its own (the toolbar's outlines, the categories'
//! Silk icons, the file types' system icons); a need the set lacks keeps the classic icon.
//!
//! Built-in sets are carried in the program (`assets/themes/<set>/icons/{16,24}/<need>.svg`). A
//! theme file in a folder of its own (`themes/<id>/theme.toml`) can bring one beside it
//! (`themes/<id>/icons/16/…`), read when the theme is chosen. A one-colour set (`icon-mono =
//! true`) is drawn white and tinted with the theme's text colour, greyed when disabled.

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

/// One set: each need's SVG at the sizes it has.
#[derive(Default)]
pub struct Set {
    files: HashMap<(String, u32), Arc<[u8]>>,
    /// Drawn white, tinted with the text colour.
    pub mono: bool,
}

impl Set {
    #[cfg(test)]
    fn has(&self, need: &str) -> bool {
        self.files.contains_key(&(need.to_owned(), 16))
            || self.files.contains_key(&(need.to_owned(), 24))
    }

    /// The SVG nearest `px`: 16 up to 20 pixels, 24 above; the other size when one is missing.
    fn svg(&self, need: &str, px: f32) -> Option<(u32, Arc<[u8]>)> {
        let order = if px <= 20.0 { [16, 24] } else { [24, 16] };
        order.iter().find_map(|s| {
            self.files
                .get(&(need.to_owned(), *s))
                .map(|b| (*s, b.clone()))
        })
    }
}

/// Declares a built-in set's files, by need, at 16 and at 24 pixels.
macro_rules! builtin_set {
    ($dir:literal; 16: [$($a:literal),* $(,)?]; 24: [$($b:literal),* $(,)?]) => {{
        let mut files = HashMap::new();
        $(files.insert(
            ($a.to_owned(), 16),
            Arc::from(&include_bytes!(concat!("../assets/themes/", $dir, "/icons/16/", $a, ".svg"))[..]),
        );)*
        $(files.insert(
            ($b.to_owned(), 24),
            Arc::from(&include_bytes!(concat!("../assets/themes/", $dir, "/icons/24/", $b, ".svg"))[..]),
        );)*
        files
    }};
}

/// The sets loaded so far, by name: the built-in ones, and the theme folders' as their themes
/// are chosen.
static SETS: LazyLock<Mutex<HashMap<String, Arc<Set>>>> = LazyLock::new(|| {
    let mut sets = HashMap::new();
    // Microsoft's Fluent UI System Icons (MIT, assets/themes/metro/icons/LICENSE), recoloured
    // white to be tinted; the magnet and the torrent file are MDM's own at their weight.
    let metro = builtin_set!("metro";
        16: ["cat-all", "cat-compressed", "cat-documents", "cat-finished", "cat-folder-open",
            "cat-folder", "cat-music", "cat-programs", "cat-queues", "cat-torrents",
            "cat-unfinished", "cat-video", "ft-archive", "ft-audio", "ft-code", "ft-disk-image",
            "ft-document", "ft-executable", "ft-font", "ft-generic", "ft-image", "ft-pdf",
            "ft-presentation", "ft-spreadsheet", "ft-text", "ft-torrent", "ft-video",
            "misc-down", "misc-emblem-ok", "misc-remove", "misc-up", "st-complete",
            "st-downloading", "st-error", "st-paused", "st-queued", "st-seeding",
            "tb-add-torrent", "tb-add-url", "tb-delete-completed", "tb-delete", "tb-options",
            "tb-resume", "tb-scheduler", "tb-start-queue", "tb-stop-all", "tb-stop-queue",
            "tb-stop", "ui-close", "ui-maximize", "ui-minimize", "ui-restore", "ui-to-tray"];
        24: ["cat-all", "cat-compressed", "cat-documents", "cat-finished", "cat-folder-open",
            "cat-folder", "cat-music", "cat-programs", "cat-queues", "cat-torrents",
            "cat-unfinished", "cat-video", "ft-archive", "ft-audio", "ft-code", "ft-document",
            "ft-executable", "ft-font", "ft-generic", "ft-image", "ft-pdf", "ft-presentation",
            "ft-spreadsheet", "ft-text", "ft-torrent", "ft-video", "misc-down",
            "misc-emblem-ok", "misc-remove", "misc-up", "st-complete", "st-downloading",
            "st-error", "st-paused", "st-queued", "st-seeding", "tb-add-torrent", "tb-add-url",
            "tb-delete-completed", "tb-delete", "tb-options", "tb-resume", "tb-scheduler",
            "tb-stop-all", "tb-stop-queue", "tb-stop", "ui-close", "ui-maximize", "ui-minimize",
            "ui-restore", "ui-to-tray"]);
    sets.insert(
        "metro".to_owned(),
        Arc::new(Set {
            files: metro,
            mono: true,
        }),
    );
    Mutex::new(sets)
});

/// Reads the set in `dir` (`16/<need>.svg`, `24/<need>.svg`) under `name`, for a theme file's
/// own folder. Only the known needs are read; other files are left alone. False when the folder
/// holds none.
pub fn load_folder(name: &str, dir: &Path, mono: bool) -> bool {
    let mut files = HashMap::new();
    for (need, _) in NEEDS {
        for px in [16u32, 24] {
            let path = dir.join(px.to_string()).join(format!("{need}.svg"));
            // An icon file is small: anything over 256 KB isn't one.
            if let Ok(meta) = std::fs::metadata(&path)
                && meta.len() <= 256 * 1024
                && let Ok(bytes) = std::fs::read(&path)
            {
                files.insert(((*need).to_owned(), px), Arc::from(bytes));
            }
        }
    }
    if files.is_empty() {
        return false;
    }
    if let Ok(mut sets) = SETS.lock() {
        sets.insert(name.to_owned(), Arc::new(Set { files, mono }));
    }
    true
}

fn set(name: &str) -> Option<Arc<Set>> {
    SETS.lock().ok()?.get(name).cloned()
}

/// The active theme's icon for `need` at about `px` pixels, and whether it is drawn in one colour
/// (to be tinted). None when the theme has no set, or its set lacks this need. Made once per
/// set, need and size, then reused: drawing it each frame allocates nothing.
pub fn source(need: &str, px: f32) -> Option<(ImageSource<'static>, bool)> {
    use std::cell::RefCell;
    type Key = (Cow<'static, str>, String, u32);
    thread_local! {
        static MADE: RefCell<HashMap<Key, Option<(ImageSource<'static>, bool)>>> = RefCell::default();
    }
    let name = crate::theme::current().icon_set.clone()?;
    let size = if px <= 20.0 { 16 } else { 24 };
    MADE.with(|made| {
        made.borrow_mut()
            .entry((name.clone(), need.to_owned(), size))
            .or_insert_with(|| {
                let set = set(&name)?;
                let (px, bytes) = set.svg(need, px)?;
                // Kept for the program's life, as its texture is.
                let uri: &'static str = Box::leak(
                    format!("bytes://mdm/icon-set/{name}/{px}/{need}.svg").into_boxed_str(),
                );
                Some((
                    ImageSource::Bytes {
                        uri: uri.into(),
                        bytes: egui::load::Bytes::Shared(bytes),
                    },
                    set.mono,
                ))
            })
            .clone()
    })
}

/// The tint a set's icon is drawn with: a one-colour set takes the text colour (greyed when
/// disabled); a coloured one its own colours.
pub fn tint(mono: bool, enabled: bool) -> Color32 {
    let c = crate::theme::colors();
    match (mono, enabled) {
        (true, true) => c.text,
        (true, false) => c.icon_disabled,
        (false, true) => Color32::WHITE,
        (false, false) => Color32::WHITE.gamma_multiply(0.45),
    }
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
    let Some((src, mono)) = source(need, rc.width()) else {
        return false;
    };
    let img = egui::Image::new(src);
    if let Ok(egui::load::TexturePoll::Ready { texture }) = img.load_for_size(ui.ctx(), rc.size()) {
        painter.image(
            texture.id,
            rc,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint(mono, enabled),
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
            let (_, svg) = metro.svg(need, 16.0).unwrap();
            let text = std::str::from_utf8(&svg).unwrap();
            // Drawn white, to be tinted: no other colour left in it.
            assert!(!text.contains("#212121"), "{need} keeps Fluent's ink");
        }
    }

    #[test]
    fn a_folder_set_reads_only_known_needs() {
        let dir = std::env::temp_dir().join(format!("mdm-iconset-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("16")).unwrap();
        std::fs::write(dir.join("16").join("tb-resume.svg"), "<svg/>").unwrap();
        std::fs::write(dir.join("16").join("not-a-need.svg"), "<svg/>").unwrap();
        assert!(load_folder("test-folder-set", &dir, false));
        let s = set("test-folder-set").unwrap();
        assert!(s.has("tb-resume") && !s.has("not-a-need") && !s.has("tb-stop"));
        assert_eq!(s.svg("tb-resume", 32.0).unwrap().0, 16);
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

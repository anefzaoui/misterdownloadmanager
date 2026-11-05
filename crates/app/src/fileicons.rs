//! The icon before a file's name, in every list that shows files (the download list, Download
//! File Info, Properties, a torrent's files): Material Icon Theme's icon for its type (MIT,
//! assets/icons/material/, tools/icons/material.py), the theme's folder for a tree's folders,
//! MDM's own magnet for a .torrent, and Lucide's plain file for a type it has no icon for. The
//! same on every system: drawn from the SVGs the app carries, through the SVG loader the toolbar
//! icons already use (no shell icons, no network).

use egui::{Color32, ImageSource, Painter, Rect, Ui, pos2};

#[path = "fileicons_table.rs"]
mod table;

/// What the icon is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind<'a> {
    /// A file, by its name (the extension decides; case doesn't matter).
    File(&'a str),
    /// A folder (a torrent's folder, a tree's branch).
    Folder,
}

/// The extension of `name` (after its last dot, lowercase), if it has one.
fn extension(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 12 => {
            Some(ext.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// The theme's icon index for `ext`, if it has one.
fn index(ext: &str) -> Option<usize> {
    table::EXTENSIONS
        .binary_search_by(|(e, _)| (*e).cmp(ext))
        .ok()
        .map(|i| table::EXTENSIONS[i].1 as usize)
}

/// Lucide's "file" outline (assets/icons/lucide/file.svg), stroked in a grey that reads on both
/// the light and the dark field.
const GENERIC: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="#8a8f99" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/></svg>"##;

/// The image for `kind`. `dark`: the dark theme (a few theme icons have a light-field variant).
pub fn source(kind: Kind, dark: bool) -> ImageSource<'static> {
    let ext = match kind {
        Kind::Folder => return theme_icon(0, dark),
        Kind::File(name) => extension(name),
    };
    match ext.as_deref() {
        Some("torrent") => crate::icons::Icon::AddTorrent.source(true),
        Some(e) => match index(e) {
            Some(i) => theme_icon(i, dark),
            None => generic(),
        },
        None => generic(),
    }
}

fn theme_icon(i: usize, dark: bool) -> ImageSource<'static> {
    let icon = &table::ICONS[i];
    let (bytes, variant) = match icon.light {
        Some(l) if !dark => (l, "-light"),
        _ => (icon.svg, ""),
    };
    ImageSource::Bytes {
        uri: format!("bytes://mdm/file-icon/{}{variant}.svg", icon.name).into(),
        bytes: egui::load::Bytes::Static(bytes),
    }
}

fn generic() -> ImageSource<'static> {
    ImageSource::Bytes {
        uri: "bytes://mdm/file-icon/generic.svg".into(),
        bytes: egui::load::Bytes::Static(GENERIC.as_bytes()),
    }
}

/// Draw `kind`'s icon in `rc` (on screen) through `painter`, so it keeps to the list's clip.
/// Rendered for the screen's pixels, so it stays sharp at every scale.
pub fn paint(ui: &Ui, painter: &Painter, kind: Kind, rc: Rect) {
    let img = egui::Image::new(source(kind, crate::theme::dark()))
        .fit_to_exact_size(rc.size())
        .texture_options(egui::TextureOptions::LINEAR);
    if let Ok(egui::load::TexturePoll::Ready { texture }) = img.load_for_size(ui.ctx(), rc.size()) {
        painter.image(
            texture.id,
            rc,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// [`paint`] at English point `min` (a list's layout), `side` points square: placed where that
/// is on screen, so in a right-to-left window the icon is at the row's start there too.
pub fn paint_at(ui: &Ui, painter: &Painter, kind: Kind, min: egui::Pos2, side: f32) {
    let rc = crate::winui::rect(Rect::from_min_size(min, egui::vec2(side, side)));
    paint(ui, painter, kind, rc);
}

/// How far a name moves to make room for its 16-point icon (the icon and a gap).
pub const INDENT: f32 = 20.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn icon_of(name: &str) -> &'static str {
        match extension(name).and_then(|e| index(&e)) {
            Some(i) => table::ICONS[i].name,
            None => "generic",
        }
    }

    #[test]
    fn the_table_is_sorted_and_every_icon_is_an_svg() {
        assert!(table::EXTENSIONS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(
            table::EXTENSIONS
                .iter()
                .all(|(_, i)| (*i as usize) < table::ICONS.len())
        );
        for table::Icon { name, svg, light } in table::ICONS {
            assert!(svg.starts_with(b"<svg"), "{name}");
            assert!(light.is_none_or(|l| l.starts_with(b"<svg")), "{name}");
        }
        assert!(GENERIC.starts_with("<svg"));
    }

    #[test]
    fn a_download_managers_files_get_their_type() {
        for (name, icon) in [
            ("movie.mp4", "video"),
            ("stream.TS", "video"),
            ("song.flac", "audio"),
            ("song.ogg", "audio"),
            ("setup.exe", "exe"),
            ("setup.msi", "exe"),
            ("app.dmg", "disc"),
            ("tool.deb", "exe"),
            ("tool.rpm", "exe"),
            ("tool.AppImage", "exe"),
            ("game.apk", "android"),
            ("ubuntu.iso", "disc"),
            ("photos.7z", "zip"),
            ("backup.tar.gz", "zip"),
            ("report.pdf", "pdf"),
            ("book.mobi", "epub"),
            ("film.srt", "subtitles"),
            ("C:\\dl\\notes.TXT", "document"),
        ] {
            assert_eq!(icon_of(name), icon, "{name}");
        }
        // No extension, a dot file, an unknown type: the plain file.
        assert_eq!(icon_of("README"), "generic");
        assert_eq!(icon_of(".hidden"), "generic");
        assert_eq!(icon_of("data.qqqq"), "generic");
    }

    #[test]
    fn torrents_and_folders_have_their_own() {
        let uri = |s: ImageSource| match s {
            ImageSource::Bytes { uri, .. } => uri.to_string(),
            _ => String::new(),
        };
        assert!(uri(source(Kind::File("x.torrent"), true)).contains("add-torrent"));
        assert!(uri(source(Kind::Folder, true)).contains("folder-base"));
        assert!(uri(source(Kind::File("README"), false)).contains("generic"));
    }
}

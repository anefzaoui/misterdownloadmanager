//! Rename row (`Rename`: main name + extension) in the Add torrent dialog (#517).
//!
//! - Placed directly below the `Save to` row.
//! - Splits the target name into two separate inputs:
//!   1. **Main name (`stem`)**
//!   2. **Extension (`ext`)**
//! - When the torrent is a directory (multi-file torrent):
//!   - The main name input edits the folder name.
//!   - The extension input is greyed out and disabled (`enabled = false`).
//! - When the torrent is a single file:
//!   - Both the main name and extension inputs are enabled and editable separately.

use std::path::{Path, PathBuf};
use std::sync::Once;

use egui::{Painter, Pos2, Ui};
use mdm_torrent::Meta;

use crate::winui::{self, r, text};

#[derive(Default)]
pub struct TorrentRenamePlugin {
    pub stem: String,
    pub ext: String,
    orig_stem: String,
    orig_ext: String,
    pub is_dir: bool,
    single_idx: Option<usize>,
    orig_parent: Option<PathBuf>,
}

/// Split a single file's name into `(stem, ext)`.
pub fn split_file_name(name: &str) -> (String, String) {
    if let Some((s, e)) = name.rsplit_once('.')
        && !s.is_empty()
        && !e.is_empty()
    {
        (s.to_owned(), e.to_owned())
    } else {
        (name.to_owned(), String::new())
    }
}

fn sanitize_part(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned.trim().trim_end_matches('.').to_owned()
}

impl TorrentRenamePlugin {
    /// Translated label for the Rename row in Add torrent.
    pub fn label() -> &'static str {
        // i18n: noun: label before the new file or folder name in Add torrent
        crate::i18n::t("Rename")
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn on_meta_loaded(&mut self, meta: &Meta) {
        let non_padding: Vec<(usize, &mdm_torrent::FileEntry)> = meta
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.padding)
            .collect();
        if non_padding.len() == 1 {
            let (idx, entry) = non_padding[0];
            let file_name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| meta.name.clone());
            let parent = entry
                .path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf);
            let (stem, ext) = split_file_name(&file_name);
            self.stem = stem.clone();
            self.ext = ext.clone();
            self.orig_stem = stem;
            self.orig_ext = ext;
            self.is_dir = false;
            self.single_idx = Some(idx);
            self.orig_parent = parent;
        } else {
            self.stem = meta.name.clone();
            self.ext = String::new();
            self.orig_stem = meta.name.clone();
            self.orig_ext = String::new();
            self.is_dir = true;
            self.single_idx = None;
            self.orig_parent = None;
        }
    }

    /// Compute the effective target name and apply it to `Meta`.
    pub fn apply_to_meta(&self, meta: &mut Meta) -> String {
        if self.is_dir {
            let folder_name = sanitize_part(&self.stem);
            let effective = if folder_name.is_empty() {
                self.orig_stem.clone()
            } else {
                folder_name
            };
            meta.name = effective.clone();
            effective
        } else {
            let stem_part = sanitize_part(&self.stem);
            let stem_effective = if stem_part.is_empty() {
                self.orig_stem.clone()
            } else {
                stem_part
            };
            let ext_part = sanitize_part(self.ext.trim().trim_start_matches('.'));
            let file_name = if ext_part.is_empty() {
                stem_effective
            } else {
                format!("{stem_effective}.{ext_part}")
            };
            meta.name = file_name.clone();
            if let Some(idx) = self.single_idx
                && let Some(entry) = meta.files.get_mut(idx)
            {
                entry.path = match &self.orig_parent {
                    Some(parent) => parent.join(&file_name),
                    None => PathBuf::from(&file_name),
                };
            }
            file_name
        }
    }

    /// Render the `Rename` row at vertical position `y`.
    /// Returns `true` if the user edited either the main name or the extension.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        p: &Painter,
        o: Pos2,
        fx: f32,
        y: f32,
        has: bool,
        busy: bool,
        meta: &mut Option<Meta>,
    ) -> bool {
        let label = Self::label();
        text(p, o.x + 11.0, o.y + y + 14.0, label, winui::TEXT());

        let stem_enabled = has && !busy;
        let ext_enabled = has && !busy && !self.is_dir;

        // Main name (stem) input box
        let stem_changed = winui::edit(
            ui,
            r(o, fx, y, 468.0 - fx, 21.0),
            &mut self.stem,
            stem_enabled,
            false,
        )
        .changed();

        // Dot separator between main name and extension
        let dot_color = if ext_enabled {
            winui::TEXT()
        } else {
            winui::LIGHT_DISABLED()
        };
        text(p, o.x + 472.0, o.y + y + 14.0, ".", dot_color); // not translated: file extension dot

        // Extension input box (greyed out / disabled when downloading a directory)
        let ext_changed = winui::edit(
            ui,
            r(o, 480.0, y, 589.0 - 480.0, 21.0),
            &mut self.ext,
            ext_enabled,
            false,
        )
        .changed();

        let changed = (stem_changed || ext_changed) && has && !busy;
        if changed && let Some(m) = meta.as_mut() {
            self.apply_to_meta(m);
        }
        changed
    }
}

fn session_dir() -> Option<PathBuf> {
    crate::torrents::state_dir().map(|d| d.join("session"))
}

/// Ensure saved single-file renames are loaded into memory on startup.
pub fn ensure_loaded() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if let Some(dir) = session_dir() {
            let path = dir.join("renames.json");
            if let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(map) =
                    serde_json::from_str::<std::collections::HashMap<String, String>>(&text)
            {
                for (hash, rel) in map {
                    mdm_torrent::rename_plugin::set_renamed_file(&dir, &hash, &rel);
                }
            }
        }
    });
}

/// Persist single-file torrent rename when adding a torrent from `TorrentDialog`,
/// only when the file name was actually changed from the torrent's original name.
pub fn on_add_torrent(meta: &Meta) {
    ensure_loaded();
    let Some(dir) = session_dir() else { return };
    on_add_torrent_in(&dir, meta);
}

pub fn on_add_torrent_in(session_dir: &Path, meta: &Meta) {
    let non_padding: Vec<&mdm_torrent::FileEntry> =
        meta.files.iter().filter(|f| !f.padding).collect();
    if let [single] = non_padding.as_slice() {
        mdm_torrent::rename_plugin::sync_renamed_file(
            session_dir,
            &meta.info_hash,
            &meta.torrent,
            &single.path,
        );
    }
}

/// Clean up rename mapping when a torrent is removed from the list.
pub fn on_forget_torrent(hash: &str) {
    ensure_loaded();
    if let Some(dir) = session_dir() {
        mdm_torrent::rename_plugin::remove_renamed_file(&dir, hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_torrent::FileEntry;

    fn sample_meta(name: &str, files: &[(&str, u64)]) -> Meta {
        Meta {
            name: name.to_owned(),
            info_hash: "a".repeat(40),
            files: files
                .iter()
                .map(|(p, s)| FileEntry {
                    path: PathBuf::from(p),
                    size: *s,
                    padding: false,
                })
                .collect(),
            piece_length: 16384,
            torrent: Vec::new(),
            peers: Vec::new(),
        }
    }

    #[test]
    fn single_file_splits_stem_and_extension_and_renames_independently() {
        let file = "[LoliHouse] Dark Summoner to Dekiteiru - 06 [WebRip 1080p HEVC-10bit AAC].mkv";
        let mut meta = sample_meta(file, &[(file, 424_433_664)]);
        let mut plugin = TorrentRenamePlugin::default();
        plugin.on_meta_loaded(&meta);

        assert!(!plugin.is_dir);
        assert_eq!(
            plugin.stem,
            "[LoliHouse] Dark Summoner to Dekiteiru - 06 [WebRip 1080p HEVC-10bit AAC]"
        );
        assert_eq!(plugin.ext, "mkv");

        // Modify stem and extension separately
        plugin.stem = "Dark Summoner - EP06".to_owned();
        plugin.ext = "mp4".to_owned();
        plugin.apply_to_meta(&mut meta);

        assert_eq!(meta.name, "Dark Summoner - EP06.mp4");
        assert_eq!(
            meta.files[0].path,
            PathBuf::from("Dark Summoner - EP06.mp4")
        );
    }

    #[test]
    fn directory_torrent_disables_extension_and_renames_folder() {
        let mut meta = sample_meta(
            "Season.01.1080p.Pack",
            &[("ep01.mkv", 100), ("ep02.mkv", 200)],
        );
        let mut plugin = TorrentRenamePlugin::default();
        plugin.on_meta_loaded(&meta);

        assert!(plugin.is_dir);
        assert_eq!(plugin.stem, "Season.01.1080p.Pack");
        assert_eq!(plugin.ext, "");

        plugin.stem = "My Renamed Folder".to_owned();
        plugin.apply_to_meta(&mut meta);

        assert_eq!(meta.name, "My Renamed Folder");
        assert_eq!(
            meta.folder_in(Path::new("/tmp/dl")),
            PathBuf::from("/tmp/dl/My Renamed Folder")
        );
    }
}

//! Safe deferred rename (`Item::rename_on_complete`) from a download's progress window.
//!
//! Typing a new name while a download runs never touches the live HTTP stream, signed URL,
//! temporary `.mdm.part` state, or torrent session in flight. Instead, the chosen main name is
//! stored on the item and applied in place once the download reaches `Status::Complete`:
//! - Single-file downloads replace only the finalized file's main stem, keeping its real
//!   extension (including compound archive suffixes such as `.tar.gz`).
//! - Folder downloads (multi-file torrents, image galleries) replace the root folder's name,
//!   leaving every file and extension inside untouched.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::model::Item;

/// Idle delay after typing in the progress window's Rename box before saving automatically.
pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(3);

const COMPOUND_EXTS: &[&str] = &[
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4", ".tar.lz", ".tar.br",
];

/// Unicode format characters (category Cf), matching `mdm_engine::naming`.
fn is_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{600}'..='\u{605}'
            | '\u{61C}'
            | '\u{6DD}'
            | '\u{70F}'
            | '\u{890}'..='\u{891}'
            | '\u{8E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'..='\u{E007F}'
    )
}

fn is_windows_reserved(s: &str) -> bool {
    let head = s.split('.').next().unwrap_or_default().trim();
    let upper = head.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" // not translated: Windows DOS device names
    ) || ((upper.starts_with("COM") || upper.starts_with("LPT")) // not translated: Windows DOS port prefixes
        && upper.chars().count() == 4
        && upper
            .chars()
            .nth(3)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³')))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameError {
    TrailingDot,
    TooLong,
    InvalidChar,
    ReservedDevice,
}

/// Validate the text typed in the Rename box.
///
/// - `Ok(None)`: blank / whitespace-only (clears any pending deferred rename).
/// - `Ok(Some(clean))`: safe single path component (trimmed) ready to save.
/// - `Err(reason)`: forbidden path characters, traversal dots, or reserved device name.
pub fn validate_name(raw: &str) -> Result<Option<String>, RenameError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed == "." || trimmed == ".." || trimmed.ends_with('.') {
        return Err(RenameError::TrailingDot);
    }
    if trimmed.encode_utf16().count() > 200 {
        return Err(RenameError::TooLong);
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || is_format_char(c) || r#"<>:"/\|?*"#.contains(c))
    {
        return Err(RenameError::InvalidChar);
    }
    if is_windows_reserved(trimmed) {
        return Err(RenameError::ReservedDevice);
    }
    Ok(Some(trimmed.to_owned()))
}

/// Split a finalized file name into `(stem, ext_with_dot)`.
///
/// Preserves compound archive suffixes (`.tar.gz`, `.tar.xz`, …) and treats leading-dot names
/// without another dot (`.gitignore`) as extensionless.
pub fn split_stem_and_ext(file_name: &str) -> (&str, &str) {
    let lower = file_name.to_ascii_lowercase();
    for &cext in COMPOUND_EXTS {
        if lower.len() > cext.len() && lower.ends_with(cext) {
            let at = file_name.len() - cext.len();
            if at > 0 && &file_name[..at] != "." {
                return (&file_name[..at], &file_name[at..]);
            }
        }
    }
    match file_name.rfind('.') {
        Some(i) if i > 0 && i + 1 < file_name.len() => (&file_name[..i], &file_name[i..]),
        _ => (file_name, ""),
    }
}

/// Strip a duplicate trailing extension when the user typed it themselves (e.g. typing
/// `"Movie.mp4"` for a file whose finalized extension is already `.mp4`).
fn strip_duplicate_ext<'a>(desired: &'a str, real_ext: &str) -> &'a str {
    if real_ext.is_empty() || desired.len() <= real_ext.len() {
        return desired;
    }
    let split_at = desired.len() - real_ext.len();
    if !desired.is_char_boundary(split_at) {
        return desired;
    }
    let (prefix, tail) = desired.split_at(split_at);
    let trimmed = prefix.trim_end_matches(['.', ' ']);
    if tail.eq_ignore_ascii_case(real_ext) && !trimmed.is_empty() {
        trimmed
    } else {
        desired
    }
}

/// Build the target entry name inside the download's parent folder.
pub fn build_renamed_file_name(current_name: &str, desired_stem: &str, is_dir: bool) -> String {
    if is_dir {
        return desired_stem.to_owned();
    }
    let (_, real_ext) = split_stem_and_ext(current_name);
    let base = strip_duplicate_ext(desired_stem, real_ext);
    format!("{base}{real_ext}")
}

/// Whether `item`'s destination stands for a folder (multi-file torrent, gallery, multi-file
/// debrid transfer) rather than a single file.
pub fn is_folder_target(item: &Item) -> bool {
    item.gallery.is_some()
        || item.torrent.as_ref().is_some_and(|t| t.folder == item.dest)
        || item.dest.is_dir()
}

/// Whether two paths refer to the same existing file or folder (e.g. a case-only rename on a
/// case-insensitive APFS or NTFS volume, where `canonicalize` resolves both to the stored entry).
fn same_existing_entry(a: &Path, b: &Path) -> bool {
    let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) else {
        return false;
    };
    ca == cb
}

fn entry_taken(p: &Path) -> bool {
    p.exists() || mdm_engine::naming::part_path(p).exists()
}

/// Resolve the collision-safe destination path in `current_dest`'s parent folder, or `None`
/// when `current_dest` already has the exact desired name.
pub fn resolved_target_path(
    current_dest: &Path,
    desired_stem: &str,
    is_dir: bool,
) -> Option<PathBuf> {
    let parent = current_dest.parent()?;
    let cur_name = current_dest.file_name()?.to_string_lossy();
    let raw_wanted = build_renamed_file_name(&cur_name, desired_stem, is_dir);
    let wanted = mdm_engine::naming::fit_name(parent, &raw_wanted).unwrap_or(raw_wanted);
    if wanted == cur_name.as_ref() {
        return None;
    }
    let first = parent.join(&wanted);
    if !entry_taken(&first) || same_existing_entry(current_dest, &first) {
        return Some(first);
    }
    let (base, ext) = if is_dir {
        (wanted.as_str(), "")
    } else {
        split_stem_and_ext(&wanted)
    };
    let numbered = (1u32..)
        .map(|n| {
            let candidate = format!("{base} ({n}){ext}");
            let fitted = mdm_engine::naming::fit_name(parent, &candidate).unwrap_or(candidate);
            parent.join(fitted)
        })
        .find(|p| !entry_taken(p))?;
    Some(numbered)
}

/// Rename `from` to `to` within the same parent folder, stepping through a temporary sibling
/// if a case-insensitive filesystem rejects a direct case-only rename.
pub fn rename_entry(from: &Path, to: &Path) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    let case_only = from.parent() == to.parent()
        && from.file_name().zip(to.file_name()).is_some_and(|(a, b)| {
            a.to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy())
        })
        && same_existing_entry(from, to);
    if !case_only && std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    if case_only {
        let parent = from.parent().unwrap_or_else(|| Path::new("."));
        let stamp = std::process::id();
        for k in 0..16u32 {
            let hop = parent.join(format!(".mdm-rename-{stamp}-{k}.tmp"));
            if !hop.exists() && std::fs::rename(from, &hop).is_ok() {
                return std::fs::rename(&hop, to).map_err(|e| {
                    let _ = std::fs::rename(&hop, from);
                    e.to_string()
                });
            }
        }
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_safe_and_unsafe_names() {
        assert_eq!(validate_name(""), Ok(None));
        assert_eq!(validate_name("   "), Ok(None));
        assert_eq!(
            validate_name("  My Movie (2026)  "),
            Ok(Some("My Movie (2026)".into()))
        );
        assert_eq!(validate_name("归来_第01集"), Ok(Some("归来_第01集".into())));
        for bad in [
            ".",
            "..",
            "trailing.",
            "sub/file",
            r"sub\file",
            "a:b",
            "a*b",
            "a?b",
            "CON",
            "aux",
            "lpt1",
            "COM¹",
            "nul.part",
            "bad\u{202E}name",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn splits_single_and_compound_extensions() {
        assert_eq!(split_stem_and_ext("89af12.mp4"), ("89af12", ".mp4"));
        assert_eq!(
            split_stem_and_ext("archive.v1.tar.gz"),
            ("archive.v1", ".tar.gz")
        );
        assert_eq!(split_stem_and_ext("ARCHIVE.TAR.XZ"), ("ARCHIVE", ".TAR.XZ"));
        assert_eq!(split_stem_and_ext("README"), ("README", ""));
        assert_eq!(split_stem_and_ext(".gitignore"), (".gitignore", ""));
    }

    #[test]
    fn replaces_stem_keeping_real_extension_or_folder_name() {
        // Cloud-drive unknown download whose server finalized it as a random hash .mp4:
        assert_eq!(
            build_renamed_file_name("9f82a10b3c.mp4", "Lecture 01", false),
            "Lecture 01.mp4"
        );
        // Compound archive suffix stays whole:
        assert_eq!(
            build_renamed_file_name("snapshot-20261111.tar.gz", "clean-backup", false),
            "clean-backup.tar.gz"
        );
        // Typing the same extension manually doesn't double it:
        assert_eq!(
            build_renamed_file_name("9f82a10b3c.mp4", "Lecture 01.MP4", false),
            "Lecture 01.mp4"
        );
        // Version numbers in stems stay intact:
        assert_eq!(
            build_renamed_file_name("installer.dmg", "App v2.4", false),
            "App v2.4.dmg"
        );
        // Folder target replaces the whole folder name without touching dots:
        assert_eq!(
            build_renamed_file_name("Ubuntu-24.04-Torrent", "Linux Mint 22.1", true),
            "Linux Mint 22.1"
        );
    }

    #[test]
    fn resolves_collisions_and_case_only_renames_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("unknown_hash.tar.gz");
        std::fs::write(&src, b"payload").unwrap();

        // Already occupying "backup.tar.gz": picks "backup (1).tar.gz" keeping compound ext.
        std::fs::write(dir.path().join("backup.tar.gz"), b"other").unwrap();
        let to = resolved_target_path(&src, "backup", false).unwrap();
        assert_eq!(to, dir.path().join("backup (1).tar.gz"));
        rename_entry(&src, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"payload");
        assert_eq!(
            std::fs::read(dir.path().join("backup.tar.gz")).unwrap(),
            b"other"
        );

        // Folder collision with a dot in its name numbers at the end ("Season 1.5 (1)").
        let fsrc = dir.path().join("raw_pack");
        std::fs::create_dir_all(&fsrc).unwrap();
        std::fs::write(fsrc.join("ep01.mkv"), b"ep1").unwrap();
        std::fs::create_dir_all(dir.path().join("Season 1.5")).unwrap();
        let fto = resolved_target_path(&fsrc, "Season 1.5", true).unwrap();
        assert_eq!(fto, dir.path().join("Season 1.5 (1)"));
        rename_entry(&fsrc, &fto).unwrap();
        assert_eq!(std::fs::read(fto.join("ep01.mkv")).unwrap(), b"ep1");

        // Case-only rename never appends " (1)".
        let lower = dir.path().join("movie.mp4");
        std::fs::write(&lower, b"video").unwrap();
        let upper = resolved_target_path(&lower, "Movie", false).unwrap();
        assert_eq!(upper, dir.path().join("Movie.mp4"));
        rename_entry(&lower, &upper).unwrap();
        assert_eq!(std::fs::read(&upper).unwrap(), b"video");
    }
}

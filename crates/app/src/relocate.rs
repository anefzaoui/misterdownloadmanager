//! A download's files moved to another folder: a finished file (Move/Rename, File Properties →
//! Move), and an unfinished download's part file and resume state (#1739), so it resumes there
//! from where it stopped instead of starting over.
//!
//! An unfinished download's move is safe at every step, a crash included. Its row first names
//! both places ([`crate::model::Item::move_from`], the list saved with it), then:
//!
//! 1. its part file is renamed to the new place, or (another drive) copied there and flushed;
//! 2. its resume state is written there, naming its new place;
//! 3. the old part file (after a copy) and the old resume state are deleted.
//!
//! A resume state at the new place therefore means a whole part file is there with it. A move
//! that fails undoes what it did, so the download stays where it was, whole; one that MDM's end
//! cut short is finished or undone by [`settle`] at the next start, by that same rule. Only the
//! download's own `.mdm.part` and `.mdm.json` are ever touched, never a finished file.

use std::io;
use std::path::{Path, PathBuf};

use mdm_engine::naming::part_path;
use mdm_engine::state::{ResumeState, state_path};

/// How files are renamed, copied and deleted, and how much room a folder has: the disk's own
/// ([`Disk::REAL`]), or a test's (a copy that fails half-way, a drive that is full).
pub struct Disk {
    pub rename: fn(&Path, &Path) -> io::Result<()>,
    /// A copy that is on the disk when it returns (flushed).
    pub copy: fn(&Path, &Path) -> io::Result<()>,
    pub remove: fn(&Path) -> io::Result<()>,
    /// The bytes a new file may take in this folder (or the nearest one above it that exists);
    /// None when the system can't tell (then nothing is refused for it).
    pub free: fn(&Path) -> Option<u64>,
}

impl Disk {
    pub const REAL: Disk = Disk {
        rename: real_rename,
        copy: copy_flushed,
        remove: real_remove,
        free: real_free,
    };
}

fn real_rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

fn real_remove(p: &Path) -> io::Result<()> {
    std::fs::remove_file(p)
}

fn real_free(dir: &Path) -> Option<u64> {
    mdm_engine::disk::volume_of(dir).map(|v| v.free)
}

/// A copy, flushed to the disk before it counts: a power cut after the original is deleted
/// must not leave a copy whose bytes were still in the cache.
fn copy_flushed(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::copy(from, to)?;
    std::fs::OpenOptions::new().write(true).open(to)?.sync_all()
}

/// The folder `to` goes into, made if it isn't there (as a download's folder is made as it
/// starts).
fn make_folder(to: &Path) -> Result<(), String> {
    match to.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())
        }
        _ => Ok(()),
    }
}

/// `len` bytes fit in `to`'s folder, with room to spare ([`mdm_engine::disk::MARGIN`], as a new
/// download leaves): else why not, before a byte is copied.
fn fits(to: &Path, len: u64, disk: &Disk) -> Result<(), String> {
    let dir = to.parent().unwrap_or(Path::new("."));
    match (disk.free)(dir) {
        Some(free) if free < len.saturating_add(mdm_engine::disk::MARGIN) => {
            Err(crate::i18n::fmt(
                // i18n: why a download's file could not be moved to another drive (after "The file could not be moved: "); {needed} and {free} are sizes ("1.2 GB")
                crate::i18n::t(
                    "there isn't enough free space there ({needed} needed, {free} free)",
                ),
                &[
                    (
                        "needed",
                        &crate::i18n::ltr(&crate::dialogs::dialog_size(len)),
                    ),
                    (
                        "free",
                        &crate::i18n::ltr(&crate::dialogs::dialog_size(free)),
                    ),
                ],
            ))
        }
        _ => Ok(()),
    }
}

/// Move a finished file to `to` (its name too): a rename on the same drive, else a copy (when it
/// fits) and the original deleted. A copy whose original can't go (open in another program) is
/// taken back: the file stays in one place. The folder is made if it isn't there.
pub fn move_file(from: &Path, to: &Path, disk: &Disk) -> Result<(), String> {
    make_folder(to)?;
    if (disk.rename)(from, to).is_ok() {
        return Ok(());
    }
    let len = std::fs::metadata(from).map_err(|e| e.to_string())?.len();
    fits(to, len, disk)?;
    if let Err(e) = (disk.copy)(from, to) {
        let _ = (disk.remove)(to);
        return Err(e.to_string());
    }
    (disk.remove)(from).map_err(|e| {
        let _ = (disk.remove)(to);
        e.to_string()
    })
}

/// Move an unfinished download from `old` to `new` (its finished file's places, which aren't
/// there yet): its part file and resume state, the state rewritten to name its new place (the
/// engine resumes only a state that names its own). Either all of it is at `new` (Ok), or all of
/// it is still at `old` and the error says why. The caller has saved the list with both places
/// first, holds both names ([`mdm_engine::naming::reserve`]), and has made sure the download no
/// longer writes there ([`mdm_engine::naming::wait_settled`]). `had_data`: some of it had come in (its row's count);
/// one that hasn't started may have no folder yet, and only takes its new place.
pub fn move_unfinished(old: &Path, new: &Path, had_data: bool, disk: &Disk) -> Result<(), String> {
    let (old_part, old_state) = (part_path(old), state_path(old));
    let (new_part, new_state) = (part_path(new), state_path(new));
    // A folder that isn't there (a drive taken out, a network share away): its files can't be
    // told from files that are gone, and the download keeps its place until it is back.
    if let Some(dir) = old.parent().filter(|d| !d.as_os_str().is_empty())
        && !dir.is_dir()
    {
        if !had_data {
            return make_folder(new);
        }
        return Err(crate::i18n::fmt(
            // i18n: why an unfinished download could not be moved (after "The file could not be moved: "): its folder isn't there now (a drive taken out); {folder} is that folder
            crate::i18n::t("its folder {folder} can't be reached"),
            &[("folder", &crate::i18n::ltr(&dir.display().to_string()))],
        ));
    }
    // Its new name was free when it was chosen; something written there since stays untouched.
    if let Some(there) = [new, &new_part, &new_state]
        .into_iter()
        .find(|p| p.exists())
    {
        return Err(crate::i18n::fmt(
            // i18n: why a download could not be moved (after "The file could not be moved: "): a file of that name came into the new folder meanwhile; {file} is its name
            crate::i18n::t("{file} is already there"),
            &[(
                "file",
                &crate::i18n::isolate(&there.file_name().unwrap_or_default().to_string_lossy()),
            )],
        ));
    }
    make_folder(new)?;
    let part = old_part.is_file();
    let state = old_state.is_file();
    // The state as it is to be at the new place. One that can't be read goes as it is: it
    // couldn't be resumed where it was either, and the engine says so.
    let rewritten = state
        .then(|| ResumeState::load(&old_state).ok())
        .flatten()
        .map(|mut s| {
            s.dest = new.to_owned();
            s
        });
    // 1. The part file, whole at the new place.
    let mut renamed = false;
    if part {
        if (disk.rename)(&old_part, &new_part).is_ok() {
            renamed = true;
        } else {
            let len = std::fs::metadata(&old_part)
                .map_err(|e| e.to_string())?
                .len();
            fits(new, len, disk)?;
            if let Err(e) = (disk.copy)(&old_part, &new_part) {
                let _ = (disk.remove)(&new_part);
                return Err(e.to_string());
            }
        }
    }
    let undo_part = || {
        if renamed {
            let _ = (disk.rename)(&new_part, &old_part);
        } else if part {
            let _ = (disk.remove)(&new_part);
        }
    };
    // 2. Its resume state, naming the new place: from here the move counts as done.
    if state {
        let wrote = match &rewritten {
            Some(s) => s.write(&new_state),
            None => (disk.copy)(&old_state, &new_state),
        };
        if let Err(e) = wrote {
            let _ = (disk.remove)(&new_state);
            undo_part();
            return Err(e.to_string());
        }
    }
    // 3. What is left at the old place. A copied part file whose original can't go (another
    // program has it open) is taken back, its new state first: the download stays where it was.
    if part
        && !renamed
        && let Err(e) = (disk.remove)(&old_part)
    {
        let _ = (disk.remove)(&new_state);
        let _ = (disk.remove)(&new_part);
        return Err(e.to_string());
    }
    if state && let Err(e) = (disk.remove)(&old_state) {
        // Its part file is gone from there: the old state alone resumes nothing.
        tracing::warn!(
            "move: the old resume state {} stayed: {e}",
            old_state.display()
        );
    }
    Ok(())
}

/// Where an unfinished download is after a move that MDM's end cut short (it was found with
/// [`crate::model::Item::move_from`] at start): its new place when its resume state was written
/// there (its part file came before it, whole), and what is left of it at the old place goes;
/// else its old place, and what the move had put at the new one is taken back. Only the
/// download's own part files and resume states are touched.
pub fn settle(old: &Path, new: &Path, disk: &Disk) -> PathBuf {
    let (old_part, old_state) = (part_path(old), state_path(old));
    let (new_part, new_state) = (part_path(new), state_path(new));
    if new_state.is_file() {
        for f in [&old_part, &old_state] {
            if f.exists() {
                let _ = (disk.remove)(f);
            }
        }
        return new.to_owned();
    }
    if old_part.exists() {
        // A copy cut short (or whole, but its state never came): the original is whole.
        if new_part.exists() {
            let _ = (disk.remove)(&new_part);
        }
    } else if new_part.exists() && (disk.rename)(&new_part, &old_part).is_err() {
        // Renamed there, and it can't come back now: the part file is where it is.
        return new.to_owned();
    }
    old.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_engine::planner::SavedSegment;
    use mdm_engine::state::Validators;

    const PART: &[u8] = b"0123456789abcdefghij";

    /// An unfinished download at `dir/name`: its part file and a resume state of two segments.
    fn unfinished(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let dest = dir.join(name);
        std::fs::write(part_path(&dest), PART).unwrap();
        let url: url::Url = "http://example.com/a.bin".parse().unwrap();
        let mut s = ResumeState::new(url.clone(), url, dest.clone(), 40, Validators::default());
        s.segments = vec![
            SavedSegment {
                start: 0,
                cursor: 10,
                end: 20,
            },
            SavedSegment {
                start: 20,
                cursor: 30,
                end: 40,
            },
        ];
        s.write(&state_path(&dest)).unwrap();
        dest
    }

    /// Everything of it is at `dest`, whole, its state naming `dest`; nothing at `gone`.
    fn assert_at(dest: &Path, gone: &Path) {
        assert_eq!(std::fs::read(part_path(dest)).unwrap(), PART);
        let s = ResumeState::load(&state_path(dest)).unwrap();
        assert_eq!(s.dest, dest, "the state names its new place");
        assert_eq!(s.downloaded(), 20, "it goes on from where it was");
        assert!(!part_path(gone).exists() && !state_path(gone).exists());
        assert!(!dest.exists() && !gone.exists(), "no finished file is made");
    }

    fn cross_device(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("not the same device"))
    }

    fn copy_half_then_fail(from: &Path, to: &Path) -> io::Result<()> {
        let bytes = std::fs::read(from)?;
        std::fs::write(to, &bytes[..bytes.len() / 2])?;
        Err(io::Error::other("the drive was unplugged"))
    }

    fn full(_: &Path) -> Option<u64> {
        Some(1000)
    }

    /// Another drive: no rename, a copy then the original deleted.
    const OTHER_DRIVE: Disk = Disk {
        rename: cross_device,
        ..Disk::REAL
    };

    #[test]
    fn on_the_same_drive_its_files_are_renamed_and_its_state_names_the_new_place() {
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = dir.path().join("b").join("file.bin");
        move_unfinished(&old, &new, true, &Disk::REAL).unwrap();
        assert_at(&new, &old);
    }

    #[test]
    fn to_another_drive_its_part_is_copied_then_the_original_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = dir.path().join("b").join("file (1).bin");
        move_unfinished(&old, &new, true, &OTHER_DRIVE).unwrap();
        assert_at(&new, &old);
    }

    #[test]
    fn a_folder_that_isnt_there_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = dir.path().join("new").join("deeper").join("file.bin");
        move_unfinished(&old, &new, true, &OTHER_DRIVE).unwrap();
        assert_at(&new, &old);
    }

    #[test]
    fn a_copy_that_fails_half_way_leaves_the_download_where_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = dir.path().join("b").join("file.bin");
        let disk = Disk {
            copy: copy_half_then_fail,
            ..OTHER_DRIVE
        };
        let e = move_unfinished(&old, &new, true, &disk).unwrap_err();
        assert!(e.contains("unplugged"), "{e}");
        assert_at(&old, &new);
    }

    #[test]
    fn a_drive_without_room_refuses_before_a_byte_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = dir.path().join("b").join("file.bin");
        let disk = Disk {
            free: full,
            ..OTHER_DRIVE
        };
        let e = move_unfinished(&old, &new, true, &disk).unwrap_err();
        assert!(e.contains("free space"), "{e}");
        assert_at(&old, &new);
    }

    #[test]
    fn an_original_that_cant_go_takes_the_copy_back() {
        fn in_use(p: &Path) -> io::Result<()> {
            if p.parent().is_some_and(|d| d.ends_with("a")) {
                return Err(io::Error::other("in use by another program"));
            }
            std::fs::remove_file(p)
        }
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = dir.path().join("b").join("file.bin");
        let disk = Disk {
            remove: in_use,
            ..OTHER_DRIVE
        };
        let e = move_unfinished(&old, &new, true, &disk).unwrap_err();
        assert!(e.contains("in use"), "{e}");
        assert_at(&old, &new);
    }

    #[test]
    fn a_name_taken_at_the_new_place_meanwhile_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let old = unfinished(&dir.path().join("a"), "file.bin");
        let new = unfinished(&dir.path().join("b"), "file.bin");
        assert!(move_unfinished(&old, &new, true, &Disk::REAL).is_err());
        assert_at(&old, &dir.path().join("a").join("other.bin"));
        assert_at(&new, &dir.path().join("b").join("other.bin"));
    }

    /// The name it gets in a folder that has one of its name: numbered on, as a new download's
    /// is, past a finished file and past another download's part file or resume state.
    #[test]
    fn a_name_taken_in_the_new_folder_is_numbered() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("b");
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("file.bin"), b"someone's").unwrap();
        std::fs::write(part_path(&to.join("file (1).bin")), b"another's").unwrap();
        std::fs::write(state_path(&to.join("file (2).bin")), b"{}").unwrap();
        let place = mdm_engine::naming::destination(&to, "file.bin");
        assert_eq!(place, to.join("file (3).bin"));
        let old = unfinished(&dir.path().join("a"), "file.bin");
        move_unfinished(&old, &place, true, &Disk::REAL).unwrap();
        assert_at(&place, &old);
        assert_eq!(std::fs::read(to.join("file.bin")).unwrap(), b"someone's");
    }

    #[test]
    fn a_folder_that_cant_be_reached_keeps_the_download_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("gone").join("file.bin");
        let new = dir.path().join("b").join("file.bin");
        let e = move_unfinished(&old, &new, true, &Disk::REAL).unwrap_err();
        assert!(e.contains("can't be reached"), "{e}");
        assert!(!new.parent().unwrap().exists());
    }

    /// Not started, or stopped before its resume state was written: nothing to carry, and the
    /// move only takes the new place.
    #[test]
    fn nothing_on_the_disk_yet_moves_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("a").join("file.bin");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        let new = dir.path().join("b").join("file.bin");
        move_unfinished(&old, &new, true, &Disk::REAL).unwrap();
        assert!(!part_path(&new).exists() && !state_path(&new).exists());
        // Not started, its folder not made yet (the engine makes it as it starts): it moves too.
        let never = dir.path().join("not yet").join("file.bin");
        let to = dir.path().join("c").join("file.bin");
        move_unfinished(&never, &to, false, &Disk::REAL).unwrap();
        assert!(to.parent().unwrap().is_dir());
    }

    /// MDM ended during a move (the list names both places): the next start finds where it
    /// is by the files, and clears the other place of them.
    #[test]
    fn a_move_cut_short_is_settled_by_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        // A copy cut short: the original stays, the half copy goes.
        let old = unfinished(&a, "copy.bin");
        let new = b.join("copy.bin");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(part_path(&new), &PART[..5]).unwrap();
        assert_eq!(settle(&old, &new, &Disk::REAL), old);
        assert_at(&old, &new);
        // Renamed there, its state not written yet: the part file comes back.
        let old = unfinished(&a, "rename.bin");
        let new = b.join("rename.bin");
        std::fs::rename(part_path(&old), part_path(&new)).unwrap();
        assert_eq!(settle(&old, &new, &Disk::REAL), old);
        assert_at(&old, &new);
        // Its state written there, the old files not deleted yet: the move is finished.
        let old = unfinished(&a, "done.bin");
        let new = b.join("done.bin");
        move_unfinished(&old, &new, true, &OTHER_DRIVE).unwrap();
        std::fs::write(part_path(&old), PART).unwrap();
        std::fs::write(state_path(&old), b"{}").unwrap();
        assert_eq!(settle(&old, &new, &Disk::REAL), new);
        assert_at(&new, &old);
        // All done before MDM ended (only the list's last save missing).
        assert_eq!(settle(&old, &new, &Disk::REAL), new);
        assert_at(&new, &old);
    }

    #[test]
    fn a_finished_file_moves_into_a_new_folder_and_to_another_drive() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("done.bin");
        std::fs::write(&from, PART).unwrap();
        let to = dir.path().join("x").join("y").join("done.bin");
        move_file(&from, &to, &OTHER_DRIVE).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), PART);
        assert!(!from.exists());
    }
}

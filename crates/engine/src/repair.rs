//! Downloading again the damaged parts of a finished file (#1661): an archive test found entries
//! whose bytes don't read back, and the format said where they are. The file becomes a part file
//! again, with a resume state whose segments hold every byte but those: resuming it (as any
//! stopped download is resumed) fetches only them, under the validators the file was downloaded
//! with. Its `If-Range`, the overlap check before each part and its digest apply as on any
//! resume, so a file that changed on the server is never mixed with the one on disk: the resume
//! fails (or starts over), as it would for a download stopped halfway.

use std::io;
use std::time::SystemTime;

use crate::naming;
use crate::planner::SavedSegment;
use crate::state::{ResumeState, state_path};

/// Bytes before each damaged part counted as on disk, so the worker that fetches it compares
/// them first (the resume overlap check, `Options::verify_overlap`'s default): a server without
/// validators that now sends another file is caught there.
pub const OVERLAP: u64 = 64;

/// The segments of a `size`-byte file whose bytes are all on disk but `bad` (`[start, end)`
/// ranges, in any order): each damaged part a segment of its own, starting up to `overlap` bytes
/// before it (bytes on disk, for the overlap check). None when nothing of the file is damaged.
pub fn segments_for(size: u64, bad: &[(u64, u64)], overlap: u64) -> Option<Vec<SavedSegment>> {
    let mut bad: Vec<(u64, u64)> = bad
        .iter()
        .map(|&(s, e)| (s.min(size), e.min(size)))
        .filter(|(s, e)| s < e)
        .collect();
    bad.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(bad.len());
    for (s, e) in bad {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    if merged.is_empty() {
        return None;
    }
    let mut segments = Vec::with_capacity(merged.len() * 2 + 1);
    let mut at = 0;
    for (s, e) in merged {
        let start = s.saturating_sub(overlap).max(at);
        if start > at {
            segments.push(SavedSegment {
                start: at,
                cursor: start,
                end: start,
            });
        }
        segments.push(SavedSegment {
            start,
            cursor: s,
            end: e,
        });
        at = e;
    }
    if at < size {
        segments.push(SavedSegment {
            start: at,
            cursor: size,
            end: size,
        });
    }
    Some(segments)
}

/// Make the finished file `state` names (`state.dest`, its kept state: `Snapshot::kept`) a part
/// file again whose `bad` ranges are still to download; resuming its download then fetches them.
/// Returns how many bytes that is. Refused, nothing changed, when the file isn't the one
/// downloaded any more (another size, or modified since `modified`), or a part file or state is
/// at its place already.
///
/// The state is written before the file is renamed: stopped in between (a crash, a power cut),
/// the file is still in place, whole, and a resume finds it so and completes.
pub fn reopen(
    state: &ResumeState,
    bad: &[(u64, u64)],
    modified: Option<SystemTime>,
) -> io::Result<u64> {
    let dest = &state.dest;
    let meta = std::fs::metadata(dest)?;
    let changed = || io::Error::new(io::ErrorKind::InvalidData, "the file changed since");
    if !meta.is_file() || meta.len() != state.size {
        return Err(changed());
    }
    if modified.is_some_and(|m| meta.modified().ok() != Some(m)) {
        return Err(changed());
    }
    let part = naming::part_path(dest);
    let saved = state_path(dest);
    if part.exists() || saved.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a part file or resume state is already there",
        ));
    }
    let segments = segments_for(state.size, bad, OVERLAP).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "no damaged range in the file")
    })?;
    let mut reopened = state.clone();
    reopened.segments = segments;
    let missing = reopened.size - reopened.downloaded();
    reopened.write(&saved)?;
    // Antivirus scanners and indexers briefly lock files on Windows: retried, as the final
    // rename is.
    let renamed = craft_atomic::retry_rename(craft_atomic::RenameRetry::platform(), || {
        std::fs::rename(dest, &part)
    });
    if let Err(e) = renamed {
        let _ = std::fs::remove_file(&saved);
        return Err(e);
    }
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::Planner;
    use crate::state::Validators;
    use std::path::Path;

    fn seg(start: u64, cursor: u64, end: u64) -> SavedSegment {
        SavedSegment { start, cursor, end }
    }

    #[test]
    fn damaged_parts_become_segments_to_fetch() {
        // One part in the middle: on disk before it (64 bytes of them in its segment), after it.
        assert_eq!(
            segments_for(1000, &[(400, 500)], 64).unwrap(),
            [seg(0, 336, 336), seg(336, 400, 500), seg(500, 1000, 1000)]
        );
        // At the start, at the end, touching ones merged, out of order, past the end cut.
        assert_eq!(
            segments_for(1000, &[(900, 2000), (0, 10), (10, 20)], 64).unwrap(),
            [seg(0, 0, 20), seg(20, 836, 836), seg(836, 900, 1000)]
        );
        // Close together: the overlap of the second stays inside what is on disk between.
        assert_eq!(
            segments_for(1000, &[(100, 200), (230, 300)], 64).unwrap(),
            [
                seg(0, 36, 36),
                seg(36, 100, 200),
                seg(200, 230, 300),
                seg(300, 1000, 1000)
            ]
        );
        assert!(segments_for(1000, &[], 64).is_none());
        assert!(segments_for(1000, &[(5, 5), (2000, 3000)], 64).is_none());
        // The planner takes them, and has only the damaged bytes left.
        for bad in [vec![(400, 500)], vec![(0, 1000)], vec![(0, 1), (999, 1000)]] {
            let s = segments_for(1000, &bad, 64).unwrap();
            let p = Planner::resume(1000, 1, &s).unwrap();
            let missing: u64 = bad.iter().map(|(a, b)| b - a).sum();
            assert_eq!(1000 - p.downloaded(), missing, "{bad:?}");
        }
    }

    fn state_of(dest: &Path, size: u64) -> ResumeState {
        let url: url::Url = "http://127.0.0.1/a.zip".parse().unwrap();
        let mut s = ResumeState::new(
            url.clone(),
            url,
            dest.to_owned(),
            size,
            Validators::default(),
        );
        s.segments = vec![seg(0, size, size)];
        s
    }

    #[test]
    fn a_finished_file_reopens_with_only_its_damage_to_fetch() {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("a.zip");
        std::fs::write(&dest, vec![7u8; 1000]).unwrap();
        let state = state_of(&dest, 1000);
        let modified = std::fs::metadata(&dest).unwrap().modified().ok();
        assert_eq!(reopen(&state, &[(400, 500)], modified).unwrap(), 100);
        assert!(!dest.exists());
        let part = naming::part_path(&dest);
        assert_eq!(std::fs::read(&part).unwrap(), vec![7u8; 1000], "untouched");
        let saved = ResumeState::load(&state_path(&dest)).unwrap();
        assert_eq!(saved.downloaded(), 900);
        assert_eq!(saved.validators, state.validators);
        // Again: a part file is there now, nothing changes.
        std::fs::write(&dest, vec![7u8; 1000]).unwrap();
        assert_eq!(
            reopen(&state, &[(0, 10)], None).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(dest.exists() && part.exists());
    }

    #[test]
    fn a_file_changed_since_is_left_alone() {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("a.zip");
        std::fs::write(&dest, vec![7u8; 999]).unwrap();
        let state = state_of(&dest, 1000);
        // Another size.
        assert_eq!(
            reopen(&state, &[(0, 10)], None).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        // The same size, modified since it was tested.
        std::fs::write(&dest, vec![7u8; 1000]).unwrap();
        let then = SystemTime::UNIX_EPOCH;
        assert_eq!(
            reopen(&state, &[(0, 10)], Some(then)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        // Gone.
        std::fs::remove_file(&dest).unwrap();
        assert!(reopen(&state, &[(0, 10)], None).is_err());
        for p in [naming::part_path(&dest), state_path(&dest)] {
            assert!(!p.exists(), "{}", p.display());
        }
    }
}

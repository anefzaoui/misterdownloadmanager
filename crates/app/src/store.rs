//! The download list on disk: `%APPDATA%\misterdownloadmanager\downloads.json`, written with craft-atomic.
//! Also holds the list's column layout (order, widths, visibility, sort).
//!
//! Generations: `downloads.json.bak` is the list before the last save; `.1.bak` and `.2.bak`
//! are older, rotated at most every `GENERATION_EVERY` and only when the list changed. Loading
//! takes the newest that parses. A damaged list is kept as `.corrupt-<unix time>.bak`, and a
//! list that exists but can't be read is never overwritten.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use mdm_engine::state::{ResumeState, state_path};
use serde::{Deserialize, Serialize};

use crate::list::Layout;
use crate::model::{Item, Status};
use crate::queue::Queue;
use crate::settings::Settings;

#[derive(Deserialize, Default)]
struct ListFile {
    next_id: u64,
    items: Vec<Item>,
    #[serde(default)]
    columns: Option<Layout>,
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    queues: Vec<Queue>,
    /// The list format: 2 since queues default to 4 files at once (D-012).
    #[serde(default)]
    version: u32,
}

/// The list format written now.
const VERSION: u32 = 2;

pub struct Loaded {
    pub next_id: u64,
    pub items: Vec<Item>,
    pub columns: Option<Layout>,
    pub settings: Settings,
    pub queues: Vec<Queue>,
}

pub fn path() -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join("downloads.json"))
}

/// Load the list; a missing file gives an empty list, a damaged one is kept aside.
pub fn load() -> Loaded {
    let empty = Loaded {
        next_id: 1,
        items: Vec::new(),
        columns: None,
        settings: Settings::default(),
        queues: Vec::new(),
    };
    let Some(p) = path() else { return empty };
    load_from(&p, empty)
}

/// How often an older generation is kept.
const GENERATION_EVERY: Duration = Duration::from_secs(10 * 60);

/// Lists that exist but couldn't be read (permissions, a lock): never saved over.
fn blocked() -> &'static Mutex<HashSet<PathBuf>> {
    static BLOCKED: std::sync::OnceLock<Mutex<HashSet<PathBuf>>> = std::sync::OnceLock::new();
    BLOCKED.get_or_init(Default::default)
}

fn load_from(p: &Path, empty: Loaded) -> Loaded {
    let p = p.to_owned();
    let parsed = match std::fs::read(&p) {
        Ok(bytes) => parse_bytes(&bytes).or_else(|| {
            // Damaged: keep it aside (never overwrite an earlier one) and use a generation.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let _ = std::fs::rename(&p, p.with_extension(format!("json.corrupt-{stamp}.bak")));
            newest_generation(&p)
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => newest_generation(&p),
        // There but unreadable: work from a generation, and don't save over the real list.
        Err(_) => {
            blocked().lock().unwrap().insert(p.clone());
            newest_generation(&p)
        }
    };
    match parsed {
        Some(mut f) => {
            for i in &mut f.items {
                // Downloads that were running when the app closed are now stopped (resumable).
                if i.status == Status::Downloading {
                    i.status = Status::Stopped;
                }
                // Live progress isn't written to this file; the engine's resume state is the
                // record of what is on disk.
                if i.status != Status::Complete {
                    // Started under the former codename: rename its files so it resumes.
                    mdm_engine::state::migrate_legacy(&i.dest);
                }
                if i.status != Status::Complete
                    && let Ok(st) = ResumeState::load(&state_path(&i.dest))
                {
                    i.downloaded = st.downloaded();
                    i.size = Some(st.size);
                }
            }
            if f.version < 2 {
                // Before version 2 nothing could set "files at the same time" (no Scheduler),
                // so every 1 there is the old default: IDM's (and our) default is 4.
                for q in f.queues.iter_mut().filter(|q| q.files_at_once == 1) {
                    q.files_at_once = 4;
                }
            }
            Loaded {
                next_id: f.next_id.max(1),
                items: f.items,
                columns: f.columns,
                settings: f.settings,
                queues: f.queues,
            }
        }
        None => empty,
    }
}

/// The list file at `p`, if it reads and parses.
fn parse(p: &Path) -> Option<ListFile> {
    parse_bytes(&std::fs::read(p).ok()?)
}

/// A UTF-8 byte-order mark is tolerated (editors and PowerShell add one).
fn parse_bytes(bytes: &[u8]) -> Option<ListFile> {
    let json = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice::<ListFile>(json).ok()
}

/// The list before the last save.
fn backup_path(p: &Path) -> PathBuf {
    p.with_extension("json.bak")
}

/// Older generations: `.1.bak` (at least `GENERATION_EVERY` old when replaced), `.2.bak`.
fn generation_path(p: &Path, n: u32) -> PathBuf {
    p.with_extension(format!("json.{n}.bak"))
}

fn newest_generation(p: &Path) -> Option<ListFile> {
    [backup_path(p), generation_path(p, 1), generation_path(p, 2)]
        .iter()
        .find_map(|g| parse(g))
}

/// Before a save: the current list becomes `.bak`, and every `GENERATION_EVERY` (when it
/// changed) also `.1.bak`, the old `.1.bak` moving to `.2.bak`. Only a list that parses is kept:
/// a damaged file never replaces a good generation.
fn keep_generations(p: &Path) {
    let Ok(current) = std::fs::read(p) else {
        return;
    };
    if parse_bytes(&current).is_none() {
        return;
    }
    let _ = std::fs::write(backup_path(p), &current);
    let (g1, g2) = (generation_path(p, 1), generation_path(p, 2));
    let g1_age = std::fs::metadata(&g1)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok());
    let due = match g1_age {
        None => true,
        Some(age) => age >= GENERATION_EVERY && std::fs::read(&g1).ok().as_ref() != Some(&current),
    };
    if due {
        if g1.exists() {
            let _ = std::fs::rename(&g1, &g2);
        }
        let _ = std::fs::write(&g1, &current);
    }
}

pub fn save(
    next_id: u64,
    items: &[Item],
    columns: &Layout,
    settings: &Settings,
    queues: &[Queue],
) -> std::io::Result<()> {
    let Some(p) = path() else { return Ok(()) };
    save_to(&p, next_id, items, columns, settings, queues)
}

fn save_to(
    p: &Path,
    next_id: u64,
    items: &[Item],
    columns: &Layout,
    settings: &Settings,
    queues: &[Queue],
) -> std::io::Result<()> {
    let p = p.to_owned();
    if blocked().lock().unwrap().contains(&p) {
        return Err(std::io::Error::other(
            "the download list could not be read at start; not overwriting it",
        ));
    }
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[derive(Serialize)]
    struct ListRef<'a> {
        version: u32,
        next_id: u64,
        items: &'a [Item],
        columns: &'a Layout,
        settings: &'a Settings,
        queues: &'a [Queue],
    }
    let json = serde_json::to_vec_pretty(&ListRef {
        version: VERSION,
        next_id,
        items,
        columns,
        settings,
        queues,
    })
    .map_err(std::io::Error::other)?;
    keep_generations(&p);
    craft_atomic::atomic_write(&p, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Loaded {
        Loaded {
            next_id: 1,
            items: Vec::new(),
            columns: None,
            settings: Settings::default(),
            queues: Vec::new(),
        }
    }

    #[test]
    fn a_damaged_list_falls_back_to_the_previous_generation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("downloads.json");
        let layout = crate::list::ListState::default().layout();
        save_to(&p, 7, &[], &layout, &Settings::default(), &[]).unwrap();
        save_to(&p, 9, &[], &layout, &Settings::default(), &[]).unwrap();
        assert_eq!(load_from(&p, empty()).next_id, 9);
        std::fs::write(&p, b"{ not json").unwrap();
        let back = load_from(&p, empty());
        assert_eq!(
            back.next_id, 7,
            "the backup holds the generation before the last save"
        );
        let kept = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(kept, "damaged file kept aside");
    }

    fn save_n(p: &Path, n: u64) {
        let layout = crate::list::ListState::default().layout();
        save_to(p, n, &[], &layout, &Settings::default(), &[]).unwrap();
    }

    /// Pretend `.1.bak` was written `age` ago.
    fn age_generation(p: &Path, age: Duration) {
        let f = std::fs::File::options()
            .write(true)
            .open(generation_path(p, 1))
            .unwrap();
        f.set_modified(std::time::SystemTime::now() - age).unwrap();
    }

    #[test]
    fn older_generations_rotate_at_most_every_ten_minutes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("downloads.json");
        save_n(&p, 1);
        save_n(&p, 2); // .1 = list 1
        save_n(&p, 3); // too soon: .1 stays list 1
        assert_eq!(parse(&generation_path(&p, 1)).unwrap().next_id, 1);
        age_generation(&p, Duration::from_secs(11 * 60));
        save_n(&p, 4); // .1 = list 3, .2 = list 1
        assert_eq!(parse(&generation_path(&p, 1)).unwrap().next_id, 3);
        assert_eq!(parse(&generation_path(&p, 2)).unwrap().next_id, 1);
        assert_eq!(parse(&backup_path(&p)).unwrap().next_id, 3);
    }

    #[test]
    fn loading_falls_back_through_the_generations() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("downloads.json");
        save_n(&p, 1);
        save_n(&p, 2);
        std::fs::write(&p, b"broken").unwrap();
        std::fs::write(backup_path(&p), b"broken too").unwrap();
        assert_eq!(load_from(&p, empty()).next_id, 1, "from .1.bak");
    }

    #[test]
    fn old_lists_get_four_files_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("downloads.json");
        let old = r#"{"next_id":3,"items":[],"queues":[{"name":"Main download queue",
            "files_at_once":1,"items":[],"sync":false}]}"#;
        std::fs::write(&p, old).unwrap();
        assert_eq!(load_from(&p, empty()).queues[0].files_at_once, 4);
        // Written as version 2, a 1 chosen in the Scheduler stays.
        let mut q = Queue::new("Main download queue", false);
        q.files_at_once = 1;
        let layout = crate::list::ListState::default().layout();
        save_to(&p, 3, &[], &layout, &Settings::default(), &[q]).unwrap();
        assert_eq!(load_from(&p, empty()).queues[0].files_at_once, 1);
    }

    #[test]
    fn an_unreadable_list_is_never_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: it exists, but reading it fails.
        let p = dir.path().join("downloads.json");
        std::fs::create_dir(&p).unwrap();
        let _ = load_from(&p, empty());
        let layout = crate::list::ListState::default().layout();
        assert!(save_to(&p, 5, &[], &layout, &Settings::default(), &[]).is_err());
        assert!(p.is_dir());
    }

    #[test]
    fn a_damaged_file_never_replaces_a_good_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("downloads.json");
        let layout = crate::list::ListState::default().layout();
        save_to(&p, 3, &[], &layout, &Settings::default(), &[]).unwrap();
        save_to(&p, 4, &[], &layout, &Settings::default(), &[]).unwrap();
        std::fs::write(&p, b"garbage").unwrap();
        save_to(&p, 5, &[], &layout, &Settings::default(), &[]).unwrap();
        assert_eq!(parse(&backup_path(&p)).unwrap().next_id, 3);
    }
}

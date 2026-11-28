//! Testing archives when their download completes (#1661): a ZIP, 7z or RAR (a split one once
//! every volume is in) is read back whole and checked before its download counts as done,
//! with Options → Archives → "Test archives when their download completes" (on, up to a size
//! there, 4 GB at first: a test reads the whole file); right-click → Test archive tests one by
//! hand, whatever its size. One archive at a time, on a thread of its own (mdm-extract's `check`
//! does the work, through the 7-Zip MDM extracts with, a ZIP in process without it); the row says
//! "Checking 45%" meanwhile, its queue takes it as still running, and its Download complete, its
//! sound and its extraction wait for the outcome.
//!
//! A damaged archive says so in its row ("Damaged") and its log, and a small window offers to
//! download it again (Redownload). A ZIP in one file whose damaged entries MDM's reader places
//! (`check::Damage::ranges`) has those bytes downloaded again instead, from the resume state its
//! download just kept (`mdm_engine::repair`: the same validators, so never another version of
//! the file), and is tested once more; still damaged then, it is offered for Redownload. 7z and
//! RAR (solid blocks, which 7-Zip's output doesn't place) and split archives are tested and told
//! only.
//!
//! A split archive (`x.zip.001`, `x.z01`, `x.7z.001`, `x.part1.rar`, `x.r00`) is tested once,
//! from its first volume, when the last of its volumes to finish is in: every volume on disk, the
//! last saying it is the last (`mdm_extract::on_disk`), and none still a download running,
//! queued or stopped in the list. Its volumes often finish one after another (a queue taking one
//! at a time, a host's wait between them), and the ones in so far read as an archive cut short:
//! never damage. 7-Zip's own "missing volume" and "unexpected end" on a split archive say the
//! same (not complete yet), and Test archive on one says it isn't complete.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use mdm_engine::state::ResumeState;
use mdm_extract::{Error, Verdict, volume_key};

use crate::dialogs::{Outcome, window};
use crate::model::{Item, Status};
use crate::settings::Settings;
use crate::winui::{self, ButtonKind, LIGHT_BG, LIGHT_TEXT};

/// Is this file name a ZIP's, 7z's or RAR's (or one volume of one)? The archives tested; by name
/// only, its first bytes decide once it is tested.
pub fn tested_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    volume_key(&lower).is_some()
        || mdm_extract::spanned_part(&lower)
        || [".zip", ".7z", ".rar"]
            .iter()
            .any(|e| lower.ends_with(e) && lower.len() > e.len())
}

/// Whether `item` is a finished file download whose archive can be tested.
pub fn testable(item: &Item) -> bool {
    item.status == Status::Complete
        && item.torrent.is_none()
        && item.video.is_none()
        && item.gallery.is_none()
        && item.remote.is_none()
        && item.zip.is_none()
        && tested_name(&item.file_name)
}

/// One archive to test.
#[derive(Clone)]
struct Job {
    /// The download whose completion waits for the test (none: tested by hand, or again at start
    /// after MDM closed during its test).
    held: Option<u64>,
    /// The row that says "Checking..." meanwhile.
    shown: u64,
    /// Its volumes' downloads, the first volume's first.
    ids: Vec<u64>,
    /// The first volume (what is tested).
    archive: PathBuf,
    by_hand: bool,
    /// Its damaged parts were just downloaded again: the test once more, never a second re-fetch.
    again: bool,
    /// Its download's kept resume state (an archive in one file): what a re-fetch goes on from.
    kept: Option<Arc<ResumeState>>,
}

/// A test's outcome, from its thread: the verdict, and the file's modification time as it was
/// tested (a re-fetch is refused if the file changed since).
type Slot = Arc<Mutex<Option<(Result<Verdict, Error>, Option<SystemTime>)>>>;

struct Running {
    job: Job,
    result: Slot,
    cancel: Arc<AtomicBool>,
}

/// What the app does with a test's outcome.
#[derive(Debug, PartialEq)]
pub enum Done {
    /// Download `id` counts as done now: its finishing goes on (Download complete, its sound, its
    /// extraction).
    Release(u64),
    /// Download `ranges` of `id`'s file again from `state` (`mdm_engine::repair::reopen`), then
    /// test it once more. `modified`: the file as tested.
    Refetch {
        id: u64,
        state: Arc<ResumeState>,
        ranges: Vec<(u64, u64)>,
        modified: Option<SystemTime>,
    },
}

#[derive(Default)]
pub struct Tests {
    queue: VecDeque<Job>,
    running: Option<Running>,
    /// Downloads whose archive is being looked at beside the window's thread ([`look_at`]).
    looks: Vec<Look>,
    /// The window to wake once a look is done ([`Tests::look_beside`]); None: the disk is looked
    /// at where it is asked (the tests).
    wake: Option<egui::Context>,
    /// A damaged archive, offered to download again: one window at a time (the others are said
    /// in their logs).
    pub damaged: Option<DamagedDialog>,
    /// A test by hand that ended without damage: its outcome in a message box (title, text).
    pub told: Option<String>,
}

impl Tests {
    /// From now on the disk is looked at beside the window's thread (#1292), and `ctx` woken
    /// when it was: whether an archive is complete opens it (its end record), and an antivirus
    /// checking a file just downloaded can hold that open for seconds.
    pub fn look_beside(&mut self, ctx: &egui::Context) {
        if self.wake.is_none() {
            self.wake = Some(ctx.clone());
        }
    }

    /// Look at `id`'s archive on the disk for `why`: beside the window's thread once
    /// [`Tests::look_beside`] said so (what it found comes to [`Tests::poll`]), else now.
    fn look(&mut self, id: u64, dest: &Path, why: Why) -> Option<Looked> {
        let Some(ctx) = self.wake.clone() else {
            return Some(look_at(dest));
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let dest = dest.to_path_buf();
        crate::launch::spawn("archive look", move || {
            let _ = tx.send(look_at(&dest));
            ctx.request_repaint();
        });
        self.looks.push(Look { id, why, rx });
        None
    }

    /// Download `id` just finished: whether it waits for its archive's test (true: its finishing
    /// comes with [`Done::Release`]). Tested when Options says so and it is no larger than its
    /// limit, a split archive once every volume of it is in ([`ready`]); always when its damaged
    /// parts were just downloaded again. While its archive is looked at beside the window's
    /// thread it waits too, and is released by [`Tests::poll`] if it isn't tested.
    pub fn hold(&mut self, id: u64, items: &mut [Item], settings: &Settings) -> bool {
        let Some(item) = items.iter().find(|i| i.id == id) else {
            return false;
        };
        if !testable(item) {
            return false;
        }
        let again = item.refetched;
        if !settings.test_archives && !again {
            return false;
        }
        // Already looked at: it waits for that look.
        if self.looks.iter().any(|l| l.id == id) {
            return true;
        }
        let dest = item.dest.clone();
        match self.look(id, &dest, Why::Hold) {
            Some(looked) => self.hold_looked(id, items, settings, looked),
            None => true,
        }
    }

    /// [`Tests::hold`], with what the disk says of its archive.
    fn hold_looked(
        &mut self,
        id: u64,
        items: &mut [Item],
        settings: &Settings,
        looked: Looked,
    ) -> bool {
        let Some(item) = items.iter().find(|i| i.id == id) else {
            return false;
        };
        let again = item.refetched;
        // A split archive once every volume is in (#1661): before, its end is missing, which
        // a test reads as damage. Its last volume to finish tests it.
        let (archive, files, ids) = match ready_from(item, items, looked.disk) {
            Ready::Yes {
                archive,
                files,
                ids,
            } => (archive, files, ids),
            not => {
                tracing::info!("archive test: download {id}: not yet: {not:?}");
                return false;
            }
        };
        if self.waiting(&archive) {
            return false;
        }
        // Every volume's, the ones not downloaded by MDM too (the list's size when a file
        // doesn't say).
        let size: u64 = files
            .iter()
            .zip(looked.sizes.iter().copied().chain(std::iter::repeat(None)))
            .filter_map(|(f, on_disk)| {
                on_disk.or_else(|| items.iter().find(|i| &i.dest == f)?.size)
            })
            .sum();
        let limit = settings.test_limit();
        if !again && size > limit {
            if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                i.note(crate::i18n::fmt(
                    // i18n: in a download's log: an archive larger than Options → Archives' limit for its test wasn't tested; {limit} like "4.00 GB"; "Test archive" is the right-click command
                    crate::i18n::t(
                        "Not tested: it is larger than {limit} (Options → Archives). Right-click → Test archive tests it",
                    ),
                    &[("limit", &crate::model::size(limit))],
                ));
            }
            return false;
        }
        let kept = (files.len() == 1)
            .then(|| item.snap.as_ref().and_then(|s| s.kept.clone()))
            .flatten();
        let job = Job {
            held: Some(id),
            shown: id,
            ids,
            archive,
            by_hand: false,
            again,
            kept,
        };
        self.push(job, items);
        true
    }

    /// Right-click → Test archive: test `item`'s archive (a split one from whichever volume),
    /// whatever Options says. Its download is done already: nothing waits for it.
    pub fn by_hand(&mut self, id: u64, items: &mut [Item]) {
        let Some(item) = items.iter().find(|i| i.id == id) else {
            return;
        };
        if !testable(item) || item.testing || self.looks.iter().any(|l| l.id == id) {
            return;
        }
        let dest = item.dest.clone();
        if let Some(looked) = self.look(id, &dest, Why::ByHand) {
            self.by_hand_looked(id, items, looked);
        }
    }

    /// [`Tests::by_hand`], with what the disk says of its archive.
    fn by_hand_looked(&mut self, id: u64, items: &mut [Item], looked: Looked) {
        let Some(item) = items.iter().find(|i| i.id == id) else {
            return;
        };
        let (archive, files, ids) = match ready_from(item, items, looked.disk) {
            Ready::Yes {
                archive,
                files,
                ids,
            } => (archive, files, ids),
            // Not complete yet: said so, never damaged.
            not => {
                let name = item.file_name.clone();
                let text = match not {
                    Ready::Unreadable(e) => not_tested(&name, &Error::Io(e)),
                    not => incomplete_text(&name, &not),
                };
                if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                    i.note(text.clone());
                }
                self.told = Some(text);
                return;
            }
        };
        if self.waiting(&archive) {
            return;
        }
        let kept = (files.len() == 1)
            .then(|| item.snap.as_ref().and_then(|s| s.kept.clone()))
            .flatten();
        let job = Job {
            held: None,
            shown: id,
            ids,
            archive,
            by_hand: true,
            again: item.refetched,
            kept,
        };
        self.push(job, items);
    }

    /// At start: the downloads whose test MDM's closing stopped are tested again (their rows
    /// say "Checking..." until then). Options turned off since: they stay as they are.
    pub fn resume(&mut self, items: &mut [Item], settings: &Settings) {
        let ids: Vec<u64> = items.iter().filter(|i| i.testing).map(|i| i.id).collect();
        for id in ids {
            let Some(item) = items.iter().find(|i| i.id == id) else {
                continue;
            };
            if !(settings.test_archives && testable(item)) {
                if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                    i.testing = false;
                }
                continue;
            }
            let dest = item.dest.clone();
            if let Some(looked) = self.look(id, &dest, Why::Resume) {
                self.resume_looked(id, items, looked);
            }
        }
    }

    /// [`Tests::resume`] for `id`, with what the disk says of its archive.
    fn resume_looked(&mut self, id: u64, items: &mut [Item], looked: Looked) {
        let Some(item) = items.iter().find(|i| i.id == id) else {
            return;
        };
        let set = match ready_from(item, items, looked.disk) {
            Ready::Yes { archive, ids, .. } => Some((archive, ids)),
            _ => None,
        };
        let Some((archive, ids)) = set.filter(|(a, _)| !self.waiting(a)) else {
            if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                i.testing = false;
            }
            return;
        };
        let job = Job {
            held: None,
            shown: id,
            ids,
            archive,
            by_hand: false,
            again: false,
            kept: None,
        };
        self.push(job, items);
    }

    fn push(&mut self, job: Job, items: &mut [Item]) {
        if let Some(i) = items.iter_mut().find(|i| i.id == job.shown) {
            i.testing = true;
            i.test_progress = None;
        }
        self.queue.push_back(job);
    }

    /// Is `archive` queued or being tested?
    fn waiting(&self, archive: &Path) -> bool {
        self.queue.iter().any(|j| j.archive == archive)
            || self
                .running
                .as_ref()
                .is_some_and(|r| r.job.archive == archive)
    }

    /// Stop testing the archive of any of `ids` (they are being deleted, or downloaded again),
    /// and forget the ones queued.
    pub fn forget(&mut self, ids: &[u64]) {
        self.looks.retain(|l| !ids.contains(&l.id));
        if let Some(r) = &self.running
            && r.job.ids.iter().any(|i| ids.contains(i))
        {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.queue
            .retain(|j| !j.ids.iter().any(|i| ids.contains(i)));
        if self
            .damaged
            .as_ref()
            .is_some_and(|d| d.ids.iter().any(|i| ids.contains(i)))
        {
            self.damaged = None;
        }
    }

    /// Closing: the test running stops (its download is tested again at the next start).
    pub fn stop(&mut self) {
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.queue.clear();
        self.looks.clear();
    }

    /// The looks at the disk done beside the window's thread, taken where they were asked:
    /// a download held for one is tested, or released (`out`).
    fn looked(&mut self, items: &mut [Item], settings: &Settings, out: &mut Vec<Done>) {
        let mut done = Vec::new();
        self.looks.retain(|l| match l.rx.try_recv() {
            Ok(looked) => {
                done.push((l.id, l.why, looked));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                done.push((l.id, l.why, Looked::unread()));
                false
            }
        });
        for (id, why, looked) in done {
            match why {
                Why::Hold => {
                    if items.iter().any(|i| i.id == id)
                        && !self.hold_looked(id, items, settings, looked)
                    {
                        out.push(Done::Release(id));
                    }
                }
                Why::ByHand => self.by_hand_looked(id, items, looked),
                Why::Resume => self.resume_looked(id, items, looked),
            }
        }
    }

    /// A test done: its outcome into the logs, and what the app does next; the next one
    /// started.
    pub fn poll(
        &mut self,
        items: &mut [Item],
        settings: &Settings,
        ctx: &egui::Context,
    ) -> Vec<Done> {
        let mut out = Vec::new();
        self.looked(items, settings, &mut out);
        let done = self
            .running
            .as_ref()
            .and_then(|r| r.result.lock().unwrap().take());
        if let Some((result, modified)) = done
            && let Some(run) = self.running.take()
        {
            out.extend(self.finish(run.job, result, modified, items));
        }
        if self.running.is_none() {
            // A job whose downloads were deleted meanwhile is gone with them (`forget`).
            while let Some(job) = self.queue.pop_front() {
                if !items.iter().any(|i| i.id == job.shown) {
                    continue;
                }
                if let Some(i) = items.iter_mut().find(|i| i.id == job.shown) {
                    // i18n: in a download's log: its archive is being tested (read back and checked) before the download counts as done
                    i.note(crate::i18n::t("Testing the archive..."));
                    let progress = Arc::new(AtomicU32::new(0));
                    i.test_progress = Some(progress.clone());
                    self.running = Some(start(job, progress, ctx));
                }
                break;
            }
        }
        if self.running.is_some() {
            // The row's figure as the test reads.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        out
    }

    fn finish(
        &mut self,
        job: Job,
        result: Result<Verdict, Error>,
        modified: Option<SystemTime>,
        items: &mut [Item],
    ) -> Vec<Done> {
        let name = job
            .archive
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(i) = items.iter_mut().find(|i| i.id == job.shown) {
            i.test_progress = None;
        }
        let note = |items: &mut [Item], text: &str| {
            for i in items.iter_mut().filter(|i| job.ids.contains(&i.id)) {
                i.note(text.to_owned());
            }
        };
        let release = |items: &mut [Item]| {
            if let Some(i) = items.iter_mut().find(|i| i.id == job.shown) {
                i.testing = false;
                i.refetched = false;
            }
            job.held.map(Done::Release).into_iter().collect::<Vec<_>>()
        };
        let damage = match result {
            Ok(Verdict::Whole) => {
                let text = if job.again {
                    // i18n: in a download's log: the archive's damaged parts were downloaded again, and its test now finds no error
                    crate::i18n::t("Tested again: the archive has no errors now")
                } else {
                    // i18n: in a download's log: the archive was read back whole and checked
                    crate::i18n::t("The archive test found no errors")
                };
                note(items, text);
                for i in items.iter_mut().filter(|i| job.ids.contains(&i.id)) {
                    i.damaged = false;
                }
                if job.by_hand {
                    self.told = Some(format_told(&name, text));
                }
                return release(items);
            }
            Ok(Verdict::Damaged(d)) => d,
            // Deleted, or MDM is closing: nothing to say (at the next start it is tested again).
            Err(Error::Cancelled) => return Vec::new(),
            // A split archive a volume of which is still to come (7-Zip found its end missing):
            // not complete yet, never damage.
            Err(Error::MissingVolume(n)) => {
                let text = incomplete_text(&name, &Ready::Missing(n));
                note(items, &text);
                if job.by_hand {
                    self.told = Some(text);
                }
                return release(items);
            }
            // Its bytes are no ZIP, 7z or RAR, as its name says: it is damaged (a web page
            // saved in its place, cut short), nowhere to place it.
            Err(Error::Unsupported) => mdm_extract::Damage {
                said: "not a ZIP, 7z or RAR archive".into(),
                ..Default::default()
            },
            Err(e) => {
                let text = not_tested(&name, &e);
                note(items, &text);
                if job.by_hand {
                    self.told = Some(text);
                }
                return release(items);
            }
        };
        tracing::info!(
            "archive test: {name}: damaged ({} entries; {}): {}",
            damage.entries.len(),
            if damage.ranges.is_some() {
                "placed"
            } else {
                "not placed"
            },
            damage.said
        );
        let text = damaged_text(&damage.entries);
        // A ZIP in one file whose damage is placed, once: those bytes again.
        if !job.again
            && let (Some(ranges), Some(state)) = (damage.ranges.clone(), job.kept.clone())
            && job.ids.len() == 1
        {
            let bytes: u64 = ranges.iter().map(|(s, e)| e - s).sum();
            note(items, &text);
            note(
                items,
                &crate::i18n::fmt(
                    // i18n: in a download's log: after its archive test found damaged files, only their bytes are downloaded again from the same server; {size} like "1.20 MB"
                    crate::i18n::t("Downloading the damaged parts again ({size})"),
                    &[("size", &crate::model::size(bytes))],
                ),
            );
            return vec![Done::Refetch {
                id: job.shown,
                state,
                ranges,
                modified,
            }];
        }
        note(items, &text);
        for i in items.iter_mut().filter(|i| job.ids.contains(&i.id)) {
            i.damaged = true;
        }
        if self.damaged.is_none() {
            self.damaged = Some(DamagedDialog {
                ids: job.ids.clone(),
                name,
            });
        }
        release(items)
    }

    /// Re-fetching the damaged parts couldn't start (`why`, in the interface language): the
    /// archive is damaged, offered for Redownload, and its download counts as done.
    pub fn refetch_failed(&mut self, id: u64, why: &str, items: &mut [Item]) -> Option<u64> {
        let item = items.iter_mut().find(|i| i.id == id)?;
        item.note(crate::i18n::fmt(
            // i18n: in a download's log: the bytes of the archive's damaged files couldn't be downloaded again; {error} why
            crate::i18n::t("The damaged parts couldn't be downloaded again: {error}"),
            &[("error", why)],
        ));
        item.damaged = true;
        item.testing = false;
        item.refetched = false;
        if self.damaged.is_none() {
            self.damaged = Some(DamagedDialog {
                ids: vec![id],
                name: item.file_name.clone(),
            });
        }
        Some(id)
    }
}

/// Why an archive is looked at on the disk ([`Tests::look`]).
#[derive(Clone, Copy, Debug)]
enum Why {
    /// Its download just finished ([`Tests::hold`]).
    Hold,
    /// Right-click → Test archive ([`Tests::by_hand`]).
    ByHand,
    /// Its test stopped when MDM closed ([`Tests::resume`]).
    Resume,
}

/// A look at the disk on its way ([`Tests::look`]).
struct Look {
    id: u64,
    why: Why,
    rx: std::sync::mpsc::Receiver<Looked>,
}

/// What the disk says of an archive: whether every volume is there ([`mdm_extract::on_disk`])
/// and, when it is, each volume's size (None where it didn't say).
struct Looked {
    disk: std::io::Result<mdm_extract::OnDisk>,
    sizes: Vec<Option<u64>>,
}

impl Looked {
    /// A look whose thread ended without an answer.
    fn unread() -> Looked {
        Looked {
            disk: Err(std::io::Error::other(
                "the look at the disk ended without an answer",
            )),
            sizes: Vec::new(),
        }
    }
}

/// Look at the archive at `dest` (it opens the file: its end record, a split one's volumes).
fn look_at(dest: &Path) -> Looked {
    let disk = mdm_extract::on_disk(dest);
    let sizes = match &disk {
        Ok(mdm_extract::OnDisk::Complete { files, .. }) => files
            .iter()
            .map(|f| std::fs::metadata(f).map(|m| m.len()).ok())
            .collect(),
        _ => Vec::new(),
    };
    Looked { disk, sizes }
}

/// Whether `item`'s archive (every volume of a split one) can be tested now (#1661).
#[derive(Debug)]
enum Ready {
    /// Every volume is on disk and none is still to download: the file to test (its first
    /// volume), every volume's file, and their downloads in the list (the first volume's first).
    Yes {
        archive: PathBuf,
        files: Vec<PathBuf>,
        ids: Vec<u64>,
    },
    /// A volume of it is a download still running, queued or stopped in the list.
    Downloading,
    /// Volume `n` of it (0: one, which isn't known) isn't on disk: more is to come.
    Missing(u32),
    /// Its folder or its files didn't read.
    Unreadable(std::io::Error),
}

/// Is `item`'s archive complete? The list and the disk both say: no volume of it is a download
/// still to finish (same folder, a name of the same set), and every volume is on disk, the last
/// saying it is the last (`mdm_extract::on_disk`). A split archive's volumes finish one after
/// another (a queue taking one at a time, a host's wait between them, some added later): the
/// volumes finished so far are no complete archive, whatever the list holds yet.
#[cfg(test)]
fn ready(item: &Item, items: &[Item]) -> Ready {
    ready_from(item, items, mdm_extract::on_disk(&item.dest))
}

/// [`ready`], with what the disk says of `item`'s archive (`mdm_extract::on_disk`).
fn ready_from(item: &Item, items: &[Item], disk: std::io::Result<mdm_extract::OnDisk>) -> Ready {
    let dir = item.dest.parent();
    let set: Vec<&Item> = match mdm_extract::set_key(&item.file_name) {
        Some(key) => items
            .iter()
            .filter(|i| {
                i.dest.parent() == dir
                    && mdm_extract::set_key(&i.file_name).as_deref() == Some(key.as_str())
            })
            .collect(),
        None => vec![item],
    };
    let unfinished = |i: &&Item| {
        i.id != item.id
            && matches!(
                i.status,
                Status::Downloading | Status::Queued | Status::Stopped
            )
    };
    // A ZIP in one file is its own whole set: another download of the same name still to come
    // (the same file again, to be numbered) is none of its volumes.
    let one_zip = volume_key(&item.file_name).is_none()
        && matches!(&disk, Ok(mdm_extract::OnDisk::Complete { files, .. }) if files.len() == 1);
    if !one_zip && set.iter().any(unfinished) {
        return Ready::Downloading;
    }
    let (archive, files) = match disk {
        Ok(mdm_extract::OnDisk::Complete { open, files }) => (open, files),
        Ok(mdm_extract::OnDisk::Incomplete(n)) => return Ready::Missing(n),
        Err(e) => return Ready::Unreadable(e),
    };
    // The downloads of its files, in volume order (a volume downloaded another way has none).
    let same = |a: &Path, b: &Path| {
        a.parent() == b.parent()
            && a.file_name().map(|n| n.to_string_lossy().to_lowercase())
                == b.file_name().map(|n| n.to_string_lossy().to_lowercase())
    };
    let mut ids: Vec<u64> = files
        .iter()
        .filter_map(|f| set.iter().find(|i| same(&i.dest, f)).map(|i| i.id))
        .collect();
    if !ids.contains(&item.id) {
        ids.push(item.id);
    }
    Ready::Yes {
        archive,
        files,
        ids,
    }
}

/// The words for a split archive not complete yet (`not`: why), never called damaged: in its
/// log, and Test archive's message.
fn incomplete_text(name: &str, not: &Ready) -> String {
    let why = match not {
        Ready::Downloading => {
            // i18n: why a split archive (in several files, its volumes) isn't tested yet, in "{name} isn't complete ({why})": some of its files are still downloading or waiting in the list
            crate::i18n::t("some of its volumes are still downloading").to_owned()
        }
        Ready::Missing(n) if *n > 0 => crate::i18n::fmt(
            // i18n: why a split archive (in several files, its volumes) isn't tested yet, in "{name} isn't complete ({why})"; {n} is the number of the volume not downloaded yet
            crate::i18n::t("volume {n} isn't there yet"),
            &[("n", &n.to_string())],
        ),
        // i18n: why a split archive (in several files, its volumes) isn't tested yet, in "{name} isn't complete ({why})": one of its files isn't downloaded yet
        _ => crate::i18n::t("a volume of it isn't there yet").to_owned(),
    };
    crate::i18n::fmt(
        // i18n: in a download's log and the Test archive message: a split archive is tested only once every volume is downloaded; {name} is its file name, {why} which volume is missing
        crate::i18n::t("Not tested yet: {name} isn't complete ({why})"),
        &[("name", &crate::i18n::isolate(name)), ("why", &why)],
    )
}

/// The log's words for an archive found damaged: its damaged files named (the first three).
fn damaged_text(entries: &[String]) -> String {
    if entries.is_empty() {
        // i18n: in a download's log: the archive's own structure doesn't read back (its headers, its end), or it isn't the archive its name says
        return crate::i18n::t(
            "The archive test failed: it is damaged, or not the archive its name says",
        )
        .to_owned();
    }
    let mut files: Vec<String> = entries
        .iter()
        .take(3)
        .map(|e| crate::i18n::isolate(e))
        .collect();
    if entries.len() > 3 {
        files.push("...".into());
    }
    crate::i18n::fmt(
        // i18n: in a download's log: the archive test found files in it that don't read back whole; {n} how many, {files} their names
        &crate::i18n::tn(
            entries.len() as u64,
            "The archive test found {n} damaged file: {files}",
            "The archive test found {n} damaged files: {files}",
        ),
        &[("files", &files.join(", "))],
    )
}

/// Why `name` wasn't tested, for its log.
fn not_tested(name: &str, e: &Error) -> String {
    let why = match e {
        Error::Damaged(said) => {
            tracing::info!("archive test: {name}: 7-Zip didn't test it: {said}");
            crate::i18n::fmt(
                // i18n: why an archive wasn't tested (after "Couldn't test {name}: "); 7-Zip is a program's name, {error} its words, in English
                crate::i18n::t("7-Zip couldn't test it ({error})"),
                &[("error", &crate::i18n::ltr(said))],
            )
        }
        e => crate::extractq::reason(name, e),
    };
    crate::i18n::fmt(
        // i18n: in a download's log: {name} is the archive's file name, {why} the reason it wasn't tested (as after "Couldn't extract {name}: ")
        crate::i18n::t("Couldn't test {name}: {why}"),
        &[("name", &crate::i18n::isolate(name)), ("why", &why)],
    )
}

/// A by-hand test's outcome for its message box: the archive named, the outcome under it, a
/// paragraph of its own (a translation's single line breaks are flowed into one line, where a
/// right-to-left one would put the name after the words).
fn format_told(name: &str, text: &str) -> String {
    format!("{}\n\n{text}", crate::i18n::isolate(name))
}

fn start(job: Job, progress: Arc<AtomicU32>, ctx: &egui::Context) -> Running {
    let result: Slot = Arc::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let (slot, stop, ctx) = (result.clone(), cancel.clone(), ctx.clone());
    let archive = job.archive.clone();
    let spawned = std::thread::Builder::new()
        .name("archive-test".into())
        .spawn(move || {
            let modified = std::fs::metadata(&archive).and_then(|m| m.modified()).ok();
            let tool = crate::extractq::seven_zip();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                mdm_extract::check::test(
                    &archive,
                    &mdm_extract::check::Test {
                        tool: tool.as_deref(),
                        cancel: Some(&stop),
                        progress: Some(&progress),
                    },
                )
            }))
            .unwrap_or_else(|_| Err(Error::Damaged("the test failed".into())));
            // Changed while it was read (another program wrote to it): not the file tested.
            let after = std::fs::metadata(&archive).and_then(|m| m.modified()).ok();
            let modified = modified.filter(|m| Some(*m) == after);
            *slot.lock().unwrap() = Some((r, modified));
            ctx.request_repaint();
        });
    if let Err(e) = spawned {
        *result.lock().unwrap() = Some((Err(Error::Io(e)), None));
    }
    Running {
        job,
        result,
        cancel,
    }
}

/// "Damaged archive": its test found errors; Redownload downloads it (every volume of a split
/// one) again from the start.
pub struct DamagedDialog {
    /// Its volumes' downloads.
    pub ids: Vec<u64>,
    name: String,
}

impl DamagedDialog {
    /// `Done(true)`: download it again; `Done(false)` or Closed: leave it.
    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<bool> {
        let split = self.ids.len() > 1;
        window(
            ctx,
            "damaged-archive",
            // i18n: window title: an archive (zip, 7z, rar) that was downloaded is damaged
            "Damaged archive",
            [400.0, 118.0],
            false,
            LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let line = crate::i18n::fmt(
                    if split {
                        // i18n: in the "Damaged archive" window: {name} is the archive's first volume's file name
                        crate::i18n::t(
                            "{name} is damaged: its test found errors in one of its volumes.",
                        )
                    } else {
                        // i18n: in the "Damaged archive" window: {name} is the archive's file name
                        crate::i18n::t("{name} is damaged: its test found errors in it.")
                    },
                    &[("name", &crate::i18n::isolate(&self.name))],
                );
                winui::paragraph(
                    &p,
                    o.x + 11.0,
                    o.y + 21.0,
                    &line,
                    LIGHT_TEXT(),
                    o.x + 389.0,
                    14.0,
                );
                winui::paragraph(
                    &p,
                    o.x + 11.0,
                    o.y + 56.0,
                    if split {
                        // i18n: in the "Damaged archive" window, under what was found; "Redownload" is the button beside
                        crate::i18n::t("Redownload downloads every volume again from the start.")
                    } else {
                        // i18n: in the "Damaged archive" window, under what was found; "Redownload" is the button beside
                        crate::i18n::t("Redownload downloads it again from the start.")
                    },
                    winui::MUTED(),
                    o.x + 389.0,
                    14.0,
                );
                let row = winui::buttons_light_right(
                    ui,
                    o,
                    389.0,
                    86.0,
                    &[
                        (crate::i18n::mark("Redownload"), ButtonKind::Default),
                        (crate::i18n::mark("Close"), ButtonKind::Normal),
                    ],
                );
                if row[0].clicked() || winui::enter_pressed(ui) {
                    Outcome::Done(true)
                } else if row[1].clicked() || winui::escape_pressed(ui) {
                    Outcome::Done(false)
                } else {
                    Outcome::Open
                }
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;

    fn item(id: u64, name: &str, size: u64) -> Item {
        let mut i = Item::new(
            id,
            format!("https://x.org/{name}").parse().unwrap(),
            PathBuf::from("dl").join(name),
            Category::Compressed,
        );
        i.status = Status::Complete;
        i.size = Some(size);
        i
    }

    fn on() -> Settings {
        Settings {
            test_archives: true,
            ..Default::default()
        }
    }

    #[test]
    fn names_of_tested_archives() {
        for n in [
            "a.zip",
            "B.7Z",
            "c.rar",
            "d.part2.rar",
            "e.r00",
            "f.7z.003",
            "g.zip.001",
        ] {
            assert!(tested_name(n), "{n}");
        }
        // Extracted, but not tested: no CRC of their own to read back (a gz's is the file's).
        for n in [
            "a.tar.gz",
            "b.tgz",
            "c.xz",
            "d.gz",
            "e.tar",
            "zip",
            ".zip",
            "movie.mkv",
        ] {
            assert!(!tested_name(n), "{n}");
        }
    }

    /// #1292: in the app the disk is looked at beside the window's thread (whether an archive is
    /// complete opens it). Its download waits meanwhile, not yet "Checking"; once looked at, one
    /// whose archive can't be tested yet is released and one that can is tested.
    #[test]
    fn the_disk_is_looked_at_beside_the_window_thread() {
        let dir = tempfile::tempdir().unwrap();
        let mut items = vec![
            item(1, "x.7z.001", 100),
            item(2, "a.zip", 100),
            item(3, "x.7z.002", 100),
        ];
        for i in &mut items {
            i.dest = dir.path().join(&i.file_name);
            std::fs::write(&i.dest, b"not an archive").unwrap();
        }
        // Its second volume is still to download.
        items[2].status = Status::Queued;
        let ctx = egui::Context::default();
        let mut q = Tests::default();
        q.look_beside(&ctx);
        assert!(q.hold(1, &mut items, &on()), "waits while it is looked at");
        assert!(q.hold(2, &mut items, &on()));
        assert!(
            !items[0].testing && !items[1].testing,
            "looked at, not checked yet"
        );
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut out = Vec::new();
        let mut zip_taken = false;
        while !q.looks.is_empty() {
            assert!(std::time::Instant::now() < until, "the looks never came");
            let zip_looking = q.looks.iter().any(|l| l.id == 2);
            out.extend(q.poll(&mut items, &on(), &ctx));
            // The poll that takes the ZIP's look starts its test (which can't have ended yet):
            // it is checked now, never released.
            if zip_looking && !q.looks.iter().any(|l| l.id == 2) {
                assert!(items[1].testing, "the ZIP is tested");
                assert!(!out.contains(&Done::Release(2)), "{out:?}");
                zip_taken = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(zip_taken);
        assert!(
            out.contains(&Done::Release(1)),
            "{out:?}: not tested while a volume is to come"
        );
        q.stop();
    }

    #[test]
    fn a_finished_archive_waits_for_its_test_under_its_limit() {
        let mut items = vec![
            item(1, "a.zip", 10 << 20),
            item(2, "big.zip", 5 << 30),
            item(3, "notes.txt", 10),
        ];
        let mut q = Tests::default();
        // Off in Options: nothing waits.
        assert!(!q.hold(
            1,
            &mut items,
            &Settings {
                test_archives: false,
                ..on()
            }
        ));
        assert!(!items[0].testing);
        assert!(q.hold(1, &mut items, &on()));
        assert!(items[0].testing, "its row says it is being checked");
        // Once, however often it is said.
        assert!(!q.hold(1, &mut items, &on()));
        // Past the limit (4 GB at first): not tested, and the log says how to test it.
        assert_eq!(on().test_limit(), 4 << 30);
        assert!(!q.hold(2, &mut items, &on()));
        assert!(items[1].log.iter().any(|(_, t)| t.contains("Test archive")));
        // Not an archive.
        assert!(!q.hold(3, &mut items, &on()));
        assert_eq!(q.queue.len(), 1);
        // Deleted: forgotten.
        q.forget(&[1]);
        assert!(q.queue.is_empty());
    }

    /// A download of `name` in `dir`, finished, its file written (`data`), or none yet.
    fn volume(id: u64, dir: &Path, name: &str, data: Option<&[u8]>) -> Item {
        let mut i = item(id, name, 300);
        i.dest = dir.join(name);
        match data {
            Some(d) => std::fs::write(&i.dest, d).unwrap(),
            None => i.status = Status::Queued,
        }
        i
    }

    /// A 7z of 900 bytes (its start header says so), cut in three volumes of 300.
    fn seven_parts() -> Vec<Vec<u8>> {
        let mut data = vec![b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4, 0, 0, 0, 0];
        data.extend_from_slice(&800u64.to_le_bytes());
        data.extend_from_slice(&68u64.to_le_bytes());
        data.resize(900, 1);
        data.chunks(300).map(<[u8]>::to_vec).collect()
    }

    /// Its volumes finishing one after another, the middle one last: tested once, when that
    /// one is in, from the first; never before, whatever the list holds so far.
    #[test]
    fn a_split_archive_is_tested_once_every_volume_is_in() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let parts = seven_parts();
        let mut q = Tests::default();
        // A queue taking one at a time: only volume 1 is in the list yet, finished.
        let mut items = vec![volume(1, d, "m.7z.001", Some(&parts[0]))];
        assert!(!q.hold(1, &mut items, &on()), "volumes 2 and 3 are to come");
        assert!(!items[0].testing && q.queue.is_empty());
        // Volumes 2 (queued) and 3 (finished) added; 3 finishes before 2.
        items.push(volume(2, d, "m.7z.002", None));
        items.push(volume(3, d, "m.7z.003", Some(&parts[2])));
        assert!(!q.hold(3, &mut items, &on()), "volume 2 is still queued");
        assert!(matches!(ready(&items[2], &items), Ready::Downloading));
        // Running: the same.
        items[1].status = Status::Downloading;
        assert!(!q.hold(3, &mut items, &on()));
        // Finished in the list but its file not there (moved, deleted): the disk says.
        items[1].status = Status::Complete;
        assert!(matches!(ready(&items[1], &items), Ready::Missing(2)));
        assert!(!q.hold(2, &mut items, &on()));
        std::fs::write(&items[1].dest, &parts[1]).unwrap();
        assert!(q.hold(2, &mut items, &on()));
        assert_eq!(q.queue.len(), 1, "once");
        let job = &q.queue[0];
        assert_eq!(job.ids, [1, 2, 3]);
        assert_eq!(job.archive, d.join("m.7z.001"));
        assert_eq!((job.held, job.shown), (Some(2), 2));
        assert!(
            job.kept.is_none(),
            "a split archive's damage is never re-fetched"
        );
        // Said again by another of its volumes: still once.
        assert!(!q.hold(3, &mut items, &on()));
        assert_eq!(q.queue.len(), 1);
    }

    /// A ZIP in one file with another download of the same name still queued (the same file
    /// again, to be numbered): no volume of it, it is tested.
    #[test]
    fn a_zip_in_one_file_does_not_wait_for_a_namesake() {
        let mut items = vec![item(1, "a.zip", 1000), item(2, "a.zip", 1000)];
        items[1].status = Status::Queued;
        let mut q = Tests::default();
        assert!(q.hold(1, &mut items, &on()));
        assert_eq!(q.queue[0].ids, [1]);
    }

    /// The last volume on disk says more is to come (a RAR's end header): not tested, though
    /// every volume the list has is finished.
    #[test]
    fn a_volume_that_says_more_follows_waits() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let end = |more: u8| {
            let mut v = b"Rar!\x1A\x07\x01\x00 some data".to_vec();
            let body = [3u8, 5, 0, more];
            v.extend_from_slice(&crc32(&body).to_le_bytes());
            v.extend_from_slice(&body);
            v
        };
        let mut items = vec![
            volume(1, d, "m.part1.rar", Some(&end(1))),
            volume(2, d, "m.part2.rar", Some(&end(1))),
        ];
        let mut q = Tests::default();
        assert!(!q.hold(2, &mut items, &on()));
        assert!(matches!(ready(&items[1], &items), Ready::Missing(3)));
        items.push(volume(3, d, "m.part3.rar", Some(&end(0))));
        assert!(q.hold(3, &mut items, &on()));
        assert_eq!(q.queue[0].archive, d.join("m.part1.rar"));
    }

    /// The CRC-32 a RAR 5 header carries (zlib's, by hand: the app has no crc32 crate of its own).
    fn crc32(b: &[u8]) -> u32 {
        let mut c = !0u32;
        for &x in b {
            c ^= x as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    /// Test archive on a split archive not complete yet: said so, nothing tested, nothing
    /// damaged; 7-Zip finding its end missing says the same.
    #[test]
    fn an_incomplete_split_archive_is_said_incomplete_never_damaged() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let parts = seven_parts();
        let mut items = vec![
            volume(1, d, "m.7z.001", Some(&parts[0])),
            volume(3, d, "m.7z.003", Some(&parts[2])),
        ];
        let mut q = Tests::default();
        q.by_hand(1, &mut items);
        assert!(q.queue.is_empty() && !items[0].testing && !items[0].damaged);
        let told = q.told.take().unwrap();
        assert!(
            told.contains("isn't complete") && told.contains("volume 2"),
            "{told}"
        );
        assert!(
            items[0]
                .log
                .iter()
                .any(|(_, t)| t.contains("isn't complete"))
        );
        // Volume 2 still downloading.
        items.push(volume(2, d, "m.7z.002", None));
        items[2].status = Status::Downloading;
        q.by_hand(3, &mut items);
        let told = q.told.take().unwrap();
        assert!(told.contains("still downloading"), "{told}");
        // Complete as far as the disk says, but 7-Zip finds a volume missing: not damage.
        std::fs::write(&items[2].dest, &parts[1]).unwrap();
        items[2].status = Status::Complete;
        q.by_hand(1, &mut items);
        assert_eq!(q.queue[0].ids, [1, 2, 3]);
        let done = outcome(&mut q, &mut items, Err(Error::MissingVolume(0)));
        assert!(done.is_empty(), "by hand: nothing waits");
        assert!(q.damaged.is_none());
        assert!(items.iter().all(|i| !i.damaged && !i.testing));
        assert!(
            q.told
                .as_deref()
                .is_some_and(|t| t.contains("isn't complete"))
        );
        // Its download waiting for the test: released, not damaged.
        assert!(q.hold(2, &mut items, &on()));
        let done = outcome(&mut q, &mut items, Err(Error::MissingVolume(0)));
        assert_eq!(done, [Done::Release(2)]);
        assert!(q.damaged.is_none() && items.iter().all(|i| !i.damaged));
    }

    fn outcome(q: &mut Tests, items: &mut [Item], r: Result<Verdict, Error>) -> Vec<Done> {
        let run = q.queue.pop_front().unwrap();
        q.finish(run, r, None, items)
    }

    fn kept(item: &mut Item) {
        let url: url::Url = "http://127.0.0.1/a.zip".parse().unwrap();
        let state = ResumeState::new(
            url.clone(),
            url,
            item.dest.clone(),
            1000,
            Default::default(),
        );
        let mut snap = mdm_engine::Snapshot {
            state: mdm_engine::download::State::Done,
            url: item.url.clone(),
            file: Some(item.dest.clone()),
            size: Some(1000),
            downloaded: 1000,
            resumable: Some(true),
            segments: Vec::new(),
            connections: Vec::new(),
            error: None,
            failure: None,
            events: Vec::new(),
            at_service: None,
            server: None,
            finishing: None,
            kept: None,
            checksum_warning: None,
        };
        snap.kept = Some(Arc::new(state));
        item.snap = Some(snap);
    }

    fn damaged(entries: &[&str], ranges: Option<Vec<(u64, u64)>>) -> Result<Verdict, Error> {
        Ok(Verdict::Damaged(mdm_extract::Damage {
            entries: entries.iter().map(|s| s.to_string()).collect(),
            ranges,
            said: "ERROR: CRC Failed".into(),
        }))
    }

    #[test]
    fn whole_releases_it_and_damage_is_said_and_offered() {
        let mut items = vec![item(1, "a.zip", 1000), item(2, "b.7z", 1000)];
        let mut q = Tests::default();
        q.hold(1, &mut items, &on());
        assert_eq!(
            outcome(&mut q, &mut items, Ok(Verdict::Whole)),
            [Done::Release(1)]
        );
        assert!(!items[0].testing && !items[0].damaged);
        assert!(items[0].log.iter().any(|(_, t)| t.contains("no errors")));
        // A 7z's damage: said, offered for Redownload, done.
        q.hold(2, &mut items, &on());
        let done = outcome(&mut q, &mut items, damaged(&["a.txt", "b/c.bin"], None));
        assert_eq!(done, [Done::Release(2)]);
        assert!(items[1].damaged && !items[1].testing);
        assert!(
            items[1]
                .log
                .iter()
                .any(|(_, t)| t.contains("2 damaged files"))
        );
        assert_eq!(q.damaged.as_ref().map(|d| d.ids.clone()), Some(vec![2]));
        // Not tested (a password): said, done, nothing offered.
        let mut items = vec![item(3, "c.zip", 1000)];
        let mut q = Tests::default();
        q.hold(3, &mut items, &on());
        let done = outcome(&mut q, &mut items, Err(Error::NeedsPassword));
        assert_eq!(done, [Done::Release(3)]);
        assert!(!items[0].damaged && q.damaged.is_none());
        assert!(
            items[0]
                .log
                .iter()
                .any(|(_, t)| t.contains("Couldn't test"))
        );
    }

    #[test]
    fn a_placed_zip_damage_is_fetched_again_once() {
        let mut items = vec![item(1, "a.zip", 1000)];
        kept(&mut items[0]);
        let mut q = Tests::default();
        q.hold(1, &mut items, &on());
        let done = outcome(
            &mut q,
            &mut items,
            damaged(&["b.bin"], Some(vec![(100, 400)])),
        );
        let [Done::Refetch { id, ranges, .. }] = &done[..] else {
            panic!("{done:?}")
        };
        assert_eq!((*id, ranges.clone()), (1, vec![(100, 400)]));
        assert!(items[0].testing, "still not done");
        assert!(!items[0].damaged);
        // Its parts came again (the app marks it so): tested once more, whatever Options says;
        // still damaged: offered, never fetched a second time.
        items[0].refetched = true;
        assert!(q.hold(
            1,
            &mut items,
            &Settings {
                test_archives: false,
                ..on()
            }
        ));
        assert!(q.queue[0].again);
        let done = outcome(
            &mut q,
            &mut items,
            damaged(&["b.bin"], Some(vec![(100, 400)])),
        );
        assert_eq!(done, [Done::Release(1)]);
        assert!(items[0].damaged && !items[0].refetched && q.damaged.is_some());
        // Whole the second time: "now".
        let mut items = vec![item(2, "z.zip", 1000)];
        items[0].refetched = true;
        let mut q = Tests::default();
        q.hold(2, &mut items, &on());
        assert_eq!(
            outcome(&mut q, &mut items, Ok(Verdict::Whole)),
            [Done::Release(2)]
        );
        assert!(
            items[0]
                .log
                .iter()
                .any(|(_, t)| t.contains("no errors now"))
        );
    }

    #[test]
    fn by_hand_nothing_waits_and_the_outcome_is_told() {
        let mut items = vec![item(1, "a.zip", 9 << 30)];
        let mut q = Tests::default();
        q.by_hand(1, &mut items);
        assert_eq!(q.queue[0].held, None, "done already");
        assert!(outcome(&mut q, &mut items, Ok(Verdict::Whole)).is_empty());
        assert!(q.told.as_deref().is_some_and(|t| t.contains("a.zip")));
    }

    #[test]
    fn a_test_stopped_by_closing_runs_again_at_start() {
        let mut items = vec![item(1, "a.zip", 1000), item(2, "b.zip", 1000)];
        items[0].testing = true;
        let mut q = Tests::default();
        q.resume(&mut items, &on());
        assert_eq!(q.queue.len(), 1);
        assert_eq!(q.queue[0].held, None);
        // Turned off since: no longer waiting.
        let mut q = Tests::default();
        q.resume(
            &mut items,
            &Settings {
                test_archives: false,
                ..on()
            },
        );
        assert!(q.queue.is_empty() && !items[0].testing);
    }
}

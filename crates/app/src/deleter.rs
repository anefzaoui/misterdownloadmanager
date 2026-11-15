//! Downloads' files deleted beside the window (#1170). Delete and Shift+Delete, Delete All
//! Completed and the automation API's `delete` take their rows off the list at once; the files
//! go here, on a thread of their own, one batch after the other in the order they were deleted.
//! Windows can take seconds over one delete (a disk waking up, an antivirus scanning the file,
//! a network or USB drive, a file another program holds), and the window waited for each: 8 s in
//! #1170's report, for one file.
//!
//! A deleted download's place stays held ([`mdm_engine::naming::reserve`]) from the moment its
//! row leaves the list until its files are gone, so a download added meanwhile under that name
//! (or a video under that stem) is numbered as it would be for a file still there: what is
//! deleted here is only ever what the deleted download had, never a file of one that came after.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use mdm_engine::naming::{self, Reservation};

/// Files found when their download's turn comes, not when it is deleted (a video's leftovers:
/// looking for them reads its folder).
pub type Later = Box<dyn FnOnce() -> Vec<PathBuf> + Send>;

/// How a file is deleted (`std::fs::remove_file`; the tests' own).
type Remove = dyn Fn(&Path) -> io::Result<()> + Send + Sync;

/// What goes from the disk with one download taken off the list.
pub struct Doomed {
    /// Its name as the list shows it: said when its file can't go.
    name: String,
    /// Where it is saved.
    place: PathBuf,
    /// It was complete: its finished file at `place` goes, and one that can't is said.
    finished: bool,
    /// What else it left there, looked for in its turn.
    later: Option<Later>,
    /// `place`, held from the moment its row left the list. None: another download in this
    /// process held it then (one of this row's that has just stopped and is letting go).
    hold: Option<Reservation>,
}

impl Doomed {
    /// A download's files, as its row leaves the list (on the window's thread: no disk here).
    pub fn new(name: String, place: PathBuf, finished: bool, later: Option<Later>) -> Doomed {
        let hold = naming::reserve(&place);
        Doomed {
            name,
            place,
            finished,
            later,
            hold,
        }
    }
}

/// A finished file that couldn't be deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub name: String,
    pub error: String,
}

/// How long a deleted download's place may stay held by its own run that has just stopped,
/// before the place is taken for another download's and its files are left alone.
const LET_GO: Duration = Duration::from_secs(2);

/// One download's files: its resume state and part file, what it left beside them, then its
/// finished file, last, so its name stays taken on the disk as long as anything of it is there.
/// A file already gone is no failure, and only the finished file's are told: the rest is the
/// download's own working data.
fn delete_one(d: Doomed, remove: &dyn Fn(&Path) -> io::Result<()>) -> Option<Failure> {
    let hold = d.hold.or_else(|| {
        let until = Instant::now() + LET_GO;
        loop {
            if let Some(h) = naming::reserve(&d.place) {
                break Some(h);
            }
            if Instant::now() >= until {
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let Some(_hold) = hold else {
        tracing::warn!(
            "delete: another download is saving as {} now; the deleted one's files stay",
            d.place.display()
        );
        return None;
    };
    let working = [
        mdm_engine::state::state_path(&d.place),
        naming::part_path(&d.place),
    ];
    for f in working
        .into_iter()
        .chain(d.later.map(|l| l()).unwrap_or_default())
    {
        let _ = remove(&f);
    }
    if !d.finished {
        return None;
    }
    match remove(&d.place) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Some(Failure {
            name: d.name,
            error: e.to_string(),
        }),
        _ => None,
    }
}

/// A batch, in order; the finished files that couldn't go.
fn delete_batch(batch: Vec<Doomed>, remove: &dyn Fn(&Path) -> io::Result<()>) -> Vec<Failure> {
    batch
        .into_iter()
        .filter_map(|d| delete_one(d, remove))
        .collect()
}

/// What is said of a batch's failures: one message however many there were, the first named.
pub fn failures_text(failures: &[Failure]) -> Option<String> {
    let first = failures.first()?;
    let mut text = crate::i18n::fmt(
        // i18n: a message: a deleted download's file stayed on the disk; {file} = its name
        crate::i18n::t("{file} could not be deleted from the disk: {error}"),
        &[
            ("file", &crate::i18n::isolate(&first.name)),
            ("error", &first.error),
        ],
    );
    let more = failures.len() as u64 - 1;
    if more > 0 {
        text.push('\n');
        text += &crate::i18n::tn(
            // i18n: the message's next line: how many more deleted files stayed on the disk
            more,
            "{n} more file could not be deleted either.",
            "{n} more files could not be deleted either.",
        );
    }
    Some(text)
}

/// The thread that deletes, and the batches it has yet to finish.
pub struct Deleter {
    /// None when no thread could be made: then each batch goes on the caller's.
    tx: Option<mpsc::Sender<Vec<Doomed>>>,
    remove: Arc<Remove>,
    report: Arc<dyn Fn(Vec<Failure>) + Send + Sync>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

impl Deleter {
    /// Deleting from the disk; a batch's failures told in the main window's notice
    /// (`dialogs::tell`), which `wake` has drawn.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Deleter {
        Deleter::with(
            |p| std::fs::remove_file(p),
            move |failures| {
                if let Some(text) = failures_text(&failures) {
                    crate::dialogs::tell(crate::i18n::mark("Delete"), text);
                    wake();
                }
            },
        )
    }

    /// [`Deleter::new`] with its own way to delete a file and to say what failed (tests).
    fn with(
        remove: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
        report: impl Fn(Vec<Failure>) + Send + Sync + 'static,
    ) -> Deleter {
        let remove: Arc<Remove> = Arc::new(remove);
        let report: Arc<dyn Fn(Vec<Failure>) + Send + Sync> = Arc::new(report);
        let pending: Arc<(Mutex<usize>, Condvar)> = Arc::default();
        let (tx, rx) = mpsc::channel::<Vec<Doomed>>();
        let spawned = std::thread::Builder::new()
            .name("delete files".into())
            .spawn({
                let (remove, report, pending) = (remove.clone(), report.clone(), pending.clone());
                move || {
                    for batch in rx {
                        let (many, began) = (batch.len(), Instant::now());
                        let failures = delete_batch(batch, &*remove);
                        // What a disk held, for a report's log.
                        if began.elapsed() >= Duration::from_secs(1) {
                            tracing::info!(
                                "delete: {many} deleted download(s)' files took {} ms, beside \
                                 the window",
                                began.elapsed().as_millis()
                            );
                        }
                        if !failures.is_empty() {
                            report(failures);
                        }
                        let (n, done) = &*pending;
                        *n.lock().unwrap() -= 1;
                        done.notify_all();
                    }
                }
            });
        let tx = match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                tracing::warn!("delete: no thread to delete files on ({e}); they go at once");
                None
            }
        };
        Deleter {
            tx,
            remove,
            report,
            pending,
        }
    }

    /// Delete these downloads' files, after the batches before. Returns at once.
    pub fn delete(&self, batch: Vec<Doomed>) {
        if batch.is_empty() {
            return;
        }
        *self.pending.0.lock().unwrap() += 1;
        let batch = match &self.tx {
            Some(tx) => match tx.send(batch) {
                Ok(()) => return,
                Err(mpsc::SendError(batch)) => batch,
            },
            None => batch,
        };
        let failures = delete_batch(batch, &*self.remove);
        if !failures.is_empty() {
            (self.report)(failures);
        }
        *self.pending.0.lock().unwrap() -= 1;
    }

    /// Wait until every batch handed over is deleted, for at most `wait` (MDM's end: a disk that
    /// holds a delete doesn't hold the exit). True when none is left.
    pub fn finish(&self, wait: Duration) -> bool {
        let (n, done) = &*self.pending;
        let left = n.lock().unwrap();
        let (left, _) = done.wait_timeout_while(left, wait, |n| *n > 0).unwrap();
        *left == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doomed(place: &Path, finished: bool) -> Doomed {
        Doomed::new(
            place.file_name().unwrap().to_string_lossy().into_owned(),
            place.to_owned(),
            finished,
            None,
        )
    }

    /// A deleter whose deletes are recorded (and then made by `remove`), its failures sent back.
    #[allow(clippy::type_complexity)]
    fn recorded(
        remove: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
    ) -> (
        Deleter,
        Arc<Mutex<Vec<PathBuf>>>,
        mpsc::Receiver<Vec<Failure>>,
    ) {
        let seen: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let deleter = Deleter::with(
            {
                let seen = seen.clone();
                move |p| {
                    seen.lock().unwrap().push(p.to_owned());
                    remove(p)
                }
            },
            move |f| {
                let _ = tx.lock().unwrap().send(f);
            },
        );
        (deleter, seen, rx)
    }

    /// Each download's files in turn, in the order deleted, batch after batch: its state and
    /// part file, then what it left, then its finished file (an unfinished one has none).
    #[test]
    fn files_go_in_order_the_finished_one_last() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let (a, b, c) = (d.join("a.zip"), d.join("b.iso"), d.join("c.mp4"));
        for f in [
            &a,
            &naming::part_path(&b),
            &mdm_engine::state::state_path(&b),
            &d.join("c.f137.mp4"),
        ] {
            std::fs::write(f, b"x").unwrap();
        }
        let (deleter, seen, failures) = recorded(|p| std::fs::remove_file(p));
        let left = d.join("c.f137.mp4");
        let later: Later = Box::new(move || vec![left]);
        deleter.delete(vec![doomed(&a, true), doomed(&b, false)]);
        deleter.delete(vec![Doomed::new(
            "c.mp4".into(),
            c.clone(),
            true,
            Some(later),
        )]);
        assert!(deleter.finish(Duration::from_secs(10)));
        let state = mdm_engine::state::state_path;
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                state(&a),
                naming::part_path(&a),
                a.clone(),
                state(&b),
                naming::part_path(&b),
                state(&c),
                naming::part_path(&c),
                d.join("c.f137.mp4"),
                c.clone(),
            ]
        );
        assert_eq!(std::fs::read_dir(d).unwrap().count(), 0, "all gone");
        // A finished file already gone (c.mp4 never was there) isn't a failure.
        assert!(failures.try_recv().is_err());
    }

    /// A finished file that can't go is said, once a batch for all of them; a state or part
    /// file that can't, or a file gone already, isn't; the rest of the batch still goes.
    #[test]
    fn failures_are_collected_per_batch() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let names = ["held 1.bin", "gone.bin", "held 2.bin", "fine.bin"];
        for n in ["held 1.bin", "held 2.bin", "fine.bin"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        let (deleter, seen, failures) = recorded(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            if name.starts_with("held") {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "in use"))
            } else {
                std::fs::remove_file(p)
            }
        });
        deleter.delete(names.iter().map(|n| doomed(&d.join(n), true)).collect());
        let told = failures.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            told,
            vec![
                Failure {
                    name: "held 1.bin".into(),
                    error: "in use".into()
                },
                Failure {
                    name: "held 2.bin".into(),
                    error: "in use".into()
                },
            ]
        );
        assert!(deleter.finish(Duration::from_secs(10)));
        assert!(failures.try_recv().is_err(), "one report for the batch");
        assert!(!d.join("fine.bin").exists());
        assert!(seen.lock().unwrap().contains(&d.join("gone.bin")));
        let text = failures_text(&told).unwrap();
        assert!(
            text.contains("held 1.bin") && text.contains("in use"),
            "{text}"
        );
        assert!(text.contains("1 more file could not be deleted"), "{text}");
        assert!(!failures_text(&told[..1]).unwrap().contains("more"));
        assert_eq!(failures_text(&[]), None);
    }

    /// The window hands a batch over and goes on: a delete the disk holds for seconds holds the
    /// deleter's thread only. Its place stays taken meanwhile: a download added under its name
    /// is numbered, and the name is free once its files are gone.
    #[test]
    fn a_slow_disk_never_holds_the_caller() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let file = d.join("slow.bin");
        std::fs::write(&file, b"x").unwrap();
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let (deleter, _, _) = recorded(move |p| {
            if p.extension().is_some_and(|e| e == "bin") {
                // The disk answers when the test says so (at most 30 s).
                let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(30));
            }
            std::fs::remove_file(p)
        });
        let began = Instant::now();
        deleter.delete(vec![doomed(&file, true)]);
        deleter.delete(vec![doomed(&d.join("next.bin"), true)]);
        assert!(
            began.elapsed() < Duration::from_millis(500),
            "{:?}",
            began.elapsed()
        );
        assert!(
            !deleter.finish(Duration::from_millis(200)),
            "still deleting"
        );
        assert!(file.exists());
        assert_eq!(naming::unique_path(d, "slow.bin"), d.join("slow (1).bin"));
        go.send(()).unwrap();
        go.send(()).unwrap();
        assert!(deleter.finish(Duration::from_secs(10)));
        assert!(!file.exists());
        assert_eq!(naming::unique_path(d, "slow.bin"), d.join("slow.bin"));
    }

    /// A place another download holds (for longer than its own run takes to let go) is that
    /// download's now: nothing of it is deleted.
    #[test]
    fn a_place_another_download_holds_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let place = dir.path().join("taken.bin");
        let part = naming::part_path(&place);
        std::fs::write(&part, b"another download's").unwrap();
        let other = naming::reserve(&place).unwrap();
        let (deleter, seen, _) = recorded(|p| std::fs::remove_file(p));
        deleter.delete(vec![doomed(&place, false)]);
        assert!(deleter.finish(LET_GO + Duration::from_secs(10)));
        assert!(part.exists());
        assert!(seen.lock().unwrap().is_empty());
        drop(other);
        // One whose own run lets go meanwhile: its files go.
        let held = naming::reserve(&place).unwrap();
        deleter.delete(vec![doomed(&place, false)]);
        std::thread::sleep(Duration::from_millis(200));
        drop(held);
        assert!(deleter.finish(Duration::from_secs(10)));
        assert!(!part.exists());
    }
}

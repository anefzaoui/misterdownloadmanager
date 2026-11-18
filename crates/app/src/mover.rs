//! Downloads moved beside the window (follows #743, #1170).
//!
//! File Properties' **Move** and **Move/Rename** on a selection numbered each download's name in
//! the chosen folder on the window's thread (`naming::destination`), which reads a folder that may
//! sit on a sleeping hard disk or a network share that doesn't answer; **Move/Rename** on finished
//! files also started a copy thread per file without holding the chosen name until the copy landed.
//!
//! Here one thread (`move files`) works through the batches in the order asked:
//! - a finished file moved to a folder takes a free name there ([`mdm_engine::naming::reserve`]
//!   held until the move is done, so a download starting or another move behind it numbers past
//!   it), makes the folder if needed, moves the file, and wakes the window;
//! - a finished file moved to an exact path (`Move/Rename`'s Save As) makes its parent folder if
//!   needed, moves the file, and wakes the window;
//! - an unstarted download moved to a folder takes its folder at once on the window's thread
//!   (fitted and kept apart from the other rows listed there, with `number_at_start` so a start
//!   meanwhile never waits on the window's thread), and settles its numbered name past the
//!   folder's files once the mover has looked at the folder in turn.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use mdm_engine::naming::{self, Reservation};

type Touch = dyn Fn(&Path) + Send + Sync;
type Transfer = dyn Fn(&Path, &Path) -> Result<(), String> + Send + Sync;
type Wake = dyn Fn() + Send + Sync;

/// Move a file: its parent folder made if needed, then a rename, else a copy and the original
/// deleted (a copy whose original can't go, in use, is taken back: the file stays in one place).
pub fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| e.to_string())?;
    std::fs::remove_file(from).map_err(|e| {
        let _ = std::fs::remove_file(to);
        e.to_string()
    })
}

/// One download's move or folder placement in a batch.
pub enum Step {
    /// A finished download moved to an exact path chosen in Save As.
    Exact { id: u64, from: PathBuf, to: PathBuf },
    /// A finished download moved into `dir` under `name` (numbered if taken there).
    InFolder {
        id: u64,
        from: PathBuf,
        dir: PathBuf,
        name: String,
    },
    /// An unstarted download placed into `dir` under `name` (numbered past `dir`'s files and
    /// earlier steps of the batch). `asked` is the temporary path its row took at once.
    Place {
        id: u64,
        asked: PathBuf,
        dir: PathBuf,
        name: String,
        /// `Some(named)` for a video (`named`: the user typed its name); `None` for a file.
        video: Option<bool>,
        /// Lowercase paths (files) or stems (videos) of rows already in `dir` before this batch.
        listed: HashSet<String>,
    },
}

/// What finished on the mover's thread, for the window to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Moved {
        id: u64,
        result: Result<PathBuf, String>,
    },
    Placed {
        id: u64,
        asked: PathBuf,
        to: PathBuf,
    },
}

fn low(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Where an unstarted file download named `name` goes in `dir` before the disk is looked at:
/// fitted to `dir`'s path limits and kept apart from `taken` (lowercase paths in `dir`).
/// Never touches the disk.
pub fn guess_file_in(dir: &Path, name: &str, taken: &mut HashSet<String>) -> PathBuf {
    let fitted = naming::fit_name(dir, name).unwrap_or_else(|| name.to_owned());
    let (stem, ext) = split_ext(&fitted);
    for cand in
        std::iter::once(fitted.clone()).chain((1u32..).map(|n| format!("{stem} ({n}){ext}")))
    {
        let p = dir.join(&cand);
        if taken.insert(low(&p)) {
            return p;
        }
    }
    unreachable!("unbounded search finds a free name")
}

/// Where an unstarted video named `name` goes in `dir` before the disk is looked at: its stem
/// fitted to `dir`'s limits (unless `named` by the user) and kept apart from `taken_stems`
/// (lowercase stems in `dir`). Never touches the disk.
pub fn guess_video_in(
    dir: &Path,
    name: &str,
    named: bool,
    taken_stems: &mut HashSet<String>,
) -> PathBuf {
    let fitted = if named {
        name.to_owned()
    } else {
        naming::fit_stem(dir, name).unwrap_or_else(|| name.to_owned())
    };
    let (stem, ext) = split_ext(&fitted);
    for cand in std::iter::once(stem.to_owned()).chain((1u32..).map(|n| format!("{stem} ({n})"))) {
        if taken_stems.insert(cand.to_lowercase()) {
            return dir.join(format!("{cand}{ext}"));
        }
    }
    unreachable!("unbounded search finds a free stem")
}

fn disk_taken(p: &Path) -> bool {
    p.exists() || naming::part_path(p).exists() || mdm_engine::state::state_path(p).exists()
}

/// Pick and reserve a free path in `dir` for a finished file `name`, skipping `taken`.
fn reserve_in_folder(
    dir: &Path,
    name: &str,
    taken: &HashSet<String>,
) -> (PathBuf, String, Reservation) {
    let fitted = naming::fit_name(dir, name).unwrap_or_else(|| name.to_owned());
    let (stem, ext) = split_ext(&fitted);
    for cand in
        std::iter::once(fitted.clone()).chain((1u32..).map(|n| format!("{stem} ({n}){ext}")))
    {
        let p = dir.join(&cand);
        let key = low(&p);
        if taken.contains(&key) || disk_taken(&p) {
            continue;
        }
        if let Some(hold) = naming::reserve(&p) {
            return (p, key, hold);
        }
    }
    unreachable!("unbounded search finds a free name")
}

/// Pick a free path in `dir` for an unstarted file `name`, skipping `taken`, `listed`, disk files
/// and any path currently reserved in this process.
fn place_file_in_folder(
    dir: &Path,
    name: &str,
    taken: &mut HashSet<String>,
    listed: &HashSet<String>,
) -> PathBuf {
    let fitted = naming::fit_name(dir, name).unwrap_or_else(|| name.to_owned());
    let (stem, ext) = split_ext(&fitted);
    for cand in
        std::iter::once(fitted.clone()).chain((1u32..).map(|n| format!("{stem} ({n}){ext}")))
    {
        let p = dir.join(&cand);
        let key = low(&p);
        if taken.contains(&key) || listed.contains(&key) || disk_taken(&p) {
            continue;
        }
        let Some(hold) = naming::reserve(&p) else {
            continue;
        };
        drop(hold);
        taken.insert(key);
        return p;
    }
    unreachable!("unbounded search finds a free name")
}

fn run_batch(
    batch: Vec<Step>,
    touch: &Touch,
    transfer: &Transfer,
    wake: &Wake,
    emit: &mut dyn FnMut(Outcome),
) {
    let mut taken_files = HashSet::new();
    let mut taken_stems: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for step in batch {
        match step {
            Step::Exact { id, from, to } => {
                touch(to.parent().unwrap_or(&to));
                let result = transfer(&from, &to).map(|()| to);
                emit(Outcome::Moved { id, result });
                wake();
            }
            Step::InFolder {
                id,
                from,
                dir,
                name,
            } => {
                touch(&dir);
                naming::wait_cleared(&dir);
                let (to, key, hold) = reserve_in_folder(&dir, &name, &taken_files);
                let r = transfer(&from, &to);
                drop(hold);
                if r.is_ok() {
                    taken_files.insert(key);
                    if let Some(s) = to.file_stem() {
                        taken_stems
                            .entry(dir)
                            .or_default()
                            .insert(s.to_string_lossy().to_lowercase());
                    }
                }
                emit(Outcome::Moved {
                    id,
                    result: r.map(|()| to),
                });
                wake();
            }
            Step::Place {
                id,
                asked,
                dir,
                name,
                video,
                listed,
            } => {
                touch(&dir);
                naming::wait_cleared(&dir);
                let to = if let Some(named) = video {
                    let stems = taken_stems.entry(dir.clone()).or_default();
                    let in_use = |s: &str| {
                        let s = s.to_lowercase();
                        stems.contains(&s) || listed.contains(&s)
                    };
                    let p = if named {
                        naming::typed_stem_destination(&dir, &name, in_use)
                    } else {
                        naming::stem_destination(&dir, &name, in_use)
                    };
                    if let Some(s) = p.file_stem() {
                        stems.insert(s.to_string_lossy().to_lowercase());
                    }
                    taken_files.insert(low(&p));
                    p
                } else {
                    let p = place_file_in_folder(&dir, &name, &mut taken_files, &listed);
                    if let Some(s) = p.file_stem() {
                        taken_stems
                            .entry(dir)
                            .or_default()
                            .insert(s.to_string_lossy().to_lowercase());
                    }
                    p
                };
                emit(Outcome::Placed { id, asked, to });
                wake();
            }
        }
    }
}

/// The thread that moves finished files and settles folder names, in order.
pub struct Mover {
    tx: Option<mpsc::Sender<Vec<Step>>>,
    rx: mpsc::Receiver<Outcome>,
    ready: Vec<Outcome>,
    in_flight: HashSet<u64>,
    touch: Arc<Touch>,
    transfer: Arc<Transfer>,
    wake: Arc<Wake>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

impl Mover {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Mover {
        Mover::with(|_| {}, move_file, wake)
    }

    pub(crate) fn with(
        touch: impl Fn(&Path) + Send + Sync + 'static,
        transfer: impl Fn(&Path, &Path) -> Result<(), String> + Send + Sync + 'static,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Mover {
        let touch: Arc<Touch> = Arc::new(touch);
        let transfer: Arc<Transfer> = Arc::new(transfer);
        let wake: Arc<Wake> = Arc::new(wake);
        let pending: Arc<(Mutex<usize>, Condvar)> = Arc::default();
        let (work_tx, work_rx) = mpsc::channel::<Vec<Step>>();
        let (out_tx, out_rx) = mpsc::channel::<Outcome>();
        let spawned = std::thread::Builder::new()
            .name("move files".into())
            .spawn({
                let (touch, transfer, wake, pending) = (
                    touch.clone(),
                    transfer.clone(),
                    wake.clone(),
                    pending.clone(),
                );
                move || {
                    for batch in work_rx {
                        let (many, began) = (batch.len(), Instant::now());
                        run_batch(batch, &*touch, &*transfer, &*wake, &mut |out| {
                            let _ = out_tx.send(out);
                        });
                        if began.elapsed() >= Duration::from_secs(1) {
                            tracing::info!(
                                "move: {many} download(s) took {} ms, beside the window",
                                began.elapsed().as_millis()
                            );
                        }
                        let (n, done) = &*pending;
                        *n.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
                        done.notify_all();
                    }
                }
            });
        let tx = match spawned {
            Ok(_) => Some(work_tx),
            Err(e) => {
                tracing::warn!("move: no thread to move files on ({e}); they move at once");
                None
            }
        };
        Mover {
            tx,
            rx: out_rx,
            ready: Vec::new(),
            in_flight: HashSet::new(),
            touch,
            transfer,
            wake,
            pending,
        }
    }

    /// Hand `batch` to the mover, after the batches before. Returns at once.
    pub fn submit(&mut self, batch: Vec<Step>) {
        if batch.is_empty() {
            return;
        }
        for step in &batch {
            if let Step::Exact { id, .. } | Step::InFolder { id, .. } = step {
                self.in_flight.insert(*id);
            }
        }
        *self.pending.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        let batch = match &self.tx {
            Some(tx) => match tx.send(batch) {
                Ok(()) => return,
                Err(mpsc::SendError(batch)) => batch,
            },
            None => batch,
        };
        run_batch(
            batch,
            &*self.touch,
            &*self.transfer,
            &*self.wake,
            &mut |out| {
                self.ready.push(out);
            },
        );
        *self.pending.0.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
    }

    /// Whether finished download `id`'s file is currently moving.
    pub fn is_moving(&self, id: u64) -> bool {
        self.in_flight.contains(&id)
    }

    /// Whether any finished file is currently moving.
    pub fn has_moves(&self) -> bool {
        !self.in_flight.is_empty()
    }

    /// Drain the outcomes that have finished since the last call. Never waits.
    pub fn take(&mut self) -> Vec<Outcome> {
        let mut out = std::mem::take(&mut self.ready);
        while let Ok(o) = self.rx.try_recv() {
            out.push(o);
        }
        for o in &out {
            if let Outcome::Moved { id, .. } = o {
                self.in_flight.remove(id);
            }
        }
        out
    }

    /// Wait until every batch handed over is done, for at most `wait` (MDM's end: a disk that
    /// holds a move doesn't hold the exit). True when none is left.
    pub fn finish(&self, wait: Duration) -> bool {
        let (n, done) = &*self.pending;
        let left = n.lock().unwrap_or_else(|e| e.into_inner());
        let (left, _) = done
            .wait_timeout_while(left, wait, |n| *n > 0)
            .unwrap_or_else(|e| e.into_inner());
        *left == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_disk_never_holds_the_caller_and_keeps_names_apart() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(dst.path().join("a.bin"), b"existing").unwrap();
        let f1 = src.path().join("one.bin");
        let f2 = src.path().join("two.bin");
        std::fs::write(&f1, b"first").unwrap();
        std::fs::write(&f2, b"second").unwrap();

        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let mut mover = Mover::with(
            {
                let gate = gate.clone();
                move |_| drop(gate.lock().unwrap())
            },
            move_file,
            || {},
        );

        let began = Instant::now();
        mover.submit(vec![
            Step::InFolder {
                id: 1,
                from: f1.clone(),
                dir: dst.path().to_owned(),
                name: "a.bin".into(),
            },
            Step::InFolder {
                id: 2,
                from: f2.clone(),
                dir: dst.path().to_owned(),
                name: "a.bin".into(),
            },
            Step::Place {
                id: 3,
                asked: dst.path().join("a.bin"),
                dir: dst.path().to_owned(),
                name: "a.bin".into(),
                video: None,
                listed: HashSet::new(),
            },
        ]);
        assert!(began.elapsed() < Duration::from_millis(500));
        assert!(mover.is_moving(1) && mover.is_moving(2) && !mover.is_moving(3));
        assert!(!mover.finish(Duration::from_millis(100)));
        assert!(f1.exists() && f2.exists());

        drop(held);
        assert!(mover.finish(Duration::from_secs(10)));
        let out = mover.take();
        assert!(!mover.has_moves());
        assert_eq!(
            out,
            vec![
                Outcome::Moved {
                    id: 1,
                    result: Ok(dst.path().join("a (1).bin")),
                },
                Outcome::Moved {
                    id: 2,
                    result: Ok(dst.path().join("a (2).bin")),
                },
                Outcome::Placed {
                    id: 3,
                    asked: dst.path().join("a.bin"),
                    to: dst.path().join("a (3).bin"),
                },
            ]
        );
        assert_eq!(
            std::fs::read(dst.path().join("a.bin")).unwrap(),
            b"existing"
        );
        assert_eq!(
            std::fs::read(dst.path().join("a (1).bin")).unwrap(),
            b"first"
        );
        assert_eq!(
            std::fs::read(dst.path().join("a (2).bin")).unwrap(),
            b"second"
        );
    }

    #[test]
    fn a_move_under_way_holds_its_destination_reservation() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let from = src.path().join("a.bin");
        std::fs::write(&from, b"x").unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let mut mover = Mover::with(
            |_| {},
            move |f, to| {
                let _ = entered_tx.send(to.to_owned());
                let _ = release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10));
                move_file(f, to)
            },
            || {},
        );
        mover.submit(vec![Step::InFolder {
            id: 1,
            from,
            dir: dst.path().to_owned(),
            name: "a.bin".into(),
        }]);
        let chosen = entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(chosen, dst.path().join("a.bin"));
        assert_eq!(
            naming::unique_path(dst.path(), "a.bin"),
            dst.path().join("a (1).bin"),
            "held while the copy is under way"
        );
        release_tx.send(()).unwrap();
        assert!(mover.finish(Duration::from_secs(10)));
    }

    #[test]
    fn a_failed_move_leaves_its_chosen_name_free_for_the_next() {
        let dst = tempfile::tempdir().unwrap();
        let ok_src = tempfile::tempdir().unwrap();
        let ok_file = ok_src.path().join("a.bin");
        std::fs::write(&ok_file, b"ok").unwrap();

        let mut mover = Mover::with(
            |_| {},
            |from, to| {
                if from.ends_with("locked.bin") {
                    Err("in use".into())
                } else {
                    move_file(from, to)
                }
            },
            || {},
        );
        mover.submit(vec![
            Step::InFolder {
                id: 1,
                from: dst.path().join("locked.bin"),
                dir: dst.path().to_owned(),
                name: "a.bin".into(),
            },
            Step::InFolder {
                id: 2,
                from: ok_file,
                dir: dst.path().to_owned(),
                name: "a.bin".into(),
            },
        ]);
        assert!(mover.finish(Duration::from_secs(10)));
        let out = mover.take();
        assert_eq!(
            out,
            vec![
                Outcome::Moved {
                    id: 1,
                    result: Err("in use".into()),
                },
                Outcome::Moved {
                    id: 2,
                    result: Ok(dst.path().join("a.bin")),
                },
            ]
        );
    }

    #[test]
    fn long_names_fit_and_number_without_looping() {
        let dst = tempfile::tempdir().unwrap();
        let long = format!("{}.bin", "a".repeat(260));
        let mut taken = HashSet::new();
        let first = guess_file_in(dst.path(), &long, &mut taken);
        let second = guess_file_in(dst.path(), &long, &mut taken);
        assert_ne!(first, second);
        assert!(naming::path_fits(&first) && naming::path_fits(&second));
    }
}

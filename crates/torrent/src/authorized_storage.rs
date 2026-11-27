//! App-authorized file handles. Storage never reopens a torrent path for writing.
//!
//! An unticked file the app didn't hand over is never made at its path: the parts of the pieces
//! it shares with ticked files (its first and last piece at most) go to a parts file of MDM's
//! own, `<session>/<hash>.parts/<index>`, as other BitTorrent clients do. Ticked later, it is
//! made then, and what the parts file held is copied into it.
//!
//! A paused torrent holds none of its files (`close_files`): another program may move, rename or
//! delete them meanwhile. When it starts they are given again by its grant, checked by the app
//! as at Start, and a file that isn't the one it was is an error, never written into. On
//! Windows a finished file is held read-only and shared (`on_file_completed`), so it can be
//! opened, copied, renamed or moved while it is seeded.
//!
//! Handles are counted in librqbit's budget of open files, shared by the whole process
//! (`open_files::global`): a torrent of thousands of files keeps a few open, the least recently
//! used one closed to open another. A grant made with [`authorize_lazily`] gives such a file
//! again through the app (`open_one`: the same object, at its path, checked as at Start), never
//! by opening the path here; without it, nothing is closed early.
use librqbit::storage::TorrentStorage;
use librqbit::storage::filesystem::open_files::{CachedHandle, Entry, OpenFiles};
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug)]
pub struct PreparedFile {
    pub index: usize,
    pub path: PathBuf,
    pub file: File,
}

/// A file the app checked and closed again (a torrent of many files is not held open all at
/// once): the engine gets it through the grant's `open_one` when it first reads or writes it.
/// `identity` is the object's, as the app reads it (opaque here).
#[derive(Debug, Clone)]
pub struct LaterFile {
    pub index: usize,
    pub path: PathBuf,
    pub identity: [u64; 5],
}

/// What the app hands the engine: the files it checked (open, or `later`), and the unticked
/// ones it didn't make, whose shared pieces go to a parts file instead (`parted`, by index).
/// `pinned`: files among `files` that are never closed early, as the app couldn't tell it is
/// the same object when giving it again (its filesystem has no file identity).
#[derive(Debug, Default)]
pub struct Prepared {
    pub files: Vec<PreparedFile>,
    pub parted: Vec<usize>,
    pub later: Vec<LaterFile>,
    pub pinned: Vec<usize>,
}

impl From<Vec<PreparedFile>> for Prepared {
    fn from(files: Vec<PreparedFile>) -> Self {
        Self {
            files,
            ..Default::default()
        }
    }
}

type Reopen = Arc<dyn Fn() -> io::Result<Prepared> + Send + Sync>;
/// The app gives one file of the torrent again (by index): the object it handed over, at its
/// path, checked as at Start. Never made.
pub type OpenOne = Arc<dyn Fn(usize) -> io::Result<PreparedFile> + Send + Sync>;
struct Grant {
    folder: PathBuf,
    first: Mutex<Option<Prepared>>,
    reopen: Reopen,
    open_one: Option<OpenOne>,
}
type Key = (PathBuf, String);
fn grants() -> &'static Mutex<HashMap<Key, Arc<Grant>>> {
    static GRANTS: OnceLock<Mutex<HashMap<Key, Arc<Grant>>>> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

/// Which files of each torrent are in a parts file now (by index), as its storage last opened.
fn parted() -> &'static Mutex<HashMap<Key, HashSet<usize>>> {
    static PARTED: OnceLock<Mutex<HashMap<Key, HashSet<usize>>>> = OnceLock::new();
    PARTED.get_or_init(Default::default)
}

/// The first storage initialization consumes exactly the handles checked by the app.
/// A subsequent initialization (file selection or session restart) must authorize again.
/// Every file but padding must be among `files`.
pub fn authorize(
    session: &Path,
    hash: &str,
    folder: &Path,
    files: Vec<PreparedFile>,
    reopen: Arc<dyn Fn() -> io::Result<Vec<PreparedFile>> + Send + Sync>,
) {
    authorize_parted(
        session,
        hash,
        folder,
        files.into(),
        Arc::new(move || reopen().map(Prepared::from)),
    );
}

/// [`authorize`], where the files in `prepared.parted` (unticked, not made) are kept in a parts
/// file.
pub fn authorize_parted(
    session: &Path,
    hash: &str,
    folder: &Path,
    prepared: Prepared,
    reopen: Reopen,
) {
    grant(session, hash, folder, prepared, reopen, None);
}

/// [`authorize_parted`], where the storage may close a file's handle to keep the process within
/// its budget of open files, and gets it again by `open_one`; the files in `prepared.later` are
/// only ever given so.
pub fn authorize_lazily(
    session: &Path,
    hash: &str,
    folder: &Path,
    prepared: Prepared,
    reopen: Reopen,
    open_one: OpenOne,
) {
    grant(session, hash, folder, prepared, reopen, Some(open_one));
}

fn grant(
    session: &Path,
    hash: &str,
    folder: &Path,
    prepared: Prepared,
    reopen: Reopen,
    open_one: Option<OpenOne>,
) {
    grants().lock().unwrap().insert(
        (session.to_owned(), hash.to_owned()),
        Arc::new(Grant {
            folder: folder.to_owned(),
            first: Mutex::new(Some(prepared)),
            reopen,
            open_one,
        }),
    );
}

pub fn forget(session: &Path, hash: &str) {
    grants()
        .lock()
        .unwrap()
        .remove(&(session.to_owned(), hash.to_owned()));
}

/// Whether file `index` of the torrent is kept in a parts file by its running storage: ticking
/// it needs the torrent opened again, on a file the app makes.
pub fn is_parted(session: &Path, hash: &str, index: usize) -> bool {
    parted()
        .lock()
        .unwrap()
        .get(&(session.to_owned(), hash.to_owned()))
        .is_some_and(|p| p.contains(&index))
}

/// Whether file `index` of the torrent has bytes kept for it in a parts file, or is kept in one
/// by its running storage: ticking it needs the torrent in the session, opened on a file the app
/// makes. Read from the disk too, so it holds for a torrent out of the session (finished, the
/// session stopped while idle) and after MDM starts again.
pub fn has_parts(session: &Path, hash: &str, index: usize) -> bool {
    is_parted(session, hash, index)
        || parts_dir(session, hash).is_some_and(|d| d.join(index.to_string()).is_file())
}

/// The torrent's parts files, under the session's folder (None: not an info hash).
fn parts_dir(session: &Path, hash: &str) -> Option<PathBuf> {
    (hash.len() == 40 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        .then(|| session.join(format!("{}.parts", hash.to_ascii_lowercase())))
}

/// Delete the torrent's parts files (it is taken off the list).
pub fn remove_parts(session: &Path, hash: &str) -> io::Result<()> {
    parted()
        .lock()
        .unwrap()
        .remove(&(session.to_owned(), hash.to_owned()));
    let Some(dir) = parts_dir(session, hash) else {
        return Ok(());
    };
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Where an unticked file's bytes are kept. Only the pieces it shares with ticked files are
/// ever written, and those are within its first and its last piece: the parts file holds those
/// two (a file of two pieces or less, all of it at the same offsets).
struct Parts {
    path: PathBuf,
    len: u64,
    piece: u64,
    entry: Entry,
    // Held for reading by each read and write: the budget closes it only when none is in flight.
    file: RwLock<Option<File>>,
    written: AtomicBool,
}

impl Parts {
    fn new(path: PathBuf, len: u64, piece: u64, budget: &'static OpenFiles) -> Self {
        Self {
            path,
            len,
            piece,
            entry: Entry::new(budget),
            file: RwLock::new(None),
            written: AtomicBool::new(false),
        }
    }

    /// Where `n` bytes at `offset` of the file are in the parts file (None: in the middle of a
    /// long file, never shared with a ticked one).
    fn map(&self, offset: u64, n: u64) -> Option<u64> {
        map_offset(self.len, self.piece, offset, n)
    }

    /// What the parts file holds at `offset` of the file; zeros for what it doesn't (never
    /// written, or the middle of a long file). A piece read so fails its hash and counts as
    /// missing, and an error here would make librqbit's check skip the rest of the file, the
    /// last piece it shares with the next file among them.
    fn read(self: &Arc<Self>, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        buf.fill(0);
        let Some(at) = self.map(offset, buf.len() as u64) else {
            return Ok(());
        };
        let file = match self.open(false) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let file = file.as_ref().ok_or_else(|| io::Error::other("closed"))?;
        let mut done = 0;
        while done < buf.len() {
            #[cfg(windows)]
            let n = {
                use std::os::windows::fs::FileExt;
                file.seek_read(&mut buf[done..], at + done as u64)?
            };
            #[cfg(unix)]
            let n = {
                use std::os::unix::fs::FileExt;
                file.read_at(&mut buf[done..], at + done as u64)?
            };
            if n == 0 {
                break;
            }
            done += n;
        }
        Ok(())
    }

    /// The parts file's handle, held for reading (opened, and made if `create`, when there is
    /// none). It is MDM's own file, in the session's folder: opened by its path.
    fn open(
        self: &Arc<Self>,
        create: bool,
    ) -> io::Result<std::sync::RwLockReadGuard<'_, Option<File>>> {
        self.entry.touch();
        // Closed by the budget between opening it and reading through it: opened again.
        for _ in 0..3 {
            {
                let g = self.file.read().map_err(|_| io::Error::other("lock"))?;
                if g.is_some() {
                    return Ok(g);
                }
            }
            let mut slot = self.file.write().map_err(|_| io::Error::other("lock"))?;
            if slot.is_none() {
                if create && let Some(dir) = self.path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                self.entry.admit(self);
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(create)
                    .truncate(false)
                    .open(&self.path)
                {
                    Ok(f) => *slot = Some(f),
                    Err(e) => {
                        self.entry.closed();
                        return Err(e);
                    }
                }
            }
        }
        Err(io::Error::other("a parts file was closed while in use"))
    }

    /// Writes what was written to disk and closes the handle; one the budget closed after it
    /// was written is opened again (by its path: MDM's own file) to be written to disk.
    fn close(&self) -> io::Result<()> {
        let file = self
            .file
            .write()
            .map_err(|_| io::Error::other("lock"))?
            .take();
        if file.is_some() {
            self.entry.closed();
        }
        if !self.written.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let file = match file {
            Some(f) => f,
            None => match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.path)
            {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(e) => {
                    self.written.store(true, Ordering::Release);
                    return Err(e);
                }
            },
        };
        file.sync_data()
    }
}

impl CachedHandle for Parts {
    fn last_used(&self) -> u64 {
        self.entry.last_used()
    }

    /// Closed by the budget, not written to disk here: a pause does that ([`Parts::close`]).
    fn try_close(&self) -> bool {
        let Ok(mut slot) = self.file.try_write() else {
            return false;
        };
        if slot.is_none() {
            return false;
        }
        drop(slot.take());
        self.entry.closed();
        true
    }
}

impl Drop for Parts {
    fn drop(&mut self) {
        if self.file.get_mut().is_ok_and(|f| f.is_some()) {
            self.entry.closed();
        }
    }
}

fn map_offset(len: u64, piece: u64, offset: u64, n: u64) -> Option<u64> {
    let end = offset.checked_add(n)?;
    if end > len {
        return None;
    }
    if len <= piece.saturating_mul(2) || end <= piece {
        return Some(offset);
    }
    let tail = len - piece;
    (offset >= tail).then(|| piece + (offset - tail))
}

/// Copy what an unticked file's parts file held into the file made for it now that it is
/// ticked, at the offsets it has in the file. Only into a file still empty: one with bytes
/// already is the file itself.
fn move_parts_into(parts: &Path, file: &File, len: u64, piece: u64) -> io::Result<()> {
    let held = match std::fs::read(parts) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() == 0 && !held.is_empty() {
        let piece_us = usize::try_from(piece).unwrap_or(usize::MAX);
        if len <= piece.saturating_mul(2) {
            let n = held.len().min(usize::try_from(len).unwrap_or(usize::MAX));
            write_at(file, 0, &held[..n])?;
        } else {
            write_at(file, 0, &held[..held.len().min(piece_us)])?;
            if held.len() > piece_us {
                let rest = &held[piece_us..];
                let rest = &rest[..rest.len().min(piece_us)];
                write_at(file, len - piece, rest)?;
            }
        }
        file.sync_data()?;
    }
    std::fs::remove_file(parts)
}

fn read_at(file: &File, mut offset: u64, buf: &mut [u8]) -> io::Result<()> {
    let mut rest = buf;
    while !rest.is_empty() {
        #[cfg(windows)]
        let n = {
            use std::os::windows::fs::FileExt;
            file.seek_read(rest, offset)?
        };
        #[cfg(unix)]
        let n = {
            use std::os::unix::fs::FileExt;
            file.read_at(rest, offset)?
        };
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        offset += n as u64;
        rest = &mut rest[n..];
    }
    Ok(())
}

fn write_at(file: &File, mut offset: u64, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = {
            use std::os::windows::fs::FileExt;
            file.seek_write(buf, offset)?
        };
        #[cfg(unix)]
        let n = {
            use std::os::unix::fs::FileExt;
            file.write_at(buf, offset)?
        };
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        offset += n as u64;
        buf = &buf[n..];
    }
    Ok(())
}

enum Slot {
    Padding,
    File(Arc<FileSlot>),
    Parts(Arc<Parts>),
}

/// A file the app handed over.
struct FileSlot {
    index: usize,
    /// Where the app opened it: a handle given again must be for the same path.
    path: PathBuf,
    entry: Entry,
    /// The budget may close it (the grant can give it again: `open_one`).
    evictable: bool,
    // One lock per file: positioned writes to different pieces remain concurrent.
    file: RwLock<Option<File>>,
    /// Written through since it was handed over: written to disk before the handle closes.
    written: AtomicBool,
    /// Every piece of it is ours (`on_file_completed`): it is only read from now on.
    finished: AtomicBool,
    /// The handle can't write (Windows, finished: see `on_file_completed`).
    read_only: AtomicBool,
    /// Its length when it was closed: a file given again must be as long, or it is not the
    /// one the pieces are in (made again empty, or another one put there).
    closed_len: Mutex<Option<u64>>,
    /// Held while the app gives it again (`Storage::open_one`): once at a time.
    opening: Mutex<()>,
}

impl FileSlot {
    fn new(
        index: usize,
        path: PathBuf,
        file: Option<File>,
        evictable: bool,
        budget: &'static OpenFiles,
    ) -> Arc<Self> {
        let slot = Arc::new(Self {
            index,
            path,
            entry: Entry::new(budget),
            evictable,
            file: RwLock::new(None),
            written: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            read_only: AtomicBool::new(false),
            closed_len: Mutex::new(None),
            opening: Mutex::new(()),
        });
        if let Some(file) = file {
            slot.install(file);
        }
        slot
    }

    /// The handle becomes this one's (none was there), counted in the budget if it may be
    /// closed early.
    fn install(self: &Arc<Self>, file: File) {
        if self.evictable {
            // Counted in (room made) before it is held: never past the budget.
            self.entry.admit(self);
        }
        let mut slot = self.file.write().unwrap_or_else(|e| e.into_inner());
        *slot = Some(file);
        self.read_only.store(false, Ordering::Release);
    }

    /// Moves it, handle and all, to the storage `take()` makes; this one is left without.
    fn take(&self) -> io::Result<Arc<Self>> {
        let file = self
            .file
            .write()
            .map_err(|_| io::Error::other("lock"))?
            .take();
        if file.is_some() && self.evictable {
            self.entry.closed();
        }
        let slot = Arc::new(Self {
            index: self.index,
            path: self.path.clone(),
            entry: Entry::new(self.entry.budget()),
            evictable: self.evictable,
            file: RwLock::new(None),
            written: AtomicBool::new(self.written.swap(false, Ordering::AcqRel)),
            finished: AtomicBool::new(self.finished.load(Ordering::Acquire)),
            read_only: AtomicBool::new(self.read_only.load(Ordering::Acquire)),
            closed_len: Mutex::new(
                *self
                    .closed_len
                    .lock()
                    .map_err(|_| io::Error::other("lock"))?,
            ),
            opening: Mutex::new(()),
        });
        if let Some(file) = file {
            *slot.file.write().map_err(|_| io::Error::other("lock"))? = Some(file);
            if slot.evictable {
                slot.entry.opened(&slot);
            }
        }
        Ok(slot)
    }

    /// Writes what was written to disk and closes the handle, noting the file's length. One the
    /// budget closed after it was written stays `written`: the storage gives it again to write
    /// it to disk ([`Storage::close_files`]).
    fn close(&self) -> io::Result<()> {
        // Taking the lock for writing waited for every read and write still using it.
        let Some(file) = self
            .file
            .write()
            .map_err(|_| io::Error::other("lock"))?
            .take()
        else {
            return Ok(());
        };
        if self.evictable {
            self.entry.closed();
        }
        let len = file.metadata().map(|m| m.len()).ok();
        *self
            .closed_len
            .lock()
            .map_err(|_| io::Error::other("lock"))? = len;
        if self.written.swap(false, Ordering::AcqRel) {
            file.sync_data()?;
        }
        Ok(())
    }

    /// Windows: the handle swapped for a read-only one of the same file, which other programs
    /// may read, run, rename, move and delete while it is seeded (`into_read_only`). Elsewhere
    /// an open file stops none of that, and the handle stays as it is.
    fn hold_read_only(&self) -> io::Result<()> {
        #[cfg(windows)]
        {
            let mut slot = self.file.write().map_err(|_| io::Error::other("lock"))?;
            if self.read_only.load(Ordering::Acquire) {
                return Ok(());
            }
            let Some(file) = slot.take() else {
                return Ok(());
            };
            if self.written.swap(false, Ordering::AcqRel)
                && let Err(e) = file.sync_data()
            {
                *slot = Some(file);
                return Err(e);
            }
            match librqbit::storage::filesystem::into_read_only(file) {
                Ok(read_only) => {
                    *slot = Some(read_only);
                    self.read_only.store(true, Ordering::Release);
                }
                Err((file, e)) => {
                    *slot = Some(file);
                    return Err(e);
                }
            }
        }
        Ok(())
    }
}

impl CachedHandle for FileSlot {
    fn last_used(&self) -> u64 {
        self.entry.last_used()
    }

    /// Closed by the budget when no read or write uses it, its length noted. Not written to
    /// disk here (a sync for each of thousands of files): a pause does that, as it did when
    /// every file was held open.
    fn try_close(&self) -> bool {
        if !self.evictable {
            return false;
        }
        let Ok(mut slot) = self.file.try_write() else {
            return false;
        };
        let Some(file) = slot.as_ref() else {
            return false;
        };
        let len = file.metadata().map(|m| m.len()).ok();
        if let Ok(mut closed) = self.closed_len.lock() {
            *closed = len;
        }
        drop(slot.take());
        self.entry.closed();
        true
    }
}

impl Drop for FileSlot {
    fn drop(&mut self) {
        if self.evictable && self.file.get_mut().is_ok_and(|f| f.is_some()) {
            self.entry.closed();
        }
    }
}

/// Whether a storage's files are open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Open,
    /// Closed while the torrent is paused (`close_files`): given again on their next use, by
    /// the grant.
    Closed,
    /// Moved to the storage `take()` made: this one never has a file again.
    Taken,
}

/// A file is not the one it was when the torrent was paused (moved, deleted, replaced): nothing
/// of it is written, nor read as its pieces.
fn changed_while_paused() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "a file of this torrent was moved, deleted or replaced while it was paused",
    )
}

pub(crate) struct Storage {
    session: PathBuf,
    hash: String,
    files: Vec<Slot>,
    phase: Mutex<Phase>,
    budget: &'static OpenFiles,
}
impl Storage {
    /// `budget`: the open files its handles count in (the process's, but for tests).
    pub(crate) fn new(session: PathBuf, hash: String, budget: &'static OpenFiles) -> Self {
        Self {
            session,
            hash,
            files: Vec::new(),
            phase: Mutex::new(Phase::Open),
            budget,
        }
    }

    fn grant(&self) -> io::Result<Arc<Grant>> {
        grants()
            .lock()
            .unwrap()
            .get(&(self.session.clone(), self.hash.clone()))
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the app gave no files for this torrent",
                )
            })
    }

    fn with_file<T>(
        &self,
        index: usize,
        write: bool,
        padding: impl FnOnce() -> T,
        op: impl FnOnce(&File) -> io::Result<T>,
    ) -> anyhow::Result<T> {
        let slot = self
            .files
            .get(index)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let slot = match slot {
            Slot::Padding => return Ok(padding()),
            Slot::Parts(_) => return Err(io::Error::from(io::ErrorKind::InvalidInput).into()),
            Slot::File(slot) => slot,
        };
        slot.entry.touch();
        let mut op = Some(op);
        for attempt in 0..3 {
            {
                let file = slot
                    .file
                    .read()
                    .map_err(|_| io::Error::from(io::ErrorKind::Other))?;
                if let Some(file) = file.as_ref() {
                    if write {
                        // A finished file is never written: a piece we have is never asked
                        // for again.
                        if slot.read_only.load(Ordering::Acquire) {
                            return Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "a finished file is not written",
                            )
                            .into());
                        }
                        slot.written.store(true, Ordering::Release);
                    }
                    let op = op.take().expect("called once");
                    return Ok(op(file)?);
                }
            }
            match attempt {
                // Paused: the files given again first.
                0 => self.open_again()?,
                // Closed by the budget, or handed over for later: this one given again.
                1 => self.open_one(slot)?,
                _ => {}
            }
        }
        Err(io::Error::from(io::ErrorKind::NotConnected).into())
    }

    /// A file the budget closed after it was written (still `written`, no handle), given again
    /// by the app to be written to disk before the pause's bitfield counts its pieces. One the
    /// app can't give (moved, deleted) has nothing of ours left to write; the next start finds
    /// it changed.
    fn sync_closed(slot: &FileSlot, grant: &Option<Arc<Grant>>) -> io::Result<()> {
        if !slot.written.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(open_one) = grant.as_ref().and_then(|g| g.open_one.as_ref()) else {
            return Ok(());
        };
        let _opening = slot.opening.lock().map_err(|_| io::Error::other("lock"))?;
        match open_one(slot.index) {
            Ok(given) if given.path == slot.path => {
                given.file.sync_data()?;
                slot.written.store(false, Ordering::Release);
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::debug!("a torrent file closed early wasn't given again to sync: {e}");
                slot.written.store(false, Ordering::Release);
                Ok(())
            }
        }
    }

    /// One file given again by the app (`open_one`): it was closed to keep the process within
    /// its budget of open files, or handed over closed (`Prepared::later`). It must be at its
    /// path with the length it had when closed. Without `open_one` (an older grant) a file is
    /// never closed early, and this is an error.
    fn open_one(&self, slot: &Arc<FileSlot>) -> anyhow::Result<()> {
        if *self.phase.lock().map_err(|_| io::Error::other("lock"))? != Phase::Open {
            return Err(io::Error::from(io::ErrorKind::NotConnected).into());
        }
        let grant = self.grant()?;
        let Some(open_one) = grant.open_one.as_ref() else {
            return Err(io::Error::from(io::ErrorKind::NotConnected).into());
        };
        // One opening at a time for this file; another one may have opened it meanwhile.
        let _opening = slot.opening.lock().map_err(|_| io::Error::other("lock"))?;
        if slot
            .file
            .read()
            .map_err(|_| io::Error::other("lock"))?
            .is_some()
        {
            return Ok(());
        }
        let given = open_one(slot.index)?;
        if given.path != slot.path {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "a file was given again at another path",
            )
            .into());
        }
        let len = given.file.metadata()?.len();
        let was = *slot
            .closed_len
            .lock()
            .map_err(|_| io::Error::other("lock"))?;
        if was.is_some_and(|was| was != len) {
            return Err(changed_while_paused().into());
        }
        slot.install(given.file);
        drop(_opening);
        if slot.finished.load(Ordering::Acquire)
            && let Err(e) = slot.hold_read_only()
        {
            tracing::debug!("torrent file kept read/write: {e}");
        }
        Ok(())
    }

    /// The files of a paused torrent given again (it starts, or a read or write came): the
    /// handles the app checked for this start (`Run::start`, which also told whether a file
    /// was swapped), or else from the grant's `reopen`, as a later `init` does. Every file given
    /// must be at its path with the length it had when it was closed; then all are installed at
    /// once. Nothing is made here, and nothing is written into a file that isn't the one it
    /// was. With `open_one`, files not given now are given when first used, one at a time.
    fn open_again(&self) -> anyhow::Result<()> {
        let mut phase = self.phase.lock().map_err(|_| io::Error::other("lock"))?;
        match *phase {
            Phase::Open => return Ok(()),
            Phase::Taken => return Err(io::Error::from(io::ErrorKind::NotConnected).into()),
            Phase::Closed => {}
        }
        let grant = self.grant()?;
        let lazy = grant.open_one.is_some();
        let first = grant.first.lock().unwrap().take();
        let prepared = match first {
            Some(p) => p,
            None if lazy => Prepared::default(),
            None => (grant.reopen)()?,
        };
        let mut given: HashMap<_, _> = prepared.files.into_iter().map(|f| (f.index, f)).collect();
        let mut opened = Vec::new();
        for slot in &self.files {
            let Slot::File(slot) = slot else {
                continue;
            };
            let Some(file) = given.remove(&slot.index) else {
                if lazy {
                    continue;
                }
                return Err(changed_while_paused().into());
            };
            if file.path != slot.path {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "a file was given again at another path",
                )
                .into());
            }
            let len = file.file.metadata()?.len();
            let was = *slot
                .closed_len
                .lock()
                .map_err(|_| io::Error::other("lock"))?;
            if was.is_some_and(|was| was != len) {
                return Err(changed_while_paused().into());
            }
            opened.push((slot, file.file));
        }
        // What is left (files kept in a parts file now) is closed as it goes.
        drop(given);
        for (slot, file) in opened {
            slot.install(file);
            if slot.finished.load(Ordering::Acquire)
                && let Err(e) = slot.hold_read_only()
            {
                tracing::debug!("torrent file kept read/write: {e}");
            }
        }
        *phase = Phase::Open;
        Ok(())
    }
}

impl TorrentStorage for Storage {
    fn init(
        &mut self,
        _shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        let key = (self.session.clone(), self.hash.clone());
        let grant = self.grant()?;
        let first = grant.first.lock().unwrap().take();
        let prepared = match first {
            Some(p) => p,
            None => (grant.reopen)()?,
        };
        let lazy = grant.open_one.is_some();
        let parted_ok: HashSet<usize> = prepared.parted.into_iter().collect();
        let pinned: HashSet<usize> = prepared.pinned.into_iter().collect();
        let mut later: HashMap<_, _> = prepared.later.into_iter().map(|f| (f.index, f)).collect();
        if !later.is_empty() && !lazy {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        let mut prepared: HashMap<_, _> =
            prepared.files.into_iter().map(|f| (f.index, f)).collect();
        let dir = parts_dir(&self.session, &self.hash)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let piece = u64::from(metadata.lengths().default_piece_length());
        let mut opened = Vec::with_capacity(metadata.file_infos.len());
        let mut now_parted = HashSet::new();
        for (index, info) in metadata.file_infos.iter().enumerate() {
            if info.attrs.padding {
                opened.push(Slot::Padding);
                continue;
            }
            let parts = dir.join(index.to_string());
            let evictable = lazy && !pinned.contains(&index);
            let expected = grant.folder.join(&info.relative_filename);
            match (prepared.remove(&index), later.remove(&index)) {
                (Some(file), None) => {
                    if file.path != expected {
                        return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
                    }
                    // Ticked since it was kept in a parts file: what it held goes in.
                    move_parts_into(&parts, &file.file, info.len, piece)?;
                    opened.push(Slot::File(FileSlot::new(
                        index,
                        file.path,
                        Some(file.file),
                        evictable,
                        self.budget,
                    )));
                }
                (None, Some(file)) => {
                    if file.path != expected || !evictable {
                        return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
                    }
                    let slot = FileSlot::new(index, file.path, None, true, self.budget);
                    if parts.is_file() {
                        // Ticked since it was kept in a parts file: opened now for what it held.
                        let given = (grant.open_one.as_ref().expect("lazy"))(index)?;
                        if given.path != slot.path {
                            return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
                        }
                        move_parts_into(&parts, &given.file, info.len, piece)?;
                        slot.install(given.file);
                    }
                    opened.push(Slot::File(slot));
                }
                (None, None) if parted_ok.contains(&index) => {
                    now_parted.insert(index);
                    opened.push(Slot::Parts(Arc::new(Parts::new(
                        parts,
                        info.len,
                        piece,
                        self.budget,
                    ))));
                }
                _ => return Err(io::Error::from(io::ErrorKind::PermissionDenied).into()),
            }
        }
        if !prepared.is_empty() || !later.is_empty() {
            return Err(io::Error::from(io::ErrorKind::InvalidInput).into());
        }
        parted().lock().unwrap().insert(key, now_parted);
        self.files = opened;
        *self.phase.get_mut().map_err(|_| io::Error::other("lock"))? = Phase::Open;
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        match self.files.get(file_id) {
            // Handle padding separately because both closures cannot borrow the output mutably.
            Some(Slot::Padding) => {
                buf.fill(0);
                return Ok(());
            }
            Some(Slot::Parts(p)) => return Ok(p.read(offset, buf)?),
            _ => {}
        }
        self.with_file(file_id, false, || (), |file| read_at(file, offset, buf))
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        if let Some(Slot::Parts(p)) = self.files.get(file_id) {
            let at = p
                .map(offset, buf.len() as u64)
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
            let file = p.open(true)?;
            let file = file.as_ref().ok_or_else(|| io::Error::other("closed"))?;
            p.written.store(true, Ordering::Release);
            return Ok(write_at(file, at, buf)?);
        }
        self.with_file(file_id, true, || (), |file| write_at(file, offset, buf))
    }

    fn ensure_file_length(&self, file_id: usize, length: u64) -> anyhow::Result<()> {
        // A parts file holds what was written and no more.
        if let Some(Slot::Parts(_)) = self.files.get(file_id) {
            return Ok(());
        }
        // As long already (resumed, seeded): nothing to write, nor to sync at the pause.
        let len = self.with_file(
            file_id,
            false,
            || None,
            |file| Ok(Some(file.metadata()?.len())),
        )?;
        if len.is_none_or(|len| len == length) {
            return Ok(());
        }
        self.with_file(
            file_id,
            true,
            || (),
            |file| mdm_engine::disk::resize_download_file(file, length),
        )
    }
    fn remove_file(&self, _: usize, _: &Path) -> anyhow::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied).into())
    }
    fn remove_directory_if_empty(&self, _: &Path) -> anyhow::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied).into())
    }
    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let mut phase = self.phase.lock().map_err(|_| io::Error::other("lock"))?;
        let mut files = Vec::with_capacity(self.files.len());
        for slot in &self.files {
            files.push(match slot {
                Slot::Padding => Slot::Padding,
                Slot::File(slot) => Slot::File(slot.take()?),
                Slot::Parts(p) => {
                    // The parts file is closed; the new storage opens it again when it needs it.
                    p.close()?;
                    Slot::Parts(Arc::new(Parts::new(
                        p.path.clone(),
                        p.len,
                        p.piece,
                        self.budget,
                    )))
                }
            });
        }
        let now = std::mem::replace(&mut *phase, Phase::Taken);
        Ok(Box::new(Self {
            session: self.session.clone(),
            hash: self.hash.clone(),
            files,
            phase: Mutex::new(now),
            budget: self.budget,
        }))
    }

    /// The torrent is paused: every file and parts file written to disk and closed, so that
    /// other programs may move, rename or delete them meanwhile. When it starts again they are
    /// given again by the grant ([`Storage::open_again`]); handles the app checked for a start
    /// this storage never used (paused again before any read or write) are closed too.
    fn close_files(&self) -> anyhow::Result<()> {
        let mut phase = self.phase.lock().map_err(|_| io::Error::other("lock"))?;
        if *phase == Phase::Taken {
            return Ok(());
        }
        let mut first_error = None;
        let grant = self.grant().ok();
        for slot in &self.files {
            let closed = match slot {
                Slot::Padding => Ok(()),
                Slot::File(slot) => slot.close().and_then(|()| Self::sync_closed(slot, &grant)),
                Slot::Parts(p) => p.close(),
            };
            if let Err(e) = closed {
                first_error.get_or_insert(e);
            }
        }
        if *phase == Phase::Closed
            && let Ok(grant) = self.grant()
        {
            drop(grant.first.lock().unwrap().take());
        }
        *phase = Phase::Closed;
        first_error.map_or(Ok(()), |e| Err(e.into()))
    }

    /// Every piece of the file is ours: on Windows it is held read-only from now on (see
    /// [`FileSlot::hold_read_only`]); a paused torrent's files are given again first, as this
    /// is when a finished torrent starts to be shared. A file closed by the budget is held
    /// read-only when it is given again.
    fn on_file_completed(&self, file_id: usize) -> anyhow::Result<()> {
        let Some(Slot::File(slot)) = self.files.get(file_id) else {
            return Ok(());
        };
        slot.finished.store(true, Ordering::Release);
        if cfg!(windows) {
            self.open_again()?;
            slot.hold_read_only()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use librqbit::storage::filesystem::open_files;

    #[test]
    fn parts_keep_the_first_and_last_piece_of_a_long_file() {
        // 10 pieces of 4: the first piece at 0, the last at 4.
        assert_eq!(map_offset(40, 4, 0, 4), Some(0));
        assert_eq!(map_offset(40, 4, 1, 2), Some(1));
        assert_eq!(map_offset(40, 4, 36, 4), Some(4));
        assert_eq!(map_offset(40, 4, 38, 2), Some(6));
        // The middle is never shared with another file: never kept.
        assert_eq!(map_offset(40, 4, 4, 4), None);
        assert_eq!(map_offset(40, 4, 3, 2), None);
        assert_eq!(map_offset(40, 4, 39, 2), None);
        // Two pieces or less: all of it, where it is.
        assert_eq!(map_offset(8, 4, 5, 3), Some(5));
        assert_eq!(map_offset(3, 4, 0, 3), Some(0));
    }

    #[test]
    fn what_parts_dont_hold_reads_as_zeros() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("p");
        // A 40-byte file of 4-byte pieces, nothing written yet: no parts file at all.
        let p = Arc::new(Parts::new(path.clone(), 40, 4, open_files::global()));
        let mut buf = [9u8; 4];
        p.read(0, &mut buf).unwrap();
        assert_eq!(buf, [0; 4]);
        // Its last piece written: kept at 4.
        std::fs::write(&path, b"\0\0\0\0TAIL").unwrap();
        let p = Arc::new(Parts::new(path, 40, 4, open_files::global()));
        p.read(36, &mut buf).unwrap();
        assert_eq!(&buf, b"TAIL");
        // The middle is never kept, and isn't an error: librqbit's check goes on to the end.
        let mut buf = [9u8; 4];
        p.read(20, &mut buf).unwrap();
        assert_eq!(buf, [0; 4]);
    }

    #[test]
    fn ticked_files_get_what_their_parts_file_held() {
        let tmp = tempfile::tempdir().unwrap();
        let parts = tmp.path().join("parts");
        // A 40-byte file of 4-byte pieces: its first piece and its last were written.
        std::fs::write(&parts, b"HEADTAIL").unwrap();
        let path = tmp.path().join("file");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        move_parts_into(&parts, &file, 40, 4).unwrap();
        file.set_len(40).unwrap();
        let got = std::fs::read(&path).unwrap();
        assert_eq!(&got[..4], b"HEAD");
        assert_eq!(&got[36..], b"TAIL");
        assert!(got[4..36].iter().all(|b| *b == 0));
        assert!(!parts.exists());
    }

    #[test]
    fn a_file_with_bytes_is_not_overwritten_from_parts() {
        let tmp = tempfile::tempdir().unwrap();
        let parts = tmp.path().join("parts");
        std::fs::write(&parts, b"zz").unwrap();
        let path = tmp.path().join("file");
        std::fs::write(&path, b"mine").unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        move_parts_into(&parts, &file, 4, 4).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        assert!(!parts.exists());
    }

    #[test]
    fn parts_folders_are_only_for_info_hashes() {
        let s = Path::new("s");
        assert!(parts_dir(s, "../../x").is_none());
        assert_eq!(
            parts_dir(s, &"AB".repeat(20)).unwrap(),
            s.join(format!("{}.parts", "ab".repeat(20)))
        );
    }
}

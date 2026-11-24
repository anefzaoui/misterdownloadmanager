//! Testing a downloaded archive (#1661): every entry read back and checked against its CRC, as
//! `7z t` does, without writing anything. 7-Zip tests ZIP, 7z and RAR (and their volumes) when
//! it is installed; a ZIP is tested in process without it.
//!
//! Where the format says where each entry's bytes are, the damaged ones are named by their place
//! in the file ([`Damage::ranges`]), so a downloader can fetch those bytes again: a ZIP in one
//! file only. Its central directory gives every entry's local header offset; an entry's bytes
//! run from there to the next entry's header (or the central directory), which covers its
//! header, data and data descriptor whatever they claim themselves. The damaged entries are found
//! by MDM's own reader ([`zip_damage`]), and the ranges are given only when it found every entry
//! 7-Zip named (or 7-Zip wasn't there to name any). 7z and RAR keep entries in solid blocks
//! whose places 7-Zip's output doesn't give: those, and split archives, are only said to be
//! damaged.

use std::io::{Read, Seek};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use zip::result::ZipError;

use crate::volumes::{MultiFile, OnDisk, VolumeKind, on_disk, volume_key};
use crate::{Error, Format};

/// What a test found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Every entry read back whole.
    Whole,
    Damaged(Damage),
}

/// A damaged archive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Damage {
    /// The entries found damaged (`/`-separated, as the archive names them); empty when it is
    /// the archive's own structure (its headers, its end) that doesn't read.
    pub entries: Vec<String>,
    /// The bytes to fetch again, `[start, end)` offsets in the file, sorted and apart; None when
    /// the format doesn't say where the damage is, or not reliably.
    pub ranges: Option<Vec<(u64, u64)>>,
    /// What the tester said, its first lines (English, for the log).
    pub said: String,
}

/// How to test one archive.
pub struct Test<'a> {
    /// 7-Zip's program ([`crate::find_7zip`]); None: a ZIP is tested in process, a 7z or RAR
    /// isn't ([`Error::NoTool`]).
    pub tool: Option<&'a Path>,
    /// Set from elsewhere to stop (the download was deleted, MDM is closing).
    pub cancel: Option<&'a AtomicBool>,
    /// How far it is, in thousandths, as it reads.
    pub progress: Option<&'a AtomicU32>,
}

/// Test `archive` (any volume of a split archive: it is tested from its first). `Err`: it wasn't
/// tested ([`Error::NeedsPassword`]: it is encrypted; [`Error::NoTool`]: 7-Zip is needed and
/// isn't there; [`Error::MissingVolume`]: a split archive not complete yet, a volume of it still
/// to come; [`Error::Unsupported`]: not a ZIP, 7z or RAR).
pub fn test(archive: &Path, how: &Test<'_>) -> Result<Verdict, Error> {
    // A set in several files is tested only once every volume is in: before, its end is
    // missing, which reads as damage (#1661).
    let (archive, several) = match on_disk(archive)? {
        OnDisk::Incomplete(n) => return Err(Error::MissingVolume(n)),
        OnDisk::Complete { open, files } => (open, files.len() > 1),
    };
    let archive = archive.as_path();
    let format = Format::detect(archive)?.ok_or(Error::Unsupported)?;
    if !matches!(format, Format::Zip | Format::SevenZip | Format::Rar) {
        return Err(Error::Unsupported);
    }
    let name = archive
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let split = volume_key(&name).is_some_and(|v| v.kind == VolumeKind::Split);
    // Named as one volume of several (x.part1.rar, x.7z.001): 7-Zip's "the archive ends too
    // soon" is a volume still to come, even when the files there look like all of them.
    let several = several || volume_key(&name).is_some_and(|v| v.kind != VolumeKind::RarOld);
    // A spanned ZIP (x.z01… x.zip): only 7-Zip reads it.
    let spanned = format == Format::Zip && several && !split;
    if split {
        MultiFile::open(archive)?.map_err(Error::MissingVolume)?;
    }
    let Some(tool) = how.tool else {
        if format != Format::Zip || spanned {
            return Err(Error::NoTool);
        }
        let found = zip_damage(archive, how.cancel, how.progress)?;
        return found.verdict(split);
    };
    let said = run_7zip(tool, archive, how.cancel, how.progress)?;
    match said {
        Said::Whole => Ok(Verdict::Whole),
        Said::Password => Err(Error::NeedsPassword),
        Said::CantTest(why) => Err(Error::Damaged(why)),
        Said::MissingVolume => Err(Error::MissingVolume(0)),
        Said::Damaged { ended: true, .. } if several => Err(Error::MissingVolume(0)),
        Said::Damaged { entries, said, .. } => {
            // Where the damage is: a ZIP in one file, read by MDM's own reader, which must find
            // every entry 7-Zip named (7-Zip may read a method MDM doesn't).
            let ranges = if format == Format::Zip && !several {
                match zip_damage(archive, how.cancel, how.progress) {
                    Ok(found) if covers(&found.bad, &entries) => found.ranges(),
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    _ => None,
                }
            } else {
                None
            };
            Ok(Verdict::Damaged(Damage {
                entries,
                ranges,
                said,
            }))
        }
    }
}

/// Every entry 7-Zip named is one MDM's reader found damaged, and it named some (an archive-wide
/// error has no entry to place).
fn covers(found: &[Bad], named: &[String]) -> bool {
    !named.is_empty()
        && named
            .iter()
            .all(|n| found.iter().any(|b| b.name.trim_end_matches('/') == n))
}

/// What 7-Zip's `t` said.
#[derive(Debug, PartialEq, Eq)]
enum Said {
    Whole,
    /// Encrypted: nothing can be checked without its password (and none is given).
    Password,
    /// It didn't test it (an unsupported method, its own failure): its words.
    CantTest(String),
    MissingVolume,
    Damaged {
        entries: Vec<String>,
        said: String,
        /// It said the archive ends too soon ("Unexpected end of archive", "... of data"): a
        /// split archive's volume still to come.
        ended: bool,
    },
}

/// Run `7z t` on `archive`: with an empty password (an encrypted archive fails at once instead
/// of waiting for one), no output but errors (on stderr) and its progress (on stdout).
fn run_7zip(
    tool: &Path,
    archive: &Path,
    cancel: Option<&AtomicBool>,
    progress: Option<&AtomicU32>,
) -> Result<Said, Error> {
    let archive = std::path::absolute(archive).unwrap_or_else(|_| archive.to_owned());
    let dir = archive.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut cmd = Command::new(tool);
    cmd.args(["t", "-y", "-bso0", "-bse2", "-bsp1", "-p", "--"])
        .arg(&archive)
        // Its own folder: never the one MDM was started in (its install folder, #834).
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    tracing::info!("archive test: {} t {}", tool.display(), archive.display());
    let mut child = cmd.spawn()?;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let errors = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err.read_to_end(&mut s);
        String::from_utf8_lossy(&s).into_owned()
    });
    // Its progress: "  42% 3 - name", rewritten in place with backspaces and carriage returns.
    let reader = std::thread::scope(|scope| {
        let read = scope.spawn(|| {
            let mut buf = [0u8; 4096];
            let mut tail = String::new();
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                tail.push_str(&String::from_utf8_lossy(&buf[..n]));
                if let (Some(p), Some(pc)) = (progress, last_percent(&tail)) {
                    p.store(pc * 10, Ordering::Relaxed);
                }
                // Only its end matters: the last figure written.
                if tail.len() > 256 {
                    let cut = tail.len() - 64;
                    let cut = (cut..tail.len())
                        .find(|i| tail.is_char_boundary(*i))
                        .unwrap_or(0);
                    tail.drain(..cut);
                }
            }
        });
        let status = loop {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(Ok(status)),
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(e) => {
                    let _ = child.kill();
                    break Some(Err(e));
                }
            }
        };
        let _ = read.join();
        status
    });
    let errors = errors.join().unwrap_or_default();
    let status = match reader {
        None => return Err(Error::Cancelled),
        Some(s) => s?,
    };
    Ok(read_said(status.code(), &errors))
}

/// The last figure in 7-Zip's progress output: each update ("  42% 3 - name", "  5% T name") is
/// written over the one before with carriage returns and backspaces; a name may hold a `%`.
fn last_percent(text: &str) -> Option<u32> {
    text.split(['\r', '\n', '\u{8}']).rev().find_map(|chunk| {
        let chunk = chunk.trim_start();
        let digits = chunk.len() - chunk.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 || !chunk[digits..].starts_with('%') {
            return None;
        }
        chunk[..digits].parse().ok().filter(|n| *n <= 100)
    })
}

/// What 7-Zip's exit `code` and its error lines say (7-Zip 9.20 to 26 word them so; the names
/// of entries after " : ").
fn read_said(code: Option<i32>, errors: &str) -> Said {
    // 0: no error; 1: warnings only (data after the archive's end, which its test passes).
    if matches!(code, Some(0 | 1)) {
        return Said::Whole;
    }
    let first = || {
        errors
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(4)
            .collect::<Vec<_>>()
            .join(" | ")
    };
    // Not a test's verdict: a wrong command line (7), no memory (8), stopped (255).
    if code != Some(2) {
        return Said::CantTest(format!("7-Zip ended with {code:?}: {}", first()));
    }
    let lower = errors.to_lowercase();
    if lower.contains("missing volume") {
        return Said::MissingVolume;
    }
    let mut entries = Vec::new();
    let (mut password, mut unsupported, mut whole_archive) = (false, false, false);
    let ended = lower.contains("unexpected end");
    for line in errors.lines().map(str::trim) {
        let l = line.to_lowercase();
        if l.contains("wrong password") || l.contains("encrypted") {
            password = true;
            continue;
        }
        if l.contains("unsupported method") || l.contains("unsupported feature") {
            unsupported = true;
            continue;
        }
        let entry = line.strip_prefix("ERROR: ").and_then(|rest| {
            const KINDS: [&str; 5] = [
                "CRC Failed : ",
                "Data Error : ",
                "Unexpected end of data : ",
                "Headers Error : ",
                "There are some data after the end of the payload data : ",
            ];
            KINDS.iter().find_map(|k| rest.strip_prefix(k))
        });
        match entry {
            Some(name) => entries.push(name.replace('\\', "/")),
            None if l.contains("headers error")
                || l.contains("unexpected end")
                || l.contains("cannot open")
                || l.contains("is not archive")
                || l.contains("data error")
                || l.contains("crc failed") =>
            {
                whole_archive = true;
            }
            None => {}
        }
    }
    entries.dedup();
    if entries.is_empty() && !whole_archive {
        if password {
            return Said::Password;
        }
        if unsupported {
            return Said::CantTest(first());
        }
    }
    // A damaged encrypted archive's headers ("Cannot open encrypted archive. Wrong password?")
    // read as its password: only damage it can show without one counts.
    if entries.is_empty() && password {
        return Said::Password;
    }
    Said::Damaged {
        entries,
        said: first(),
        ended,
    }
}

/// An entry MDM's reader found damaged.
#[derive(Debug)]
struct Bad {
    name: String,
    /// Its bytes in the file: from its local header to the next entry's (or the central
    /// directory).
    range: (u64, u64),
}

/// What MDM's own reader found in a ZIP.
#[derive(Debug, Default)]
struct ZipFound {
    bad: Vec<Bad>,
    /// Entries it couldn't check: encrypted ones (no password), a method it doesn't read.
    encrypted: bool,
    unread: bool,
    /// Every entry's range is its own (no two at one place, all inside the file): the ranges
    /// can be trusted.
    placed: bool,
}

impl ZipFound {
    /// Without 7-Zip: its verdict. Damage found is damage; none found is whole only when every
    /// entry was checked.
    fn verdict(self, split: bool) -> Result<Verdict, Error> {
        if !self.bad.is_empty() {
            let ranges = if split { None } else { self.ranges() };
            let said = format!(
                "damaged: {}",
                self.bad
                    .iter()
                    .map(|b| b.name.as_str())
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Ok(Verdict::Damaged(Damage {
                entries: self.bad.into_iter().map(|b| b.name).collect(),
                ranges,
                said,
            }));
        }
        if self.encrypted {
            return Err(Error::NeedsPassword);
        }
        if self.unread {
            return Err(Error::NoTool);
        }
        Ok(Verdict::Whole)
    }

    /// The damaged entries' ranges, sorted and merged where they touch.
    fn ranges(&self) -> Option<Vec<(u64, u64)>> {
        if !self.placed || self.bad.is_empty() {
            return None;
        }
        let mut r: Vec<(u64, u64)> = self.bad.iter().map(|b| b.range).collect();
        r.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(r.len());
        for (s, e) in r {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        Some(merged)
    }
}

/// Read every entry of the ZIP at `archive` (a split one's first part) and check it (its CRC,
/// its compressed data), without writing it anywhere. An archive whose central directory doesn't
/// read is damaged as a whole.
fn zip_damage(
    archive: &Path,
    cancel: Option<&AtomicBool>,
    progress: Option<&AtomicU32>,
) -> Result<ZipFound, Error> {
    let name = archive
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if volume_key(&name).is_some_and(|v| v.kind == VolumeKind::Split) {
        let multi = MultiFile::open(archive)?.map_err(Error::MissingVolume)?;
        read_zip(multi, cancel, progress)
    } else {
        read_zip(std::fs::File::open(archive)?, cancel, progress)
    }
}

fn read_zip<R: Read + Seek>(
    mut reader: R,
    cancel: Option<&AtomicBool>,
    progress: Option<&AtomicU32>,
) -> Result<ZipFound, Error> {
    let len = reader.seek(std::io::SeekFrom::End(0))?;
    reader.seek(std::io::SeekFrom::Start(0))?;
    let mut zip = match zip::ZipArchive::new(reader) {
        Ok(z) => z,
        Err(ZipError::Io(e)) if e.kind() != std::io::ErrorKind::UnexpectedEof => {
            return Err(Error::Io(e));
        }
        // Its directory doesn't read: damaged as a whole, nowhere to place it.
        Err(_) => {
            return Ok(ZipFound {
                bad: vec![Bad {
                    name: String::new(),
                    range: (0, len),
                }],
                placed: false,
                ..Default::default()
            });
        }
    };
    // Each entry's bytes: from its local header to the next one's, or to the directory.
    let dir = zip.central_directory_start();
    let mut starts: Vec<(u64, usize)> = (0..zip.len())
        .filter_map(|i| zip.by_index_data(i).ok().map(|e| (e.header_start(), i)))
        .collect();
    starts.sort_unstable();
    let mut placed = starts.len() == zip.len()
        && starts.windows(2).all(|w| w[0].0 < w[1].0)
        && starts.last().is_none_or(|(s, _)| *s < dir)
        && dir <= len;
    let mut range = vec![(0u64, 0u64); zip.len()];
    for (k, (start, i)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(dir, |(s, _)| *s);
        if end <= *start {
            placed = false;
        }
        range[*i] = (*start, end);
    }
    let total: u64 = (0..zip.len())
        .filter_map(|i| zip.by_index_data(i).ok().map(|e| e.compressed_size()))
        .sum::<u64>()
        .max(1);
    let mut done = 0u64;
    let mut found = ZipFound {
        placed,
        ..Default::default()
    };
    let mut buf = vec![0u8; 64 * 1024];
    for (i, &range) in range.iter().enumerate() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        let (name, encrypted, csize) = match zip.by_index_data(i) {
            Ok(e) => (
                e.name().map(|n| n.into_owned()).unwrap_or_default(),
                e.encrypted(),
                e.compressed_size(),
            ),
            Err(_) => (String::new(), false, 0),
        };
        if encrypted {
            found.encrypted = true;
            done += csize;
            continue;
        }
        let bad = |found: &mut ZipFound| {
            found.bad.push(Bad {
                name: name.clone(),
                range,
            })
        };
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            // A method it doesn't read (LZMA, bzip2, zstd in a zip): not checked here.
            Err(ZipError::UnsupportedArchive(m)) if m != ZipError::PASSWORD_REQUIRED => {
                found.unread = true;
                done += csize;
                continue;
            }
            Err(ZipError::UnsupportedArchive(_)) => {
                found.encrypted = true;
                done += csize;
                continue;
            }
            // Its local header doesn't read.
            Err(_) => {
                bad(&mut found);
                done += csize;
                continue;
            }
        };
        // Read to its end: the reader checks the CRC there.
        let ok = loop {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(Error::Cancelled);
            }
            match entry.read(&mut buf) {
                Ok(0) => break true,
                Ok(_) => {}
                Err(_) => break false,
            }
        };
        drop(entry);
        if !ok {
            bad(&mut found);
        }
        done += csize;
        if let Some(p) = progress {
            p.store(((done.min(total) * 1000) / total) as u32, Ordering::Relaxed);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sevens_words_read_as_verdicts() {
        assert_eq!(read_said(Some(0), ""), Said::Whole);
        assert_eq!(
            read_said(
                Some(1),
                "WARNINGS:\nThere are data after the end of archive"
            ),
            Said::Whole
        );
        assert_eq!(
            read_said(Some(2), "ERROR: CRC Failed : dir\\b.bin\n"),
            Said::Damaged {
                entries: vec!["dir/b.bin".into()],
                said: "ERROR: CRC Failed : dir\\b.bin".into(),
                ended: false,
            }
        );
        let Said::Damaged { entries, ended, .. } = read_said(
            Some(2),
            "\nERRORS:\nUnexpected end of archive\n\nERROR: Data Error : dir\\b.bin\n",
        ) else {
            panic!("damaged")
        };
        assert_eq!(entries, ["dir/b.bin"]);
        assert!(ended, "cut short: a split one's volume still to come");
        // Not an archive at all: damaged, no entry to place.
        assert_eq!(
            read_said(
                Some(2),
                "ERROR: x.zip\nx.zip\nOpen ERROR: Cannot open the file as [zip] archive\n\nERRORS:\nIs not archive\n"
            ),
            Said::Damaged {
                entries: vec![],
                said: "ERROR: x.zip | x.zip | Open ERROR: Cannot open the file as [zip] archive | ERRORS:".into(),
                ended: false,
            }
        );
        // Encrypted: not tested, not damaged.
        assert_eq!(
            read_said(Some(2), "ERROR: Wrong password : a.txt\n"),
            Said::Password
        );
        assert_eq!(
            read_said(
                Some(2),
                "ERROR: enc.7z\nCannot open encrypted archive. Wrong password?\n\nERRORS:\nHeaders Error\n"
            ),
            Said::Password
        );
        assert!(matches!(
            read_said(Some(2), "ERROR: Unsupported Method : a.bin\n"),
            Said::CantTest(_)
        ));
        assert_eq!(
            read_said(Some(2), "ERROR: Missing volume : x.7z.003\n"),
            Said::MissingVolume
        );
        assert!(matches!(
            read_said(Some(8), "out of memory"),
            Said::CantTest(_)
        ));
        assert!(matches!(read_said(None, ""), Said::CantTest(_)));
    }

    #[test]
    fn its_progress_is_the_last_figure() {
        assert_eq!(
            last_percent("  0M Scan\r   \r  0%\r  \r 42% 3 - a.txt"),
            Some(42)
        );
        assert_eq!(last_percent("\u{8}\u{8}100%"), Some(100));
        assert_eq!(last_percent("no figure"), None);
        assert_eq!(last_percent("999%"), None);
        // A name with a figure and a % in it isn't the progress.
        assert_eq!(
            last_percent("  5% T big.txt\u{8}\u{8} 34% T 50% off.txt  "),
            Some(34)
        );
    }

    fn noise(n: usize, mut x: u64) -> Vec<u8> {
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    /// A ZIP of three entries, the middle one incompressible; with `aes`, encrypted. Each
    /// entry's name and local header offset.
    fn zip_of(path: &Path, aes: Option<&str>) -> Vec<(String, u64)> {
        use std::io::Write;
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let entries: [(&str, Vec<u8>); 3] = [
            ("a.txt", b"alpha ".repeat(2000)),
            ("dir/b.bin", noise(200_000, 0x9E37_79B9_7F4A_7C15)),
            ("c.txt", b"gamma ".repeat(3000)),
        ];
        for (name, data) in &entries {
            let opts = zip::write::SimpleFileOptions::default();
            let opts = match aes {
                Some(p) => opts.with_aes_encryption(zip::AesMode::Aes256, p),
                None => opts,
            };
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        (0..zip.len())
            .map(|i| {
                let e = zip.by_index_raw(i).unwrap();
                (e.name().unwrap().into_owned(), e.header_start())
            })
            .collect()
    }

    /// Flip `n` bytes at `at`.
    fn spoil(path: &Path, at: u64, n: usize) {
        let mut data = std::fs::read(path).unwrap();
        for b in &mut data[at as usize..at as usize + n] {
            *b ^= 0xFF;
        }
        std::fs::write(path, data).unwrap();
    }

    fn tested(path: &Path, tool: Option<&Path>) -> Result<Verdict, Error> {
        let progress = AtomicU32::new(0);
        let r = test(
            path,
            &Test {
                tool,
                cancel: None,
                progress: Some(&progress),
            },
        );
        if matches!(r, Ok(Verdict::Whole)) && tool.is_none() {
            assert_eq!(progress.load(Ordering::Relaxed), 1000, "read to its end");
        }
        r
    }

    /// The 7-Zip here, and none: each way a good ZIP is whole, and a damaged entry is found
    /// and placed from its local header to the next entry's.
    #[test]
    fn a_damaged_zip_entry_is_found_and_placed() {
        let t = tempfile::tempdir().unwrap();
        let good = t.path().join("good.zip");
        let heads = zip_of(&good, None);
        let size = std::fs::metadata(&good).unwrap().len();
        let bad = t.path().join("bad.zip");
        std::fs::copy(&good, &bad).unwrap();
        // In b.bin's data, well past its header.
        spoil(&bad, heads[1].1 + 5_000, 64);
        let dir = zip::ZipArchive::new(std::fs::File::open(&good).unwrap())
            .unwrap()
            .central_directory_start();
        assert!(heads[2].1 < dir && dir < size);
        let tools: Vec<Option<std::path::PathBuf>> = vec![None, crate::find_7zip()];
        for tool in tools.iter().map(Option::as_deref) {
            assert_eq!(tested(&good, tool).unwrap(), Verdict::Whole, "{tool:?}");
            let Verdict::Damaged(d) = tested(&bad, tool).unwrap() else {
                panic!("damaged ({tool:?})")
            };
            assert_eq!(d.entries, ["dir/b.bin"], "{tool:?}");
            assert_eq!(d.ranges, Some(vec![(heads[1].1, heads[2].1)]), "{tool:?}");
            assert!(!d.said.is_empty());
        }
        // The last entry's range ends at the central directory.
        let last = t.path().join("last.zip");
        std::fs::copy(&good, &last).unwrap();
        spoil(&last, heads[2].1 + 60, 8);
        for tool in tools.iter().map(Option::as_deref) {
            let Verdict::Damaged(d) = tested(&last, tool).unwrap() else {
                panic!("damaged ({tool:?})")
            };
            assert_eq!(d.ranges, Some(vec![(heads[2].1, dir)]), "{tool:?}");
        }
        // Two damaged entries side by side: one range.
        let two = t.path().join("two.zip");
        std::fs::copy(&bad, &two).unwrap();
        spoil(&two, heads[2].1 + 60, 8);
        for tool in tools.iter().map(Option::as_deref) {
            let Verdict::Damaged(d) = tested(&two, tool).unwrap() else {
                panic!("damaged ({tool:?})")
            };
            assert_eq!(d.ranges, Some(vec![(heads[1].1, dir)]), "{tool:?}");
        }
    }

    /// Its central directory spoilt: damaged, and nowhere to place it (Redownload only).
    #[test]
    fn a_zip_whose_directory_is_spoilt_has_no_ranges() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("dir.zip");
        zip_of(&p, None);
        let len = std::fs::metadata(&p).unwrap().len();
        // The end of central directory record's signature.
        spoil(&p, len - 22, 4);
        for tool in [None, crate::find_7zip()] {
            match tested(&p, tool.as_deref()) {
                Ok(Verdict::Damaged(d)) => assert_eq!(d.ranges, None, "{tool:?}"),
                other => panic!("{other:?} ({tool:?})"),
            }
        }
    }

    /// Encrypted, no password given: not tested (never called damaged).
    #[test]
    fn an_encrypted_zip_is_not_tested() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("enc.zip");
        zip_of(&p, Some("s3cret"));
        for tool in [None, crate::find_7zip()] {
            assert!(
                matches!(tested(&p, tool.as_deref()), Err(Error::NeedsPassword)),
                "{tool:?}"
            );
        }
    }

    /// Not a ZIP, 7z or RAR: not tested; a 7z without 7-Zip: needs it.
    #[test]
    fn only_zip_7z_and_rar_are_tested() {
        let t = tempfile::tempdir().unwrap();
        let page = t.path().join("page.zip");
        std::fs::write(&page, b"<html>no archive</html>").unwrap();
        assert!(matches!(tested(&page, None), Err(Error::Unsupported)));
        let gz = t.path().join("a.gz");
        std::fs::write(&gz, [0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3]).unwrap();
        assert!(matches!(tested(&gz, None), Err(Error::Unsupported)));
        let sz = t.path().join("a.7z");
        std::fs::write(&sz, [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4]).unwrap();
        assert!(matches!(tested(&sz, None), Err(Error::NoTool)));
    }

    /// A split ZIP's damage isn't placed (its bytes are in several files, each its own
    /// download); a volume missing is said.
    #[test]
    fn a_split_zip_is_tested_but_not_placed() {
        let t = tempfile::tempdir().unwrap();
        let whole = t.path().join("w.zip");
        let heads = zip_of(&whole, None);
        spoil(&whole, heads[1].1 + 5_000, 16);
        let data = std::fs::read(&whole).unwrap();
        let (a, b) = (data.len() / 3, data.len() * 2 / 3);
        let one = t.path().join("s.zip.001");
        std::fs::write(&one, &data[..a]).unwrap();
        std::fs::write(t.path().join("s.zip.002"), &data[a..b]).unwrap();
        std::fs::write(t.path().join("s.zip.003"), &data[b..]).unwrap();
        for tool in [None, crate::find_7zip()] {
            let Verdict::Damaged(d) = tested(&one, tool.as_deref()).unwrap() else {
                panic!("damaged ({tool:?})")
            };
            assert_eq!(d.ranges, None, "{tool:?}");
        }
        // Not complete yet (the last volume to come, or one in the middle): never damage, with
        // 7-Zip as without; asked from a later volume, it is the set's.
        let last = std::fs::read(t.path().join("s.zip.003")).unwrap();
        std::fs::remove_file(t.path().join("s.zip.003")).unwrap();
        for tool in [None, crate::find_7zip()] {
            let r = tested(&t.path().join("s.zip.002"), tool.as_deref());
            assert!(
                matches!(r, Err(Error::MissingVolume(3))),
                "{r:?} ({tool:?})"
            );
        }
        std::fs::write(t.path().join("s.zip.003"), last).unwrap();
        std::fs::remove_file(t.path().join("s.zip.002")).unwrap();
        for tool in [None, crate::find_7zip()] {
            let r = tested(&one, tool.as_deref());
            assert!(
                matches!(r, Err(Error::MissingVolume(2))),
                "{r:?} ({tool:?})"
            );
        }
    }

    /// A spanned ZIP as Info-ZIP's `zip -s` writes it (`x.z01`, `x.z02`… `x.zip`) from the ZIP at
    /// `whole`, in volumes of `vol` bytes: its first starts with the spanning mark, its central
    /// directory (in the last) names each entry's volume and its offset there.
    fn span(whole: &Path, dir: &Path, stem: &str, vol: usize) -> Vec<std::path::PathBuf> {
        let data = std::fs::read(whole).unwrap();
        let eocd = data.len() - 22;
        assert_eq!(&data[eocd..eocd + 4], b"PK\x05\x06");
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let entries = u16_at(&data, eocd + 10);
        let (cd_size, cd_off) = (u32_at(&data, eocd + 12), u32_at(&data, eocd + 16));
        let mut local = b"PK\x07\x08".to_vec();
        local.extend_from_slice(&data[..cd_off as usize]);
        let mut cd = data[cd_off as usize..(cd_off + cd_size) as usize].to_vec();
        let mut pieces: Vec<Vec<u8>> = local.chunks(vol).map(<[u8]>::to_vec).collect();
        let mut p = 0;
        for _ in 0..entries {
            assert_eq!(&cd[p..p + 4], b"PK\x01\x02");
            let lens = u16_at(&cd, p + 28) + u16_at(&cd, p + 30) + u16_at(&cd, p + 32);
            let off = u32_at(&cd, p + 42) as usize + 4;
            cd[p + 34..p + 36].copy_from_slice(&((off / vol) as u16).to_le_bytes());
            cd[p + 42..p + 46].copy_from_slice(&((off % vol) as u32).to_le_bytes());
            p += 46 + lens;
        }
        let last = pieces.len() - 1;
        let at = pieces[last].len() as u32;
        let mut end = b"PK\x05\x06".to_vec();
        for f in [last, last, entries, entries] {
            end.extend_from_slice(&(f as u16).to_le_bytes());
        }
        end.extend_from_slice(&cd_size.to_le_bytes());
        end.extend_from_slice(&at.to_le_bytes());
        end.extend_from_slice(&[0, 0]);
        pieces[last].extend_from_slice(&cd);
        pieces[last].extend_from_slice(&end);
        pieces
            .iter()
            .enumerate()
            .map(|(k, piece)| {
                let name = if k == last {
                    format!("{stem}.zip")
                } else {
                    format!("{stem}.z{:02}", k + 1)
                };
                let path = dir.join(name);
                std::fs::write(&path, piece).unwrap();
                path
            })
            .collect()
    }

    /// A spanned ZIP: tested by 7-Zip from its first volume once every one is in, from any of
    /// them; its damage said; a volume missing is no damage; without 7-Zip, not tested.
    #[test]
    fn a_spanned_zip_is_tested_once_every_volume_is_in() {
        let t = tempfile::tempdir().unwrap();
        let whole = t.path().join("w.zip");
        let heads = zip_of(&whole, None);
        let dir = t.path().join("dl");
        std::fs::create_dir(&dir).unwrap();
        let files = span(&whole, &dir, "sv", 90_000);
        assert_eq!(files.len(), 3, "{files:?}");
        assert!(matches!(tested(&files[0], None), Err(Error::NoTool)));
        let Some(tool) = crate::find_7zip() else {
            eprintln!("7-Zip not found: skipped");
            return;
        };
        for f in &files {
            assert_eq!(tested(f, Some(&tool)).unwrap(), Verdict::Whole, "{f:?}");
        }
        let middle = std::fs::read(&files[1]).unwrap();
        std::fs::remove_file(&files[1]).unwrap();
        let r = tested(&files[2], Some(&tool));
        assert!(matches!(r, Err(Error::MissingVolume(2))), "{r:?}");
        std::fs::write(&files[1], middle).unwrap();
        // b.bin's data, in the second volume: damaged, not placed.
        let at = heads[1].1 as usize + 4 + 100_000 - 90_000;
        spoil(&files[1], at as u64, 32);
        let Verdict::Damaged(d) = tested(&files[0], Some(&tool)).unwrap() else {
            panic!("damaged")
        };
        assert_eq!((d.entries, d.ranges), (vec!["dir/b.bin".to_string()], None));
    }

    /// 7z through 7-Zip: whole, or damaged without a place (its entries are in solid blocks).
    /// Stopped, 7-Zip is ended, not waited for.
    #[test]
    fn a_damaged_7z_is_said_without_ranges() {
        let Some(tool) = crate::find_7zip() else {
            eprintln!("7-Zip not found: skipped");
            return;
        };
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"alpha ".repeat(5000)).unwrap();
        std::fs::write(src.join("b.bin"), noise(300_000, 7)).unwrap();
        let good = t.path().join("g.7z");
        let ok = Command::new(&tool)
            .args(["a", "-bso0", "-bsp0", "-mx1"])
            .arg(&good)
            .arg(src.join("*"))
            .status()
            .unwrap()
            .success();
        assert!(ok);
        assert_eq!(tested(&good, Some(&tool)).unwrap(), Verdict::Whole);
        let bad = t.path().join("b.7z");
        std::fs::copy(&good, &bad).unwrap();
        spoil(&bad, 2_000, 64);
        let Verdict::Damaged(d) = tested(&bad, Some(&tool)).unwrap() else {
            panic!("damaged")
        };
        assert_eq!(d.ranges, None);
        let stop = AtomicBool::new(true);
        let how = Test {
            tool: Some(&tool),
            cancel: Some(&stop),
            progress: None,
        };
        assert!(matches!(test(&good, &how), Err(Error::Cancelled)));
    }
}

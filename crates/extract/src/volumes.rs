//! Archives in several files (#37): which file is which volume of what, and reading a byte-split
//! archive (`x.7z.001`, `.002`…) as one file.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// `name` without the ASCII suffix that starts at `at` in `lower` (`name`'s lowercase): the
/// suffix ends `name` the same number of bytes from its end. Indexing `name` with an offset
/// measured in `lower` is wrong wherever lowercasing changes a character's byte length (`İ` is
/// 2 bytes, its lowercase `i̇` is 3): it slices into the wrong place or panics.
fn without_lower_suffix<'a>(name: &'a str, lower: &str, at: usize) -> &'a str {
    let suffix = lower.len() - at;
    name.get(..name.len() - suffix).unwrap_or(name)
}

/// How an archive is split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKind {
    /// `x.part1.rar`, `x.part2.rar`… (RAR 3 and later).
    RarPart,
    /// `x.rar`, `x.r00`, `x.r01`… (older RAR): `x.rar` is volume 1.
    RarOld,
    /// `x.7z.001`, `x.zip.001`…: the archive cut in pieces, byte for byte.
    Split,
}

/// A file's place in a multi-volume archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// The name without its volume suffix and archive extension (`movie` for
    /// `movie.part2.rar`): volumes of one archive share it.
    pub base: String,
    pub kind: VolumeKind,
    /// 1-based.
    pub index: u32,
}

impl Volume {
    /// The volume extraction starts from.
    pub fn is_first(&self) -> bool {
        self.index == 1
    }
}

/// The volume `name` is, if it's one (case doesn't matter). A lone `x.rar` is volume 1 of an
/// old-style set, which may have no other volume.
pub fn volume_key(name: &str) -> Option<Volume> {
    let lower = name.to_lowercase();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // x.partN.rar
    if let Some(stem) = lower.strip_suffix(".rar")
        && let Some(i) = stem.rfind(".part")
        && digits(&stem[i + 5..])
    {
        return Some(Volume {
            base: without_lower_suffix(name, &lower, i).to_owned(),
            kind: VolumeKind::RarPart,
            index: stem[i + 5..].parse().ok()?,
        });
    }
    // x.rar, x.r00 → 1, 2
    if lower.ends_with(".rar") {
        return Some(Volume {
            base: without_lower_suffix(name, &lower, lower.len() - 4).to_owned(),
            kind: VolumeKind::RarOld,
            index: 1,
        });
    }
    if let Some(i) = lower.rfind(".r")
        && lower.len() - i == 4
        && digits(&lower[i + 2..])
    {
        return Some(Volume {
            base: without_lower_suffix(name, &lower, i).to_owned(),
            kind: VolumeKind::RarOld,
            index: lower[i + 2..].parse::<u32>().ok()? + 2,
        });
    }
    // x.7z.001, x.zip.001
    for ext in [".7z.", ".zip."] {
        if let Some(i) = lower.rfind(ext) {
            let num = &lower[i + ext.len()..];
            if num.len() == 3 && digits(num) {
                return Some(Volume {
                    base: without_lower_suffix(name, &lower, i).to_owned(),
                    kind: VolumeKind::Split,
                    index: num.parse().ok()?,
                });
            }
        }
    }
    None
}

/// A byte-split archive's parts, read as one file: `first` is `x.7z.001`; `.002`, `.003`… follow
/// as long as they exist.
pub struct MultiFile {
    parts: Vec<(File, u64)>,
    total: u64,
    pos: u64,
}

impl MultiFile {
    pub fn open(first: &Path) -> io::Result<Result<MultiFile, u32>> {
        Self::of(&split_parts(first))
    }

    /// These parts, in order, read as one file; `Err(n)`: part `n` (1-based) isn't there.
    pub fn of(parts_paths: &[PathBuf]) -> io::Result<Result<MultiFile, u32>> {
        let mut parts = Vec::new();
        let mut total = 0;
        for (n, p) in parts_paths.iter().enumerate() {
            match File::open(p) {
                Ok(f) => {
                    let len = f.metadata()?.len();
                    total += len;
                    parts.push((f, len));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Ok(Err(n as u32 + 1));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Ok(MultiFile {
            parts,
            total,
            pos: 0,
        }))
    }
}

/// A spanned ZIP's place (Info-ZIP's `zip -s`, WinZip's split ZIP): `x.z01`, `x.z02`… then
/// `x.zip`, the last volume, whose end record says how many there are. `name`'s base (`x`) and
/// its volume number, None for the `.zip` (a ZIP in one file reads the same by name).
fn zip_span(name: &str) -> Option<(&str, Option<u32>)> {
    let lower = name.to_lowercase();
    if lower.ends_with(".zip") && lower.len() > 4 {
        return Some((without_lower_suffix(name, &lower, lower.len() - 4), None));
    }
    let i = lower.rfind(".z")?;
    let num = &lower[i + 2..];
    if i == 0 || num.len() < 2 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = num.parse().ok().filter(|n| *n > 0)?;
    Some((without_lower_suffix(name, &lower, i), Some(n)))
}

/// What every volume of `name`'s set shares (lowercase): its base, and a byte-split one's
/// extension too (`x.7z.001` and `x.zip.001` are two sets).
fn set_base(name: &str, v: &Volume) -> String {
    let lower = name.to_lowercase();
    match v.kind {
        VolumeKind::Split => lower[..lower.len() - 3].to_owned(),
        _ => v.base.to_lowercase(),
    }
}

/// Is `name` a volume of a spanned ZIP before its last (`x.z01`, `x.z02`…)?
pub fn spanned_part(name: &str) -> bool {
    zip_span(name).is_some_and(|(_, n)| n.is_some())
}

/// What the volumes of one archive share, by name (case doesn't matter): two files with the same
/// key are volumes of one set. None: no volume of a set by its name (`x.7z`, `x.tar`); a `.zip`
/// has one, a spanned ZIP's last volume reading the same as a ZIP in one file.
pub fn set_key(name: &str) -> Option<String> {
    if let Some(v) = volume_key(name) {
        return Some(format!("{:?}/{}", v.kind, set_base(name, &v)));
    }
    zip_span(name).map(|(base, _)| format!("ZipSpan/{}", base.to_lowercase()))
}

/// Whether every volume of an archive is on disk (#1661), from the files in its folder and what
/// its last volume says of itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnDisk {
    /// Every volume is there (an archive in one file: itself). `open`: the first volume, the
    /// file an archive tool is given; `files`: every volume's, in order.
    Complete { open: PathBuf, files: Vec<PathBuf> },
    /// Not yet: volume `n` (1-based) isn't there, or 0 when which isn't known (a spanned ZIP's
    /// last). Its other volumes are still to come: never damage.
    Incomplete(u32),
}

/// The files in `dir` (names and paths).
fn files_in(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_type().is_ok_and(|t| t.is_file()) {
            out.push((e.file_name().to_string_lossy().into_owned(), e.path()));
        }
    }
    Ok(out)
}

/// The first number missing from `found`'s (sorted) 1, 2, 3…, if any.
fn gap(found: &[(u32, PathBuf)]) -> Option<u32> {
    found
        .iter()
        .enumerate()
        .find(|(k, (n, _))| *n != *k as u32 + 1)
        .map(|(k, _)| k as u32 + 1)
}

/// Is every volume of the archive `path` is part of on disk? Its folder's files are matched by
/// name (never a name built to open), then the last one's own bytes say whether more is to come:
/// a 7z's start header gives its whole size, a ZIP's end record is at its very end (and a
/// spanned one's says how many volumes it has), a RAR volume's end header says whether another
/// follows. A last volume that says nothing (another format, encrypted headers, an old RAR) is
/// taken as the last when none is missing before it. An archive in one file is complete as it
/// is.
pub fn on_disk(path: &Path) -> io::Result<OnDisk> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new(""));
    let single = || OnDisk::Complete {
        open: path.to_path_buf(),
        files: vec![path.to_path_buf()],
    };
    if let Some(v) = volume_key(&name) {
        let base = set_base(&name, &v);
        let mut found: Vec<(u32, PathBuf)> = files_in(dir)?
            .into_iter()
            .filter_map(|(n, p)| {
                let w = volume_key(&n)?;
                (w.kind == v.kind && set_base(&n, &w) == base).then_some((w.index, p))
            })
            .collect();
        found.sort();
        found.dedup_by_key(|(n, _)| *n);
        if found.is_empty() {
            return Ok(OnDisk::Incomplete(v.index));
        }
        if let Some(n) = gap(&found) {
            return Ok(OnDisk::Incomplete(n));
        }
        let files: Vec<PathBuf> = found.into_iter().map(|(_, p)| p).collect();
        let more = match v.kind {
            VolumeKind::Split => split_more(&files)?,
            VolumeKind::RarPart | VolumeKind::RarOld => rar_more(files.last().expect("one"))?,
        };
        if more == Some(true) {
            return Ok(OnDisk::Incomplete(files.len() as u32 + 1));
        }
        return Ok(OnDisk::Complete {
            open: files[0].clone(),
            files,
        });
    }
    let Some((base, own)) = zip_span(&name) else {
        return Ok(single());
    };
    let (base, is_last) = (base.to_lowercase(), own.is_none());
    // How many volumes a `.zip` says it has: 1 for a ZIP in one file, and for what doesn't read
    // as one (its test says what it is).
    let disks_of = |p: &Path| -> io::Result<u32> {
        match File::open(p) {
            Ok(mut f) => Ok(zip_end(&mut f)?.map_or(1, |e| e.disks)),
            Err(_) => Ok(1),
        }
    };
    // A ZIP in one file: its folder isn't looked at.
    if is_last && disks_of(path)? <= 1 {
        return Ok(single());
    }
    let mut parts: Vec<(u32, PathBuf)> = Vec::new();
    let mut last = None;
    for (n, p) in files_in(dir)? {
        match zip_span(&n) {
            Some((b, Some(k))) if b.to_lowercase() == base => parts.push((k, p)),
            Some((b, None)) if b.to_lowercase() == base => last = Some(p),
            _ => {}
        }
    }
    // A part (x.z01) whose last volume isn't in yet, or beside a ZIP in one file (a stray one,
    // no volume of it).
    let Some(last) = last else {
        return Ok(OnDisk::Incomplete(0));
    };
    let disks = disks_of(&last)?;
    if disks <= 1 {
        return Ok(if is_last {
            single()
        } else {
            OnDisk::Incomplete(0)
        });
    }
    parts.sort();
    parts.dedup_by_key(|(n, _)| *n);
    parts.retain(|(n, _)| *n < disks);
    if let Some(n) = gap(&parts) {
        return Ok(OnDisk::Incomplete(n));
    }
    if (parts.len() as u32) < disks - 1 {
        return Ok(OnDisk::Incomplete(parts.len() as u32 + 1));
    }
    let mut files: Vec<PathBuf> = parts.into_iter().map(|(_, p)| p).collect();
    files.push(last);
    Ok(OnDisk::Complete {
        open: files[0].clone(),
        files,
    })
}

/// A ZIP's end of central directory record, found at the end of what `r` reads.
struct ZipEnd {
    /// How many volumes it says it has (a ZIP in one file: 1).
    disks: u32,
}

/// The ZIP end record at the end of `r`: its signature at most 64 KiB (its comment's longest)
/// before the end, and either its comment running exactly to the end or its central directory
/// just before it (data after the archive). None: no ZIP ends there (it is cut short, or more is
/// to come).
fn zip_end<R: Read + Seek>(r: &mut R) -> io::Result<Option<ZipEnd>> {
    const MIN: usize = 22;
    let len = r.seek(SeekFrom::End(0))?;
    if len < MIN as u64 {
        return Ok(None);
    }
    let take = len.min(MIN as u64 + 0xFFFF);
    r.seek(SeekFrom::Start(len - take))?;
    let mut tail = vec![0u8; take as usize];
    r.read_exact(&mut tail)?;
    let at = |i: usize, n: usize| -> u64 {
        tail[i..i + n]
            .iter()
            .rev()
            .fold(0u64, |acc, x| (acc << 8) | *x as u64)
    };
    for p in (0..=tail.len() - MIN).rev() {
        if &tail[p..p + 4] != b"PK\x05\x06" {
            continue;
        }
        let comment = at(p + 20, 2) as usize;
        let (cd_size, cd_off) = (at(p + 12, 4), at(p + 16, 4));
        let pos = len - take + p as u64;
        let zip64 = p >= 20 && &tail[p - 20..p - 16] == b"PK\x06\x07";
        let to_end = p + MIN + comment == tail.len();
        // Data after it: its directory ends where the record starts (offsets in the first
        // volume, so a ZIP in one file or a byte-split one; a zip64 one by its locator).
        let fits = zip64 || (cd_off != 0xFFFF_FFFF && cd_off + cd_size == pos);
        if !to_end && !fits {
            continue;
        }
        let disk = at(p + 4, 2) as u32;
        let disks = if zip64 && disk == 0xFFFF {
            at(p - 20 + 16, 4) as u32
        } else {
            disk + 1
        };
        return Ok(Some(ZipEnd { disks }));
    }
    Ok(None)
}

/// A byte-split archive's parts (`x.7z.001`…) on disk: is more to come? A 7z says how long it
/// is in its start header; a ZIP ends with its end record. None: another format, which doesn't
/// say.
fn split_more(files: &[PathBuf]) -> io::Result<Option<bool>> {
    let mut whole = match MultiFile::of(files)? {
        Ok(m) => m,
        Err(_) => return Ok(Some(true)),
    };
    let total = whole.total;
    let mut head = [0u8; 32];
    let mut n = 0;
    while n < head.len() {
        let got = whole.read(&mut head[n..])?;
        if got == 0 {
            break;
        }
        n += got;
    }
    let head = &head[..n];
    if head.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        if n < 32 {
            return Ok(Some(true));
        }
        let u64_at = |i: usize| u64::from_le_bytes(head[i..i + 8].try_into().expect("8 bytes"));
        let need = 32u64.saturating_add(u64_at(12)).saturating_add(u64_at(20));
        return Ok(Some(total < need));
    }
    if [&b"PK\x03\x04"[..], b"PK\x05\x06", b"PK\x07\x08", b"PK00"]
        .iter()
        .any(|s| head.starts_with(s))
    {
        return Ok(Some(zip_end(&mut whole)?.is_none()));
    }
    Ok(None)
}

/// A RAR volume's end of archive header, the last thing in it: does it say another volume
/// follows? None: no such header there (encrypted headers, an old RAR that wrote none).
fn rar_more(last: &Path) -> io::Result<Option<bool>> {
    let mut f = File::open(last)?;
    let len = f.seek(SeekFrom::End(0))?;
    let take = len.min(64);
    f.seek(SeekFrom::Start(len - take))?;
    let mut tail = vec![0u8; take as usize];
    f.read_exact(&mut tail)?;
    Ok(rar5_end(&tail).or_else(|| rar4_end(&tail)))
}

/// A RAR 5 variable-length integer at `b[i..]`: its value and its length.
fn vint(b: &[u8], i: usize) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for k in 0..10 {
        let byte = *b.get(i + k)?;
        v |= ((byte & 0x7F) as u64) << (7 * k);
        if byte & 0x80 == 0 {
            return Some((v, k + 1));
        }
    }
    None
}

/// RAR 5's end of archive header ending `tail`: its CRC32 (of what follows it), its size, type
/// 5, its flags (and its extra area's size, if flagged), then its own flags, 0x0001 "a volume,
/// not the last".
fn rar5_end(tail: &[u8]) -> Option<bool> {
    for p in (0..tail.len().saturating_sub(7)).rev() {
        let Some((size, n)) = vint(tail, p + 4) else {
            continue;
        };
        let start = p + 4 + n;
        if size == 0 || start as u64 + size != tail.len() as u64 {
            continue;
        }
        let crc = u32::from_le_bytes(tail[p..p + 4].try_into().ok()?);
        if crc32fast::hash(&tail[p + 4..]) != crc {
            continue;
        }
        let (kind, a) = vint(tail, start)?;
        if kind != 5 {
            return None;
        }
        let (flags, b) = vint(tail, start + a)?;
        let mut i = start + a + b;
        if flags & 0x0001 != 0 {
            i += vint(tail, i)?.1;
        }
        return Some(vint(tail, i)?.0 & 0x0001 != 0);
    }
    None
}

/// RAR 4's end of archive block ending `tail`: its CRC16 (the low half of the CRC32 from its
/// type on), type 0x7B, its flags (0x0001: another volume follows), its size.
fn rar4_end(tail: &[u8]) -> Option<bool> {
    for p in (0..tail.len().saturating_sub(6)).rev() {
        if tail[p + 2] != 0x7B {
            continue;
        }
        let size = u16::from_le_bytes([tail[p + 5], tail[p + 6]]) as usize;
        if size < 7 || p + size != tail.len() {
            continue;
        }
        let crc = u16::from_le_bytes([tail[p], tail[p + 1]]);
        if crc32fast::hash(&tail[p + 2..]) as u16 != crc {
            continue;
        }
        return Some(u16::from_le_bytes([tail[p + 3], tail[p + 4]]) & 0x0001 != 0);
    }
    None
}

/// `x.7z.001`'s parts on disk: `.001`, `.002`… while they exist, and any later one found after a
/// gap (so a missing middle volume is reported, not taken for the end).
pub fn split_parts(first: &Path) -> Vec<PathBuf> {
    let name = first
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(stem) = name.get(..name.len().saturating_sub(3)) else {
        return vec![first.to_path_buf()];
    };
    let dir = first.parent().unwrap_or(Path::new(""));
    let path = |n: u32| dir.join(format!("{stem}{n:03}"));
    let mut last = 1;
    // The highest part there (up to 999), a gap or not.
    for n in 2..=999 {
        if path(n).exists() {
            last = n;
        } else if n > last + 10 {
            break;
        }
    }
    (1..=last).map(path).collect()
}

impl Read for MultiFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut start = 0;
        for (f, len) in &mut self.parts {
            if self.pos < start + *len {
                f.seek(SeekFrom::Start(self.pos - start))?;
                let room = (*len - (self.pos - start)) as usize;
                let take = buf.len().min(room);
                let n = f.read(&mut buf[..take])?;
                self.pos += n as u64;
                return Ok(n);
            }
            start += *len;
        }
        Ok(0)
    }
}

impl Seek for MultiFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(d) => self.total as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_of_one_set_share_a_key() {
        let key = |n: &str| set_key(n);
        assert_eq!(key("M.part1.rar"), key("m.PART02.rar"));
        assert_eq!(key("x.zip.001"), key("X.zip.004"));
        assert_eq!(key("x.7z.001"), key("x.7z.010"));
        assert_eq!(key("x.rar"), key("x.r00"));
        assert_eq!(key("x.z01"), key("x.zip"));
        assert_eq!(key("x.Z12"), key("X.ZIP"));
        // Other sets.
        assert_ne!(key("x.zip"), key("x.zip.001"));
        assert_ne!(key("x.part1.rar"), key("y.part2.rar"));
        assert_ne!(key("x.7z.001"), key("x.zip.001"));
        assert_eq!(key("x.7z"), None);
        assert_eq!(key("x.tar.gz"), None);
        assert!(spanned_part("backup.z01") && spanned_part("b.z100"));
        for n in ["b.zip", "b.z", "b.z1", ".z01", "b.zst", "b.z00", "b.7z"] {
            assert!(!spanned_part(n), "{n}");
        }
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

    /// A stored ZIP of two incompressible entries, its bytes.
    fn zip_bytes() -> Vec<u8> {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, seed) in [("a.bin", 1u64), ("b/c.bin", 2)] {
            z.start_file(name, stored).unwrap();
            z.write_all(&noise(150_000, seed)).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    /// `data` cut into `parts` files `name.001`… in `dir`, as 7-Zip's `-v` does.
    fn split(dir: &Path, name: &str, data: &[u8], parts: usize) -> Vec<PathBuf> {
        let size = data.len().div_ceil(parts);
        data.chunks(size)
            .enumerate()
            .map(|(k, c)| {
                let p = dir.join(format!("{name}.{:03}", k + 1));
                std::fs::write(&p, c).unwrap();
                p
            })
            .collect()
    }

    fn complete(open: &Path, files: &[PathBuf]) -> OnDisk {
        OnDisk::Complete {
            open: open.to_path_buf(),
            files: files.to_vec(),
        }
    }

    /// A byte-split ZIP is complete once its end record is in, whichever volume came last.
    #[test]
    fn a_split_zip_is_complete_once_its_end_is_in() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let parts = split(&src, "parts.zip", &zip_bytes(), 4);
        let dir = t.path().join("dl");
        std::fs::create_dir(&dir).unwrap();
        let put = |n: usize| {
            std::fs::copy(&parts[n - 1], dir.join(parts[n - 1].file_name().unwrap())).unwrap()
        };
        let first = dir.join("parts.zip.001");
        put(1);
        assert_eq!(on_disk(&first).unwrap(), OnDisk::Incomplete(2));
        put(2);
        put(3);
        // Asked from any volume: the same answer.
        assert_eq!(
            on_disk(&dir.join("parts.zip.003")).unwrap(),
            OnDisk::Incomplete(4)
        );
        // The middle one last: 1, 2 and 4 in, 3 not.
        std::fs::remove_file(dir.join("parts.zip.003")).unwrap();
        put(4);
        assert_eq!(on_disk(&first).unwrap(), OnDisk::Incomplete(3));
        put(3);
        let files: Vec<PathBuf> = (1..=4)
            .map(|n| dir.join(format!("parts.zip.{n:03}")))
            .collect();
        assert_eq!(
            on_disk(&dir.join("parts.zip.004")).unwrap(),
            complete(&first, &files)
        );
        // A volume cut at a ZIP entry's end (no end record anywhere in it) is not the last.
        let cut = t.path().join("cut");
        std::fs::create_dir(&cut).unwrap();
        let data = zip_bytes();
        std::fs::write(cut.join("c.zip.001"), &data[..data.len() - 30]).unwrap();
        assert_eq!(
            on_disk(&cut.join("c.zip.001")).unwrap(),
            OnDisk::Incomplete(2)
        );
        // Data after its end (its directory right before the record): complete.
        let tail = t.path().join("tail");
        std::fs::create_dir(&tail).unwrap();
        let mut more = data.clone();
        more.extend_from_slice(b"some data after the archive");
        std::fs::write(tail.join("t.zip.001"), &more).unwrap();
        assert!(matches!(
            on_disk(&tail.join("t.zip.001")).unwrap(),
            OnDisk::Complete { .. }
        ));
    }

    /// A 7z's start header says how long it is.
    #[test]
    fn a_split_7z_is_complete_at_its_whole_size() {
        let t = tempfile::tempdir().unwrap();
        let mut data = vec![b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4, 0, 0, 0, 0];
        // Next header at 900 000 after the start header, 100 bytes long: 900 132 in all.
        data.extend_from_slice(&900_000u64.to_le_bytes());
        data.extend_from_slice(&100u64.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.resize(900_132, 7);
        let parts = split(t.path(), "n.7z", &data, 3);
        let first = &parts[0];
        std::fs::remove_file(&parts[2]).unwrap();
        assert_eq!(on_disk(first).unwrap(), OnDisk::Incomplete(3));
        std::fs::write(&parts[2], &data[600_088..]).unwrap();
        assert_eq!(on_disk(first).unwrap(), complete(first, &parts));
        // One byte short: more to come.
        std::fs::write(&parts[2], &data[600_088..data.len() - 1]).unwrap();
        assert_eq!(on_disk(first).unwrap(), OnDisk::Incomplete(4));
    }

    /// A RAR 5 end of archive header: `more`, another volume follows.
    fn rar5(more: bool) -> Vec<u8> {
        let mut v = b"Rar!\x1A\x07\x01\x00".to_vec();
        v.extend_from_slice(&noise(5000, 3));
        let body = [3u8, 5, 0, more as u8];
        v.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
        v.extend_from_slice(&body);
        v
    }

    /// A RAR 4 end of archive block (with its data CRC and volume number, 13 bytes).
    fn rar4(more: bool) -> Vec<u8> {
        let mut v = b"Rar!\x1A\x07\x00".to_vec();
        v.extend_from_slice(&noise(5000, 4));
        let flags: u16 = more as u16 | 0x0002 | 0x0008;
        let mut body = vec![0x7B];
        body.extend_from_slice(&flags.to_le_bytes());
        body.extend_from_slice(&13u16.to_le_bytes());
        body.extend_from_slice(&[1, 2, 3, 4, 1, 0]);
        let crc = crc32fast::hash(&body) as u16;
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&body);
        v
    }

    /// A RAR volume's end header says whether another follows.
    #[test]
    fn a_rar_set_is_complete_at_the_volume_that_says_it_is_the_last() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let p = |n: &str| d.join(n);
        std::fs::write(p("m.part1.rar"), rar5(true)).unwrap();
        assert_eq!(on_disk(&p("m.part1.rar")).unwrap(), OnDisk::Incomplete(2));
        std::fs::write(p("m.part3.rar"), rar5(false)).unwrap();
        assert_eq!(on_disk(&p("m.part3.rar")).unwrap(), OnDisk::Incomplete(2));
        std::fs::write(p("m.part2.rar"), rar5(true)).unwrap();
        let files = [p("m.part1.rar"), p("m.part2.rar"), p("m.part3.rar")];
        assert_eq!(
            on_disk(&p("m.part2.rar")).unwrap(),
            complete(&files[0], &files)
        );
        // RAR 4, old names: x.rar, x.r00, x.r01.
        std::fs::write(p("old.rar"), rar4(true)).unwrap();
        assert_eq!(on_disk(&p("old.rar")).unwrap(), OnDisk::Incomplete(2));
        std::fs::write(p("old.r00"), rar4(true)).unwrap();
        assert_eq!(on_disk(&p("old.r00")).unwrap(), OnDisk::Incomplete(3));
        std::fs::write(p("old.r01"), rar4(false)).unwrap();
        let files = [p("old.rar"), p("old.r00"), p("old.r01")];
        assert_eq!(on_disk(&p("old.rar")).unwrap(), complete(&files[0], &files));
        // A RAR in one file, and one whose end doesn't say (encrypted headers): as it is.
        std::fs::write(p("one.rar"), rar5(false)).unwrap();
        assert_eq!(
            on_disk(&p("one.rar")).unwrap(),
            complete(&p("one.rar"), &[p("one.rar")])
        );
        std::fs::write(p("enc.part1.rar"), noise(3000, 9)).unwrap();
        assert!(matches!(
            on_disk(&p("enc.part1.rar")).unwrap(),
            OnDisk::Complete { .. }
        ));
    }

    /// The end record of a spanned ZIP's last volume: `disks` volumes.
    fn span_end(disks: u16) -> Vec<u8> {
        let mut v = noise(2000, 5);
        v.extend_from_slice(b"PK\x05\x06");
        for f in [disks - 1, disks - 1, 3, 3] {
            v.extend_from_slice(&f.to_le_bytes());
        }
        v.extend_from_slice(&500u32.to_le_bytes());
        v.extend_from_slice(&1000u32.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v
    }

    /// x.z01, x.z02… then x.zip, which says how many there are.
    #[test]
    fn a_spanned_zip_is_complete_with_its_zip_and_every_part() {
        let t = tempfile::tempdir().unwrap();
        let p = |n: &str| t.path().join(n);
        std::fs::write(p("s.z02"), noise(100, 1)).unwrap();
        // Its last volume not in: which volume is missing isn't known.
        assert_eq!(on_disk(&p("s.z02")).unwrap(), OnDisk::Incomplete(0));
        std::fs::write(p("s.zip"), span_end(4)).unwrap();
        assert_eq!(on_disk(&p("s.zip")).unwrap(), OnDisk::Incomplete(1));
        std::fs::write(p("s.z01"), noise(100, 2)).unwrap();
        assert_eq!(on_disk(&p("s.z01")).unwrap(), OnDisk::Incomplete(3));
        std::fs::write(p("S.Z03"), noise(100, 3)).unwrap();
        let files = [p("s.z01"), p("s.z02"), p("S.Z03"), p("s.zip")];
        assert_eq!(on_disk(&p("s.zip")).unwrap(), complete(&files[0], &files));
        // A ZIP in one file: itself, whatever is beside it (its folder isn't even read).
        std::fs::write(p("one.zip"), zip_bytes()).unwrap();
        std::fs::write(p("one.z01"), noise(100, 4)).unwrap();
        assert_eq!(
            on_disk(&p("one.zip")).unwrap(),
            complete(&p("one.zip"), &[p("one.zip")])
        );
        assert_eq!(
            on_disk(Path::new("no-such-folder/a.zip")).unwrap(),
            complete(
                Path::new("no-such-folder/a.zip"),
                &[PathBuf::from("no-such-folder/a.zip")]
            )
        );
        // ... and a stray part beside it is no volume of it.
        assert_eq!(on_disk(&p("one.z01")).unwrap(), OnDisk::Incomplete(0));
    }
}

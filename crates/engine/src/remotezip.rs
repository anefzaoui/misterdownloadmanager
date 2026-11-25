//! Files out of a ZIP on a server (#1775). A ZIP keeps its table of contents (the central
//! directory) at its end, so on a server that sends parts of a file (`Range`) MDM lists what is
//! inside with a few small requests ([`look`] as a download is about to start, [`list`]), and
//! takes the files picked out by fetching only their own bytes: each one's local header and data
//! ([`Fetch`]). Stored and Deflate files are unpacked here,
//! their bytes counted and checked against the CRC-32 the table gives, and handed to the caller,
//! which writes them (the app: `mdm_extract::put_file`, the extraction's own writer). Anything
//! else is refused with its reason ([`Problem`]): another compression method, an encrypted file,
//! an archive split over several disks, a server that sends no parts.
//!
//! Everything the server sends is read defensively. The table is at most [`MAX_DIRECTORY`] bytes
//! and [`MAX_ENTRIES`] entries, each name at most [`MAX_NAME`] bytes, each extra field read
//! within its own record. Sizes and offsets are read and checked, never trusted: an entry's data
//! lies before the table and apart from every other entry's (two that overlap are a ZIP bomb's
//! trick, and refuse the archive), a file's header must say what the table says, its unpacked
//! bytes are exactly as many as the table says and its CRC-32 the table's. A name that isn't a
//! plain relative path (`..`, an absolute path, a drive letter or a stream name, a control
//! character) is left out of the listing: it is never offered.

use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::download::{
    ConnStatus, ConnView, Error, Options, SegmentView, Snapshot, State, build, cancellable,
    http_client, is_web_page, parse_content_range, retry_after,
};
use crate::limiter::Limiter;

/// The largest table of contents read from a server.
pub const MAX_DIRECTORY: u64 = 32 << 20;
/// The most entries a table may hold (the extraction's own limit, `mdm_extract::Limits`).
pub const MAX_ENTRIES: u64 = 100_000;
/// The longest name an entry may have, in bytes.
pub const MAX_NAME: usize = 4096;
/// The end-of-directory record (22 bytes), its longest comment and the ZIP64 locator before it:
/// what the first request asks for, from the end.
pub const TAIL: u64 = 22 + 0xFFFF + 20;

const END: &[u8; 4] = b"PK\x05\x06";
const LOCATOR: &[u8; 4] = b"PK\x06\x07";
const END64: &[u8; 4] = b"PK\x06\x06";
const CENTRAL: &[u8; 4] = b"PK\x01\x02";
const LOCAL: &[u8; 4] = b"PK\x03\x04";
/// The fixed parts of the records.
const END_FIXED: usize = 22;
const END64_FIXED: usize = 56;
const LOCATOR_FIXED: usize = 20;
const CENTRAL_FIXED: usize = 46;
/// A local header's fixed part.
pub const LOCAL_FIXED: usize = 30;

/// Why files can't be taken out of a ZIP on its server: the whole archive can still be
/// downloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The server sends no parts of the file (it answers a range with the whole file).
    NoRanges,
    /// No end-of-directory record at its end: not a ZIP (or one with something after it).
    NotZip,
    /// Split over several disks or volumes.
    Spanned,
    /// Its table is larger than [`MAX_DIRECTORY`] or holds more than [`MAX_ENTRIES`] entries.
    TooBig,
    /// Its table doesn't add up (cut short, entries outside the data or over each other); what,
    /// in English, for the log.
    Damaged(&'static str),
    /// A file's bytes aren't what the table says: its header, its size or its CRC-32.
    Mismatch(&'static str),
    /// The file is encrypted.
    Encrypted,
    /// The file is compressed with this method, which MDM doesn't unpack here (only stored, 0,
    /// and Deflate, 8).
    Method(u16),
    /// The file is no longer in the archive (or never was).
    Missing,
    /// The archive changed on the server since its table was read.
    Changed,
}

impl std::fmt::Display for Problem {
    /// English, for the command line and the log (the app words it in its language).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::NoRanges => write!(f, "the server sends no parts of the ZIP"),
            Problem::NotZip => write!(f, "not a ZIP archive (no table of contents at its end)"),
            Problem::Spanned => write!(f, "the ZIP is split over several files"),
            Problem::TooBig => write!(f, "the ZIP's table of contents is too large"),
            Problem::Damaged(what) => write!(f, "the ZIP's table of contents is damaged: {what}"),
            Problem::Mismatch(what) => {
                write!(f, "the file in the ZIP doesn't match its table: {what}")
            }
            Problem::Encrypted => write!(f, "the file is encrypted in the ZIP"),
            Problem::Method(m) => write!(f, "the file is compressed with method {m}"),
            Problem::Missing => write!(f, "the file is no longer in the ZIP"),
            Problem::Changed => write!(f, "the ZIP changed on the server"),
        }
    }
}

impl From<Problem> for Error {
    fn from(p: Problem) -> Self {
        Error::Zip(p)
    }
}

/// The name of a compression method, for a refusal's sentence.
pub fn method_name(method: u16) -> String {
    match method {
        1 => "Shrink".into(),
        6 => "Implode".into(),
        9 => "Deflate64".into(),
        12 => "BZIP2".into(),
        14 => "LZMA".into(),
        93 => "Zstandard".into(),
        95 => "XZ".into(),
        98 => "PPMd".into(),
        99 => "AES".into(),
        m => format!("#{m}"),
    }
}

/// One entry of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its path in the archive, `/`-separated (a `\` some tools write made a `/`), a plain
    /// relative path: every part a plain name.
    pub name: String,
    /// A folder (its name ends in `/`).
    pub dir: bool,
    /// A symbolic link (a Unix archiver's): never taken out.
    pub link: bool,
    pub method: u16,
    pub encrypted: bool,
    pub crc32: u32,
    pub compressed: u64,
    pub size: u64,
    /// Where its local header begins.
    pub offset: u64,
    /// Where the next entry (or the table) begins: its header and data end by then.
    pub limit: u64,
    /// When it was last changed, as the table says it (MS-DOS's date and time, local time, to
    /// two seconds): [Entry::modified] reads it.
    pub dos_time: (u16, u16),
    /// Its name as the table has it, which its local header must repeat.
    raw_name: Vec<u8>,
}

impl Entry {
    /// A file MDM can offer: not a folder, not a link.
    pub fn is_file(&self) -> bool {
        !self.dir && !self.link
    }

    /// Why it can't be taken out here, if it can't: encrypted, or a method other than stored
    /// and Deflate.
    pub fn refused(&self) -> Option<Problem> {
        if self.encrypted {
            Some(Problem::Encrypted)
        } else if !matches!(self.method, 0 | 8) {
            Some(Problem::Method(self.method))
        } else {
            None
        }
    }

    /// Its folders and its name, each a plain name.
    pub fn parts(&self) -> Vec<&str> {
        self.name.split('/').filter(|p| !p.is_empty()).collect()
    }

    /// When it was last changed: year, month, day, hour, minute, second, as the archiver's
    /// clock said it; None for a date that isn't one (0, a month 13).
    pub fn modified(&self) -> Option<(u16, u8, u8, u8, u8, u8)> {
        let (date, time) = self.dos_time;
        let (month, day) = (((date >> 5) & 0x0F) as u8, (date & 0x1F) as u8);
        let (hour, minute, second) = (
            (time >> 11) as u8,
            ((time >> 5) & 0x3F) as u8,
            ((time & 0x1F) * 2) as u8,
        );
        ((1..=12).contains(&month)
            && (1..=31).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60)
            .then_some((1980 + (date >> 9), month, day, hour, minute, second))
    }

    /// Its own name, without its folders.
    pub fn file_name(&self) -> &str {
        self.name
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
    }
}

/// Where the table is, as the end of the archive says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    /// The table's offset and size.
    pub directory: u64,
    pub directory_size: u64,
    pub entries: u64,
}

/// What the archive's last bytes say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    End(End),
    /// A ZIP64 archive: its ZIP64 end record is at this offset ([`zip64_end`] reads it); the
    /// classic record ends at the second value (what the ZIP64 record must come before).
    Zip64 {
        at: u64,
        before: u64,
    },
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The end of an archive of `size` bytes, from its last bytes `tail` (beginning at `tail_at`):
/// the end-of-directory record whose comment runs to the archive's very end, and the ZIP64
/// locator before it when there is one.
pub fn tail(tail: &[u8], tail_at: u64, size: u64) -> Result<Tail, Problem> {
    if tail.len() < END_FIXED || tail_at.checked_add(tail.len() as u64) != Some(size) {
        return Err(Problem::NotZip);
    }
    // Backwards, the record nearest the end first: its comment's length must bring it to the
    // end (a signature inside a comment doesn't).
    let pos = (0..=tail.len() - END_FIXED)
        .rev()
        .find(|&p| {
            &tail[p..p + 4] == END
                && u16_at(tail, p + 20)
                    .is_some_and(|c| p + END_FIXED + usize::from(c) == tail.len())
        })
        .ok_or(Problem::NotZip)?;
    let rec = &tail[pos..];
    let field16 = |at| u16_at(rec, at).unwrap_or(0);
    let field32 = |at| u32_at(rec, at).unwrap_or(0);
    let end_at = tail_at + pos as u64;
    if pos >= LOCATOR_FIXED && &tail[pos - LOCATOR_FIXED..pos - LOCATOR_FIXED + 4] == LOCATOR {
        let loc = &tail[pos - LOCATOR_FIXED..pos];
        let disk = u32_at(loc, 4).unwrap_or(1);
        let at = u64_at(loc, 8).unwrap_or(u64::MAX);
        let disks = u32_at(loc, 16).unwrap_or(2);
        if disk != 0 || disks > 1 {
            return Err(Problem::Spanned);
        }
        let before = end_at - LOCATOR_FIXED as u64;
        if at
            .checked_add(END64_FIXED as u64)
            .is_none_or(|e| e > before)
        {
            return Err(Problem::Damaged(
                "the ZIP64 record isn't before its locator",
            ));
        }
        return Ok(Tail::Zip64 { at, before });
    }
    let (disk, dir_disk, here, entries) = (field16(4), field16(6), field16(8), field16(10));
    if disk != 0 || dir_disk != 0 || here != entries {
        return Err(Problem::Spanned);
    }
    check_end(
        End {
            directory: u64::from(field32(16)),
            directory_size: u64::from(field32(12)),
            entries: u64::from(entries),
        },
        end_at,
    )
    .map(Tail::End)
}

/// `end`'s table lies before `before` (the record that gives it), within the limits.
fn check_end(end: End, before: u64) -> Result<End, Problem> {
    if end.directory_size > MAX_DIRECTORY || end.entries > MAX_ENTRIES {
        return Err(Problem::TooBig);
    }
    if end
        .directory
        .checked_add(end.directory_size)
        .is_none_or(|e| e > before)
    {
        return Err(Problem::Damaged("the table lies outside the archive"));
    }
    // Every entry takes at least a record's fixed part.
    if end.entries.saturating_mul(CENTRAL_FIXED as u64) > end.directory_size {
        return Err(Problem::Damaged("the table is too small for its entries"));
    }
    Ok(end)
}

/// The ZIP64 end record `rec` (its first bytes at least), which must end by `before`.
pub fn zip64_end(rec: &[u8], before: u64, at: u64) -> Result<End, Problem> {
    if rec.len() < END64_FIXED || &rec[..4] != END64 {
        return Err(Problem::Damaged("no ZIP64 record where its locator says"));
    }
    let field32 = |o| u32_at(rec, o).unwrap_or(1);
    let field64 = |o| u64_at(rec, o).unwrap_or(u64::MAX);
    let (disk, dir_disk, here, entries) = (field32(16), field32(20), field64(24), field64(32));
    if disk != 0 || dir_disk != 0 || here != entries {
        return Err(Problem::Spanned);
    }
    if at
        .checked_add(END64_FIXED as u64)
        .is_none_or(|e| e > before)
    {
        return Err(Problem::Damaged(
            "the ZIP64 record isn't before its locator",
        ));
    }
    check_end(
        End {
            directory: field64(48),
            directory_size: field64(40),
            entries,
        },
        at,
    )
}

/// CP 437's characters from 0x80 on: the names of a ZIP whose entries don't say UTF-8.
const CP437: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// An entry's name: UTF-8 when its flags say so (bit 11), else CP 437, as archivers write it.
fn decode_name(raw: &[u8], utf8: bool) -> String {
    if utf8 || raw.is_ascii() {
        return String::from_utf8_lossy(raw).into_owned();
    }
    raw.iter()
        .map(|&b| {
            if b < 0x80 {
                char::from(b)
            } else {
                CP437.chars().nth(usize::from(b - 0x80)).unwrap_or('?')
            }
        })
        .collect()
}

/// A relative path whose every part is a plain name: no absolute path, no `..`, no drive letter
/// or stream name (`:`), no control character, no empty part. A folder's trailing `/` aside.
fn plain_path(name: &str) -> bool {
    let inner = name.strip_suffix('/').unwrap_or(name);
    !inner.is_empty()
        && !inner.starts_with('/')
        && inner
            .split('/')
            .all(|p| !p.contains(':') && crate::folder::plain_name(p))
}

/// The table `dir` (exactly `end.directory_size` bytes) read: its entries whose names are plain
/// paths, and how many were left out for theirs. Every entry is checked, those left out too:
/// each one's data must lie before the table and apart from every other's.
pub fn directory(dir: &[u8], end: &End) -> Result<(Vec<Entry>, usize), Problem> {
    if dir.len() as u64 != end.directory_size {
        return Err(Problem::Damaged(
            "the table isn't as long as the archive says",
        ));
    }
    if end.entries > MAX_ENTRIES || end.directory_size > MAX_DIRECTORY {
        return Err(Problem::TooBig);
    }
    let cut = || Problem::Damaged("the table is cut short");
    // Every entry's header offset and where its data ends at the least (its local header's
    // fixed part and name, its data), and its place in `kept` when it is offered.
    let mut spans: Vec<(u64, u64, Option<usize>)> = Vec::with_capacity(end.entries as usize);
    let mut kept: Vec<Entry> = Vec::new();
    let mut left_out = 0;
    let mut at = 0usize;
    for _ in 0..end.entries {
        let rec = dir.get(at..at + CENTRAL_FIXED).ok_or_else(cut)?;
        if &rec[..4] != CENTRAL {
            return Err(Problem::Damaged("an entry of the table has no signature"));
        }
        let field16 = |o| u16_at(rec, o).unwrap_or(0);
        let field32 = |o| u32_at(rec, o).unwrap_or(0);
        let made_by = field16(4);
        let flags = field16(8);
        let method = field16(10);
        let crc32 = field32(16);
        let mut compressed = u64::from(field32(20));
        let mut size = u64::from(field32(24));
        let name_len = usize::from(field16(28));
        let extra_len = usize::from(field16(30));
        let comment_len = usize::from(field16(32));
        let mut disk = u32::from(field16(34));
        let external = field32(38);
        let mut offset = u64::from(field32(42));
        if name_len > MAX_NAME {
            return Err(Problem::Damaged("a name is too long"));
        }
        let name_at = at + CENTRAL_FIXED;
        let extra_at = name_at + name_len;
        let next = extra_at + extra_len + comment_len;
        if next > dir.len() {
            return Err(cut());
        }
        let raw = &dir[name_at..extra_at];
        let extra = &dir[extra_at..extra_at + extra_len];
        // ZIP64's sizes and offset, each present only when its field above says 0xFFFFFFFF, in
        // this order (APPNOTE 4.5.3), read within the extra field's own length.
        let wants = [
            size == 0xFFFF_FFFF,
            compressed == 0xFFFF_FFFF,
            offset == 0xFFFF_FFFF,
            disk == 0xFFFF,
        ];
        if wants.iter().any(|w| *w) {
            let field = extra_field(extra, 0x0001)
                .ok_or(Problem::Damaged("a ZIP64 entry has no ZIP64 field"))?;
            let mut o = 0;
            let missing = Problem::Damaged("a ZIP64 field is cut short");
            if wants[0] {
                size = u64_at(field, o).ok_or(missing.clone())?;
                o += 8;
            }
            if wants[1] {
                compressed = u64_at(field, o).ok_or(missing.clone())?;
                o += 8;
            }
            if wants[2] {
                offset = u64_at(field, o).ok_or(missing.clone())?;
                o += 8;
            }
            if wants[3] {
                disk = u32_at(field, o).ok_or(missing)?;
            }
        }
        if disk != 0 {
            return Err(Problem::Spanned);
        }
        // Its header and data lie before the table.
        let data_end = offset
            .checked_add((LOCAL_FIXED + name_len) as u64)
            .and_then(|e| e.checked_add(compressed))
            .filter(|e| *e <= end.directory)
            .ok_or(Problem::Damaged("an entry lies outside the archive's data"))?;
        let name = decode_name(raw, flags & (1 << 11) != 0).replace('\\', "/");
        let place = if plain_path(&name) {
            kept.push(Entry {
                dir: name.ends_with('/'),
                // A Unix archiver's link: its host (the high byte of "made by") is Unix and its
                // mode (the high half of the external attributes) a link's.
                link: made_by >> 8 == 3 && (external >> 16) & 0o170_000 == 0o120_000,
                method,
                // Bit 0: encrypted; bit 6: strong encryption.
                encrypted: flags & 0x41 != 0,
                crc32,
                compressed,
                size,
                offset,
                limit: end.directory,
                dos_time: (field16(14), field16(12)),
                raw_name: raw.to_vec(),
                name,
            });
            Some(kept.len() - 1)
        } else {
            left_out += 1;
            None
        };
        spans.push((offset, data_end, place));
        at = next;
    }
    // In the order they lie in: each ends before the next begins. Two over each other (one
    // compressed block that many entries point to) are a ZIP bomb's trick, or damage.
    spans.sort_unstable_by_key(|s| s.0);
    for i in 0..spans.len() {
        let next = spans.get(i + 1).map_or(end.directory, |s| s.0);
        if spans[i].1 > next {
            return Err(Problem::Damaged("entries overlap"));
        }
        if let Some(k) = spans[i].2 {
            kept[k].limit = next;
        }
    }
    Ok((kept, left_out))
}

/// The data of the extra field `id` in `extra` (an entry's extra fields), read within it.
fn extra_field(extra: &[u8], id: u16) -> Option<&[u8]> {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let this = u16_at(extra, at)?;
        let len = usize::from(u16_at(extra, at + 2)?);
        let data = extra.get(at + 4..at + 4 + len)?;
        if this == id {
            return Some(data);
        }
        at += 4 + len;
    }
    None
}

/// An entry's local header, its fixed part `head`: how many bytes of name and extra field follow
/// it. It must be where the table says, of the same method, and leave room for the data before
/// the next entry.
pub fn local_header(head: &[u8], e: &Entry) -> Result<(usize, usize), Problem> {
    if head.len() < LOCAL_FIXED || &head[..4] != LOCAL {
        return Err(Problem::Mismatch("no header where the table says"));
    }
    if u16_at(head, 8) != Some(e.method) {
        return Err(Problem::Mismatch("its header gives another method"));
    }
    let name = usize::from(u16_at(head, 26).unwrap_or(0));
    let extra = usize::from(u16_at(head, 28).unwrap_or(0));
    if name != e.raw_name.len() {
        return Err(Problem::Mismatch("its header gives another name"));
    }
    let data_end = e
        .offset
        .checked_add((LOCAL_FIXED + name + extra) as u64)
        .and_then(|d| d.checked_add(e.compressed));
    if data_end.is_none_or(|d| d > e.limit) {
        return Err(Problem::Mismatch("its data runs into the next entry"));
    }
    Ok((name, extra))
}

/// Whether `name`, the name in an entry's local header, is the table's.
pub fn same_name(name: &[u8], e: &Entry) -> bool {
    name == e.raw_name
}

/// A ZIP on a server, its table read.
#[derive(Clone, Debug)]
pub struct Listing {
    /// The archive's size.
    pub size: u64,
    /// Its entries whose names are plain paths, folders and links among them.
    pub entries: Vec<Entry>,
    /// Entries left out for their names (not plain paths).
    pub left_out: usize,
    /// What tells this copy of the archive (its strong ETag, else its date): every later request
    /// asks with it (`If-Range`), so a changed archive is noticed.
    validator: Option<HeaderValue>,
}

/// The table of the ZIP at `url` (`size` bytes long when that is known: a look-up said it), read
/// with a few range requests: its end (its last [`TAIL`] bytes), then its ZIP64 record and its
/// table where they aren't in those.
pub async fn list(
    url: &Url,
    headers: &HeaderMap,
    opts: &Options,
    size: Option<u64>,
) -> Result<Listing, Error> {
    let server = Server::new(url, headers, opts)?;
    server
        .list(size, false)
        .await
        .map(|l| l.expect("a whole listing"))
}

/// One look at the ZIP at `url`, as a download of it is about to start: a single request for its
/// end ([`TAIL`] bytes; `size`, when known, saves asking for it). Whether files can be taken out
/// of it here: `Ok(Some)` with its table when the table came with its end, `Ok(None)` when the
/// server sends parts and the table lies further back (read when it is asked for, [`list`]),
/// else why not ([`Problem::NoRanges`] above all).
pub async fn look(
    url: &Url,
    headers: &HeaderMap,
    opts: &Options,
    size: Option<u64>,
) -> Result<Option<Listing>, Error> {
    Server::new(url, headers, opts)?.list(size, true).await
}

/// The archive's server, asked as a download of it is.
struct Server<'a> {
    client: Client,
    url: &'a Url,
    headers: &'a HeaderMap,
    opts: &'a Options,
    cancel: CancellationToken,
}

impl<'a> Server<'a> {
    fn new(url: &'a Url, headers: &'a HeaderMap, opts: &'a Options) -> Result<Self, Error> {
        Ok(Server {
            client: http_client(opts, url)?,
            url,
            headers,
            opts,
            cancel: CancellationToken::new(),
        })
    }

    /// Send a request for the archive with `extra` headers besides the download's.
    async fn send(&self, extra: HeaderMap) -> Result<Response, Error> {
        let mut asked = self.headers.clone();
        asked.extend(extra);
        cancellable(&self.cancel, async {
            Ok(build(
                &self.client,
                self.url.clone(),
                None,
                &asked,
                self.opts.site_agent(),
            )
            .send()
            .await?)
        })
        .await
    }

    /// The archive's table, its end asked for as `size` allows (an explicit range when it is
    /// known, else a suffix range, whose answer says the size); with `tail_only`, None rather
    /// than a second request when the table isn't in the end.
    async fn list(&self, size: Option<u64>, tail_only: bool) -> Result<Option<Listing>, Error> {
        let mut resp = match size.filter(|s| *s > 0) {
            Some(size) => {
                self.send(range_of(size.saturating_sub(TAIL), size - 1))
                    .await?
            }
            None => {
                let mut suffix = HeaderMap::new();
                suffix.insert(
                    header::RANGE,
                    HeaderValue::from_str(&format!("bytes=-{TAIL}")).expect("ascii"),
                );
                self.send(suffix).await?
            }
        };
        if size.is_none() && resp.status() == StatusCode::OK && !is_web_page(resp.headers()) {
            // Some servers answer a suffix longer than the file (a ZIP under `TAIL` bytes) with
            // the whole file: its size from its first byte, then its end asked for plainly. Its
            // body isn't read.
            drop(resp);
            let first = self.send(range_of(0, 0)).await?;
            if first.status() == StatusCode::OK {
                return Err(Problem::NoRanges.into());
            }
            answered(first.status(), first.headers())?;
            let size = parse_content_range(first.headers())?
                .2
                .ok_or_else(|| Error::BadRange("no size".into()))?;
            drop(first);
            if size == 0 {
                return Err(Problem::NotZip.into());
            }
            resp = self
                .send(range_of(size.saturating_sub(TAIL), size - 1))
                .await?;
        }
        let status = resp.status();
        if status == StatusCode::OK {
            // The whole archive where a part was asked: no ranges here. Its body isn't read.
            return Err(if is_web_page(resp.headers()) {
                Error::GotWebPage
            } else {
                Problem::NoRanges.into()
            });
        }
        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(Problem::NotZip.into());
        }
        answered(status, resp.headers())?;
        let (start, last, total) = parse_content_range(resp.headers())?;
        let total = total.ok_or_else(|| Error::BadRange("no size".into()))?;
        if size.is_some_and(|s| s != total) {
            return Err(Problem::Changed.into());
        }
        if last.checked_add(1) != Some(total) || start > last || last - start >= TAIL {
            return Err(Error::BadRange(format!(
                "asked the last {TAIL} bytes, got {start}-{last}/{total}"
            )));
        }
        let mut listing = Listing {
            size: total,
            entries: Vec::new(),
            left_out: 0,
            validator: validator(resp.headers()),
        };
        let end_bytes = read_exact(resp, last - start + 1, &self.cancel).await?;
        let have = |from: u64, len: u64| {
            from.checked_sub(start)
                .and_then(|o| o.checked_add(len))
                .is_some_and(|e| e <= end_bytes.len() as u64)
        };
        let end = match tail(&end_bytes, start, total)? {
            Tail::End(end) => end,
            Tail::Zip64 { at, .. } if tail_only && !have(at, END64_FIXED as u64) => {
                return Ok(None);
            }
            Tail::Zip64 { at, before } => {
                let rec = self
                    .part(&listing, &end_bytes, start, at, END64_FIXED as u64)
                    .await?;
                zip64_end(&rec, before, at)?
            }
        };
        if tail_only && !have(end.directory, end.directory_size) {
            return Ok(None);
        }
        let dir = self
            .part(
                &listing,
                &end_bytes,
                start,
                end.directory,
                end.directory_size,
            )
            .await?;
        (listing.entries, listing.left_out) = directory(&dir, &end)?;
        Ok(Some(listing))
    }

    /// Bytes `[from, from + len)`: from `have` (the archive's bytes from `have_at`) when they
    /// are in it, else asked for.
    async fn part(
        &self,
        listing: &Listing,
        have: &[u8],
        have_at: u64,
        from: u64,
        len: u64,
    ) -> Result<Vec<u8>, Error> {
        if let Some(o) = from.checked_sub(have_at)
            && o.checked_add(len).is_some_and(|e| e <= have.len() as u64)
        {
            return Ok(have[o as usize..(o + len) as usize].to_vec());
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        let resp = self.ask(listing, from, len).await?;
        read_exact(resp, len, &self.cancel).await
    }

    /// Ask for bytes `[from, from + len)` of the archive `listing` read: exactly that part of
    /// the same archive, else why not.
    async fn ask(&self, listing: &Listing, from: u64, len: u64) -> Result<Response, Error> {
        let to = from
            .checked_add(len)
            .filter(|t| *t <= listing.size && len > 0)
            .ok_or_else(|| Error::BadRange(format!("{from}+{len} past {}", listing.size)))?;
        let mut extra = HeaderMap::new();
        extra.insert(
            header::RANGE,
            HeaderValue::from_str(&format!("bytes={from}-{}", to - 1)).expect("ascii"),
        );
        if let Some(v) = &listing.validator {
            extra.insert(header::IF_RANGE, v.clone());
        }
        let resp = self.send(extra).await?;
        if resp.status() == StatusCode::OK {
            // With `If-Range`, a whole answer is a changed file; without one, a server that took
            // a range a moment ago and not now.
            return Err(if listing.validator.is_some() {
                Problem::Changed
            } else {
                Problem::NoRanges
            }
            .into());
        }
        answered(resp.status(), resp.headers())?;
        let (start, last, total) = parse_content_range(resp.headers())?;
        if total.is_some_and(|t| t != listing.size) {
            return Err(Problem::Changed.into());
        }
        if start != from || last != to - 1 {
            return Err(Error::BadRange(format!(
                "asked {from}-{}, got {start}-{last}",
                to - 1
            )));
        }
        Ok(resp)
    }
}

/// `Range: bytes=<first>-<last>`.
fn range_of(first: u64, last: u64) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        header::RANGE,
        HeaderValue::from_str(&format!("bytes={first}-{last}")).expect("ascii"),
    );
    h
}

/// An answer that isn't a part: the server's refusal, as the engine words it.
fn answered(status: StatusCode, headers: &HeaderMap) -> Result<(), Error> {
    match status {
        StatusCode::PARTIAL_CONTENT => Ok(()),
        s if s == StatusCode::TOO_MANY_REQUESTS || s == StatusCode::SERVICE_UNAVAILABLE => {
            Err(Error::Throttled(s, retry_after(headers)))
        }
        s => Err(Error::Status(s)),
    }
}

/// The validator a later request asks with (`If-Range`): a strong ETag, else the date.
fn validator(h: &HeaderMap) -> Option<HeaderValue> {
    h.get(header::ETAG)
        .filter(|v| !v.as_bytes().starts_with(b"W/"))
        .or_else(|| h.get(header::LAST_MODIFIED))
        .cloned()
}

/// Exactly `len` bytes of `resp`'s body: a body cut short or longer than its range is an error,
/// and no more than `len` is ever kept.
async fn read_exact(
    mut resp: Response,
    len: u64,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(len.min(1 << 20) as usize);
    loop {
        let chunk = cancellable(cancel, async { Ok(resp.chunk().await?) }).await?;
        let Some(chunk) = chunk else { break };
        if out.len() as u64 + chunk.len() as u64 > len {
            return Err(Error::BadRange("more bytes than asked".into()));
        }
        out.extend_from_slice(&chunk);
    }
    if (out.len() as u64) < len {
        return Err(Error::ShortBody);
    }
    Ok(out)
}

/// Writes a file taken out of a ZIP: given its entry and its unpacked bytes, which fail
/// (`InvalidData`) when they aren't what the table says, it writes them and says where the file
/// went. Called once per file, on a thread that may wait for the disk.
pub type Sink = Box<dyn FnMut(&Entry, &mut dyn Read) -> io::Result<PathBuf> + Send>;

/// What to take out of a ZIP on its server ([`Fetch::start`]).
#[derive(Clone, Debug, Default)]
pub struct Pick {
    /// The files picked, by their paths in the archive ([`Entry::name`]).
    pub files: Vec<String>,
    /// Those of them already taken out by an earlier run: not fetched again, their bytes
    /// counted as downloaded.
    pub done: Vec<String>,
    /// The archive's size, when known: its end is asked for with one plain range.
    pub size: Option<u64>,
    /// The folder they go into, below which each keeps its path (the view's file).
    pub folder: PathBuf,
}

/// Files taken out of a ZIP on its server, running: its table read again, then each picked
/// file's own bytes fetched (one request each), unpacked, checked and handed to the [`Sink`]. A
/// view as a download's ([`Snapshot`]): its size and bytes are the picked files' unpacked ones
/// together, its file the folder. A stop and a start go on with the files not taken out yet
/// ([`Fetch::done`]); one stopped part-way is fetched again whole.
pub struct Fetch {
    view: Arc<Mutex<Snapshot>>,
    cancel: CancellationToken,
    limiter: Arc<Limiter>,
    done: Arc<Mutex<Vec<(String, PathBuf)>>>,
}

impl Fetch {
    /// Take `pick`'s files out of the ZIP at `url`, the server asked as a download of it is
    /// (`headers`, `opts`). Must be called inside a Tokio runtime.
    pub fn start(url: Url, headers: HeaderMap, opts: Options, pick: Pick, sink: Sink) -> Fetch {
        let view = Arc::new(Mutex::new(Snapshot {
            state: State::Probing,
            url: url.clone(),
            file: None,
            size: None,
            downloaded: 0,
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
        }));
        let cancel = CancellationToken::new();
        let limiter = Arc::new(Limiter::new(opts.speed_limit));
        let done: Arc<Mutex<Vec<(String, PathBuf)>>> = Default::default();
        let folder = pick.folder.clone();
        let run = Run {
            url,
            headers,
            opts,
            pick,
            view: view.clone(),
            cancel: cancel.clone(),
            limiter: limiter.clone(),
            done: done.clone(),
        };
        let v = view.clone();
        tokio::spawn(async move {
            let out = run.go(sink).await;
            let mut v = v.lock().unwrap();
            for c in &mut v.connections {
                c.finish();
            }
            match out {
                Ok(()) => {
                    v.state = State::Done;
                    v.file = Some(folder);
                    if let Some(size) = v.size {
                        v.downloaded = size;
                    }
                }
                Err(Error::Cancelled) => v.state = State::Cancelled,
                Err(e) => {
                    v.state = State::Failed;
                    v.error = Some(e.to_string());
                    v.failure = Some(e);
                }
            }
        });
        Fetch {
            view,
            cancel,
            limiter,
            done,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.view.lock().unwrap().clone()
    }

    /// The files taken out by this run so far: each one's path in the archive and where it went.
    pub fn done(&self) -> Vec<(String, PathBuf)> {
        self.done.lock().unwrap().clone()
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Change its speed limit (bytes/s, None = unlimited) while it runs.
    pub fn set_speed_limit(&self, rate: Option<u64>) {
        self.limiter.set(rate);
    }

    pub fn speed_limit(&self) -> Option<u64> {
        self.limiter.get()
    }
}

/// [`Fetch`]'s work.
struct Run {
    url: Url,
    headers: HeaderMap,
    opts: Options,
    pick: Pick,
    view: Arc<Mutex<Snapshot>>,
    cancel: CancellationToken,
    limiter: Arc<Limiter>,
    done: Arc<Mutex<Vec<(String, PathBuf)>>>,
}

impl Run {
    async fn go(self, mut sink: Sink) -> Result<(), Error> {
        let mut server = Server::new(&self.url, &self.headers, &self.opts)?;
        server.cancel = self.cancel.clone();
        // The table again, as it is now: each file is found by its name, where it is now.
        let listing = server
            .list(self.pick.size, false)
            .await?
            .ok_or(Problem::Missing)?;
        let mut files = Vec::with_capacity(self.pick.files.len());
        for name in &self.pick.files {
            let e = listing
                .entries
                .iter()
                .find(|e| e.name == *name && e.is_file())
                .ok_or(Problem::Missing)?;
            if let Some(why) = e.refused() {
                return Err(why.into());
            }
            files.push((e.clone(), self.pick.done.contains(name)));
        }
        let total: u64 = files.iter().map(|(e, _)| e.size).sum();
        let mut base: u64 = files.iter().filter(|f| f.1).map(|(e, _)| e.size).sum();
        {
            let mut v = self.view.lock().unwrap();
            v.state = State::Downloading;
            v.size = Some(total);
            v.downloaded = base;
            v.segments = vec![SegmentView {
                start: 0,
                cursor: base,
                end: total,
            }];
            v.connections = vec![ConnView {
                id: 0,
                downloaded: 0,
                status: ConnStatus::Connecting,
                speed: 0,
            }];
        }
        for (entry, _) in files.into_iter().filter(|f| !f.1) {
            // Its header (the fixed part, its name and an extra field as long as one can be)
            // and its data, in one request, never past the next entry.
            let len = (LOCAL_FIXED + entry.raw_name.len() + 0xFFFF) as u64 + entry.compressed;
            let len = len.min(entry.limit - entry.offset);
            let resp = server.ask(&listing, entry.offset, len).await?;
            if let Some(c) = self.view.lock().unwrap().connections.first_mut() {
                c.status = ConnStatus::Receiving;
            }
            let failure: Arc<Mutex<Option<Error>>> = Default::default();
            let body = Body {
                rt: tokio::runtime::Handle::current(),
                resp,
                chunk: Bytes::new(),
                left: len,
                cancel: self.cancel.clone(),
                limiter: self.limiter.clone(),
                global: self.opts.global.clone(),
                failure: failure.clone(),
            };
            let view = self.view.clone();
            let hook = self.opts.on_file_created.clone();
            let task = tokio::task::spawn_blocking(move || {
                let out = (|| -> Result<(Entry, PathBuf), Error> {
                    let reader = Unpack::open(body, &entry, failure.clone())?;
                    let mut checked = Checked {
                        inner: reader,
                        hasher: crc32fast::Hasher::new(),
                        base,
                        count: 0,
                        entry: &entry,
                        view,
                        failure: failure.clone(),
                    };
                    match sink(&entry, &mut checked) {
                        Ok(path) => {
                            // As a download's part file is marked when it is made (the Mark of
                            // the Web, macOS's quarantine).
                            if let Some(h) = hook {
                                (h.0)(&path);
                            }
                            Ok((entry.clone(), path))
                        }
                        Err(e) => Err(failure
                            .lock()
                            .unwrap()
                            .take()
                            .unwrap_or_else(|| Error::Io(Arc::new(e)))),
                    }
                })();
                (out, sink)
            });
            let (out, back) = task
                .await
                .map_err(|e| Error::Io(Arc::new(io::Error::other(e.to_string()))))?;
            sink = back;
            let (entry, path) = out?;
            base += entry.size;
            self.done.lock().unwrap().push((entry.name, path));
        }
        Ok(())
    }
}

/// The answer's body as bytes to read, on a thread that may wait: at most `left` bytes, each
/// paid for under the download's limit and the global one, a cancel heard at every wait.
struct Body {
    rt: tokio::runtime::Handle,
    resp: Response,
    chunk: Bytes,
    left: u64,
    cancel: CancellationToken,
    limiter: Arc<Limiter>,
    global: Option<Arc<Limiter>>,
    /// Why a read failed, as the engine says it (a read's `io::Error` can't carry it whole).
    failure: Arc<Mutex<Option<Error>>>,
}

impl Body {
    fn fail(&self, e: Error) -> io::Error {
        let text = e.to_string();
        *self.failure.lock().unwrap() = Some(e);
        io::Error::other(text)
    }
}

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.left == 0 {
            return Ok(0);
        }
        if self.chunk.is_empty() {
            let (resp, cancel) = (&mut self.resp, &self.cancel);
            let next = self
                .rt
                .block_on(cancellable(cancel, async { Ok(resp.chunk().await?) }));
            match next {
                Ok(Some(c)) => self.chunk = c,
                Ok(None) => return Err(self.fail(Error::ShortBody)),
                Err(e) => return Err(self.fail(e)),
            }
            let n = (self.chunk.len() as u64).min(self.left);
            let (limiter, global, cancel) = (&self.limiter, &self.global, &self.cancel);
            let paid = self.rt.block_on(cancellable(cancel, async {
                limiter.acquire(n).await;
                if let Some(g) = global {
                    g.acquire(n).await;
                }
                Ok(())
            }));
            if let Err(e) = paid {
                return Err(self.fail(e));
            }
        }
        let n = buf.len().min(self.chunk.len()).min(self.left as usize);
        buf[..n].copy_from_slice(&self.chunk[..n]);
        self.chunk = self.chunk.slice(n..);
        self.left -= n as u64;
        Ok(n)
    }
}

/// An entry's data unpacked: its header read and checked first.
enum Unpack {
    Stored(Box<io::Take<Body>>),
    Deflate(Box<flate2::read::DeflateDecoder<io::Take<Body>>>),
}

impl Unpack {
    fn open(mut body: Body, e: &Entry, failure: Arc<Mutex<Option<Error>>>) -> Result<Self, Error> {
        let taken = |r: io::Error| {
            failure
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Error::Io(Arc::new(r)))
        };
        let mut head = [0u8; LOCAL_FIXED];
        body.read_exact(&mut head).map_err(taken)?;
        let (name_len, extra_len) = local_header(&head, e)?;
        let mut name = vec![0u8; name_len];
        body.read_exact(&mut name).map_err(taken)?;
        if !same_name(&name, e) {
            return Err(Problem::Mismatch("its header gives another name").into());
        }
        io::copy(&mut (&mut body).take(extra_len as u64), &mut io::sink()).map_err(taken)?;
        let data = body.take(e.compressed);
        Ok(match e.method {
            0 => Unpack::Stored(Box::new(data)),
            8 => Unpack::Deflate(Box::new(flate2::read::DeflateDecoder::new(data))),
            m => return Err(Problem::Method(m).into()),
        })
    }
}

impl Read for Unpack {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Unpack::Stored(r) => r.read(buf),
            Unpack::Deflate(r) => r.read(buf),
        }
    }
}

/// The unpacked bytes counted and checked: never more than the table's size, and at their end
/// exactly that many with the table's CRC-32, else an `InvalidData` error (and the reason kept).
/// Each read moves the download's view on.
struct Checked<'a> {
    inner: Unpack,
    hasher: crc32fast::Hasher,
    /// The bytes of the files taken out before this one (the view counts them all).
    base: u64,
    count: u64,
    entry: &'a Entry,
    view: Arc<Mutex<Snapshot>>,
    failure: Arc<Mutex<Option<Error>>>,
}

impl Checked<'_> {
    fn mismatch(&self, what: &'static str) -> io::Error {
        *self.failure.lock().unwrap() = Some(Problem::Mismatch(what).into());
        io::Error::new(io::ErrorKind::InvalidData, what)
    }
}

impl Read for Checked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = match self.inner.read(buf) {
            Ok(n) => n,
            Err(e) => {
                // The decoder's own complaint (a damaged Deflate stream), unless the network's
                // came first.
                let mut f = self.failure.lock().unwrap();
                if f.is_none() && e.kind() == io::ErrorKind::InvalidData {
                    *f = Some(Problem::Mismatch("its data doesn't unpack").into());
                }
                return Err(e);
            }
        };
        if n == 0 {
            if self.count != self.entry.size {
                return Err(self.mismatch("fewer bytes than the table says"));
            }
            if self.hasher.clone().finalize() != self.entry.crc32 {
                return Err(self.mismatch("its CRC-32 isn't the table's"));
            }
            return Ok(0);
        }
        self.count += n as u64;
        if self.count > self.entry.size {
            return Err(self.mismatch("more bytes than the table says"));
        }
        self.hasher.update(&buf[..n]);
        let mut v = self.view.lock().unwrap();
        v.downloaded = self.base + self.count;
        if let Some(s) = v.segments.first_mut() {
            s.cursor = self.base + self.count;
        }
        if let Some(c) = v.connections.first_mut() {
            c.downloaded = self.count;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZIP made by hand: `(name, method, data as stored, crc, size)` per entry, its central
    /// records with `extra` each; ZIP64 records when `zip64`.
    pub(crate) fn zip_of(files: &[(&str, u16, &[u8], u32, u64)], zip64: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, method, data, crc, size) in files {
            let offset = out.len() as u64;
            out.extend_from_slice(LOCAL);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(*size as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central.extend_from_slice(CENTRAL);
            central.extend_from_slice(&[20, 3, 20, 0]);
            central.extend_from_slice(&0x0800u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 4]);
            central.extend_from_slice(&crc.to_le_bytes());
            let (csize, usize_, off, extra) = if zip64 {
                let mut x = Vec::new();
                x.extend_from_slice(&1u16.to_le_bytes());
                x.extend_from_slice(&24u16.to_le_bytes());
                x.extend_from_slice(&size.to_le_bytes());
                x.extend_from_slice(&(data.len() as u64).to_le_bytes());
                x.extend_from_slice(&offset.to_le_bytes());
                (u32::MAX, u32::MAX, u32::MAX, x)
            } else {
                (data.len() as u32, *size as u32, offset as u32, Vec::new())
            };
            central.extend_from_slice(&csize.to_le_bytes());
            central.extend_from_slice(&usize_.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            // Comment length, disk, internal attributes.
            central.extend_from_slice(&[0; 6]);
            central.extend_from_slice(&((0o100_644u32) << 16).to_le_bytes());
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
            central.extend_from_slice(&extra);
        }
        let dir_at = out.len() as u64;
        out.extend_from_slice(&central);
        let n = files.len() as u64;
        if zip64 {
            let rec_at = out.len() as u64;
            out.extend_from_slice(END64);
            out.extend_from_slice(&44u64.to_le_bytes());
            out.extend_from_slice(&[45, 3, 45, 0]);
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&(central.len() as u64).to_le_bytes());
            out.extend_from_slice(&dir_at.to_le_bytes());
            out.extend_from_slice(LOCATOR);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&rec_at.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
        }
        out.extend_from_slice(END);
        out.extend_from_slice(&[0; 4]);
        let (count, dsize, dat) = if zip64 {
            (0xFFFFu16, u32::MAX, u32::MAX)
        } else {
            (n as u16, central.len() as u32, dir_at as u32)
        };
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&dsize.to_le_bytes());
        out.extend_from_slice(&dat.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc(data: &[u8]) -> u32 {
        let mut h = crc32fast::Hasher::new();
        h.update(data);
        h.finalize()
    }

    /// Everything [`list`] reads, from the bytes alone.
    fn read_all(zip: &[u8]) -> Result<(Vec<Entry>, usize), Problem> {
        let size = zip.len() as u64;
        let from = size.saturating_sub(TAIL);
        let end = match tail(&zip[from as usize..], from, size)? {
            Tail::End(e) => e,
            Tail::Zip64 { at, before } => zip64_end(&zip[at as usize..], before, at)?,
        };
        let d = end.directory as usize;
        directory(&zip[d..d + end.directory_size as usize], &end)
    }

    #[test]
    fn a_zip_lists_its_files_classic_and_zip64() {
        for zip64 in [false, true] {
            let zip = zip_of(
                &[
                    ("a.txt", 0, b"hello", crc(b"hello"), 5),
                    ("docs/", 0, b"", 0, 0),
                    ("docs/b.bin", 0, b"xyz", crc(b"xyz"), 3),
                ],
                zip64,
            );
            let (entries, left_out) = read_all(&zip).unwrap();
            assert_eq!(left_out, 0);
            let names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.dir)).collect();
            assert_eq!(
                names,
                [("a.txt", false), ("docs/", true), ("docs/b.bin", false)]
            );
            assert_eq!(entries[2].size, 3);
            assert_eq!(entries[2].parts(), ["docs", "b.bin"]);
            assert_eq!(entries[2].file_name(), "b.bin");
            // The hand-made archive says no date (0): none, not 1980.
            assert_eq!(entries[2].modified(), None);
            // Each one ends before the next begins.
            assert_eq!(entries[0].limit, entries[1].offset);
            let (n, x) = local_header(&zip[entries[0].offset as usize..], &entries[0]).unwrap();
            assert_eq!((n, x), (5, 0));
        }
    }

    /// Names that would leave the folder aren't offered; the others are.
    #[test]
    fn hostile_names_are_left_out() {
        let names = [
            "../up.txt",
            "/etc/passwd",
            "C:/evil.txt",
            "c:evil.txt",
            "a/../../up.txt",
            "a\\..\\up.txt",
            "ok/./x.txt",
            "nul\u{1}.txt",
            "a.txt:Zone.Identifier",
            "...",
            "ok.txt",
            "sub\\inner.txt",
        ];
        let files: Vec<_> = names
            .iter()
            .map(|n| (*n, 0u16, &b"x"[..], crc(b"x"), 1u64))
            .collect();
        let (entries, left_out) = read_all(&zip_of(&files, false)).unwrap();
        let kept: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(kept, ["ok.txt", "sub/inner.txt"]);
        assert_eq!(left_out, names.len() - 2);
    }

    #[test]
    fn overlapping_entries_refuse_the_archive() {
        let mut zip = zip_of(
            &[
                ("a.bin", 0, b"aaaa", crc(b"aaaa"), 4),
                ("b.bin", 0, b"bbbb", crc(b"bbbb"), 4),
            ],
            false,
        );
        // The second entry's offset made the first's (one block, two names: a bomb's trick).
        let end = zip.len() - END_FIXED;
        let dir = u32_at(&zip, end + 16).unwrap() as usize;
        let second = dir + CENTRAL_FIXED + "a.bin".len();
        zip[second + 42..second + 46].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(read_all(&zip), Err(Problem::Damaged("entries overlap")));
    }

    #[test]
    fn limits_and_lies_are_refused() {
        let zip = zip_of(&[("a.bin", 0, b"aaaa", crc(b"aaaa"), 4)], false);
        let end = zip.len() - END_FIXED;
        // A table larger than MDM reads.
        let mut big = zip.clone();
        big[end + 12..end + 16].copy_from_slice(&(MAX_DIRECTORY as u32 + 1).to_le_bytes());
        assert_eq!(read_all(&big), Err(Problem::TooBig));
        // More entries than the table could hold.
        let mut many = zip.clone();
        many[end + 8..end + 12].copy_from_slice(&[9, 0, 9, 0]);
        assert!(matches!(read_all(&many), Err(Problem::Damaged(_))));
        // A table past the archive's end.
        let mut past = zip.clone();
        past[end + 16..end + 20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(read_all(&past), Err(Problem::Damaged(_))));
        // An entry's data said to run past the table.
        let mut long = zip.clone();
        let dir = u32_at(&zip, end + 16).unwrap() as usize;
        long[dir + 20..dir + 24].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(matches!(read_all(&long), Err(Problem::Damaged(_))));
        // Split over disks.
        let mut spanned = zip.clone();
        spanned[end + 4..end + 6].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(read_all(&spanned), Err(Problem::Spanned));
        // Not a ZIP at all, and one with bytes after its end.
        assert_eq!(
            read_all(b"just some bytes, no zip here"),
            Err(Problem::NotZip)
        );
        let mut after = zip.clone();
        after.extend_from_slice(b"junk");
        assert_eq!(read_all(&after), Err(Problem::NotZip));
    }

    #[test]
    fn methods_and_encryption_say_why() {
        let zip = zip_of(&[("a.bz2", 12, b"x", 0, 1)], false);
        let (entries, _) = read_all(&zip).unwrap();
        assert_eq!(entries[0].refused(), Some(Problem::Method(12)));
        assert_eq!(method_name(12), "BZIP2");
        let mut enc = zip.clone();
        enc[6..8].copy_from_slice(&1u16.to_le_bytes());
        let end = enc.len() - END_FIXED;
        let dir = u32_at(&enc, end + 16).unwrap() as usize;
        enc[dir + 8..dir + 10].copy_from_slice(&0x0801u16.to_le_bytes());
        let (entries, _) = read_all(&enc).unwrap();
        assert_eq!(entries[0].refused(), Some(Problem::Encrypted));
    }

    #[test]
    fn cp437_names_read_as_archivers_wrote_them() {
        assert_eq!(CP437.chars().count(), 128);
        assert_eq!(decode_name(b"caf\x82.txt", false), "café.txt");
        assert_eq!(decode_name("café.txt".as_bytes(), true), "café.txt");
    }

    #[test]
    fn a_local_header_must_match_its_table() {
        let zip = zip_of(&[("a.txt", 0, b"hello", crc(b"hello"), 5)], false);
        let (entries, _) = read_all(&zip).unwrap();
        let e = &entries[0];
        let mut other = zip.clone();
        other[8..10].copy_from_slice(&8u16.to_le_bytes());
        assert!(local_header(&other, e).is_err());
        let mut long = zip.clone();
        long[28..30].copy_from_slice(&0xFFFFu16.to_le_bytes());
        assert!(
            local_header(&long, e).is_err(),
            "its data would run past its room"
        );
        assert!(local_header(b"PK\x01\x02", e).is_err());
    }

    /// Archives another implementation wrote (Python's zipfile: the fuzz seeds), a ZIP64 one
    /// and one with data descriptors among them, list as it does.
    #[test]
    fn another_archivers_zips_list_alike() {
        for (name, zip) in [
            (
                "classic",
                &include_bytes!("../../../fuzz/seeds/zip_directory/classic")[..],
            ),
            (
                "comment",
                &include_bytes!("../../../fuzz/seeds/zip_directory/comment")[..],
            ),
            (
                "zip64",
                &include_bytes!("../../../fuzz/seeds/zip_directory/zip64")[..],
            ),
            (
                "descriptor",
                &include_bytes!("../../../fuzz/seeds/zip_directory/descriptor")[..],
            ),
        ] {
            let (entries, left_out) = read_all(zip).expect(name);
            let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, ["readme.txt", "docs/", "docs/guide.txt"], "{name}");
            assert_eq!(left_out, 2, "{name}");
            assert_eq!(entries[0].modified(), Some((2025, 9, 1, 0, 0, 0)));
            for e in &entries {
                let (n, x) = local_header(&zip[e.offset as usize..], e).expect(name);
                let at = e.offset as usize + LOCAL_FIXED;
                assert!(same_name(&zip[at..at + n], e), "{name}");
                if e.name == "readme.txt" {
                    let data = at + n + x;
                    assert_eq!(&zip[data..data + 6], b"hello\n", "{name}");
                    assert_eq!(e.crc32, crc(b"hello\n"));
                }
            }
        }
    }
}

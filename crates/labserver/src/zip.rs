//! ZIPs for the tests of taking files out of a ZIP on its server (#1775): made in memory, the
//! same bytes every time, served whole or in parts (`/zip/{name}`), or with no parts at all
//! (`?no_range=1`, the server-wide fault too).
//!
//! - `files.zip`: stored and Deflate files, a folder, an empty file, names with a space and
//!   letters past ASCII, one written with a data descriptor (its sizes after its data), one
//!   encrypted and one in BZIP2 (both refused), and 1 MiB of the generator's bytes.
//! - `zip64.zip`: the same files in ZIP64 records (the archive's end, each entry's sizes and
//!   offset).
//! - `hostile.zip`: names that try to leave the folder (`../`, absolute paths, drive letters, a
//!   stream name, a control character), a symbolic link, and among them `ok.txt` and
//!   `sub/inner.txt`; `bad-crc.txt`, whose CRC-32 isn't its bytes', `liar.txt`, whose header
//!   names another file, and `short.txt`, whose size the table gives larger than it is.
//! - `overlap.zip`: two entries over the same bytes (a ZIP bomb's trick).
//! - `bigdir.zip`: an end record that claims a table larger than a reader takes.

use std::io::Write as _;
use std::sync::OnceLock;

/// The comment at each ZIP's end (a reader finds the end record before it).
const COMMENT: &[u8] = b"made by the MDM lab server";

/// Where the end record of `z` (one of `build`'s) begins.
fn end_of(z: &[u8]) -> usize {
    z.len() - 22 - COMMENT.len()
}

/// One file of a made-up ZIP.
#[derive(Clone)]
pub struct File {
    pub name: String,
    /// Its bytes, unpacked.
    pub data: Vec<u8>,
    /// 0 stored, 8 Deflate; any other is written as stored bytes under that method's number.
    pub method: u16,
    /// Bit 0: encrypted (its bytes as they are); bit 3: a data descriptor after its data.
    pub flags: u16,
    /// A CRC-32 other than its bytes'.
    pub crc: Option<u32>,
    /// A name in its local header other than the table's.
    pub local_name: Option<String>,
    /// A size in the table other than its bytes'.
    pub size: Option<u64>,
    /// A symbolic link (a Unix archiver's), its bytes the target.
    pub link: bool,
}

impl File {
    pub fn new(name: &str, data: impl Into<Vec<u8>>, method: u16) -> File {
        File {
            name: name.to_owned(),
            data: data.into(),
            method,
            flags: 0,
            crc: None,
            local_name: None,
            size: None,
            link: false,
        }
    }
}

fn crc(data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(data);
    h.finalize()
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("in memory");
    e.finish().expect("in memory")
}

/// The ZIP of `files`, in ZIP64 records when `zip64`.
pub fn build(files: &[File], zip64: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for f in files {
        let packed = match f.method {
            8 => deflate(&f.data),
            _ => f.data.clone(),
        };
        let crc32 = f.crc.unwrap_or_else(|| crc(&f.data));
        let size = f.size.unwrap_or(f.data.len() as u64);
        let flags = f.flags | 0x0800;
        let descriptor = flags & 0x08 != 0;
        let offset = out.len() as u64;
        let local_name = f.local_name.as_deref().unwrap_or(&f.name);
        let u16le = |v: u16| v.to_le_bytes();
        let u32le = |v: u32| v.to_le_bytes();
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&u16le(if zip64 { 45 } else { 20 }));
        out.extend_from_slice(&u16le(flags));
        out.extend_from_slice(&u16le(f.method));
        out.extend_from_slice(&[0, 0, 0x21, 0x5b]);
        let (lcrc, lc, lu) = if descriptor {
            (0, 0, 0)
        } else {
            (crc32, packed.len() as u32, size as u32)
        };
        out.extend_from_slice(&u32le(lcrc));
        out.extend_from_slice(&u32le(lc));
        out.extend_from_slice(&u32le(lu));
        out.extend_from_slice(&u16le(local_name.len() as u16));
        out.extend_from_slice(&u16le(0));
        out.extend_from_slice(local_name.as_bytes());
        out.extend_from_slice(&packed);
        if descriptor {
            out.extend_from_slice(b"PK\x07\x08");
            out.extend_from_slice(&u32le(crc32));
            out.extend_from_slice(&u32le(packed.len() as u32));
            out.extend_from_slice(&u32le(size as u32));
        }
        let mut extra = Vec::new();
        let (cc, cu, co) = if zip64 {
            extra.extend_from_slice(&u16le(1));
            extra.extend_from_slice(&u16le(24));
            extra.extend_from_slice(&size.to_le_bytes());
            extra.extend_from_slice(&(packed.len() as u64).to_le_bytes());
            extra.extend_from_slice(&offset.to_le_bytes());
            (u32::MAX, u32::MAX, u32::MAX)
        } else {
            (packed.len() as u32, size as u32, offset as u32)
        };
        let mode: u32 = if f.link {
            0o120_777
        } else if f.name.ends_with('/') {
            0o040_755
        } else {
            0o100_644
        };
        central.extend_from_slice(b"PK\x01\x02");
        // Made by Unix (3), version 2.0 or 4.5.
        central.extend_from_slice(&[if zip64 { 45 } else { 20 }, 3]);
        central.extend_from_slice(&u16le(if zip64 { 45 } else { 20 }));
        central.extend_from_slice(&u16le(flags));
        central.extend_from_slice(&u16le(f.method));
        central.extend_from_slice(&[0, 0, 0x21, 0x5b]);
        central.extend_from_slice(&u32le(crc32));
        central.extend_from_slice(&u32le(cc));
        central.extend_from_slice(&u32le(cu));
        central.extend_from_slice(&u16le(f.name.len() as u16));
        central.extend_from_slice(&u16le(extra.len() as u16));
        central.extend_from_slice(&u16le(0));
        central.extend_from_slice(&u16le(0));
        central.extend_from_slice(&u16le(0));
        central.extend_from_slice(&u32le(mode << 16));
        central.extend_from_slice(&u32le(co));
        central.extend_from_slice(f.name.as_bytes());
        central.extend_from_slice(&extra);
    }
    let dir_at = out.len() as u64;
    out.extend_from_slice(&central);
    let n = files.len() as u64;
    if zip64 {
        let rec_at = out.len() as u64;
        out.extend_from_slice(b"PK\x06\x06");
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&[45, 3, 45, 0]);
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(central.len() as u64).to_le_bytes());
        out.extend_from_slice(&dir_at.to_le_bytes());
        out.extend_from_slice(b"PK\x06\x07");
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&rec_at.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
    }
    let comment = COMMENT;
    out.extend_from_slice(b"PK\x05\x06");
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
    out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    out.extend_from_slice(comment);
    out
}

/// `files.zip`'s files (and `zip64.zip`'s).
pub fn files() -> Vec<File> {
    let mut big = vec![0u8; 1 << 20];
    crate::content::fill(1775, 0, &mut big);
    let text: Vec<u8> = (0..4000)
        .flat_map(|i| {
            format!("line {i}: the quick brown fox jumps over the lazy dog\n").into_bytes()
        })
        .collect();
    let mut stream = File::new("docs/stream.txt", text.clone(), 8);
    stream.flags = 0x08;
    let mut secret = File::new("secret.txt", b"not really encrypted".to_vec(), 0);
    secret.flags = 0x01;
    vec![
        File::new(
            "readme.txt",
            b"Files picked out of a ZIP on its server.\n".to_vec(),
            0,
        ),
        File::new("docs/", Vec::new(), 0),
        File::new("docs/guide.txt", text, 8),
        stream,
        File::new("data/random.bin", big, 0),
        File::new("data/empty.txt", Vec::new(), 0),
        File::new("my files/photo 1.txt", b"a photo, in words\n".to_vec(), 8),
        File::new(
            "ünïcode/café.txt",
            "café, crème brûlée\n".as_bytes().to_vec(),
            8,
        ),
        secret,
        File::new("old.bz2", b"BZh9 pretend".to_vec(), 12),
    ]
}

/// `hostile.zip`'s files.
pub fn hostile() -> Vec<File> {
    let mut link = File::new("up", b"../../etc".to_vec(), 0);
    link.link = true;
    let mut bad_crc = File::new("bad-crc.txt", b"these bytes aren't the CRC's\n".to_vec(), 8);
    bad_crc.crc = Some(0x1234_5678);
    let mut liar = File::new("liar.txt", b"my header names another file\n".to_vec(), 0);
    liar.local_name = Some("lies.txt".into());
    let mut short = File::new("short.txt", b"shorter than the table says\n".to_vec(), 0);
    short.size = Some(1000);
    let mut out: Vec<File> = [
        "../escape.txt",
        "/etc/passwd",
        "C:/evil.txt",
        "D:evil.txt",
        "\\\\server\\share\\evil.txt",
        "a/../../up.txt",
        "a\\..\\..\\up.txt",
        "ok.txt:Zone.Identifier",
        "ctl\u{7}.txt",
        "...",
    ]
    .iter()
    .map(|n| File::new(n, b"must never be written\n".to_vec(), 0))
    .collect();
    out.extend([
        File::new("ok.txt", b"the one safe file\n".to_vec(), 0),
        File::new("sub/inner.txt", b"and its neighbour\n".to_vec(), 8),
        link,
        bad_crc,
        liar,
        short,
    ]);
    out
}

/// The ZIP `name` serves, made once.
pub fn zip(name: &str) -> Option<&'static [u8]> {
    static FILES: OnceLock<Vec<u8>> = OnceLock::new();
    static ZIP64: OnceLock<Vec<u8>> = OnceLock::new();
    static HOSTILE: OnceLock<Vec<u8>> = OnceLock::new();
    static OVERLAP: OnceLock<Vec<u8>> = OnceLock::new();
    static BIGDIR: OnceLock<Vec<u8>> = OnceLock::new();
    Some(match name {
        "files.zip" => FILES.get_or_init(|| build(&files(), false)),
        "zip64.zip" => ZIP64.get_or_init(|| build(&files(), true)),
        "hostile.zip" => HOSTILE.get_or_init(|| build(&hostile(), false)),
        "overlap.zip" => OVERLAP.get_or_init(|| {
            let mut z = build(
                &[
                    File::new("a.bin", vec![b'a'; 4096], 0),
                    File::new("b.bin", vec![b'a'; 4096], 0),
                ],
                false,
            );
            // The second entry's header offset made the first's.
            let end = end_of(&z);
            let dir = u32::from_le_bytes(z[end + 16..end + 20].try_into().unwrap());
            let second = dir as usize + 46 + "a.bin".len();
            z[second + 42..second + 46].copy_from_slice(&0u32.to_le_bytes());
            z
        }),
        "bigdir.zip" => BIGDIR.get_or_init(|| {
            let mut z = build(&[File::new("a.txt", b"a".to_vec(), 0)], false);
            // The table said to be 64 MiB long, from the archive's start.
            let end = end_of(&z);
            z[end + 12..end + 16].copy_from_slice(&(64u32 << 20).to_le_bytes());
            z[end + 16..end + 20].copy_from_slice(&0u32.to_le_bytes());
            z
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The made-up ZIPs begin and end as ZIPs do (the engine's tests read them whole).
    #[test]
    fn the_zips_end_where_they_say() {
        for name in ["files.zip", "zip64.zip", "hostile.zip"] {
            let z = zip(name).unwrap();
            let end = end_of(z);
            assert_eq!(&z[end..end + 4], b"PK\x05\x06", "{name}");
            assert_eq!(&z[..4], b"PK\x03\x04", "{name}");
        }
        let z = zip("zip64.zip").unwrap();
        let loc = end_of(z) - 20;
        assert_eq!(&z[loc..loc + 4], b"PK\x06\x07");
        assert!(zip("nothing.zip").is_none());
    }
}

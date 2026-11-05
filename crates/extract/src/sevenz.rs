//! 7z (#37) with sevenz-rust2's decoder: LZMA, LZMA2, BCJ, Deflate and AES-256; a split
//! `x.7z.001` read through [`MultiFile`]. Links (a reparse point, or a Unix link in the high
//! bits of the attributes) are left out.

use std::io::{Read, Seek};
use std::path::Path;

use sevenz_rust2::{ArchiveReader, Password};

use crate::Error;
use crate::safe::Writer;
use crate::volumes::{MultiFile, VolumeKind, volume_key};

/// Windows' reparse point attribute; 7-Zip's Unix extension (mode in the high 16 bits).
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const UNIX_EXTENSION: u32 = 0x8000;
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;

pub fn extract(archive: &Path, w: &mut Writer, password: Option<&str>) -> Result<(), Error> {
    let name = archive
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if volume_key(&name).is_some_and(|v| v.kind == VolumeKind::Split) {
        let multi = MultiFile::open(archive)?.map_err(Error::MissingVolume)?;
        run(multi, w, password)
    } else {
        run(std::fs::File::open(archive)?, w, password)
    }
}

fn sevenz_error(e: sevenz_rust2::Error, password: Option<&str>) -> Error {
    use sevenz_rust2::Error as E;
    match e {
        E::PasswordRequired => Error::NeedsPassword,
        E::MaybeBadPassword(_) => Error::WrongPassword,
        E::ChecksumVerificationFailed if password.is_some() => Error::WrongPassword,
        E::Io(e, _) if e.kind() != std::io::ErrorKind::InvalidData => Error::Io(e),
        other => Error::Damaged(other.to_string()),
    }
}

fn run<R: Read + Seek>(reader: R, w: &mut Writer, password: Option<&str>) -> Result<(), Error> {
    let pw = password.map(Password::from).unwrap_or_else(Password::empty);
    let mut ar = ArchiveReader::new(reader, pw).map_err(|e| sevenz_error(e, password))?;
    // Our own error, kept while the decoder's callback stops it.
    let mut ours: Option<Error> = None;
    let res = ar.for_each_entries(|entry, data| {
        let name = entry.name.clone();
        let attrs = if entry.has_windows_attributes {
            entry.windows_attributes
        } else {
            0
        };
        let unix_type = (attrs & UNIX_EXTENSION != 0).then_some((attrs >> 16) & S_IFMT);
        let r = if entry.is_anti_item {
            Ok(())
        } else if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unix_type.is_some_and(|t| t != 0 && t != S_IFREG && t != S_IFDIR)
        {
            w.skip(&name, "link");
            std::io::copy(data, &mut std::io::sink())
                .map(|_| ())
                .map_err(Error::Io)
        } else if entry.is_directory {
            w.dir(&name)
        } else {
            let mode = unix_type.map(|_| attrs >> 16);
            w.file(&name, Some(entry.size), data, mode)
        };
        match r {
            Ok(()) => Ok(true),
            Err(e) => {
                ours = Some(e);
                Ok(false)
            }
        }
    });
    if let Some(e) = ours {
        return Err(match e {
            // The data didn't check out while being written: with a password, it's the wrong one.
            Error::Damaged(_) if password.is_some() => Error::WrongPassword,
            e => e,
        });
    }
    res.map_err(|e| sevenz_error(e, password))
}

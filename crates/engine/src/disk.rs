//! Disk preflight: will a download fit where it is going? (notes/PLAN.md "Disk preflight";
//! lessons from AB Download Manager, Motrix, JDownloader, aria2.)

use std::path::Path;

use sysinfo::Disks;

/// Room kept free beyond the file itself (the OS, our list file, other programs).
pub const MARGIN: u64 = 128 << 20;
/// The longest the volume probe may take before the check is skipped.
pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// FAT32's largest file: 4 GiB − 1.
pub const FAT32_MAX: u64 = (4 << 30) - 1;

/// Free space and file system of the volume holding `path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub free: u64,
    pub file_system: String,
}

/// The volume with the longest mount point that contains `path` (None when unknown, e.g. a
/// network path sysinfo doesn't list: the check is then skipped).
pub fn volume_of(path: &Path) -> Option<Volume> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let disks = Disks::new_with_refreshed_list().ok()?;
    disks
        .iter()
        .filter(|d| starts_with_ignoring_verbatim(&path, d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| Volume {
            free: d.available_space(),
            file_system: d.file_system().to_string_lossy().into_owned(),
        })
}

/// `canonicalize` on Windows yields `\\?\C:\…`; mount points are `C:\`.
fn starts_with_ignoring_verbatim(path: &Path, mount: &Path) -> bool {
    let p = path.to_string_lossy();
    let p = p.strip_prefix(r"\\?\").unwrap_or(&p);
    let m = mount.to_string_lossy();
    p.to_ascii_lowercase().starts_with(&m.to_ascii_lowercase())
}

/// Why a file of `size` bytes can't go on `volume`, if it can't.
pub fn check(volume: &Volume, size: u64) -> Result<(), Refusal> {
    let fs = volume.file_system.to_ascii_uppercase();
    if size > FAT32_MAX && (fs == "FAT32" || fs == "VFAT" || fs == "FAT") {
        return Err(Refusal::TooBigForFileSystem {
            size,
            file_system: volume.file_system.clone(),
        });
    }
    let needed = size.saturating_add(MARGIN);
    if needed > volume.free {
        return Err(Refusal::NoSpace {
            needed: size,
            free: volume.free.saturating_sub(MARGIN.min(volume.free)),
        });
    }
    Ok(())
}

/// A size for messages: `512 bytes`, `1.5 MB`, `4.00 GB` (binary units, as IDM shows them).
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    let decimals = if v < 10.0 {
        2
    } else if v < 100.0 {
        1
    } else {
        0
    };
    format!("{v:.decimals$} {}", UNITS[unit])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoSpace { needed: u64, free: u64 },
    TooBigForFileSystem { size: u64, file_system: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(free: u64, fs: &str) -> Volume {
        Volume {
            free,
            file_system: fs.into(),
        }
    }

    #[test]
    fn needs_the_file_plus_a_margin() {
        assert!(check(&vol(10 << 30, "NTFS"), 1 << 30).is_ok());
        assert!(matches!(
            check(&vol((1 << 30) + MARGIN - 1, "NTFS"), 1 << 30),
            Err(Refusal::NoSpace { .. })
        ));
    }

    #[test]
    fn fat32_refuses_4_gib_files() {
        assert!(check(&vol(64 << 30, "FAT32"), FAT32_MAX).is_ok());
        assert!(matches!(
            check(&vol(64 << 30, "FAT32"), 4 << 30),
            Err(Refusal::TooBigForFileSystem { .. })
        ));
        assert!(check(&vol(64 << 30, "exFAT"), 8 << 30).is_ok());
    }

    #[test]
    fn sizes_read_like_idms() {
        assert_eq!(human(512), "512 bytes");
        assert_eq!(human(1536), "1.50 KB");
        assert_eq!(human(4 << 30), "4.00 GB");
        assert_eq!(human(150 << 20), "150 MB");
    }

    #[test]
    fn finds_the_volume_of_a_real_path() {
        let v = volume_of(&std::env::temp_dir()).expect("temp dir is on a listed volume");
        assert!(v.free > 0 && !v.file_system.is_empty(), "{v:?}");
    }
}

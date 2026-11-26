//! 7z and RAR through 7-Zip, run as its own program (#37): UnRAR's licence forbids using its
//! code in a GPL program, so MDM never links it, and 7z's decoder isn't built in either (it cost
//! about 1 MB). 7-Zip writes into the staging folder, held to the same limits as our own
//! extraction while it runs ([`supervise`]); what it wrote is then checked as our own entries
//! are ([`safe::check_tree`]) before anything is moved into place.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;

use crate::Error;
use crate::safe::{self, Writer};
use crate::supervise::{self, Tally};

/// 7-Zip's program, the first there of [`candidates`].
pub fn find_7zip() -> Option<PathBuf> {
    candidates(|var| std::env::var_os(var))
        .into_iter()
        .find(|p| p.is_file())
}

/// Where 7-Zip's program may be, in the order looked at (`var`: an environment variable). On
/// Windows 7-Zip's own install folders first, then the PATH, then the 7-Zip another archiver
/// brings with it (#1525): PeaZip's, in its install folder. Elsewhere `7zz`, `7z` or `7za` on
/// the PATH (Linux and macOS packages name it so). NanaZip, a Store app, has no folder to look
/// in: its command line is `NanaZipC.exe` on the PATH (an app execution alias, a file to
/// `is_file`). Any other (a portable copy, a per-user install) is chosen in Options → Archives.
fn candidates(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Vec<PathBuf> {
    const PROGRAMS: [&str; 3] = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"];
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for dir in PROGRAMS.iter().filter_map(|v| var(v)) {
            candidates.push(PathBuf::from(dir).join("7-Zip").join("7z.exe"));
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["7z.exe", "7za.exe", "NanaZipC.exe"]
    } else {
        &["7zz", "7z", "7za"]
    };
    if let Some(path) = var("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in names {
                candidates.push(dir.join(n));
            }
        }
    }
    if cfg!(windows) {
        for dir in PROGRAMS.iter().filter_map(|v| var(v)) {
            candidates.push(
                PathBuf::from(dir)
                    .join("PeaZip")
                    .join("res")
                    .join("bin")
                    .join("7z")
                    .join("7z.exe"),
            );
        }
    }
    candidates
}

/// 7-Zip extracts `archive` into the writer's staging folder, watched while it runs
/// ([`supervise::run`]): a cancel stops it (or keeps it from starting), and so does more in the
/// staging folder than the writer's limits allow, bytes or entries. Then what it wrote is checked
/// as our own entries are, and the writer counts it (the report's bytes).
pub fn extract(
    tool: &Path,
    archive: &Path,
    w: &mut Writer,
    password: Option<&str>,
) -> Result<(), Error> {
    // Full paths: 7-Zip runs in the staging folder, MDM's own and writable, never in the folder
    // MDM was started in (its install folder from the Start menu, #834).
    let full = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_owned());
    let (root, archive) = (full(w.root()), full(archive));
    let mut cmd = Command::new(tool);
    cmd.arg("x").arg("-y").arg("-bso0").arg("-bsp0");
    // A password goes in on 7-Zip's standard input, where it asks for one when the archive is
    // encrypted, never on its command line, which other programs can read. Without one, an
    // empty `-p`: 7-Zip then fails at once on an encrypted archive instead of waiting for an
    // answer.
    if password.is_none() {
        cmd.arg("-p");
    }
    cmd.arg(format!("-o{}", root.display()))
        .arg("--")
        .arg(&archive)
        .current_dir(&root);
    // The password is never logged.
    tracing::info!("extract: {} x {}", tool.display(), archive.display());
    let limits = w.limits();
    let mut look = || Tally::over(&root, limits).map(|_| ());
    let out = supervise::run(cmd, password, None, w.cancel(), &mut look).inspect_err(|e| {
        if matches!(e, Error::TooBig { .. } | Error::TooMany) {
            tracing::warn!("extract: 7-Zip stopped: {e}");
        }
    })?;
    // Cancelled right as it ended: nothing is placed.
    if w.cancel().is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(Error::Cancelled);
    }
    let err = out.errors.to_lowercase();
    if !out.status.success() {
        return Err(
            if err.contains("wrong password") || err.contains("encrypted") {
                if password.is_some() {
                    Error::WrongPassword
                } else {
                    Error::NeedsPassword
                }
            } else if err.contains("missing volume") || err.contains("unexpected end of archive") {
                Error::MissingVolume(0)
            } else {
                Error::Damaged(
                    err.lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("7-Zip failed")
                        .trim()
                        .to_owned(),
                )
            },
        );
    }
    for (name, why) in safe::check_tree(w.root(), 64)? {
        w.skip(&name, why);
    }
    w.wrote(Tally::of(w.root()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn looked_at(var: impl Fn(&str) -> Option<OsString>) -> Vec<String> {
        candidates(var)
            .iter()
            .map(|p| p.display().to_string())
            .collect()
    }

    /// #1525: 7-Zip's own folders first, then the PATH (NanaZip's command line there too), then
    /// the 7-Zip PeaZip brings with it, so an installed 7-Zip stays the one used.
    #[cfg(windows)]
    #[test]
    fn seven_zip_is_looked_for_in_its_folders_the_path_then_peazip() {
        let var = |v: &str| match v {
            "ProgramFiles" => Some(OsString::from(r"C:\Program Files")),
            "ProgramFiles(x86)" => Some(OsString::from(r"C:\Program Files (x86)")),
            "PATH" => Some(OsString::from(r"C:\tools;D:\bin")),
            _ => None,
        };
        assert_eq!(
            looked_at(var),
            [
                r"C:\Program Files\7-Zip\7z.exe",
                r"C:\Program Files (x86)\7-Zip\7z.exe",
                r"C:\tools\7z.exe",
                r"C:\tools\7za.exe",
                r"C:\tools\NanaZipC.exe",
                r"D:\bin\7z.exe",
                r"D:\bin\7za.exe",
                r"D:\bin\NanaZipC.exe",
                r"C:\Program Files\PeaZip\res\bin\7z\7z.exe",
                r"C:\Program Files (x86)\PeaZip\res\bin\7z\7z.exe",
            ]
        );
        // Nothing set: nowhere to look, and no panic.
        assert!(looked_at(|_| None).is_empty());
    }

    /// Linux and macOS: the names their packages give it, on the PATH.
    #[cfg(not(windows))]
    #[test]
    fn seven_zip_is_looked_for_on_the_path() {
        let var = |v: &str| (v == "PATH").then(|| OsString::from("/usr/bin:/opt/bin"));
        assert_eq!(
            looked_at(var),
            [
                "/usr/bin/7zz",
                "/usr/bin/7z",
                "/usr/bin/7za",
                "/opt/bin/7zz",
                "/opt/bin/7z",
                "/opt/bin/7za",
            ]
        );
        assert!(looked_at(|_| None).is_empty());
    }
}

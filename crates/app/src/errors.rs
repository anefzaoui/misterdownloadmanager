//! Why a download failed, in the interface language (every visible text goes through the
//! catalog; D-019). The engine's own English (`Display`) stays for the command line and the
//! event log. Every engine error is named here, so a new one can't reach the list untranslated.
//! Data inside a message (status codes, paths, sizes) goes in as isolates (crate::i18n::fmt).

use mdm_engine::{Error, NetKind};

use crate::i18n::{fmt, ltr, t};

/// The sentence the list, the progress window and Properties show for `e`.
pub fn describe(e: &Error) -> String {
    match e {
        Error::Http(h) => http(h),
        Error::Status(s) => fmt(
            t("the server answered {status}"),
            &[("status", &ltr(&s.to_string()))],
        ),
        Error::Throttled(s, _) => fmt(
            t("the server is busy ({status}); try again later"),
            &[("status", &ltr(&s.to_string()))],
        ),
        Error::BadRange(what) => fmt(
            t("the server sent an unexpected part of the file ({detail})"),
            &[("detail", &ltr(what))],
        ),
        Error::ShortBody => t("the connection closed before the part was complete").to_owned(),
        Error::FileChanged => t("the file changed on the server").to_owned(),
        Error::NoRanges => {
            t("the server can't send parts of the file, so it can't be resumed").to_owned()
        }
        Error::BadState(what) => fmt(t("cannot resume: {detail}"), &[("detail", &ltr(what))]),
        Error::Io(io) => fmt(
            t("disk error: {error}"),
            &[("error", &os_text(&io.to_string()))],
        ),
        Error::NoSpace { needed, free } => fmt(
            t("not enough disk space: {needed} needed, {free} free"),
            &[
                ("needed", &crate::model::size(*needed)),
                ("free", &crate::model::size(*free)),
            ],
        ),
        Error::DiskFull => t("the disk is full").to_owned(),
        Error::TooBigForDrive { size, file_system } => fmt(
            t("{size} is too big for this {file_system} drive (files up to 4 GB)"),
            &[
                ("size", &crate::model::size(*size)),
                ("file_system", &ltr(file_system)),
            ],
        ),
        Error::Cancelled => t("cancelled").to_owned(),
        Error::InUse(path) => fmt(
            t("already being downloaded: {path}"),
            &[("path", &ltr(&path.display().to_string()))],
        ),
        Error::DestinationUnavailable(path) => fmt(
            t("the folder {path} is not available (drive removed or network share offline?)"),
            &[("path", &ltr(&path.display().to_string()))],
        ),
        Error::DriveGone => t("the drive or network share is no longer available").to_owned(),
        Error::PathTooLong(path) => fmt(
            t("the path is too long for Windows: {path}"),
            &[("path", &ltr(&path.display().to_string()))],
        ),
        Error::GotWebPage => t(
            "the server sent a web page instead of the file (sign-in needed or the link expired?)",
        )
        .to_owned(),
        Error::RangesUnavailable => t("the server did not send the requested part").to_owned(),
        Error::LinkExpired => {
            t("the download link has expired (refresh the download address)").to_owned()
        }
        Error::SlowConnection => {
            t("much slower than the other connections; reconnecting").to_owned()
        }
        Error::ChecksumMismatch(algorithm) => fmt(
            t("the file's {algorithm} checksum doesn't match (the data is kept; redownload it)"),
            &[("algorithm", &ltr(algorithm))],
        ),
        Error::FtpReply { code, text } => fmt(
            // i18n: {code} is the FTP server's reply number (such as 550), {reply} the words it sent with it (left as sent)
            t("the FTP server answered {code} {reply}"),
            &[("code", &ltr(&code.to_string())), ("reply", &ltr(text))],
        ),
        Error::Net { kind, cause } => {
            let what = match kind {
                NetKind::Connect => t("cannot connect to the server"),
                NetKind::Timeout => t("the server did not answer in time"),
                NetKind::Lost => t("the connection was lost while receiving data"),
                // i18n: an FTPS (or other TLS) connection could not be secured
                NetKind::Tls => t("the secure connection failed"),
            };
            let cause = os_text(cause);
            if cause.is_empty() {
                what.to_owned()
            } else {
                fmt(
                    t("{problem} ({cause})"),
                    &[("problem", what), ("cause", &cause)],
                )
            }
        }
    }
}

/// A network error in words a user can act on, with Windows' own description of the cause
/// (Windows words it in its display language) but not the address, which the window shows.
fn http(e: &reqwest::Error) -> String {
    let what = if e.is_timeout() {
        t("the server did not answer in time")
    } else if e.is_connect() {
        t("cannot connect to the server")
    } else if e.is_body() || e.is_decode() {
        t("the connection was lost while receiving data")
    } else if e.is_redirect() {
        t("too many redirects")
    } else {
        t("network error")
    };
    let mut cause = None;
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        cause = Some(s.to_string());
        source = s.source();
    }
    match cause.map(|c| os_text(&c)) {
        Some(c) if !c.is_empty() && !c.contains("://") => fmt(
            t("{problem} ({cause})"),
            &[("problem", what), ("cause", &c)],
        ),
        _ => what.to_owned(),
    }
}

/// An OS message without its "(os error N)" tail and final period.
fn os_text(s: &str) -> String {
    const TAIL: &str = " (os error "; // not translated: Rust's own wording, parsed
    let s = match s.rfind(TAIL) {
        Some(i) if s.ends_with(')') => &s[..i],
        _ => s,
    };
    s.trim().trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_their_data() {
        assert!(describe(&Error::Status(reqwest::StatusCode::NOT_FOUND)).contains("404"));
        assert_eq!(
            os_text("Access is denied. (os error 5)"),
            "Access is denied"
        );
    }
}

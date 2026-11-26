//! Real-Debrid's REST API 1.0 and its OAuth v2 device flow, as documented at
//! <https://api.real-debrid.com/> (read 2026-11-04). Only what MDM uses: the account, the
//! supported hosts, unrestricting a link, and signing in.
//!
//! - Every call carries `Authorization: Bearer <token>` (never `?auth_token=`, which ends up in
//!   logs).
//! - Errors are HTTP 4xx/5xx with `{"error": "...", "error_code": N}` (`fault`).
//! - `remote` is always 0: MDM is a desktop program, and remote traffic is for servers.

use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{Fault, Http, ServiceKind};

/// The client id Real-Debrid gives open-source programs for the device flow ("Opensource Apps").
pub const OPEN_SOURCE_CLIENT_ID: &str = "X245A4XAIBGVM";
/// The grant the device flow and refreshes use.
const DEVICE_GRANT: &str = "http://oauth.net/grant_type/device/1.0";

/// Where Real-Debrid answers (tests: the lab server's fake, `Endpoints::at`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub rest: Url,
    pub oauth: Url,
    /// A 503 without an error code to `unrestrict/link` (#1868) is asked again this often...
    pub busy_every: Duration,
    /// ...for up to this long, then taken as the file being gone.
    pub busy_for: Duration,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            rest: Url::parse("https://api.real-debrid.com/rest/1.0/").expect("constant"),
            oauth: Url::parse("https://api.real-debrid.com/oauth/v2/").expect("constant"),
            busy_every: Duration::from_secs(5 * 60),
            busy_for: Duration::from_secs(60 * 60),
        }
    }
}

impl Endpoints {
    /// The lab server's fake Real-Debrid at `base` (`http://127.0.0.1:PORT/`), whose waits
    /// after a bare 503 are a moment's, not minutes'.
    pub fn at(base: &Url) -> Self {
        Endpoints {
            rest: base.join("rd/rest/1.0/").expect("relative"),
            oauth: base.join("rd/oauth/v2/").expect("relative"),
            busy_every: Duration::from_millis(50),
            busy_for: Duration::from_millis(400),
        }
    }
}

/// `GET /user`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct User {
    pub username: String,
    /// "premium" or "free".
    #[serde(rename = "type")]
    pub kind: String,
    /// Premium seconds left.
    #[serde(default)]
    pub premium: i64,
}

/// `POST /unrestrict/link`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Unrestricted {
    pub filename: Option<String>,
    /// 0: unknown.
    #[serde(default)]
    pub filesize: i64,
    /// "Max Chunks allowed".
    #[serde(default)]
    pub chunks: i64,
    /// The direct link.
    pub download: Option<String>,
}

/// `GET oauth/v2/device/code`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub interval: u64,
    pub expires_in: u64,
    pub verification_url: String,
}

/// `GET oauth/v2/device/credentials`: the user-bound credentials, once the user allowed MDM.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct DeviceCredentials {
    pub client_id: String,
    pub client_secret: String,
}

/// `POST oauth/v2/token`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Token {
    pub access_token: String,
    pub expires_in: u64,
    pub refresh_token: String,
}

/// An error answer: `{"error": "...", "error_code": N}` (the code is optional).
#[derive(Debug, Deserialize)]
struct ErrorBody {
    #[serde(default)]
    error: String,
    error_code: Option<i64>,
}

/// A 503 without an error code: Real-Debrid's answer for a file it can't get, and also what it
/// (or a proxy in front of it) sends while it is down for maintenance.
fn bare_503(status: u16, body: &[u8]) -> bool {
    status == 503
        && serde_json::from_slice::<ErrorBody>(body)
            .ok()
            .is_none_or(|e| e.error_code.is_none())
}

/// After bare 503s for `since`: how long to wait before asking again (`every`, the last wait
/// ending at `limit`), or None once `limit` has gone by and the file is taken as gone.
fn after_bare_503(since: Duration, every: Duration, limit: Duration) -> Option<Duration> {
    limit
        .checked_sub(since)
        .filter(|left| !left.is_zero())
        .map(|left| left.min(every))
}

/// What an error answer means for MDM (the API documentation's list of codes).
pub(crate) fn fault(status: u16, body: &[u8], retry_after: Option<u64>) -> Fault {
    use mdm_engine::ServiceFault as F;
    let e: ErrorBody = serde_json::from_slice(body).unwrap_or(ErrorBody {
        error: String::new(),
        error_code: None,
    });
    let fault = match (e.error_code, status) {
        (Some(8), _) | (None, 401) => F::SignIn,
        (Some(9 | 14 | 15), _) => F::AccountBlocked,
        (Some(20), _) => F::NotPremium,
        (Some(16), _) => return Fault::Unsupported,
        (Some(17 | 19), _) => F::HostDown,
        (Some(18 | 23 | 36), _) => F::LimitReached,
        (Some(21), _) => F::TooManyActive,
        (Some(22), _) => F::IpRefused,
        // A 503 with no error code is the service being down (maintenance, a proxy in front of
        // it), as every 5xx is; `unrestrict` gives it an hour before it takes the file as gone.
        (Some(24), _) => F::FileUnavailable,
        (Some(28 | 35), _) => F::FileRefused,
        (Some(5 | 34), _) | (None, 429) => {
            F::TooManyRequests(retry_after.map(std::time::Duration::from_secs))
        }
        (Some(25 | -1), _) => F::ServiceDown,
        (None, 403) => F::AccountBlocked,
        (None, s) if s >= 500 => F::ServiceDown,
        _ if !e.error.is_empty() => F::Other(e.error),
        _ => F::Other(format!("HTTP {status}")),
    };
    Fault::Service(fault)
}

/// The account: name, premium or not.
pub(crate) async fn user(http: &Http, token: &str) -> Result<User, Fault> {
    let url = http.services.rd.rest.join("user").expect("relative");
    http.json(http.get(&url).bearer_auth(token), fault).await
}

/// The domains Real-Debrid supports (no sign-in needed).
pub(crate) async fn domains(http: &Http) -> Result<Vec<String>, Fault> {
    let url = http
        .services
        .rd
        .rest
        .join("hosts/domains")
        .expect("relative");
    http.json(http.get(&url), fault).await
}

/// Today's direct link for `link`.
///
/// A bare 503 (`bare_503`) may be the service down for a while: asked again every 5 minutes
/// (`Endpoints::busy_every`), said in the event log, for up to an hour (`busy_for`); then the
/// file is taken as gone (#1868), so a dead file isn't asked for forever.
pub(crate) async fn unrestrict(
    http: &Http,
    token: &str,
    link: &Url,
    note: &(dyn Fn(String, Option<f32>) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<Unrestricted, Fault> {
    use mdm_engine::ServiceFault as F;
    let rd = &http.services.rd;
    let url = rd.rest.join("unrestrict/link").expect("relative");
    let form = [("link", link.as_str()), ("remote", "0")];
    let first = Instant::now();
    loop {
        let req = http.post_form(&url, &form).bearer_auth(token);
        let answer = http.send(ServiceKind::RealDebrid, req).await?;
        if !bare_503(answer.0, &answer.1) {
            return http.read_json(answer, fault);
        }
        let Some(wait) = after_bare_503(first.elapsed(), rd.busy_every, rd.busy_for) else {
            return Err(Fault::Service(F::FileUnavailable));
        };
        let secs = wait.as_secs_f32().ceil() as u64;
        let when = if secs >= 60 {
            format!("{} min", secs.div_ceil(60))
        } else {
            format!("{secs} s")
        };
        note(
            format!("Real-Debrid answers 503 (down or busy): asking again in {when}"),
            None,
        );
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = cancel.cancelled() => return Err(Fault::Service(F::Other("cancelled".into()))),
        }
    }
}

/// Sign-in by code, step 1: the code the user types at Real-Debrid.
pub async fn device_code(http: &Http) -> Result<DeviceCode, Fault> {
    let mut url = http
        .services
        .rd
        .oauth
        .join("device/code")
        .expect("relative");
    url.query_pairs_mut()
        .append_pair("client_id", OPEN_SOURCE_CLIENT_ID)
        .append_pair("new_credentials", "yes");
    http.json(http.get(&url), fault).await
}

/// Step 2, asked every `interval` seconds: the user-bound credentials once the user allowed
/// MDM, None while waiting.
pub(crate) async fn device_credentials(
    http: &Http,
    device_code: &str,
) -> Result<Option<DeviceCredentials>, Fault> {
    let mut url = http
        .services
        .rd
        .oauth
        .join("device/credentials")
        .expect("relative");
    url.query_pairs_mut()
        .append_pair("client_id", OPEN_SOURCE_CLIENT_ID)
        .append_pair("code", device_code);
    match http.json(http.get(&url), fault).await {
        Ok(c) => Ok(Some(c)),
        // Not allowed yet: Real-Debrid answers with an error until then (its wording isn't
        // documented); a service that is down or asks to slow down is said as such.
        Err(Fault::Service(
            mdm_engine::ServiceFault::ServiceDown | mdm_engine::ServiceFault::TooManyRequests(_),
        )) => Err(Fault::Service(mdm_engine::ServiceFault::ServiceDown)),
        Err(Fault::Service(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Step 3, and every refresh: an access token for the user-bound client. `code` is the device
/// code the first time, then the refresh token.
pub(crate) async fn token(
    http: &Http,
    client_id: &str,
    client_secret: &str,
    code: &str,
) -> Result<Token, Fault> {
    let url = http.services.rd.oauth.join("token").expect("relative");
    let form = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("grant_type", DEVICE_GRANT),
    ];
    http.json(http.post_form(&url, &form), fault).await
}

/// `POST torrents/addMagnet` (201): the torrent's id.
pub(crate) async fn add_magnet(http: &Http, token: &str, magnet: &str) -> Result<String, Fault> {
    #[derive(Deserialize)]
    struct Added {
        id: String,
    }
    let url = http
        .services
        .rd
        .rest
        .join("torrents/addMagnet")
        .expect("relative");
    let a: Added = http
        .json(
            http.post_form(&url, &[("magnet", magnet)])
                .bearer_auth(token),
            fault,
        )
        .await?;
    Ok(a.id)
}

/// `GET torrents/info/{id}`; a torrent waiting for its files to be chosen gets the ones ticked in
/// Add torrent, `chosen` (`POST torrents/selectFiles/{id}`, `files` their ids comma-separated;
/// None: `all`). A ticked file it doesn't list: nothing is selected.
pub(crate) async fn torrent_status(
    http: &Http,
    token: &str,
    id: &str,
    chosen: Option<&[String]>,
) -> Result<crate::torrents::Status, Fault> {
    let id = crate::plain_id(id)?;
    let rest = &http.services.rd.rest;
    let url = rest.join(&format!("torrents/info/{id}")).expect("relative");
    let info: crate::torrents::RdInfo = http.json(http.get(&url).bearer_auth(token), fault).await?;
    if info.status == "waiting_files_selection" {
        let files = crate::torrents::rd_selection(&info, chosen)
            .map_err(|p| Fault::Service(mdm_engine::ServiceFault::ChosenFileMissing(p)))?;
        let url = rest
            .join(&format!("torrents/selectFiles/{id}"))
            .expect("relative");
        http.empty(
            http.post_form(&url, &[("files", files.as_str())])
                .bearer_auth(token),
            fault,
        )
        .await?;
        return Ok(crate::torrents::Status::Working {
            progress: Some(0.0),
            what: "files chosen".into(),
        });
    }
    Ok(crate::torrents::rd_status(info))
}

/// `DELETE torrents/delete/{id}` (204).
pub(crate) async fn torrent_delete(http: &Http, token: &str, id: &str) -> Result<(), Fault> {
    let id = crate::plain_id(id)?;
    let url = http
        .services
        .rd
        .rest
        .join(&format!("torrents/delete/{id}"))
        .expect("relative");
    http.empty(http.delete(&url).bearer_auth(token), fault)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_engine::ServiceFault as F;

    #[test]
    fn a_bare_503_is_asked_again_every_5_minutes_for_an_hour() {
        let (every, limit) = (Duration::from_secs(300), Duration::from_secs(3600));
        let min = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(after_bare_503(Duration::ZERO, every, limit), Some(min(5)));
        assert_eq!(after_bare_503(min(30), every, limit), Some(min(5)));
        // The last wait ends at the hour, not after it.
        assert_eq!(after_bare_503(min(58), every, limit), Some(min(2)));
        assert_eq!(after_bare_503(min(60), every, limit), None);
        assert_eq!(after_bare_503(min(61), every, limit), None);
        // Asked at 0, 5, ..., 55 and 60 minutes: 13 times, then the file is gone.
        let (mut at, mut asked) = (Duration::ZERO, 1);
        while let Some(wait) = after_bare_503(at, every, limit) {
            at += wait;
            asked += 1;
        }
        assert_eq!((asked, at), (13, limit));
        let real = Endpoints::default();
        assert_eq!((real.busy_every, real.busy_for), (every, limit));
    }

    #[test]
    fn documented_errors_mean_what_they_say() {
        let f = |status, body: &str| fault(status, body.as_bytes(), None);
        assert_eq!(
            f(401, r#"{"error":"bad_token","error_code":8}"#),
            Fault::Service(F::SignIn)
        );
        assert_eq!(
            f(403, r#"{"error":"x","error_code":20}"#),
            Fault::Service(F::NotPremium)
        );
        assert_eq!(
            f(503, r#"{"error":"hoster_unsupported","error_code":16}"#),
            Fault::Unsupported
        );
        assert_eq!(
            f(503, r#"{"error":"unavailable_file","error_code":24}"#),
            Fault::Service(F::FileUnavailable)
        );
        assert_eq!(
            f(503, r#"{"error":"hoster_unavailable","error_code":19}"#),
            Fault::Service(F::HostDown)
        );
        assert_eq!(
            f(403, r#"{"error":"traffic_exhausted","error_code":23}"#),
            Fault::Service(F::LimitReached)
        );
        assert_eq!(
            f(403, r#"{"error":"ip_not_allowed","error_code":22}"#),
            Fault::Service(F::IpRefused)
        );
        assert_eq!(
            fault(
                429,
                br#"{"error":"too_many_requests","error_code":34}"#,
                Some(30)
            ),
            Fault::Service(F::TooManyRequests(Some(std::time::Duration::from_secs(30))))
        );
        assert_eq!(f(500, "not json"), Fault::Service(F::ServiceDown));
        assert_eq!(
            f(503, "not json"),
            Fault::Service(F::ServiceDown),
            "a bare 503 is the service, not the file"
        );
        assert!(bare_503(503, b""));
        assert!(bare_503(503, b"<html>Service Unavailable</html>"));
        assert!(bare_503(503, br#"{"error":"busy"}"#));
        assert!(!bare_503(
            503,
            br#"{"error":"unavailable_file","error_code":24}"#
        ));
        assert!(!bare_503(500, b""));
        assert_eq!(
            f(400, r#"{"error":"something new"}"#),
            Fault::Service(F::Other("something new".into()))
        );
    }

    #[test]
    fn the_documented_unrestrict_answer_reads() {
        // The shape of the documentation's example.
        let body = r#"{"id":"ABC","filename":"x.rar","mimeType":"application/x-rar","filesize":1048576,
            "link":"https://host/file/x","host":"host.com","chunks":16,"crc":1,
            "download":"https://abc.download.real-debrid.com/d/ABC/x.rar","streamable":0}"#;
        let u: Unrestricted = serde_json::from_str(body).unwrap();
        assert_eq!(u.filename.as_deref(), Some("x.rar"));
        assert_eq!((u.filesize, u.chunks), (1048576, 16));
        assert!(u.download.unwrap().starts_with("https://abc.download"));
    }
}

//! Debrid services (#44): the user's own account at a service such as Real-Debrid turns a file
//! host's link into a direct link, which MDM's engine then downloads as any file (every
//! connection, resume). Only the services' official, documented APIs, with the user's own
//! account; MDM ships no host lists or link sources, and never shows or copies the direct links
//! (the services forbid sharing them).
//!
//! The engine knows nothing of accounts: the app asks [`Debrid::unlocker_for`] for each download
//! (`mdm_engine::Options::unlocker`). Secrets (tokens) live in the app's settings, sealed there;
//! here they are only in memory, and never written to a log.
//!
//! Phase 1 is Real-Debrid (`realdebrid`). AllDebrid and Premiumize come next on the same
//! interface (`ServiceKind`).

pub mod realdebrid;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use mdm_engine::{ServiceFault, Unlocked};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use url::Url;

/// The services MDM knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceKind {
    #[default]
    RealDebrid,
}

impl ServiceKind {
    /// The service's name, as it writes it (not translated).
    pub fn name(self) -> &'static str {
        match self {
            ServiceKind::RealDebrid => "Real-Debrid",
        }
    }

    /// The service's page where a user finds their API token.
    pub fn token_page(self) -> &'static str {
        match self {
            ServiceKind::RealDebrid => "https://real-debrid.com/apitoken",
        }
    }
}

/// How an account signs in: what the app keeps sealed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Credential {
    /// A token the user pasted (Real-Debrid's personal API token).
    Token { token: String },
    /// The device flow's user-bound client and its refresh token (Real-Debrid).
    Oauth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
}

/// Never print a secret, even in a debug dump.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credential::Token { .. } => f.write_str("Token(…)"),
            Credential::Oauth { .. } => f.write_str("Oauth(…)"),
        }
    }
}

/// An account as the app gives it: its id (stable), service and credential.
#[derive(Clone, Debug)]
pub struct Account {
    pub id: u64,
    pub service: ServiceKind,
    pub credential: Credential,
}

/// What the service says about the account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountInfo {
    pub user: String,
    pub premium: bool,
    /// Premium time left.
    pub premium_left: Option<Duration>,
}

/// Why a call failed: the service's word, or "not a host this service takes".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    Service(ServiceFault),
    /// The service doesn't support this file host: download the link directly.
    Unsupported,
}

impl From<ServiceFault> for Fault {
    fn from(f: ServiceFault) -> Self {
        Fault::Service(f)
    }
}

impl Fault {
    fn service(self) -> ServiceFault {
        match self {
            Fault::Service(f) => f,
            Fault::Unsupported => ServiceFault::Other("this file host isn't supported".into()),
        }
    }
}

/// Requests to the services: through the user's proxy, DNS and network adapter (`net`, the
/// engine's own client), at most `PER_MINUTE` a minute, and none for a while after the service
/// said "too many requests".
pub struct Http {
    client: reqwest::Client,
    pub(crate) rd: realdebrid::Endpoints,
    budget: Mutex<Budget>,
}

/// Real-Debrid allows 250 requests a minute (refused ones count, and abuse gets a block): MDM
/// keeps well under it.
const PER_MINUTE: usize = 120;

#[derive(Default)]
struct Budget {
    sent: std::collections::VecDeque<Instant>,
    paused_until: Option<Instant>,
}

impl Http {
    /// `net`: the downloads' options (proxy, DNS, adapter); `rd`: where Real-Debrid answers.
    pub fn new(net: &mdm_engine::Options, rd: realdebrid::Endpoints) -> Result<Self, String> {
        let client = mdm_engine::http_client(net, &rd.rest).map_err(|e| e.to_string())?;
        Ok(Http {
            client,
            rd,
            budget: Mutex::default(),
        })
    }

    /// Wait for the budget to allow one more request.
    async fn slot(&self) -> Result<(), Fault> {
        loop {
            let wait = {
                let mut b = self.budget.lock().unwrap();
                let now = Instant::now();
                if let Some(until) = b.paused_until.filter(|u| *u > now) {
                    // Asked to slow down: no request until then (refused ones count too).
                    return Err(Fault::Service(ServiceFault::TooManyRequests(Some(
                        until - now,
                    ))));
                }
                while b
                    .sent
                    .front()
                    .is_some_and(|t| now - *t >= Duration::from_secs(60))
                {
                    b.sent.pop_front();
                }
                if b.sent.len() < PER_MINUTE {
                    b.sent.push_back(now);
                    return Ok(());
                }
                Duration::from_secs(60) - (now - b.sent[0])
            };
            tokio::time::sleep(wait).await;
        }
    }

    fn get(&self, url: &Url) -> reqwest::RequestBuilder {
        self.client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
    }

    fn post_form(&self, url: &Url, form: &[(&str, &str)]) -> reqwest::RequestBuilder {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        self.client
            .post(url.clone())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
    }

    /// Send `req` and read its JSON answer; an error answer through `fault`.
    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
        fault: fn(u16, &[u8], Option<u64>) -> Fault,
    ) -> Result<T, Fault> {
        self.slot().await?;
        let resp = req
            .send()
            .await
            .map_err(|_| Fault::Service(ServiceFault::ServiceDown))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let body = resp
            .bytes()
            .await
            .map_err(|_| Fault::Service(ServiceFault::ServiceDown))?;
        if (200..300).contains(&status) {
            return serde_json::from_slice(&body).map_err(|_| {
                Fault::Service(ServiceFault::Other("an answer MDM can't read".into()))
            });
        }
        let f = fault(status, &body, retry_after);
        if let Fault::Service(ServiceFault::TooManyRequests(d)) = &f {
            self.budget.lock().unwrap().paused_until =
                Some(Instant::now() + d.unwrap_or(Duration::from_secs(60)));
        }
        Err(f)
    }
}

const USER_AGENT: &str = concat!("MisterDownloadManager/", env!("CARGO_PKG_VERSION"));

/// One account, signed in: its access token in memory (Real-Debrid's device flow gives tokens
/// that expire; a pasted token doesn't).
struct Session {
    account: Account,
    http: Arc<Http>,
    access: tokio::sync::Mutex<Option<(String, Instant)>>,
}

impl Session {
    /// A token to call with: the pasted one, or the device flow's access token, refreshed five
    /// minutes before it expires (or when `stale`, after a sign-in error).
    async fn token(&self, stale: bool) -> Result<String, Fault> {
        match &self.account.credential {
            Credential::Token { token } => Ok(token.clone()),
            Credential::Oauth {
                client_id,
                client_secret,
                refresh_token,
            } => {
                let mut access = self.access.lock().await;
                if let Some((t, until)) = access.as_ref()
                    && !stale
                    && *until > Instant::now()
                {
                    return Ok(t.clone());
                }
                let t =
                    realdebrid::token(&self.http, client_id, client_secret, refresh_token).await?;
                let until =
                    Instant::now() + Duration::from_secs(t.expires_in.saturating_sub(300).max(60));
                *access = Some((t.access_token.clone(), until));
                Ok(t.access_token)
            }
        }
    }

    /// `call` with a token; once more with a fresh one after a sign-in error.
    async fn with_token<T, F, Fut>(&self, call: F) -> Result<T, Fault>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, Fault>>,
    {
        match call(self.token(false).await?).await {
            Err(Fault::Service(ServiceFault::SignIn))
                if matches!(self.account.credential, Credential::Oauth { .. }) =>
            {
                call(self.token(true).await?).await
            }
            r => r,
        }
    }

    async fn unlock(&self, link: &Url) -> Result<Option<Unlocked>, Fault> {
        match self.account.service {
            ServiceKind::RealDebrid => {
                let u = match self
                    .with_token(
                        |t| async move { realdebrid::unrestrict(&self.http, &t, link).await },
                    )
                    .await
                {
                    Ok(u) => u,
                    Err(Fault::Unsupported) => return Ok(None),
                    Err(e) => return Err(e),
                };
                // A multi-link answer (a video site) has no single `download`: not a file.
                let Some(url) = u.download.as_deref().and_then(|d| Url::parse(d).ok()) else {
                    return Err(Fault::Service(ServiceFault::Other(
                        "the service gave no file for this link".into(),
                    )));
                };
                Ok(Some(Unlocked {
                    url,
                    file_name: u.filename.filter(|n| !n.trim().is_empty()),
                    size: (u.filesize > 0).then_some(u.filesize as u64),
                    max_connections: (u.chunks >= 1).then_some(u.chunks as u32),
                }))
            }
        }
    }
}

/// Does `host` belong to one of `domains` (the domain itself or a subdomain of it; `www.` and
/// case ignored)? `notexample.com` is not `example.com`'s.
pub fn host_matches(host: &str, domains: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    domains.iter().any(|d| {
        let d = d.trim().trim_end_matches('.').to_ascii_lowercase();
        let d = d.strip_prefix("www.").unwrap_or(&d);
        !d.is_empty() && (host == d || host.ends_with(&format!(".{d}")))
    })
}

/// The app's handle: the accounts in priority order, each with its service's supported domains.
#[derive(Clone, Default)]
pub struct Debrid {
    sessions: Arc<RwLock<Vec<Arc<Session>>>>,
    domains: Arc<RwLock<HashMap<u64, Vec<String>>>>,
}

impl std::fmt::Debug for Debrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Debrid")
            .field("accounts", &self.sessions.read().unwrap().len())
            .finish()
    }
}

impl Debrid {
    /// Use these accounts (in this order), through `http`. Their host lists are kept for the
    /// accounts still there; `refresh_hosts` reads them again.
    pub fn set_accounts(&self, accounts: Vec<Account>, http: Arc<Http>) {
        let mut sessions = self.sessions.write().unwrap();
        let kept: Vec<Arc<Session>> = accounts
            .into_iter()
            .map(|account| {
                // The same account (service and credential) keeps its access token.
                sessions
                    .iter()
                    .find(|s| {
                        s.account.id == account.id
                            && s.account.credential == account.credential
                            && Arc::ptr_eq(&s.http, &http)
                    })
                    .cloned()
                    .unwrap_or_else(|| {
                        Arc::new(Session {
                            account,
                            http: http.clone(),
                            access: tokio::sync::Mutex::new(None),
                        })
                    })
            })
            .collect();
        let ids: Vec<u64> = kept.iter().map(|s| s.account.id).collect();
        self.domains
            .write()
            .unwrap()
            .retain(|id, _| ids.contains(id));
        *sessions = kept;
    }

    /// Read each account's service's supported domains (no sign-in needed).
    pub async fn refresh_hosts(&self) -> Result<(), Fault> {
        let sessions = self.sessions.read().unwrap().clone();
        for s in sessions {
            let list = match s.account.service {
                ServiceKind::RealDebrid => realdebrid::domains(&s.http).await?,
            };
            self.domains.write().unwrap().insert(s.account.id, list);
        }
        Ok(())
    }

    /// Whether the host lists are there for every account.
    pub fn hosts_known(&self) -> bool {
        let d = self.domains.read().unwrap();
        self.sessions
            .read()
            .unwrap()
            .iter()
            .all(|s| d.contains_key(&s.account.id))
    }

    /// The account that would take `url` (the first whose service supports its host).
    pub fn account_for(&self, url: &Url) -> Option<(u64, ServiceKind)> {
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let host = url.host_str()?;
        let d = self.domains.read().unwrap();
        self.sessions.read().unwrap().iter().find_map(|s| {
            d.get(&s.account.id)
                .filter(|list| host_matches(host, list))
                .map(|_| (s.account.id, s.account.service))
        })
    }

    /// The unlocker for `url` (`mdm_engine::Options::unlocker`), when an account takes its host.
    pub fn unlocker_for(&self, url: &Url) -> Option<Arc<dyn mdm_engine::Unlocker>> {
        let (id, _) = self.account_for(url)?;
        let session = self
            .sessions
            .read()
            .unwrap()
            .iter()
            .find(|s| s.account.id == id)?
            .clone();
        Some(Arc::new(AccountUnlocker { session }))
    }

    /// Ask the service about account `id` (Options, after signing in).
    pub async fn account_info(&self, id: u64) -> Result<AccountInfo, Fault> {
        let s = self
            .sessions
            .read()
            .unwrap()
            .iter()
            .find(|s| s.account.id == id)
            .cloned()
            .ok_or(Fault::Service(ServiceFault::SignIn))?;
        account_info(&s).await
    }
}

async fn account_info(s: &Session) -> Result<AccountInfo, Fault> {
    match s.account.service {
        ServiceKind::RealDebrid => {
            let u = s
                .with_token(|t| async move { realdebrid::user(&s.http, &t).await })
                .await?;
            Ok(AccountInfo {
                premium: u.kind == "premium" && u.premium > 0,
                premium_left: (u.premium > 0).then(|| Duration::from_secs(u.premium as u64)),
                user: u.username,
            })
        }
    }
}

/// Check a credential before keeping it: the account it signs in to.
pub async fn check(
    http: &Http,
    service: ServiceKind,
    credential: Credential,
) -> Result<AccountInfo, Fault> {
    let s = Session {
        account: Account {
            id: 0,
            service,
            credential,
        },
        http: Arc::new(Http {
            client: http.client.clone(),
            rd: http.rd.clone(),
            budget: Mutex::default(),
        }),
        access: tokio::sync::Mutex::new(None),
    };
    account_info(&s).await
}

/// One account's unlocker for the engine.
#[derive(Clone)]
struct AccountUnlocker {
    session: Arc<Session>,
}

impl std::fmt::Debug for AccountUnlocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AccountUnlocker({})",
            self.session.account.service.name()
        )
    }
}

impl mdm_engine::Unlocker for AccountUnlocker {
    fn name(&self) -> &str {
        self.session.account.service.name()
    }

    fn unlock<'a>(
        &'a self,
        url: &'a Url,
        _note: &'a (dyn Fn(String) + Send + Sync),
        cancel: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<Option<Unlocked>, ServiceFault>> {
        Box::pin(async move {
            tokio::select! {
                r = self.session.unlock(url) => r.map_err(Fault::service),
                _ = cancel.cancelled() => Err(ServiceFault::Other("cancelled".into())),
            }
        })
    }
}

/// Sign-in by code (Real-Debrid's device flow): the code to show, then a poll until the user
/// allowed MDM at the service's page.
pub mod signin {
    use super::*;

    /// Step 1: the code and the page to type it at.
    pub async fn start(http: &Http) -> Result<realdebrid::DeviceCode, Fault> {
        realdebrid::device_code(http).await
    }

    /// Step 2, every `interval` seconds until `expires_in`: the credential once allowed.
    pub async fn poll(
        http: &Http,
        code: &realdebrid::DeviceCode,
    ) -> Result<Option<Credential>, Fault> {
        let Some(c) = realdebrid::device_credentials(http, &code.device_code).await? else {
            return Ok(None);
        };
        let t = realdebrid::token(http, &c.client_id, &c.client_secret, &code.device_code).await?;
        Ok(Some(Credential::Oauth {
            client_id: c.client_id,
            client_secret: c.client_secret,
            refresh_token: t.refresh_token,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_match_their_domains_and_subdomains_only() {
        let d = vec!["rapidgator.net".to_owned(), "WWW.1fichier.com".to_owned()];
        for yes in [
            "rapidgator.net",
            "www.rapidgator.net",
            "RG.rapidgator.NET",
            "1fichier.com",
            "a.1fichier.com",
        ] {
            assert!(host_matches(yes, &d), "{yes}");
        }
        for no in [
            "notrapidgator.net",
            "rapidgator.net.evil.com",
            "fichier.com",
            "",
        ] {
            assert!(!host_matches(no, &d), "{no}");
        }
    }

    #[test]
    fn a_credential_never_prints_its_secret() {
        let c = Credential::Oauth {
            client_id: "cid".into(),
            client_secret: "SECRET".into(),
            refresh_token: "REFRESH".into(),
        };
        let shown = format!(
            "{c:?} {:?}",
            Credential::Token {
                token: "TOKEN".into()
            }
        );
        assert!(
            !shown.contains("SECRET") && !shown.contains("REFRESH") && !shown.contains("TOKEN")
        );
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""kind":"oauth""#));
        assert_eq!(serde_json::from_str::<Credential>(&json).unwrap(), c);
    }
}

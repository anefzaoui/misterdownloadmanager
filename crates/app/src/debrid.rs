//! Debrid service accounts in the app (#44): where the services answer, the account handle the
//! downloads use (`App::debrid`), and Options → Accounts' sign-in (`SignIn`), which runs on a
//! thread of its own while the dialog shows its state.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdm_debrid::{AccountInfo, Credential, Fault, Http, ServiceKind, Services};

/// Where the services answer. Debug builds only: MDM_DEBRID_LAB=http://127.0.0.1:PORT/ points
/// at the lab server's fakes (labserver `debrid`), for tests by hand.
pub fn services() -> Services {
    #[cfg(debug_assertions)]
    if let Some(base) = std::env::var("MDM_DEBRID_LAB")
        .ok()
        .and_then(|b| url::Url::parse(&b).ok())
    {
        return Services::at(&base);
    }
    Services::default()
}

/// A client for the services, through MDM's own network settings (proxy, DNS, adapter).
pub fn http() -> Result<Arc<Http>, String> {
    Http::new(&crate::settings::own_net(), services()).map(Arc::new)
}

/// "premium, 143 days left", "free account": what Options shows for an account.
pub fn state_text(info: &AccountInfo) -> String {
    match (info.premium, info.premium_left) {
        (true, Some(left)) => crate::i18n::fmt(
            // i18n: Options → Accounts: a debrid account's state; {days} the days of premium left
            crate::i18n::t("premium, {days} days left"),
            &[("days", &(left.as_secs() / 86400).to_string())],
        ),
        // i18n: Options → Accounts: a debrid account with premium (no end date given)
        (true, None) => crate::i18n::t("premium").to_owned(),
        // i18n: Options → Accounts: a debrid account without premium (it can't unlock links)
        (false, _) => crate::i18n::t("free account: links need premium").to_owned(),
    }
}

/// Why a sign-in or a check failed, in the interface language.
pub fn fault_text(service: ServiceKind, f: &Fault) -> String {
    match f {
        Fault::Service(f) => crate::errors::service(service.name(), f),
        // i18n: Options → Accounts: the service doesn't take this file host (shouldn't happen when signing in)
        Fault::Unsupported => crate::i18n::t("not supported by this service").to_owned(),
    }
}

/// Signing in, as the dialog shows it.
#[derive(Clone, Debug, Default)]
pub enum SignInState {
    #[default]
    Idle,
    /// Asking the service for a code.
    Starting,
    /// The code to type at `page`; the poll runs until `until`.
    Waiting {
        code: String,
        page: String,
        until: Instant,
    },
    /// Checking the account.
    Checking,
    /// Signed in: the account's name and state, and the credential to keep (sealed).
    Done {
        user: String,
        state: String,
        credential: String,
    },
    Failed(String),
}

/// A sign-in running on its own thread; the dialog reads `state` each frame.
#[derive(Clone, Default)]
pub struct SignIn {
    pub state: Arc<Mutex<SignInState>>,
    /// Set when the dialog closes: the poll stops.
    pub stop: Arc<std::sync::atomic::AtomicBool>,
}

impl SignIn {
    pub fn get(&self) -> SignInState {
        self.state.lock().unwrap().clone()
    }

    fn set(&self, s: SignInState, ctx: &egui::Context) {
        *self.state.lock().unwrap() = s;
        ctx.request_repaint();
    }

    /// Sign in by code (Real-Debrid's device flow, AllDebrid's PIN), in the browser.
    pub fn by_code(&self, service: ServiceKind, ctx: &egui::Context) {
        let (me, ctx) = (self.clone(), ctx.clone());
        me.set(SignInState::Starting, &ctx);
        std::thread::spawn(move || {
            let run = async {
                let http = http()?;
                let code = mdm_debrid::signin::start(&http, service)
                    .await
                    .map_err(|f| fault_text(service, &f))?;
                let until = Instant::now() + Duration::from_secs(code.expires_in);
                me.set(
                    SignInState::Waiting {
                        code: code.shown.clone(),
                        page: code.page.clone(),
                        until,
                    },
                    &ctx,
                );
                // The page opens in the browser, except in off-screen test runs and against the
                // lab's fake service (whose page is the real one's address).
                if !crate::offscreen() && std::env::var_os("MDM_DEBRID_LAB").is_none() {
                    let _ = open::that_detached(&code.page);
                }
                loop {
                    if me.stop.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err(String::new());
                    }
                    if Instant::now() >= until {
                        // i18n: Options → Accounts: the sign-in code ran out before it was used
                        return Err(crate::i18n::t("the code expired: sign in again").to_owned());
                    }
                    tokio::time::sleep(Duration::from_secs(code.interval.max(1))).await;
                    if let Some(c) = mdm_debrid::signin::poll(&http, &code)
                        .await
                        .map_err(|f| fault_text(service, &f))?
                    {
                        return Ok((http, c));
                    }
                }
            };
            me.finish(service, run, &ctx);
        });
    }

    /// Sign in with a token the user pasted (the service's API token page).
    pub fn by_token(&self, service: ServiceKind, token: String, ctx: &egui::Context) {
        let (me, ctx) = (self.clone(), ctx.clone());
        me.set(SignInState::Checking, &ctx);
        std::thread::spawn(move || {
            let run = async {
                Ok((
                    http()?,
                    Credential::Token {
                        token: token.trim().to_owned(),
                    },
                ))
            };
            me.finish(service, run, &ctx);
        });
    }

    /// Run `run` (a credential), check the account it signs in to, and say how it went.
    fn finish(
        &self,
        service: ServiceKind,
        run: impl std::future::Future<Output = Result<(Arc<Http>, Credential), String>>,
        ctx: &egui::Context,
    ) {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => return self.set(SignInState::Failed(e.to_string()), ctx),
        };
        let out = rt.block_on(async {
            let (http, credential) = run.await?;
            self.set(SignInState::Checking, ctx);
            let info = mdm_debrid::check(&http, service, credential.clone())
                .await
                .map_err(|f| fault_text(service, &f))?;
            let credential = serde_json::to_string(&credential).map_err(|e| e.to_string())?;
            Ok::<_, String>(SignInState::Done {
                user: info.user.clone(),
                state: state_text(&info),
                credential,
            })
        });
        match out {
            Ok(done) => self.set(done, ctx),
            // Closed meanwhile: nothing to say.
            Err(why) if why.is_empty() => self.set(SignInState::Idle, ctx),
            Err(why) => self.set(SignInState::Failed(why), ctx),
        }
    }
}

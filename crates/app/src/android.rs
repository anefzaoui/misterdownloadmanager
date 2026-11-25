//! Android's platform calls, which `unix.rs` re-exports under the names the rest of the app calls
//! on Linux and macOS (as `win32.rs` holds Windows'). What only Java can do there (the keystore,
//! exact alarms, wake locks) the Android app does for the core through a [`Host`], given once at
//! its start ([`set_host`]).
//!
//! Built on this system too with the `android-core` feature, so its tests run here:
//! `cargo test -p mdm-app --features android-core --lib android`.

use std::sync::{Arc, OnceLock};

/// What only the Android side can do, called from the core's threads.
pub trait Host: Send + Sync + 'static {
    /// Sign-ins (debrid accounts, site logins, the proxy's password) in the Android keystore:
    /// `op` is `probe`, `read`, `write`, `delete` or `list`. `Ok(None)` for "not there"; a
    /// list's accounts come one per line.
    fn keychain(
        &self,
        op: &str,
        service: &str,
        account: &str,
        secret: Option<&str>,
    ) -> Result<Option<String>, String>;
    /// Start the app at `at` (seconds since 1970, UTC) for the Scheduler, or cancel (None):
    /// an exact alarm.
    fn wake_at(&self, at: Option<i64>) -> Result<(), String>;
    /// Hold the CPU and Wi-Fi awake while downloads run (`Some(why)`), or let go (None).
    fn keep_awake(&self, reason: Option<&str>) -> bool;
}

static HOST: OnceLock<Arc<dyn Host>> = OnceLock::new();

/// The Android app's [`Host`], given once before the core starts; false when one was given
/// already (the first stays).
pub fn set_host(host: Arc<dyn Host>) -> bool {
    HOST.set(host).is_ok()
}

fn host() -> Option<&'static Arc<dyn Host>> {
    HOST.get()
}

/// The default browser, for the User-Agent of links added by hand: none known here.
pub fn default_browser() -> Option<mdm_engine::agent::Found> {
    None
}

/// A file type's icon from the system: none here (MDM's own are drawn).
pub fn file_icon(_what: &crate::fileicons::What, _px: u32) -> Option<crate::fileicons::Native> {
    None
}

/// The system's picture of a file, for the preview pane: none asked of Android yet (the pane
/// shows the type's icon).
pub fn thumbnail(_path: &std::path::Path, _px: u32) -> Option<crate::preview::Picture> {
    None
}

/// The Scheduler's start time, as an exact alarm (the host's).
pub fn wake_at(at: Option<i64>) -> Result<(), String> {
    match host() {
        Some(h) => h.wake_at(at),
        None => Err("no Android host".into()),
    }
}

/// Whether the Scheduler can start the app at a time: with the host's exact alarm, once there
/// is a host.
pub fn wake_support() -> crate::wake::Support {
    if host().is_some() {
        crate::wake::Support::Ready
    } else {
        crate::wake::Support::NoAlarm
    }
}

/// A wake lock and a Wi-Fi lock while downloads run (the host's).
pub fn keep_awake(reason: Option<&str>) -> bool {
    host().is_some_and(|h| h.keep_awake(reason))
}

fn keychain(
    op: &str,
    service: &str,
    account: &str,
    secret: Option<&str>,
) -> Result<Option<String>, crate::secrets::Fault> {
    let Some(h) = host() else {
        return Err(crate::secrets::Fault::Missing);
    };
    h.keychain(op, service, account, secret)
        .map_err(crate::secrets::Fault::Failed)
}

pub fn keychain_probe(service: &str) -> Result<(), crate::secrets::Fault> {
    keychain("probe", service, "", None).map(|_| ())
}

pub fn keychain_read(
    service: &str,
    account: &str,
) -> Result<Option<String>, crate::secrets::Fault> {
    keychain("read", service, account, None)
}

pub fn keychain_write(
    service: &str,
    account: &str,
    _label: &str,
    secret: &str,
) -> Result<(), crate::secrets::Fault> {
    keychain("write", service, account, Some(secret)).map(|_| ())
}

pub fn keychain_delete(service: &str, account: &str) -> Result<(), crate::secrets::Fault> {
    keychain("delete", service, account, None).map(|_| ())
}

pub fn keychain_list(service: &str) -> Result<Vec<String>, crate::secrets::Fault> {
    keychain("list", service, "", None).map(|l| {
        l.unwrap_or_default()
            .lines()
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Fault;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// The Android app's side, as a test stands in for it: a keystore in memory, the alarm and
    /// the wake lock remembered.
    #[derive(Default)]
    struct Fake {
        keys: Mutex<BTreeMap<(String, String), String>>,
        alarm: Mutex<Option<i64>>,
        awake: Mutex<Option<String>>,
    }

    impl Host for Fake {
        fn keychain(
            &self,
            op: &str,
            service: &str,
            account: &str,
            secret: Option<&str>,
        ) -> Result<Option<String>, String> {
            let mut keys = self.keys.lock().unwrap();
            let key = (service.to_owned(), account.to_owned());
            match op {
                "probe" => Ok(None),
                "read" => Ok(keys.get(&key).cloned()),
                "write" => {
                    keys.insert(key, secret.ok_or("no secret")?.to_owned());
                    Ok(None)
                }
                "delete" => Ok(keys.remove(&key).map(|_| String::new())),
                "list" => Ok(Some(
                    keys.keys()
                        .filter(|(s, _)| s == service)
                        .map(|(_, a)| format!("{a}\n"))
                        .collect(),
                )),
                _ => Err(format!("unknown op {op}")),
            }
        }
        fn wake_at(&self, at: Option<i64>) -> Result<(), String> {
            *self.alarm.lock().unwrap() = at;
            Ok(())
        }
        fn keep_awake(&self, reason: Option<&str>) -> bool {
            *self.awake.lock().unwrap() = reason.map(str::to_owned);
            true
        }
    }

    /// One test for the whole module: the host is given once per process.
    #[test]
    fn calls_reach_the_host() {
        assert!(matches!(keychain_probe("mdm"), Err(Fault::Missing)));
        assert!(wake_at(Some(1)).is_err());
        assert_eq!(wake_support(), crate::wake::Support::NoAlarm);
        assert!(!keep_awake(Some("downloads")));

        let fake = Arc::new(Fake::default());
        assert!(set_host(fake.clone()));
        assert!(!set_host(Arc::new(Fake::default())), "the first host stays");

        assert!(keychain_probe("mdm").is_ok());
        assert_eq!(keychain_read("mdm", "debrid").unwrap(), None);
        keychain_write("mdm", "debrid", "label", "token-1").unwrap();
        keychain_write("mdm", "proxy", "label", "secret").unwrap();
        keychain_write("other", "x", "label", "y").unwrap();
        assert_eq!(
            keychain_read("mdm", "debrid").unwrap().as_deref(),
            Some("token-1")
        );
        assert_eq!(keychain_list("mdm").unwrap(), ["debrid", "proxy"]);
        keychain_delete("mdm", "debrid").unwrap();
        assert_eq!(keychain_list("mdm").unwrap(), ["proxy"]);

        assert_eq!(wake_support(), crate::wake::Support::Ready);
        wake_at(Some(1_800_000_000)).unwrap();
        assert_eq!(*fake.alarm.lock().unwrap(), Some(1_800_000_000));
        wake_at(None).unwrap();
        assert_eq!(*fake.alarm.lock().unwrap(), None);
        assert!(keep_awake(Some("downloads")));
        assert_eq!(fake.awake.lock().unwrap().as_deref(), Some("downloads"));
        assert!(keep_awake(None));
        assert_eq!(*fake.awake.lock().unwrap(), None);

        assert!(default_browser().is_none());
    }
}

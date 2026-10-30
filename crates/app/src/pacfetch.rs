//! The proxy auto-config script of Options → Proxy / Socks ("Use automatic configuration
//! script"): fetched from its address (http, https, file or a path) when the settings are
//! loaded or changed, and kept for the engine to run (mdm_engine::pac). Until it has arrived,
//! or when it can't be had, downloads use the system's proxy settings.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

static SCRIPTS: Mutex<Option<HashMap<String, Arc<str>>>> = Mutex::new(None);

/// The script fetched from `address`, if it has arrived.
pub fn script(address: &str) -> Option<Arc<str>> {
    SCRIPTS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(address.trim()).cloned())
}

/// Fetch the script at `address` (again), in the background.
pub fn refresh(address: &str, rt: &tokio::runtime::Handle) {
    let address = address.trim().to_owned();
    if address.is_empty() {
        return;
    }
    rt.spawn(async move {
        if let Ok(text) = fetch(&address).await {
            SCRIPTS
                .lock()
                .unwrap()
                .get_or_insert_with(HashMap::new)
                .insert(address, Arc::from(text.as_str()));
        }
    });
}

async fn fetch(address: &str) -> Result<String, String> {
    if let Ok(url) = url::Url::parse(address)
        && matches!(url.scheme(), "http" | "https")
    {
        // Directly: the script says which proxy to use, it isn't fetched through one.
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        let r = client.get(url).send().await.map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("HTTP {}", r.status()));
        }
        return r.text().await.map_err(|e| e.to_string());
    }
    let path = url::Url::parse(address)
        .ok()
        .filter(|u| u.scheme() == "file")
        .and_then(|u| u.to_file_path().ok())
        .unwrap_or_else(|| std::path::PathBuf::from(address));
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

//! The browser extension's identity and whether a browser has it, shared by the native messaging
//! host (which serves it and notes each browser that starts it) and the app (which offers to
//! install it until one has).

/// The extension's ID loaded unpacked (the public key in extension/manifest.json).
pub const UNPACKED_ID: &str = "oogbclbbninliongijffeeggalppliok";
/// Its ID in the Chrome Web Store, which signs it with the store's own key
/// (tools/extension/pack.ps1 leaves the manifest's key out of the store's copy).
pub const STORE_ID: &str = "STORE_ID_PENDING";
/// Both: the native host serves either.
pub const CHROMIUM_IDS: [&str; 2] = [UNPACKED_ID, STORE_ID];
/// The Firefox add-on's ID (extension/manifest.firefox.json).
pub const FIREFOX_ID: &str = "integration@misterdownloadmanager.com";
/// Chrome's update service: an external-install entry naming it makes Chrome offer the store's
/// extension at its next start (developer.chrome.com, "Alternative extension distribution
/// options").
pub const STORE_UPDATE_URL: &str = "https://clients2.google.com/service/update2/crx";

/// The extension's page in the Chrome Web Store; Chrome, Edge, Brave, Opera and Vivaldi all
/// install from it.
pub fn store_page() -> String {
    format!("https://chromewebstore.google.com/detail/{STORE_ID}")
}

/// The tutorial that adds the extension by hand (Firefox, until it has a listing of its own).
pub const GUIDE_PAGE: &str =
    "https://misterdownloadmanager.com/help/guide-catch-browser-downloads.html";

/// Marks the data folder once a browser has started the host for the extension.
const SEEN: &str = "extension-seen";

/// The host, started by a browser for the extension: the extension is installed there. The
/// file holds the last browser's name (`capture::browser_of`), for the log.
pub fn mark_seen(browser: &str) {
    if let Some(dir) = crate::data_dir() {
        let file = dir.join(SEEN);
        let known = std::fs::read_to_string(&file).ok();
        let new_name = !browser.is_empty() && known.as_deref() != Some(browser);
        if known.is_none() || new_name {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(file, browser);
        }
    }
}

/// Has any browser's extension reached the app?
pub fn seen() -> bool {
    crate::data_dir().is_some_and(|d| d.join(SEEN).exists())
}

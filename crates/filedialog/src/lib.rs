//! The system's file and folder dialogs, which `mdm-app` uses under the name `rfd` (its
//! Cargo.toml): [rfd](https://crates.io/crates/rfd) itself on Windows, Linux and macOS. On
//! Android a stand-in with the methods MDM calls: the desktop dialogs that call it never open
//! there (the Android interface, android/, picks folders and files with the system's Storage
//! Access Framework and hands the app paths), and every pick answers "cancelled".

#[cfg(not(target_os = "android"))]
pub use rfd::*;

#[cfg(target_os = "android")]
pub use android::FileDialog;

#[cfg(target_os = "android")]
mod android {
    use std::path::{Path, PathBuf};

    /// rfd's `FileDialog`, with the methods MDM calls; every pick is cancelled.
    #[derive(Clone, Debug, Default)]
    pub struct FileDialog;

    impl FileDialog {
        pub fn new() -> Self {
            FileDialog
        }
        pub fn set_title(self, _title: impl Into<String>) -> Self {
            self
        }
        pub fn set_directory<P: AsRef<Path>>(self, _path: P) -> Self {
            self
        }
        pub fn set_file_name(self, _name: impl Into<String>) -> Self {
            self
        }
        pub fn add_filter(self, _name: impl Into<String>, _extensions: &[impl ToString]) -> Self {
            self
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            None
        }
        pub fn pick_folder(self) -> Option<PathBuf> {
            None
        }
        pub fn save_file(self) -> Option<PathBuf> {
            None
        }
    }
}

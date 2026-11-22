//! The system's file and folder dialogs, which `mdm-app` uses under the name `rfd` (its
//! Cargo.toml): [rfd](https://crates.io/crates/rfd) itself on Windows, Linux and macOS. rfd has
//! no Android dialogs, so there a stand-in with the methods MDM calls answers every pick as
//! cancelled, until the Android app gives them the system's own pickers (the Storage Access
//! Framework).

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

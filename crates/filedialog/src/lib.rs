//! The system's file and folder dialogs, which `mdm-app` uses under the name `rfd` (its
//! Cargo.toml): [rfd](https://crates.io/crates/rfd) itself on Windows, Linux and macOS. rfd has
//! no Android dialogs, so there a stand-in with the methods MDM calls asks the picker the Android
//! app gives ([`set_picker`]: the Storage Access Framework's; a file to save goes in a folder
//! picked there); without one, every pick is cancelled.

#[cfg(not(target_os = "android"))]
pub use rfd::*;

#[cfg(target_os = "android")]
pub use android::{FileDialog, Pick, set_picker};

#[cfg(target_os = "android")]
mod android {
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    /// What is picked.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Pick {
        /// A folder (a download's or a category's): its path on the phone's storage.
        Folder,
        /// A file to open (a `.torrent`, a list of links): a copy MDM can read.
        File,
    }

    /// The Android app's picker: what is picked, and the folder to start in; waits for the
    /// person's choice (None: cancelled). Called on a thread of its own, never the window's.
    pub type Picker = Box<dyn Fn(Pick, Option<&Path>) -> Option<PathBuf> + Send + Sync>;

    static PICKER: OnceLock<Picker> = OnceLock::new();

    /// The picker every [`FileDialog`] asks, given once by the Android app; false when one was
    /// given already.
    pub fn set_picker(picker: Picker) -> bool {
        PICKER.set(picker).is_ok()
    }

    fn pick(kind: Pick, dir: Option<&Path>) -> Option<PathBuf> {
        PICKER.get()?(kind, dir)
    }

    /// rfd's `FileDialog`, with the methods MDM calls, asking the Android app's picker.
    #[derive(Clone, Debug, Default)]
    pub struct FileDialog {
        dir: Option<PathBuf>,
        name: Option<String>,
    }

    impl FileDialog {
        pub fn new() -> Self {
            FileDialog::default()
        }
        pub fn set_title(self, _title: impl Into<String>) -> Self {
            self
        }
        pub fn set_directory<P: AsRef<Path>>(mut self, path: P) -> Self {
            self.dir = Some(path.as_ref().to_owned());
            self
        }
        pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
            self.name = Some(name.into());
            self
        }
        /// Android's picker shows every file: the filters aren't its.
        pub fn add_filter(self, _name: impl Into<String>, _extensions: &[impl ToString]) -> Self {
            self
        }
        pub fn pick_file(self) -> Option<PathBuf> {
            pick(Pick::File, self.dir.as_deref())
        }
        pub fn pick_folder(self) -> Option<PathBuf> {
            pick(Pick::Folder, self.dir.as_deref())
        }
        /// Where to save a file: the folder picked, with the name given (Android's own
        /// "create a document" gives a document, not a path MDM could write to). Without a
        /// name, cancelled.
        pub fn save_file(self) -> Option<PathBuf> {
            let name = self.name.filter(|n| !n.is_empty())?;
            pick(Pick::Folder, self.dir.as_deref()).map(|d| d.join(name))
        }
    }
}

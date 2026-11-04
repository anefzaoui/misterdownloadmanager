//! The browsers the extension runs in, and where each reads native messaging host manifests:
//! one table for the native host (which writes the manifests), and the app (which lists the
//! browsers and checks the manifests are there), #41.
//!
//! Sources: each browser's own code or packaging (Chromium's `launch_context_win.cc`, Firefox's
//! `nsXREDirProvider.cpp`, Helium's `scan-chrome-native-messaging-hosts.patch` and branding
//! patches, Zen's issue #10622); folders marked "unverified" in issue #41 come from other
//! projects' installers and cost nothing when absent.

use std::path::{Path, PathBuf};

/// Which manifest a browser reads: Chromium's (`allowed_origins`) or Firefox's
/// (`allowed_extensions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Chromium,
    Gecko,
}

/// A browser MDM knows by name.
#[derive(Debug)]
pub struct Browser {
    /// Its name in the app (the offer window, Options).
    pub name: &'static str,
    pub family: Family,
    /// macOS: its app bundles, in /Applications or ~/Applications.
    pub mac_apps: &'static [&'static str],
    /// Linux: its commands on the PATH.
    pub linux_cmds: &'static [&'static str],
    /// macOS: its data folders under ~/Library/Application Support. A Chromium browser reads
    /// `<folder>/NativeMessagingHosts`; for a Gecko one the folder only says it is there.
    pub mac_data: &'static [&'static str],
    /// Linux: the same, under $XDG_CONFIG_HOME (~/.config).
    pub linux_config: &'static [&'static str],
    /// Linux: Gecko data folders under $HOME (`.zen`).
    pub linux_home: &'static [&'static str],
}

const fn chromium(
    name: &'static str,
    mac_apps: &'static [&'static str],
    linux_cmds: &'static [&'static str],
    mac_data: &'static [&'static str],
    linux_config: &'static [&'static str],
) -> Browser {
    Browser {
        name,
        family: Family::Chromium,
        mac_apps,
        linux_cmds,
        mac_data,
        linux_config,
        linux_home: &[],
    }
}

const fn gecko(
    name: &'static str,
    mac_apps: &'static [&'static str],
    linux_cmds: &'static [&'static str],
    mac_data: &'static [&'static str],
    linux_config: &'static [&'static str],
    linux_home: &'static [&'static str],
) -> Browser {
    Browser {
        name,
        family: Family::Gecko,
        mac_apps,
        linux_cmds,
        mac_data,
        linux_config,
        linux_home,
    }
}

/// Every browser MDM names, Chromium's first.
pub const BROWSERS: &[Browser] = &[
    chromium(
        "Google Chrome",
        &["Google Chrome.app"],
        &["google-chrome", "google-chrome-stable"],
        &["Google/Chrome"],
        &[
            "google-chrome",
            "google-chrome-beta",
            "google-chrome-unstable",
        ],
    ),
    chromium(
        "Chromium",
        &["Chromium.app"],
        &["chromium", "chromium-browser"],
        &["Chromium"],
        &["chromium"],
    ),
    chromium(
        "Microsoft Edge",
        &["Microsoft Edge.app"],
        &["microsoft-edge", "microsoft-edge-stable"],
        &["Microsoft Edge"],
        &["microsoft-edge"],
    ),
    chromium(
        "Brave",
        &[
            "Brave Browser.app",
            "Brave Browser Beta.app",
            "Brave Browser Nightly.app",
        ],
        &["brave-browser", "brave"],
        &[
            "BraveSoftware/Brave-Browser",
            "BraveSoftware/Brave-Browser-Beta",
            "BraveSoftware/Brave-Browser-Nightly",
        ],
        &[
            "BraveSoftware/Brave-Browser",
            "BraveSoftware/Brave-Browser-Beta",
            "BraveSoftware/Brave-Browser-Nightly",
        ],
    ),
    chromium(
        "Vivaldi",
        &["Vivaldi.app"],
        &["vivaldi", "vivaldi-stable"],
        &["Vivaldi"],
        &["vivaldi"],
    ),
    // Opera's folders: unverified.
    chromium(
        "Opera",
        &["Opera.app"],
        &["opera"],
        &["com.operasoftware.Opera"],
        &["opera"],
    ),
    // Helium reads its own folder, then Chrome's (on Linux and macOS); `.dev` is an unofficial
    // build's.
    chromium(
        "Helium",
        &["Helium.app"],
        &["helium", "helium-browser"],
        &["net.imput.helium"],
        &["net.imput.helium", "net.imput.helium.dev"],
    ),
    // Thorium's and Arc's folders: unverified.
    chromium(
        "Thorium",
        &["Thorium.app"],
        &["thorium-browser"],
        &["Thorium"],
        &["thorium"],
    ),
    chromium("Arc", &["Arc.app"], &[], &["Arc/User Data"], &[]),
    gecko(
        "Mozilla Firefox",
        &["Firefox.app"],
        &["firefox"],
        &["Mozilla", "Firefox"],
        &["mozilla"],
        &[".mozilla"],
    ),
    gecko(
        "Zen Browser",
        &["Zen.app", "Zen Browser.app"],
        &["zen", "zen-browser", "zen-bin"],
        &["Zen"],
        &["zen"],
        &[".zen"],
    ),
    gecko(
        "LibreWolf",
        &["LibreWolf.app"],
        &["librewolf"],
        &["LibreWolf"],
        &[],
        &[".librewolf"],
    ),
    // Floorp's and Waterfox's: unverified (no patch found; they read Firefox's folder).
    gecko(
        "Floorp",
        &["Floorp.app"],
        &["floorp"],
        &["Floorp"],
        &[],
        &[".floorp"],
    ),
    gecko(
        "Waterfox",
        &["Waterfox.app"],
        &["waterfox"],
        &["Waterfox"],
        &[],
        &[".waterfox"],
    ),
];

/// The browser named `name` in the table.
pub fn named(name: &str) -> Option<&'static Browser> {
    BROWSERS.iter().find(|b| b.name == name)
}

/// Linux and macOS (`mac`): the folders to write the host manifest into, each with the family
/// whose manifest goes there, for this user (`home`, `config` = $XDG_CONFIG_HOME) or for
/// everyone. A user's folder is given only for a browser that is there.
///
/// Firefox and its forks (Zen, LibreWolf, Floorp, Waterfox) all read Mozilla's own user folder,
/// `~/.mozilla/native-messaging-hosts` or `~/Library/Application Support/Mozilla/
/// NativeMessagingHosts`, even when they keep their data elsewhere (Firefox's newer
/// `~/.config/mozilla`, Zen's `~/.zen`): it is given, to be created, when any of them is there.
pub fn host_dirs(home: &Path, config: &Path, mac: bool, all_users: bool) -> Vec<(PathBuf, Family)> {
    let mut dirs: Vec<(PathBuf, Family)> = Vec::new();
    if all_users {
        let (chromium, gecko): (&[&str], &[&str]) = if mac {
            (
                &[
                    "/Library/Google/Chrome/NativeMessagingHosts",
                    "/Library/Application Support/Chromium/NativeMessagingHosts",
                    "/Library/Microsoft/Edge/NativeMessagingHosts",
                ],
                &["/Library/Application Support/Mozilla/NativeMessagingHosts"],
            )
        } else {
            (
                &[
                    "/etc/opt/chrome/native-messaging-hosts",
                    "/etc/chromium/native-messaging-hosts",
                    "/etc/opt/edge/native-messaging-hosts",
                ],
                // Firefox reads lib64's on 64-bit Fedora and openSUSE (MDN, "Native manifests").
                &[
                    "/usr/lib/mozilla/native-messaging-hosts",
                    "/usr/lib64/mozilla/native-messaging-hosts",
                ],
            )
        };
        dirs.extend(
            chromium
                .iter()
                .map(|d| (PathBuf::from(d), Family::Chromium)),
        );
        dirs.extend(gecko.iter().map(|d| (PathBuf::from(d), Family::Gecko)));
        // Everyone's: only where the browser's folder is.
        dirs.retain(|(d, _)| d.parent().is_some_and(Path::is_dir));
        return dirs;
    }
    let support = home.join("Library/Application Support");
    let data = |b: &Browser| -> Vec<PathBuf> {
        if mac {
            b.mac_data.iter().map(|d| support.join(d)).collect()
        } else {
            b.linux_config
                .iter()
                .map(|d| config.join(d))
                .chain(b.linux_home.iter().map(|d| home.join(d)))
                .collect()
        }
    };
    let mut push = |d: PathBuf, f: Family| {
        if !dirs.iter().any(|(x, _)| *x == d) {
            dirs.push((d, f));
        }
    };
    let mut gecko_there = false;
    for b in BROWSERS {
        let there: Vec<PathBuf> = data(b).into_iter().filter(|d| d.is_dir()).collect();
        match b.family {
            Family::Chromium => {
                for d in there {
                    push(d.join("NativeMessagingHosts"), Family::Chromium);
                }
            }
            Family::Gecko => gecko_there |= !there.is_empty(),
        }
    }
    if gecko_there {
        if mac {
            push(support.join("Mozilla/NativeMessagingHosts"), Family::Gecko);
            // LibreWolf reads its own first.
            if support.join("LibreWolf").is_dir() {
                push(
                    support.join("LibreWolf/NativeMessagingHosts"),
                    Family::Gecko,
                );
            }
        } else {
            push(home.join(".mozilla/native-messaging-hosts"), Family::Gecko);
            if home.join(".librewolf").is_dir() {
                push(
                    home.join(".librewolf/native-messaging-hosts"),
                    Family::Gecko,
                );
            }
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(home: &Path, mac: bool) -> Vec<(PathBuf, Family)> {
        host_dirs(home, &home.join(".config"), mac, false)
    }

    #[test]
    fn nothing_installed_nothing_written() {
        let h = tempfile::tempdir().unwrap();
        assert!(user(h.path(), false).is_empty());
        assert!(user(h.path(), true).is_empty());
    }

    /// #41: a Zen-only user has no ~/.mozilla; Zen reads it all the same.
    #[test]
    fn zen_alone_gets_mozillas_folder() {
        let h = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(h.path().join(".zen")).unwrap();
        assert_eq!(
            user(h.path(), false),
            vec![(
                h.path().join(".mozilla/native-messaging-hosts"),
                Family::Gecko
            )]
        );
        let m = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(m.path().join("Library/Application Support/Zen")).unwrap();
        assert_eq!(
            user(m.path(), true),
            vec![(
                m.path()
                    .join("Library/Application Support/Mozilla/NativeMessagingHosts"),
                Family::Gecko
            )]
        );
    }

    /// Firefox with only the newer ~/.config/mozilla still reads ~/.mozilla.
    #[test]
    fn firefox_xdg_alone_gets_mozillas_folder() {
        let h = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(h.path().join(".config/mozilla/firefox")).unwrap();
        assert_eq!(
            user(h.path(), false),
            vec![(
                h.path().join(".mozilla/native-messaging-hosts"),
                Family::Gecko
            )]
        );
    }

    /// #41: Helium reads its own folder (then Chrome's, which a Helium-only user doesn't have).
    #[test]
    fn helium_gets_its_own_folder() {
        let h = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(h.path().join(".config/net.imput.helium")).unwrap();
        assert_eq!(
            user(h.path(), false),
            vec![(
                h.path()
                    .join(".config/net.imput.helium/NativeMessagingHosts"),
                Family::Chromium
            )]
        );
        let m = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(
            m.path()
                .join("Library/Application Support/net.imput.helium"),
        )
        .unwrap();
        assert_eq!(
            user(m.path(), true),
            vec![(
                m.path()
                    .join("Library/Application Support/net.imput.helium/NativeMessagingHosts"),
                Family::Chromium
            )]
        );
    }

    #[test]
    fn several_browsers_each_once() {
        let h = tempfile::tempdir().unwrap();
        for d in [".config/google-chrome", ".mozilla", ".zen", ".librewolf"] {
            std::fs::create_dir_all(h.path().join(d)).unwrap();
        }
        assert_eq!(
            user(h.path(), false),
            vec![
                (
                    h.path().join(".config/google-chrome/NativeMessagingHosts"),
                    Family::Chromium
                ),
                (
                    h.path().join(".mozilla/native-messaging-hosts"),
                    Family::Gecko
                ),
                (
                    h.path().join(".librewolf/native-messaging-hosts"),
                    Family::Gecko
                ),
            ]
        );
    }

    #[test]
    fn names_are_unique_and_found() {
        for b in BROWSERS {
            assert_eq!(named(b.name).map(|x| x.name), Some(b.name));
        }
        assert!(named("Netscape").is_none());
    }
}

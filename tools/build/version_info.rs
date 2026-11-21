// The version info every Windows program of MDM carries (Explorer's Details tab; code signing
// checks its product name and version, .signpath/artifact-configuration.xml), one copy for all:
// `include!`d by the build.rs of the app, the browser helper, the command line launcher and the
// MCP server. Its product and file version are the crate's (winresource takes
// CARGO_PKG_VERSION, Cargo.toml's workspace version), the installer's the same (mdm.iss).
//
// For a Windows program only (not one built on Windows for Linux or macOS); winresource builds
// on Windows only, so elsewhere this does nothing.

/// Embeds the version info, described as `description`, and `icon` when given (a path from the
/// crate's folder).
#[cfg(windows)]
fn version_info(description: &str, icon: Option<&str>) {
    println!("cargo:rerun-if-changed=../../tools/build/version_info.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    if let Some(icon) = icon {
        println!("cargo:rerun-if-changed={icon}");
        res.set_icon(icon);
    }
    res.set("ProductName", "Mister Download Manager")
        .set("FileDescription", description)
        .set("CompanyName", "Ahmed Nefzaoui")
        .set(
            "LegalCopyright",
            "Copyright (C) 2026 Ahmed Nefzaoui. GPL-3.0-or-later.",
        );
    if let Err(e) = res.compile() {
        // A missing resource compiler must not break a build; the app's window still sets its
        // icon at runtime. A release's signing refuses a program without it.
        println!("cargo:warning=version info not embedded: {e}");
    }
}

#[cfg(not(windows))]
fn version_info(_description: &str, _icon: Option<&str>) {
    println!("cargo:rerun-if-changed=../../tools/build/version_info.rs");
}

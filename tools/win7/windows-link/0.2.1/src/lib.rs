// Mister Download Manager's copy of windows-link 0.2.1 (microsoft/windows-rs, MIT OR Apache-2.0,
// license-mit and license-apache-2.0 here), patched in through the workspace's [patch.crates-io]
// for Windows 7's build (docs/windows-7.md). One change: built for Rust's win7 targets, a function
// windows-rs links from combase.dll (Windows 8's) is linked from ole32.dll, which exports the same
// COM functions on Windows 7 and forwards them to combase.dll from Windows 8 on. On every other
// target the macro is windows-link's own. tools/win7/imports.py fails the build if such a function
// is one Windows 7's ole32.dll lacks.

#![doc = include_str!("../readme.md")]
#![no_std]

/// Defines an external function to import.
#[cfg(all(windows, target_arch = "x86"))]
#[macro_export]
macro_rules! link {
    // Windows 7's build: combase.dll's functions from ole32.dll (see the top of the file).
    ("combase.dll" $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[cfg_attr(target_vendor = "win7", link(name = "ole32.dll", kind = "raw-dylib", modifiers = "+verbatim", import_name_type = "undecorated"))]
        #[cfg_attr(not(target_vendor = "win7"), link(name = "combase.dll", kind = "raw-dylib", modifiers = "+verbatim", import_name_type = "undecorated"))]
        extern $abi {
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }
    );
    ($library:literal $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[link(name = $library, kind = "raw-dylib", modifiers = "+verbatim", import_name_type = "undecorated")]
        extern $abi {
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }
    )
}

/// Defines an external function to import.
#[cfg(all(windows, not(target_arch = "x86")))]
#[macro_export]
macro_rules! link {
    // Windows 7's build: combase.dll's functions from ole32.dll (see the top of the file).
    ("combase.dll" $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[cfg_attr(target_vendor = "win7", link(name = "ole32.dll", kind = "raw-dylib", modifiers = "+verbatim"))]
        #[cfg_attr(not(target_vendor = "win7"), link(name = "combase.dll", kind = "raw-dylib", modifiers = "+verbatim"))]
        extern $abi {
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }
    );
    ($library:literal $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[link(name = $library, kind = "raw-dylib", modifiers = "+verbatim")]
        extern $abi {
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }
    )
}

/// Defines an external function to import.
#[cfg(not(windows))]
#[macro_export]
macro_rules! link {
    ($library:literal $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        extern $abi {
            pub fn $($function)*;
        }
    )
}

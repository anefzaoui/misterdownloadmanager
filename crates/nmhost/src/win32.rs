//! The native host's Windows calls: letting the app come to the front, reading its registry
//! entries.
#![allow(unsafe_code)]

/// Let process `pid` (the app) bring its windows to the front. Windows refuses that to a program
/// in the background (the foreground lock), so a window opened at the browser's request (Options
/// from the video button, Download File Info) would open behind it. The browser is in front and
/// started this host, which lets the host pass that right on. Nothing happens when it can't.
pub fn allow_foreground(pid: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    // SAFETY: a plain call with a process id; it fails harmlessly for a wrong or ended one.
    unsafe {
        AllowSetForegroundWindow(pid);
    }
}

/// The default value of the key at `place` (HKLM or HKCU, in its view: #1688), when it is a
/// string.
pub fn reg_string(place: &mdm_ipc::browsers::RegPlace) -> Option<String> {
    use mdm_ipc::browsers::RegView;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
        RRF_SUBKEY_WOW6464KEY, RegGetValueW,
    };
    let root = if place.machine {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    };
    // The computer's 32-bit view is WOW6432Node's (32-bit Windows has one view: ignored there).
    let view = match place.view {
        Some(RegView::Machine32) => RRF_SUBKEY_WOW6432KEY,
        Some(RegView::Machine64) => RRF_SUBKEY_WOW6464KEY,
        _ => 0,
    };
    let key: Vec<u16> = place.key.encode_utf16().chain(Some(0)).collect();
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: a registry read into `buf`, whose size in bytes is passed in `size`; a null value
    // name reads the key's default value.
    let rc = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ | view,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if rc != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

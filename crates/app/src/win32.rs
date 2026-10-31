//! The only Windows FFI in Mister Download Manager (D-008): IDM's dialog frame for our dialog windows, and the
//! shell's own small file-type icons (the ones Explorer and IDM show — Windows' artwork, not IDM's),
//! and the drop target's OLE `IDropTarget` (`ole`).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, ReleaseDC,
};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES,
    SHGetFileInfoW, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetIconInfo, GetWindowLongPtrW,
    GetWindowTextW, GetWindowThreadProcessId, ICONINFO, NCCALCSIZE_PARAMS, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WM_NCCALCSIZE,
    WM_NCHITTEST, WS_EX_DLGMODALFRAME, WS_THICKFRAME,
};

struct Find {
    pid: u32,
    title: Vec<u16>,
    found: HWND,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
    // SAFETY: lparam is the &mut Find passed to EnumWindows below, alive for the call.
    let f = unsafe { &mut *(lparam as *mut Find) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid != f.pid {
        return 1;
    }
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n > 0 && buf[..n as usize] == f.title[..] {
        f.found = hwnd;
        return 0;
    }
    1
}

/// This process's top-level window with exactly `title`.
pub(crate) fn find_own_window(title: &str) -> Option<HWND> {
    let mut f = Find {
        pid: unsafe { GetCurrentProcessId() },
        title: title.encode_utf16().collect(),
        found: std::ptr::null_mut(),
    };
    unsafe { EnumWindows(Some(enum_cb), &mut f as *mut Find as LPARAM) };
    (!f.found.is_null()).then_some(f.found)
}

/// Give our window titled `title` IDM's modal-dialog frame (WS_EX_DLGMODALFRAME, no sizing
/// frame: 3 px borders, 26 px caption) and resize it so the client stays `client` points, at
/// `scale` pixels per point (the display's scaling times any zoom), with the frame measured for
/// the window's own DPI. `light` dialogs get a light caption.
/// Returns true once applied (call again each frame until it is).
pub fn dialog_frame(title: &str, client: [f32; 2], light: bool, scale: f32) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
    let Some(hwnd) = find_own_window(title) else {
        return false;
    };
    unsafe {
        // IDM's light dialogs keep a light caption even in dark mode.
        let dark: i32 = (!light).into();
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
        // Owned by the main window, as IDM's dialogs are: no taskbar button, kept above it.
        if let Some(main) = find_own_window(crate::MAIN_TITLE) {
            use windows_sys::Win32::UI::WindowsAndMessaging::{GWLP_HWNDPARENT, WS_EX_APPWINDOW};
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, main as isize);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & !WS_EX_APPWINDOW;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & !WS_THICKFRAME;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 | WS_EX_DLGMODALFRAME;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: (client[0] * scale).round() as i32,
            bottom: (client[1] * scale).round() as i32,
        };
        AdjustWindowRectExForDpi(&mut rc, style, 0, ex, GetDpiForWindow(hwnd));
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            rc.right - rc.left,
            rc.bottom - rc.top,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    true
}

/// How much of the standard 8 px sizing border IDM's progress window does not have: its frame
/// is 2 px (1 px border + 1 px frame line) left, right and bottom (spec "Conventions").
const THIN_TRIM: i32 = 6;

unsafe extern "system" fn thin_frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    if msg == WM_NCCALCSIZE && wparam != 0 {
        // SAFETY: with wParam TRUE, lParam points to NCCALCSIZE_PARAMS (Win32 contract).
        let p = unsafe { &mut *(lparam as *mut NCCALCSIZE_PARAMS) };
        p.rgrc[0].left -= THIN_TRIM;
        p.rgrc[0].right += THIN_TRIM;
        p.rgrc[0].bottom += THIN_TRIM;
    }
    // Fixed size, like IDM's: the sizing edges (HTLEFT..HTBOTTOMRIGHT) act as plain border.
    if msg == WM_NCHITTEST && (10..=17).contains(&res) {
        return 18; // HTBORDER
    }
    res
}

/// Give our window titled `title` IDM's thin progress-window frame: standard caption, 2 px
/// borders, client staying `client` points at `scale` pixels per point (frame sizes follow the
/// window's DPI). Returns true once applied.
pub fn thin_frame(title: &str, client: [f32; 2], scale: f32) -> bool {
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    let Some(hwnd) = find_own_window(title) else {
        return false;
    };
    unsafe {
        SetWindowSubclass(hwnd, Some(thin_frame_proc), 0x1d6, 0);
        let dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            (client[0] * scale + 4.0 * dpi).round() as i32,
            (client[1] * scale + 33.0 * dpi).round() as i32,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    true
}

/// Menu windows answer mouse-activation with "don't activate": clicking an item must not take
/// focus from the main window (which would close the menu before the click completes).
unsafe extern "system" fn menu_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    const WM_MOUSEACTIVATE: u32 = 0x0021;
    const MA_NOACTIVATE: LRESULT = 3;
    if msg == WM_MOUSEACTIVATE {
        return MA_NOACTIVATE;
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Make our borderless window titled `title` behave and look like a Win32 menu popup: never
/// activated (the main window keeps focus and keyboard), no taskbar entry, Windows 11's small
/// rounded corners, shadow and a 1 px border in `border` (0x00BBGGRR), and no window-open
/// animation (menus just appear). Cheap to call every frame: returns true only on the call
/// that applied it — the window is created hidden and should be shown then.
pub fn menu_window(title: &str, border: u32) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUNDSMALL, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW};
    let Some(hwnd) = find_own_window(title) else {
        return false;
    };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_NOACTIVATE != 0 {
            return false;
        }
        let off: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED as u32,
            &off as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            (ex | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize,
        );
        SetWindowSubclass(hwnd, Some(menu_proc), 0x3e1, 0);
        let corner = DWMWCP_ROUNDSMALL;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const _ as *const c_void,
            size_of::<i32>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
    }
    true
}

/// Width of `text` in Segoe UI 12 px as GDI measures it (whole-pixel hinted advances) — the
/// width Win32 menus size themselves by. Cached per string.
pub fn menu_text_width(text: &str) -> i32 {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::SIZE;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateFontW, GetTextExtentPoint32W, HFONT, SelectObject,
    };
    thread_local! {
        static FONT: HFONT = {
            let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            // SAFETY: plain GDI object creation; the font lives for the thread.
            unsafe { CreateFontW(-12, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, face.as_ptr()) }
        };
        static CACHE: RefCell<HashMap<String, i32>> = RefCell::new(HashMap::new());
    }
    if let Some(w) = CACHE.with(|c| c.borrow().get(text).copied()) {
        return w;
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    let w = FONT.with(|font| unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let old = SelectObject(dc, *font);
        let mut size = SIZE { cx: 0, cy: 0 };
        GetTextExtentPoint32W(dc, wide.as_ptr(), wide.len() as i32, &mut size);
        SelectObject(dc, old);
        ReleaseDC(std::ptr::null_mut(), dc);
        size.cx
    });
    CACHE.with(|c| c.borrow_mut().insert(text.to_owned(), w));
    w
}

/// Give our window titled `title` a dark or light caption (DWM). Returns false if not found.
pub fn caption_dark(title: &str, dark: bool) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let Some(hwnd) = find_own_window(title) else {
        return false;
    };
    use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
    use windows_sys::Win32::Graphics::Gdi::{RDW_FRAME, RDW_INVALIDATE, RedrawWindow};
    let v: i32 = dark.into();
    // SAFETY: documented attribute with a 4-byte BOOL value.
    unsafe {
        let mut cur: i32 = -1;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &mut cur as *mut i32 as *mut c_void,
            size_of::<i32>() as u32,
        );
        if cur == v {
            return true;
        }
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &v as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
        // Windows repaints a caption only on the next activation change; force it now.
        RedrawWindow(
            hwnd,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_FRAME | RDW_INVALIDATE,
        );
    }
    true
}

/// One of our windows by handle, for crates that take a `raw-window-handle` window.
struct OwnWindow(HWND);

impl raw_window_handle::HasWindowHandle for OwnWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let h = std::num::NonZeroIsize::new(self.0 as isize)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let raw =
            raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(h));
        // SAFETY: the handle belongs to a live window of this process for the call's duration.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

/// Start dragging `file` out of our window titled `title` (to Explorer, a mail, a chat…), like
/// IDM's Download complete grip. The drag preview is the file's shell icon.
pub fn drag_file_out(title: &str, file: &std::path::Path) -> Result<(), String> {
    let hwnd = find_own_window(title).ok_or("window not found")?;
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    let preview = shell_icon_large(ext)
        .and_then(|rgba| {
            let img = image::RgbaImage::from_raw(32, 32, rgba)?;
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .ok()?;
            Some(png)
        })
        .unwrap_or_default();
    drag::start_drag(
        &OwnWindow(hwnd),
        drag::DragItem::Files(vec![file.to_path_buf()]),
        drag::Image::Raw(preview),
        |_, _| {},
        drag::Options::default(),
    )
    .map_err(|e| e.to_string())
}

/// Windows' "Choose your app mode": true when apps should be dark (`AppsUseLightTheme` = 0).
/// Missing value (older Windows): light.
pub fn apps_use_dark() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: plain registry read into a u32 of the stated size.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    rc == 0 && value == 0
}

/// Seconds since the last keyboard or mouse input anywhere ("limit while I'm using the
/// computer").
pub fn idle_seconds() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a LASTINPUTINFO with its size set, as the call requires.
    let (ok, now) = unsafe { (GetLastInputInfo(&mut info), GetTickCount()) };
    if ok == 0 {
        return 0;
    }
    u64::from(now.wrapping_sub(info.dwTime) / 1000)
}

// ---- taskbar progress (ITaskbarList3) -----------------------------------------------------
//
// windows-sys has no COM interface tables, so the three calls used are declared here in
// ITaskbarList3's vtable order: IUnknown (3), ITaskbarList (HrInit, AddTab, DeleteTab,
// ActivateTab, SetActiveAlt), ITaskbarList2 (MarkFullscreenWindow), ITaskbarList3
// (SetProgressValue, SetProgressState, …).

#[repr(C)]
struct TaskbarVtbl {
    _query_interface: usize,
    _add_ref: usize,
    _release: usize,
    hr_init: unsafe extern "system" fn(*mut c_void) -> i32,
    _add_tab: usize,
    _delete_tab: usize,
    _activate_tab: usize,
    _set_active_alt: usize,
    _mark_fullscreen_window: usize,
    set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> i32,
    set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, u32) -> i32,
}

/// The taskbar object (created on first use on the UI thread) and the last value shown.
struct Taskbar {
    obj: *mut *const TaskbarVtbl,
    shown: Option<Option<u32>>,
}

thread_local! {
    static TASKBAR: std::cell::RefCell<Option<Taskbar>> = const { std::cell::RefCell::new(None) };
}

/// The window's taskbar button shows `progress` (0..=1, the green bar), or nothing (None).
/// Only changes (in 1/1000 steps) reach the shell.
pub fn taskbar_progress(title: &str, progress: Option<f64>) {
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows_sys::core::GUID;
    const CLSID_TASKBARLIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
    const IID_ITASKBARLIST3: GUID = GUID::from_u128(0xea1afb91_9e28_4b86_90e9_9e9f8a5eefaf);
    const TBPF_NOPROGRESS: u32 = 0;
    const TBPF_NORMAL: u32 = 2;
    let value = progress.map(|p| (p.clamp(0.0, 1.0) * 1000.0).round() as u32);
    TASKBAR.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let mut obj: *mut c_void = std::ptr::null_mut();
            // SAFETY: COM on the UI thread (a second CoInitializeEx is harmless); the out
            // pointer receives an ITaskbarList3 on success, checked before use.
            unsafe {
                CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
                let hr = CoCreateInstance(
                    &CLSID_TASKBARLIST,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_ITASKBARLIST3,
                    &mut obj,
                );
                if hr < 0 || obj.is_null() {
                    return;
                }
                let tb = obj as *mut *const TaskbarVtbl;
                ((**tb).hr_init)(obj);
                *slot = Some(Taskbar {
                    obj: tb,
                    shown: None,
                });
            }
        }
        let Some(tb) = slot.as_mut() else { return };
        if tb.shown == Some(value) {
            return;
        }
        let Some(hwnd) = find_own_window(title) else {
            return;
        };
        // SAFETY: `obj` is a live ITaskbarList3 (never released: it lives as long as the UI
        // thread); the calls match the vtable above.
        unsafe {
            let this = tb.obj as *mut c_void;
            match value {
                Some(v) => {
                    ((**tb.obj).set_progress_state)(this, hwnd, TBPF_NORMAL);
                    ((**tb.obj).set_progress_value)(this, hwnd, u64::from(v), 1000);
                }
                None => {
                    ((**tb.obj).set_progress_state)(this, hwnd, TBPF_NOPROGRESS);
                }
            }
        }
        tb.shown = Some(value);
    });
}

/// A REG_SZ / REG_DWORD value under HKEY_CURRENT_USER\`key` (None when absent).
fn reg_string(key: &str, name: &str) -> Option<String> {
    reg_string_in(
        windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
        key,
        name,
    )
}

/// The language the installer's wizard ran in (tools/installer/mdm.iss writes it on every
/// install), taken once: read, then deleted. A languages.tsv code (`pt-br`); Inno's language
/// names have `_` for `-`.
pub fn take_install_language() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyValueW};
    const KEY: &str = r"Software\MisterDownloadManager";
    const NAME: &str = "InstallLanguage";
    let value = reg_string(KEY, NAME)?;
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let (key, name) = (w(KEY), w(NAME));
    // SAFETY: NUL-terminated key and value names.
    unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
    }
    Some(install_language_code(&value))
}

/// An Inno language name (`lang_pt_br`, tools/installer/languages.py) as a languages.tsv code.
fn install_language_code(inno: &str) -> String {
    inno.strip_prefix("lang_").unwrap_or(inno).replace('_', "-")
}

#[cfg(test)]
mod install_language_tests {
    #[test]
    fn inno_names_become_codes() {
        assert_eq!(super::install_language_code("lang_pt_br"), "pt-br");
        assert_eq!(super::install_language_code("lang_or"), "or");
        assert_eq!(
            super::install_language_code("lang_ca_es_valencia"),
            "ca-es-valencia"
        );
    }
}

/// The user's Windows region (Control Panel → Region): its short date's order, whether its
/// clock is 24-hour, and the first day of the week (0 = Sunday). None if Windows can't say.
pub fn region_formats() -> Option<(crate::regional::DateOrder, bool, u8)> {
    use windows_sys::Win32::Globalization::{
        GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK, LOCALE_SSHORTDATE, LOCALE_STIMEFORMAT,
    };
    let info = |what: u32| -> Option<String> {
        let mut buf = [0u16; 80];
        // SAFETY: null locale = the user's; the buffer's length is passed in characters.
        let n =
            unsafe { GetLocaleInfoEx(std::ptr::null(), what, buf.as_mut_ptr(), buf.len() as i32) };
        (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
    };
    let date = info(LOCALE_SSHORTDATE)?;
    let at = |c: char| date.find(c).unwrap_or(usize::MAX);
    let (d, m, y) = (at('d'), at('M'), at('y'));
    let order = if y < m && y < d {
        crate::regional::DateOrder::Ymd
    } else if m < d {
        crate::regional::DateOrder::Mdy
    } else {
        crate::regional::DateOrder::Dmy
    };
    // "HH:mm:ss" (24-hour) or "h:mm:ss tt".
    let clock24 = info(LOCALE_STIMEFORMAT).is_some_and(|t| t.contains('H'));
    // Windows counts from Monday (0) to Sunday (6).
    let first = info(LOCALE_IFIRSTDAYOFWEEK)
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map_or(0, |n| (n + 1) % 7);
    Some((order, clock24, first))
}

/// `t` (local time) as the user's Windows region writes a short date and a time with seconds
/// (Control Panel → Region): `30/10/2026 00:38:09`, `2026/10/30 0:38:09`… None if Windows can't.
pub fn local_date_time(t: chrono::NaiveDateTime) -> Option<String> {
    use chrono::{Datelike, Timelike};
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::Globalization::{DATE_SHORTDATE, GetDateFormatEx, GetTimeFormatEx};
    let st = SYSTEMTIME {
        wYear: t.year() as u16,
        wMonth: t.month() as u16,
        wDayOfWeek: t.weekday().num_days_from_sunday() as u16,
        wDay: t.day() as u16,
        wHour: t.hour() as u16,
        wMinute: t.minute() as u16,
        wSecond: t.second() as u16,
        wMilliseconds: 0,
    };
    let mut date = [0u16; 80];
    let mut time = [0u16; 80];
    // SAFETY: null locale = the user's; the buffers' lengths are passed in characters.
    let (d, h) = unsafe {
        (
            GetDateFormatEx(
                std::ptr::null(),
                DATE_SHORTDATE,
                &st,
                std::ptr::null(),
                date.as_mut_ptr(),
                date.len() as i32,
                std::ptr::null(),
            ),
            GetTimeFormatEx(
                std::ptr::null(),
                0,
                &st,
                std::ptr::null(),
                time.as_mut_ptr(),
                time.len() as i32,
            ),
        )
    };
    if d <= 1 || h <= 1 {
        return None;
    }
    let date = String::from_utf16_lossy(&date[..d as usize - 1]);
    let time = String::from_utf16_lossy(&time[..h as usize - 1]);
    Some(format!("{date} {time}"))
}

/// Windows' edition, release and build, e.g. "Windows 11 Pro 24H2 (build 26200)" — for problem
/// reports. ProductName still says "Windows 10" on 11; the build tells them apart.
pub fn windows_version() -> String {
    use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let get = |n| reg_string_in(HKEY_LOCAL_MACHINE, KEY, n);
    let build = get("CurrentBuild").unwrap_or_default();
    let mut product = get("ProductName").unwrap_or_else(|| "Windows".into());
    if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        product = product.replace("Windows 10", "Windows 11");
    }
    let release = get("DisplayVersion")
        .map(|v| format!(" {v}"))
        .unwrap_or_default();
    format!("{product}{release} (build {build})")
}

fn reg_string_in(
    root: windows_sys::Win32::System::Registry::HKEY,
    key: &str,
    name: &str,
) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: registry read into `buf`, whose size in bytes is passed in `size`.
    let rc = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
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

fn reg_dword(key: &str, name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: plain registry read into a u32 of the stated size.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    (rc == 0).then_some(value)
}

/// Windows' manual proxy (Settings → Network → Proxy), as (host, port), when it is on.
/// `ProxyServer` is "host:port" or "http=host:port;https=…": the http (else first) entry.
pub fn system_proxy() -> Option<(String, String)> {
    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";
    if reg_dword(KEY, "ProxyEnable")? == 0 {
        return None;
    }
    let server = reg_string(KEY, "ProxyServer")?;
    let entries: Vec<&str> = server.split(';').map(str::trim).collect();
    let pick = entries
        .iter()
        .find_map(|e| e.strip_prefix("http="))
        .or_else(|| entries.iter().find(|e| !e.contains('=')).copied())
        .or_else(|| {
            entries
                .first()
                .and_then(|e| e.split_once('='))
                .map(|(_, v)| v)
        })?;
    let (host, port) = pick.rsplit_once(':')?;
    Some((host.to_owned(), port.to_owned()))
}

/// Play a .wav file without waiting (Options → Sounds).
pub fn play_sound(file: &str) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    let wide: Vec<u16> = file.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated path; SND_ASYNC copies what it needs before returning.
    unsafe {
        PlaySoundW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

/// Options → General → "Launch … on startup": our entry under HKCU\…\Run (set or removed).
/// Make MDM (on) or stop it being (off) what Windows opens magnet links and .torrent files
/// with, for the current user (HKCU\Software\Classes; no admin). Off removes only what points
/// at us. Windows keeps a choice the user made in "Open with" above this (it can't be set by
/// programs), so another app chosen there stays in charge until the user picks MDM.
pub fn set_torrent_handlers(on: bool) {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW,
        RegSetKeyValueW,
    };
    const PROGID: &str = "MisterDownloadManager.Torrent";
    let w = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let set = |key: &str, name: Option<&str>, value: &str| {
        let (key, value) = (w(key), w(value));
        let name = name.map(w);
        // SAFETY: NUL-terminated strings; the size covers the value's bytes.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                REG_SZ,
                value.as_ptr() as *const c_void,
                (value.len() * 2) as u32,
            );
        }
    };
    let get = |key: &str, name: Option<&str>| -> Option<String> {
        let key = w(key);
        let name = name.map(w);
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: NUL-terminated key/name; `len` is the buffer's size in bytes.
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut c_void,
                &mut len,
            )
        };
        (r == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe = exe.display().to_string();
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    if on {
        set("Software\\Classes\\magnet", None, "URL:Magnet link");
        set("Software\\Classes\\magnet", Some("URL Protocol"), "");
        set("Software\\Classes\\magnet\\DefaultIcon", None, &icon);
        set(
            "Software\\Classes\\magnet\\shell\\open\\command",
            None,
            &open,
        );
        let prog = format!("Software\\Classes\\{PROGID}");
        set(&prog, None, "Torrent file");
        set(&format!("{prog}\\DefaultIcon"), None, &icon);
        set(&format!("{prog}\\shell\\open\\command"), None, &open);
        set("Software\\Classes\\.torrent", None, PROGID);
    } else {
        let ours = get("Software\\Classes\\magnet\\shell\\open\\command", None)
            .is_some_and(|c| c.contains(&exe));
        let tree = |k: &str| {
            let k = w(k);
            // SAFETY: NUL-terminated key under HKCU.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr()) };
        };
        if ours {
            tree("Software\\Classes\\magnet");
        }
        tree(&format!("Software\\Classes\\{PROGID}"));
        if get("Software\\Classes\\.torrent", None).as_deref() == Some(PROGID) {
            let (k, n) = (w("Software\\Classes\\.torrent"), w(""));
            // SAFETY: NUL-terminated key; an empty name is the default value.
            unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
        }
    }
    // Tell Explorer the associations changed.
    // SAFETY: no pointers are passed.
    unsafe {
        windows_sys::Win32::UI::Shell::SHChangeNotify(
            windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,
            windows_sys::Win32::UI::Shell::SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

/// Is Mister Download Manager in the user's Run key (set by Options or by the installer's
/// "Start when Windows starts")? The registry is the truth for Options → "Launch on startup".
pub fn run_at_startup() -> bool {
    reg_string_in(
        windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        "MisterDownloadManager",
    )
    .is_some()
}

pub fn set_run_at_startup(on: bool) {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Run\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "MisterDownloadManager\0".encode_utf16().collect();
    if on {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let value: Vec<u16> = format!("\"{}\"", exe.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: NUL-terminated key, name and value; the size covers the value's bytes.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr() as *const c_void,
                (value.len() * 2) as u32,
            );
        }
    } else {
        // SAFETY: NUL-terminated key and value name.
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
        }
    }
}

/// The work area (left, top, right, bottom) of the monitor nearest to screen point (x, y).
pub fn work_area(x: i32, y: i32) -> Option<[i32; 4]> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        (GetMonitorInfoW(mon, &mut mi) != 0).then_some([
            mi.rcWork.left,
            mi.rcWork.top,
            mi.rcWork.right,
            mi.rcWork.bottom,
        ])
    }
}

/// The shell's 16 × 16 icon for files with extension `ext` (e.g. "zip"), as RGBA.
pub fn shell_icon(ext: &str) -> Option<Vec<u8>> {
    icon_for(
        &format!("file.{ext}"),
        FILE_ATTRIBUTE_NORMAL,
        SHGFI_SMALLICON,
        16,
    )
}

/// The shell's 32 × 32 icon for files with extension `ext`, as RGBA.
pub fn shell_icon_large(ext: &str) -> Option<Vec<u8>> {
    icon_for(&format!("file.{ext}"), FILE_ATTRIBUTE_NORMAL, 0, 32)
}

/// Windows' type name for a file name ("BIN File", "Compressed (zipped) Folder").
pub fn type_name(file_name: &str) -> Option<String> {
    use windows_sys::Win32::UI::Shell::SHGFI_TYPENAME;
    let name: Vec<u16> = file_name.encode_utf16().chain([0]).collect();
    // SAFETY: `info` is a zeroed SHFILEINFOW of the stated size; the name is NUL-terminated;
    // with SHGFI_USEFILEATTRIBUTES the file need not exist.
    unsafe {
        let mut info: SHFILEINFOW = zeroed();
        let ok = SHGetFileInfoW(
            name.as_ptr(),
            FILE_ATTRIBUTE_NORMAL,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES,
        );
        let len = info.szTypeName.iter().position(|c| *c == 0).unwrap_or(0);
        (ok != 0 && len > 0).then(|| String::from_utf16_lossy(&info.szTypeName[..len]))
    }
}

/// RGBA pixels (size × size) of the shell icon for `path`; `extra` adds SHGFI flags.
fn icon_for(path: &str, attrs: u32, extra: u32, size: i32) -> Option<Vec<u8>> {
    let name: Vec<u16> = path.encode_utf16().chain([0]).collect();
    unsafe {
        let mut info: SHFILEINFOW = zeroed();
        let ok = SHGetFileInfoW(
            name.as_ptr(),
            attrs,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_USEFILEATTRIBUTES | extra,
        );
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        hicon_rgba(info.hIcon, size)
    }
}

/// RGBA pixels (size × size) of icon, which is destroyed.
unsafe fn hicon_rgba(
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    size: i32,
) -> Option<Vec<u8>> {
    unsafe {
        let mut ii: ICONINFO = zeroed();
        let got = GetIconInfo(icon, &mut ii) != 0;
        let mut out = None;
        if got && !ii.hbmColor.is_null() {
            let mut bmi: BITMAPINFO = zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            };
            let mut px = vec![0u8; (size * size * 4) as usize];
            let dc = GetDC(std::ptr::null_mut());
            let lines = GetDIBits(
                dc,
                ii.hbmColor,
                0,
                size as u32,
                px.as_mut_ptr() as *mut c_void,
                &mut bmi,
                DIB_RGB_COLORS,
            );
            ReleaseDC(std::ptr::null_mut(), dc);
            if lines == size {
                let has_alpha = px.chunks(4).any(|c| c[3] != 0);
                for c in px.chunks_mut(4) {
                    c.swap(0, 2); // BGRA → RGBA
                    if !has_alpha {
                        c[3] = 255; // legacy icons without alpha
                    }
                }
                out = Some(px);
            }
        }
        if got {
            DeleteObject(ii.hbmColor as _);
            DeleteObject(ii.hbmMask as _);
        }
        DestroyIcon(icon);
        out
    }
}

/// Windows' own large (32 × 32) "open folder" icon, as RGBA (Download complete).
pub fn folder_open_icon_large() -> Option<Vec<u8>> {
    use windows_sys::Win32::UI::Shell::{
        SHGSI_ICON, SHGSI_LARGEICON, SHGetStockIconInfo, SHSTOCKICONINFO, SIID_FOLDEROPEN,
    };
    unsafe {
        let mut info: SHSTOCKICONINFO = zeroed();
        info.cbSize = size_of::<SHSTOCKICONINFO>() as u32;
        if SHGetStockIconInfo(SIID_FOLDEROPEN, SHGSI_ICON | SHGSI_LARGEICON, &mut info) != 0
            || info.hIcon.is_null()
        {
            return None;
        }
        hicon_rgba(info.hIcon, 32)
    }
}

/// The drop target's OLE side (`droptarget.rs`): a minimal `IDropTarget` that accepts links
/// (UniformResourceLocatorW), text and files.
pub mod ole {

    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, mpsc::Sender};

    use windows_sys::Win32::Foundation::HWND;

    type HResult = i32;
    const S_OK: HResult = 0;
    const E_NOINTERFACE: HResult = 0x8000_4002_u32 as i32;
    const DROPEFFECT_NONE: u32 = 0;
    const DROPEFFECT_COPY: u32 = 1;
    const CF_UNICODETEXT: u16 = 13;
    const CF_HDROP: u16 = 15;
    const TYMED_HGLOBAL: u32 = 1;
    const DVASPECT_CONTENT: u32 = 1;

    #[repr(C)]
    #[derive(PartialEq)]
    struct Guid(u32, u16, u16, [u8; 8]);
    const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IDROPTARGET: Guid = Guid(0x0000_0122, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

    #[repr(C)]
    struct PointL {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct FormatEtc {
        cf: u16,
        ptd: *mut c_void,
        aspect: u32,
        lindex: i32,
        tymed: u32,
    }

    #[repr(C)]
    struct StgMedium {
        tymed: u32,
        handle: *mut c_void,
        unk_for_release: *mut c_void,
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn OleInitialize(reserved: *mut c_void) -> HResult;
        fn RegisterDragDrop(hwnd: HWND, target: *mut c_void) -> HResult;
        fn RevokeDragDrop(hwnd: HWND) -> HResult;
        fn ReleaseStgMedium(medium: *mut StgMedium);
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalLock(h: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(h: *mut c_void) -> i32;
        fn GlobalSize(h: *mut c_void) -> usize;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(hdrop: *mut c_void, i: u32, buf: *mut u16, cch: u32) -> u32;
    }

    /// IDataObject's vtable up to GetData (index 3), all we call.
    #[repr(C)]
    struct DataObjectVtbl {
        _query_interface: usize,
        _add_ref: usize,
        _release: usize,
        get_data:
            unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium) -> HResult,
    }

    #[repr(C)]
    struct Vtbl {
        query_interface:
            unsafe extern "system" fn(*mut Target, *const Guid, *mut *mut c_void) -> HResult,
        add_ref: unsafe extern "system" fn(*mut Target) -> u32,
        release: unsafe extern "system" fn(*mut Target) -> u32,
        drag_enter:
            unsafe extern "system" fn(*mut Target, *mut c_void, u32, PointL, *mut u32) -> HResult,
        drag_over: unsafe extern "system" fn(*mut Target, u32, PointL, *mut u32) -> HResult,
        drag_leave: unsafe extern "system" fn(*mut Target) -> HResult,
        drop: unsafe extern "system" fn(*mut Target, *mut c_void, u32, PointL, *mut u32) -> HResult,
    }

    #[repr(C)]
    struct Target {
        vtbl: &'static Vtbl,
        refs: AtomicU32,
        tx: Sender<String>,
        over: Arc<AtomicBool>,
        /// What the current drag carries (decided at DragEnter).
        accepts: AtomicBool,
        /// Woken at enter, leave and drop: OLE's drag loop sends the window no input, so
        /// egui wouldn't draw the highlight, or take the drop, until something else came.
        wake: egui::Context,
    }

    static VTBL: Vtbl = Vtbl {
        query_interface,
        add_ref,
        release,
        drag_enter,
        drag_over,
        drag_leave,
        drop,
    };

    unsafe extern "system" fn query_interface(
        this: *mut Target,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> HResult {
        // SAFETY: COM calls with valid pointers.
        unsafe {
            if *iid == IID_IUNKNOWN || *iid == IID_IDROPTARGET {
                add_ref(this);
                *out = this.cast();
                S_OK
            } else {
                *out = std::ptr::null_mut();
                E_NOINTERFACE
            }
        }
    }

    unsafe extern "system" fn add_ref(this: *mut Target) -> u32 {
        // SAFETY: `this` is a live Target (COM reference counting).
        unsafe { (*this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
    }

    unsafe extern "system" fn release(this: *mut Target) -> u32 {
        // SAFETY: the last release frees the Box made in `Registration::new`.
        unsafe {
            let n = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
            if n == 0 {
                std::mem::drop(Box::from_raw(this));
            }
            n
        }
    }

    unsafe extern "system" fn drag_enter(
        this: *mut Target,
        data: *mut c_void,
        _keys: u32,
        _pt: PointL,
        effect: *mut u32,
    ) -> HResult {
        // SAFETY: valid for the call.
        unsafe {
            let ok = !read(data).is_empty();
            (*this).accepts.store(ok, Ordering::Relaxed);
            (*this).over.store(ok, Ordering::Relaxed);
            (*this).wake.request_repaint();
            *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        }
        S_OK
    }

    unsafe extern "system" fn drag_over(
        this: *mut Target,
        _keys: u32,
        _pt: PointL,
        effect: *mut u32,
    ) -> HResult {
        // SAFETY: valid for the call.
        unsafe {
            *effect = if (*this).accepts.load(Ordering::Relaxed) {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            };
        }
        S_OK
    }

    unsafe extern "system" fn drag_leave(this: *mut Target) -> HResult {
        // SAFETY: valid for the call.
        unsafe {
            (*this).over.store(false, Ordering::Relaxed);
            (*this).wake.request_repaint();
        }
        S_OK
    }

    unsafe extern "system" fn drop(
        this: *mut Target,
        data: *mut c_void,
        _keys: u32,
        _pt: PointL,
        effect: *mut u32,
    ) -> HResult {
        // SAFETY: valid for the call.
        unsafe {
            (*this).over.store(false, Ordering::Relaxed);
            let found = read(data);
            *effect = if found.is_empty() {
                DROPEFFECT_NONE
            } else {
                DROPEFFECT_COPY
            };
            for s in found {
                let _ = (*this).tx.send(s);
            }
            (*this).wake.request_repaint();
        }
        S_OK
    }

    /// The addresses (or file paths) a drag carries: a link, else text that holds addresses,
    /// else dropped files.
    unsafe fn read(data: *mut c_void) -> Vec<String> {
        // SAFETY: `data` is the IDataObject OLE passed in.
        unsafe {
            let name: Vec<u16> = "UniformResourceLocatorW\0".encode_utf16().collect();
            let url_format = RegisterClipboardFormatW(name.as_ptr()) as u16;
            for cf in [url_format, CF_UNICODETEXT] {
                if let Some(text) = get_text(data, cf) {
                    let urls: Vec<String> = text
                        .split_whitespace()
                        .filter(|w| {
                            let w = w.to_ascii_lowercase();
                            w.starts_with("http://")
                                || w.starts_with("https://")
                                || w.starts_with("ftp://")
                                || w.starts_with("ftps://")
                                || w.starts_with("magnet:")
                        })
                        .map(str::to_owned)
                        .collect();
                    if !urls.is_empty() {
                        return urls;
                    }
                }
            }
            files(data)
        }
    }

    unsafe fn medium(data: *mut c_void, cf: u16) -> Option<StgMedium> {
        // SAFETY: IDataObject::GetData through its vtable.
        unsafe {
            let vtbl = *(data as *const *const DataObjectVtbl);
            let fmt = FormatEtc {
                cf,
                ptd: std::ptr::null_mut(),
                aspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            };
            let mut m = StgMedium {
                tymed: 0,
                handle: std::ptr::null_mut(),
                unk_for_release: std::ptr::null_mut(),
            };
            (((*vtbl).get_data)(data, &fmt, &mut m) == S_OK && !m.handle.is_null()).then_some(m)
        }
    }

    unsafe fn get_text(data: *mut c_void, cf: u16) -> Option<String> {
        // SAFETY: a locked HGLOBAL of UTF-16, read within its size, then released.
        unsafe {
            let mut m = medium(data, cf)?;
            let p = GlobalLock(m.handle) as *const u16;
            let text = (!p.is_null()).then(|| {
                let n = GlobalSize(m.handle) / 2;
                let s = std::slice::from_raw_parts(p, n);
                let end = s.iter().position(|c| *c == 0).unwrap_or(n);
                String::from_utf16_lossy(&s[..end])
            });
            GlobalUnlock(m.handle);
            ReleaseStgMedium(&mut m);
            text
        }
    }

    unsafe fn files(data: *mut c_void) -> Vec<String> {
        // SAFETY: an HDROP medium, queried with DragQueryFileW, then released.
        unsafe {
            let Some(mut m) = medium(data, CF_HDROP) else {
                return Vec::new();
            };
            let n = DragQueryFileW(m.handle, u32::MAX, std::ptr::null_mut(), 0);
            let mut out = Vec::new();
            for i in 0..n {
                let len = DragQueryFileW(m.handle, i, std::ptr::null_mut(), 0);
                let mut buf = vec![0u16; len as usize + 1];
                DragQueryFileW(m.handle, i, buf.as_mut_ptr(), len + 1);
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            ReleaseStgMedium(&mut m);
            out
        }
    }

    /// Our target registered on the drop window; revoked when dropped.
    pub struct Registration {
        hwnd: HWND,
        target: *mut Target,
    }

    impl Registration {
        /// None until the window exists (tried again next frame).
        pub fn new(
            title: &str,
            tx: Sender<String>,
            over: Arc<AtomicBool>,
            wake: &egui::Context,
        ) -> Option<Self> {
            let hwnd = crate::win32::find_own_window(title)?;
            let target = Box::into_raw(Box::new(Target {
                vtbl: &VTBL,
                refs: AtomicU32::new(1),
                tx,
                over,
                accepts: AtomicBool::new(false),
                wake: wake.clone(),
            }));
            // SAFETY: OLE on the UI thread (winit initialised it already: S_FALSE is fine);
            // any target winit registered goes first, then ours (RegisterDragDrop AddRefs it).
            unsafe {
                OleInitialize(std::ptr::null_mut());
                RevokeDragDrop(hwnd);
                if RegisterDragDrop(hwnd, target.cast()) != S_OK {
                    release(target);
                    return None;
                }
            }
            Some(Registration { hwnd, target })
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY: revoke OLE's reference, then ours.
            unsafe {
                RevokeDragDrop(self.hwnd);
                release(self.target);
            }
        }
    }
}

/// A named mutex held while the app runs, so the installer can tell (Inno Setup's `AppMutex`)
/// and wait for `/exit` to finish before replacing or removing the program. The handle closes
/// with the process.
pub fn mark_running() {
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "MisterDownloadManager.Running\0".encode_utf16().collect();
    // SAFETY: NUL-terminated name; the handle is deliberately kept for the process's life.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
    }
}

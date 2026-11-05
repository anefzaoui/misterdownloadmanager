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
    DefSubclassProc, SHFILEINFOW, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW, SetWindowSubclass,
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

/// The window of this thread that holds the mouse capture (a button is down in it), if any.
fn own_capture() -> Option<HWND> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture;
    // SAFETY: a plain query; GetCapture only sees this thread's windows.
    let h = unsafe { GetCapture() };
    (!h.is_null()).then_some(h)
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

/// The address a connection to `dst` would leave from (PAC's `myIpAddress`), asked of the
/// routing table (GetBestRoute2). No socket is opened: a UDP socket, the usual way to ask, is a
/// listening socket to Windows' firewall, which would ask the user about MDM. Installed as the
/// engine's lookup at start (`mdm_engine::net::set_source_lookup`); unix.rs has none (the
/// engine's UDP way raises no prompt there).
pub fn source_address(dst: std::net::IpAddr) -> Option<std::net::IpAddr> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetBestRoute2, MIB_IPFORWARD_ROW2};
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_INET};
    // SAFETY: plain-data structures, zero is a valid value for each; the union is written and
    // read by its family; GetBestRoute2 only reads `dest` and writes `row` and `src`.
    unsafe {
        let mut dest: SOCKADDR_INET = zeroed();
        match dst {
            IpAddr::V4(a) => {
                dest.Ipv4.sin_family = AF_INET;
                dest.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(a.octets());
            }
            IpAddr::V6(a) => {
                dest.Ipv6.sin6_family = AF_INET6;
                dest.Ipv6.sin6_addr.u.Byte = a.octets();
            }
        }
        let mut row: MIB_IPFORWARD_ROW2 = zeroed();
        let mut src: SOCKADDR_INET = zeroed();
        let r = GetBestRoute2(
            std::ptr::null(),
            0,
            std::ptr::null(),
            &dest,
            0,
            &mut row,
            &mut src,
        );
        if r != 0 {
            return None;
        }
        match src.si_family {
            AF_INET => Some(IpAddr::V4(Ipv4Addr::from(
                src.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes(),
            ))),
            AF_INET6 => Some(IpAddr::V6(Ipv6Addr::from(src.Ipv6.sin6_addr.u.Byte))),
            _ => None,
        }
    }
}

/// Off-screen test runs (MDM_OFFSCREEN=1): every top-level window of ours gets WS_EX_NOACTIVATE,
/// so Windows never makes it the foreground window, whatever asks (a click, a dialog coming to
/// the front, an accessibility client): the user's keystrokes stay in their own window while
/// tests run. UI Automation still works on them (Select, Invoke, Toggle need no activation).
/// Cheap: only our own windows are touched, once each.
pub fn keep_from_foreground() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, IsWindowVisible, WS_EX_NOACTIVATE,
    };
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
        let pid = lparam as u32;
        let mut owner = 0u32;
        // SAFETY: plain queries and a style change on a window of this process.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner == pid && IsWindowVisible(hwnd) != 0 {
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                if ex & WS_EX_NOACTIVATE == 0 {
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_NOACTIVATE) as isize);
                }
            }
        }
        1
    }
    // SAFETY: the callback only reads its pid from lparam.
    unsafe {
        EnumWindows(Some(cb), GetCurrentProcessId() as LPARAM);
    }
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

/// Hit-test results (winuser.h).
const HTCLIENT: LRESULT = 1;
const HTCAPTION: LRESULT = 2;
const HTSYSMENU: LRESULT = 3;
const HTBORDER: LRESULT = 18;

/// The system menu's "Hide to the notification area" (#63): a command id of ours (below
/// 0xF000, its low 4 bits clear, as WM_SYSCOMMAND wants).
const SC_HIDE_TO_TRAY: usize = 0x0E10;

/// Where a drawn title bar's parts are, in client pixels (egui publishes them each frame,
/// `set_caption_hits`): the caption strip's height, the icon (the system menu's place) and the
/// caption buttons, each [left, top, right, bottom].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptionHits {
    pub strip_h: i32,
    pub icon: [i32; 4],
    pub buttons: Vec<[i32; 4]>,
}

thread_local! {
    /// Each framed window's caption parts, by HWND. Read in its subclass proc with try_borrow:
    /// WM_NCHITTEST can arrive while egui's frame holds the map (re-entered messages).
    static CAPTION_HITS: std::cell::RefCell<std::collections::HashMap<isize, CaptionHits>> =
        Default::default();
    /// Windows whose system menu's "Hide to the notification area" was chosen.
    static HIDE_ASKED: std::cell::RefCell<std::collections::HashSet<isize>> = Default::default();
}

/// Publish where window `hwnd`'s drawn title bar's parts are (client pixels), for its hit test.
pub fn set_caption_hits(hwnd: isize, hits: CaptionHits) {
    CAPTION_HITS.with(|m| {
        if let Ok(mut m) = m.try_borrow_mut() {
            m.insert(hwnd, hits);
        }
    });
}

/// The window is gone: forget its caption parts.
pub fn forget_caption(hwnd: isize) {
    CAPTION_HITS.with(|m| {
        if let Ok(mut m) = m.try_borrow_mut() {
            m.remove(&hwnd);
        }
    });
    HIDE_ASKED.with(|s| {
        if let Ok(mut s) = s.try_borrow_mut() {
            s.remove(&hwnd);
        }
    });
}

/// Was "Hide to the notification area" chosen in window `hwnd`'s system menu (once)?
pub fn take_hide_asked(hwnd: isize) -> bool {
    HIDE_ASKED.with(|s| s.try_borrow_mut().is_ok_and(|mut s| s.remove(&hwnd)))
}

fn inside(r: &[i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

/// What client point `(x, y)` is: our caption buttons are the client's (egui hovers and clicks
/// them), the icon is the system menu, the rest of the caption strip is the caption (the system
/// moves, snaps and shakes the window from it, double-clicks and right-clicks it), below it the
/// client's.
fn frame_hit(x: i32, y: i32, hits: &CaptionHits) -> LRESULT {
    if hits.buttons.iter().any(|b| inside(b, x, y)) {
        HTCLIENT
    } else if inside(&hits.icon, x, y) {
        HTSYSMENU
    } else if y >= 0 && y < hits.strip_h {
        HTCAPTION
    } else {
        HTCLIENT
    }
}

/// IDM's progress-window frame (2 px borders, fixed size) with the title bar ours (#63, the
/// approach Windows Terminal, Zed and WezTerm take): the window keeps its system frame and
/// WS_CAPTION (Aero Snap, shake, the system menu and Alt+Space, the DWM shadow and rounded
/// corners), its caption given to the client, which draws it; the hit test reads where the
/// drawn parts are.
unsafe extern "system" fn thin_frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_NCACTIVATE, WM_SYSCOMMAND};
    if msg == WM_NCCALCSIZE && wparam != 0 {
        // SAFETY: with wParam TRUE, lParam points to NCCALCSIZE_PARAMS (Win32 contract).
        let p = unsafe { &mut *(lparam as *mut NCCALCSIZE_PARAMS) };
        let top = p.rgrc[0].top;
        let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        p.rgrc[0].left -= THIN_TRIM;
        p.rgrc[0].right += THIN_TRIM;
        p.rgrc[0].bottom += THIN_TRIM;
        // No system caption: the client starts at the window's top.
        p.rgrc[0].top = top;
        return res;
    }
    // Activation changes repaint the non-client area: with lParam -1 the system skips that,
    // so its own caption never flashes over ours.
    if msg == WM_NCACTIVATE {
        return unsafe { DefSubclassProc(hwnd, msg, wparam, -1) };
    }
    if msg == WM_SYSCOMMAND && (wparam & 0xFFF0) == SC_HIDE_TO_TRAY {
        HIDE_ASKED.with(|s| {
            if let Ok(mut s) = s.try_borrow_mut() {
                s.insert(hwnd as isize);
            }
        });
        return 0;
    }
    let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    if msg == WM_NCHITTEST {
        // Fixed size, like IDM's: the sizing edges (HTLEFT..HTBOTTOMRIGHT) act as plain border.
        if (10..=17).contains(&res) {
            return HTBORDER;
        }
        if res == HTCLIENT {
            use windows_sys::Win32::Foundation::POINT;
            use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
            let mut pt = POINT {
                x: (lparam & 0xffff) as i16 as i32,
                y: ((lparam >> 16) & 0xffff) as i16 as i32,
            };
            // SAFETY: our own window; the point is a local.
            unsafe { ScreenToClient(hwnd, &mut pt) };
            let hit = CAPTION_HITS.with(|m| {
                m.try_borrow()
                    .ok()
                    .and_then(|m| m.get(&(hwnd as isize)).map(|h| frame_hit(pt.x, pt.y, h)))
            });
            return hit.unwrap_or(HTCLIENT);
        }
    }
    res
}

/// Give our window titled `title` IDM's thin progress-window frame with its title bar drawn by
/// us (winui::caption, #63): 2 px borders, no system caption, the client staying `client`
/// points (the drawn title bar included) at `scale` pixels per point. With `hide_item` (the
/// translated "Hide to the notification area"), the system menu gets it, Alt+Space reaching it
/// from the keyboard; None takes it out. Call while the window is still hidden, and again when
/// the size, the scale or the item changes. Returns the window, once applied.
pub fn thin_frame(
    title: &str,
    client: [f32; 2],
    scale: f32,
    hide_item: Option<&str>,
) -> Option<isize> {
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, DeleteMenu, GetSystemMenu, MF_BYCOMMAND, MF_STRING,
    };
    let hwnd = find_own_window(title)?;
    unsafe {
        SetWindowSubclass(hwnd, Some(thin_frame_proc), 0x1d6, 0);
        let menu = GetSystemMenu(hwnd, 0);
        if !menu.is_null() {
            DeleteMenu(menu, SC_HIDE_TO_TRAY as u32, MF_BYCOMMAND);
            if let Some(text) = hide_item {
                let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                AppendMenuW(menu, MF_STRING, SC_HIDE_TO_TRAY, w.as_ptr());
            }
        }
        let dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            (client[0] * scale + 4.0 * dpi).round() as i32,
            (client[1] * scale + 2.0 * dpi).round() as i32,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    Some(hwnd as isize)
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    #[test]
    fn the_drawn_title_bar_hits_as_caption_icon_and_buttons() {
        // 548 × 32 points at 1.5 px per point, 4 buttons of 46 at the end, the icon at the start.
        let h = CaptionHits {
            strip_h: 48,
            icon: [0, 0, 48, 48],
            buttons: (0..4)
                .map(|i| {
                    let x1 = 822 - i * 69;
                    [x1 - 69, 0, x1, 48]
                })
                .collect(),
        };
        assert_eq!(frame_hit(10, 10, &h), HTSYSMENU);
        assert_eq!(frame_hit(300, 10, &h), HTCAPTION);
        assert_eq!(frame_hit(821, 10, &h), HTCLIENT);
        assert_eq!(frame_hit(822 - 4 * 69, 10, &h), HTCLIENT);
        assert_eq!(frame_hit(822 - 4 * 69 - 1, 10, &h), HTCAPTION);
        // Below the bar: the client's.
        assert_eq!(frame_hit(300, 48, &h), HTCLIENT);
        // Nothing published yet: everything the client's (frame_hit isn't asked then).
        assert_eq!(frame_hit(300, 10, &CaptionHits::default()), HTCLIENT);
    }
}

/// The main window's "Minimize to the notification area" (#63): its minimize command (title bar,
/// system menu, taskbar) asks the app to hide it instead. The app can't see the window go
/// minimized: an idle app runs no frame while minimized, so it would never know.
struct MainMinimize {
    hwnd: isize,
    ctx: Option<egui::Context>,
    enabled: bool,
    asked: bool,
}

thread_local! {
    static MAIN_MINIMIZE: std::cell::RefCell<MainMinimize> = const {
        std::cell::RefCell::new(MainMinimize { hwnd: 0, ctx: None, enabled: false, asked: false })
    };
}

unsafe extern "system" fn main_minimize_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_SYSCOMMAND;
    const SC_MINIMIZE: usize = 0xF020;
    if msg == WM_SYSCOMMAND && (wparam & 0xFFF0) == SC_MINIMIZE {
        let caught = MAIN_MINIMIZE.with(|m| {
            let Ok(mut m) = m.try_borrow_mut() else {
                return false;
            };
            if !m.enabled {
                return false;
            }
            m.asked = true;
            if let Some(ctx) = &m.ctx {
                ctx.request_repaint();
            }
            true
        });
        if caught {
            return 0;
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Keep the main window's minimize command in step with "Minimize to the notification area"
/// (`enabled`: the option is on and a tray shows MDM's icon). Call every frame (cheap once the
/// window is found). True once after the command came: hide the window now.
pub fn main_minimize_to_tray(ctx: &egui::Context, enabled: bool) -> bool {
    MAIN_MINIMIZE.with(|m| {
        let Ok(mut m) = m.try_borrow_mut() else {
            return false;
        };
        if m.hwnd == 0
            && let Some(hwnd) = find_own_window(crate::MAIN_TITLE)
        {
            // SAFETY: our own window; the proc lives as long as the program.
            unsafe { SetWindowSubclass(hwnd, Some(main_minimize_proc), 0x7a1, 0) };
            m.hwnd = hwnd as isize;
            m.ctx = Some(ctx.clone());
        }
        m.enabled = enabled;
        std::mem::take(&mut m.asked)
    })
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

/// Start dragging `file` out of our window (to Explorer, a mail, a chat…), like IDM's Download
/// complete grip. The drag preview is the file's shell icon. The window is the one holding the
/// mouse (the button is down on its grip: winit captured the mouse for it), so it is the right
/// one when several show the same title; else our window titled `title`, given in the interface
/// language, as it shows.
pub fn drag_file_out(title: &str, file: &std::path::Path) -> Result<(), String> {
    let hwnd = own_capture()
        .or_else(|| find_own_window(title))
        .ok_or("window not found")?;
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let what = crate::fileicons::What::of(crate::fileicons::Kind::File(name));
    let preview = file_icon(&what, 32)
        .and_then(|icon| {
            let crate::fileicons::Native::Rgba { side, rgba, .. } = icon else {
                return None;
            };
            let img = image::RgbaImage::from_raw(side, side, rgba)?;
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

/// Sleep now (not hibernate: rundll32's SetSuspendState read its own window handle as
/// "hibernate"). `force`: don't let programs delay it. False when Windows refused.
pub fn sleep(force: bool) -> bool {
    use windows_sys::Win32::System::Power::SetSuspendState;
    // SAFETY: a plain call; FALSE / force / FALSE (no wake events disabled).
    unsafe { SetSuspendState(false, force, false) }
}

/// Is this screen point (a saved window's corner, in points) on a monitor now? A window put
/// back where a since-removed screen was would open out of sight.
pub fn on_a_screen(x: i32, y: i32) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
    // SAFETY: a plain query.
    !unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) }.is_null()
}

/// Rows a notch of the mouse wheel scrolls (Mouse settings → "lines to scroll at a time", 3
/// unless changed); None when it is set to a screen at a time.
pub fn wheel_lines() -> Option<f32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETWHEELSCROLLLINES, SystemParametersInfoW,
    };
    let mut lines: u32 = 3;
    // SAFETY: a plain query into a u32 it writes.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETWHEELSCROLLLINES, 0, (&raw mut lines).cast(), 0) };
    match lines {
        _ if ok == 0 => Some(3.0),
        // WHEEL_PAGESCROLL
        u32::MAX => None,
        n => Some(n as f32),
    }
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

/// The shell's icon for a file type or a folder (fileicons.rs), as Explorer shows it, from the
/// system image list that fits `px` pixels: small, large, extra large (16, 32, 48 at 100 %, more
/// with the system's scale), then jumbo (256) for anything bigger. The file need not exist. Run
/// on fileicons.rs's lookup thread: the first lookup of a type loads its shell handler.
///
/// One lookup at a time in the process (the drag picture asks from the UI thread): two threads
/// in the shell at once could miss an icon. A miss is tried once more (a handler's first load).
pub fn file_icon(what: &crate::fileicons::What, px: u32) -> Option<crate::fileicons::Native> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    file_icon_once(what, px).or_else(|| file_icon_once(what, px))
}

fn file_icon_once(what: &crate::fileicons::What, px: u32) -> Option<crate::fileicons::Native> {
    use crate::fileicons::What;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
    use windows_sys::Win32::UI::Shell::SHGFI_SYSICONINDEX;
    com_on_this_thread();
    let (name, attrs) = match what {
        What::FolderOpen => return list_icon(open_folder_index()?, px),
        What::Folder => ("folder".to_string(), FILE_ATTRIBUTE_DIRECTORY),
        What::File(Some(ext)) => (format!("file.{ext}"), FILE_ATTRIBUTE_NORMAL),
        What::File(None) => ("file".to_string(), FILE_ATTRIBUTE_NORMAL),
    };
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `info` is a zeroed SHFILEINFOW of the stated size; the name is NUL-terminated;
    // with SHGFI_USEFILEATTRIBUTES the file need not exist. SYSICONINDEX returns no icon to free.
    let index = unsafe {
        let mut info: SHFILEINFOW = zeroed();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            attrs,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES,
        );
        if ok == 0 {
            return None;
        }
        info.iIcon
    };
    list_icon(index, px)
}

/// The system image lists' icon `index` at the list that fits `px` pixels (see `file_icon`).
fn list_icon(index: i32, px: u32) -> Option<crate::fileicons::Native> {
    use windows_sys::Win32::UI::Shell::{SHIL_EXTRALARGE, SHIL_JUMBO, SHIL_LARGE, SHIL_SMALL};
    let mut got: Option<(u32, Vec<u8>)> = None;
    for list in [SHIL_SMALL, SHIL_LARGE, SHIL_EXTRALARGE, SHIL_JUMBO] {
        if got.as_ref().is_some_and(|(side, _)| *side >= px) {
            break;
        }
        let Some((side, rgba)) = image_list_icon(list, index) else {
            continue;
        };
        // A type with no picture that big gets its smaller one in the jumbo list's top-left
        // corner: keep the smaller one then.
        if list == SHIL_JUMBO
            && let Some((prev, _)) = &got
            && drawn_within(&rgba, side, *prev)
        {
            break;
        }
        got = Some((side, rgba));
    }
    let (side, rgba) = got?;
    Some(crate::fileicons::Native::Rgba {
        side,
        rgba,
        premultiplied: false,
    })
}

/// Whether every visible pixel of the `side`-square `rgba` lies in its top-left `within` square.
fn drawn_within(rgba: &[u8], side: u32, within: u32) -> bool {
    rgba.as_chunks::<4>().0.iter().enumerate().all(|(i, c)| {
        let (x, y) = (i as u32 % side, i as u32 / side);
        c[3] == 0 || (x < within && y < within)
    })
}

/// COM for this thread (apartment-threaded, as the shell's icon handlers expect), once.
fn com_on_this_thread() {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    DONE.with(|done| {
        if !done.replace(true) {
            // SAFETY: no reserved pointer; the thread keeps COM for its lifetime.
            unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        }
    });
}

/// The start of the shell's `IImageList` vtable (commoncontrols.h), as far as `GetIcon`. The list
/// is called through it rather than comctl32's `ImageList_GetIcon`, which needs the version 6
/// controls to read the shell's lists.
#[repr(C)]
struct ImageListVtbl {
    _query_interface: usize,
    _add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    // Add, ReplaceIcon, SetOverlayImage, Replace, AddMasked, Draw, Remove.
    _before_get_icon: [usize; 7],
    get_icon: unsafe extern "system" fn(
        *mut c_void,
        i32,
        u32,
        *mut windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    ) -> windows_sys::core::HRESULT,
}

/// IID_IImageList, {46EB5926-582E-4017-9FDF-E8998DAA0950}.
const IID_IMAGE_LIST: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x46EB_5926,
    data2: 0x582E,
    data3: 0x4017,
    data4: [0x9F, 0xDF, 0xE8, 0x99, 0x8D, 0xAA, 0x09, 0x50],
};

/// Icon `index` of the system image list `list` (SHIL_*), as (side, RGBA).
fn image_list_icon(list: u32, index: i32) -> Option<(u32, Vec<u8>)> {
    use windows_sys::Win32::UI::Controls::ILD_TRANSPARENT;
    use windows_sys::Win32::UI::Shell::SHGetImageList;
    // SAFETY: SHGetImageList hands out an IImageList with a reference for us, released below; the
    // vtable is read only as far as GetIcon, whose icon hicon_rgba takes and destroys.
    unsafe {
        let mut list_ptr: *mut c_void = std::ptr::null_mut();
        if SHGetImageList(list as i32, &IID_IMAGE_LIST, &mut list_ptr) < 0 || list_ptr.is_null() {
            return None;
        }
        let vtbl = &**(list_ptr as *const *const ImageListVtbl);
        let mut icon = std::ptr::null_mut();
        let hr = (vtbl.get_icon)(list_ptr, index, ILD_TRANSPARENT, &mut icon);
        (vtbl.release)(list_ptr);
        if hr < 0 || icon.is_null() {
            return None;
        }
        hicon_rgba(icon)
    }
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

/// `icon`'s pixels as (side, straight RGBA), at the size its bitmap has; the icon is destroyed.
/// An icon without alpha (an old one) takes its transparency from its mask.
unsafe fn hicon_rgba(
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<(u32, Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::{BITMAP, GetObjectW};
    unsafe {
        let mut ii: ICONINFO = zeroed();
        let got = GetIconInfo(icon, &mut ii) != 0;
        let mut out = None;
        if got && !ii.hbmColor.is_null() {
            let mut bm: BITMAP = zeroed();
            let have = GetObjectW(
                ii.hbmColor as _,
                size_of::<BITMAP>() as i32,
                &mut bm as *mut BITMAP as *mut c_void,
            ) != 0;
            let side = bm.bmWidth;
            if have
                && side > 0
                && side == bm.bmHeight.abs()
                && side <= 1024
                && let Some(mut px) = dib_rgba(ii.hbmColor, side)
            {
                if !px.as_chunks::<4>().0.iter().any(|c| c[3] != 0) {
                    // The mask: white (set) where the icon is transparent.
                    let mask = (!ii.hbmMask.is_null())
                        .then(|| dib_rgba(ii.hbmMask, side))
                        .flatten();
                    for (i, c) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        c[3] = match &mask {
                            Some(m) if m[i * 4] != 0 => 0,
                            _ => 255,
                        };
                    }
                }
                out = Some((side as u32, px));
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

/// The first `side` × `side` pixels of `bitmap`, top-down, as RGBA (32 bits per pixel).
unsafe fn dib_rgba(
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    side: i32,
) -> Option<Vec<u8>> {
    unsafe {
        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side,
            biHeight: -side, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut px = vec![0u8; (side * side * 4) as usize];
        let dc = GetDC(std::ptr::null_mut());
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            side as u32,
            px.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(std::ptr::null_mut(), dc);
        (lines == side).then(|| {
            for c in px.as_chunks_mut::<4>().0 {
                c.swap(0, 2); // BGRA → RGBA
            }
            px
        })
    }
}

/// Windows' own "open folder" (Download complete's, as IDM shows), as its system image list index.
fn open_folder_index() -> Option<i32> {
    use windows_sys::Win32::UI::Shell::{
        SHGSI_SYSICONINDEX, SHGetStockIconInfo, SHSTOCKICONINFO, SIID_FOLDEROPEN,
    };
    // SAFETY: `info` is zeroed with its size set; SYSICONINDEX returns no icon to free.
    unsafe {
        let mut info: SHSTOCKICONINFO = zeroed();
        info.cbSize = size_of::<SHSTOCKICONINFO>() as u32;
        (SHGetStockIconInfo(SIID_FOLDEROPEN, SHGSI_SYSICONINDEX, &mut info) == 0)
            .then_some(info.iSysImageIndex)
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

// ---- Crash reports (crash.rs, mdm-crash) -------------------------------------------------------

static CRASH_HANDLER: std::sync::OnceLock<mdm_crash::crash_handler::CrashHandler> =
    std::sync::OnceLock::new();

/// Catch native crashes (access violations, stack overflows, … : Windows' structured exceptions)
/// and have `reporter`'s monitor dump this process when one comes. False when it can't attach.
pub fn attach_crash_handler(reporter: std::sync::Arc<mdm_crash::client::Reporter>) -> bool {
    use mdm_crash::crash_handler::{CrashEventResult, CrashHandler, make_crash_event};
    // SAFETY: the closure runs in the exception's context; it only sends the dump request over
    // the already-open socket (no allocation needed by minidumper's request).
    let event = unsafe { make_crash_event(move |cc| CrashEventResult::Handled(reporter.dump(cc))) };
    match CrashHandler::attach(event) {
        Ok(h) => CRASH_HANDLER.set(h).is_ok(),
        Err(e) => {
            tracing::warn!("crash handler not attached: {e}");
            false
        }
    }
}

/// Have the monitor dump this process now (a panic, a freeze): every thread, this one asking.
/// The process goes on.
pub fn dump_now() -> bool {
    CRASH_HANDLER.get().is_some_and(|h| {
        matches!(
            h.simulate_exception(None),
            mdm_crash::crash_handler::CrashEventResult::Handled(true)
        )
    })
}

/// This thread's id as Windows (and so the dump) knows it.
pub fn thread_id() -> u32 {
    // SAFETY: a plain query.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

/// Give this process's working set back to the system while the window is hidden (#72): the
/// pages go to the standby list, still in RAM, and come back on first touch (soft faults, no
/// disk). What Task Manager's "Memory" column shows for a tray app is this working set.
pub fn trim_working_set() {
    use windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a plain call on this process's own pseudo-handle.
    let ok = unsafe { EmptyWorkingSet(GetCurrentProcess()) } != 0;
    if !ok {
        tracing::debug!("EmptyWorkingSet failed");
    }
}

/// Move `files` to the Recycle Bin, silently (no confirmation, no progress, no error box): an
/// archive and its volumes after extracting (#37), restorable from there. False when the shell
/// refused or stopped part way; the files left are kept.
pub fn to_recycle_bin(files: &[std::path::PathBuf]) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    if files.is_empty() {
        return true;
    }
    // Whole paths, each ended by a NUL, the list by another (the shell's own format).
    let mut from: Vec<u16> = Vec::new();
    for f in files {
        let whole = std::path::absolute(f).unwrap_or_else(|_| f.clone());
        from.extend(whole.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        ..Default::default()
    };
    // SAFETY: `op` points at `from`, a double-NUL-ended list alive for the call; no window.
    let r = unsafe { SHFileOperationW(&mut op) };
    r == 0 && op.fAnyOperationsAborted == 0
}

/// Thread `tid`'s processor time so far, and this process's page faults so far (soft ones
/// included: Windows counts no hard faults per process here): crash.rs's freeze notes.
pub fn thread_usage(tid: u32) -> Option<crate::crash::Usage> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
    };
    // 100 ns units to ms.
    let ms = |t: FILETIME| (((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64) / 10_000;
    // SAFETY: the handle is checked and closed; the out-parameters are locals of the right size.
    unsafe {
        let h = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, tid);
        if h.is_null() {
            return None;
        }
        let z = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (z, z, z, z);
        let ok = GetThreadTimes(h, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(h);
        if !ok {
            return None;
        }
        let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        m.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let faults = (GetProcessMemoryInfo(GetCurrentProcess(), &mut m, m.cb) != 0)
            .then_some(m.PageFaultCount as u64);
        Some(crate::crash::Usage {
            cpu_ms: ms(kernel) + ms(user),
            faults,
        })
    }
}

/// Is process `pid` still running?
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: the handle is checked and closed.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return false;
        }
        let running = WaitForSingleObject(h, 0) == WAIT_TIMEOUT;
        CloseHandle(h);
        running
    }
}

/// The command line in this windowed program (main.rs, `misterdownloadmanager-cli`'s
/// launcher started it): the console it was started from, for what it prints and reads and for
/// Ctrl+C. Input and output the launcher handed over stay as they are (a file, a pipe, the console
/// itself); only those it didn't have get the console's. With the launcher's process id, this
/// process ends when the launcher does first (killed): nothing else would wait for it.
pub fn cli_console(launcher: Option<u32>) {
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };
    use windows_sys::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    let usable = |h: HANDLE| !h.is_null() && h != INVALID_HANDLE_VALUE;
    let ids = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    // SAFETY: plain queries and settings of this process's standard handles; the names are
    // NUL-terminated, and the handles opened are kept for the process's life.
    unsafe {
        let given = ids.map(|id| GetStdHandle(id));
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            for (id, h) in ids.into_iter().zip(given) {
                if usable(h) {
                    SetStdHandle(id, h);
                } else if !usable(GetStdHandle(id)) {
                    let name = if id == STD_INPUT_HANDLE {
                        "CONIN$\0"
                    } else {
                        "CONOUT$\0"
                    };
                    let name: Vec<u16> = name.encode_utf16().collect();
                    let console = CreateFileW(
                        name.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        std::ptr::null(),
                        OPEN_EXISTING,
                        0,
                        std::ptr::null_mut(),
                    );
                    if usable(console) {
                        SetStdHandle(id, console);
                    }
                }
            }
        }
        let watched = launcher.map_or(std::ptr::null_mut(), |pid| {
            OpenProcess(PROCESS_SYNCHRONIZE, 0, pid)
        });
        if !watched.is_null() {
            // A handle isn't Send; its value is, and the handle stays open.
            let watched = watched as usize;
            std::thread::spawn(move || {
                WaitForSingleObject(watched as HANDLE, INFINITE);
                std::process::exit(1);
            });
        }
    }
}

/// End process `pid` (a frozen MDM the user chose to close from the crash report).
pub fn end_process(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    // SAFETY: the handle is checked and closed.
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let ok = TerminateProcess(h, 1) != 0;
        CloseHandle(h);
        ok
    }
}

/// The crash test's access violation (MDM_CRASH_TEST=1, `/show crash-test:crash`).
pub fn crash_now() {
    // SAFETY: none: a write through a null pointer, on purpose, to test the crash reports.
    unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u32>(), 1) };
}

/// The names of the subkeys of `key` under `root`.
fn reg_subkeys(root: windows_sys::Win32::System::Registry::HKEY, key: &str) -> Vec<String> {
    use windows_sys::Win32::System::Registry::{
        HKEY, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
    };
    let wide: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: opens a key for reading into `h`, closed below.
    if unsafe { RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_READ, &mut h) } != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        // SAFETY: the subkey's name into `buf`, whose length in characters is in `len`.
        let rc = unsafe {
            RegEnumKeyExW(
                h,
                i,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    // SAFETY: `h` was opened above.
    unsafe { RegCloseKey(h) };
    out
}

/// The browsers installed, by name, with their program: those Windows lists for "default
/// browser" (StartMenuInternet, this user's then everyone's), named by their program
/// (mdm_ipc::browsers::windows_named: Helium's chrome.exe isn't Chrome) or else by their own
/// display name; then the known programs registered only under the shell's App Paths (a zip or
/// portable install). The browser extension's offer (extension.rs) opens its page in each.
pub fn browsers() -> Vec<(String, std::path::PathBuf)> {
    use mdm_ipc::browsers::{command_program, windows_can_run_extension, windows_named};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    const CLIENTS: &str = r"Software\Clients\StartMenuInternet";
    const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";
    let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut add = |name: String, path: std::path::PathBuf| {
        let same = |p: &std::path::PathBuf| {
            p.to_string_lossy()
                .eq_ignore_ascii_case(&path.to_string_lossy())
        };
        if path.is_file() && !out.iter().any(|(n, p)| same(p) || *n == name) {
            out.push((name, path));
        }
    };
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for sub in reg_subkeys(root, CLIENTS) {
            let key = format!(r"{CLIENTS}\{sub}");
            let Some(program) = reg_string_in(root, &format!(r"{key}\shell\open\command"), "")
                .as_deref()
                .and_then(command_program)
            else {
                continue;
            };
            if !windows_can_run_extension(&program) {
                continue;
            }
            let name = windows_named(&program)
                .map(str::to_owned)
                .or_else(|| reg_string_in(root, &key, "").filter(|n| !n.trim().is_empty()));
            add(name.unwrap_or(sub), std::path::PathBuf::from(program));
        }
    }
    for (exe, _, _) in mdm_ipc::browsers::WINDOWS_PROGRAMS {
        let key = format!(r"{APP_PATHS}\{exe}");
        let Some(path) = reg_string_in(HKEY_CURRENT_USER, &key, "")
            .or_else(|| reg_string_in(HKEY_LOCAL_MACHINE, &key, ""))
        else {
            continue;
        };
        let path = path.trim().trim_matches('"').to_owned();
        if let Some(name) = windows_named(&path) {
            add(name.to_owned(), std::path::PathBuf::from(path));
        }
    }
    out
}

/// Open `url` in the browser whose program is `program` (from `browsers`).
pub fn open_in(program: &std::path::Path, url: &str) {
    let _ = std::process::Command::new(program).arg(url).spawn();
}

/// Mark-of-the-Web on a file being downloaded from `url` (`referrer`'s page): the zone stream
/// browsers write, so SmartScreen checks a program, Office opens a document in Protected View and
/// archive tools pass the mark on, as for the browser's own download. Written on the part file
/// as it is created (the engine renames it, the stream going along).
pub fn mark_downloaded(path: &std::path::Path, url: &str, referrer: Option<&str>) {
    // Only addresses from the web: never a login's password (the engine has none in `url`).
    let clean = |u: &str| u.split(['\r', '\n']).next().unwrap_or("").to_owned();
    let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if let Some(r) = referrer.filter(|r| r.starts_with("http")) {
        text.push_str(&format!("ReferrerUrl={}\r\n", clean(r)));
    }
    text.push_str(&format!("HostUrl={}\r\n", clean(url)));
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let _ = std::fs::write(stream, text);
}

/// Browser integration is registered: a browser looking for the native host (Chrome's key,
/// which Chromium browsers read; Firefox's) finds a manifest that exists, for this user or for
/// everyone (Options → General → Test browser integration).
pub fn host_registered() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    [
        r"Software\Google\Chrome\NativeMessagingHosts\com.misterdownloadmanager.host",
        r"Software\Mozilla\NativeMessagingHosts\com.misterdownloadmanager.host",
    ]
    .iter()
    .any(|key| {
        [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .any(|root| {
                reg_string_in(root, key, "")
                    .is_some_and(|p| std::path::Path::new(p.trim().trim_matches('"')).is_file())
            })
    })
}

/// Options → Sites Logins' passwords, encrypted for this Windows account (DPAPI) before the list
/// is written: another account, or a copy of the file, can't read them.
#[allow(unsafe_code)]
pub fn protect(plain: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData};
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(plain.len()).ok()?,
        pbData: plain.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: `input` points at `plain` for the call (only read); `output` is filled by the
    // system with a buffer we copy and then free with LocalFree, as CryptProtectData requires.
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    blob_bytes(ok, output)
}

/// The password `protect` encrypted, for this account.
#[allow(unsafe_code)]
pub fn unprotect(sealed: &[u8]) -> Option<String> {
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(sealed.len()).ok()?,
        pbData: sealed.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: as in `protect`.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    String::from_utf8(blob_bytes(ok, output)?).ok()
}

/// A DPAPI result's bytes, its buffer freed.
#[allow(unsafe_code)]
fn blob_bytes(
    ok: i32,
    blob: windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
) -> Option<Vec<u8>> {
    if ok == 0 || blob.pbData.is_null() {
        return None;
    }
    // SAFETY: the system gave `cbData` bytes at `pbData`, ours until LocalFree.
    let bytes = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec();
    // SAFETY: the buffer came from DPAPI, which allocates with LocalAlloc.
    unsafe { windows_sys::Win32::Foundation::LocalFree(blob.pbData.cast()) };
    Some(bytes)
}

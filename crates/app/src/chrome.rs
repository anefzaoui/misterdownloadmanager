//! The main window's chrome painted to IDM 6.42's pixels (notes/ui-spec/main-window.md,
//! notes/ui-spec/menus.md): menu bar, popup menus, toolbar, Categories tree, shell icons.
//!
//! Our client area also holds the menu bar (IDM's is non-client), so every main-window
//! spec y is shifted by `MENU_H` = 20. Main-window spec "baseline" is origin + ascent (the row
//! under the glyphs); `winui::text_font` takes the last ink row, i.e. spec baseline − 1.

use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};

use crate::icons::Icon;
use crate::scrollbar;
use crate::winui::{self, text_font, text_width_font};

/// Height of our menu bar band incl. its #f0f0f0 bottom line (IDM: non-client y −20…−1).
pub const MENU_H: f32 = 20.0;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
fn grey(v: u8) -> Color32 {
    rgb(v, v, v)
}
use crate::theme::themed;
themed! {
    pub WINDOW_BG = rgb(0x39, 0x39, 0x39), grey(0xf0);
    MENUBAR_BG = rgb(0x20, 0x20, 0x20), grey(0xff);
    /// The open menu's item. Light: #f9f9f9, the popup's own background (captured 2026-10-30).
    MENUBAR_OPEN = rgb(0x50, 0x50, 0x50), grey(0xf9);
    /// An item under the pointer, no menu open (captured 2026-10-30): dark #505050 from 1 px
    /// below the bar's top; light #f5f5f5, 1 px inside the bar's top and bottom.
    MENUBAR_HOT = rgb(0x50, 0x50, 0x50), grey(0xf5);
    MENUBAR_LINE = rgb(0xf0, 0xf0, 0xf0), grey(0xf0);
    POPUP_BG = rgb(0x2b, 0x2b, 0x2b), grey(0xf9);
    // The native menu window's frame (Windows draws menus in their own window).
    #[cfg_attr(not(windows), allow(dead_code))]
    POPUP_BORDER = rgb(0x3e, 0x3e, 0x3e), grey(0xe5);
    POPUP_SEP = rgb(0x3e, 0x3e, 0x3e), grey(0xd7);
    /// Light: #f0f0f0, rounded, captured 2026-10-30.
    POPUP_HOVER = rgb(0x35, 0x35, 0x35), grey(0xf0);
    POPUP_ITEM = rgb(0x2c, 0x2c, 0x2c), grey(0xf9);
    TEXT = rgb(0xff, 0xff, 0xff), grey(0x00);
    /// Bullets and submenu arrows (light: dark cores #1a1a1a / #282828).
    GLYPH = rgb(0xff, 0xff, 0xff), grey(0x1a);
    TEXT_DISABLED_MENU = rgb(0x6d, 0x6d, 0x6d), grey(0x9f);
    TB_DISABLED = rgb(0xa0, 0xa0, 0xa0), grey(0xa0);
    TB_ARROW = rgb(0x28, 0x28, 0x28), grey(0x28);
    CAT_FRAME = rgb(0xdc, 0xdc, 0xdc), grey(0xdc);
    TREE_BG = rgb(0x20, 0x20, 0x20), grey(0xff);
    TREE_LINES = rgb(0x6d, 0x6d, 0x6d), grey(0x6d);
}

use crate::winui::{fill, px};

// ---- shell icons ---------------------------------------------------------------------------

/// What a 16 × 16 icon shows: a file type (by extension) or a folder.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum IconKey {
    Ext(String),
    /// 32 × 32 (Download File Info).
    ExtLarge(String),
    /// Windows' 32 × 32 "open folder" (Download complete).
    FolderOpenLarge,
    /// A FamFamFam Silk icon (CC BY 2.5, assets/icons/silk/), by name.
    Silk(&'static str),
}

/// The embedded Silk icons (XP-era look, like IDM's tree icons).
fn silk(name: &str) -> Option<egui::ImageSource<'static>> {
    macro_rules! icons {
        ($($n:literal),*) => {
            match name {
                $($n => Some(egui::include_image!(concat!("../assets/icons/silk/", $n, ".png"))),)*
                _ => None,
            }
        };
    }
    icons!(
        "folder",
        "folder_go",
        "folder_page",
        "page_white_zip",
        "page_white",
        "music",
        "application_xp",
        "film",
        "tick",
        "page_white_stack",
        "arrow_down",
        "arrow_up",
        "cross",
        "folder_link",
        "folder_delete"
    )
}

/// Windows shell icons as egui textures, loaded once (None elsewhere or on failure).
#[derive(Default)]
pub struct ShellIcons {
    map: HashMap<IconKey, Option<TextureHandle>>,
}

impl ShellIcons {
    pub fn get(&mut self, ctx: &egui::Context, key: IconKey) -> Option<egui::TextureId> {
        self.map
            .entry(key.clone())
            .or_insert_with(|| {
                #[cfg(windows)]
                {
                    let rgba = match &key {
                        IconKey::Ext(e) => crate::win32::shell_icon(e),
                        IconKey::ExtLarge(e) => crate::win32::shell_icon_large(e),
                        IconKey::FolderOpenLarge => crate::win32::folder_open_icon_large(),
                        IconKey::Silk(_) => None,
                    }?;
                    let side = if matches!(key, IconKey::ExtLarge(_) | IconKey::FolderOpenLarge) {
                        32
                    } else {
                        16
                    };
                    let img = egui::ColorImage::from_rgba_unmultiplied([side, side], &rgba);
                    let name = format!("shell-icon-{:?}", std::ptr::addr_of!(rgba));
                    Some(ctx.load_texture(name, img, egui::TextureOptions::NEAREST))
                }
                #[cfg(not(windows))]
                {
                    let _ = ctx;
                    None
                }
            })
            .as_ref()
            .map(|t| t.id())
    }

    /// Paint the icon for `key` with its top-left at English point `min` (16 × 16, the large ones
    /// 32): placed where that is on screen, not mirrored. A generic page if unavailable.
    pub fn paint(&mut self, ui: &Ui, key: IconKey, min: Pos2) {
        self.paint_in(ui, ui.painter(), key, min);
    }

    /// `paint` through `painter`, so the icon keeps to its clip (a list scrolled sideways must
    /// not draw its icons over the pane beside it).
    pub fn paint_in(&mut self, ui: &Ui, painter: &egui::Painter, key: IconKey, min: Pos2) {
        let side = if matches!(key, IconKey::ExtLarge(_) | IconKey::FolderOpenLarge) {
            32.0
        } else {
            16.0
        };
        let rc = winui::rect(Rect::from_min_size(min, vec2(side, side)));
        let min = rc.min;
        if let IconKey::Silk(name) = key {
            if let Some(src) = silk(name) {
                paint_image(ui, painter, src, rc);
            }
            return;
        }
        match self.get(ui.ctx(), key) {
            Some(id) => {
                painter.image(
                    id,
                    rc,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => generic_page(painter, min),
        }
    }
}

/// An image source drawn in `rc` through `painter` (its clip), pixel-sharp.
pub fn paint_image(ui: &Ui, painter: &egui::Painter, src: egui::ImageSource<'static>, rc: Rect) {
    let img = egui::Image::new(src).texture_options(egui::TextureOptions::NEAREST);
    if let Ok(egui::load::TexturePoll::Ready { texture }) = img.load_for_size(ui.ctx(), rc.size()) {
        painter.image(
            texture.id,
            rc,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// IDM's generic page: 12 × 16 at +2, #787673 outline, #fafafa fill, folded corner.
fn generic_page(p: &egui::Painter, min: Pos2) {
    let (x, y) = (min.x + 2.0, min.y);
    fill(p, x, y, x + 11.0, y + 15.0, rgb(0x78, 0x76, 0x73));
    fill(
        p,
        x + 1.0,
        y + 1.0,
        x + 10.0,
        y + 14.0,
        rgb(0xfa, 0xfa, 0xfa),
    );
    fill(p, x + 8.0, y, x + 11.0, y + 3.0, WINDOW_BG());
    fill(p, x + 8.0, y + 1.0, x + 8.0, y + 3.0, rgb(0x78, 0x76, 0x73));
    fill(
        p,
        x + 8.0,
        y + 3.0,
        x + 11.0,
        y + 3.0,
        rgb(0x78, 0x76, 0x73),
    );
}

// ---- menu bar --------------------------------------------------------------------------------

/// Top-level menus: label, item rect x and width, text x (measured, "Ta" kerned by the font).
/// Dark mode is IDM's owner-drawn bar; light mode the standard Win32 one (text at item.x + 7,
/// width = text + 14; light-main.md).
const MENUS_DARK: [(&str, f32, f32, f32); 5] = [
    ("Tasks", 0.0, 49.0, 10.0),
    ("File", 49.0, 37.0, 58.0),
    ("Downloads", 86.0, 84.0, 98.0),
    ("View", 170.0, 44.0, 179.0),
    ("Help", 214.0, 43.0, 223.0),
];
const MENUS_LIGHT: [(&str, f32, f32, f32); 5] = [
    ("Tasks", 0.0, 42.0, 7.0),
    ("File", 42.0, 32.0, 49.0),
    ("Downloads", 74.0, 73.0, 81.0),
    ("View", 147.0, 39.0, 154.0),
    ("Help", 186.0, 39.0, 193.0),
];

/// The menu bar in the interface language: English keeps the measured rects; a translation is
/// laid out by the same rules (dark: text at item.x + 10, width = text + 20; light, Win32's:
/// text at item.x + 7, width = text + 14) from the translated label's width: GDI's, or the
/// width it is drawn at when wider (a script drawn in a fallback font, such as Japanese).
pub fn menus(p: &egui::Painter) -> Vec<(&'static str, f32, f32, f32)> {
    let table = if crate::theme::dark() {
        &MENUS_DARK
    } else {
        &MENUS_LIGHT
    };
    let translated: Vec<&'static str> = table
        .iter()
        .map(|(l, ..)| crate::i18n::tc("menubar", l))
        .collect();
    if translated
        .iter()
        .zip(table.iter())
        .all(|(t, (l, ..))| t == l)
    {
        return table.to_vec();
    }
    let (pad, inset) = if crate::theme::dark() {
        (20.0, 10.0)
    } else {
        (14.0, 7.0)
    };
    let mut x = 0.0;
    translated
        .into_iter()
        .map(|label| {
            let w = menu_width(p, label) + pad;
            let item = (label, x, w, x + inset);
            x += w;
            item
        })
        .collect()
}

/// Paint the menu bar; returns the index of a top item that was clicked or hovered while
/// another menu is open (menu switching).
pub fn menu_bar(ui: &mut Ui, o: Pos2, width: f32, open: Option<usize>) -> Option<usize> {
    let p = ui.painter().clone();
    fill(
        &p,
        o.x,
        o.y,
        o.x + width - 1.0,
        o.y + MENU_H - 2.0,
        MENUBAR_BG(),
    );
    if !crate::theme::dark() {
        // Light: the bar's last row is #f2f2f2.
        fill(
            &p,
            o.x,
            o.y + MENU_H - 2.0,
            o.x + width - 1.0,
            o.y + MENU_H - 2.0,
            grey(0xf2),
        );
    }
    fill(
        &p,
        o.x,
        o.y + MENU_H - 1.0,
        o.x + width - 1.0,
        o.y + MENU_H - 1.0,
        MENUBAR_LINE(),
    );
    let mut hit = None;
    for (i, (label, x, w, tx)) in menus(&p).iter().enumerate() {
        // From the leading edge: the first menu at the right in RTL.
        let rc = winui::rect(Rect::from_min_size(o + vec2(*x, 0.0), vec2(*w, MENU_H)));
        let resp = ui.interact(rc, egui::Id::unique(("menubar", i)), winui::pointer_only());
        if open == Some(i) {
            p.rect_filled(rc, 0.0, MENUBAR_OPEN());
        } else if open.is_none() && resp.hovered() {
            let bottom = if crate::theme::dark() { 0.0 } else { 1.0 };
            let hot = Rect::from_min_max(rc.min + vec2(0.0, 1.0), rc.max - vec2(0.0, bottom));
            p.rect_filled(hot, 0.0, MENUBAR_HOT());
        }
        let drawn = text_font(&p, winui::segoe(), o.x + tx, o.y + 13.0, label, TEXT());
        // Dark mode always underlines mnemonics (1 px under the first letter's advance);
        // light mode follows the Windows rule: once the keyboard is in use (`cues`).
        if cues(ui.ctx()) {
            let (x0, x1) = winui::char_span(&p, &winui::segoe(), label, 0, drawn);
            fill(&p, x0, o.y + 15.0, x1, o.y + 15.0, TEXT());
        }
        if resp.clicked() || (open.is_some() && open != Some(i) && resp.hovered()) {
            hit = Some(i);
        }
    }
    hit
}

// ---- popup menus ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum MenuEntry<C> {
    /// Label (with `&` before the mnemonic letter), command, enabled, checked.
    Item(&'static str, C, bool, bool),
    /// Like `Item`, but checked shows a bullet (Win32 radio-check items).
    Radio(&'static str, C, bool, bool),
    /// Label, enabled, children.
    Sub(&'static str, bool, Vec<MenuEntry<C>>),
    Sep,
}

/// A menu label made at run time (a queue's name): each distinct text is kept once for the
/// life of the app, so menus rebuilt every frame don't allocate or leak more than that.
pub fn label(text: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static LABELS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut set = LABELS.get_or_init(Default::default).lock().unwrap();
    if let Some(l) = set.get(text) {
        return l;
    }
    let l: &'static str = Box::leak(text.to_owned().into_boxed_str());
    set.insert(l);
    l
}

const SLOT: f32 = 22.0;
const SEP: f32 = 9.0;

fn plain(label: &str) -> (String, Option<usize>) {
    let mut out = String::new();
    let mut mnemonic = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' && chars.peek().is_some() {
            mnemonic = Some(out.chars().count());
            out.push(chars.next().unwrap());
        } else {
            out.push(c);
        }
    }
    (out, mnemonic)
}

pub enum PopupResult<C> {
    Open,
    Chosen(C),
    Dismissed,
    /// Left or Right on a menu of the bar: open the menu before (-1) or after (+1) it.
    Neighbor(i32),
}

/// A key or key combination as written in the shortcuts list ("Ctrl+N", "Shift+Delete"), as
/// this system shows it: macOS's symbols (⌘N, ⇧⌦), elsewhere the translated key names.
pub fn key_label(spec: &'static str) -> String {
    if cfg!(target_os = "macos") {
        spec.replace("Ctrl+", "\u{2318}")
            .replace("Shift+", "\u{21e7}")
            .replace("Alt+", "\u{2325}")
            .replace("Delete", "\u{2326}")
            .replace("Enter", "\u{21a9}")
    } else {
        crate::i18n::tc("key", spec).to_owned()
    }
}

/// A menu label without the shortcut IDM writes into some labels ("Find (Ctrl-F)"), when the
/// menu shows the shortcut in its own column.
fn without_shortcut(text: &str) -> &str {
    let t = text.trim_end();
    if let Some(open) = t.rfind(" (")
        && t.ends_with(')')
    {
        let inner = &t[open + 2..t.len() - 1];
        let keyish = inner.len() <= 12
            && !inner.contains(' ')
            && (inner.contains('-') || inner.contains('+') || inner.starts_with('F'));
        if keyish {
            return &t[..open];
        }
    }
    t
}

/// Open a menu from the keyboard: its first item is highlighted.
pub fn start_keyboard(ctx: &egui::Context, id: &str) {
    show_cues(ctx);
    let kb = egui::Id::unique(("popup-sub", id)).with("kb");
    ctx.data_mut(|d| d.insert_temp::<(Option<usize>, Option<usize>)>(kb, (Some(usize::MAX), None)));
}

/// Keyboard cues on: the user reached for the keyboard (Alt, F10, a menu opened by keys), so
/// light mode underlines access letters from now on, the Windows rule (dark mode always does).
pub fn show_cues(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("keyboard-cues"), true));
}

/// Whether access letters are underlined: always in dark mode, after [`show_cues`] in light.
pub fn cues(ctx: &egui::Context) -> bool {
    crate::theme::dark()
        || ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::unique("keyboard-cues"))
                .unwrap_or(false)
        })
}

/// The next item of `level` from `from` in direction `dir`, skipping separators (wraps).
fn step<C>(level: &[MenuEntry<C>], from: Option<usize>, dir: i32) -> Option<usize> {
    let n = level.len() as i32;
    if n == 0 {
        return None;
    }
    let mut i = match from {
        Some(f) if f < level.len() => f as i32,
        _ if dir > 0 => -1,
        _ => n,
    };
    for _ in 0..n {
        i = (i + dir).rem_euclid(n);
        if !matches!(level[i as usize], MenuEntry::Sep) {
            return Some(i as usize);
        }
    }
    None
}

fn entry_label<C>(e: &MenuEntry<C>) -> Option<(&'static str, bool)> {
    match e {
        MenuEntry::Item(l, _, en, _) | MenuEntry::Radio(l, _, en, _) | MenuEntry::Sub(l, en, _) => {
            Some((l, *en))
        }
        MenuEntry::Sep => None,
    }
}

/// The keys an open menu takes from `i` (without Ctrl or Command: those are shortcuts).
fn menu_keys(i: &egui::InputState) -> Vec<egui::Key> {
    i.events
        .iter()
        .filter_map(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if !modifiers.command && !modifiers.ctrl => Some(*key),
            _ => None,
        })
        .collect()
}

/// Keys for an open menu (pressed in the main window or in the menu's own windows): Up and Down
/// move the highlight (Home / End to the first / last), Right opens a submenu or the next menu, Left closes a submenu or opens
/// the previous menu, Enter or an item's underlined letter runs it, Escape closes a submenu.
fn keyboard<C: Clone>(
    ctx: &egui::Context,
    entries: &[MenuEntry<C>],
    sub_key: egui::Id,
) -> Option<PopupResult<C>> {
    let kb_key = sub_key.with("kb");
    let mut keys = ctx.input(menu_keys);
    // Pressed in the menu's own windows: popup_level noted them as those windows drew.
    keys.extend(
        ctx.data_mut(|d| d.remove_temp::<Vec<egui::Key>>(sub_key.with("window-keys")))
            .unwrap_or_default(),
    );
    let (mut hl0, mut hl1): (Option<usize>, Option<usize>) =
        ctx.data(|d| d.get_temp(kb_key)).unwrap_or_default();
    if hl0 == Some(usize::MAX) {
        hl0 = step(entries, None, 1);
        ctx.data_mut(|d| d.insert_temp(kb_key, (hl0, hl1)));
    }
    if keys.is_empty() {
        return None;
    }
    // The keys are the menu's: the list underneath doesn't move too.
    ctx.input_mut(|i| {
        for k in &keys {
            i.consume_key(egui::Modifiers::NONE, *k);
        }
    });
    let mut open_sub = ctx.data(|d| d.get_temp::<usize>(sub_key));
    let mut out = None;
    for key in keys {
        let children: Option<&[MenuEntry<C>]> = open_sub.and_then(|i| match entries.get(i) {
            Some(MenuEntry::Sub(_, _, c)) => Some(c.as_slice()),
            _ => None,
        });
        let in_sub = children.is_some() && hl1.is_some();
        let level = if in_sub {
            children.unwrap_or(&[])
        } else {
            entries
        };
        let hl = if in_sub { hl1 } else { hl0 };
        let mut target = None;
        match key {
            egui::Key::ArrowDown | egui::Key::ArrowUp => {
                let n = step(level, hl, if key == egui::Key::ArrowDown { 1 } else { -1 });
                if in_sub { hl1 = n } else { hl0 = n }
            }
            // Home / End: the first / last item, as Windows' menus have them.
            egui::Key::Home | egui::Key::End => {
                let n = step(level, None, if key == egui::Key::Home { 1 } else { -1 });
                if in_sub { hl1 = n } else { hl0 = n }
            }
            egui::Key::Escape if in_sub => {
                open_sub = None;
                hl1 = None;
            }
            egui::Key::Escape => out = Some(PopupResult::Dismissed),
            egui::Key::ArrowLeft if in_sub => {
                open_sub = None;
                hl1 = None;
            }
            egui::Key::ArrowLeft => out = Some(PopupResult::Neighbor(-1)),
            egui::Key::ArrowRight => match hl.and_then(|h| level.get(h)) {
                Some(MenuEntry::Sub(_, true, c)) if !in_sub => {
                    open_sub = hl;
                    hl1 = step(c, None, 1);
                }
                _ => out = Some(PopupResult::Neighbor(1)),
            },
            egui::Key::Enter | egui::Key::Space => target = hl,
            other => {
                let name = other.name().to_lowercase();
                if name.chars().count() == 1 {
                    target = level.iter().position(|e| {
                        entry_label(e).is_some_and(|(l, en)| {
                            let (t, m) = plain(crate::i18n::tc("menu", l));
                            en && m
                                .and_then(|m| t.chars().nth(m))
                                .is_some_and(|c| c.to_lowercase().to_string() == name)
                        })
                    });
                }
            }
        }
        if let Some(t) = target {
            if in_sub {
                hl1 = Some(t)
            } else {
                hl0 = Some(t)
            }
            match level.get(t) {
                Some(MenuEntry::Item(_, c, true, _) | MenuEntry::Radio(_, c, true, _)) => {
                    out = Some(PopupResult::Chosen(c.clone()));
                }
                Some(MenuEntry::Sub(_, true, c)) if !in_sub => {
                    open_sub = Some(t);
                    hl1 = step(c, None, 1);
                }
                _ => {}
            }
        }
        if out.is_some() {
            break;
        }
    }
    ctx.data_mut(|d| {
        d.insert_temp(kb_key, (hl0, hl1));
        match open_sub {
            Some(i) => {
                d.insert_temp(sub_key, i);
            }
            None => {
                d.remove::<usize>(sub_key);
            }
        }
    });
    out
}

/// Show a popup menu with its top-left at `pos` (main-window client coordinates, on screen) —
/// its top-right in a right-to-left interface, where menus open to the left.
///
/// Like a Win32 menu, each level is its own borderless top-level window (an immediate egui
/// viewport): it can extend past the main window, flips to stay on the monitor, never takes
/// activation (so Escape still reaches the main window), and clicks on it never reach the
/// widgets underneath. Call every frame while open, from the main window's `ui`.
pub fn popup<C: Clone>(
    ctx: &egui::Context,
    id: &str,
    pos: Pos2,
    entries: &[MenuEntry<C>],
    hint: &dyn Fn(&C) -> Option<&'static str>,
) -> PopupResult<C> {
    let sub_key = egui::Id::unique(("popup-sub", id));
    if let Some(r) = keyboard(ctx, entries, sub_key) {
        ctx.data_mut(|d| {
            d.remove::<usize>(sub_key);
            d.remove::<(Option<usize>, Option<usize>)>(sub_key.with("kb"));
        });
        return r;
    }
    let origin = ctx
        .input(|i| i.viewport().inner_rect)
        .map_or(Pos2::ZERO, |r| r.min);
    let mut result = None;
    // Set by the menu's windows as they draw (popup_level) when one of them has the focus.
    let window_focus = sub_key.with("window-focus");
    ctx.data_mut(|d| d.insert_temp(window_focus, false));
    popup_level(
        ctx,
        id,
        origin + pos.to_vec2(),
        None,
        entries,
        sub_key,
        &mut result,
        0,
        hint,
    );
    if let Some(c) = result {
        ctx.data_mut(|d| {
            d.remove::<usize>(sub_key);
            d.remove::<(Option<usize>, Option<usize>)>(sub_key.with("kb"));
        });
        return PopupResult::Chosen(c);
    }
    // The menus are other windows: any press that reaches the main window, Escape, or the
    // app losing focus closes them (as Win32 menus do) — except on the frame the menu opened,
    // whose press may be the very click that opened it.
    let pass = ctx.cumulative_pass_nr();
    let shown = sub_key.with("shown");
    // Focus in the menu's own windows is still ours: on macOS and Linux a click in a menu window
    // gives that window the focus, and must not count as clicking away.
    let focused = ctx.input(|i| i.viewport().focused) == Some(true)
        || ctx.data(|d| d.get_temp::<bool>(window_focus).unwrap_or(false));
    // (first pass, last pass, focused since): focus is only *lost* if it was had.
    let (first, lost_focus) = ctx.data_mut(|d| {
        let (first, last, had) = d
            .get_temp::<(u64, u64, bool)>(shown)
            .unwrap_or((pass, pass, focused));
        let first = if last + 1 < pass { pass } else { first };
        d.insert_temp(shown, (first, pass, had || focused));
        (first == pass, had && !focused)
    });
    if !first
        && (lost_focus
            || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.any_pressed()))
    {
        ctx.data_mut(|d| {
            d.remove::<usize>(sub_key);
            d.remove::<(Option<usize>, Option<usize>)>(sub_key.with("kb"));
        });
        return PopupResult::Dismissed;
    }
    PopupResult::Open
}

/// A menu label's width as Win32 sizes menus: GDI's whole-pixel width (egui's advances differ
/// by up to a pixel either way: IDM's menus are 236, 291 wide where egui gives 237, 290), or the
/// width it is drawn at when that is wider by more (a script drawn in a fallback font, such as
/// Japanese).
fn menu_width(p: &egui::Painter, label: &str) -> f32 {
    let drawn = text_width_font(p, winui::segoe(), label).ceil();
    #[cfg(windows)]
    {
        let gdi = crate::win32::menu_text_width(label) as f32;
        if drawn > gdi + 1.0 { drawn } else { gdi }
    }
    #[cfg(not(windows))]
    {
        drawn
    }
}

/// Size of a menu window: widest label + 98, slots of 22 (separators 9), 3 px top and bottom.
fn menu_size<C>(
    ctx: &egui::Context,
    entries: &[MenuEntry<C>],
    hint: &dyn Fn(&C) -> Option<&'static str>,
) -> egui::Vec2 {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let width = |t: &str| menu_width(&painter, t);
    let widest = entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item(l, c, ..) | MenuEntry::Radio(l, c, ..) => Some(match hint(c) {
                Some(k) => {
                    width(without_shortcut(&plain(crate::i18n::tc("menu", l)).0))
                        + 28.0
                        + text_width_font(&painter, winui::segoe(), &key_label(k)).ceil()
                }
                None => width(&plain(crate::i18n::tc("menu", l)).0),
            }),
            MenuEntry::Sub(l, ..) => Some(width(&plain(crate::i18n::tc("menu", l)).0)),
            MenuEntry::Sep => None,
        })
        .fold(0.0, f32::max);
    let h: f32 = entries
        .iter()
        .map(|e| {
            if matches!(e, MenuEntry::Sep) {
                SEP
            } else {
                SLOT
            }
        })
        .sum();
    vec2(widest + 98.0, h + 6.0)
}

/// Place a menu of `size` at screen point `at`, flipped to stay inside the monitor's work area:
/// up (or left) of the point, or for a submenu (`parent`: the parent menu's screen rect) to the
/// parent's left side. In a right-to-left interface `at` is the menu's top-right: it opens to
/// the left, and flips to the right (a submenu to the parent's right side).
fn place_menu(at: Pos2, size: egui::Vec2, parent: Option<Rect>) -> Pos2 {
    #[cfg(windows)]
    if let Some([l, t, r, b]) = crate::win32::work_area(at.x as i32, at.y as i32) {
        let (l, t, r, b) = (l as f32, t as f32, r as f32, b as f32);
        let mut p = at;
        if crate::i18n::rtl() {
            p.x = at.x - size.x;
            if p.x < l {
                p.x = match parent {
                    Some(pr) => pr.right() - 4.0,
                    None => at.x,
                };
            }
        } else if p.x + size.x > r {
            p.x = match parent {
                Some(pr) => pr.left() - size.x + 4.0,
                None => at.x - size.x,
            };
        }
        if p.y + size.y > b {
            p.y = match parent {
                Some(_) => b - size.y,
                None => at.y - size.y,
            };
        }
        if crate::i18n::rtl() {
            p.x = p.x.min(r - size.x);
        }
        return pos2(p.x.max(l), p.y.max(t));
    }
    let _ = parent;
    if crate::i18n::rtl() {
        return pos2(at.x - size.x, at.y);
    }
    at
}

#[allow(clippy::too_many_arguments)]
fn popup_level<C: Clone>(
    ctx: &egui::Context,
    id: &str,
    at: Pos2,
    parent: Option<Rect>,
    entries: &[MenuEntry<C>],
    sub_key: egui::Id,
    result: &mut Option<C>,
    depth: usize,
    hint: &dyn Fn(&C) -> Option<&'static str>,
) {
    let size = menu_size(ctx, entries, hint);
    let kb_key = sub_key.with("kb");
    let kb: Option<usize> = ctx
        .data(|d| d.get_temp::<(Option<usize>, Option<usize>)>(kb_key))
        .and_then(|(a, b)| if depth == 0 { a } else { b });
    let screen = Rect::from_min_size(place_menu(at, size, parent), size);
    let title = format!("misterdownloadmanager menu {depth}"); // not translated: an id
    let builder = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_active(false)
        // Hidden until styled (no open animation, no activation), then shown below.
        .with_visible(!cfg!(windows))
        .with_position(screen.min)
        .with_inner_size(size);
    let open_sub = if depth == 0 {
        ctx.data(|d| d.get_temp::<usize>(sub_key))
    } else {
        None
    };
    // (index, slot top) of the submenu to show beside this menu.
    let mut sub_at: Option<(usize, f32)> = None;
    let vid = egui::ViewportId::from_hash_of(("menu", id, depth));
    let parent_vid = ctx.viewport_id();
    ctx.show_viewport_immediate(vid, builder, |ui, _| {
        // What happens in this window, for popup() and keyboard(), which run in the parent's
        // pass: they must not ask egui about a menu window by its id, which creates one when it
        // isn't open (a submenu, mostly): an untitled copy of the main window, opened and closed
        // every frame (issues #4 and #5: fading windows; on a full-screen Mac, a new space).
        let (focused, keys) = ui.input(|i| (i.viewport().focused == Some(true), menu_keys(i)));
        ui.ctx().data_mut(|d| {
            if focused {
                d.insert_temp(sub_key.with("window-focus"), true);
            }
            if !keys.is_empty() {
                d.get_temp_mut_or_default::<Vec<egui::Key>>(sub_key.with("window-keys"))
                    .extend(keys.iter().copied());
            }
        });
        if !keys.is_empty() {
            // keyboard() takes them on the parent's next pass.
            ui.ctx().request_repaint_of(parent_vid);
        }
        let w = size.x;
        let painter = ui.painter().clone();
        let o = ui.max_rect().min;
        // Right to left: text right-aligned, check marks and bullets on the right, the submenu
        // arrow on the left pointing left.
        let _surface = winui::Surface::enter(o.x, o.x + w);
        let rtl = winui::rtl();
        // Body; Windows 11 rounds the window and draws the border and shadow (menu_window).
        fill(
            &painter,
            o.x,
            o.y,
            o.x + w - 1.0,
            o.y + size.y - 1.0,
            POPUP_BG(),
        );
        let pointer = ui.ctx().pointer_hover_pos();
        let clicked = ui.input(|i| i.pointer.primary_released());
        // The pointer moving over an item takes the highlight from the keyboard.
        let moving = ui.input(|i| i.pointer.is_moving());
        let mut y = o.y + 3.0;
        for (i, e) in entries.iter().enumerate() {
            if matches!(e, MenuEntry::Sep) {
                winui::fill_l(
                    &painter,
                    o.x + 9.0,
                    y + 4.0,
                    o.x + w - 10.0,
                    y + 4.0,
                    POPUP_SEP(),
                );
                y += SEP;
                continue;
            }
            let (label, enabled, checked) = match e {
                MenuEntry::Item(l, _, en, ch) | MenuEntry::Radio(l, _, en, ch) => (*l, *en, *ch),
                MenuEntry::Sub(l, en, _) => (*l, *en, false),
                MenuEntry::Sep => unreachable!(),
            };
            let slot = Rect::from_min_max(pos2(o.x + 6.0, y), pos2(o.x + w - 6.0, y + SLOT));
            let hovered = pointer.is_some_and(|p| slot.contains(p));
            if hovered && moving && kb.is_some() && kb != Some(i) {
                ui.ctx().data_mut(|d| {
                    let (a, b) = d
                        .get_temp::<(Option<usize>, Option<usize>)>(kb_key)
                        .unwrap_or_default();
                    d.insert_temp(
                        kb_key,
                        if depth == 0 {
                            (Some(i), b)
                        } else {
                            (a, Some(i))
                        },
                    );
                });
            }
            let lit = match kb {
                Some(k) => k == i,
                None => hovered,
            };
            if lit || open_sub == Some(i) {
                painter.rect_filled(slot, 4.0, POPUP_HOVER());
            } else {
                // Spec: an item's normal background is one level lighter than the padding.
                fill(
                    &painter,
                    slot.left(),
                    y,
                    slot.right() - 1.0,
                    y + SLOT - 1.0,
                    POPUP_ITEM(),
                );
            }
            let color = if enabled {
                TEXT()
            } else {
                TEXT_DISABLED_MENU()
            };
            // Bullets, check marks and arrows (light mode: dark cores, not pure black).
            let glyph = if enabled {
                GLYPH()
            } else {
                TEXT_DISABLED_MENU()
            };
            let (t, mn) = plain(crate::i18n::tc("menu", label));
            let keys = match e {
                MenuEntry::Item(_, c, ..) | MenuEntry::Radio(_, c, ..) => hint(c),
                _ => None,
            };
            let t = if keys.is_some() {
                without_shortcut(&t).to_owned()
            } else {
                t
            };
            let drawn = text_font(&painter, winui::segoe(), o.x + 38.0, y + 14.0, &t, color);
            // The shortcut at the trailing edge, in the menu's quieter ink.
            if let Some(k) = keys {
                winui::text_right(
                    &painter,
                    o.x + w - 24.0,
                    y + 14.0,
                    &key_label(k),
                    TEXT_DISABLED_MENU(),
                );
            }
            // Underlined mnemonics: always in dark mode, once the keyboard is in use in light.
            if let Some(m) = mn
                && cues(ctx)
            {
                let (x0, x1) = winui::char_span(&painter, &winui::segoe(), &t, m, drawn);
                fill(&painter, x0, y + 16.0, x1, y + 16.0, color);
            }
            // The check box column: x 12..21 from the leading edge (the glyphs keep their
            // shape).
            let gx = |x: f32| if rtl { x + w - 33.0 } else { x };
            if checked && matches!(e, MenuEntry::Radio(..)) {
                // Measured 4×4 bullet at rel x 15..18, slot rows 9..12 (ink over #2c).
                const BULLET: [[u8; 4]; 4] = [
                    [0x54, 0xca, 0xd7, 0x54],
                    [0xd7, 0xff, 0xff, 0xca],
                    [0xca, 0xff, 0xff, 0xd7],
                    [0x54, 0xd7, 0xca, 0x54],
                ];
                for (dy, row) in BULLET.iter().enumerate() {
                    for (dx, v) in row.iter().enumerate() {
                        let a = ((*v as f32 - 44.0) / (255.0 - 44.0) * 255.0).round() as u8;
                        let c = Color32::from_rgba_unmultiplied(glyph.r(), glyph.g(), glyph.b(), a);
                        px(&painter, gx(o.x + 15.0 + dx as f32), y + 9.0 + dy as f32, c);
                    }
                }
            } else if checked {
                painter.line(
                    vec![
                        pos2(gx(o.x + 12.5), y + 11.5),
                        pos2(gx(o.x + 15.5), y + 14.5),
                        pos2(gx(o.x + 20.5), y + 8.5),
                    ],
                    Stroke::new(1.3, glyph),
                );
            }
            match e {
                MenuEntry::Sub(..) => {
                    // At the trailing edge, pointing where the submenu opens.
                    painter.line(
                        vec![
                            pos2(winui::mx(o.x + w - 20.5), y + 7.5),
                            pos2(winui::mx(o.x + w - 16.5), y + 11.5),
                            pos2(winui::mx(o.x + w - 20.5), y + 15.5),
                        ],
                        Stroke::new(1.2, glyph),
                    );
                    if depth == 0 && enabled && hovered && (kb.is_none() || moving) {
                        ui.ctx().data_mut(|d| d.insert_temp(sub_key, i));
                    }
                    if depth == 0 && (open_sub == Some(i) || (enabled && hovered && kb.is_none())) {
                        sub_at = Some((i, y - o.y));
                    }
                }
                MenuEntry::Item(_, c, ..) | MenuEntry::Radio(_, c, ..) => {
                    // Hovering another item of the root closes an open submenu.
                    if depth == 0 && hovered && open_sub.is_some() {
                        ui.ctx().data_mut(|d| d.remove::<usize>(sub_key));
                        sub_at = None;
                    }
                    if hovered && clicked && enabled {
                        *result = Some(c.clone());
                    }
                }
                MenuEntry::Sep => {}
            }
            y += SLOT;
        }
    });
    #[cfg(windows)]
    if crate::win32::menu_window(
        &title,
        u32::from_le_bytes([
            POPUP_BORDER().r(),
            POPUP_BORDER().g(),
            POPUP_BORDER().b(),
            0,
        ]),
    ) {
        ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Visible(true));
    }
    if let Some((i, slot_y)) = sub_at
        && let MenuEntry::Sub(_, _, children) = &entries[i]
    {
        // The submenu's top-left: parent right − 6, slot top − 3 (as measured); in RTL its
        // top-right at parent left + 6.
        let x = if crate::i18n::rtl() {
            screen.min.x + 6.0
        } else {
            screen.max.x - 6.0
        };
        let at = pos2(x, screen.min.y + slot_y - 3.0);
        popup_level(
            ctx,
            id,
            at,
            Some(screen),
            children,
            sub_key,
            result,
            depth + 1,
            hint,
        );
    }
}

// ---- toolbar -------------------------------------------------------------------------------

pub enum ToolbarEvent {
    Click(usize),
    /// A drop-down button's arrow: its menu opens at this point (button left, y 87).
    Drop(usize, Pos2),
    /// Right-click on any button, enabled or not (IDM's toolbar menu; none on empty space).
    Context(Pos2),
}

/// Paint the etch (y 20, 21) and the toolbar band (y 22–91).
/// Client y where the panes (Categories, list) start: below the toolbar band (dark 72, light
/// 70 — the light band is 2 px shorter).
pub fn pane_top() -> f32 {
    if crate::theme::dark() { 72.0 } else { 70.0 }
}

pub fn toolbar(
    ui: &mut Ui,
    o: Pos2,
    width: f32,
    buttons: &[(Icon, &str, bool)],
) -> Option<ToolbarEvent> {
    let p = ui.painter().clone();
    let top = o.y + MENU_H;
    fill(&p, o.x, top, o.x + width - 1.0, top, grey(0xa0));
    fill(&p, o.x, top + 1.0, o.x + width - 1.0, top + 1.0, grey(0xff));
    // Light mode (light-main.md): Tahoma labels, 66 px buttons, a 68 px band.
    let dark = crate::theme::dark();
    let (font, base_w, band_h) = if dark {
        (winui::segoe(), 70.0f32, 70.0)
    } else {
        (winui::tahoma(), 66.0f32, 68.0)
    };
    let band = Rect::from_min_size(pos2(o.x, top + 2.0), vec2(width, band_h));
    p.rect_filled(band, 0.0, WINDOW_BG());
    let mut x = o.x;
    let mut event = None;
    let is_dropdown =
        |icon: &Icon| matches!(icon, Icon::StartQueue | Icon::StopQueue | Icon::AddTorrent);
    // A toolbar wider than the window (long translations): the longest labels give way first,
    // cut with "..." and whole on hover, so every button stays on screen.
    let natural: Vec<(f32, f32)> = buttons
        .iter()
        .map(|(icon, label, _)| {
            let ext = text_width_font(&p, font.clone(), crate::i18n::tc("toolbar", label)).round();
            (ext, if is_dropdown(icon) { 15.0 } else { 0.0 })
        })
        .collect();
    let cap = label_cap(&natural, base_w, width);
    for (i, (icon, label, enabled)) in buttons.iter().enumerate() {
        let full = crate::i18n::tc("toolbar", label);
        let dropdown = is_dropdown(icon);
        // IDM's buttons are 70 px (light 66); ours grow only when a full label needs it (user
        // decision), up to the cap that keeps the toolbar inside the window.
        let full_ext = text_width_font(&p, font.clone(), full).round();
        let bw = base_w.max((full_ext + 12.0).min(cap));
        let cut = full_ext + 12.0 > bw;
        let shown = if cut {
            winui::ellipsize(&p, &font, full, bw - 12.0)
        } else {
            full.to_owned()
        };
        let label = &shown;
        let ext = text_width_font(&p, font.clone(), label).round();
        let total = bw + if dropdown { 15.0 } else { 0.0 };
        // Laid out in English from the leading edge; each part made where it is on screen (the
        // first button at the right in RTL, a drop-down's arrow after its button).
        let rc = winui::rect(Rect::from_min_size(
            pos2(x, band.top()),
            vec2(total, band_h),
        ));
        let resp = ui.interact(rc, egui::Id::unique(("tb", i)), winui::pointer_only());
        let resp = if cut { resp.on_hover_text(full) } else { resp };
        let past_button = |q: Pos2| winui::mx(q.x) >= x + bw;
        if *enabled && resp.hovered() {
            // Hover (IDM 6.42, captured 2026-10-30): the button's own width, from the band's top,
            // square. Dark: a flat #33414d, 64 px tall. Light: Windows' hot button, #e5f3ff with a
            // 1 px #cce8ff border, 61 px tall. Beyond IDM (which lights only the button part, user
            // request 2026-10-29): a drop-down is a split button — the part under the pointer is
            // lit (pressed while the mouse is down), the other part faintly, a 1 px divider between.
            let h = if dark { 64.0 } else { 61.0 };
            let (hot_c, faint_c, down_c, edge_c) = if dark {
                (
                    rgb(0x33, 0x41, 0x4d),
                    rgb(0x37, 0x3b, 0x3f),
                    rgb(0x2a, 0x36, 0x40),
                    rgb(0x4d, 0x5f, 0x6e),
                )
            } else {
                (
                    rgb(0xe5, 0xf3, 0xff),
                    rgb(0xf3, 0xf9, 0xff),
                    rgb(0xcc, 0xe8, 0xff),
                    rgb(0xcc, 0xe8, 0xff),
                )
            };
            let on_arrow = dropdown && resp.hover_pos().is_some_and(past_button);
            let down = resp.is_pointer_button_down_on();
            let main = winui::rect(Rect::from_min_size(pos2(x, band.top()), vec2(bw, h)));
            // The arrow part starts on the button's last column: their borders share one line.
            let arrow = winui::rect(Rect::from_min_size(
                pos2(x + bw - 1.0, band.top()),
                vec2(total - bw + 1.0, h),
            ));
            let lit = |r: Rect, under: bool| {
                let c = match (under, down) {
                    (true, true) => down_c,
                    (true, false) => hot_c,
                    (false, _) => faint_c,
                };
                p.rect_filled(r, 0.0, c);
                if !dark {
                    p.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(1.0, edge_c),
                        egui::StrokeKind::Inside,
                    );
                }
            };
            lit(main, !on_arrow);
            if dropdown {
                lit(arrow, on_arrow);
                // The divider: the parts' shared edge, one line.
                winui::fill_l(
                    &p,
                    x + bw - 1.0,
                    band.top(),
                    x + bw - 1.0,
                    band.top() + h - 1.0,
                    edge_c,
                );
            }
        }
        let cx = x + bw / 2.0;
        let icon_rect = Rect::from_min_size(
            pos2(cx - 16.0, top + 9.5 + 20.0 - 20.0 + 0.0),
            vec2(32.0, 32.0),
        )
        .translate(vec2(0.0, 0.0));
        let icon_rect = Rect::from_center_size(pos2(cx, top + 25.5), icon_rect.size());
        // Placed, not mirrored: an icon keeps its look.
        icon.image(*enabled).paint_at(ui, winui::rect(icon_rect));
        // Label: x = btn.x + floor((w − ext)/2); dark: −1 for enabled normal buttons, last ink
        // row at band y 59; light: +1 for drop-downs, last ink row 58. Drop-downs 1 px lower.
        let mut lx = x + ((bw - ext) / 2.0).floor();
        if dark && *enabled && !dropdown {
            lx -= 1.0;
        }
        if !dark && dropdown {
            lx += 1.0;
        }
        let ly = top + if dark { 59.0 } else { 58.0 } + if dropdown { 1.0 } else { 0.0 };
        text_font(
            &p,
            font.clone(),
            lx,
            ly,
            label,
            if *enabled { TEXT() } else { TB_DISABLED() },
        );
        if dropdown {
            let ax = x + bw + 4.0;
            let ay = top + if dark { 32.0 } else { 31.0 };
            // The split's divider at rest (user choice 2026-10-29, "B"): a classic etched
            // toolbar separator — a shadow line and a highlight beside it — 24 px tall, centred
            // on the arrow; the hover draws the full divider instead.
            if !(*enabled && resp.hovered()) {
                let (shadow, light) = if dark {
                    (grey(0x26), grey(0x4d))
                } else {
                    (grey(0xc6), grey(0xff))
                };
                let (y0, y1) = (ay - 10.0, ay + 13.0);
                winui::fill_l(&p, x + bw - 1.0, y0, x + bw - 1.0, y1, shadow);
                winui::fill_l(&p, x + bw, y0, x + bw, y1, light);
            }
            for (row, half) in [3.0f32, 2.0, 1.0, 0.0].iter().enumerate() {
                winui::fill_l(
                    &p,
                    ax + 3.0 - half,
                    ay + row as f32,
                    ax + 3.0 + half,
                    ay + row as f32,
                    TB_ARROW(),
                );
            }
        }
        if resp.secondary_clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            event = Some(ToolbarEvent::Context(at));
        } else if resp.clicked() && *enabled {
            let on_arrow = dropdown && resp.interact_pointer_pos().is_some_and(past_button);
            // IDM: the drop-down menu's top-left is the button's left edge at client y 87 (light:
            // 2 px higher with the band [?]); in RTL its top-right at the button's right edge.
            event = Some(if on_arrow {
                ToolbarEvent::Drop(i, pos2(winui::mx(x), o.y + if dark { 87.0 } else { 85.0 }))
            } else {
                ToolbarEvent::Click(i)
            });
        }
        x += total;
    }
    event
}

/// The widest a toolbar button may grow for its label so that all of them fit in `width`:
/// unlimited when they fit whole; else the largest cap at which they do (never under `base_w`).
/// `buttons`: each label's width and its drop-down arrow's.
fn label_cap(buttons: &[(f32, f32)], base_w: f32, width: f32) -> f32 {
    let total = |cap: f32| -> f32 {
        buttons
            .iter()
            .map(|(ext, drop)| base_w.max((ext + 12.0).min(cap)) + drop)
            .sum()
    };
    if total(f32::INFINITY) <= width {
        return f32::INFINITY;
    }
    let (mut lo, mut hi) = (
        base_w,
        buttons.iter().map(|b| b.0 + 12.0).fold(base_w, f32::max),
    );
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if total(mid) <= width {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo.floor()
}

#[cfg(test)]
mod label_cap_tests {
    #[test]
    fn labels_give_way_only_when_the_toolbar_is_too_wide() {
        let b = [(40.0, 0.0), (200.0, 15.0), (300.0, 0.0)];
        // Room for all: no cap.
        assert!(super::label_cap(&b, 70.0, 1000.0).is_infinite());
        // Too wide: the cap brings it inside, and the short label keeps its button.
        let cap = super::label_cap(&b, 70.0, 450.0);
        let total: f32 = b
            .iter()
            .map(|(e, d)| 70f32.max((e + 12.0).min(cap)) + d)
            .sum();
        assert!(total <= 450.0 && cap >= 70.0, "{cap} {total}");
    }
}

// ---- Categories pane -----------------------------------------------------------------------

#[derive(Clone)]
pub struct TreeRow<'a> {
    pub level: usize,
    pub label: std::borrow::Cow<'a, str>,
    /// None: leaf; Some(open): has an expander.
    pub expander: Option<bool>,
    pub selected: bool,
    pub icon: IconKey,
    /// A small Silk icon overlaid at the icon's bottom-right (Finished ✓, queue stacks).
    pub badge: Option<&'static str>,
    /// How many downloads the node holds, shown after its label (none when empty).
    pub count: Option<usize>,
}

/// A tree node's count, after its label: "(7)".
fn count_text(n: usize) -> String {
    format!("({})", crate::i18n::integer(n as u64))
}

pub enum TreeEvent {
    Select(usize),
    Toggle(usize),
    Close,
    /// Right-click: the row whose menu opens (the selection does not move) and where.
    Context(usize, Pos2),
}

/// The Categories pane: frame x 6 .. `right` (IDM: 148, resizable by the splitter), caption, close "x", tree (rows 16 px, indent 19).
pub fn categories(
    ui: &mut Ui,
    icons: &mut ShellIcons,
    o: Pos2,
    bottom: f32,
    right: f32,
    rows: &[TreeRow<'_>],
) -> Option<TreeEvent> {
    let p = ui.painter().clone();
    let y0 = o.y + pane_top() + MENU_H; // frame top
    // Laid out in English (frame x 6 .. right); every part made where it is on screen — the pane
    // on the right in RTL, its tree indenting from the right.
    let frame = winui::rect(Rect::from_min_max(
        pos2(o.x + 6.0, y0),
        pos2(right, bottom - 5.0),
    ));
    p.rect_stroke(
        frame,
        0.0,
        Stroke::new(1.0, CAT_FRAME()),
        egui::StrokeKind::Inside,
    );
    winui::fill_l(
        &p,
        o.x + 6.0,
        y0 + 18.0,
        right - 1.0,
        y0 + 18.0,
        CAT_FRAME(),
    ); // caption underline
    // i18n: title of the side pane that lists the categories
    let caption = crate::i18n::t("Categories");
    // Short of the close box: in a narrow pane or a long translation, "..." and whole on hover.
    let shown = winui::ellipsize(&p, &winui::tahoma(), caption, right - 21.0 - (o.x + 12.0));
    let drawn = text_font(&p, winui::tahoma(), o.x + 12.0, y0 + 13.0, &shown, TEXT());
    if shown != caption {
        winui::full_on_hover(ui, drawn, "categories-caption", caption);
    }
    let mut event = None;
    let close = winui::rect(Rect::from_min_size(
        pos2(right - 19.0, y0 + 2.0),
        vec2(16.0, 15.0),
    ));
    let cr = ui
        .interact(close, egui::Id::unique("cat-close"), Sense::click())
        // i18n: tooltip of the × that closes the Categories pane
        .on_hover_text(crate::i18n::t("Close"));
    if let Some(c) = winui::subtle_fill(winui::State::of(&cr, true), crate::theme::dark()) {
        p.rect_filled(close, 2.0, c);
    }
    text_font(
        &p,
        winui::arial_bold(),
        right - 15.0,
        y0 + 13.0,
        crate::i18n::t("x"),
        TEXT(),
    );
    if cr.clicked() {
        event = Some(TreeEvent::Close);
    }
    let tree = Rect::from_min_max(pos2(o.x + 8.0, y0 + 18.0), pos2(right - 3.0, bottom - 9.0));
    p.rect_filled(winui::rect(tree), 0.0, TREE_BG());

    // Scroll bars (notes/ui-spec/scrollbars.md "Categories tree"): by rows vertically, by
    // pixels horizontally; each shows when the content overflows (Win32 rule).
    let content_h = rows.len() as f32 * 16.0;
    let content_w = rows
        .iter()
        .map(|r| {
            let text_x = if r.level == 0 { 51.0 } else { 70.0 };
            let count = r.count.map_or(0.0, |n| {
                5.0 + text_width_font(&p, winui::tahoma(), &count_text(n))
            });
            text_x - 8.0
                + text_width_font(&p, winui::tahoma(), crate::i18n::t(&r.label))
                + count
                + 5.0
        })
        .fold(0.0, f32::max)
        .ceil();
    let (mut need_v, mut need_h) = (false, false);
    for _ in 0..2 {
        need_h = content_w > tree.width() - if need_v { scrollbar::THICK } else { 0.0 };
        need_v = content_h > tree.height() - if need_h { scrollbar::THICK } else { 0.0 };
    }
    let area = Rect::from_min_max(
        tree.min,
        pos2(
            tree.right() - if need_v { scrollbar::THICK } else { 0.0 },
            tree.bottom() - if need_h { scrollbar::THICK } else { 0.0 },
        ),
    );
    let scroll_id = egui::Id::unique("tree-scroll");
    let (mut vpos, mut hpos) = ui
        .ctx()
        .data(|d| d.get_temp::<(f32, f32)>(scroll_id))
        .unwrap_or_default();
    let page_rows = (area.height() / 16.0).floor().max(1.0);
    if ui.rect_contains_pointer(winui::rect(tree)) {
        let d = ui.input(|i| i.smooth_scroll_delta);
        vpos -= d.y / 16.0;
        hpos -= d.x;
    }
    vpos = vpos.clamp(0.0, (rows.len() as f32 - page_rows).max(0.0));
    hpos = hpos.clamp(0.0, (content_w - area.width()).max(0.0)).round();
    let first_row = vpos.round();
    // Everything below is laid out from a horizontally scrolled origin.
    let o = pos2(o.x - hpos, o.y);
    let tp = p.with_clip_rect(winui::rect(area));
    let row_top = |k: usize| area.top() + 16.0 * (k as f32 - first_row);
    // Blank space (registered first, so the rows sit above it): the selected node's menu, at
    // the pointer (IDM places it beside the node; user decision: at the pointer).
    let blank = ui.interact(
        winui::rect(area),
        egui::Id::unique("tree-blank"),
        winui::pointer_only(),
    );
    if blank.secondary_clicked()
        && let Some(k) = rows.iter().position(|r| r.selected)
        && let Some(at) = blank.interact_pointer_pos()
    {
        event = Some(TreeEvent::Context(k, at));
    }

    // Connector dots (#6d6d6d, every other pixel), painted first; expander boxes cover them.
    let roots: Vec<usize> = (0..rows.len()).filter(|&k| rows[k].level == 0).collect();
    if let (Some(&first), Some(&last)) = (roots.first(), roots.last()) {
        let mut y = row_top(first);
        while y <= row_top(last) + 8.0 {
            if (y - tree.top()) as i32 % 2 == 0 {
                winui::px_l(&tp, o.x + 19.0, y, TREE_LINES());
            }
            y += 1.0;
        }
        for &k in &roots {
            let mut x = o.x + 19.0;
            while x <= o.x + 29.0 {
                winui::px_l(&tp, x, row_top(k) + 8.0, TREE_LINES());
                x += 2.0;
            }
        }
    }
    for (k, r) in rows.iter().enumerate() {
        if r.level == 1 {
            let mut x = o.x + 38.0;
            while x <= o.x + 48.0 {
                winui::px_l(&tp, x, row_top(k) + 8.0, TREE_LINES());
                x += 2.0;
            }
            let is_last = rows.get(k + 1).is_none_or(|n| n.level == 0);
            let start = row_top(k);
            let end = if is_last {
                row_top(k) + 8.0
            } else {
                row_top(k) + 15.0
            };
            let mut y = start;
            while y <= end {
                if (y - tree.top()) as i32 % 2 == 0 {
                    winui::px_l(&tp, o.x + 38.0, y, TREE_LINES());
                }
                y += 1.0;
            }
        }
    }

    for (k, r) in rows.iter().enumerate() {
        let top = row_top(k);
        // Rows outside the viewport (icons are not clipped by `tp`).
        if top < area.top() || top >= area.bottom() {
            continue;
        }
        let icon_x = o.x + if r.level == 0 { 30.0 } else { 49.0 };
        let text_x = o.x + if r.level == 0 { 51.0 } else { 70.0 };
        let ext = text_width_font(&p, winui::tahoma(), crate::i18n::t(&r.label));
        let dark = crate::theme::dark();
        if r.selected && dark {
            let sel = winui::rect(Rect::from_min_max(
                pos2(icon_x - 2.0, top),
                pos2(text_x + ext + 4.0, top + 16.0),
            ));
            tp.rect_filled(sel, 2.0, grey(0x33));
            tp.rect_stroke(
                sel,
                2.0,
                Stroke::new(1.0, grey(0x2f)),
                egui::StrokeKind::Inside,
            );
        } else if r.selected {
            // Light (tree not focused): flat #f0f0f0 behind the label only.
            winui::fill_l(
                &tp,
                text_x - 2.0,
                top,
                text_x + ext + 1.0,
                top + 15.0,
                grey(0xf0),
            );
        }
        if let Some(open) = r.expander {
            if dark {
                winui::fill_l(&tp, o.x + 11.0, top, o.x + 26.0, top + 15.0, TREE_BG());
                chevron(&tp, o.x, top, open, r.selected);
            } else {
                // Classic box; nothing opaque around it, so connectors run up to it.
                expander_box(&tp, o.x, top, open);
            }
        }
        // Icons placed, not mirrored.
        icons.paint_in(ui, &tp, r.icon.clone(), pos2(icon_x, top));
        if let Some(name) = r.badge
            && let Some(src) = silk(name)
        {
            let rc = Rect::from_min_size(pos2(icon_x + 7.0, top + 7.0), vec2(9.0, 9.0));
            paint_image(ui, &tp, src, winui::rect(rc));
        }
        text_font(
            &tp,
            winui::tahoma(),
            text_x,
            top + 11.0,
            crate::i18n::t(&r.label),
            TEXT(),
        );
        if let Some(n) = r.count {
            let count = count_text(n);
            text_font(
                &tp,
                winui::tahoma(),
                text_x + ext + 5.0,
                top + 11.0,
                &count,
                winui::MUTED(),
            );
        }

        let row = winui::rect(
            Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), 16.0)).intersect(area),
        );
        let resp = ui.interact(
            row,
            egui::Id::unique(("tree-row", k)),
            winui::pointer_only(),
        );
        if resp.clicked() {
            let on_expander = resp.interact_pointer_pos().is_some_and(|q| {
                let x = winui::mx(q.x);
                x >= o.x + 11.0 && x <= o.x + 26.0
            });
            event = Some(if on_expander && r.expander.is_some() {
                TreeEvent::Toggle(k)
            } else {
                TreeEvent::Select(k)
            });
        }
        if resp.double_clicked() && r.expander.is_some() {
            event = Some(TreeEvent::Toggle(k));
        }
        if resp.secondary_clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            event = Some(TreeEvent::Context(k, at));
        }
    }

    if need_v {
        // At the trailing edge: the left in RTL.
        let track = winui::rect(Rect::from_min_max(
            pos2(area.right(), tree.top()),
            pos2(tree.right(), area.bottom()),
        ));
        let total = rows.len() as f32;
        if scrollbar::bar(
            ui,
            scroll_id.with("v"),
            track,
            true,
            total,
            page_rows,
            1.0,
            &mut vpos,
        ) {
            vpos = vpos.round();
            ui.ctx().request_repaint();
        }
    }
    if need_h {
        let track = winui::rect(Rect::from_min_max(
            pos2(tree.left(), area.bottom()),
            pos2(area.right(), tree.bottom()),
        ));
        if scrollbar::bar(
            ui,
            scroll_id.with("h"),
            track,
            false,
            content_w,
            area.width(),
            15.0,
            &mut hpos,
        ) {
            hpos = hpos.round();
            ui.ctx().request_repaint();
        }
    }
    if need_v && need_h {
        scrollbar::corner(ui, area.right(), area.bottom());
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(scroll_id, (vpos, hpos)));
    event
}

/// Light mode's classic 9×9 +/- box at x 15..23, y top+4..top+12 (light-main.md grids).
fn expander_box(p: &egui::Painter, ox: f32, top: f32, open: bool) {
    const MINUS: [[u32; 9]; 9] = [
        [
            0xbabbbc, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191,
            0xbabbbc,
        ],
        [
            0x919191, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb,
            0x919191,
        ],
        [
            0x919191, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb,
            0x919191,
        ],
        [
            0x919191, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0xfafbfb,
            0x919191,
        ],
        [
            0x919191, 0xededec, 0x4b63a7, 0x4b63a7, 0x4b63a7, 0x4b63a7, 0x4b63a7, 0xededec,
            0x919191,
        ],
        [
            0x919191, 0xededec, 0xededec, 0xededec, 0xededec, 0xededec, 0xededec, 0xededec,
            0x919191,
        ],
        [
            0x919191, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3,
            0x919191,
        ],
        [
            0x919191, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3,
            0x919191,
        ],
        [
            0xbabbbc, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191,
            0xbabbbc,
        ],
    ];
    const PLUS: [[u32; 9]; 9] = [
        [
            0xbabbbc, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191,
            0xbabbbc,
        ],
        [
            0x919191, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc,
            0x919191,
        ],
        [
            0x919191, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc, 0x294272, 0xfcfcfc, 0xfcfcfc, 0xfcfcfc,
            0x919191,
        ],
        [
            0x919191, 0xfafbfb, 0xfafbfb, 0xfafbfb, 0x294272, 0xfafbfb, 0xfafbfb, 0xfafbfb,
            0x919191,
        ],
        [
            0x919191, 0xfafbfb, 0x4b63a7, 0x4b63a7, 0x4b63a7, 0x4b63a7, 0x4b63a7, 0xfafbfb,
            0x919191,
        ],
        [
            0x919191, 0xededec, 0xededec, 0xededec, 0x294272, 0xededec, 0xededec, 0xededec,
            0x919191,
        ],
        [
            0x919191, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0x294272, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3,
            0x919191,
        ],
        [
            0x919191, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3, 0xe3e3e3,
            0x919191,
        ],
        [
            0xbabbbc, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191, 0x919191,
            0xbabbbc,
        ],
    ];
    let grid = if open { &MINUS } else { &PLUS };
    for (dy, row) in grid.iter().enumerate() {
        for (dx, v) in row.iter().enumerate() {
            let c = rgb((v >> 16) as u8, (v >> 8) as u8, *v as u8);
            winui::px_l(p, ox + 15.0 + dx as f32, top + 4.0 + dy as f32, c);
        }
    }
}

/// Explorer dark-theme chevrons, pixel for pixel (spec "Expander chevrons").
fn chevron(p: &egui::Painter, ox: f32, top: f32, open: bool, bright: bool) {
    const COLLAPSED: [[u8; 6]; 9] = [
        [0xc9, 0x95, 0x23, 0x20, 0x20, 0x20],
        [0x8d, 0xd9, 0x97, 0x23, 0x20, 0x20],
        [0x21, 0x8d, 0xd9, 0x97, 0x23, 0x20],
        [0x20, 0x21, 0x8d, 0xd9, 0x97, 0x22],
        [0x20, 0x20, 0x22, 0xbb, 0xd9, 0x4a],
        [0x20, 0x21, 0x8d, 0xd9, 0x8d, 0x21],
        [0x21, 0x8d, 0xd9, 0x8d, 0x21, 0x20],
        [0x8d, 0xd9, 0x8d, 0x21, 0x20, 0x20],
        [0xc9, 0x8d, 0x21, 0x20, 0x20, 0x20],
    ];
    const EXPANDED: [[u8; 9]; 6] = [
        [0xec, 0xad, 0x23, 0x20, 0x20, 0x20, 0x21, 0xa3, 0xec],
        [0xa3, 0xff, 0xb0, 0x23, 0x20, 0x21, 0xa4, 0xff, 0xa3],
        [0x21, 0xa4, 0xff, 0xa8, 0x22, 0xa4, 0xff, 0xa4, 0x21],
        [0x20, 0x21, 0xa4, 0xff, 0xdb, 0xff, 0xa4, 0x21, 0x20],
        [0x20, 0x20, 0x21, 0xa4, 0xff, 0xa4, 0x21, 0x20, 0x20],
        [0x20, 0x20, 0x20, 0x21, 0x53, 0x21, 0x20, 0x20, 0x20],
    ];
    let _ = bright;
    if open {
        for (dy, row) in EXPANDED.iter().enumerate() {
            for (dx, v) in row.iter().enumerate() {
                if *v != 0x20 {
                    winui::px_l(p, ox + 14.0 + dx as f32, top + 5.0 + dy as f32, grey(*v));
                }
            }
        }
    } else {
        for (dy, row) in COLLAPSED.iter().enumerate() {
            for (dx, v) in row.iter().enumerate() {
                if *v != 0x20 {
                    winui::px_l(p, ox + 16.0 + dx as f32, top + 4.0 + dy as f32, grey(*v));
                }
            }
        }
    }
}

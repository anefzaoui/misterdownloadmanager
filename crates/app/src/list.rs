//! The download list, painted to IDM 6.42's pixels (notes/ui-spec/main-window.md "List view"),
//! with IDM's header behaviour: drag a header to reorder columns, drag a border to resize, click
//! to sort, right-click to choose columns.

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Ui, pos2};

use crate::chrome::{IconKey, ShellIcons};
use crate::scrollbar;
use crate::selection::Pick;
use crate::theme::themed;
// The list is laid out in English pixels; `fill` makes each one where it is on screen (the
// first column at the right in a right-to-left interface).
use crate::winui::{self, fill_l as fill, text_font};

pub const HEADER_H: f32 = 24.0;
pub const ROW_H: f32 = 17.0;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
const FRAME: Color32 = rgb(0x82, 0x87, 0x90);
themed! {
    /// Inner ring and row background.
    BG = rgb(0x39, 0x39, 0x39), rgb(0xff, 0xff, 0xff);
    HEADER_BG = rgb(0x19, 0x19, 0x19), rgb(0xff, 0xff, 0xff);
    HEADER_SEP = rgb(0x63, 0x63, 0x63), rgb(0xe5, 0xe5, 0xe5);
    /// Sort indicator (an addition: IDM's header shows none).
    SORTED_BG = rgb(0x26, 0x26, 0x26), rgb(0xf0, 0xf0, 0xf0);
    SORT_ARROW = rgb(0xc8, 0xc8, 0xc8), rgb(0x60, 0x60, 0x60);
    /// Light: the one grid colour, 1 px lines (light-main.md).
    GRID_H = rgb(0x55, 0x55, 0x55), rgb(0xf0, 0xf0, 0xf0);
    GRID_V_LEFT = rgb(0x57, 0x57, 0x57), rgb(0xf0, 0xf0, 0xf0);
    GRID_V_RIGHT = rgb(0x55, 0x55, 0x55), rgb(0xf0, 0xf0, 0xf0);
    /// Selected row (list focused). Dark: #2d4b63 fill, 1 px black outline, #466279 right edge
    /// (measured). Light: classic #0066cc with white text and a dotted focus outline in its
    /// inverse, #ff9933 (captured 2026-10-30).
    SELECTED = rgb(0x2d, 0x4b, 0x63), rgb(0x00, 0x66, 0xcc);
    SELECTED_EDGE = rgb(0x00, 0x00, 0x00), rgb(0x00, 0x66, 0xcc);
    SELECTED_RIGHT = rgb(0x46, 0x62, 0x79), rgb(0x00, 0x66, 0xcc);
    TEXT = rgb(0xff, 0xff, 0xff), rgb(0x00, 0x00, 0x00);
    /// A filtered column's funnel.
    FILTER_ON = rgb(0x4c, 0xc2, 0xff), rgb(0x00, 0x78, 0xd7);
}

/// Columns, by their fixed data slot in [`Row::cells`]: IDM's eight, then the user's additions:
/// "Added" (2026-10-30: when the download was added; sorts in order of addition) and "Source"
/// (D-014: a Web or Torrent chip, first by default).
pub const COLS: usize = 10;
pub const TITLES: [&str; COLS] = [
    "File Name",
    // i18n: column title: one letter for Queue (the column is narrow)
    "Q",
    // i18n: noun: the file's size
    "Size",
    // i18n: noun: the download's state or progress
    "Status",
    "Time left",
    "Transfer rate",
    "Last Try Date",
    // i18n: noun: the download's description
    "Description",
    // i18n: past participle: when the download was added
    "Added",
    // i18n: noun: where the download comes from (Web or Torrent)
    "Source",
];
/// The "Added" column's data slot.
pub const ADDED: usize = 8;
/// The "Source" column's data slot: its cell is "Web" or "Torrent", painted as a chip.
pub const SOURCE: usize = 9;

/// IDM's default column widths (and ours for Added and Source).
pub const DEFAULT_WIDTHS: [f32; COLS] = [
    150.0, 20.0, 75.0, 65.0, 75.0, 95.0, 65.0, 200.0, 125.0, 62.0,
];
/// Default display order: Source first, then IDM's, with Added after Last Try Date.
const DEFAULT_ORDER: [usize; COLS] = [SOURCE, 0, 1, 2, 3, 4, 5, 6, ADDED, 7];

/// The chips' colours (white text on both).
const WEB_CHIP: Color32 = rgb(0x2e, 0x9d, 0x52);
const TORRENT_CHIP: Color32 = rgb(0x7e, 0x4f, 0xd0);
const MIN_WIDTH: f32 = 16.0;
/// How far from a separator a press grabs it for resizing.
const GRIP: f32 = 4.0;
/// Pointer travel before a header press becomes a column drag.
const DRAG_START: f32 = 5.0;

/// A column filter (beyond IDM, user 2026-10-29): a few fixed values to keep, or text the cell
/// must contain (case-insensitive).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    Values(Vec<&'static str>),
    Text(String),
}

/// The values a column filters by, for columns that have a few (None: a text search).
pub fn filter_values(col: usize) -> Option<&'static [&'static str]> {
    match col {
        // i18n: where a download comes from: Web (web or FTP server) or Torrent (BitTorrent)
        SOURCE => Some(&["Web", "Torrent"]),
        1 => Some(&["In a queue", "Not in a queue"]),
        // i18n: a download's status (state it is in, not a command)
        3 => Some(&["Downloading", "Stopped", "Waiting", "Complete", "Error"]),
        _ => None,
    }
}

/// The value a row has for a value-list column.
fn row_value(row: &Row, col: usize) -> &str {
    match col {
        1 if row.queued => "In a queue",
        1 => "Not in a queue",
        3 => row.status_group,
        c => &row.cells[c],
    }
}

/// Does the row pass every column filter?
pub fn matches(st: &ListState, row: &Row) -> bool {
    st.filters.iter().enumerate().all(|(c, f)| match f {
        None => true,
        Some(Filter::Values(keep)) => keep.contains(&row_value(row, c)),
        Some(Filter::Text(t)) => row.cells[c].to_lowercase().contains(&t.to_lowercase()),
    })
}

#[derive(Clone, Copy)]
enum HeaderDrag {
    /// Resizing column `col` (data slot) from `width` at pointer `x0`.
    Resize {
        col: usize,
        width: f32,
        x0: f32,
    },
    /// A press on column `col` at `x0`; becomes a move once it travels.
    Press {
        col: usize,
        x0: f32,
    },
    Move {
        col: usize,
        grab: f32,
    },
}

pub struct ListState {
    /// Display order: data slots, left to right.
    pub order: Vec<usize>,
    pub widths: [f32; COLS],
    pub visible: [bool; COLS],
    /// Sorted column (data slot) and ascending?
    pub sort: Option<(usize, bool)>,
    /// Vertical scroll in px (shown snapped to whole rows, as a Win32 list view scrolls).
    pub scroll: f32,
    /// Horizontal scroll in px.
    pub hscroll: f32,
    drag: Option<HeaderDrag>,
    /// Like a Windows list view, the selection only shows while the list has focus.
    focused: bool,
    /// Bring this row into view on the next frame (Find).
    pub scroll_to: Option<usize>,
    /// Column filters, by data slot (kept for the session).
    pub filters: Vec<Option<Filter>>,
    /// The open filter box: column and its top-left.
    filter_open: Option<(usize, Pos2)>,
    /// A selection box being dragged: where it started (x in content, y in rows' space) and
    /// whether Ctrl adds it to the selection.
    marquee: Option<(Pos2, bool)>,
    /// Type-ahead: the letters typed so far and when the last one came (seconds).
    typed: String,
    typed_at: f64,
}

impl Default for ListState {
    fn default() -> Self {
        ListState {
            order: DEFAULT_ORDER.to_vec(),
            widths: DEFAULT_WIDTHS,
            visible: [true; COLS],
            sort: None,
            scroll: 0.0,
            hscroll: 0.0,
            drag: None,
            focused: false,
            scroll_to: None,
            filters: vec![None; COLS],
            filter_open: None,
            marquee: None,
            typed: String::new(),
            typed_at: 0.0,
        }
    }
}

/// The part of the column state that is saved with the download list.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    pub order: Vec<usize>,
    /// Lists (not fixed arrays), so layouts saved before a column was added still load.
    pub widths: Vec<f32>,
    pub visible: Vec<bool>,
    pub sort: Option<(usize, bool)>,
}

impl ListState {
    pub fn layout(&self) -> Layout {
        Layout {
            order: self.order.clone(),
            widths: self.widths.to_vec(),
            visible: self.visible.to_vec(),
            sort: self.sort,
        }
    }

    /// Restore a saved layout. One saved before "Added" existed gets it after Last Try Date,
    /// one saved before "Source" gets it first; a layout that doesn't name every column
    /// exactly once is ignored.
    pub fn apply(&mut self, mut l: Layout) {
        if !l.order.contains(&SOURCE) {
            l.order.insert(0, SOURCE);
        }
        if !l.order.contains(&ADDED) {
            let at = l
                .order
                .iter()
                .position(|c| *c == 6)
                .map_or(l.order.len(), |p| p + 1);
            l.order.insert(at, ADDED);
        }
        let mut seen = l.order.clone();
        seen.sort_unstable();
        if seen != (0..COLS).collect::<Vec<_>>() {
            return;
        }
        self.order = l.order;
        for (c, default) in DEFAULT_WIDTHS.iter().enumerate() {
            self.widths[c] = l.widths.get(c).copied().unwrap_or(*default).max(MIN_WIDTH);
            self.visible[c] = l.visible.get(c).copied().unwrap_or(true);
        }
        self.visible[0] = true;
        self.sort = l.sort.filter(|(c, _)| *c < COLS);
    }

    /// Visible columns in display order.
    fn shown(&self) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|c| self.visible[*c])
            .collect()
    }
}

pub struct Row {
    /// What the Status filter sees: Downloading, Stopped, Waiting, Complete or Error.
    pub status_group: &'static str,
    pub icon: IconKey,
    /// In a queue: the Q column shows a queue icon (IDM: stacked files).
    pub queued: bool,
    pub cells: [String; COLS],
    /// The Status cell's tooltip: the download's recent events (empty: none).
    pub status_tip: String,
}

#[derive(Default)]
pub struct ListResponse {
    /// How the selection changes (a click, a key, a selection box).
    pub pick: Option<Pick>,
    /// Double-click or Enter on this row.
    pub double_clicked: Option<usize>,
    /// Alt+Enter: the focused row's Properties.
    pub properties: Option<usize>,
    /// Right-click: the row (None = blank space) and where its menu opens.
    pub context: Option<(Option<usize>, Pos2)>,
    /// The header was right-clicked here (column chooser).
    pub header_context: Option<Pos2>,
    /// The sort column or direction changed.
    pub sort_changed: bool,
}

/// A Windows list view's keys while the list has the focus (and no text box does), with its
/// multiple selection. Up / Down, Page Up / Page Down (a page of rows) and Home / End move the
/// focus: alone they select that row, with Shift they select from the anchor to it, with Ctrl
/// they only move the focus, with Ctrl+Shift they add that range. Space selects the focused row,
/// Ctrl+Space puts it in or out, Ctrl+A selects all (also before the list was clicked). Enter is
/// a double-click on the focused row, Alt+Enter opens its Properties. Typing selects the next
/// file whose name starts with the letters typed (within a second of each other; one letter
/// again and again goes through the names starting with it). Returns true for Shift+F10 / the
/// Menu key: the focused row's context menu.
fn keyboard(
    ui: &Ui,
    st: &mut ListState,
    rows: &[Row],
    focus: Option<usize>,
    client_h: f32,
    out: &mut ListResponse,
) -> bool {
    let n = rows.len();
    if n == 0 || ui.ctx().memory(|m| m.focused().is_some()) {
        return false;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
        st.focused = true;
        out.pick = Some(Pick::All);
        return false;
    }
    if !st.focused {
        return false;
    }
    let page = ((client_h - ROW_H) / ROW_H).floor().max(1.0) as isize;
    let last = n as isize - 1;
    let cur = focus.map(|s| s as isize);
    let (to, mods) = ui.input(|i| {
        let k = |key| i.key_pressed(key) && !i.modifiers.alt;
        let to = if k(egui::Key::ArrowDown) {
            Some(cur.map_or(0, |c| c + 1))
        } else if k(egui::Key::ArrowUp) {
            Some(cur.map_or(0, |c| c - 1))
        } else if k(egui::Key::PageDown) {
            Some(cur.map_or(0, |c| c + page))
        } else if k(egui::Key::PageUp) {
            Some(cur.map_or(0, |c| c - page))
        } else if k(egui::Key::Home) {
            Some(0)
        } else if k(egui::Key::End) {
            Some(last)
        } else {
            None
        };
        (to, i.modifiers)
    });
    if let Some(to) = to {
        let to = to.clamp(0, last) as usize;
        out.pick = Some(match (mods.command, mods.shift) {
            (false, false) => Pick::Only(to),
            (true, false) => Pick::Focus(to),
            (ctrl, true) => Pick::Range { to, add: ctrl },
        });
        st.scroll_to = Some(to);
        st.typed.clear();
        return false;
    }
    let (menu, enter, alt_enter, space) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Menu)
                || (i.key_pressed(egui::Key::F10) && i.modifiers.shift_only()),
            i.key_pressed(egui::Key::Enter) && i.modifiers.is_none(),
            i.key_pressed(egui::Key::Enter) && i.modifiers.alt,
            i.key_pressed(egui::Key::Space),
        )
    });
    if let Some(f) = focus {
        if enter {
            out.double_clicked = Some(f);
        } else if alt_enter {
            out.properties = Some(f);
        }
    }
    // Type-ahead (a Space while typing is part of the name).
    let now = ui.input(|i| i.time);
    if now - st.typed_at > 1.0 {
        st.typed.clear();
    }
    let typed: String = ui.input(|i| {
        if i.modifiers.command || i.modifiers.alt {
            return String::new();
        }
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    });
    let typing = !typed.is_empty() && !(typed == " " && st.typed.is_empty());
    if typing {
        st.typed.push_str(&typed.to_lowercase());
        st.typed_at = now;
        let one_letter = st.typed.chars().all(|c| st.typed.starts_with(c));
        let (prefix, from) = if one_letter {
            // The same letter again: the next name starting with it.
            (
                st.typed.chars().take(1).collect::<String>(),
                focus.map_or(0, |f| f + 1),
            )
        } else {
            (st.typed.clone(), focus.unwrap_or(0))
        };
        if let Some(i) = (0..n)
            .map(|k| (from + k) % n)
            .find(|i| rows[*i].cells[0].to_lowercase().starts_with(&prefix))
        {
            out.pick = Some(Pick::Only(i));
            st.scroll_to = Some(i);
        }
    } else if space && let Some(f) = focus {
        out.pick = Some(match (mods.command, mods.shift) {
            (true, _) => Pick::Toggle(f),
            (false, true) => Pick::Range { to: f, add: false },
            (false, false) => Pick::Only(f),
        });
    }
    menu
}

/// Paint the list inside `outer` (its 1 px #828790 border rect). `selected`: each row's
/// selection; `focus`: the focused row (the dotted outline, where keys move from).
pub fn show(
    ui: &mut Ui,
    outer: Rect,
    st: &mut ListState,
    rows: &[Row],
    selected: &[bool],
    focus: Option<usize>,
    icons: &mut ShellIcons,
) -> ListResponse {
    let mut out = ListResponse::default();
    // `outer` and everything below are English (the main window's layout): each part is made where
    // it is on screen (`screen`, `fill`, `text_font`), and the pointer is read in English to find
    // the column or row under it.
    let screen = winui::rect;
    let english = |q: Pos2| pos2(winui::mx(q.x), q.y);
    // Focus follows the last press: inside the list focuses it, anywhere else unfocuses it.
    if ui.input(|i| i.pointer.any_pressed()) {
        st.focused = ui
            .ctx()
            .pointer_interact_pos()
            .is_some_and(|p| screen(outer).contains(p));
    }
    let p = ui.painter_at(screen(outer));
    let (l, t, r, b) = (
        outer.left(),
        outer.top(),
        outer.right() - 1.0,
        outer.bottom() - 1.0,
    );
    winui::solid(&p, screen(outer), FRAME);
    fill(&p, l + 1.0, t + 1.0, r - 1.0, b - 1.0, BG());
    let (cl, ct, cr, cb) = (l + 2.0, t + 2.0, r - 2.0, b - 2.0);
    let font = winui::tahoma();

    // Scroll bars (notes/ui-spec/scrollbars.md), decided like Win32: each one shows when the
    // content overflows the room the other one leaves.
    let content_w: f32 = st.shown().iter().map(|c| st.widths[*c]).sum();
    let (client_w, client_h) = (cr - cl + 1.0, cb - ct + 1.0);
    let rows_h = rows.len() as f32 * ROW_H;
    let menu_key = keyboard(ui, st, rows, focus, client_h, &mut out);
    let (mut need_v, mut need_h) = (false, false);
    for _ in 0..2 {
        need_h = content_w > client_w - if need_v { scrollbar::THICK } else { 0.0 };
        need_v = rows_h > client_h - HEADER_H - if need_h { scrollbar::THICK } else { 0.0 };
    }
    // Inclusive right edge of the header and rows, and bottom of the rows.
    let rr = cr - if need_v { scrollbar::THICK } else { 0.0 };
    let rb = cb - if need_h { scrollbar::THICK } else { 0.0 };
    let row_w = rr - cl + 1.0;
    st.hscroll = st.hscroll.clamp(0.0, (content_w - row_w).max(0.0)).round();

    // Column spans in display order: (slot, x, right) with the separator on the last pixel.
    let spans = |st: &ListState| {
        let mut v = Vec::new();
        let mut x = cl - st.hscroll;
        for c in st.shown() {
            v.push((c, x, x + st.widths[c] - 1.0));
            x += st.widths[c];
        }
        v
    };

    // ---- header interaction ----------------------------------------------------------------
    let header = Rect::from_min_max(pos2(cl, ct), pos2(rr + 1.0, ct + HEADER_H));
    let hresp = ui.interact(
        screen(header),
        egui::Id::unique("list-header"),
        Sense::click_and_drag(),
    );
    let pointer = ui.ctx().pointer_interact_pos().map(english);
    let cols = spans(st);
    let near_sep = |x: f32| {
        cols.iter()
            .find(|(_, _, x1)| (x - (x1 + 0.5)).abs() <= GRIP)
            .map(|s| s.0)
    };
    if let Some(pp) = hresp.hover_pos().map(english)
        && near_sep(pp.x).is_some()
    {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    // Resize or move is decided where the button went down: a drag is only recognised after the
    // pointer has travelled a few pixels, by then off a border's grip (user report 2026-10-29:
    // dragging a border moved the column instead of resizing it).
    if hresp.drag_started()
        && let Some(pp) = ui
            .input(|i| i.pointer.press_origin())
            .map(english)
            .or(pointer)
    {
        st.drag = match near_sep(pp.x) {
            Some(col) => Some(HeaderDrag::Resize {
                col,
                width: st.widths[col],
                x0: pp.x,
            }),
            None => cols
                .iter()
                .find(|(_, x0, x1)| pp.x >= *x0 && pp.x <= *x1)
                .map(|(col, _, _)| HeaderDrag::Press {
                    col: *col,
                    x0: pp.x,
                }),
        };
    }
    if let (Some(drag), Some(pp)) = (st.drag, pointer) {
        match drag {
            HeaderDrag::Resize { col, width, x0 } => {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                st.widths[col] = (width + pp.x - x0).round().max(MIN_WIDTH);
            }
            HeaderDrag::Press { col, x0 } if (pp.x - x0).abs() >= DRAG_START => {
                let start = cols.iter().find(|s| s.0 == col).map_or(x0, |s| s.1);
                st.drag = Some(HeaderDrag::Move {
                    col,
                    grab: x0 - start,
                });
            }
            _ => {}
        }
    }
    if hresp.drag_stopped() {
        if let (Some(HeaderDrag::Move { col, .. }), Some(pp)) = (st.drag, pointer) {
            move_column(st, &cols, col, pp.x);
        }
        st.drag = None;
    }
    // A click on a header's funnel opens its filter box instead of sorting.
    let funnel_hit = |st: &ListState, pp: Pos2| {
        cols.iter()
            .find(|(c, x0, x1)| funnel_rect(st, *c, *x0, *x1, ct).contains(pp))
            .map(|(c, x0, x1)| (*c, funnel_rect(st, *c, *x0, *x1, ct).left_bottom(), *x1))
    };
    let funnel_click = hresp
        .clicked()
        .then_some(pointer)
        .flatten()
        .and_then(|pp| funnel_hit(st, pp));
    if let Some((col, at, _)) = funnel_click {
        st.filter_open = match st.filter_open {
            Some((c, _)) if c == col => None,
            _ => Some((col, pos2(at.x.min(outer.right() - FILTER_W), ct + HEADER_H))),
        };
    }
    if hresp.clicked()
        && funnel_click.is_none()
        && let Some(pp) = pointer
        && near_sep(pp.x).is_none()
        && let Some((col, ..)) = cols.iter().find(|(_, x0, x1)| pp.x >= *x0 && pp.x <= *x1)
    {
        // Ascending → descending → unsorted (order of addition).
        st.sort = match st.sort {
            Some((c, true)) if c == *col => Some((c, false)),
            Some((c, false)) if c == *col => None,
            _ => Some((*col, true)),
        };
        out.sort_changed = true;
    }
    if hresp.secondary_clicked() {
        out.header_context = ui.ctx().pointer_interact_pos();
    }

    // ---- header painting -------------------------------------------------------------------
    let cols = spans(st);
    let p_all = p.clone();
    let p = p.with_clip_rect(screen(header));
    fill(&p, cl, ct, rr, ct + HEADER_H - 1.0, HEADER_BG());
    for (c, x0, x1) in &cols {
        let hovered = hresp
            .hover_pos()
            .map(english)
            .is_some_and(|h| h.x >= *x0 && h.x <= *x1 && st.drag.is_none());
        // The column under the pointer, and pressed while the mouse button is held on it (as
        // Windows' headers); the shared tint (winui::subtle_fill).
        let state = match (hovered, hresp.is_pointer_button_down_on()) {
            (false, _) => winui::State::Normal,
            (true, false) => winui::State::Hot,
            (true, true) => winui::State::Pressed,
        };
        let tint = winui::subtle_fill(state, crate::theme::dark());
        // The sorted column: highlighted header and a chevron at its right edge (▲ ascending,
        // ▼ descending).
        if let Some((sc, asc)) = st.sort
            && sc == *c
        {
            fill(
                &p,
                *x0,
                ct,
                x1 - 1.0,
                ct + HEADER_H - 1.0,
                tint.unwrap_or(SORTED_BG()),
            );
            // At the cell's right edge, vertically centred in the header.
            let cx = x1 - 9.0;
            let mid = ct + (HEADER_H / 2.0).floor() - 2.0;
            for i in 0..4 {
                let y = mid + if asc { i as f32 } else { 3.0 - i as f32 };
                let half = i as f32;
                fill(&p, cx - half, y, cx + half, y, SORT_ARROW());
            }
        } else if let Some(t) = tint {
            fill(&p, *x0, ct, x1 - 1.0, ct + HEADER_H - 1.0, t);
        }
        fill(&p, *x1, ct, *x1, ct + HEADER_H - 1.0, HEADER_SEP());
        let mut arrow_room = if st.sort.is_some_and(|(sc, _)| sc == *c) {
            14.0
        } else {
            0.0
        };
        // The funnel (user, 2026-10-30): always shown, faint; darker under the pointer or while
        // its box is open; blue while the column is filtered. A column too narrow for a label
        // and the funnel (Q) shows it only under the pointer.
        let filtered = st.filters[*c].is_some();
        let open = st.filter_open.is_some_and(|(fc, _)| fc == *c);
        let roomy = x1 - x0 >= 44.0;
        if roomy || filtered || hovered || open {
            let fr = funnel_rect(st, *c, *x0, *x1, ct);
            let color = if filtered {
                FILTER_ON()
            } else if hovered || open {
                SORT_ARROW()
            } else {
                SORT_ARROW().gamma_multiply(0.45)
            };
            if !roomy {
                p.rect_filled(screen(fr), 2.0, HEADER_BG());
            }
            funnel(&p, fr, color);
            if roomy {
                arrow_room += 14.0;
            }
        }
        let label = winui::ellipsize(
            &p,
            &font,
            crate::i18n::tc("column", TITLES[*c]),
            x1 - x0 - 10.0 - arrow_room,
        );
        let clip = p.with_clip_rect(screen(Rect::from_x_y_ranges(*x0..=*x1, ct..=ct + HEADER_H)));
        text_font(&clip, font.clone(), x0 + 6.0, ct + 15.0, &label, TEXT());
    }

    // ---- body ------------------------------------------------------------------------------
    let p = p_all;
    let body_top = ct + HEADER_H;
    let body = Rect::from_min_max(pos2(cl, body_top), pos2(rr + 1.0, rb + 1.0));
    let resp = ui.interact(
        screen(body),
        egui::Id::unique("download-list"),
        Sense::click_and_drag(),
    );
    let bp = p.with_clip_rect(screen(body));
    // Fully visible rows; the list scrolls by whole rows until the last one is fully shown.
    let page_rows = (body.height() / ROW_H).floor().max(1.0);
    let max_scroll = (rows.len() as f32 - page_rows).max(0.0) * ROW_H;
    if resp.hovered() {
        let d = ui.input(|i| i.smooth_scroll_delta);
        st.scroll -= d.y;
        st.hscroll -= d.x;
    }
    if let Some(row) = st.scroll_to.take() {
        let top = row as f32 * ROW_H;
        if top < st.scroll {
            st.scroll = top;
        } else if top + ROW_H > st.scroll + page_rows * ROW_H {
            st.scroll = top + ROW_H - page_rows * ROW_H;
        }
    }
    st.scroll = st.scroll.clamp(0.0, max_scroll);
    st.hscroll = st.hscroll.clamp(0.0, (content_w - row_w).max(0.0)).round();
    let shown_scroll = (st.scroll / ROW_H).round() * ROW_H;
    let first = (shown_scroll / ROW_H).floor() as usize;
    let offset = 0.0;
    let visible = (body.height() / ROW_H).ceil() as usize + 1;
    let last_right = cols.last().map_or(cl, |s| s.2);
    let mut cut_under_pointer = None;
    for k in 0..visible {
        let idx = first + k;
        let top = body_top + k as f32 * ROW_H - offset;
        if let Some(row) = rows.get(idx) {
            let sel = selected.get(idx).copied().unwrap_or(false) && st.focused;
            let has_focus = focus == Some(idx) && st.focused;
            if (sel || has_focus) && !crate::theme::dark() {
                // Light: the row above its grid line; the focused row has a dotted outline
                // (every other pixel), in the fill's inverse on a selected row.
                let (right, bottom) = (last_right - 1.0, top + ROW_H - 2.0);
                if sel {
                    fill(&bp, cl, top, right, bottom, SELECTED());
                }
                if has_focus {
                    let dot = if sel {
                        rgb(0xff, 0x99, 0x33)
                    } else {
                        rgb(0x00, 0x00, 0x00)
                    };
                    dotted(&bp, cl, top, right, bottom, dot);
                }
            } else if has_focus && !sel {
                // Dark: a focused row that isn't selected (Ctrl+arrows, Ctrl+Space) has a dotted
                // outline only.
                let (right, bottom) = (last_right - 1.0, top + ROW_H - 1.0);
                dotted(&bp, cl, top, right, bottom, SELECTED_RIGHT());
            } else if sel {
                let right = last_right - 1.0;
                fill(&bp, cl, top, right, top + ROW_H - 1.0, SELECTED_EDGE());
                fill(
                    &bp,
                    cl + 1.0,
                    top + 1.0,
                    right - 1.0,
                    top + ROW_H - 2.0,
                    SELECTED(),
                );
                fill(
                    &bp,
                    right,
                    top + 1.0,
                    right,
                    top + ROW_H - 2.0,
                    SELECTED_RIGHT(),
                );
            }
            for (c, x0, x1) in &cols {
                // The file-type icon sits in the File Name column, wherever it is.
                let tx = if *c == 0 {
                    icons.paint_in(ui, &bp, row.icon.clone(), pos2(x0 + 4.0, top));
                    x0 + 22.0
                } else {
                    x0 + 6.0
                };
                if *c == 1 && row.queued {
                    icons.paint_in(
                        ui,
                        &bp,
                        IconKey::Silk("page_white_stack"),
                        pos2(x0 + 2.0, top),
                    );
                }
                let cell = Rect::from_min_max(pos2(*x0, top), pos2(*x1 - 1.0, top + ROW_H));
                if *c == SOURCE {
                    let cut = chip(
                        &bp.with_clip_rect(screen(cell.intersect(body))),
                        &font,
                        (*x0, *x1),
                        top,
                        &row.cells[*c],
                    );
                    let hovered = resp
                        .hover_pos()
                        .map(english)
                        .is_some_and(|h| cell.contains(h));
                    if cut && hovered {
                        cut_under_pointer = Some(crate::i18n::t(&row.cells[*c]).to_owned());
                    }
                    continue;
                }
                let value = cell_text(*c, &row.cells[*c]);
                let s = winui::ellipsize(&bp, &font, &value, x1 - tx - 3.0);
                // A cut cell shows its whole text on hover (Windows' list label tip); the
                // Status cell shows the download's recent events.
                if resp
                    .hover_pos()
                    .map(english)
                    .is_some_and(|h| cell.contains(h))
                {
                    if *c == 3 && !row.status_tip.is_empty() {
                        cut_under_pointer = Some(row.status_tip.clone());
                    } else if s != value {
                        cut_under_pointer = Some(row.cells[*c].clone());
                    }
                }
                let clip = bp.with_clip_rect(screen(cell.intersect(body)));
                let ink = if sel { Color32::WHITE } else { TEXT() };
                text_font(&clip, font.clone(), tx, top + 12.0, &s, ink);
            }
        }
        // Dark: the grid line is the first pixel row of the next item, over everything.
        // Light: the item's own last row (top + 16), so icons are not clipped.
        let gy = top
            + if crate::theme::dark() {
                ROW_H
            } else {
                ROW_H - 1.0
            };
        fill(&bp, cl, gy, rr, gy, GRID_H());
    }
    if let Some(full) = cut_under_pointer {
        resp.clone().on_hover_text(full);
    }
    filter_box(ui, st);
    for (_, _, x1) in &cols {
        if crate::theme::dark() {
            fill(&bp, x1 - 1.0, body_top, x1 - 1.0, rb, GRID_V_LEFT());
            fill(&bp, *x1, body_top, *x1, rb, GRID_V_RIGHT());
        } else {
            // Light: one line at col.right + 1.
            fill(&bp, x1 + 1.0, body_top, x1 + 1.0, rb, GRID_V_RIGHT());
        }
    }
    // Dark only: IDM draws a 2 px sliver right of the last column in every body row (spec
    // "Artifact"); light mode has none.
    for k in 0..if crate::theme::dark() { visible } else { 0 } {
        let top = body_top + k as f32 * ROW_H - offset;
        fill(
            &bp,
            last_right + 1.0,
            top + 1.0,
            last_right + 1.0,
            top + ROW_H - 1.0,
            rgb(0x97, 0x97, 0x97),
        );
        fill(
            &bp,
            last_right + 2.0,
            top + 1.0,
            last_right + 2.0,
            top + ROW_H - 1.0,
            rgb(0x29, 0x4a, 0x6c),
        );
    }

    // While a column is dragged: a translucent copy of its header follows the pointer, and a
    // 2 px mark shows where it will land.
    if let (Some(HeaderDrag::Move { col, grab }), Some(pp)) = (st.drag, pointer) {
        let w = st.widths[col];
        let ghost = Rect::from_min_size(pos2(pp.x - grab, ct), egui::vec2(w, HEADER_H));
        let top = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("col-ghost"),
        ));
        top.rect_filled(
            screen(ghost),
            0.0,
            Color32::from_rgba_unmultiplied(0x60, 0x60, 0x60, 160),
        );
        text_font(
            &top,
            font.clone(),
            ghost.left() + 6.0,
            ct + 15.0,
            crate::i18n::tc("column", TITLES[col]),
            TEXT(),
        );
        let mark = drop_x(&cols, pp.x);
        fill(
            &top,
            mark - 1.0,
            ct,
            mark,
            ct + HEADER_H - 1.0,
            rgb(0x00, 0x78, 0xd7),
        );
    }

    // ---- scroll bars -----------------------------------------------------------------------
    // Vertical: beside the header too (client top to the corner), in rows.
    if need_v {
        // At the trailing edge: the left in RTL.
        let track = screen(Rect::from_min_max(
            pos2(rr + 1.0, ct),
            pos2(cr + 1.0, rb + 1.0),
        ));
        let mut pos_rows = shown_scroll / ROW_H;
        if scrollbar::bar(
            ui,
            egui::Id::unique("list-vbar"),
            track,
            true,
            rows.len() as f32,
            page_rows,
            1.0,
            &mut pos_rows,
        ) {
            st.scroll = pos_rows.round() * ROW_H;
            ui.ctx().request_repaint();
        }
    }
    // Horizontal: under the rows, in pixels.
    if need_h {
        let track = screen(Rect::from_min_max(
            pos2(cl, rb + 1.0),
            pos2(rr + 1.0, cb + 1.0),
        ));
        let mut x = st.hscroll;
        if scrollbar::bar(
            ui,
            egui::Id::unique("list-hbar"),
            track,
            false,
            content_w,
            row_w,
            15.0,
            &mut x,
        ) {
            st.hscroll = x.round();
            ui.ctx().request_repaint();
        }
    }
    if need_v && need_h {
        scrollbar::corner(ui, rr + 1.0, rb + 1.0);
    }

    // ---- row hit testing -------------------------------------------------------------------
    let row_at = |pos: Pos2| -> Option<usize> {
        if !body.contains(pos) || pos.x > last_right {
            return None;
        }
        let idx = ((pos.y - body_top + shown_scroll) / ROW_H).floor() as usize;
        (idx < rows.len()).then_some(idx)
    };
    let mods = ui.input(|i| i.modifiers);
    if let Some(on_screen) = resp.interact_pointer_pos()
        && let pos = english(on_screen)
        && body.contains(pos)
    {
        if resp.clicked() {
            // Windows: Ctrl+click puts a row in or out, Shift+click selects from the anchor,
            // Ctrl+Shift+click adds that range; a click on blank space clears (not with Ctrl).
            out.pick = match (row_at(pos), mods.command, mods.shift) {
                (Some(i), false, false) => Some(Pick::Only(i)),
                (Some(i), true, false) => Some(Pick::Toggle(i)),
                (Some(to), ctrl, true) => Some(Pick::Range { to, add: ctrl }),
                (None, false, _) => Some(Pick::Clear),
                (None, true, _) => None,
            };
        }
        if resp.double_clicked() {
            out.double_clicked = row_at(pos);
        }
        if resp.secondary_clicked() {
            // A right-click inside the selection keeps it (the menu acts on all of it); outside,
            // the row alone is selected; on blank space the selection is cleared. The menu opens
            // at the pointer (IDM opens the blank-space one at the list's centre; user decision).
            let row = row_at(pos);
            out.pick = Some(match row {
                Some(i) if selected.get(i).copied().unwrap_or(false) => Pick::Focus(i),
                Some(i) => Pick::Only(i),
                None => Pick::Clear,
            });
            out.context = Some((row, on_screen));
        }
    }
    // Shift+F10 / the Menu key: the focused row's menu, under it.
    if menu_key {
        let at = focus.map_or(body.left_top() + egui::vec2(20.0, 4.0), |f| {
            pos2(
                cl + 20.0,
                (body_top + (f as f32 + 1.0) * ROW_H - shown_scroll)
                    .clamp(body.top(), body.bottom()),
            )
        });
        // `at` is English; the menu opens at its screen point.
        out.context = Some((focus, pos2(winui::mx(at.x), at.y)));
    }

    // A selection box, dragged from blank space (below the rows or right of the columns), as in
    // Explorer: the rows it touches are selected, Ctrl adds them; dragging past the top or bottom
    // scrolls.
    if resp.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin()).map(english)
        && body.contains(origin)
        && row_at(origin).is_none()
    {
        st.marquee = Some((
            pos2(origin.x + st.hscroll, origin.y - body_top + shown_scroll),
            mods.command,
        ));
    }
    if let Some((start, add)) = st.marquee {
        if let Some(pp) = pointer {
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            let step = 30.0 * ROW_H * dt;
            if pp.y < body.top() {
                st.scroll = (st.scroll - step).max(0.0);
                ui.ctx().request_repaint();
            } else if pp.y > body.bottom() {
                st.scroll = (st.scroll + step).min(max_scroll);
                ui.ctx().request_repaint();
            }
            let now = pos2(pp.x + st.hscroll, pp.y - body_top + st.scroll);
            let (y0, y1) = (start.y.min(now.y), start.y.max(now.y));
            let (x0, x1) = (start.x.min(now.x), start.x.max(now.x));
            let touches_columns = x0 <= last_right + st.hscroll && x1 >= cl + st.hscroll;
            let picked: Vec<usize> = if touches_columns {
                let first = (y0.max(0.0) / ROW_H).floor() as usize;
                let last = (y1.max(0.0) / ROW_H).floor() as usize;
                (first..=last.min(rows.len().saturating_sub(1)))
                    .filter(|i| *i < rows.len())
                    .collect()
            } else {
                Vec::new()
            };
            out.pick = Some(Pick::Rows { rows: picked, add });
            let screen = Rect::from_min_max(
                pos2(x0 - st.hscroll, y0 + body_top - st.scroll),
                pos2(x1 - st.hscroll, y1 + body_top - st.scroll),
            );
            let bp = p.with_clip_rect(winui::rect(body));
            let screen = winui::rect(screen);
            bp.rect_filled(
                screen,
                0.0,
                Color32::from_rgba_unmultiplied(0x33, 0x99, 0xff, 0x40),
            );
            bp.rect_stroke(
                screen,
                0.0,
                egui::Stroke::new(1.0, rgb(0x33, 0x99, 0xff)),
                egui::StrokeKind::Inside,
            );
        }
        if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            st.marquee = None;
            out.pick = Some(Pick::BoxDone);
        }
    }
    out
}

/// A cell's text as drawn in column `col`: data keeps its own direction in the interface's — a
/// file name or a description as a first-strong isolate (an Arabic name reads right to left in
/// an English window, a Latin one left to right in an Arabic window), a date left to right. The
/// cut happens inside the isolate, so the "..." ends the name.
fn cell_text(col: usize, s: &str) -> String {
    match col {
        0 | 7 => crate::i18n::isolate(s),
        6 | ADDED => crate::i18n::ltr(s),
        _ => s.to_owned(),
    }
}

/// A focus outline: every other pixel of the rectangle's edge (inclusive pixel bounds).
fn dotted(p: &egui::Painter, l: f32, t: f32, r: f32, b: f32, color: Color32) {
    let mut x = l;
    while x <= r {
        fill(p, x, t, x, t, color);
        fill(p, x, b, x, b, color);
        x += 2.0;
    }
    let mut y = t;
    while y <= b {
        fill(p, l, y, l, y, color);
        fill(p, r, y, r, y, color);
        y += 2.0;
    }
}

/// A Source chip: a rounded pill, green "Web" or purple "Torrent", white text, at the cell's
/// left of the cell `x0..x1`. A label too long for the cell (a translation in a narrow column)
/// is cut with "..."; returns whether it was.
fn chip(
    p: &egui::Painter,
    font: &egui::FontId,
    (x0, x1): (f32, f32),
    top: f32,
    label: &str,
) -> bool {
    if label.is_empty() {
        return false;
    }
    let fill = if label == "Torrent" {
        TORRENT_CHIP
    } else {
        WEB_CHIP
    };
    // The cell holds the English key (filters match on it); the chip shows it translated.
    let full = crate::i18n::t(label);
    // Room: the pill starts 5 px in, pads its text 6 px each side, and ends 3 px before the cell.
    let label = &winui::ellipsize(p, font, full, (x1 - x0 - 5.0 - 12.0 - 3.0).max(0.0));
    let w = winui::text_width_font(p, font.clone(), label).round();
    let pill = Rect::from_min_size(pos2(x0 + 5.0, top + 2.0), egui::vec2(w + 12.0, ROW_H - 4.0));
    p.rect_filled(winui::rect(pill), (ROW_H - 4.0) / 2.0, fill);
    text_font(
        p,
        font.clone(),
        pill.left() + 6.0,
        top + 12.0,
        label,
        Color32::WHITE,
    );
    label != full
}

/// Where a column's funnel sits in its header: at the right, left of the sort chevron.
fn funnel_rect(st: &ListState, col: usize, x0: f32, x1: f32, ct: f32) -> Rect {
    let sorted = st.sort.is_some_and(|(sc, _)| sc == col);
    let right = x1 - if sorted { 18.0 } else { 4.0 };
    let left = (right - 12.0).max(x0 + 2.0);
    Rect::from_min_max(pos2(left, ct + 6.0), pos2(right, ct + HEADER_H - 6.0))
}

/// A small funnel: a triangle over a stem, in English rect `rc`.
fn funnel(p: &egui::Painter, rc: Rect, color: Color32) {
    let rc = winui::rect(rc);
    let (cx, top) = (rc.center().x, rc.top() + 1.0);
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(cx - 5.0, top),
            pos2(cx + 5.0, top),
            pos2(cx + 1.0, top + 5.0),
            pos2(cx - 1.0, top + 5.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
    winui::fill(p, cx - 1.0, top + 5.0, cx, top + 10.0, color);
}

const FILTER_W: f32 = 230.0;

/// The open filter box under a header: a checkbox per value, or a "contains" search box; it
/// filters as you change it. Escape or a click elsewhere closes it.
fn filter_box(ui: &mut Ui, st: &mut ListState) {
    let Some((col, at)) = st.filter_open else {
        return;
    };
    let values = filter_values(col);
    let h = match values {
        Some(v) => 58.0 + 18.0 * v.len() as f32,
        None => 92.0,
    };
    // Laid out in English under its header (`at`); shown where that is on screen.
    let rect = Rect::from_min_size(at, egui::vec2(FILTER_W, h));
    let shown = winui::rect(rect);
    let r = winui::rect;
    let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if ui.input(|i| i.pointer.any_pressed())
        && ui
            .ctx()
            .pointer_interact_pos()
            .is_some_and(|pp| !shown.contains(pp) && pp.y > at.y)
    {
        close = true;
    }
    egui::Area::new(egui::Id::unique(("list-filter", col)))
        .order(egui::Order::Foreground)
        .fixed_pos(shown.min)
        .show(ui.ctx(), |ui| {
            let (_, _) = ui.allocate_exact_size(rect.size(), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(shown, 0.0, winui::BG());
            p.rect_stroke(
                shown,
                0.0,
                egui::Stroke::new(1.0, FRAME),
                egui::StrokeKind::Inside,
            );
            let o = rect.min;
            match values {
                Some(vals) => {
                    // i18n: verb: label before the filter's list, i.e. show the rows that…
                    winui::text(
                        &p,
                        o.x + 10.0,
                        o.y + 20.0,
                        crate::i18n::t("Show"),
                        winui::TEXT(),
                    );
                    let mut keep: Vec<&'static str> = match &st.filters[col] {
                        Some(Filter::Values(k)) => k.clone(),
                        _ => vals.to_vec(),
                    };
                    for (i, v) in vals.iter().enumerate() {
                        let mut on = keep.contains(v);
                        let y = o.y + 28.0 + 18.0 * i as f32;
                        if winui::checkbox(ui, pos2(o.x + 12.0, y), &mut on, v, true).clicked() {
                            keep.retain(|k| k != v);
                            if on {
                                keep.push(v);
                            }
                        }
                    }
                    st.filters[col] = (keep.len() < vals.len()).then(|| {
                        Filter::Values(vals.iter().copied().filter(|v| keep.contains(v)).collect())
                    });
                    if winui::button(
                        ui,
                        r(Rect::from_min_size(
                            pos2(o.x + 10.0, o.y + h - 28.0),
                            egui::vec2(90.0, 21.0),
                        )),
                        "Show all",
                        winui::ButtonKind::Normal,
                        false,
                        None,
                    )
                    .clicked()
                    {
                        st.filters[col] = None;
                    }
                }
                None => {
                    let label = crate::i18n::fmt(
                        crate::i18n::t("{column} contains"),
                        &[("column", crate::i18n::tc("column", TITLES[col]))],
                    );
                    winui::text(&p, o.x + 10.0, o.y + 20.0, &label, winui::TEXT());
                    let mut t = match &st.filters[col] {
                        Some(Filter::Text(t)) => t.clone(),
                        _ => String::new(),
                    };
                    let edit = winui::edit(
                        ui,
                        r(Rect::from_min_size(
                            pos2(o.x + 10.0, o.y + 28.0),
                            egui::vec2(FILTER_W - 20.0, 21.0),
                        )),
                        &mut t,
                        true,
                        false,
                    );
                    if !edit.has_focus() && !ui.ctx().memory(|m| m.focused().is_some()) {
                        edit.request_focus();
                    }
                    st.filters[col] = (!t.is_empty()).then_some(Filter::Text(t));
                    if winui::button(
                        ui,
                        r(Rect::from_min_size(
                            pos2(o.x + 10.0, o.y + h - 30.0),
                            egui::vec2(90.0, 21.0),
                        )),
                        // i18n: verb: clears this filter
                        "Clear",
                        winui::ButtonKind::Normal,
                        false,
                        None,
                    )
                    .clicked()
                    {
                        st.filters[col] = None;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        close = true;
                    }
                }
            }
            if winui::button(
                ui,
                r(Rect::from_min_size(
                    pos2(
                        o.x + FILTER_W - 90.0,
                        o.y + h - if values.is_some() { 28.0 } else { 30.0 },
                    ),
                    egui::vec2(80.0, 21.0),
                )),
                // i18n: verb: closes the filter box
                "Close",
                winui::ButtonKind::Default,
                false,
                None,
            )
            .clicked()
            {
                close = true;
            }
        });
    if close {
        st.filter_open = None;
    }
}

/// The x of the column boundary nearest to `x` (where a dragged column would be inserted).
fn drop_x(cols: &[(usize, f32, f32)], x: f32) -> f32 {
    let mut best = cols.first().map_or(x, |c| c.1);
    for (_, x0, x1) in cols {
        for edge in [*x0, x1 + 1.0] {
            if (edge - x).abs() < (best - x).abs() {
                best = edge;
            }
        }
    }
    best
}

/// Move column `col` to the boundary nearest `x` (within the visible columns' order).
fn move_column(st: &mut ListState, cols: &[(usize, f32, f32)], col: usize, x: f32) {
    let edge = drop_x(cols, x);
    // Insertion index among visible columns: number of columns whose start is left of the edge.
    let mut target = cols.iter().filter(|(_, x0, _)| *x0 < edge).count();
    let from = cols.iter().position(|c| c.0 == col).unwrap_or(0);
    if target > from {
        target -= 1;
    }
    let mut shown: Vec<usize> = cols.iter().map(|c| c.0).collect();
    let moved = shown.remove(from);
    shown.insert(target.min(shown.len()), moved);
    // Hidden columns keep their place after the visible ones.
    let hidden: Vec<usize> = st
        .order
        .iter()
        .copied()
        .filter(|c| !st.visible[*c])
        .collect();
    st.order = shown.into_iter().chain(hidden).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_cells_keep_their_own_direction() {
        const FSI: char = '\u{2068}';
        const LRI: char = '\u{2066}';
        const PDI: char = '\u{2069}';
        // A file name and a description: first-strong isolates, whatever their script.
        assert_eq!(cell_text(0, "تقرير.pdf"), format!("{FSI}تقرير.pdf{PDI}"));
        assert_eq!(cell_text(0, "report.zip"), format!("{FSI}report.zip{PDI}"));
        assert_eq!(cell_text(7, "ملاحظة"), format!("{FSI}ملاحظة{PDI}"));
        // Dates read left to right.
        assert_eq!(
            cell_text(ADDED, "Oct 29 2026 20:00:00"),
            format!("{LRI}Oct 29 2026 20:00:00{PDI}")
        );
        assert_eq!(cell_text(6, "Oct 29"), format!("{LRI}Oct 29{PDI}"));
        // Interface text (a status) is laid out in the interface's direction as it is.
        assert_eq!(cell_text(3, "Complete"), "Complete");
        // A cut keeps the "..." inside the isolate: the closing mark goes, the dots follow the
        // name, not the paragraph.
        let cut: String = cell_text(0, "abcdef").chars().take(4).collect::<String>() + "...";
        assert_eq!(cut, format!("{FSI}abc..."));
    }

    fn spans(st: &ListState) -> Vec<(usize, f32, f32)> {
        let mut x = 0.0;
        st.shown()
            .into_iter()
            .map(|c| {
                let s = (c, x, x + st.widths[c] - 1.0);
                x += st.widths[c];
                s
            })
            .collect()
    }

    #[test]
    fn moving_a_column_right_and_left() {
        let mut st = ListState::default();
        let cols = spans(&st);
        // Drop "File Name" (slot 0) at the boundary after "Size" (x = 62 + 150 + 20 + 75 = 307).
        move_column(&mut st, &cols, 0, 308.0);
        assert_eq!(st.order[..5], [SOURCE, 1, 2, 0, 3]);
        let cols = spans(&st);
        // And to the very left.
        move_column(&mut st, &cols, 0, 1.0);
        assert_eq!(st.order[..3], [0, SOURCE, 1]);
    }

    fn row(source: &str, status: &'static str, name: &str, queued: bool) -> Row {
        let mut cells: [String; COLS] = Default::default();
        cells[0] = name.into();
        cells[SOURCE] = source.into();
        Row {
            status_group: status,
            icon: IconKey::Silk("page_white"),
            queued,
            cells,
            status_tip: String::new(),
        }
    }

    #[test]
    fn filters_by_values_and_text() {
        let mut st = ListState::default();
        let rows = [
            row("Web", "Complete", "Setup.EXE", false),
            row("Torrent", "Downloading", "debian.iso", true),
            row("Web", "Error", "notes.pdf", false),
        ];
        let shown = |st: &ListState| rows.iter().filter(|r| matches(st, r)).count();
        assert_eq!(shown(&st), 3);
        st.filters[SOURCE] = Some(Filter::Values(vec!["Torrent"]));
        assert_eq!(shown(&st), 1);
        st.filters[SOURCE] = None;
        st.filters[3] = Some(Filter::Values(vec!["Complete", "Error"]));
        assert_eq!(shown(&st), 2);
        st.filters[0] = Some(Filter::Text("setup".into()));
        assert_eq!(shown(&st), 1, "case-insensitive, combined with the others");
        st.filters = vec![None; COLS];
        st.filters[1] = Some(Filter::Values(vec!["In a queue"]));
        assert_eq!(shown(&st), 1);
    }

    #[test]
    fn hidden_columns_keep_their_place() {
        let mut st = ListState::default();
        st.visible[1] = false; // hide "Q"
        let cols = spans(&st);
        move_column(&mut st, &cols, 7, 0.0); // Description to the front
        assert_eq!(st.order[0], 7);
        assert!(st.order.contains(&1));
        assert_eq!(st.order.len(), COLS);
    }

    #[test]
    fn a_layout_saved_before_the_added_column_still_loads() {
        let old = Layout {
            order: vec![0, 2, 1, 3, 4, 5, 6, 7],
            widths: vec![200.0, 20.0, 75.0, 65.0, 75.0, 95.0, 65.0, 300.0],
            visible: vec![true; 8],
            sort: Some((2, false)),
        };
        let mut st = ListState::default();
        st.apply(old);
        assert_eq!(st.order, [SOURCE, 0, 2, 1, 3, 4, 5, 6, ADDED, 7]);
        assert_eq!(st.widths[0], 200.0, "the saved widths stay");
        assert_eq!(st.widths[ADDED], DEFAULT_WIDTHS[ADDED]);
        assert!(st.visible[ADDED]);
        assert!(st.visible[SOURCE]);
        assert_eq!(st.sort, Some((2, false)));
    }
}

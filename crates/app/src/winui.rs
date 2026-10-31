//! Windows dark-dialog controls, painted to IDM 6.42's measured pixels
//! (notes/ui-spec/dialogs.md "Dark dialog control styles"). Coordinates are client pixels;
//! "baseline" is the last pixel row of cap glyphs, as in the spec.

use egui::{
    Color32, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    pos2, vec2,
};
use unicode_segmentation::UnicodeSegmentation;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

crate::theme::themed! {
    /// Dialog background (light: light-dialogs.md).
    pub BG = rgb(0x39, 0x39, 0x39), rgb(0xf0, 0xf0, 0xf0);
    pub TEXT = rgb(0xff, 0xff, 0xff), rgb(0x00, 0x00, 0x00);
    /// Light: embossed #a0a0a0 statics (drawn flat here).
    pub TEXT_DISABLED = rgb(0x80, 0x80, 0x80), rgb(0xa0, 0xa0, 0xa0);
    pub EDIT_TEXT_DISABLED = rgb(0x6d, 0x6d, 0x6d), rgb(0x6d, 0x6d, 0x6d);
}
pub const GROUP_LINE: Color32 = rgb(0xdc, 0xdc, 0xdc);
pub const SELECTION: Color32 = rgb(0x00, 0x78, 0xd7);

/// Tahoma −11 ("MS Shell Dlg 2", 8 pt): every dialog body text.
pub fn tahoma() -> FontId {
    FontId::new(11.0, FontFamily::Name("tahoma".into()))
}

/// Tahoma Bold −13: the Options pages' header titles.
pub fn tahoma_bold() -> FontId {
    FontId::new(13.0, FontFamily::Name("tahoma-bold".into()))
}

/// Tahoma (IDM's dialogs), Tahoma Bold and Arial Bold from Windows' fonts folder. A family
/// whose file isn't there (macOS, Linux) is the interface font's, so drawing with it never fails.
pub fn install_fonts(defs: &mut egui::FontDefinitions, font_dir: Option<&std::path::Path>) {
    let ui = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (family, file) in [
        ("tahoma", "tahoma.ttf"),
        ("tahoma-bold", "tahomabd.ttf"),
        ("arial-bold", "arialbd.ttf"),
    ] {
        let fonts = match font_dir.and_then(|d| std::fs::read(d.join(file)).ok()) {
            Some(bytes) => {
                defs.font_data.insert(
                    family.into(),
                    std::sync::Arc::new(egui::FontData::from_owned(bytes)),
                );
                vec![family.to_owned()]
            }
            _ => ui.clone(),
        };
        defs.families.insert(FontFamily::Name(family.into()), fonts);
    }
}

// ---- right to left (D-019) -----------------------------------------------------------------
//
// Every window is laid out to IDM's English pixels. In a right-to-left interface (Arabic,
// Persian, Hebrew, Urdu, Uyghur) each window's client area is a mirrored *surface*: what is made
// for it is made at its mirrored place, so it is drawn and hit-tested there.
//
// - `r` and the rect helpers (`rect`) give a rect where it is on screen: `x → L + R − x − w`.
//   Everything that takes a `Rect` takes such a rect, and puts its insides by their leading
//   (start) and trailing (end) edges (`lead`, `trail`, `fwd`).
// - Text helpers take an English pen x: the *start* of the text. In RTL the text box is placed
//   with its start at the mirrored edge — right-aligned there; a right-aligned text
//   (`text_right`) ends left-aligned; a centred one stays centred. Text itself is never
//   mirrored, and is laid out in the interface's direction (`direction`), so a mixed line reads in
//   order; data goes in as isolates (`i18n::ltr`, `i18n::isolate`).
// - Points given to widgets (a check box's glyph) are English points.
// - The pixel painters (`fill`, `px`, `solid`) paint where they are told; `fill_l` takes English
//   pixels.

thread_local! {
    /// The surfaces being laid out (left and right edge of each), innermost last; only in a
    /// right-to-left interface.
    static SURFACES: std::cell::RefCell<Vec<(f32, f32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A surface laid out while this lives: a window's client area from `left` to `right`, mirrored
/// in a right-to-left interface (see above). A left-to-right one needs nothing.
#[must_use]
pub struct Surface(());

impl Surface {
    pub fn enter(left: f32, right: f32) -> Self {
        Self::enter_if(crate::i18n::rtl(), left, right)
    }

    /// [`Surface::enter`] with the direction given (tests; a surface that is never mirrored).
    pub fn enter_if(rtl: bool, left: f32, right: f32) -> Self {
        SURFACES.with(|s| {
            s.borrow_mut().push(if rtl {
                (left, right)
            } else {
                (f32::NAN, f32::NAN)
            })
        });
        Self(())
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        SURFACES.with(|s| s.borrow_mut().pop());
    }
}

/// The mirrored surface being laid out: its left + right edge.
fn axis() -> Option<f32> {
    SURFACES.with(|s| {
        s.borrow()
            .last()
            .filter(|(l, _)| !l.is_nan())
            .map(|(l, r)| l + r)
    })
}

/// Is the surface being laid out mirrored (a right-to-left interface)?
pub fn rtl() -> bool {
    axis().is_some()
}

/// The screen x of an English edge x (a rect's side, a text's pen).
pub fn mx(x: f32) -> f32 {
    axis().map_or(x, |a| a - x)
}

/// The screen column of an English pixel column (inclusive pixel coordinates).
pub fn mcol(x: f32) -> f32 {
    axis().map_or(x, |a| a - 1.0 - x)
}

/// The screen rect of an English rect.
pub fn rect(rc: Rect) -> Rect {
    match axis() {
        Some(a) => Rect::from_min_max(pos2(a - rc.max.x, rc.min.y), pos2(a - rc.min.x, rc.max.y)),
        None => rc,
    }
}

/// The screen point of an English point that starts something `w` wide (its top-left in English
/// is its top-right in RTL): the top-left of the screen rect.
pub fn at(p: Pos2, w: f32) -> Pos2 {
    rect(Rect::from_min_size(p, vec2(w, 0.0))).min
}

/// A screen rect's leading (start) edge: left, or right in RTL.
pub fn lead(rc: Rect) -> f32 {
    if rtl() { rc.right() } else { rc.left() }
}

/// The reading direction along x: +1, or −1 in RTL.
pub fn fwd() -> f32 {
    if rtl() { -1.0 } else { 1.0 }
}

/// `caption` as it fits from its English pen `x` to the far edge of what the painter may draw on
/// (the window, or the box it sits in), keeping a 6 px margin, and ending 5 px before English x
/// `until` (a control after the caption): whole, or cut with "..." (true).
fn fit_caption_until(p: &Painter, x: f32, caption: &str, until: Option<f32>) -> (String, bool) {
    let clip = p.clip_rect();
    // The far edge in English coordinates: the screen's left edge when mirrored.
    let edge = if rtl() { mx(clip.left()) } else { clip.right() };
    let end = until.map_or(edge - 6.0, |u| (u - 5.0).min(edge - 6.0));
    let room = end - x;
    if text_width(p, caption) <= room {
        (caption.to_owned(), false)
    } else {
        (ellipsize(p, &tahoma(), caption, room.max(0.0)), true)
    }
}

/// A widget's id: `key` within its window. Ids are made from a widget's label and place, so the
/// viewport is part of them: two windows with the same widget in the same place (two progress
/// windows' Pause buttons) must not share one.
fn widget_id(ui: &Ui, key: impl std::hash::Hash + std::fmt::Debug) -> egui::Id {
    egui::Id::unique((ui.ctx().viewport_id(), key))
}

/// Pixel rect from spec numbers (x, y, w, h) relative to `origin`, where it is on screen.
pub fn r(origin: Pos2, x: f32, y: f32, w: f32, h: f32) -> Rect {
    rect(Rect::from_min_size(origin + vec2(x, y), vec2(w, h)))
}

/// The paragraph direction of interface text: the interface's.
pub fn direction() -> egui::epaint::text::TextDirection {
    use egui::epaint::text::TextDirection;
    if crate::i18n::rtl() {
        TextDirection::RightToLeft
    } else {
        TextDirection::LeftToRight
    }
}

/// `s` laid out on one line in the interface's direction.
pub fn galley(p: &Painter, font: FontId, s: &str, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(s.to_owned(), font, color, f32::INFINITY);
    job.direction = direction();
    p.layout_job(job)
}

/// Draw `s` with its pen (its start) at English `x` and its baseline (last cap row) at
/// `baseline`. Returns the text's advance rect on screen.
pub fn text(p: &Painter, x: f32, baseline: f32, s: &str, color: Color32) -> Rect {
    text_font(p, tahoma(), x, baseline, s, color)
}

/// Segoe UI −12: menu bar, toolbar labels.
pub fn segoe() -> FontId {
    FontId::proportional(12.0)
}

/// Arial Bold −13: the Categories close "x".
pub fn arial_bold() -> FontId {
    FontId::new(13.0, FontFamily::Name("arial-bold".into()))
}

/// Like [`text`] in any font.
pub fn text_font(
    p: &Painter,
    font: FontId,
    x: f32,
    baseline: f32,
    s: &str,
    color: Color32,
) -> Rect {
    let g = galley(p, font, s, color);
    // The text's box in English, then on screen (its start at the mirrored edge in RTL).
    let left = rect(Rect::from_min_size(
        pos2(x.round(), 0.0),
        vec2(g.size().x, 0.0),
    ))
    .left();
    paint_galley(p, left, baseline, g, color)
}

/// Like [`text_font`] with its start at screen x `start` (a widget's inside, measured from its
/// leading edge): the text runs right from it, or left in RTL.
pub fn text_start(
    p: &Painter,
    font: FontId,
    start: f32,
    baseline: f32,
    s: &str,
    color: Color32,
) -> Rect {
    let g = galley(p, font, s, color);
    let left = if rtl() { start - g.size().x } else { start };
    paint_galley(p, left.round(), baseline, g, color)
}

/// Like [`text_font`] with its left at screen x `left` (text centred in a screen rect).
pub fn text_at(
    p: &Painter,
    font: FontId,
    left: f32,
    baseline: f32,
    s: &str,
    color: Color32,
) -> Rect {
    paint_galley(p, left, baseline, galley(p, font, s, color), color)
}

/// Paint `g` with its left at screen x `left` and its baseline (last cap row) at `baseline`.
fn paint_galley(
    p: &Painter,
    left: f32,
    baseline: f32,
    g: std::sync::Arc<egui::Galley>,
    color: Color32,
) -> Rect {
    let asc = g
        .rows
        .first()
        .map(|r| r.pos.y + r.row.glyphs.first().map_or(0.0, |g| g.pos.y))
        .unwrap_or(0.0);
    // The baseline line sits under the last cap row.
    let top = (baseline + 1.0 - asc).round();
    let size = g.size();
    let left = left.round();
    let drawn = Rect::from_min_size(pos2(left, top), size);
    fit_audit(p, drawn, g.text());
    p.galley(pos2(left, top), g, color);
    drawn
}

/// `MDM_FIT_AUDIT=<file>` (a check for translations, beyond IDM): every distinct text drawn past
/// the edge of what its painter may show (the window, a box), and every two texts drawn over each
/// other in one window, are appended to the file, so a run through the windows in each language
/// lists what doesn't fit. Off otherwise (one env lookup).
fn fit_audit(p: &Painter, drawn: Rect, text: &str) {
    use std::collections::{HashMap, HashSet};
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};
    /// What one window (viewport) has drawn in its current pass: (layer, visible rect, text).
    type Pass = (u64, Vec<(egui::LayerId, Rect, String)>);
    /// The audit file, the lines already in it, and each window's texts this pass.
    type Audit = Mutex<(
        std::fs::File,
        HashSet<String>,
        HashMap<egui::ViewportId, Pass>,
    )>;
    static AUDIT: OnceLock<Option<Audit>> = OnceLock::new();
    let Some(audit) = AUDIT.get_or_init(|| {
        let path = std::env::var_os("MDM_FIT_AUDIT")?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        Some(Mutex::new((file, Default::default(), Default::default())))
    }) else {
        return;
    };
    let mut audit = audit.lock().unwrap();
    let (file, seen, passes) = &mut *audit;
    let lang = crate::i18n::code();
    let clip = p.clip_rect();
    if (drawn.left() < clip.left() - 1.0 || drawn.right() > clip.right() + 1.0)
        && seen.insert(text.to_owned())
    {
        let _ = writeln!(
            file,
            "{}\t{:.0}..{:.0} in {:.0}..{:.0}\t{}", // not translated: a check's log line
            lang,
            drawn.left(),
            drawn.right(),
            clip.left(),
            clip.right(),
            text.replace('\n', " / ")
        );
    }
    // Over another text: what shows of each (inside its clip) crosses by more than 2 px across
    // and half a row down (rows a line apart may cross by their padding). The same text twice
    // (an embossed disabled label) is one text.
    let ctx = p.ctx();
    let viewport = ctx.viewport_id();
    let pass_nr = ctx.cumulative_pass_nr_for(viewport);
    let pass = passes.entry(viewport).or_insert((pass_nr, Vec::new()));
    if pass.0 != pass_nr {
        *pass = (pass_nr, Vec::new());
    }
    let shown = drawn.intersect(clip);
    if shown.width() <= 0.0 || shown.height() <= 0.0 {
        return;
    }
    for (layer, other, other_text) in &pass.1 {
        let both = shown.intersect(*other);
        if *layer == p.layer_id()
            && other_text != text
            && both.width() > 2.0
            && both.height() > 0.5 * shown.height().min(other.height())
        {
            let line = format!(
                "{lang}\toverlap\t{} | {}", // not translated: a check's log line
                other_text.replace('\n', " / "),
                text.replace('\n', " / ")
            );
            if seen.insert(line.clone()) {
                let _ = writeln!(file, "{line}");
            }
        }
    }
    pass.1.push((p.layer_id(), shown, text.to_owned()));
}

/// The screen x range of the `n`-th character of `s` drawn in `drawn` (a mnemonic's underline):
/// by the width of what comes before it in a left-to-right interface (as Windows does), by the
/// glyph itself in RTL, where the letters before it are to its right.
pub fn char_span(p: &Painter, font: &FontId, s: &str, n: usize, drawn: Rect) -> (f32, f32) {
    let before: String = s.chars().take(n).collect();
    let ch: String = s.chars().skip(n).take(1).collect();
    let cw = text_width_font(p, font.clone(), &ch);
    if !crate::i18n::rtl() {
        let x0 = drawn.left() + text_width_font(p, font.clone(), &before);
        return (x0, x0 + cw - 1.0);
    }
    // The glyph itself, in the form it takes in the word (a joined Arabic letter is narrower
    // than alone); the row keeps its glyphs in reading order.
    let g = galley(p, font.clone(), s, TEXT());
    let c = ch.chars().next();
    let glyph = g.rows.first().and_then(|r| {
        let k = before.chars().filter(|b| Some(*b) == c).count();
        r.row.glyphs.iter().filter(|gl| Some(gl.chr) == c).nth(k)
    });
    match glyph {
        Some(gl) => {
            let x0 = drawn.left() + gl.pos.x;
            (x0, x0 + gl.advance_width.round().max(1.0) - 1.0)
        }
        None => (drawn.left(), drawn.left() + cw - 1.0),
    }
}

pub fn text_width(p: &Painter, s: &str) -> f32 {
    text_width_font(p, tahoma(), s)
}

pub fn text_width_font(p: &Painter, font: FontId, s: &str) -> f32 {
    p.layout_no_wrap(s.to_owned(), font, TEXT()).size().x
}

/// `s` cut with "..." (three periods, as Windows' DT_END_ELLIPSIS) to fit `max` px.
pub fn ellipsize(p: &Painter, font: &FontId, s: &str, max: f32) -> String {
    if text_width_font(p, font.clone(), s) <= max {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    let cut = |n: usize| chars[..n].iter().collect::<String>() + "...";
    // The longest prefix that fits, by binary search (widths grow with the prefix).
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if text_width_font(p, font.clone(), &cut(mid)) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// [`text`] that never runs past `right`: cut with "..." when too long. Returns the drawn
/// rect and whether the text was cut (then show it whole with [`full_on_hover`]).
pub fn text_fit(
    p: &Painter,
    x: f32,
    baseline: f32,
    s: &str,
    color: Color32,
    right: f32,
) -> (Rect, bool) {
    let shown = ellipsize(p, &tahoma(), s, (right - x).max(0.0));
    let cut = shown != s;
    (text(p, x, baseline, &shown, color), cut)
}

/// [`text`] that never runs past the window's far edge: cut with "..." and shown whole on hover.
pub fn text_in_window(
    ui: &Ui,
    p: &Painter,
    x: f32,
    baseline: f32,
    s: &str,
    color: Color32,
) -> Rect {
    let (rc, cut) = text_fit(p, x, baseline, s, color, window_edges(p).1);
    if cut {
        full_on_hover(ui, rc, ("in-window", s), s);
    }
    rc
}

/// A label that leads into a control whose box starts at English `before`: at its pen `x` (IDM's
/// place) when it fits, else ending 5 px before the box and starting earlier, down to `min_x`;
/// cut with "..." only past that. Returns the drawn rect and whether it was cut.
pub fn label_into(
    p: &Painter,
    x: f32,
    baseline: f32,
    s: &str,
    color: Color32,
    before: f32,
    min_x: f32,
) -> (Rect, bool) {
    let end = before - 5.0;
    let w = text_width(p, s);
    if x + w <= end {
        return (text(p, x, baseline, s, color), false);
    }
    let start = (end - w).max(min_x);
    text_fit(p, start, baseline, s, color, end)
}

/// `s` with its single line breaks joined (a space, or nothing between letters of scripts
/// written without spaces: Chinese, Japanese, Thai…), blank lines kept as paragraph breaks.
fn reflow(s: &str) -> String {
    let spaceless = |c: char| {
        matches!(c as u32,
            0x0E00..=0x0EFF   // Thai, Lao
            | 0x1780..=0x17FF // Khmer
            | 0x3040..=0x30FF // Hiragana, Katakana
            | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF // Han
            | 0x3000..=0x303F | 0xFF00..=0xFFEF) // CJK punctuation, full-width forms
    };
    s.split("\n\n")
        .map(|para| {
            let mut out = String::new();
            for (k, line) in para.split('\n').enumerate() {
                let line = line.trim();
                if k > 0 && !out.is_empty() && !line.is_empty() {
                    let before = out.chars().last().is_some_and(spaceless);
                    let after = line.chars().next().is_some_and(spaceless);
                    if !(before && after) {
                        out.push(' ');
                    }
                }
                out.push_str(line);
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod reflow_tests {
    #[test]
    fn hard_breaks_flow_again() {
        assert_eq!(
            super::reflow("one two\nthree\n\nnew"),
            "one two three\n\nnew"
        );
        assert_eq!(super::reflow("日本語の\n文章"), "日本語の文章");
    }
}

/// Chinese and Japanese (Han, kana, their punctuation and full-width forms): broken between any
/// two characters.
fn cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
        | 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

/// Closing punctuation that a line broken between characters must not start with.
fn no_line_start(c: char) -> bool {
    "、。，．・：；？！ー）」』】〕｝〉》々ゝゞぁぃぅぇぉっゃゅょァィゥェォッャュョ,.:;?!)]}%"
        .contains(c)
}

/// A Windows 11 progress bar in `rc` (a few pixels tall): a thin track and the accent filled to
/// `fraction`; while the total is unknown (`None`) a segment runs along it (`time`: seconds,
/// for its place).
pub fn progress_bar(p: &Painter, rc: Rect, fraction: Option<f32>, time: f64) {
    let dark = crate::theme::dark();
    let (track, accent) = if dark {
        (rgb(0x9a, 0x9a, 0x9a), rgb(0x60, 0xcd, 0xff))
    } else {
        (rgb(0x86, 0x86, 0x86), rgb(0x00, 0x5f, 0xb8))
    };
    let r = rc.height() / 2.0;
    // The track is a 1 px line through the middle, as Windows 11 draws it.
    let mid = rc.center().y;
    p.rect_filled(
        rect(Rect::from_min_max(
            pos2(rc.left(), mid - 0.5),
            pos2(rc.right(), mid + 0.5),
        )),
        0.5,
        track,
    );
    let (from, to) = match fraction {
        Some(f) => (0.0, f.clamp(0.0, 1.0)),
        None => {
            let t = (time * 0.6).fract() as f32;
            ((t * 1.4 - 0.4).max(0.0), (t * 1.4).min(1.0))
        }
    };
    if to > from {
        let w = rc.width();
        p.rect_filled(
            rect(Rect::from_min_max(
                pos2(rc.left() + from * w, rc.top()),
                pos2(rc.left() + to * w, rc.bottom()),
            )),
            r,
            accent,
        );
    }
}

/// Read-only text that scrolls inside `area` (English coordinates, the box's own frame drawn by
/// the caller): the wheel or a trackpad over it, its scroll bar when the text is longer than the
/// box, and with `keys` the window's Up, Down, Page Up, Page Down, Home and End. Where it is
/// scrolled to is kept under `id`.
#[allow(clippy::too_many_arguments)]
pub fn scroll_text(
    ui: &mut Ui,
    id: egui::Id,
    area: Rect,
    s: &str,
    color: Color32,
    line_h: f32,
    keys: bool,
) {
    use crate::scrollbar;
    const PAD: f32 = 6.0;
    let p = ui.painter().clone();
    let visible = ((area.height() - 6.0) / line_h).floor().max(1.0);
    // Laid out first where nothing shows: the bar takes room only when the text needs it.
    let measure = p.with_clip_rect(Rect::NOTHING);
    let lines = |right: f32| paragraph(&measure, area.left() + PAD, 0.0, s, color, right, line_h);
    let mut total = lines(area.right() - PAD) as f32;
    let bar = total > visible;
    let text_right = if bar {
        let right = area.right() - 1.0 - scrollbar::THICK - 3.0;
        total = lines(right) as f32;
        right
    } else {
        area.right() - PAD
    };
    let max_top = (total - visible).max(0.0);
    let mut top: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
    // The wheel scrolls three lines a notch, as Windows text boxes do.
    if ui
        .interact(rect(area), id.with("area"), Sense::hover())
        .hovered()
    {
        top -= ui.input(|i| i.smooth_scroll_delta.y) / 40.0 * 3.0;
    }
    if keys {
        let page = (visible - 1.0).max(1.0);
        ui.input(|i| {
            for (key, by) in [
                (egui::Key::ArrowUp, -1.0),
                (egui::Key::ArrowDown, 1.0),
                (egui::Key::PageUp, -page),
                (egui::Key::PageDown, page),
                (egui::Key::Home, -total),
                (egui::Key::End, total),
            ] {
                if i.key_pressed(key) {
                    top += by;
                }
            }
        });
    }
    top = top.clamp(0.0, max_top);
    // Moved by the wheel or a key: draw the new place now, not at the next event.
    if ui
        .data(|d| d.get_temp::<f32>(id))
        .is_some_and(|was| was != top)
    {
        ui.ctx().request_repaint();
    }
    let clip = p.with_clip_rect(rect(area.shrink(2.0)));
    paragraph(
        &clip,
        area.left() + PAD,
        area.top() + 16.0 - top * line_h,
        s,
        color,
        text_right,
        line_h,
    );
    if bar {
        let track = rect(Rect::from_min_max(
            pos2(area.right() - 1.0 - scrollbar::THICK, area.top() + 1.0),
            pos2(area.right() - 1.0, area.bottom() - 1.0),
        ));
        if scrollbar::bar(
            ui,
            id.with("bar"),
            track,
            true,
            total,
            visible,
            1.0,
            &mut top,
        ) {
            ui.ctx().request_repaint();
        }
    }
    ui.data_mut(|d| d.insert_temp(id, top));
}

/// A paragraph from `x` to `right`, first baseline at `baseline`, `line_h` apart; returns the
/// lines drawn. English keeps its line breaks (broken where IDM breaks it); a translation's
/// copy of them is flowed again (`reflow`). A line too long for the room wraps at spaces, or
/// between any two characters in scripts written without spaces (Chinese, Japanese, Thai…).
pub fn paragraph(
    p: &Painter,
    x: f32,
    baseline: f32,
    s: &str,
    color: Color32,
    right: f32,
    line_h: f32,
) -> usize {
    let room = (right - x).max(1.0);
    let mut lines: Vec<String> = Vec::new();
    // IDM's English is hard-wrapped; a translation's copy of those breaks is flowed again (a
    // blank line still starts a new paragraph). English keeps its own lines.
    let flowed;
    let s = if crate::i18n::code() == "en" {
        s
    } else {
        flowed = reflow(s);
        flowed.as_str()
    };
    for given in s.split('\n') {
        let mut line = String::new();
        for word in given.split_inclusive(' ') {
            let candidate = format!("{line}{word}");
            if line.is_empty()
                || text_width(p, candidate.trim_end()) <= room
                // Chinese and Japanese fill the line: broken between characters below.
                || word.chars().next().is_some_and(cjk)
            {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line).trim_end().to_owned());
                line = word.to_owned();
            }
            // Wider than the room (a script without spaces), wherever it stands in the line:
            // break between characters, a letter and its marks kept together.
            while text_width(p, line.trim_end()) > room {
                let g: Vec<&str> = line.graphemes(true).collect();
                let mut n = 0;
                let mut w = String::new();
                for (k, gr) in g.iter().enumerate() {
                    w.push_str(gr);
                    if text_width(p, &w) > room {
                        break;
                    }
                    n = k + 1;
                }
                let latin = |k: usize| g[k].chars().all(|c| c.is_ascii_alphanumeric());
                // Not inside a Latin word ("Mister Download Manager" in Japanese): before it.
                if n > 0
                    && n < g.len()
                    && latin(n - 1)
                    && latin(n)
                    && let Some(space) = g[..n].iter().rposition(|gr| *gr == " ")
                {
                    n = space + 1;
                }
                // A closing mark never starts a line (Japanese kinsoku): the character before
                // it goes down with it.
                if n > 1 && g.get(n).is_some_and(|gr| gr.chars().all(no_line_start)) {
                    n -= 1;
                }
                if n == 0 {
                    break;
                }
                lines.push(g[..n].concat().trim_end().to_owned());
                line = g[n..].concat().trim_start().to_owned();
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    for (i, l) in lines.iter().enumerate() {
        text(p, x, baseline + line_h * i as f32, l, color);
    }
    lines.len()
}

/// Where the text before a line's first input box goes in a [`sentence`].
#[derive(Clone, Copy, Debug)]
pub enum Lead {
    /// Starts at this x; the box moves right when the text is too long for the room.
    Left(f32),
    /// Ends at this x (a column of labels right-aligned to their boxes); longer text grows left.
    Right(f32),
}

/// One line of a [`sentence`], placed as the English has it.
#[derive(Clone, Copy, Debug)]
pub struct SentenceLine {
    pub lead: Lead,
    pub lead_baseline: f32,
    /// The top-left of the line's first box.
    pub boxes: Pos2,
    /// From a box to the text after it.
    pub gap: f32,
    /// The baseline of the text after the first box.
    pub baseline: f32,
}

/// A piece of a [`sentence`] template.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece<'a> {
    Text(&'a str),
    /// A `{name}` that is one of the sentence's input boxes.
    Slot(&'a str),
}

/// `template`'s lines (`\n`), each split at the `{name}` placeholders that are in `slots`;
/// text pieces trimmed (the layout spaces them), empty ones dropped.
pub fn sentence_pieces<'a>(template: &'a str, slots: &[&str]) -> Vec<Vec<Piece<'a>>> {
    template
        .split('\n')
        .map(|line| {
            let mut out = Vec::new();
            let (mut from, mut at) = (0, 0);
            while let Some(open) = line[at..].find('{').map(|i| at + i) {
                let Some(close) = line[open..].find('}').map(|i| open + i) else {
                    break;
                };
                let name = &line[open + 1..close];
                if slots.contains(&name) {
                    out.push(Piece::Text(line[from..open].trim()));
                    out.push(Piece::Slot(name));
                    from = close + 1;
                }
                at = close + 1;
            }
            out.push(Piece::Text(line[from..].trim()));
            out.retain(|p| *p != Piece::Text(""));
            out
        })
        .collect()
}

/// Where [`sentence`] puts things: (x, baseline, text) for each text piece, and each slot's
/// rect in `slots` order. `width` measures a text. A right-aligned lead starts no further out
/// than `near` (the window's edge); the box after it moves on instead.
pub fn sentence_layout(
    template: &str,
    lines: &[SentenceLine],
    slots: &[(&str, egui::Vec2)],
    width: impl Fn(&str) -> f32,
    near: f32,
) -> (Vec<(f32, f32, String)>, Vec<Rect>) {
    let names: Vec<&str> = slots.iter().map(|(n, _)| *n).collect();
    let mut rows = sentence_pieces(template, &names);
    // More lines than the English has: the rest joins the last one.
    if rows.len() > lines.len() && !lines.is_empty() {
        let extra: Vec<Piece> = rows.drain(lines.len()..).flatten().collect();
        rows[lines.len() - 1].extend(extra);
    }
    let mut texts = Vec::new();
    let mut rects: Vec<Option<Rect>> = vec![None; slots.len()];
    // Where the last line ends: (x, box top, gap).
    let mut end = (0.0, 0.0, 8.0);
    for (row, spec) in rows.iter().zip(lines) {
        let mut x = None; // the next x after a box (or text after one)
        let mut first_box = true;
        let mut lead_end = None;
        for piece in row {
            match *piece {
                Piece::Text(s) => {
                    let w = width(s);
                    let (tx, base) = match x {
                        Some(x) => (x, spec.baseline),
                        None => match spec.lead {
                            Lead::Left(l) => (l, spec.lead_baseline),
                            Lead::Right(r) => ((r - w).max(near), spec.lead_baseline),
                        },
                    };
                    if x.is_none() {
                        lead_end = Some(tx + w);
                    }
                    texts.push((tx, base, s.to_owned()));
                    x = Some(tx + w + spec.gap);
                }
                Piece::Slot(name) => {
                    let i = names.iter().position(|n| *n == name).unwrap_or(0);
                    let size = slots[i].1;
                    let left = if first_box {
                        // Room for the lead text, at least 3 px before the box (a right-aligned
                        // one only needs it when it reached `near`).
                        lead_end.map_or(spec.boxes.x, |e| spec.boxes.x.max(e + 3.0))
                    } else {
                        x.unwrap_or(spec.boxes.x)
                    };
                    first_box = false;
                    let rc = Rect::from_min_size(pos2(left, spec.boxes.y), size);
                    if rects[i].is_none() {
                        rects[i] = Some(rc);
                    }
                    x = Some(rc.right() + spec.gap);
                }
            }
        }
        end = (x.unwrap_or(spec.boxes.x), spec.boxes.y, spec.gap);
    }
    // A box the translation left out still gets a place: after the last line.
    let rects = rects
        .into_iter()
        .zip(slots)
        .map(|(rc, (_, size))| {
            rc.unwrap_or_else(|| {
                let rc = Rect::from_min_size(pos2(end.0, end.1), *size);
                end.0 = rc.right() + end.2;
                rc
            })
        })
        .collect();
    (texts, rects)
}

/// A translated sentence with input boxes in it ("Download {files} files at the same time"):
/// the text pieces drawn with `draw(painter, x, baseline, text)` around the boxes, in the
/// translation's order, so a translator can move a box. `lines` place each line as the
/// English has it (one entry per `\n`-separated line); `slots` name the boxes and their sizes.
/// Returns each box's rect on screen, in `slots` order, for the caller to draw its edit there.
/// `draw` gets English pens (draw with [`text`]); `lines` are English too: in RTL the sentence
/// runs from the right, its first piece at the mirrored lead. Nothing starts or ends outside the
/// window: a text that would run past its far edge is cut with "..." and shown whole on hover.
pub fn sentence(
    ui: &Ui,
    p: &Painter,
    template: &str,
    lines: &[SentenceLine],
    slots: &[(&str, egui::Vec2)],
    mut draw: impl FnMut(&Painter, f32, f32, &str),
) -> Vec<Rect> {
    let (near, far) = window_edges(p);
    let (texts, rects) = sentence_layout(template, lines, slots, |s| text_width(p, s), near);
    for (i, (x, baseline, s)) in texts.iter().enumerate() {
        let shown = ellipsize(p, &tahoma(), s, (far - x).max(0.0));
        draw(p, *x, *baseline, &shown);
        if shown != *s {
            let w = text_width(p, &shown);
            let at = rect(Rect::from_min_max(
                pos2(*x, baseline - 11.0),
                pos2(x + w, baseline + 3.0),
            ));
            full_on_hover(ui, at, ("sentence", template, i), s);
        }
    }
    rects.into_iter().map(rect).collect()
}

/// The English x of the window's near and far edges, 6 px in: where a text may start and end.
fn window_edges(p: &Painter) -> (f32, f32) {
    let clip = p.clip_rect();
    if rtl() {
        (mx(clip.right()) + 6.0, mx(clip.left()) - 6.0)
    } else {
        (clip.left() + 6.0, clip.right() - 6.0)
    }
}

/// A tooltip with the whole of a cut text while the pointer is over it (as Windows shows
/// for truncated list items).
pub fn full_on_hover(
    ui: &Ui,
    rect: Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
    full: &str,
) {
    let id = ui.scope_id().with(("full-text", salt));
    ui.interact(rect, id, egui::Sense::hover())
        .on_hover_text(full);
}

/// Right-aligned label whose ink ends at `right`.
pub fn text_right(p: &Painter, right: f32, baseline: f32, s: &str, color: Color32) -> (Rect, bool) {
    // A text longer than the room to the window's near edge (6 px margin) is cut with "...":
    // it never starts outside the window. The caller shows it whole on hover when cut.
    text_right_within(p, right, baseline, s, color, window_edges(p).0)
}

/// [`text_right`] that starts no further out than English x `near` (inside a group box).
pub fn text_right_within(
    p: &Painter,
    right: f32,
    baseline: f32,
    s: &str,
    color: Color32,
    near: f32,
) -> (Rect, bool) {
    let room = right + 1.0 - near;
    let (shown, cut) = if text_width(p, s) <= room {
        (s.to_owned(), false)
    } else {
        (ellipsize(p, &tahoma(), s, room.max(0.0)), true)
    };
    let w = text_width(p, &shown);
    (text(p, right + 1.0 - w, baseline, &shown, color), cut)
}

/// Fill an exact pixel rect with no anti-aliasing (egui feathers `rect_filled` by half a pixel,
/// which erases 1 × 1 rects and softens 1 px lines).
pub fn solid(p: &Painter, rc: Rect, c: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.add_colored_rect(rc, c);
    p.add(egui::Shape::mesh(mesh));
}

/// Fill inclusive pixel coordinates `x0..=x1, y0..=y1`.
pub fn fill(p: &Painter, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
    solid(
        p,
        Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1.0, y1 + 1.0)),
        c,
    );
}

pub fn px(p: &Painter, x: f32, y: f32, c: Color32) {
    fill(p, x, y, x, y, c);
}
fn hline(p: &Painter, x0: f32, x1: f32, y: f32, c: Color32) {
    fill(p, x0, y, x1, y, c);
}
fn vline(p: &Painter, x: f32, y0: f32, y1: f32, c: Color32) {
    fill(p, x, y0, x, y1, c);
}

/// The 2 px light edit border around a dark fill (height 23).
pub fn edit_frame(p: &Painter, rc: Rect, enabled: bool) {
    let (l, t, rr, b) = (rc.left(), rc.top(), rc.right() - 1.0, rc.bottom() - 1.0);
    // Dark: the dialog colour. Light: white when editable, #f0f0f0 read-only/disabled.
    let fill_c = match (crate::theme::dark(), enabled) {
        (true, _) => BG(),
        (false, true) => Color32::WHITE,
        (false, false) => rgb(0xf0, 0xf0, 0xf0),
    };
    p.rect_filled(rc, 0.0, fill_c);
    let outer = rgb(0xec, 0xec, 0xec);
    hline(p, l + 1.0, rr - 1.0, t, outer);
    vline(p, l, t + 1.0, b - 1.0, outer);
    vline(
        p,
        rr,
        t + 1.0,
        b - 1.0,
        if enabled {
            rgb(0xe6, 0xe6, 0xe6)
        } else {
            outer
        },
    );
    hline(
        p,
        l + 1.0,
        rr - 1.0,
        b,
        if enabled {
            rgb(0x83, 0x83, 0x83)
        } else {
            outer
        },
    );
    let inner = if enabled {
        rgb(0xfe, 0xfe, 0xfe)
    } else {
        rgb(0xfb, 0xfb, 0xfb)
    };
    hline(
        p,
        l + 1.0,
        rr - 1.0,
        t + 1.0,
        if enabled {
            inner
        } else {
            rgb(0xf8, 0xf8, 0xf8)
        },
    );
    vline(p, l + 1.0, t + 1.0, b - 1.0, inner);
    vline(
        p,
        rr - 1.0,
        t + 1.0,
        b - 1.0,
        if enabled {
            rgb(0xf7, 0xf7, 0xf7)
        } else {
            inner
        },
    );
    hline(p, l + 1.0, rr - 1.0, b - 1.0, inner);
    let (ct, cb) = if enabled {
        (rgb(0x6f, 0x6f, 0x6f), rgb(0x79, 0x79, 0x79))
    } else {
        (rgb(0x72, 0x72, 0x73), rgb(0x77, 0x77, 0x77))
    };
    px(p, l, t, ct);
    px(p, rr, t, ct);
    px(p, l, b, cb);
    px(p, rr, b, cb);
}

/// A single-line edit: frame + egui text editing positioned so text starts at x+5 with its
/// baseline at top+13.
pub fn edit(ui: &mut Ui, rc: Rect, value: &mut String, enabled: bool, password: bool) -> Response {
    edit_frame(ui.painter(), rc, enabled);
    let resp = edit_text(ui, rc, value, enabled, password, 5.0);
    edit_focus_line(ui, &resp, rc);
    resp
}

/// A text box's focus, as Windows 11 shows it: an accent line along its bottom edge.
pub fn edit_focus_line(ui: &Ui, resp: &Response, rc: Rect) {
    if resp.has_focus() {
        fill(
            ui.painter(),
            rc.left() + 1.0,
            rc.bottom() - 2.0,
            rc.right() - 2.0,
            rc.bottom() - 1.0,
            accent(),
        );
    }
}

fn edit_text(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    enabled: bool,
    password: bool,
    inset: f32,
) -> Response {
    let color = if enabled {
        TEXT()
    } else {
        EDIT_TEXT_DISABLED()
    };
    edit_text_with(
        ui,
        rc,
        value,
        enabled,
        password,
        inset,
        color,
        start_align(),
    )
}

/// Text aligned to its start: left, right in RTL.
pub fn start_align() -> egui::Align {
    if rtl() {
        egui::Align::RIGHT
    } else {
        egui::Align::LEFT
    }
}

/// Text aligned to its end (numbers in a box): right, left in RTL.
pub fn end_align() -> egui::Align {
    if rtl() {
        egui::Align::LEFT
    } else {
        egui::Align::RIGHT
    }
}

#[allow(clippy::too_many_arguments)]
fn edit_text_with(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    enabled: bool,
    password: bool,
    inset: f32,
    color: Color32,
    align: egui::Align,
) -> Response {
    // Tahoma −11: ascent 11, so the text cell top is baseline − 10 = top + 3.
    let inner = Rect::from_min_max(
        pos2(rc.left() + inset, rc.top() + 3.0),
        pos2(rc.right() - inset, rc.top() + 16.0),
    );
    let te = egui::TextEdit::singleline(value)
        .align(egui::Align2([align, egui::Align::TOP]))
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(tahoma())
        .text_color(color)
        .password(password)
        .desired_width(inner.width());
    let resp = ui.put(inner, |ui: &mut Ui| {
        // The caret in the box's own ink: egui's default (a pale blue in its dark style) was
        // all but invisible on a light box.
        ui.visuals_mut().text_cursor.stroke.color = color;
        ui.add_enabled_ui(enabled, |ui| ui.add(te)).inner
    });
    if enabled {
        edit_menu(&resp, value, password, false);
    }
    resp
}

#[derive(Clone, Copy)]
enum EditCmd {
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
}

/// A text box's right-click menu: Cut, Copy, Paste, Delete and Select all, on the box's own
/// selection, as every Windows, macOS and Linux text box has (Cut and Copy not for passwords). A
/// menu window of its own (`chrome::popup`), so short dialogs never cut it off.
pub fn edit_menu(resp: &Response, value: &mut String, password: bool, multiline: bool) {
    use crate::chrome::{MenuEntry, PopupResult};
    use egui::text::{CCursor, CCursorRange};
    use egui::text_edit::TextEditState;
    let ctx = resp.ctx.clone();
    let id = resp.id;
    let key = id.with("edit-menu");
    if resp.secondary_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        ctx.data_mut(|d| d.insert_temp(key, p));
    }
    // Shift+F10 (the keyboard's way to a right-click menu) opens it under the box.
    if resp.has_focus() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10))
    {
        ctx.data_mut(|d| d.insert_temp(key, resp.rect.left_bottom()));
    }
    let Some(at) = ctx.data(|d| d.get_temp::<Pos2>(key)) else {
        return;
    };
    let mut state = TextEditState::load(&ctx, id).unwrap_or_default();
    let n = value.chars().count();
    let (a, b) = state
        .cursor
        .char_range()
        .map(|r| {
            let (x, y) = (r.primary.index.0.min(n), r.secondary.index.0.min(n));
            (x.min(y), x.max(y))
        })
        .unwrap_or((n, n));
    let has = a < b;
    let clip = arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .ok()
        .filter(|t| !t.is_empty());
    let entries = vec![
        MenuEntry::Item("Cu&t", EditCmd::Cut, has && !password, false),
        MenuEntry::Item("&Copy", EditCmd::Copy, has && !password, false),
        MenuEntry::Item("&Paste", EditCmd::Paste, clip.is_some(), false),
        MenuEntry::Item("&Delete", EditCmd::Delete, has, false),
        MenuEntry::Sep,
        MenuEntry::Item("Select &all", EditCmd::SelectAll, n > 0, false),
    ];
    let cmd = match crate::chrome::popup(&ctx, &format!("edit-{id:?}"), at, &entries, &|_| None) {
        PopupResult::Open | PopupResult::Neighbor(_) => return,
        PopupResult::Dismissed => None,
        PopupResult::Chosen(c) => Some(c),
    };
    ctx.data_mut(|d| d.remove::<Pos2>(key));
    let Some(cmd) = cmd else { return };
    let byte = |c: usize| value.char_indices().nth(c).map_or(value.len(), |(i, _)| i);
    let (ba, bb) = (byte(a), byte(b));
    let caret = match cmd {
        EditCmd::Cut => {
            ctx.copy_text(value[ba..bb].to_owned());
            value.replace_range(ba..bb, "");
            (a, a)
        }
        EditCmd::Copy => {
            ctx.copy_text(value[ba..bb].to_owned());
            (a, b)
        }
        EditCmd::Paste => {
            // A single-line box takes one line; a list box keeps the lines.
            let text = clip.unwrap_or_default();
            let text = if multiline {
                text.replace("\r\n", "\n")
            } else {
                text.replace(['\r', '\n'], " ")
            };
            value.replace_range(ba..bb, &text);
            let end = a + text.chars().count();
            (end, end)
        }
        EditCmd::Delete => {
            value.replace_range(ba..bb, "");
            (a, a)
        }
        EditCmd::SelectAll => (0, value.chars().count()),
    };
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(caret.0),
        CCursor::new(caret.1),
    )));
    state.store(&ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// A dark edit with right-aligned text (numbers, e.g. the Speed Limiter value).
pub fn edit_right(ui: &mut Ui, rc: Rect, value: &mut String, enabled: bool) -> Response {
    edit_frame(ui.painter(), rc, enabled);
    let color = if enabled { TEXT() } else { DISABLED_TEXT };
    edit_text_with(ui, rc, value, enabled, false, 5.0, color, end_align())
}

const DISABLED_TEXT: Color32 = rgb(0x80, 0x80, 0x80);

// ---- control states -------------------------------------------------------------------------

/// What a control shows this frame: at rest, under the pointer (hot), with the mouse button held
/// on it (pressed), with its list or menu open, or disabled. Every control takes its look from
/// one of these, so they all answer the pointer the same way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Normal,
    Hot,
    Pressed,
    Open,
    Disabled,
}

impl State {
    /// From the control's response this frame.
    pub fn of(resp: &Response, enabled: bool) -> Self {
        if !enabled {
            State::Disabled
        } else if resp.is_pointer_button_down_on() {
            State::Pressed
        } else if resp.hovered() {
            State::Hot
        } else {
            State::Normal
        }
    }

    /// Shown open while `open` (a combo's list, a drop-down's menu); a press still shows.
    pub fn open_if(self, open: bool) -> Self {
        if open && matches!(self, State::Normal | State::Hot) {
            State::Open
        } else {
            self
        }
    }
}

/// The fill of a part with no face of its own (a combo's arrow, a glyph button, a column
/// header) in `state`: none at rest, a tint under the pointer, a deeper one pressed or open.
/// `dark_surface`: the part sits on a dark background (the dark buttons' greys), else the light
/// icon buttons' greys.
pub fn subtle_fill(state: State, dark_surface: bool) -> Option<Color32> {
    match (state, dark_surface) {
        (State::Hot, false) => Some(rgb(0xf3, 0xf3, 0xf3)),
        (State::Pressed | State::Open, false) => Some(rgb(0xe8, 0xe8, 0xe8)),
        (State::Hot, true) => Some(rgb(0x45, 0x45, 0x45)),
        (State::Pressed | State::Open, true) => Some(rgb(0x26, 0x26, 0x26)),
        _ => None,
    }
}

/// The blue highlight of a calendar's days and arrows (Win32's month calendar) in `state`:
/// #e5f1fb under the pointer, #cce4f7 pressed; deep blues on a dark surface.
pub fn highlight_fill(state: State, dark_surface: bool) -> Option<Color32> {
    match (state, dark_surface) {
        (State::Hot, false) => Some(rgb(0xe5, 0xf1, 0xfb)),
        (State::Pressed | State::Open, false) => Some(rgb(0xcc, 0xe4, 0xf7)),
        (State::Hot, true) => Some(rgb(0x2d, 0x4a, 0x60)),
        (State::Pressed | State::Open, true) => Some(rgb(0x23, 0x3d, 0x52)),
        _ => None,
    }
}

/// Tints `zone` (a part already drawn at rest) for `state` and draws its glyph again on top.
pub fn tint_part(
    p: &Painter,
    zone: Rect,
    state: State,
    dark_surface: bool,
    glyph: impl FnOnce(&Painter),
) {
    if let Some(c) = subtle_fill(state, dark_surface) {
        p.rect_filled(zone, 2.0, c);
        glyph(p);
    }
}

/// An editable or drop-list combo's arrow zone (inside its border, at the trailing edge).
fn combo_zone(rc: Rect) -> Rect {
    if rtl() {
        Rect::from_min_max(
            rc.min + vec2(1.0, 1.0),
            pos2(rc.left() + 20.0, rc.bottom() - 1.0),
        )
    } else {
        Rect::from_min_max(
            pos2(rc.right() - 20.0, rc.top() + 1.0),
            rc.max - vec2(1.0, 1.0),
        )
    }
}

/// A combo's chevron: 8 × 6 "v" at the arrow zone's left + 6, top + 8.
fn combo_chevron(p: &Painter, rc: Rect) {
    let arrow_left = if rtl() {
        rc.left() + 1.0
    } else {
        rc.right() - 1.0 - 19.0
    };
    let (cx, cy) = (arrow_left + 6.0, rc.top() + 8.0);
    p.line(
        vec![
            pos2(cx, cy + 0.5),
            pos2(cx + 4.0, cy + 4.5),
            pos2(cx + 8.0, cy + 0.5),
        ],
        Stroke::new(1.2, rgb(0x6f, 0x6f, 0x6f)),
    );
}

/// Combo frame: 1 px border (#8d8d8d, or #0078d4 when focused), 2 px white ring, dark text
/// area, white 19 px arrow zone with a grey chevron. Height 21.
pub fn combo_frame(p: &Painter, rc: Rect, focused: bool) {
    let (l, t, rr, b) = (rc.left(), rc.top(), rc.right() - 1.0, rc.bottom() - 1.0);
    let border = if focused {
        rgb(0x00, 0x78, 0xd4)
    } else {
        rgb(0x8d, 0x8d, 0x8d)
    };
    p.rect_filled(rc, 0.0, Color32::WHITE);
    p.rect_stroke(rc, 0.0, Stroke::new(1.0, border), StrokeKind::Inside);
    let (c1, c2) = if focused {
        (rgb(0x26, 0x4f, 0x6e), rgb(0x04, 0x73, 0xc8))
    } else {
        (rgb(0x56, 0x56, 0x56), rgb(0x86, 0x86, 0x86))
    };
    for (x, y) in [(l, t), (rr, t), (l, b), (rr, b)] {
        px(p, x, y, c1);
    }
    let _ = c2;
    // Dark text area inside the ring, before the arrow zone (light: all white); the arrow zone
    // at the trailing edge (the left in RTL).
    let arrow_left = if rtl() { l + 1.0 } else { rr - 19.0 };
    if crate::theme::dark() {
        let text_area = if rtl() {
            Rect::from_min_max(pos2(l + 20.0, t + 3.0), pos2(rr - 2.0, b - 2.0))
        } else {
            Rect::from_min_max(pos2(l + 3.0, t + 3.0), pos2(arrow_left, b - 2.0))
        };
        p.rect_filled(text_area, 0.0, BG());
    }
    combo_chevron(p, rc);
}

/// Drop-list combo showing `value`; returns the click response.
pub fn combo(ui: &mut Ui, rc: Rect, value: &str, id: &str) -> Response {
    let resp = ui.interact(rc, widget_id(ui, ("combo", id)), Sense::click());
    combo_frame(ui.painter(), rc, resp.has_focus());
    // The arrow zone (always white) shows the state.
    tint_part(
        ui.painter(),
        combo_zone(rc),
        State::of(&resp, true),
        false,
        |p| combo_chevron(p, rc),
    );
    text_start(
        ui.painter(),
        tahoma(),
        lead(rc) + 5.0 * fwd(),
        rc.top() + 13.0,
        value,
        TEXT(),
    );
    resp
}

/// Editable combo (Add URL "Address", Save As): frame + edit child at +3,+3 .. right−20 (the
/// arrow zone at the trailing edge).
/// Its blue focus border while its box has the keyboard focus.
pub fn combo_edit(ui: &mut Ui, rc: Rect, value: &mut String) -> Response {
    // The frame goes under the box, so it is drawn first, with the box's focus as last frame
    // left it (a change repaints at once).
    let key = widget_id(ui, ("combo-edit-focus", rc.min.x as i32, rc.min.y as i32));
    let was = ui.data(|d| d.get_temp::<bool>(key).unwrap_or(false));
    combo_frame(ui.painter(), rc, was);
    let child = if rtl() {
        Rect::from_min_max(pos2(rc.left() + 20.0, rc.top()), rc.max - vec2(3.0, 0.0))
    } else {
        Rect::from_min_max(
            rc.min + vec2(3.0, 0.0),
            pos2(rc.right() - 20.0, rc.bottom()),
        )
    };
    let resp = edit_text(ui, child, value, true, false, 2.0);
    if resp.has_focus() != was {
        ui.data_mut(|d| d.insert_temp(key, resp.has_focus()));
        ui.ctx().request_repaint();
    }
    resp
}

/// The arrow zone of an editable combo (`combo_edit`, at the trailing edge): a click on it
/// opens the combo's drop list (`drop_list`); it shows hot, pressed, and pressed while `open`.
pub fn combo_arrow(ui: &mut Ui, rc: Rect, id: &str, open: bool) -> Response {
    let zone = combo_zone(rc);
    let resp = ui.interact(zone, widget_id(ui, ("combo-arrow", id)), pointer_only());
    let state = State::of(&resp, true).open_if(open);
    tint_part(ui.painter(), zone, state, false, |p| combo_chevron(p, rc));
    resp
}

/// The keys that open an editable combo's drop list while its box has the focus: Alt+Down or
/// F4, as Windows' combo boxes.
pub fn combo_keys(ui: &mut Ui, edit: &Response) -> bool {
    edit.has_focus()
        && ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::F4)
        })
}

/// An editable combo's drop list under `anchor`: `items` newest first, or `empty` greyed when
/// there are none. A window of its own, as the menus are (`chrome::popup`), so a list longer
/// than the dialog is never cut off, and the arrow keys, Enter and Escape work in it. Returns the
/// index picked; `open` turns false on a pick, on Escape or on a click anywhere else.
pub fn drop_list(
    ui: &mut Ui,
    id: &str,
    anchor: Rect,
    items: &[String],
    empty: &str,
    open: &mut bool,
) -> Option<usize> {
    use crate::chrome::{MenuEntry, PopupResult, label};
    // A menu label: long addresses shortened in the middle, "&" kept as text (not an access key).
    let shown = |t: &str| {
        let n = t.chars().count();
        let short: String = if n > 72 {
            t.chars()
                .take(36)
                .chain(std::iter::once('\u{2026}'))
                .chain(t.chars().skip(n - 35))
                .collect()
        } else {
            t.to_owned()
        };
        label(&short.replace('&', "&&"))
    };
    let entries: Vec<MenuEntry<usize>> = if items.is_empty() {
        vec![MenuEntry::Item(label(empty), usize::MAX, false, false)]
    } else {
        items
            .iter()
            .take(crate::settings::HISTORY)
            .enumerate()
            .map(|(i, t)| MenuEntry::Item(shown(t), i, true, false))
            .collect()
    };
    let menu = crate::chrome::popup(
        ui.ctx(),
        &format!("drop-{id}"),
        anchor.left_bottom(),
        &entries,
        &|_| None,
    );
    match menu {
        PopupResult::Chosen(i) => {
            *open = false;
            Some(i)
        }
        PopupResult::Dismissed => {
            *open = false;
            None
        }
        PopupResult::Open | PopupResult::Neighbor(_) => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Normal,
    Default,
    Disabled,
}

/// Push button: #333333 fill, 1 px rounded (r 4) border by state, text centred with its
/// baseline at top+14 (h 21) or top+13 (h 19). `mnemonic` underlines that char index.
pub fn button(
    ui: &mut Ui,
    rc: Rect,
    label: &str,
    kind: ButtonKind,
    focus_rect: bool,
    mnemonic: Option<usize>,
) -> Response {
    // The label is translated here (M16); a mnemonic index points into the English text only,
    // and becomes the "&" form every button uses.
    let translated = crate::i18n::t(label);
    let label = match mnemonic.filter(|_| translated == label) {
        Some(i) => {
            let at = label.char_indices().nth(i).map_or(label.len(), |(b, _)| b);
            format!("{}&{}", &label[..at], &label[at..])
        }
        None => translated.to_owned(),
    };
    button_face(ui, rc, &label, kind, focus_rect)
}

/// `label` as it fits on a button `rc`: whole, or cut with "..." (true) when it would reach the
/// border (English labels fit IDM's buttons, so English never changes).
fn fit_label(p: &Painter, label: &str, rc: Rect) -> (String, bool) {
    let room = rc.width() - 4.0;
    if text_width(p, label) <= room {
        (label.to_owned(), false)
    } else {
        (ellipsize(p, &tahoma(), label, room), true)
    }
}

/// Classic sunken 13 × 13 checkbox with its caption 5 px to the right (baseline box top + 10).
pub fn checkbox(
    ui: &mut Ui,
    box_min: Pos2,
    checked: &mut bool,
    caption: &str,
    enabled: bool,
) -> Response {
    checkbox_until(ui, box_min, checked, caption, enabled, None)
}

/// [`checkbox`] whose caption also stops before English x `until` (a control after it).
pub fn checkbox_until(
    ui: &mut Ui,
    box_min: Pos2,
    checked: &mut bool,
    caption: &str,
    enabled: bool,
    until: Option<f32>,
) -> Response {
    let caption = crate::i18n::t(caption);
    // Light: the Windows 11 themed checkbox, caption at glyph + 16 (light-dialogs.md).
    if !crate::theme::dark() {
        return checkbox_light_until(ui, box_min, checked, caption, enabled, until);
    }
    let p = ui.painter().clone();
    // A caption longer than its room ends in "..." at the edge, whole on hover (UI rule).
    let (shown, cut) = fit_caption_until(&p, box_min.x + 18.0, caption, until);
    let w = text_width(&p, &shown);
    // `box_min` is English; the box and its caption (after it) are placed on screen.
    let english = box_min;
    let hit = rect(Rect::from_min_size(
        english,
        vec2(13.0 + 5.0 + w + 2.0, 13.0),
    ));
    let box_min = at(english, 13.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(
        hit,
        widget_id(ui, ("check", caption, box_min.x as i32, box_min.y as i32)),
        sense,
    );
    show_focus(ui, &resp, hit.expand(1.0));
    if resp.clicked() {
        *checked = !*checked;
    }
    let (l, t) = (box_min.x, box_min.y);
    let (rr, b) = (l + 12.0, t + 12.0);
    // Interior: white, or #f0f0f0 when disabled (spec "Checkbox").
    let state = State::of(&resp, enabled);
    let interior = match state {
        State::Disabled => rgb(0xf0, 0xf0, 0xf0),
        // Under the pointer and pressed: the shared highlight (a classic box has no hot look
        // of its own; without one it didn't answer the pointer at all).
        _ => highlight_fill(state, false).unwrap_or(Color32::WHITE),
    };
    solid(&p, Rect::from_min_size(box_min, vec2(13.0, 13.0)), interior);
    hline(&p, l, rr - 1.0, t, rgb(0xa0, 0xa0, 0xa0));
    vline(&p, l, t, b - 1.0, rgb(0xa0, 0xa0, 0xa0));
    hline(&p, l, rr, b, Color32::WHITE);
    vline(&p, rr, t, b, Color32::WHITE);
    hline(&p, l + 1.0, rr - 2.0, t + 1.0, rgb(0x69, 0x69, 0x69));
    vline(&p, l + 1.0, t + 1.0, b - 2.0, rgb(0x69, 0x69, 0x69));
    hline(&p, l + 1.0, rr - 1.0, b - 1.0, rgb(0xe3, 0xe3, 0xe3));
    vline(&p, rr - 1.0, t + 1.0, b - 1.0, rgb(0xe3, 0xe3, 0xe3));
    if *checked {
        // The classic check mark, row by row from the interior's top-left (spec "Check mark").
        let ink = if enabled {
            Color32::BLACK
        } else {
            rgb(0x80, 0x80, 0x80)
        };
        let (ox, oy) = (l + 2.0, t + 2.0);
        let rows: [&[(f32, f32)]; 7] = [
            &[(7.0, 7.0)],
            &[(6.0, 7.0)],
            &[(1.0, 1.0), (5.0, 7.0)],
            &[(1.0, 2.0), (4.0, 6.0)],
            &[(1.0, 5.0)],
            &[(2.0, 4.0)],
            &[(3.0, 3.0)],
        ];
        for (i, spans) in rows.iter().enumerate() {
            for (a, b) in spans.iter() {
                hline(&p, ox + a, ox + b, oy + 1.0 + i as f32, ink);
            }
        }
    }
    if !caption.is_empty() {
        let color = if enabled { TEXT() } else { TEXT_DISABLED() };
        text(&p, english.x + 13.0 + 5.0, t + 10.0, &shown, color);
    }
    if cut {
        resp.on_hover_text(caption)
    } else {
        resp
    }
}

/// The gap a check box at English `x` leaves in a group box's top line for its caption (English
/// pixels): IDM's measured end (`english_end`) in English, else 2 px past the translated caption
/// (`light`: a [`checkbox_light`], caption at +16; else a [`checkbox`], at +18).
pub fn caption_gap(
    p: &Painter,
    x: f32,
    english_end: f32,
    caption: &str,
    light: bool,
) -> (f32, f32) {
    let shown = crate::i18n::t(caption);
    if shown == caption {
        return (x, english_end);
    }
    let lead = if light { 16.0 } else { 18.0 };
    (x, x + lead + text_width(p, shown) + 2.0)
}

/// Group box frame (`rc` on screen) with a gap in its top line for a checkbox caption (`gap` =
/// English x range, inclusive pixels).
pub fn group_box(p: &Painter, rc: Rect, gap: Option<(f32, f32)>) {
    let (l, t, rr, b) = (rc.left(), rc.top(), rc.right() - 1.0, rc.bottom() - 1.0);
    let gap = gap.map(|(g0, g1)| {
        let (a, b) = (mcol(g0), mcol(g1));
        (a.min(b), a.max(b))
    });
    match gap {
        Some((g0, g1)) => {
            hline(p, l, g0 - 1.0, t, GROUP_LINE);
            hline(p, g1 + 1.0, rr, t, GROUP_LINE);
        }
        None => hline(p, l, rr, t, GROUP_LINE),
    }
    vline(p, l, t, b, GROUP_LINE);
    vline(p, rr, t, b, GROUP_LINE);
    hline(p, l, rr, b, GROUP_LINE);
}

/// The 22 × 21 "+" / "..." buttons.
pub fn small_button(ui: &mut Ui, rc: Rect, glyph: &str) -> Response {
    button(ui, rc, glyph, ButtonKind::Normal, false, None)
}

// ---- light dialogs (Speed Limiter, Scheduler, Options) ------------------------------------
// notes/ui-spec/scheduler-and-misc.md "Light dialog control styles".

crate::theme::themed! {
    /// The "light dialogs" (Options, Scheduler, File Properties…): IDM draws them light in both
    /// modes; here they follow the mode, dark the dark dialogs' colours.
    pub LIGHT_BG = rgb(0x39, 0x39, 0x39), rgb(0xf0, 0xf0, 0xf0);
    pub LIGHT_TEXT = rgb(0xff, 0xff, 0xff), rgb(0x00, 0x00, 0x00);
    pub LIGHT_DISABLED = rgb(0x8a, 0x8a, 0x8a), rgb(0x6d, 0x6d, 0x6d);
    /// A text box's or a list's inside.
    pub FIELD = rgb(0x2b, 0x2b, 0x2b), rgb(0xff, 0xff, 0xff);
    /// A list's or a box's outline.
    pub FIELD_EDGE = rgb(0x5a, 0x5a, 0x5a), rgb(0x82, 0x87, 0x90);
    /// Grid lines between a list's rows, and header dividers.
    pub GRID = rgb(0x40, 0x40, 0x40), rgb(0xf0, 0xf0, 0xf0);
    pub HEADER_LINE = rgb(0x4e, 0x4e, 0x4e), rgb(0xe5, 0xe5, 0xe5);
    /// Etched separators: the shadow line and the highlight beside it.
    pub ETCH_SHADOW = rgb(0x22, 0x22, 0x22), rgb(0xa0, 0xa0, 0xa0);
    pub ETCH_LIGHT = rgb(0x50, 0x50, 0x50), rgb(0xff, 0xff, 0xff);
    /// A selected row in a list without the focus.
    pub SELECTED_INACTIVE = rgb(0x4a, 0x4a, 0x4a), rgb(0xd9, 0xd9, 0xd9);
    /// Error text.
    pub ERROR = rgb(0xff, 0x80, 0x80), rgb(0xc4, 0x2b, 0x1c);
    /// Secondary text (notes, sizes, versions).
    pub MUTED = rgb(0xa0, 0xa0, 0xa0), rgb(0x6d, 0x6d, 0x6d);
    /// Success text (a matching checksum).
    pub SUCCESS = rgb(0x6c, 0xcb, 0x5f), rgb(0x0f, 0x7b, 0x0f);
}

/// Light edit: the same Win11 border, white fill, black text.
pub fn edit_light(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    enabled: bool,
    align_right: bool,
) -> Response {
    edit_light_on(ui, rc, value, enabled, align_right, LIGHT_BG())
}

/// [`edit_light`] on a surface `parent` (a disabled edit shows it: `#f9f9f9` on a tab page).
pub fn edit_light_on(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    enabled: bool,
    align_right: bool,
    parent: Color32,
) -> Response {
    edit_light_with(ui, rc, value, enabled, align_right, false, parent)
}

fn edit_light_with(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    enabled: bool,
    align_right: bool,
    password: bool,
    parent: Color32,
) -> Response {
    edit_frame(ui.painter(), rc, enabled);
    let fill_c = if enabled { FIELD() } else { parent };
    fill(
        ui.painter(),
        rc.left() + 2.0,
        rc.top() + 2.0,
        rc.right() - 3.0,
        rc.bottom() - 3.0,
        fill_c,
    );
    let color = if enabled {
        LIGHT_TEXT()
    } else {
        LIGHT_DISABLED()
    };
    // A number keeps to the end (the left in RTL), text to the start.
    let align = if align_right {
        end_align()
    } else {
        start_align()
    };
    let resp = edit_text_with(ui, rc, value, enabled, password, 5.0, color, align);
    edit_focus_line(ui, &resp, rc);
    resp
}

/// A light password box: dots, and no Cut or Copy in its menu.
pub fn password_light(ui: &mut Ui, rc: Rect, value: &mut String, enabled: bool) -> Response {
    password_light_on(ui, rc, value, enabled, LIGHT_BG())
}

/// [`password_light`] on a surface `parent` (shown inside it while disabled).
pub fn password_light_on(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    enabled: bool,
    parent: Color32,
) -> Response {
    edit_light_with(ui, rc, value, enabled, false, true, parent)
}

/// A multi-line text box (lists of sites, notes) that wraps its text and scrolls, with the
/// shared frame, caret, focus line and edit menu. `id` keeps its scroll place.
pub fn multiline_light(ui: &mut Ui, rc: Rect, value: &mut String, id: &str) -> Response {
    edit_frame(ui.painter(), rc, true);
    ui.painter().rect_filled(rc.shrink(2.0), 0.0, FIELD());
    let inner = Rect::from_min_max(rc.min + vec2(4.0, 2.0), rc.max - vec2(3.0, 3.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(id));
    child.visuals_mut().text_cursor.stroke.color = LIGHT_TEXT();
    let resp = egui::ScrollArea::vertical()
        .id_salt((id, "scroll"))
        .max_height(inner.height())
        .show(&mut child, |ui| {
            let resp = ui.add(
                egui::TextEdit::multiline(value)
                    .font(tahoma())
                    .text_color(LIGHT_TEXT())
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(inner.width() - 14.0)
                    .desired_rows(3),
            );
            edit_menu(&resp, value, false, true);
            // Enter here is a new line, not the dialog's OK.
            if resp.has_focus() {
                take_key(ui, egui::Key::Enter);
            }
            resp
        })
        .inner;
    edit_focus_line(ui, &resp, rc);
    resp
}

/// Light push button: #fdfdfd fill, radius 3; border #d0d0d0 (bottom #bababa), default
/// #0078d4 (bottom #006bbe), disabled #e9e9e9 with #838383 text; dotted focus rect.
pub fn button_light(
    ui: &mut Ui,
    rc: Rect,
    label: &str,
    kind: ButtonKind,
    focus_rect: bool,
) -> Response {
    button_face(ui, rc, crate::i18n::t(label), kind, focus_rect)
}

/// A push button in the current theme (dark: IDM's dark buttons; light: Windows 11's), its
/// (translated) `label` with "&" before the access letter: the letter is underlined and
/// Alt+letter presses it. Hot and pressed as Windows 11 draws them: a pressed button's label
/// dims and its darker bottom edge flattens. `default_focus`: the dialog's default button, whose
/// focus rectangle shows while no other control has the keyboard focus; any control with the
/// focus shows it.
fn button_face(
    ui: &mut Ui,
    rc: Rect,
    label: &str,
    kind: ButtonKind,
    default_focus: bool,
) -> Response {
    let dark = crate::theme::dark();
    let enabled = kind != ButtonKind::Disabled;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(
        rc,
        widget_id(ui, ("button", label, rc.min.x as i32, rc.min.y as i32)),
        sense,
    );
    let state = State::of(&resp, enabled);
    let p = ui.painter();
    let face = button_colors(kind, state, dark);
    let rounding = if dark { 4.0 } else { 3.0 };
    p.rect_filled(rc, rounding, face.fill);
    p.rect_stroke(
        rc,
        rounding,
        Stroke::new(1.0, face.border),
        StrokeKind::Inside,
    );
    let inset = if dark { 4.0 } else { 3.0 };
    hline(
        p,
        rc.left() + inset,
        rc.right() - inset - 1.0,
        rc.bottom() - 1.0,
        face.bottom,
    );
    // "&Start now": the letter after '&' is underlined and Alt+letter presses it.
    let (shown, mnemonic) = match label.split_once('&') {
        Some((before, after)) if !after.is_empty() => (format!("{before}{after}"), Some(before)),
        _ => (label.to_owned(), None),
    };
    // A translation wider than the button ends in "..." and shows whole on hover (UI rule).
    let (shown, cut) = fit_label(p, &shown, rc);
    let mnemonic = mnemonic.filter(|_| !cut);
    let w = text_width(p, &shown);
    let x = (rc.center().x - w / 2.0 - if dark { 1.0 } else { 0.0 }).round();
    let baseline = rc.top()
        + if dark && rc.height() > 19.0 {
            14.0
        } else {
            13.0
        };
    // Centred in a rect on screen: placed by its left, not by an English pen.
    let drawn = text_at(p, tahoma(), x, baseline, &shown, face.text);
    let mut pressed_by_key = false;
    if let Some(before) = mnemonic
        && let Some(letter) = shown[before.len()..].chars().next()
    {
        // Underlined as the menus are (always dark; light once the keyboard is in use), while
        // Alt+letter always works.
        if crate::chrome::cues(ui.ctx()) {
            let (x0, x1) = char_span(p, &tahoma(), &shown, before.chars().count(), drawn);
            hline(
                p,
                x0,
                x1,
                baseline + if dark { 2.0 } else { 3.0 },
                face.text,
            );
        }
        pressed_by_key = enabled && mnemonic_pressed(ui, letter);
    }
    let resp = if pressed_by_key {
        let mut r = resp;
        r.flags.insert(egui::response::Flags::CLICKED);
        r
    } else {
        resp
    };
    if resp.has_focus() || (default_focus && ui.ctx().memory(|m| m.focused().is_none())) {
        focus_ring(ui.painter(), rc.shrink(2.0));
    }
    if cut {
        resp.on_hover_text(label.replace('&', ""))
    } else {
        resp
    }
}

/// A button's colours.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Face {
    fill: Color32,
    border: Color32,
    /// The bottom edge (a darker line at rest; flat, the border's own, while pressed).
    bottom: Color32,
    text: Color32,
}

/// A button's colours for `kind` in `state`: dark, IDM's dark buttons; light, Windows 11's.
fn button_colors(kind: ButtonKind, state: State, dark: bool) -> Face {
    let pressed = state == State::Pressed;
    let face = if dark {
        let (border, bottom) = match kind {
            ButtonKind::Normal => (rgb(0x9b, 0x9b, 0x9b), rgb(0x89, 0x89, 0x89)),
            ButtonKind::Default => (rgb(0xff, 0xff, 0xff), rgb(0xe5, 0xe5, 0xe5)),
            ButtonKind::Disabled => (rgb(0x40, 0x40, 0x40), rgb(0x40, 0x40, 0x40)),
        };
        Face {
            fill: match state {
                State::Pressed => rgb(0x26, 0x26, 0x26),
                State::Hot => rgb(0x45, 0x45, 0x45),
                _ => rgb(0x33, 0x33, 0x33),
            },
            border,
            bottom,
            text: match state {
                State::Disabled => rgb(0x80, 0x80, 0x80),
                State::Pressed => rgb(0xc8, 0xc8, 0xc8),
                _ => TEXT(),
            },
        }
    } else {
        let (border, bottom) = match kind {
            ButtonKind::Normal => (rgb(0xd0, 0xd0, 0xd0), rgb(0xba, 0xba, 0xba)),
            ButtonKind::Default => (rgb(0x00, 0x78, 0xd4), rgb(0x00, 0x6b, 0xbe)),
            ButtonKind::Disabled => (rgb(0xe9, 0xe9, 0xe9), rgb(0xe9, 0xe9, 0xe9)),
        };
        Face {
            fill: match state {
                State::Disabled => rgb(0xf9, 0xf9, 0xf9),
                State::Pressed => rgb(0xf5, 0xf5, 0xf5),
                State::Hot => rgb(0xf6, 0xf6, 0xf6),
                _ => rgb(0xfd, 0xfd, 0xfd),
            },
            border,
            bottom,
            text: match state {
                State::Disabled => rgb(0x83, 0x83, 0x83),
                State::Pressed => rgb(0x5d, 0x5d, 0x5d),
                _ => LIGHT_TEXT(),
            },
        }
    };
    if pressed {
        Face {
            bottom: face.border,
            ..face
        }
    } else {
        face
    }
}

/// Alt+`letter` was pressed (and is taken): Latin letters as egui keys, other scripts (Cyrillic,
/// Greek, Arabic…) as the text typed with Alt held.
pub fn mnemonic_pressed(ui: &Ui, letter: char) -> bool {
    let upper: String = letter.to_uppercase().collect();
    if let Some(key) = egui::Key::from_name(&upper)
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, key))
    {
        return true;
    }
    let lower: String = letter.to_lowercase().collect();
    ui.input_mut(|i| {
        let alt = i.modifiers.alt;
        let at = i
            .events
            .iter()
            .position(|e| matches!(e, egui::Event::Text(t) if alt && t.to_lowercase() == lower));
        at.map(|k| i.events.remove(k)).is_some()
    })
}

/// Enter, without a modifier, that no control took first: a dialog's default button. A control
/// that answers Enter itself (an open list, a multi-line box) takes it with [`take_key`] before
/// the dialog asks.
pub fn enter_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.is_none())
}

/// Escape that no control took first (an open list or picker closes itself): a dialog's Cancel.
pub fn escape_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.key_pressed(egui::Key::Escape))
}

/// Take `key` (without modifiers) for a control that answers it, so the dialog under it doesn't
/// too: Escape closes an open list, not the window; Enter in a multi-line box is a new line.
pub fn take_key(ui: &Ui, key: egui::Key) -> bool {
    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key))
}

/// A part clicked by the pointer that never takes the keyboard focus (egui's `click()` is also
/// a Tab stop): spinner arrows, a combo's arrow, scroll bars, toolbar and menu-bar items, rows
/// of a list that handles its own keys. Tab then only reaches real controls.
pub fn pointer_only() -> Sense {
    Sense::CLICK
}

/// [`pointer_only`] that can also be dragged (a scroll bar's thumb).
pub fn pointer_drag() -> Sense {
    Sense::CLICK | Sense::DRAG
}

/// The ring of `resp`'s control around `rc` while it has the keyboard focus.
pub fn show_focus(ui: &Ui, resp: &Response, rc: Rect) {
    if resp.has_focus() {
        focus_ring(ui.painter(), rc);
    }
}

/// The keyboard focus around `rc`: a dotted rectangle, every other pixel, in the theme's ink.
pub fn focus_ring(p: &Painter, rc: Rect) {
    let ink = if crate::theme::dark() {
        rgb(0xcc, 0xcc, 0xcc)
    } else {
        rgb(0x0d, 0x0d, 0x0d)
    };
    let (l, t, r, b) = (rc.left(), rc.top(), rc.right() - 1.0, rc.bottom() - 1.0);
    let mut x = l;
    while x <= r {
        px(p, x, t, ink);
        px(p, x, b, ink);
        x += 2.0;
    }
    let mut y = t;
    while y <= b {
        px(p, l, y, ink);
        px(p, r, y, ink);
        y += 2.0;
    }
}

/// Win11 light checkbox glyph (13 × 13) with its label at glyph.x + 16 (baseline glyph top + 10).
pub fn checkbox_light(
    ui: &mut Ui,
    glyph: Pos2,
    checked: &mut bool,
    caption: &str,
    enabled: bool,
) -> Response {
    checkbox_light_until(ui, glyph, checked, caption, enabled, None)
}

/// [`checkbox_light`] whose caption must end before English x `until` (a box after it on the
/// same row).
pub fn checkbox_light_until(
    ui: &mut Ui,
    glyph: Pos2,
    checked: &mut bool,
    caption: &str,
    enabled: bool,
    until: Option<f32>,
) -> Response {
    let caption = crate::i18n::t(caption);
    let p = ui.painter().clone();
    // A caption longer than its room ends in "..." at the edge, whole on hover (UI rule).
    let (shown, cut) = fit_caption_until(&p, glyph.x + 16.0, caption, until);
    let w = text_width(&p, &shown);
    // `glyph` is English; the glyph and its caption (after it) are placed on screen.
    let english = glyph;
    let hit = rect(Rect::from_min_size(english, vec2(16.0 + w + 2.0, 13.0)));
    let glyph = at(english, 13.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(
        hit,
        widget_id(ui, ("check-light", caption, glyph.x as i32, glyph.y as i32)),
        sense,
    );
    show_focus(ui, &resp, hit.expand(1.0));
    if resp.clicked() {
        *checked = !*checked;
    }
    let rc = Rect::from_min_size(glyph, vec2(13.0, 13.0));
    check_box_glyph(&p, rc, *checked, State::of(&resp, enabled));
    let color = if enabled {
        LIGHT_TEXT()
    } else {
        LIGHT_DISABLED()
    };
    text(&p, english.x + 16.0, glyph.y + 10.0, &shown, color);
    if cut {
        resp.on_hover_text(caption)
    } else {
        resp
    }
}

crate::theme::themed! {
    /// Tab page body.
    pub LIGHT_PAGE = rgb(0x40, 0x40, 0x40), rgb(0xf9, 0xf9, 0xf9);
    /// An unselected tab, and the page's shadow.
    pub TAB_UNSELECTED = rgb(0x33, 0x33, 0x33), rgb(0xf3, 0xf3, 0xf3);
    pub TAB_HOT = rgb(0x46, 0x46, 0x46), rgb(0xfd, 0xfd, 0xfd);
    CHEVRON = rgb(0xc8, 0xc8, 0xc8), rgb(0x6f, 0x6f, 0x6f);
    CHEVRON_DISABLED = rgb(0x6a, 0x6a, 0x6a), rgb(0xbe, 0xbe, 0xbe);
    /// Small controls' outline (spinners, icon buttons).
    CONTROL_EDGE = rgb(0x5a, 0x5a, 0x5a), rgb(0xd2, 0xd2, 0xd2);
}

/// A small control's face in `state` (spinner arrows, icon buttons): the shared tints over its
/// resting face.
fn subtle_or(state: State) -> Color32 {
    let dark = crate::theme::dark();
    match state {
        State::Disabled if dark => rgb(0x30, 0x30, 0x30),
        State::Disabled => rgb(0xf3, 0xf3, 0xf3),
        _ => subtle_fill(state, dark).unwrap_or(if dark {
            rgb(0x33, 0x33, 0x33)
        } else {
            rgb(0xfa, 0xfa, 0xfa)
        }),
    }
}

/// A Windows 11 check box's 13 × 13 box in `rc`, ticked or not, in `state`: the one look of
/// check boxes and of the check lists (Options → Browsers, Sounds).
pub fn check_box_glyph(p: &Painter, rc: Rect, checked: bool, state: State) {
    if checked {
        p.rect_filled(rc, 2.0, fluent_accent(state));
        p.line(
            vec![
                pos2(rc.left() + 3.0, rc.top() + 6.5),
                pos2(rc.left() + 5.5, rc.top() + 9.0),
                pos2(rc.left() + 10.0, rc.top() + 3.5),
            ],
            Stroke::new(1.4, fluent_mark()),
        );
    } else {
        let (fill_c, border) = fluent_box(state);
        if let Some(f) = fill_c {
            p.rect_filled(rc, 2.0, f);
        }
        p.rect_stroke(rc, 2.0, Stroke::new(1.0, border), StrokeKind::Inside);
    }
}

/// A checked Windows 11 check box's or radio's accent in `state`: lighter under the pointer,
/// lighter still while pressed, grey when disabled.
fn fluent_accent(state: State) -> Color32 {
    match (state, crate::theme::dark()) {
        (State::Disabled, false) => rgb(0xc3, 0xc3, 0xc3),
        (State::Hot, false) => rgb(0x19, 0x6e, 0xbf),
        (State::Pressed, false) => rgb(0x33, 0x7f, 0xc6),
        (_, false) => rgb(0x00, 0x5f, 0xb8),
        (State::Disabled, true) => rgb(0x5a, 0x5a, 0x5a),
        (State::Hot, true) => rgb(0x5a, 0xbc, 0xe8),
        (State::Pressed, true) => rgb(0x52, 0xa6, 0xcc),
        (_, true) => rgb(0x60, 0xcd, 0xff),
    }
}

/// The mark on a checked box or radio (white on the light accent's blue, dark on the dark's).
fn fluent_mark() -> Color32 {
    if crate::theme::dark() {
        rgb(0x00, 0x00, 0x00)
    } else {
        Color32::WHITE
    }
}

/// An unchecked Windows 11 check box's or radio's fill (none when disabled) and border in
/// `state`: darker fill under the pointer, darker still and a fainter border while pressed.
fn fluent_box(state: State) -> (Option<Color32>, Color32) {
    fluent_box_in(state, crate::theme::dark())
}

fn fluent_box_in(state: State, dark: bool) -> (Option<Color32>, Color32) {
    match (state, dark) {
        (State::Disabled, false) => (None, rgb(0xc3, 0xc3, 0xc3)),
        (State::Hot, false) => (Some(rgb(0xea, 0xea, 0xea)), rgb(0x62, 0x62, 0x62)),
        (State::Pressed, false) => (Some(rgb(0xdd, 0xdd, 0xdd)), rgb(0x9a, 0x9a, 0x9a)),
        (_, false) => (Some(rgb(0xf3, 0xf3, 0xf3)), rgb(0x62, 0x62, 0x62)),
        (State::Disabled, true) => (None, rgb(0x5a, 0x5a, 0x5a)),
        (State::Hot, true) => (Some(rgb(0x3a, 0x3a, 0x3a)), rgb(0xa0, 0xa0, 0xa0)),
        (State::Pressed, true) => (Some(rgb(0x26, 0x26, 0x26)), rgb(0x70, 0x70, 0x70)),
        (_, true) => (Some(rgb(0x2b, 0x2b, 0x2b)), rgb(0x9b, 0x9b, 0x9b)),
    }
}

/// Win11 light radio glyph (13 × 13) with its label at glyph.x + 16. The caller sets the
/// choice when the response is clicked.
pub fn radio_light(
    ui: &mut Ui,
    glyph: Pos2,
    selected: bool,
    caption: &str,
    enabled: bool,
) -> Response {
    radio_light_until(ui, glyph, selected, caption, enabled, None)
}

/// [`radio_light`] whose caption also stops before English x `until` (a control after it).
pub fn radio_light_until(
    ui: &mut Ui,
    glyph: Pos2,
    selected: bool,
    caption: &str,
    enabled: bool,
    until: Option<f32>,
) -> Response {
    let caption = crate::i18n::t(caption);
    let p = ui.painter().clone();
    // A caption longer than its room ends in "..." at the edge, whole on hover (UI rule).
    let (shown, cut) = fit_caption_until(&p, glyph.x + 16.0, caption, until);
    let w = text_width(&p, &shown);
    // `glyph` is English; the glyph and its caption (after it) are placed on screen.
    let english = glyph;
    let hit = rect(Rect::from_min_size(english, vec2(16.0 + w + 2.0, 13.0)));
    let glyph = at(english, 13.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(
        hit,
        widget_id(ui, ("radio-light", caption, glyph.x as i32, glyph.y as i32)),
        sense,
    );
    show_focus(ui, &resp, hit.expand(1.0));
    let c = glyph + vec2(6.5, 6.5);
    let state = State::of(&resp, enabled);
    if selected {
        p.circle_filled(c, 6.5, fluent_accent(state));
        // Windows 11's dot grows under the pointer and shrinks while pressed.
        let dot = match state {
            State::Hot => 3.1,
            State::Pressed => 2.1,
            _ => 2.6,
        };
        p.circle_filled(c, dot, fluent_mark());
    } else {
        let (fill_c, ring) = fluent_box(state);
        if let Some(f) = fill_c {
            p.circle_filled(c, 6.0, f);
        }
        p.circle_stroke(c, 6.0, Stroke::new(1.0, ring));
    }
    let color = if enabled {
        LIGHT_TEXT()
    } else {
        LIGHT_DISABLED()
    };
    text(&p, english.x + 16.0, glyph.y + 10.0, &shown, color);
    if cut {
        resp.on_hover_text(caption)
    } else {
        resp
    }
}

/// Etched horizontal separator in English rect `x, y, w × 2`: `#a0a0a0` over `#ffffff`, the
/// white line 1 px longer so it covers the far end.
pub fn etched_h(p: &Painter, x: f32, y: f32, w: f32) {
    let x = rect(Rect::from_min_size(pos2(x, y), vec2(w, 2.0))).left();
    hline(p, x, x + w - 2.0, y, ETCH_SHADOW());
    px(p, x + w - 1.0, y, ETCH_LIGHT());
    hline(p, x, x + w - 1.0, y + 1.0, ETCH_LIGHT());
}

/// Etched vertical separator in English rect `x, y, 2 × h`.
pub fn etched_v(p: &Painter, x: f32, y: f32, h: f32) {
    let x = rect(Rect::from_min_size(pos2(x, y), vec2(2.0, h))).left();
    vline(p, x, y, y + h - 2.0, ETCH_SHADOW());
    px(p, x, y + h - 1.0, ETCH_LIGHT());
    vline(p, x + 1.0, y, y + h - 1.0, ETCH_LIGHT());
}

/// [`fill`] in English pixels (inclusive): on screen in a mirrored surface.
pub fn fill_l(p: &Painter, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
    let (a, b) = (mcol(x0), mcol(x1));
    fill(p, a.min(b), y0, a.max(b), y1, c);
}

/// [`px`] at an English pixel.
pub fn px_l(p: &Painter, x: f32, y: f32, c: Color32) {
    px(p, mcol(x), y, c);
}

/// One tab of a light tab control: its x range (inclusive, from the client origin) when
/// selected and when not (Windows draws the selected tab wider).
pub struct LightTab {
    pub label: &'static str,
    pub sel: (f32, f32),
    pub unsel: (f32, f32),
}

/// A translated row of tabs, sized to their labels as Windows sizes them: each `pad` px either
/// side of its (translated) label, from English x `start`, `gap` px apart. A row longer than
/// `right` caps the widest tabs to share the room and cuts their labels with "...".
/// (x0, x1, label shown) per tab, English x.
pub fn tab_row(
    p: &Painter,
    labels: &[&str],
    start: f32,
    right: f32,
    gap: f32,
    pad: f32,
) -> Vec<(f32, f32, String)> {
    let widths: Vec<f32> = labels
        .iter()
        .map(|s| text_width(p, s).ceil() + 2.0 * pad)
        .collect();
    let room = right - start - gap * labels.len().saturating_sub(1) as f32;
    let cap = width_cap(&widths, room);
    let mut x0 = start;
    labels
        .iter()
        .zip(widths)
        .map(|(s, w)| {
            let w = w.min(cap);
            let shown = ellipsize(p, &tahoma(), s, w - 2.0 * pad);
            let tab = (x0, x0 + w, shown);
            x0 += w + gap;
            tab
        })
        .collect()
}

/// The largest whole width `c` with Σ min(wᵢ, c) ≤ `room` (infinite when everything fits): the
/// narrow ones keep their width, the wide ones share what is left.
pub fn width_cap(widths: &[f32], room: f32) -> f32 {
    let mut sorted = widths.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mut used = 0.0;
    for (k, w) in sorted.iter().enumerate() {
        let rest = (sorted.len() - k) as f32;
        if used + w * rest > room {
            return ((room - used) / rest).floor();
        }
        used += w;
    }
    f32::INFINITY
}

#[cfg(test)]
mod tab_row_tests {
    #[test]
    fn the_widest_share_what_is_left() {
        assert_eq!(super::width_cap(&[50.0, 60.0], 200.0), f32::INFINITY);
        // 40 keeps its width; 100 and 120 share the remaining 120.
        assert_eq!(super::width_cap(&[100.0, 40.0, 120.0], 160.0), 60.0);
    }
}

/// Light tab control in `rc` (client rect of the whole control): the pane and the tabs.
/// Returns the tab clicked this frame.
pub fn tabs_light(
    ui: &mut Ui,
    o: Pos2,
    rc: Rect,
    tabs: &[LightTab],
    selected: usize,
) -> Option<usize> {
    let p = ui.painter().clone();
    let border = HEADER_LINE();
    let shadow = TAB_UNSELECTED();
    // Pane: x rc.left ..= right − 3, y top + 20 ..= bottom − 2, then a 2 px / 1 px shadow.
    let (l, t, rr, b) = (
        rc.left(),
        rc.top() + 20.0,
        rc.right() - 3.0,
        rc.bottom() - 2.0,
    );
    fill(&p, l, t, rr, b, LIGHT_PAGE());
    hline(&p, l, rr, t, border);
    hline(&p, l, rr, b, border);
    vline(&p, l, t, b, border);
    vline(&p, rr, t, b, border);
    fill(&p, rr + 1.0, t, rr + 2.0, b + 1.0, shadow);
    hline(&p, l, rr + 2.0, b + 1.0, shadow);
    // (selected range, unselected range, label x, label shown), from the client origin.
    type Placed = ((f32, f32), (f32, f32), f32, String);
    let placed: Vec<Placed> = if tabs.iter().all(|t| crate::i18n::t(t.label) == t.label) {
        tabs.iter()
            .map(|t| (t.sel, t.unsel, t.sel.0 + 8.0, t.label.to_owned()))
            .collect()
    } else {
        // A translation: tabs sized to their labels from the first tab's place, within the pane
        // (the selected one 2 px wider each side).
        let labels: Vec<&str> = tabs.iter().map(|t| crate::i18n::t(t.label)).collect();
        let (start, left) = tabs.first().map_or((0.0, 0.0), |t| (t.unsel.0, t.sel.0));
        tab_row(&p, &labels, start, left + rc.width() - 3.0 - 2.0, 2.0, 8.0)
            .into_iter()
            .map(|(x0, x1, shown)| {
                let lx = x0 + ((x1 - x0 - text_width(&p, &shown)) / 2.0).floor();
                ((x0 - 2.0, x1 + 2.0), (x0, x1), lx, shown)
            })
            .collect()
    };
    let mut clicked = None;
    let mut draw = |i: usize, tab: &LightTab, sel: bool| {
        let (sel_x, unsel_x, label_x, shown) = &placed[i];
        let (x0, x1) = if sel { *sel_x } else { *unsel_x };
        // English pixel columns; the tabs run from the leading edge.
        let (a, b) = (mcol(o.x + x0), mcol(o.x + x1));
        let (x0, x1) = (a.min(b), a.max(b));
        let y0 = if sel { rc.top() } else { rc.top() + 2.0 };
        let hit = Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1.0, t));
        let resp = ui.interact(
            hit,
            widget_id(ui, ("tab-light", tab.label, x0 as i32)),
            Sense::click(),
        );
        show_focus(ui, &resp, hit.shrink(2.0));
        if resp.clicked() && !sel {
            clicked = Some(i);
        }
        let face = if sel {
            LIGHT_PAGE()
        } else if resp.hovered() {
            TAB_HOT()
        } else {
            shadow
        };
        fill(&p, x0, y0, x1, t, face);
        hline(&p, x0 + 1.0, x1 - 1.0, y0, border);
        vline(&p, x0, y0 + 1.0, t, border);
        vline(&p, x1, y0 + 1.0, t, border);
        if sel {
            // Open at the bottom: the pane's top line stops at the tab.
            hline(&p, x0 + 1.0, x1 - 1.0, t, LIGHT_PAGE());
        } else {
            hline(&p, x0, x1, t, border);
        }
        let base = if sel {
            rc.top() + 13.0
        } else {
            rc.top() + 15.0
        };
        let full = crate::i18n::t(tab.label);
        let drawn = text(&p, o.x + label_x, base, shown, LIGHT_TEXT());
        if shown != full {
            full_on_hover(ui, drawn, ("tab-light-label", i), full);
        }
    };
    for (i, tab) in tabs.iter().enumerate().filter(|(i, _)| *i != selected) {
        draw(i, tab, false);
    }
    if let Some(tab) = tabs.get(selected) {
        draw(selected, tab, true);
    }
    clicked
}

/// The 8 × 5 "v" of drop-downs, its top-left at `at`.
pub fn chevron_light(p: &Painter, at: Pos2, enabled: bool) {
    let c = if enabled {
        CHEVRON()
    } else {
        CHEVRON_DISABLED()
    };
    p.line(
        vec![
            at + vec2(0.5, 0.5),
            at + vec2(4.0, 4.0),
            at + vec2(7.5, 0.5),
        ],
        Stroke::new(1.2, c),
    );
}

/// The frame of a light drop-down (combo or date picker): rounded `#c8c8c8` border, `#fefefe`
/// fill; enabled ones darken on hover and take the accent border while open.
pub fn dropdown_frame_light(p: &Painter, rc: Rect, state: State) {
    // Drawn as a button (Windows 11 draws its drop-downs so): the same faces and states, in
    // the current theme; open, the accent border and no bottom edge (its list is out).
    let dark = crate::theme::dark();
    let open = state == State::Open;
    let kind = if state == State::Disabled {
        ButtonKind::Disabled
    } else {
        ButtonKind::Normal
    };
    let face = button_colors(kind, if open { State::Hot } else { state }, dark);
    let border = if open { accent() } else { face.border };
    let rounding = if dark { 4.0 } else { 3.0 };
    p.rect_filled(rc, rounding, face.fill);
    p.rect_stroke(rc, rounding, Stroke::new(1.0, border), StrokeKind::Inside);
    if !open {
        hline(
            p,
            rc.left() + 3.0,
            rc.right() - 4.0,
            rc.bottom() - 1.0,
            face.bottom,
        );
    }
}

/// The accent (Windows' blue): borders of open and focused controls.
pub fn accent() -> Color32 {
    if crate::theme::dark() {
        rgb(0x60, 0xcd, 0xff)
    } else {
        rgb(0x00, 0x78, 0xd4)
    }
}

/// Light drop-down list (CBS_DROPDOWNLIST): the chosen item and a chevron; a click opens the
/// list under it. Returns the item chosen this frame.
pub fn combo_light(
    ui: &mut Ui,
    rc: Rect,
    items: &[&str],
    selected: usize,
    enabled: bool,
    id: &str,
) -> Option<usize> {
    let key = widget_id(ui, ("combo-light", id));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rc, key.with("button"), sense);
    show_focus(ui, &resp, rc.shrink(3.0));
    let mut open = ui.data(|d| d.get_temp::<bool>(key).unwrap_or(false)) && enabled;
    if resp.clicked() {
        open = !open;
    }
    let p = ui.painter().clone();
    dropdown_frame_light(&p, rc, State::of(&resp, enabled).open_if(open));
    let color = if enabled {
        LIGHT_TEXT()
    } else {
        LIGHT_DISABLED()
    };
    // Items are shown in the interface language (data such as numbers passes through as is).
    let full = crate::i18n::t(items.get(selected).copied().unwrap_or(""));
    // Cut with "..." before the chevron when too long (a translation), whole on hover.
    let label = ellipsize(&p, &tahoma(), full, rc.width() - 5.0 - 19.0);
    let drawn = text_start(
        &p,
        tahoma(),
        lead(rc) + 5.0 * fwd(),
        rc.top() + 13.0,
        &label,
        color,
    );
    if label != full && !open {
        full_on_hover(ui, drawn, ("combo-light-label", id), full);
    }
    // The chevron at the trailing edge.
    let chevron_x = if rtl() {
        rc.left() + 5.0
    } else {
        rc.right() - 13.0
    };
    chevron_light(&p, pos2(chevron_x, rc.top() + 8.0), enabled);
    let mut chosen = None;
    if open {
        // The list: rows of 15 px under the combo, the hovered row in the accent colour.
        let row_h = 15.0;
        // As wide as the combo, or as its longest item (a translation): it grows from the
        // leading edge.
        let longest = items
            .iter()
            .map(|s| text_width(&p, crate::i18n::t(s)).ceil() + 8.0)
            .fold(rc.width(), f32::max);
        let left = if rtl() {
            rc.right() - longest
        } else {
            rc.left()
        };
        let list = Rect::from_min_size(
            pos2(left, rc.bottom()),
            vec2(longest, row_h * items.len() as f32 + 2.0),
        );
        let area = egui::Area::new(key.with("list"))
            .order(egui::Order::Foreground)
            .fixed_pos(list.min)
            .show(ui.ctx(), |ui| {
                let (_, lp) = ui.allocate_painter(list.size(), Sense::hover());
                lp.rect_filled(list, 0.0, FIELD());
                lp.rect_stroke(
                    list,
                    0.0,
                    Stroke::new(1.0, FIELD_EDGE()),
                    StrokeKind::Inside,
                );
                for (i, item) in items.iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(list.left() + 1.0, list.top() + 1.0 + row_h * i as f32),
                        vec2(list.width() - 2.0, row_h),
                    );
                    let r = ui.interact(row, key.with(("item", i)), pointer_only());
                    let hot = r.hovered() || (!ui.rect_contains_pointer(list) && i == selected);
                    if hot {
                        lp.rect_filled(row, 0.0, SELECTION);
                    }
                    let c = if hot { Color32::WHITE } else { LIGHT_TEXT() };
                    text_start(
                        &lp,
                        tahoma(),
                        lead(row) + 3.0 * fwd(),
                        row.top() + 11.0,
                        crate::i18n::t(item),
                        c,
                    );
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
            });
        let outside = ui.input(|i| i.pointer.any_pressed())
            && !area.response.contains_pointer()
            && !resp.contains_pointer();
        // An open list answers Escape and Enter itself (the dialog under it doesn't).
        if chosen.is_some()
            || outside
            || take_key(ui, egui::Key::Escape)
            || take_key(ui, egui::Key::Enter)
        {
            open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(key, open));
    chosen
}

/// Up-down buddy in `rc` (the right part of an edit): two `#fafafa` buttons with `#d2d2d2`
/// borders and 5 × 3 arrows. Returns +1 / −1 for a click (held: repeats), 0 otherwise.
pub fn updown_light(ui: &mut Ui, rc: Rect, enabled: bool, id: &str) -> i32 {
    let p = ui.painter().clone();
    let half = (rc.height() / 2.0).floor();
    let up = Rect::from_min_size(rc.min, vec2(rc.width(), half));
    let down = Rect::from_min_max(pos2(rc.left(), rc.top() + half), rc.max);
    let mut step = 0;
    for (i, part, dir) in [(0, up, 1), (1, down, -1)] {
        let sense = if enabled {
            pointer_only()
        } else {
            Sense::hover()
        };
        let r = ui.interact(part, widget_id(ui, ("updown-light", id, i)), sense);
        let face = if !enabled {
            subtle_or(State::Disabled)
        } else if r.is_pointer_button_down_on() {
            subtle_or(State::Pressed)
        } else if r.hovered() {
            subtle_or(State::Hot)
        } else {
            subtle_or(State::Normal)
        };
        let border = if enabled {
            CONTROL_EDGE()
        } else {
            HEADER_LINE()
        };
        p.rect_filled(part, 1.0, face);
        p.rect_stroke(part, 1.0, Stroke::new(1.0, border), StrokeKind::Inside);
        let c = part.center();
        let arrow = if enabled {
            LIGHT_TEXT()
        } else {
            CHEVRON_DISABLED()
        };
        let pts = if dir > 0 {
            vec![c + vec2(-3.0, 1.5), c + vec2(3.0, 1.5), c + vec2(0.0, -1.5)]
        } else {
            vec![
                c + vec2(-3.0, -1.5),
                c + vec2(3.0, -1.5),
                c + vec2(0.0, 1.5),
            ]
        };
        p.add(egui::Shape::convex_polygon(pts, arrow, Stroke::NONE));
        if r.clicked() {
            step = dir;
        }
    }
    step
}

/// A flat 19 × 21 button with a 16 × 16 icon (the Scheduler's move up / down / remove).
pub fn icon_button_light(ui: &mut Ui, rc: Rect, id: &str, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rc, widget_id(ui, ("icon-button-light", id)), sense);
    show_focus(ui, &resp, rc.shrink(2.0));
    let p = ui.painter();
    let face = subtle_or(State::of(&resp, enabled));
    p.rect_filled(rc, 2.0, face);
    p.rect_stroke(
        rc,
        2.0,
        Stroke::new(1.0, CONTROL_EDGE()),
        StrokeKind::Inside,
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pressed_button_looks_pressed_in_both_themes() {
        // Issue #7: pressed was #f5f5f5 against hot #f6f6f6, the same to the eye.
        for dark in [false, true] {
            for kind in [ButtonKind::Normal, ButtonKind::Default] {
                let rest = button_colors(kind, State::Normal, dark);
                let hot = button_colors(kind, State::Hot, dark);
                let pressed = button_colors(kind, State::Pressed, dark);
                assert_ne!(pressed.text, rest.text, "pressed dims the label");
                assert_eq!(
                    pressed.bottom, pressed.border,
                    "pressed flattens the bottom edge"
                );
                assert_ne!(
                    rest.bottom, rest.border,
                    "at rest the bottom edge is darker"
                );
                assert_ne!(hot.fill, rest.fill, "hot differs from rest");
            }
            let disabled = button_colors(ButtonKind::Disabled, State::Disabled, dark);
            let enabled = button_colors(ButtonKind::Normal, State::Normal, dark);
            let gap = |a: Color32, b: Color32| (a.r() as i32 - b.r() as i32).abs();
            assert!(
                gap(disabled.text, enabled.text) >= 0x50,
                "disabled text is clearly dimmer"
            );
        }
    }

    #[test]
    fn control_states_open_and_tint() {
        assert_eq!(State::Hot.open_if(true), State::Open);
        assert_eq!(State::Normal.open_if(true), State::Open);
        assert_eq!(State::Pressed.open_if(true), State::Pressed);
        assert_eq!(State::Disabled.open_if(true), State::Disabled);
        assert_eq!(State::Hot.open_if(false), State::Hot);
        for dark in [false, true] {
            assert_eq!(subtle_fill(State::Normal, dark), None);
            assert_eq!(subtle_fill(State::Disabled, dark), None);
            let grey = |s| subtle_fill(s, dark).unwrap().r();
            // Pressed (and open) is deeper than hot, on either surface.
            assert!(grey(State::Pressed) < grey(State::Hot));
            assert_eq!(
                subtle_fill(State::Open, dark),
                subtle_fill(State::Pressed, dark)
            );
        }
        assert!(
            highlight_fill(State::Pressed, false).unwrap().r()
                < highlight_fill(State::Hot, false).unwrap().r()
        );
        // Windows 11 boxes: deeper under the pointer, deeper still pressed.
        // Light: darker under the pointer, darker still pressed. Dark: lighter under the
        // pointer, darker pressed. Either way the three differ, and a disabled box is empty.
        let fill = |s, dark| fluent_box_in(s, dark).0.unwrap().r();
        assert!(
            fill(State::Pressed, false) < fill(State::Hot, false)
                && fill(State::Hot, false) < fill(State::Normal, false)
        );
        assert!(
            fill(State::Pressed, true) < fill(State::Normal, true)
                && fill(State::Normal, true) < fill(State::Hot, true)
        );
        for dark in [false, true] {
            assert_eq!(fluent_box_in(State::Disabled, dark).0, None);
        }
    }

    /// 6 px per character: a stand-in for the font.
    fn w(s: &str) -> f32 {
        6.0 * s.chars().count() as f32
    }

    fn limits_lines() -> [SentenceLine; 2] {
        // Options → Connection's Download limits, as IDM places them.
        let line = |lead_baseline: f32, y: f32, gap: f32, baseline: f32| SentenceLine {
            lead: Lead::Right(170.0),
            lead_baseline,
            boxes: pos2(178.0, y),
            gap,
            baseline,
        };
        [
            line(345.0, 330.0, 8.0, 346.0),
            line(366.0, 351.0, 11.0, 364.0),
        ]
    }

    const BOXES: [(&str, egui::Vec2); 2] = [
        ("mb", egui::Vec2::new(36.0, 20.0)),
        ("hours", egui::Vec2::new(36.0, 20.0)),
    ];

    #[test]
    fn pieces_split_at_the_boxes_only() {
        let rows = sentence_pieces("Get {n} of {mb} MB\n{hours} h", &["mb", "hours"]);
        assert_eq!(
            rows,
            vec![
                vec![
                    Piece::Text("Get {n} of"),
                    Piece::Slot("mb"),
                    Piece::Text("MB")
                ],
                vec![Piece::Slot("hours"), Piece::Text("h")],
            ]
        );
    }

    #[test]
    fn a_long_right_aligned_lead_stays_in_the_window() {
        let w = |s: &str| 6.0 * s.chars().count() as f32;
        // 240 px ending at 170 would start at -70: it starts at the window's edge instead and the
        // box moves right to make room.
        let template = format!("{} {{mb}} MB\nevery {{hours}} hours", "x".repeat(40));
        let (texts, rects) = sentence_layout(&template, &limits_lines(), &BOXES, w, 6.0);
        assert_eq!(texts[0].0, 6.0);
        assert_eq!(rects[0].min.x, 6.0 + 240.0 + 3.0);
        // The second line, short, keeps its English place.
        assert_eq!(rects[1].min.x, 178.0);
    }

    #[test]
    fn english_lands_where_the_pieces_were() {
        let (texts, rects) = sentence_layout(
            "Download no more than {mb} MBytes\nevery {hours} hours",
            &limits_lines(),
            &BOXES,
            w,
            f32::MIN,
        );
        let lead = w("Download no more than");
        assert_eq!(
            texts,
            vec![
                (170.0 - lead, 345.0, "Download no more than".to_owned()),
                (222.0, 346.0, "MBytes".to_owned()),
                (170.0 - w("every"), 366.0, "every".to_owned()),
                (225.0, 364.0, "hours".to_owned()),
            ]
        );
        assert_eq!(
            rects[0],
            Rect::from_min_size(pos2(178.0, 330.0), vec2(36.0, 20.0))
        );
        assert_eq!(
            rects[1],
            Rect::from_min_size(pos2(178.0, 351.0), vec2(36.0, 20.0))
        );
    }

    #[test]
    fn a_translation_moves_the_boxes() {
        // Boxes first on each line, the hours on the first line.
        let (texts, rects) =
            sentence_layout("{hours} h: {mb} MB", &limits_lines(), &BOXES, w, f32::MIN);
        assert_eq!(rects[1].min, pos2(178.0, 330.0));
        assert_eq!(texts[0], (222.0, 346.0, "h:".to_owned()));
        assert_eq!(rects[0].min, pos2(222.0 + w("h:") + 8.0, 330.0));
        assert_eq!(texts[1].0, rects[0].right() + 8.0);
    }

    #[test]
    fn a_long_lead_pushes_a_left_aligned_box() {
        let line = [SentenceLine {
            lead: Lead::Left(242.0),
            lead_baseline: 66.0,
            boxes: pos2(297.0, 53.0),
            gap: 5.0,
            baseline: 66.0,
        }];
        let files = [("files", vec2(39.0, 23.0))];
        let (_, rects) = sentence_layout("Download {files} files", &line, &files, w, f32::MIN);
        assert_eq!(rects[0].min.x, 297.0);
        let (_, rects) =
            sentence_layout("Herunterladen {files} Dateien", &line, &files, w, f32::MIN);
        assert_eq!(rects[0].min.x, 242.0 + w("Herunterladen") + 3.0);
    }

    /// A 400 px client area at x 100..500.
    fn mirrored() -> Surface {
        Surface::enter_if(true, 100.0, 500.0)
    }

    #[test]
    fn a_left_to_right_surface_changes_nothing() {
        let _s = Surface::enter_if(false, 100.0, 500.0);
        assert!(!rtl());
        let o = pos2(100.0, 50.0);
        assert_eq!(
            r(o, 10.0, 5.0, 30.0, 8.0),
            Rect::from_min_size(pos2(110.0, 55.0), vec2(30.0, 8.0))
        );
        assert_eq!((mx(110.0), mcol(110.0), fwd()), (110.0, 110.0, 1.0));
        assert_eq!(at(pos2(110.0, 7.0), 13.0), pos2(110.0, 7.0));
    }

    #[test]
    fn rects_are_made_at_their_mirrored_place() {
        let _s = mirrored();
        assert!(rtl());
        let o = pos2(100.0, 50.0);
        // x → L + R − x − w: 10 px from the left edge, 30 wide → ends 10 px from the right.
        let rc = r(o, 10.0, 5.0, 30.0, 8.0);
        assert_eq!(rc, Rect::from_min_size(pos2(460.0, 55.0), vec2(30.0, 8.0)));
        // Made twice is where it was: `rect` turns a screen rect back into its English one.
        assert_eq!(
            rect(rc),
            Rect::from_min_size(pos2(110.0, 55.0), vec2(30.0, 8.0))
        );
        // An edge, a pixel column, a point that starts something 13 wide.
        assert_eq!(mx(110.0), 490.0);
        assert_eq!(mcol(110.0), 489.0);
        assert_eq!(at(pos2(110.0, 7.0), 13.0), pos2(477.0, 7.0));
        // Leading edge and reading direction.
        assert_eq!(lead(rc), rc.right());
        assert_eq!(fwd(), -1.0);
        assert_eq!(start_align(), egui::Align::RIGHT);
        assert_eq!(end_align(), egui::Align::LEFT);
    }

    #[test]
    fn surfaces_nest_and_end() {
        {
            let _outer = mirrored();
            {
                // A popup's own surface, then a time picker's left-to-right inside.
                let _popup = Surface::enter_if(true, 0.0, 200.0);
                assert_eq!(mx(20.0), 180.0);
                let _ltr = Surface::enter_if(false, 0.0, 0.0);
                assert!(!rtl());
                assert_eq!(mx(20.0), 20.0);
            }
            assert_eq!(mx(110.0), 490.0);
        }
        assert!(!rtl());
        assert_eq!(mx(110.0), 110.0);
    }

    #[test]
    fn a_sentence_runs_from_the_right() {
        // Download limits as English places them, then mirrored: the lead text starts at the
        // right (its pen is mirrored by `text`), the boxes follow to its left.
        let (texts, rects) = sentence_layout(
            "Download no more than {mb} MBytes\nevery {hours} hours",
            &limits_lines(),
            &BOXES,
            w,
            f32::MIN,
        );
        let _s = Surface::enter_if(true, 0.0, 400.0);
        let shown: Vec<Rect> = rects.into_iter().map(rect).collect();
        assert_eq!(
            shown[0],
            Rect::from_min_size(pos2(400.0 - 178.0 - 36.0, 330.0), vec2(36.0, 20.0))
        );
        // The lead's English box ends at 170: on screen it starts there from the right, right of
        // the box.
        let lead = &texts[0];
        let lead_box = rect(Rect::from_min_size(
            pos2(lead.0, 0.0),
            vec2(w(&lead.2), 1.0),
        ));
        assert_eq!(lead_box.left(), 400.0 - 170.0);
        assert!(lead_box.left() > shown[0].right());
    }

    #[test]
    fn extra_lines_join_the_last_and_a_missing_box_still_shows() {
        let (texts, rects) = sentence_layout("a {mb}\nb\nc", &limits_lines(), &BOXES, w, f32::MIN);
        assert_eq!(texts.last().unwrap().2, "c");
        assert_eq!(texts.last().unwrap().1, 364.0);
        assert!(rects[1].min.x > 170.0);
    }
}

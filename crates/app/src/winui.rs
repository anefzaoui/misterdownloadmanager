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
    /// A greyed text box's text (dark: #8a8a8a, readable on the dark dialog; #6d6d6d was not).
    pub EDIT_TEXT_DISABLED = rgb(0x8a, 0x8a, 0x8a), rgb(0x6d, 0x6d, 0x6d);
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
    let drawn = text_font(p, tahoma(), x, baseline, s, color);
    note_caption(p, s, drawn);
    drawn
}

/// The caption painted last in this window (text, where, which pass): the next text box or
/// drop-down beside or under it takes it as its name for screen readers ([`caption_for`]).
fn caption_key(ctx: &egui::Context) -> egui::Id {
    egui::Id::unique(("last-caption", ctx.viewport_id()))
}

fn note_caption(p: &Painter, s: &str, drawn: Rect) {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > 80 {
        return;
    }
    let ctx = p.ctx();
    // The key first: asking the context inside its lock deadlocks.
    let (key, pass) = (caption_key(ctx), ctx.cumulative_pass_nr());
    ctx.data_mut(|d| d.insert_temp(key, (s.to_owned(), drawn, pass)));
}

/// The caption that names `control` (screen rect): painted this pass just before it, at its
/// leading side on the same row or right above it. Taken once.
fn caption_for(ctx: &egui::Context, control: Rect) -> Option<String> {
    let key = caption_key(ctx);
    let (s, at, pass) = ctx.data(|d| d.get_temp::<(String, Rect, u64)>(key))?;
    if pass != ctx.cumulative_pass_nr() {
        return None;
    }
    // In English (a right-to-left window has its captions at the right).
    let (c, b) = (rect(control), rect(at));
    let same_row = b.right() <= c.left() + 4.0 && b.bottom() > c.top() && b.top() < c.bottom();
    let above = b.bottom() <= c.top() + 3.0 && b.bottom() >= c.top() - 24.0 && b.left() < c.right();
    if !(same_row || above) {
        return None;
    }
    ctx.data_mut(|d| d.remove::<(String, Rect, u64)>(key));
    Some(s.trim_end_matches(':').trim().replace('&', ""))
}

/// A text box's or drop-down's name for screen readers, from its caption ([`caption_for`]).
fn name_from_caption(resp: &Response) {
    if let Some(name) = caption_for(&resp.ctx, resp.rect) {
        resp.ctx
            .accesskit_node_builder(resp.id, |node| node.set_label(name));
    }
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
    let area_resp = ui.interact(rect(area), id.with("area"), pointer_only());
    if area_resp.hovered() {
        top -= ui.input(|i| i.smooth_scroll_delta.y) / 40.0 * 3.0;
    }
    // The whole text to the clipboard from its right-click menu.
    copy_menu(&area_resp, s);
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

// ---- row list -------------------------------------------------------------------------------

/// Where a [`row_list`] is: its first row shown and its selected row (kept by the caller).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowList {
    pub first: usize,
    pub selected: Option<usize>,
    /// The selection was set from outside (typing, the caller): scrolled into view next draw.
    pending: bool,
    /// Wheel travel not yet a whole row.
    wheel: f32,
}

impl RowList {
    /// Typing selects the row whose `label` starts with what was typed ([`typed_match`]);
    /// call after [`row_list`] while it has the keys (`RowEvents::focused`).
    pub fn type_to(
        &mut self,
        ui: &Ui,
        list: &str,
        n: usize,
        label: impl Fn(usize) -> String,
    ) -> bool {
        let key = widget_id(ui, ("row-list-typed", list));
        match typed_match(ui, key, n, self.selected, label) {
            Some(i) => {
                self.selected = Some(i);
                self.pending = true;
                true
            }
            None => false,
        }
    }

    /// Scroll so row `i` shows (whole), `page` rows being visible.
    pub fn show_row(&mut self, i: usize, page: usize) {
        if i < self.first {
            self.first = i;
        } else if i >= self.first + page {
            self.first = i + 1 - page;
        }
    }
}

/// What happened in a [`row_list`] this frame (row indexes).
#[derive(Default, Debug)]
pub struct RowEvents {
    /// The selection moved to this row (a click, the keys).
    pub picked: Option<usize>,
    pub double_clicked: Option<usize>,
    /// Enter on the selected row (not taken: [`take_key`] it to keep it from the dialog).
    pub entered: Option<usize>,
    /// Space on the selected row (a list of check boxes ticks it).
    pub space: Option<usize>,
    /// Delete on the selected row.
    pub delete: Option<usize>,
    /// Right-click on a row (selected first), at this screen point.
    pub context: Option<(usize, Pos2)>,
    /// The list has the keys (a tree adds Left / Right).
    pub focused: bool,
}

/// How [`row_list`] drew a row's background: the row's text goes in `ink`.
pub struct RowLook {
    pub selected: bool,
    pub ink: Color32,
}

/// The one list of whole rows in a box (a dialog's list, a tree drawn as rows): its frame,
/// rows scrolled whole by the wheel (travel kept until it makes a row) and by a scroll bar
/// when they don't all fit, the row under the pointer tinted, the selected row in the
/// selection colour (grey while the list hasn't the keys), and the keys while it has them:
/// Up, Down, Home, End, Page Up, Page Down move the selection; Enter, Space and Delete are
/// reported. `rc` is where the box is on screen ([`r`]); `paint` draws row `i`'s contents in
/// its English rect, after its background, and may put controls in it (they take their own
/// clicks).
pub fn row_list(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rc: Rect,
    n: usize,
    row_h: f32,
    st: &mut RowList,
    mut paint: impl FnMut(&mut Ui, &Painter, usize, Rect, &RowLook),
) -> RowEvents {
    let mut ev = RowEvents::default();
    let id = widget_id(ui, ("row-list", id));
    let p = ui.painter().clone();
    solid(&p, rc, FIELD_EDGE());
    p.rect_filled(rc.shrink(1.0), 0.0, FIELD());
    // Laid out in English, made where it is on screen.
    let inner = rect(rc).shrink(1.0);
    let page = (((inner.height() - 2.0) / row_h).floor() as usize).max(1);
    let max_first = n.saturating_sub(page);
    let bar = n > page;
    let right = inner.right() - if bar { crate::scrollbar::THICK } else { 0.0 };
    let area = Rect::from_min_max(inner.min, pos2(right, inner.bottom()));
    let resp = ui.interact(rect(inner), id, Sense::click());
    expose(&resp, egui::Role::ListBox, "", true, None);
    // Its arrows (a tree's Left / Right too) stay with it.
    keep_keys(ui, &resp, true, true, false);
    let focused = resp.has_focus();
    // The wheel: whole rows, the rest kept for the next turn of it.
    if ui.rect_contains_pointer(rect(inner)) {
        st.wheel -= ui.input(|i| i.smooth_scroll_delta.y) / row_h;
        let whole = st.wheel.trunc();
        st.wheel -= whole;
        st.first = (st.first as f32 + whole).max(0.0) as usize;
    }
    if focused && n > 0 {
        let at = st.selected.map_or(-1, |s| s as isize);
        let last = n as isize - 1;
        let step = page as isize;
        let keys = [
            (egui::Key::ArrowDown, at + 1),
            (egui::Key::ArrowUp, (at - 1).max(0)),
            (egui::Key::Home, 0),
            (egui::Key::End, last),
            // As Explorer: first to the last (first) row in view, then a page on.
            (
                egui::Key::PageDown,
                if at < (st.first + page) as isize - 1 {
                    (st.first + page) as isize - 1
                } else {
                    at + step
                },
            ),
            (
                egui::Key::PageUp,
                if at > st.first as isize {
                    st.first as isize
                } else {
                    at - step
                },
            ),
        ];
        for (key, to) in keys {
            if take_key(ui, key) {
                let to = to.clamp(0, last) as usize;
                st.selected = Some(to);
                st.show_row(to, page);
                ev.picked = Some(to);
            }
        }
        if let Some(s) = st.selected {
            // Reported, not taken: a list that ignores it leaves Enter to the dialog's
            // default button (a caller that acts on it takes it).
            if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.is_none()) {
                ev.entered = Some(s);
            }
            if take_key(ui, egui::Key::Space) {
                ev.space = Some(s);
            }
            if take_key(ui, egui::Key::Delete) {
                ev.delete = Some(s);
            }
        }
    }
    if std::mem::take(&mut st.pending)
        && let Some(s) = st.selected
    {
        st.show_row(s, page);
    }
    st.first = st.first.min(max_first);
    ev.focused = focused;
    let clip = p.with_clip_rect(rect(area));
    for i in st.first..n.min(st.first + page) {
        let top = inner.top() + 1.0 + row_h * (i - st.first) as f32;
        let row = Rect::from_min_max(pos2(inner.left(), top), pos2(right, top + row_h));
        let r = ui.interact(rect(row), id.with(i), pointer_only());
        let selected = st.selected == Some(i);
        expose(&r, egui::Role::ListBoxOption, "", true, Some(selected));
        let fill = if selected && focused {
            Some(ROW_SELECTED())
        } else if selected {
            Some(SELECTED_INACTIVE())
        } else {
            // Hot, and pressed lighter still on a dark field (subtle_fill's pressed grey was the
            // dark field's own).
            match (State::of(&r, true), crate::theme::dark()) {
                (State::Pressed, true) => Some(rgb(0x4f, 0x4f, 0x4f)),
                (state, dark) => subtle_fill(state, dark),
            }
        };
        if let Some(f) = fill {
            clip.rect_filled(rect(row), 0.0, f);
        }
        if r.clicked() || r.secondary_clicked() || r.double_clicked() {
            resp.request_focus();
            if st.selected != Some(i) {
                st.selected = Some(i);
                ev.picked = Some(i);
            }
        }
        if r.double_clicked() {
            ev.double_clicked = Some(i);
        }
        if r.secondary_clicked()
            && let Some(at) = r.interact_pointer_pos()
        {
            ev.context = Some((i, at));
        }
        let look = RowLook {
            selected,
            ink: if selected && focused {
                Color32::WHITE
            } else {
                TEXT()
            },
        };
        paint(ui, &clip, i, row, &look);
        if selected && focused {
            focus_ring(&clip, rect(row));
        }
    }
    if focused
        && st
            .selected
            .is_none_or(|s| s < st.first || s >= st.first + page)
    {
        focus_ring(&p, rect(area));
    }
    if bar {
        let track = Rect::from_min_max(pos2(right, inner.top()), inner.max);
        let mut pos = st.first as f32;
        if crate::scrollbar::bar(
            ui,
            id.with("bar"),
            rect(track),
            true,
            n as f32,
            page as f32,
            1.0,
            &mut pos,
        ) {
            st.first = (pos.round().max(0.0) as usize).min(max_first);
        }
    }
    ev
}

/// Text that opens or does something when clicked: link colour, underlined under the pointer.
/// `label` is English (translated here).
pub fn link(ui: &Ui, p: &Painter, x: f32, baseline: f32, label: &str, salt: &str) -> bool {
    let rect = text(p, x, baseline, crate::i18n::t(label), LINK());
    let resp = ui
        .interact(rect, ui.scope_id().with(("link", salt)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    expose(&resp, egui::Role::Link, crate::i18n::t(label), true, None);
    // A Tab stop shows that it has the keys.
    show_focus(ui, &resp, rect.expand(2.0));
    if resp.hovered() {
        let y = rect.bottom() + 0.5;
        p.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            egui::Stroke::new(1.0, LINK()),
        );
    }
    resp.clicked()
}

/// A disabled control says why on hover: `reason` (English, translated here) while `disabled`.
pub fn why_disabled(resp: Response, disabled: bool, reason: &str) -> Response {
    if disabled {
        resp.on_hover_text(crate::i18n::t(reason))
    } else {
        resp
    }
}

/// [`text_fit`] that shows the whole text on hover whenever it cut it: the one way to draw a
/// line that may not fit (no caller can forget the hover). `salt` tells the lines apart.
#[allow(clippy::too_many_arguments)]
pub fn fit(
    ui: &Ui,
    p: &Painter,
    x: f32,
    baseline: f32,
    s: &str,
    color: Color32,
    right: f32,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> Rect {
    let (rect, cut) = text_fit(p, x, baseline, s, color, right);
    if cut {
        full_on_hover(ui, rect, salt, s);
    }
    rect
}

/// Text already drawn in `rect` that the user may want to copy (an address, a hash, a path, a
/// version): its right-click menu copies it whole ([`copy_menu`]), and it shows whole on hover
/// when `cut`. Taken by the pointer only (labels are no Tab stops).
pub fn copyable(
    ui: &Ui,
    rect: Rect,
    salt: impl std::hash::Hash + std::fmt::Debug,
    text: &str,
    cut: bool,
) -> Response {
    let resp = ui.interact(rect, ui.scope_id().with(("copyable", salt)), pointer_only());
    copy_menu(&resp, text);
    if cut { resp.on_hover_text(text) } else { resp }
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
    mesh.add_colored_rect(snap(p.pixels_per_point(), rc), c);
    p.add(egui::Shape::mesh(mesh));
}

/// `rc` on whole screen pixels at `ppp` pixels a point (125 %, 150 % scaling put a 1 pt line
/// across two pixels, blurred or uneven), never thinner than one pixel. At 100 % it is `rc`.
fn snap(ppp: f32, rc: Rect) -> Rect {
    let round = |v: f32| (v * ppp).round() / ppp;
    let (x0, y0) = (round(rc.min.x), round(rc.min.y));
    let one = 1.0 / ppp;
    let x1 = round(rc.max.x).max(x0 + if rc.width() > 0.0 { one } else { 0.0 });
    let y1 = round(rc.max.y).max(y0 + if rc.height() > 0.0 { one } else { 0.0 });
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
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

/// The rounding of a control's corners (buttons, text boxes, drop-downs): IDM's dark buttons,
/// Windows 11's light ones.
pub fn control_radius() -> f32 {
    if crate::theme::dark() { 4.0 } else { 3.0 }
}

/// A text box's frame. Light: Windows 11's, rounded as the buttons are, a subtle border with a
/// darker bottom line (flat when disabled). Dark: IDM's 2 px light border around a dark fill
/// (height 23), its corner pixels measured.
pub fn edit_frame(p: &Painter, rc: Rect, enabled: bool) {
    if !crate::theme::dark() {
        let radius = control_radius();
        let (fill_c, border, bottom) = if enabled {
            (Color32::WHITE, rgb(0xe5, 0xe5, 0xe5), rgb(0x86, 0x86, 0x86))
        } else {
            (
                rgb(0xf9, 0xf9, 0xf9),
                rgb(0xe9, 0xe9, 0xe9),
                rgb(0xe9, 0xe9, 0xe9),
            )
        };
        p.rect_filled(rc, radius, fill_c);
        p.rect_stroke(rc, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        hline(
            p,
            rc.left() + radius,
            rc.right() - radius - 1.0,
            rc.bottom() - 1.0,
            bottom,
        );
        return;
    }
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
        focus_bottom(ui.painter(), rc);
    }
}

/// The 2 px accent along a focused box's bottom edge. Light: the bottom of its rounded
/// frame, curving into the corners as Windows 11 draws it; dark: IDM's straight line.
fn focus_bottom(p: &Painter, rc: Rect) {
    if crate::theme::dark() {
        fill(
            p,
            rc.left() + 1.0,
            rc.bottom() - 2.0,
            rc.right() - 2.0,
            rc.bottom() - 1.0,
            accent(),
        );
    } else {
        let strip = Rect::from_min_max(pos2(rc.left(), rc.bottom() - 2.0), rc.max);
        p.with_clip_rect(strip)
            .rect_filled(rc, control_radius(), accent());
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
    // The whole drawn box takes clicks (the text itself fills only a strip inside it): first,
    // so the text on top still gets its own; a click elsewhere in the box focuses it.
    // Keyed by its window's opening too: a box shown again for another record (New login after
    // Edit login) starts without the last one's text in its undo.
    // (The key first: asking the context inside its own lock deadlocks.)
    let opening = crate::dialogs::opening_key(ui.ctx());
    let opened = ui.data(|d| d.get_temp::<u64>(opening)).unwrap_or(0);
    let edit_id = widget_id(ui, ("edit-box", rc.min.x as i32, rc.min.y as i32, opened));
    if enabled {
        let frame = ui.interact(rc, edit_id.with("frame"), pointer_only());
        if frame.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if frame.clicked() {
            ui.memory_mut(|m| m.request_focus(edit_id));
        }
    }
    let te = egui::TextEdit::singleline(value)
        .id(edit_id)
        .align(egui::Align2([align, egui::Align::TOP]))
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(tahoma())
        .text_color(color)
        .password(password)
        // Enter keeps the focus in the box (the dialog's default button answers it; a box that
        // lost the keys on Enter took the next typing nowhere).
        .return_key(None)
        .desired_width(inner.width());
    if ui.memory(|m| m.has_focus(edit_id)) {
        ui.input_mut(|i| {
            let mut events = Vec::with_capacity(i.events.len());
            for e in i.events.drain(..) {
                match e {
                    // Ctrl+V: one clean line (see single_line).
                    egui::Event::Paste(t) => events.push(egui::Event::Paste(single_line(&t))),
                    // A password's word moves would show where its words end: whole-box
                    // moves instead (Ctrl+Left / Right to its ends, Ctrl+Backspace / Delete
                    // to them).
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        physical_key,
                        repeat,
                    } if password
                        && modifiers.command
                        && matches!(
                            key,
                            egui::Key::ArrowLeft
                                | egui::Key::ArrowRight
                                | egui::Key::Backspace
                                | egui::Key::Delete
                        ) =>
                    {
                        let plain = egui::Modifiers {
                            shift: modifiers.shift,
                            ..Default::default()
                        };
                        let select = egui::Modifiers::SHIFT;
                        let press = |key, modifiers| egui::Event::Key {
                            key,
                            physical_key,
                            pressed: true,
                            repeat,
                            modifiers,
                        };
                        match key {
                            egui::Key::ArrowLeft => events.push(press(egui::Key::Home, plain)),
                            egui::Key::ArrowRight => events.push(press(egui::Key::End, plain)),
                            egui::Key::Backspace => {
                                events.push(press(egui::Key::Home, select));
                                events.push(press(egui::Key::Backspace, Default::default()));
                            }
                            _ => {
                                events.push(press(egui::Key::End, select));
                                events.push(press(egui::Key::Delete, Default::default()));
                            }
                        }
                    }
                    other => events.push(other),
                }
            }
            i.events = events;
        });
    }
    let resp = ui.put(inner, |ui: &mut Ui| {
        // The caret in the box's own ink: egui's default (a pale blue in its dark style) was
        // all but invisible on a light box.
        ui.visuals_mut().text_cursor.stroke.color = color;
        ui.add_enabled_ui(enabled, |ui| ui.add(te)).inner
    });
    // Named for screen readers by its caption.
    name_from_caption(&resp);
    // Tab into a box selects its text, as in Windows (a click places the caret instead).
    if resp.gained_focus() && !ui.input(|i| i.pointer.any_down()) {
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
        let n = value.chars().count();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(n),
            )));
        state.store(ui.ctx(), edit_id);
    }
    if enabled {
        edit_menu(&resp, value, password, false);
    }
    resp
}

#[derive(Clone, Copy)]
enum EditCmd {
    Undo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
}

/// A text box's right-click menu: Cut, Copy, Paste, Delete and Select all, on the box's own
/// selection, as every Windows, macOS and Linux text box has (Cut and Copy not for passwords). A
/// menu window of its own (`chrome::popup`), so short dialogs never cut it off.
/// A right-click (or Shift+F10) menu with Copy for read-only text `text` shown by `resp` (a
/// value, a version, an address): the whole text goes to the clipboard. Ctrl+C does the same
/// while `resp` has the focus.
pub fn copy_menu(resp: &Response, text: &str) {
    use crate::chrome::{MenuEntry, PopupResult};
    let ctx = resp.ctx.clone();
    let key = resp.id.with("copy-menu");
    if resp.has_focus() && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
        ctx.copy_text(text.to_owned());
    }
    if resp.secondary_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        ctx.data_mut(|d| d.insert_temp(key, p));
    }
    if resp.has_focus() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10))
    {
        ctx.data_mut(|d| d.insert_temp(key, resp.rect.left_bottom()));
    }
    let Some(at) = ctx.data(|d| d.get_temp::<Pos2>(key)) else {
        return;
    };
    let entries = [MenuEntry::Item("&Copy", (), !text.is_empty(), false)];
    match crate::chrome::popup(&ctx, &format!("copy-{:?}", resp.id), at, &entries, &|_| {
        None
    }) {
        PopupResult::Open | PopupResult::Neighbor(_) => {}
        PopupResult::Chosen(()) => {
            ctx.copy_text(text.to_owned());
            ctx.data_mut(|d| d.remove::<Pos2>(key));
        }
        PopupResult::Dismissed => ctx.data_mut(|d| d.remove::<Pos2>(key)),
    }
}

/// A read-only value in a box (a path, an address): cut to fit with "..." and whole on hover,
/// on `fill`; it takes the focus (Tab), and Ctrl+C or its right-click menu copies the whole
/// value ([`copy_menu`]). `ltr`: the value reads left to right in every language (paths,
/// addresses keep their "..." at their end in a right-to-left interface).
pub fn readonly_field(
    ui: &mut Ui,
    rc: Rect,
    value: &str,
    fill: Color32,
    ltr: bool,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> Response {
    edit_frame(ui.painter(), rc, true);
    ui.painter().rect_filled(rc.shrink(2.0), 0.0, fill);
    let p = ui.painter().clone();
    // From the field's leading edge (an English pen in its English rect).
    let e = rect(rc);
    let shown = ellipsize(&p, &tahoma(), value, e.width() - 9.0);
    let cut = shown != value;
    let shown = if ltr && rtl() {
        crate::i18n::ltr(&shown)
    } else {
        shown
    };
    text(&p, e.left() + 5.0, e.top() + 15.0, &shown, LIGHT_TEXT());
    let resp = ui.interact(rc, widget_id(ui, ("readonly", id)), Sense::click());
    focus_on_press(&resp);
    expose(&resp, egui::Role::TextInput, value, true, None);
    show_focus(ui, &resp, rc.shrink(2.0));
    copy_menu(&resp, value);
    if cut { resp.on_hover_text(value) } else { resp }
}

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
    // egui's own undo history (Ctrl+Z uses it too).
    let mut undoer = state.undoer();
    let now = (state.cursor.char_range().unwrap_or_default(), value.clone());
    let entries = vec![
        MenuEntry::Item("&Undo", EditCmd::Undo, undoer.has_undo(&now), false),
        MenuEntry::Sep,
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
        EditCmd::Undo => {
            let Some((range, text)) = undoer.undo(&now).cloned() else {
                return;
            };
            *value = text;
            state.set_undoer(undoer);
            state.cursor.set_char_range(Some(range));
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            return;
        }
        // Each change an undo step of its own (Undo after the menu's Paste did nothing).
        EditCmd::Cut => {
            ctx.copy_text(value[ba..bb].to_owned());
            undoer.add_undo(&now);
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
                single_line(&text)
            };
            undoer.add_undo(&now);
            value.replace_range(ba..bb, &text);
            let end = a + text.chars().count();
            (end, end)
        }
        EditCmd::Delete => {
            undoer.add_undo(&now);
            value.replace_range(ba..bb, "");
            (a, a)
        }
        EditCmd::SelectAll => (0, value.chars().count()),
    };
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(caret.0),
        CCursor::new(caret.1),
    )));
    state.set_undoer(undoer);
    state.store(&ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// A dark edit with right-aligned text (numbers, e.g. the Speed Limiter value).
pub fn edit_right(ui: &mut Ui, rc: Rect, value: &mut String, enabled: bool) -> Response {
    edit_frame(ui.painter(), rc, enabled);
    let color = if enabled {
        TEXT()
    } else {
        EDIT_TEXT_DISABLED()
    };
    let resp = edit_text_with(ui, rc, value, enabled, false, 5.0, color, end_align());
    // Its focus shows as every text box's does.
    edit_focus_line(ui, &resp, rc);
    resp
}

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
        } else if resp.is_pointer_button_down_on() && resp.contains_pointer() {
            // Held, it looks pressed only while the pointer is on it (as in Windows: moved
            // off, it lets go without a click).
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

/// Combo frame. Dark: 1 px border (#8d8d8d, or #0078d4 when focused), 2 px white ring, dark
/// text area, white 19 px arrow zone with a grey chevron. Height 21.
pub fn combo_frame(p: &Painter, rc: Rect, focused: bool) {
    // Light: the text box's rounded frame (an accent border while focused), the chevron.
    if !crate::theme::dark() {
        edit_frame(p, rc, true);
        // Focused as a text box is: the accent along its bottom (an editable drop-down is one).
        if focused {
            focus_bottom(p, rc);
        }
        combo_chevron(p, rc);
        return;
    }
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
    let resp = opener_click(ui, resp);
    expose_value(&resp, egui::Role::ComboBox, value, true);
    combo_frame(ui.painter(), rc, resp.has_focus());
    // The arrow zone (always white) shows the state.
    tint_part(
        ui.painter(),
        combo_zone(rc),
        State::of(&resp, true),
        false,
        |p| combo_chevron(p, rc),
    );
    // Short of the arrow zone: a longer value ends in "..." and shows whole on hover.
    let shown = ellipsize(ui.painter(), &tahoma(), value, rc.width() - 5.0 - 22.0);
    text_start(
        ui.painter(),
        tahoma(),
        lead(rc) + 5.0 * fwd(),
        rc.top() + 13.0,
        &shown,
        TEXT(),
    );
    if shown != value {
        resp.on_hover_text(value)
    } else {
        resp
    }
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
    let resp = opener_click(ui, resp);
    let state = State::of(&resp, true).open_if(open);
    tint_part(ui.painter(), zone, state, false, |p| combo_chevron(p, rc));
    resp
}

/// The keys that open an editable combo's drop list while its box has the focus: Alt+Down or
/// F4, as Windows' combo boxes.
/// Up / Down in an editable drop-down's box (the list closed): the next / previous of its
/// `n` items, as Windows' editable combos step through theirs. `current`: the one the box
/// shows, if any. Returns the item to put in the box.
pub fn history_keys(ui: &Ui, edit: &Response, n: usize, current: Option<usize>) -> Option<usize> {
    if !edit.has_focus() || n == 0 {
        return None;
    }
    keep_keys(ui, edit, false, true, false);
    if take_key(ui, egui::Key::ArrowDown) {
        Some(current.map_or(0, |c| (c + 1).min(n - 1)))
    } else if take_key(ui, egui::Key::ArrowUp) {
        Some(current.map_or(0, |c| c.saturating_sub(1)))
    } else {
        None
    }
}

/// Text pasted into a one-line box: a trailing line break dropped, line breaks and tabs inside
/// it one space each, other control characters out ("url\r\n" was "url" and two spaces).
pub fn single_line(text: &str) -> String {
    let t = text.trim_end_matches(['\r', '\n']);
    let t = t.replace("\r\n", " ").replace(['\r', '\n', '\t'], " ");
    t.chars().filter(|c| !c.is_control()).collect()
}

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
    // Under the box's leading edge (its right one in a right-to-left interface, where menus
    // open leftwards from their top-right).
    let at = if rtl() {
        anchor.right_bottom()
    } else {
        anchor.left_bottom()
    };
    let menu = crate::chrome::popup(ui.ctx(), &format!("drop-{id}"), at, &entries, &|_| None);
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

/// A checkbox's caption drawn apart from it (two lines, a paragraph) in English `caption`: a
/// click on it ticks the box too, as on a caption the box draws itself.
pub fn caption_click(
    ui: &mut Ui,
    caption: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    checked: &mut bool,
    enabled: bool,
) -> Response {
    let sense = if enabled {
        pointer_only()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect(caption), widget_id(ui, ("caption", id)), sense);
    if resp.clicked() {
        *checked = !*checked;
    }
    resp
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
        // By place, not caption: a caption that changes keeps the focus.
        widget_id(ui, ("check", box_min.x as i32, box_min.y as i32)),
        sense,
    );
    let resp = enter_is_not_a_click(ui, resp);
    focus_on_press(&resp);
    show_focus(ui, &resp, hit.expand(1.0));
    if resp.clicked() {
        *checked = !*checked;
    }
    expose(
        &resp,
        egui::Role::CheckBox,
        caption,
        enabled,
        Some(*checked),
    );
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
    /// A selected row in a focused list ([`row_list`]; the download list's colours), white
    /// text on it.
    pub ROW_SELECTED = rgb(0x2d, 0x4b, 0x63), rgb(0x00, 0x78, 0xd7);
    /// Text that opens something ([`link`]).
    pub LINK = rgb(0x99, 0xc8, 0xff), rgb(0x00, 0x66, 0xcc);
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

/// A whole number in `min..=max` typed in a box (the one number box of the dialogs): digits
/// only, no longer than `max` has; Up / Down step it by 1, Page Up / Down by 10; out of range,
/// its frame is red while typed and it comes back into range, visibly, when the box is left.
/// [`number_in`] reads it.
#[allow(clippy::too_many_arguments)]
pub fn number_field(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    min: u64,
    max: u64,
    enabled: bool,
    parent: Color32,
) -> Response {
    let resp = edit_light_on(ui, rc, value, enabled, false, parent);
    value.retain(|c| c.is_ascii_digit());
    value.truncate(max.to_string().len());
    if resp.has_focus() {
        keep_keys(ui, &resp, false, true, false);
        let n = value.parse::<u64>().unwrap_or(min) as i64;
        let step = if take_key(ui, egui::Key::ArrowUp) {
            1
        } else if take_key(ui, egui::Key::ArrowDown) {
            -1
        } else if take_key(ui, egui::Key::PageUp) {
            10
        } else if take_key(ui, egui::Key::PageDown) {
            -10
        } else {
            0
        };
        if step != 0 {
            *value = (n + step).clamp(min as i64, max as i64).to_string();
        }
    }
    if resp.lost_focus() {
        *value = number_in(value, min, max).to_string();
    }
    let ok = value.parse::<u64>().is_ok_and(|n| (min..=max).contains(&n));
    if enabled && !ok {
        ui.painter().rect_stroke(
            rc,
            control_radius(),
            Stroke::new(1.0, ERROR()),
            StrokeKind::Inside,
        );
    }
    resp
}

/// The number in a [`number_field`], brought into `min..=max` (empty: `min`).
pub fn number_in(value: &str, min: u64, max: u64) -> u64 {
    value.parse::<u64>().unwrap_or(min).clamp(min, max)
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
/// `enabled`: off, it is greyed and can't be typed in (its switch is off).
pub fn multiline_light(
    ui: &mut Ui,
    rc: Rect,
    value: &mut String,
    id: &str,
    enabled: bool,
) -> Response {
    edit_frame(ui.painter(), rc, enabled);
    if enabled {
        ui.painter().rect_filled(rc.shrink(2.0), 0.0, FIELD());
    }
    let ink = if enabled {
        LIGHT_TEXT()
    } else {
        LIGHT_DISABLED()
    };
    let inner = Rect::from_min_max(rc.min + vec2(4.0, 2.0), rc.max - vec2(3.0, 3.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(id));
    child.visuals_mut().text_cursor.stroke.color = LIGHT_TEXT();
    if !enabled {
        child.disable();
    }
    let resp = egui::ScrollArea::vertical()
        .id_salt((id, "scroll"))
        .max_height(inner.height())
        .show(&mut child, |ui| {
            let resp = ui.add(
                egui::TextEdit::multiline(value)
                    .font(tahoma())
                    .text_color(ink)
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
    // Keyed by its place, not its label: Pause becoming Resume keeps the focus.
    let resp = ui.interact(
        rc,
        widget_id(ui, ("button", rc.min.x as i32, rc.min.y as i32)),
        sense,
    );
    expose(
        &resp,
        egui::Role::Button,
        crate::i18n::t(label),
        enabled,
        None,
    );
    focus_on_press(&resp);
    // A focused button answers Enter itself (egui clicks it): the dialog's default button,
    // read after it, doesn't run as well.
    if resp.has_focus() {
        take_key(ui, egui::Key::Enter);
    }
    let state = State::of(&resp, enabled);
    let p = ui.painter();
    let face = button_colors(kind, state, dark);
    let rounding = control_radius();
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

/// Is the keyboard in use in the current window? A navigation key (Tab, the arrows, Space,
/// Enter, Home / End, Page Up / Down) turns it on, a mouse press off: focus rings show only
/// for a user moving with the keys, as Windows shows them.
pub fn keyboard_in_use(ctx: &egui::Context) -> bool {
    use egui::Key as K;
    let key = egui::Id::unique(("keyboard-in-use", ctx.viewport_id()));
    let (keys, press) = ctx.input(|i| {
        let nav = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: K::Tab
                        | K::ArrowUp
                        | K::ArrowDown
                        | K::ArrowLeft
                        | K::ArrowRight
                        | K::Space
                        | K::Enter
                        | K::Home
                        | K::End
                        | K::PageUp
                        | K::PageDown
                        | K::F6,
                    pressed: true,
                    ..
                }
            )
        });
        (nav, i.pointer.any_pressed())
    });
    let mut on = ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if keys {
        on = true;
    } else if press {
        on = false;
    }
    ctx.data_mut(|d| d.insert_temp(key, on));
    on
}

/// The keyboard focus around `rc`: a dotted rectangle, every other pixel, in the theme's ink.
pub fn focus_ring(p: &Painter, rc: Rect) {
    // Only once the keyboard is in use in this window, as Windows shows focus rectangles.
    if !keyboard_in_use(p.ctx()) {
        return;
    }
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
        widget_id(ui, ("check-light", glyph.x as i32, glyph.y as i32)),
        sense,
    );
    let resp = enter_is_not_a_click(ui, resp);
    focus_on_press(&resp);
    show_focus(ui, &resp, hit.expand(1.0));
    if resp.clicked() {
        *checked = !*checked;
    }
    expose(
        &resp,
        egui::Role::CheckBox,
        caption,
        enabled,
        Some(*checked),
    );
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
/// A check box in a list's row ([`row_list`]): its 13 × 13 box at English `at`, ticked by a
/// click (hot and pressed as any check box), not a Tab stop of its own (the list is: Space
/// ticks the selected row). `salt` names the item, so the box stays its own while the list
/// scrolls. True when clicked.
pub fn row_check(
    ui: &Ui,
    p: &Painter,
    at: Pos2,
    on: &mut bool,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let rc = rect(Rect::from_min_size(at, vec2(13.0, 13.0)));
    let resp = ui.interact(
        rc.expand(2.0),
        widget_id(ui, ("row-check", salt)),
        pointer_only(),
    );
    if resp.clicked() {
        *on = !*on;
    }
    check_box_glyph(p, rc, *on, State::of(&resp, true));
    resp.clicked()
}

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
        // Pressed lighter still, as Windows 11's dark boxes (it went darker, the wrong way).
        (State::Pressed, true) => (Some(rgb(0x47, 0x47, 0x47)), rgb(0x70, 0x70, 0x70)),
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
        widget_id(ui, ("radio-light", glyph.x as i32, glyph.y as i32)),
        sense,
    );
    let resp = enter_is_not_a_click(ui, resp);
    focus_on_press(&resp);
    show_focus(ui, &resp, hit.expand(1.0));
    expose(
        &resp,
        egui::Role::RadioButton,
        caption,
        enabled,
        Some(selected),
    );
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
        // Pressed, Windows 11 shows the dot to come.
        if state == State::Pressed {
            p.circle_filled(c, 2.1, ring);
        }
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

/// A held button that repeats, as Windows' spinners and scroll-bar arrows do: true when it is
/// pressed, then after 0.4 s every 50 ms while it stays held (redraws are asked for to keep
/// time).
pub fn auto_repeat(ui: &Ui, resp: &Response) -> bool {
    let key = resp.id.with("repeat-at");
    if !resp.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<f64>(key));
        return false;
    }
    // Paused while the pointer is off it (Windows' arrows wait for it to come back).
    if !resp.contains_pointer() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
        return false;
    }
    let now = ui.input(|i| i.time);
    let due = ui.data(|d| d.get_temp::<f64>(key));
    let (fire, next) = match due {
        None => (true, now + 0.4),
        Some(t) if now >= t => (true, t + 0.05),
        Some(t) => (false, t),
    };
    ui.data_mut(|d| d.insert_temp(key, next));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f64((next - now).max(0.01)));
    fire
}

/// What a control is for screen readers (Narrator, NVDA through AccessKit): its role, its name
/// (the full translated label, "&" taken out), whether it can be used, and for a check box,
/// radio button, tab or list row whether it is on. Our controls are painted by hand, so egui
/// can't tell otherwise. Call right after the control's `interact`, before any tooltip.
pub fn expose(resp: &Response, role: egui::Role, label: &str, enabled: bool, on: Option<bool>) {
    let name = label
        .replace("&&", "\u{1}")
        .replace('&', "")
        .replace('\u{1}', "&");
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(role, enabled, &name);
        info.selected = on;
        info
    });
}

/// [`expose`] for a control showing a value (a drop-down): named by its caption, the value read
/// after it.
pub fn expose_value(resp: &Response, role: egui::Role, value: &str, enabled: bool) {
    let name = caption_for(&resp.ctx, resp.rect).unwrap_or_default();
    let value = value.to_owned();
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(role, enabled, &name);
        info.current_text_value = Some(value.clone());
        info
    });
}

/// The click of a control that opens a popup, except the one ending the press that just closed
/// that popup (it went down on the control): a click on an open list's arrow closes it, not
/// closes and opens it again.
pub fn opener_click(ui: &Ui, mut resp: Response) -> Response {
    let key = crate::chrome::dismissed_at_key(ui.ctx());
    let Some(at) = ui.data(|d| d.get_temp::<Pos2>(key)) else {
        return resp;
    };
    if resp.clicked() && resp.rect.contains(at) {
        resp.flags.remove(egui::response::Flags::CLICKED);
        ui.data_mut(|d| d.remove::<Pos2>(key));
    } else if !ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
        // Between presses: that press is over.
        ui.data_mut(|d| d.remove::<Pos2>(key));
    }
    resp
}

/// A system file or folder dialog's answer, from its thread.
type PickSlot = std::sync::Arc<std::sync::Mutex<Option<Option<std::path::PathBuf>>>>;

/// Show a system Open / Save / folder dialog (`ask` builds and runs it) on a thread of its
/// own: it blocked the app's windows while open (progress windows stood still). One per
/// `key` at a time; [`picked`] gives its answer on a later frame.
pub fn start_pick(
    ctx: &egui::Context,
    key: &str,
    ask: impl FnOnce() -> Option<std::path::PathBuf> + Send + 'static,
) {
    let id = egui::Id::unique(("pick", key));
    if ctx.data(|d| d.get_temp::<PickSlot>(id)).is_some() {
        return;
    }
    let slot: PickSlot = Default::default();
    ctx.data_mut(|d| d.insert_temp(id, slot.clone()));
    let wake = ctx.clone();
    std::thread::spawn(move || {
        let answer = ask();
        *slot.lock().unwrap() = Some(answer);
        wake.request_repaint();
    });
}

/// The path chosen in [`start_pick`]'s dialog `key`, once (None while it is open, or when it
/// was cancelled).
pub fn picked(ctx: &egui::Context, key: &str) -> Option<std::path::PathBuf> {
    let id = egui::Id::unique(("pick", key));
    let slot = ctx.data(|d| d.get_temp::<PickSlot>(id))?;
    let answer = slot.lock().unwrap().take()?;
    ctx.data_mut(|d| d.remove::<PickSlot>(id));
    answer
}

/// A press gives a control the keys, as in Windows (egui focuses only text boxes on a click):
/// the arrows work in a radio group clicked, Ctrl+C in a field clicked. Its ring shows only
/// once the keyboard is used ([`keyboard_in_use`]).
pub fn focus_on_press(resp: &Response) {
    if resp.is_pointer_button_down_on() && !resp.has_focus() && resp.sense.senses_click() {
        resp.request_focus();
    }
}

/// egui clicks a focused control on Enter as on Space. A check box, radio button or tab answers
/// Space only, as in Windows: Enter is the dialog's default button (it ticked the box and
/// pressed OK). Links do open on Enter.
pub fn enter_is_not_a_click(ui: &Ui, mut resp: Response) -> Response {
    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        resp.flags.remove(egui::response::Flags::CLICKED);
    }
    resp
}

/// A focused control that answers arrow keys (or Escape) itself keeps them: egui otherwise also
/// moves the focus to the next control on an arrow and drops it on Escape. Call each frame
/// with the control's response.
pub fn keep_keys(ui: &Ui, resp: &Response, horizontal: bool, vertical: bool, escape: bool) {
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    horizontal_arrows: horizontal,
                    vertical_arrows: vertical,
                    escape,
                    ..Default::default()
                },
            )
        });
    }
}

/// Type-ahead, as Windows lists and drop-downs have it: letters typed while it has the keys
/// pick the next of its `n` items whose `label` starts with them (typed within a second of
/// each other; one letter again and again goes through the items starting with it). `key`
/// keeps what was typed. Returns the item to choose.
pub fn typed_match(
    ui: &Ui,
    key: egui::Id,
    n: usize,
    current: Option<usize>,
    label: impl Fn(usize) -> String,
) -> Option<usize> {
    let typed: String = ui.input(|i| {
        if i.modifiers.ctrl || i.modifiers.alt || i.modifiers.command {
            return String::new();
        }
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<String>()
    });
    let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
    // Space alone is a key of its own (it ticks a row), not a name's first letter.
    if typed.trim().is_empty() || n == 0 {
        return None;
    }
    let now = ui.input(|i| i.time);
    let (mut buf, at) = ui
        .data(|d| d.get_temp::<(String, f64)>(key))
        .unwrap_or_default();
    if now - at > 1.0 {
        buf.clear();
    }
    buf.push_str(&typed.to_lowercase());
    ui.data_mut(|d| d.insert_temp(key, (buf.clone(), now)));
    let first = buf.chars().next().unwrap_or_default();
    let repeated = buf.chars().all(|c| c == first);
    // The same letter again: the next item with it; else the first from here with the prefix.
    let (needle, start) = if repeated {
        (first.to_string(), current.map_or(0, |c| c + 1))
    } else {
        (buf, current.unwrap_or(0))
    };
    (0..n)
        .map(|k| (start + k) % n)
        .find(|&i| label(i).to_lowercase().starts_with(&needle))
}

/// A tab control's keys, as Windows has them: Ctrl+Tab / Ctrl+Page Down the next of its `n`
/// tabs, Ctrl+Shift+Tab / Ctrl+Page Up the previous one, wrapping. For a window's main tabs
/// (one tab control a window answers them).
pub fn tab_keys(ui: &Ui, n: usize, current: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let (next, prev) = ui.input_mut(|i| {
        let ctrl = egui::Modifiers::COMMAND;
        let ctrl_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        (
            i.consume_key(ctrl, egui::Key::Tab) || i.consume_key(ctrl, egui::Key::PageDown),
            i.consume_key(ctrl_shift, egui::Key::Tab) || i.consume_key(ctrl, egui::Key::PageUp),
        )
    });
    match (next, prev) {
        (true, _) => Some((current + 1) % n),
        (_, true) => Some((current + n - 1) % n),
        _ => None,
    }
}

/// A radio group's keys, as Windows has them: while one of the group's buttons (`group`, in
/// order) has the focus, Up / Left and Down / Right move the focus to the previous / next
/// enabled one, wrapping, and choose it. Returns the one chosen.
pub fn radio_keys(ui: &Ui, group: &[Response]) -> Option<usize> {
    let at = group.iter().position(Response::has_focus)?;
    keep_keys(ui, &group[at], true, true, false);
    let step: isize = if take_key(ui, egui::Key::ArrowDown) || take_key(ui, egui::Key::ArrowRight) {
        1
    } else if take_key(ui, egui::Key::ArrowUp) || take_key(ui, egui::Key::ArrowLeft) {
        -1
    } else {
        return None;
    };
    let n = group.len() as isize;
    let next = (1..n)
        .map(|k| (at as isize + step * k).rem_euclid(n) as usize)
        .find(|&j| group[j].sense.senses_click())?;
    group[next].request_focus();
    Some(next)
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
        // Keyed by the control's place and the tab's index: its x changes when it is chosen,
        // and the focus must stay on it.
        // One Tab stop, the chosen tab (Windows' tab control); the arrows choose.
        let sense = if sel { Sense::click() } else { pointer_only() };
        let id_of =
            |ui: &Ui, k: usize| widget_id(ui, ("tab-light", rc.min.x as i32, rc.min.y as i32, k));
        let resp = ui.interact(hit, id_of(ui, i), sense);
        focus_on_press(&resp);
        expose(
            &resp,
            egui::Role::Tab,
            crate::i18n::t(tab.label),
            true,
            Some(sel),
        );
        let resp = enter_is_not_a_click(ui, resp);
        if resp.clicked() && !sel {
            clicked = Some(i);
        }
        if sel {
            keep_keys(ui, &resp, true, false, false);
            if resp.has_focus() {
                let last = tabs.len().saturating_sub(1) as isize;
                let fwd = if rtl() { -1 } else { 1 };
                let to = if take_key(ui, egui::Key::ArrowRight) {
                    Some(i as isize + fwd)
                } else if take_key(ui, egui::Key::ArrowLeft) {
                    Some(i as isize - fwd)
                } else if take_key(ui, egui::Key::Home) {
                    Some(0)
                } else if take_key(ui, egui::Key::End) {
                    Some(last)
                } else {
                    None
                };
                if let Some(to) = to.map(|t| t.clamp(0, last) as usize).filter(|t| *t != i) {
                    clicked = Some(to);
                    let next = id_of(ui, to);
                    ui.memory_mut(|m| m.request_focus(next));
                }
            }
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
        // Over the tab's face (drawn before it, the face hid it).
        show_focus(ui, &resp, hit.shrink(2.0));
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
    let rounding = control_radius();
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
    let resp = opener_click(ui, resp);
    focus_on_press(&resp);
    keep_keys(ui, &resp, false, true, false);
    let mut open = ui.data(|d| d.get_temp::<bool>(key).unwrap_or(false)) && enabled;
    let popup_id = format!("combo-light-{}", key.short_debug_format());
    let mut chosen = None;
    let last = items.len().saturating_sub(1);
    // egui clicks a focused control on Enter too: on a closed one Enter is the dialog's default
    // button, not an opener.
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    if resp.clicked() && !enter {
        open = !open;
    }
    // The keys, as a Windows drop-down list has them (an open list's are the popup's).
    if resp.has_focus() && enabled && !items.is_empty() && !open {
        let opener = ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::F4)
        });
        if opener {
            open = true;
            crate::chrome::start_at(ui.ctx(), &popup_id, selected.min(last));
        }
        let to = if take_key(ui, egui::Key::ArrowDown) {
            Some((selected + 1).min(last))
        } else if take_key(ui, egui::Key::ArrowUp) {
            Some(selected.saturating_sub(1))
        } else if take_key(ui, egui::Key::Home) {
            Some(0)
        } else if take_key(ui, egui::Key::End) {
            Some(last)
        } else {
            None
        };
        // Typing picks the item starting with it (the arrows change the choice too).
        let typed = typed_match(ui, key.with("typed"), items.len(), Some(selected), |i| {
            crate::i18n::t(items[i]).to_owned()
        });
        // Closed: the arrows change the choice itself.
        if let Some(i) = to.or(typed).filter(|i| *i != selected) {
            chosen = Some(i);
        }
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
    expose_value(&resp, egui::Role::ComboBox, full, enabled);
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
    // Over the face (drawn before it, the face hid it).
    show_focus(ui, &resp, rc.shrink(3.0));
    if open {
        // The list: the shared popup (a window of its own, never clipped by the dialog, kept on
        // the screen; the keys, the pointer), the choice marked.
        use crate::chrome::{MenuEntry, PopupResult, label as menu_label};
        let entries: Vec<MenuEntry<usize>> = items
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let shown = crate::i18n::t(s).replace('&', "&&");
                MenuEntry::Radio(menu_label(&shown), i, true, i == selected)
            })
            .collect();
        let at = if rtl() {
            rc.right_bottom()
        } else {
            rc.left_bottom()
        };
        match crate::chrome::popup(ui.ctx(), &popup_id, at, &entries, &|_| None) {
            PopupResult::Chosen(i) => {
                open = false;
                chosen = Some(i).filter(|i| *i != selected);
                resp.request_focus();
            }
            PopupResult::Dismissed => {
                open = false;
                resp.request_focus();
            }
            PopupResult::Open | PopupResult::Neighbor(_) => {}
        }
        // The keys went to the list: the dialog's default button doesn't take Enter too.
        take_key(ui, egui::Key::Enter);
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
        let name = if dir > 0 {
            // i18n: a spin box's arrow, for screen readers: one more
            crate::i18n::t("Increase")
        } else {
            // i18n: a spin box's arrow, for screen readers: one less
            crate::i18n::t("Decrease")
        };
        expose(&r, egui::Role::Button, name, enabled, None);
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
        // Held, it keeps counting.
        if auto_repeat(ui, &r) {
            step = dir;
        }
    }
    step
}

/// A flat 19 × 21 button with a 16 × 16 icon (the Scheduler's move up / down / remove), its
/// `tip` (English, translated here) on hover.
pub fn icon_button_light(ui: &mut Ui, rc: Rect, id: &str, enabled: bool, tip: &str) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rc, widget_id(ui, ("icon-button-light", id)), sense);
    expose(
        &resp,
        egui::Role::Button,
        crate::i18n::t(tip),
        enabled,
        None,
    );
    let p = ui.painter();
    let face = subtle_or(State::of(&resp, enabled));
    p.rect_filled(rc, 2.0, face);
    p.rect_stroke(
        rc,
        2.0,
        Stroke::new(1.0, CONTROL_EDGE()),
        StrokeKind::Inside,
    );
    show_focus(ui, &resp, rc.shrink(2.0));
    // No label of its own: its tooltip says what it does (translated).
    resp.on_hover_text(crate::i18n::t(tip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// egui's context locks itself for `data`, `memory`, `input` and their `_mut`s: asking it
    /// again inside one (`ctx.viewport_id()` to build a key) deadlocks the app at once. The
    /// key is worked out first; this keeps it so in every file of the app.
    fn no_context_call_inside_a_context_lock() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let opens = [
            ".data(|",
            ".data_mut(|",
            ".memory(|",
            ".memory_mut(|",
            ".input(|",
            ".input_mut(|",
        ];
        let inner = [
            "ctx()",
            ".viewport_id()",
            ".cumulative_pass_nr()",
            "_key(ctx",
            "_key(ui",
            "_key(&",
        ];
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let mut src = std::fs::read_to_string(&path).unwrap();
            // Not this test's own patterns.
            if let Some(me) = src.find("fn no_context_call_inside_a_context_lock") {
                src.truncate(me);
            }
            for open in opens {
                let mut from = 0;
                while let Some(at) = src[from..].find(open) {
                    let start = from + at + open.len() - 2; // the "(" of the call
                    let mut depth = 0;
                    let mut end = start;
                    for (k, c) in src[start..].char_indices() {
                        match c {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        if depth == 0 {
                            end = start + k;
                            break;
                        }
                    }
                    let body = &src[start..end];
                    if inner.iter().any(|i| body.contains(i)) {
                        let line = src[..start].lines().count();
                        found.push(format!("{}:{line}", path.display()));
                    }
                    from = end.max(start + 1);
                }
            }
        }
        assert!(
            found.is_empty(),
            "context asked inside its own lock: {found:?}"
        );
    }

    #[test]
    fn lines_land_on_whole_pixels_at_any_scaling() {
        let r = Rect::from_min_max(pos2(10.0, 3.0), pos2(11.0, 4.0));
        assert_eq!(snap(1.0, r), r);
        // 125 %: a 1 pt line at x 10 → pixels 12.5..13.75: one whole pixel, on the grid.
        let s = snap(1.25, r);
        assert_eq!(
            ((s.min.x * 1.25).fract(), (s.max.x * 1.25).fract()),
            (0.0, 0.0)
        );
        assert!(s.width() * 1.25 >= 1.0);
    }

    #[test]
    fn a_paste_into_one_line_is_one_clean_line() {
        assert_eq!(single_line("https://x/a.zip\r\n"), "https://x/a.zip");
        assert_eq!(single_line("a\r\nb\tc\u{7}"), "a b c");
    }

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
        // pointer, lighter still pressed (Windows 11). The three differ; disabled is empty.
        let fill = |s, dark| fluent_box_in(s, dark).0.unwrap().r();
        assert!(
            fill(State::Pressed, false) < fill(State::Hot, false)
                && fill(State::Hot, false) < fill(State::Normal, false)
        );
        assert!(
            fill(State::Normal, true) < fill(State::Hot, true)
                && fill(State::Hot, true) < fill(State::Pressed, true)
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

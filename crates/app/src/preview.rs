//! The preview pane (#1368): beside the download list, as Explorer's preview and details panes
//! and Finder's preview are, the selected download's picture and what MDM knows of it. Its
//! picture is the system's own thumbnail of the file, the one its file manager shows (Windows'
//! shell, the freedesktop thumbnail cache on Linux, else a picture or a video's frame made here),
//! or its type's large icon when there is none; then its name, type, size, status, where it was
//! saved and whether its file is still there, its addresses and its dates. Several selected: how
//! many and their size; none: a word on what the pane is for.
//!
//! Shown and hidden with View → Preview pane, the toolbar's toggle and Alt+P; at the list's
//! trailing side (the left in a right-to-left interface, winui's surfaces), as wide as its
//! splitter leaves it, both kept in the settings.
//!
//! Nothing here waits for the disk: whether the file is there is looked at on a thread of its
//! own (a network folder or a sleeping drive can take seconds to answer), again every few
//! seconds while it is shown, and its picture is made on another, for the selected file only,
//! kept by its path, size and date: the window draws what it has meanwhile.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use egui::{Color32, Pos2, Rect, Ui, pos2, vec2};

use crate::fileicons::{self, Kind};
use crate::model::{Item, Status};
use crate::winui;

/// The pane's frame width at first (points).
pub const DEFAULT_WIDTH: u32 = 260;
/// The narrowest it is dragged to.
pub const MIN_WIDTH: f32 = 180.0;
/// The room the list keeps beside it.
pub const LIST_KEEPS: f32 = 240.0;

/// How long an answer about the disk holds before it is asked again, while the pane shows it:
/// a file moved or deleted while it is selected is seen within this.
const LOOK_AGAIN: Duration = Duration::from_secs(2);

// ---- the disk, beside the window ---------------------------------------------------------------

/// What the disk says of a download's place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Not answered yet.
    Looking,
    /// There, `len` bytes (a folder: a torrent's, a gallery's).
    Found { len: u64, dir: bool },
    /// Not there, its folder is: moved or deleted (#342's test, `model::finished_file_gone`).
    Gone,
    /// Neither it nor its folder: a drive unplugged, a network folder away. It may come back.
    Unreachable,
}

#[derive(Clone, Copy)]
struct Seen {
    presence: Presence,
    modified: Option<SystemTime>,
    /// When it was last asked for, and whether that ask is still out.
    asked: Instant,
    out: bool,
}

/// The pictures' key: a file as it is now (another size or date is another picture).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PicKey {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    px: u32,
}

/// A picture made for the pane.
#[derive(Clone)]
pub struct Shown {
    pub texture: egui::TextureHandle,
    /// Its pixel size.
    pub size: [u32; 2],
    /// The file's own icon (a program's), not a picture of its content: drawn at an icon's size.
    pub icon: bool,
}

#[derive(Default)]
struct Disk {
    seen: HashMap<PathBuf, Seen>,
    /// None while it is made; Some(None) when the system has no picture.
    pics: HashMap<PicKey, Option<Option<Shown>>>,
    /// The pictures' keys, oldest first (a few are kept).
    order: VecDeque<PicKey>,
    looks: Option<Sender<(PathBuf, egui::Context)>>,
    makes: Option<Sender<(PicKey, egui::Context)>>,
}

/// How many pictures are kept (a selection walked down the list comes back to them).
const KEEP: usize = 24;

static DISK: LazyLock<Mutex<Disk>> = LazyLock::new(Default::default);

fn disk() -> std::sync::MutexGuard<'static, Disk> {
    DISK.lock().unwrap_or_else(|e| e.into_inner())
}

/// One worker thread per kind of job, in the order asked; a thread that can't start leaves its
/// jobs unanswered (the pane says it is looking, the icon stands in).
fn worker<T: Send + 'static>(name: &str, job: impl Fn(T) + Send + 'static) -> Sender<T> {
    let (tx, rx) = channel::<T>();
    let spawned = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            for t in rx {
                job(t);
            }
        });
    if spawned.is_err() {
        return channel().0;
    }
    tx
}

/// What the disk says of `path` now: [`Presence::Looking`] the first time (asked now), then the
/// last answer, asked again every [`LOOK_AGAIN`] while the pane shows it.
pub fn presence(ctx: &egui::Context, path: &Path) -> (Presence, Option<SystemTime>) {
    let mut d = disk();
    let now = Instant::now();
    let ask = match d.seen.get(path) {
        None => true,
        Some(s) => !s.out && now.duration_since(s.asked) >= LOOK_AGAIN,
    };
    if ask {
        let seen = d.seen.entry(path.to_path_buf()).or_insert(Seen {
            presence: Presence::Looking,
            modified: None,
            asked: now,
            out: true,
        });
        seen.asked = now;
        seen.out = true;
        let tx = d
            .looks
            .get_or_insert_with(|| worker("preview: the disk", look))
            .clone();
        let _ = tx.send((path.to_path_buf(), ctx.clone()));
    }
    // Shown: looked at again in a while, whatever else wakes the window.
    ctx.request_repaint_after(LOOK_AGAIN);
    let s = d.seen.get(path).copied();
    s.map_or((Presence::Looking, None), |s| (s.presence, s.modified))
}

/// The disk thread's job: whether `path` is there, its size and date.
fn look((path, ctx): (PathBuf, egui::Context)) {
    let (presence, modified) = match std::fs::metadata(&path) {
        Ok(m) => (
            Presence::Found {
                len: m.len(),
                dir: m.is_dir(),
            },
            m.modified().ok(),
        ),
        Err(_) if path.parent().is_some_and(Path::is_dir) => (Presence::Gone, None),
        Err(_) => (Presence::Unreachable, None),
    };
    let mut d = disk();
    let changed = d
        .seen
        .get(&path)
        .is_none_or(|s| s.presence != presence || s.modified != modified);
    d.seen.insert(
        path,
        Seen {
            presence,
            modified,
            asked: Instant::now(),
            out: false,
        },
    );
    // A few are kept; the oldest answers go (asked again if shown again).
    if d.seen.len() > 64 {
        let oldest = d
            .seen
            .iter()
            .filter(|(_, s)| !s.out)
            .min_by_key(|(_, s)| s.asked)
            .map(|(p, _)| p.clone());
        if let Some(p) = oldest {
            d.seen.remove(&p);
        }
    }
    drop(d);
    if changed {
        ctx.request_repaint();
    }
}

/// The picture of the file at `path` as it is (`len`, `modified`), about `px` pixels on its
/// longer side: None while it is made (asked now, the first time), or when the system has none
/// (the type's icon is drawn then).
pub fn picture(
    ctx: &egui::Context,
    path: &Path,
    len: u64,
    modified: Option<SystemTime>,
    px: u32,
) -> Option<Shown> {
    let key = PicKey {
        path: path.to_path_buf(),
        len,
        modified,
        px,
    };
    let mut d = disk();
    if let Some(made) = d.pics.get(&key) {
        return made.clone().flatten();
    }
    // The same file's picture at another size while this one is made (no flash as the pane is
    // dragged wider).
    let near = d
        .pics
        .iter()
        .filter_map(|(k, v)| match v {
            Some(Some(s)) if k.path == key.path && k.len == len && k.modified == modified => {
                Some((k.px.abs_diff(px), s.clone()))
            }
            _ => None,
        })
        .min_by_key(|(gap, _)| *gap)
        .map(|(_, s)| s);
    d.pics.insert(key.clone(), None);
    d.order.push_back(key.clone());
    while d.order.len() > KEEP {
        if let Some(old) = d.order.pop_front() {
            d.pics.remove(&old);
        }
    }
    let tx = d
        .makes
        .get_or_insert_with(|| worker("preview: pictures", make))
        .clone();
    let _ = tx.send((key, ctx.clone()));
    near
}

/// The pictures thread's job: the system's picture for a file, as a texture.
fn make((key, ctx): (PicKey, egui::Context)) {
    // A key already gone (the selection moved on long ago) isn't made.
    if !disk().pics.contains_key(&key) {
        return;
    }
    // A failing handler (a shell extension that panics through us) leaves the icon, not the
    // app down.
    let got = std::panic::catch_unwind(|| system_picture(&key.path, key.px))
        .ok()
        .flatten();
    let shown = got.and_then(|p| to_texture(&ctx, &key, p));
    let mut d = disk();
    if let Some(slot) = d.pics.get_mut(&key) {
        *slot = Some(shown);
    }
    drop(d);
    ctx.request_repaint();
}

/// A picture as a system makes it: `w` × `h` RGBA pixels.
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    /// Its alpha is premultiplied (Windows' bitmaps) or straight (a decoded file).
    pub premultiplied: bool,
    /// The file's own icon, not a picture of its content.
    pub icon: bool,
}

/// The system's picture for the file at `path`, about `px` pixels on its longer side.
fn system_picture(path: &Path, px: u32) -> Option<Picture> {
    #[cfg(windows)]
    {
        crate::win32::thumbnail(path, px)
    }
    #[cfg(unix)]
    {
        crate::unix::thumbnail(path, px)
    }
}

fn to_texture(ctx: &egui::Context, key: &PicKey, p: Picture) -> Option<Shown> {
    let Picture {
        w,
        h,
        rgba,
        premultiplied,
        icon,
    } = p;
    if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 {
        return None;
    }
    // Never larger than asked (a decoded photo is thousands of pixels wide).
    let (w, h, rgba) = fit_within(w, h, rgba, premultiplied, key.px);
    let size = [w as usize, h as usize];
    let img = if premultiplied {
        egui::ColorImage::from_rgba_premultiplied(size, &rgba)
    } else {
        egui::ColorImage::from_rgba_unmultiplied(size, &rgba)
    };
    let name = format!("preview-{}-{}", key.path.display(), key.px);
    Some(Shown {
        texture: ctx.load_texture(name, img, egui::TextureOptions::LINEAR),
        size: [w, h],
        icon,
    })
}

/// `rgba` (`w` × `h`) made no larger than `px` on its longer side, its shape kept.
fn fit_within(w: u32, h: u32, rgba: Vec<u8>, premultiplied: bool, px: u32) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = fitted(w, h, px);
    if (nw, nh) == (w, h) {
        return (w, h, rgba);
    }
    // Resized on premultiplied pixels, so transparent edges don't darken; straight again after.
    let mut pre = rgba;
    if !premultiplied {
        premultiply(&mut pre);
    }
    let Some(img) = image::RgbaImage::from_raw(w, h, pre) else {
        return (w, h, Vec::new());
    };
    let mut out =
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle).into_raw();
    if !premultiplied {
        unpremultiply(&mut out);
    }
    (nw, nh, out)
}

/// `w` × `h` scaled down to fit `px` on its longer side (never up), at least a pixel each way.
pub fn fitted(w: u32, h: u32, px: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= px || long == 0 {
        return (w, h);
    }
    let s = px as f64 / long as f64;
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

fn premultiply(px: &mut [u8]) {
    for c in px.as_chunks_mut::<4>().0 {
        let a = c[3] as u32;
        for v in &mut c[..3] {
            *v = ((*v as u32 * a + 127) / 255) as u8;
        }
    }
}

fn unpremultiply(px: &mut [u8]) {
    for c in px.as_chunks_mut::<4>().0 {
        let a = c[3] as u32;
        if a > 0 && a < 255 {
            for v in &mut c[..3] {
                *v = ((*v as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

// ---- the freedesktop thumbnail cache (Linux: unix::thumbnail) ---------------------------------

/// The file's address as the Thumbnail Managing Standard names it: `file://` and its absolute
/// path, each byte outside the characters GLib leaves as they are written `%XX`
/// (`g_filename_to_uri`, which GNOME's and KDE's file managers use).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn thumbnail_uri(path: &[u8]) -> String {
    let mut out = String::from("file://");
    for &b in path {
        let keep = b.is_ascii_alphanumeric() || b"!$&'()*+,-./:=@_~".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The cache's file name for that address: its MD5 in lowercase hex, `.png`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn thumbnail_name(uri: &str) -> String {
    use md5::Digest as _;
    let sum = md5::Md5::digest(uri.as_bytes());
    let mut name: String = sum.iter().map(|b| format!("{b:02x}")).collect();
    name.push_str(".png");
    name
}

/// A PNG's text chunk `key` (`Thumb::MTime`: the file's date the thumbnail was made from, which
/// must match the file's for it to be used).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn png_text(png: &[u8], key: &str) -> Option<String> {
    let mut at = png.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
    while at.len() >= 12 {
        let len = u32::from_be_bytes(at[..4].try_into().ok()?) as usize;
        let kind = &at[4..8];
        let data = at.get(8..8 + len)?;
        if kind == b"tEXt"
            && let Some(nul) = data.iter().position(|b| *b == 0)
            && &data[..nul] == key.as_bytes()
        {
            return Some(String::from_utf8_lossy(&data[nul + 1..]).into_owned());
        }
        if kind == b"IDAT" || kind == b"IEND" {
            // The standard puts its texts before the picture.
            return None;
        }
        at = at.get(12 + len..)?;
    }
    None
}

// ---- the pane ---------------------------------------------------------------------------------

/// What the pane shows.
pub enum Shows<'a> {
    Nothing,
    One(&'a Item),
    Many(Summary),
}

/// Several downloads selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub n: usize,
    /// Their sizes added up, the known ones.
    pub bytes: u64,
    /// How many have no size yet.
    pub unknown: usize,
    pub complete: usize,
}

impl Summary {
    pub fn of<'a>(items: impl IntoIterator<Item = &'a Item>) -> Self {
        let mut s = Summary::default();
        for i in items {
            s.n += 1;
            match i.size {
                Some(b) => s.bytes += b,
                None => s.unknown += 1,
            }
            if i.status == Status::Complete {
                s.complete += 1;
            }
        }
        s
    }
}

pub enum Event {
    Close,
    /// The pane takes the keys: a screen reader moved its focus there.
    Keys,
}

/// The pane's own state: where it is scrolled to, how tall its content was.
#[derive(Default)]
pub struct Pane {
    scroll: f32,
    content_h: f32,
    /// The download shown last (a new one shows from its top).
    shown: Option<u64>,
}

/// Room between the pane's inside and what it shows.
const PAD: f32 = 10.0;
/// One line of text.
const LINE: f32 = 15.0;

impl Pane {
    /// Paint the pane in its frame from English x `left` to `right`, `top` to `bottom` (the
    /// window lays it out; winui mirrors it in a right-to-left interface). `keys`: it has the
    /// keys (Tab reached it): the arrows, Page Up / Down, Home and End scroll it.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        shows: &Shows<'_>,
        keys: bool,
    ) -> Option<Event> {
        let mut event = None;
        // i18n: noun: title of the side pane beside the download list that shows the selected download's picture and details (#1368)
        let caption = crate::i18n::tc("pane", "Preview");
        if crate::chrome::side_pane(ui, left, right, top, bottom, caption, "preview") {
            event = Some(Event::Close);
        }
        // Inside the frame, under its caption (as the Categories tree sits in its pane).
        let inner = Rect::from_min_max(
            pos2(left + 2.0, top + 18.0),
            pos2(right - 3.0, bottom - 4.0),
        );
        ui.painter()
            .rect_filled(winui::rect(inner), 0.0, crate::theme::colors().tree);

        // For screen readers: a pane named as its caption, what it shows read inside it; Tab
        // reaches it (the window gives it the keys), and a reader can move its focus there.
        let node = ui.interact(
            winui::rect(inner),
            egui::Id::unique("preview-pane"),
            egui::Sense::hover(),
        );
        winui::expose(&node, egui::Role::Pane, caption, true, None);
        ui.ctx()
            .accesskit_node_builder(node.id, |n| n.set_author_id("preview"));
        if winui::focus_target(ui.ctx(), node.id) {
            event = event.or(Some(Event::Keys));
        }
        if keys {
            winui::keys_with(ui.ctx(), node.id);
        }

        let id = match shows {
            Shows::One(item) => Some(item.id),
            _ => None,
        };
        if self.shown != id {
            self.shown = id;
            self.scroll = 0.0;
        }
        // The scroll bar while the content is taller than the room (as it was last drawn).
        let room = inner.height();
        let need_bar = self.content_h > room;
        let area = Rect::from_min_max(
            inner.min,
            pos2(
                inner.right()
                    - if need_bar {
                        crate::scrollbar::THICK
                    } else {
                        0.0
                    },
                inner.bottom(),
            ),
        );
        let max = (self.content_h - room).max(0.0);
        if ui.rect_contains_pointer(winui::rect(inner)) {
            self.scroll += winui::wheel_rows(ui, LINE, (room / LINE).floor()) * LINE;
        }
        if keys {
            use egui::{Key, Modifiers};
            let page = (room - LINE).max(LINE);
            let key = |k| ui.input_mut(|i| i.consume_key(Modifiers::NONE, k));
            if key(Key::ArrowDown) {
                self.scroll += LINE * 2.0;
            } else if key(Key::ArrowUp) {
                self.scroll -= LINE * 2.0;
            } else if key(Key::PageDown) {
                self.scroll += page;
            } else if key(Key::PageUp) {
                self.scroll -= page;
            } else if key(Key::Home) {
                self.scroll = 0.0;
            } else if key(Key::End) {
                self.scroll = max;
            }
        }
        self.scroll = self.scroll.clamp(0.0, max).round();

        // Drawn in a child kept to the area: what scrolls out of it is neither painted nor
        // hovered, and what a reader finds is the pane's.
        let screen = winui::rect(area);
        let mut body = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(screen)
                .accessibility_parent(node.id),
        );
        body.set_clip_rect(screen.intersect(ui.clip_rect()));
        let y0 = area.top() + PAD - self.scroll;
        let x0 = area.left() + PAD;
        let x1 = area.right() - PAD;
        let end = match shows {
            Shows::Nothing => nothing(&body, x0, x1, area),
            Shows::Many(s) => many(&mut body, x0, x1, y0, s),
            Shows::One(item) => one(&mut body, x0, x1, y0, item),
        };
        self.content_h = (end - y0 + 2.0 * PAD).ceil();

        if need_bar {
            // At the trailing edge: the left in RTL.
            let track = winui::rect(Rect::from_min_max(
                pos2(area.right(), inner.top()),
                pos2(inner.right(), inner.bottom()),
            ));
            let mut pos = self.scroll;
            if crate::scrollbar::bar(
                ui,
                egui::Id::unique("preview-scroll"),
                track,
                true,
                self.content_h,
                room,
                LINE,
                &mut pos,
            ) {
                self.scroll = pos.round();
                ui.ctx().request_repaint();
            }
        }
        if keys {
            winui::focus_ring(ui.painter(), winui::rect(inner.shrink(1.0)));
        }
        event
    }
}

/// The pane's text colour: the Categories tree's, on the same ground.
fn ink() -> Color32 {
    crate::theme::colors().menu_text
}

/// Nothing selected: what the pane is for, in the middle of its room. Returns where it ends.
fn nothing(ui: &Ui, x0: f32, x1: f32, area: Rect) -> f32 {
    let p = ui.painter();
    // i18n: the preview pane (beside the download list) when no download is selected: its title
    let title = crate::i18n::t("No download selected");
    // i18n: the preview pane when no download is selected, under its title
    let why = crate::i18n::t("Select a download to see its picture and details here.");
    let room = x1 - x0;
    let title_lines = winui::wrap_lines_font(p, winui::tahoma_bold(), title, room);
    let why_lines = winui::wrap_lines_font(p, winui::tahoma(), why, room);
    let block = 34.0 + 12.0 + LINE * (title_lines.len() + why_lines.len()) as f32 + 6.0;
    let mut y = (area.center().y - block / 2.0).max(area.top() + PAD);
    let cx = (x0 + x1) / 2.0;
    // A picture frame in the muted ink, outlined as the list's own empty state draws its glyph.
    let ink = egui::Stroke::new(1.5, winui::MUTED());
    let frame = winui::rect(Rect::from_center_size(pos2(cx, y + 15.0), vec2(36.0, 28.0)));
    p.rect_stroke(frame, 3.0, ink, egui::StrokeKind::Inside);
    let sun = winui::rect(Rect::from_center_size(
        pos2(cx + 7.0, y + 8.0),
        vec2(1.0, 1.0),
    ));
    p.circle_stroke(sun.center(), 3.0, ink);
    let base = y + 26.0;
    let pts = [
        (cx - 15.0, base),
        (cx - 5.0, base - 9.0),
        (cx + 3.0, base - 2.0),
        (cx + 8.0, base - 6.0),
        (cx + 15.0, base),
    ];
    for w in pts.windows(2) {
        let a = winui::rect(Rect::from_min_size(pos2(w[0].0, w[0].1), vec2(0.0, 0.0))).min;
        let b = winui::rect(Rect::from_min_size(pos2(w[1].0, w[1].1), vec2(0.0, 0.0))).min;
        p.line_segment([a, b], ink);
    }
    y += 34.0 + 12.0;
    for l in &title_lines {
        let w = winui::text_width_font(p, winui::tahoma_bold(), l);
        winui::text_font(
            p,
            winui::tahoma_bold(),
            cx - w / 2.0,
            y,
            l,
            ink_or_muted(true),
        );
        y += LINE;
    }
    y += 6.0;
    for l in &why_lines {
        let w = winui::text_width(p, l);
        winui::text(p, cx - w / 2.0, y, l, winui::MUTED());
        y += LINE;
    }
    let said = format!("{title}. {why}");
    winui::readable(
        ui,
        winui::rect(Rect::from_min_max(pos2(x0, area.top()), pos2(x1, y))),
        "preview-nothing",
        &said,
    );
    y
}

fn ink_or_muted(strong: bool) -> Color32 {
    if strong { ink() } else { winui::MUTED() }
}

/// Several selected: how many, their size, how many are finished.
fn many(ui: &mut Ui, x0: f32, x1: f32, y0: f32, s: &Summary) -> f32 {
    let p = ui.painter().clone();
    let side = 48.0;
    // The stack of pages: the type icons of several files, as Explorer shows a selection.
    for k in 0..3 {
        let off = (2 - k) as f32 * 5.0;
        let rc = Rect::from_min_size(pos2(x0 + off, y0 + off), vec2(side, side));
        fileicons::paint(ui, &p, Kind::File(""), winui::rect(rc));
    }
    let tx = x0 + side + 10.0 + 14.0;
    let mut y = y0 + 16.0;
    let n = crate::i18n::tn(
        s.n as u64,
        "{n} download selected",
        "{n} downloads selected",
    );
    let shown = winui::wrap_lines_font(&p, winui::tahoma_bold(), &n, (x1 - tx).max(20.0));
    for l in &shown {
        winui::text_font(&p, winui::tahoma_bold(), tx, y, l, ink());
        y += LINE;
    }
    let complete = crate::i18n::tn(s.complete as u64, "{n} finished", "{n} finished");
    winui::fit(
        ui,
        &p,
        tx,
        y,
        &complete,
        winui::MUTED(),
        x1,
        "preview-many-complete",
    );
    let said = format!("{n}, {complete}");
    winui::readable(
        ui,
        winui::rect(Rect::from_min_max(pos2(tx, y0), pos2(x1, y + 4.0))),
        "preview-many",
        &said,
    );
    y = y.max(y0 + side + 10.0) + 14.0;
    winui::etched_h(&p, x0, y - 8.0, x1 - x0);
    let total = crate::i18n::ltr(&crate::model::size(s.bytes));
    let mut size = total;
    if s.unknown > 0 {
        size = crate::i18n::fmt(
            // i18n: the preview pane, several downloads selected: their total size, then how many of them have no known size yet; {size} e.g. "4.20 GB"
            &crate::i18n::tn(
                s.unknown as u64,
                "{size} and {n} of unknown size",
                "{size} and {n} of unknown size",
            ),
            &[("size", &size)],
        );
    }
    // i18n: label in the preview pane: the size of the selected downloads added up
    let label = crate::i18n::t("Total size:");
    let c = Cols::new(&p, x0, x1, &[label]);
    field(ui, c, y, label, &size, Value::Plain, "total")
}

/// How a value is drawn.
#[derive(Clone, Copy, PartialEq)]
enum Value {
    /// Interface text or a number: cut at its end.
    Plain,
    /// A path or an address: cut in its middle, left to right in every language, copyable.
    Data,
    /// A warning (the file is gone).
    Warning,
}

/// Where the details go, from English `x0` to `x1`: each value beside its label (at `value_x`,
/// after the widest label, as Explorer's details pane and Finder's preview lay them out), or
/// under it when the pane is too narrow for that (a long translation).
#[derive(Clone, Copy)]
struct Cols {
    x0: f32,
    x1: f32,
    value_x: Option<f32>,
}

impl Cols {
    /// For `labels` (translated) in `p`'s font.
    fn new(p: &egui::Painter, x0: f32, x1: f32, labels: &[&str]) -> Self {
        let widest = labels
            .iter()
            .map(|l| winui::text_width(p, l))
            .fold(0.0_f32, f32::max)
            .ceil();
        let value_x = x0 + widest + 10.0;
        let beside = widest <= (x1 - x0) * 0.45 && x1 - value_x >= 110.0;
        Cols {
            x0,
            x1,
            value_x: beside.then_some(value_x),
        }
    }

    /// Where a row's value starts.
    fn value_x(&self) -> f32 {
        self.value_x.unwrap_or(self.x0)
    }
}

/// A label (muted) and its value, beside it or under it ([`Cols`]), the row's top at `y`; read
/// by a screen reader as one line. Returns where the next row starts.
fn field(ui: &mut Ui, c: Cols, y: f32, label: &str, value: &str, how: Value, salt: &str) -> f32 {
    let p = ui.painter().clone();
    let label_base = y + 11.0;
    let (value_x, value_base) = match c.value_x {
        Some(x) => (x, label_base),
        None => (c.x0, label_base + LINE),
    };
    let label_end = c.value_x.map_or(c.x1, |x| x - 6.0);
    winui::fit(
        ui,
        &p,
        c.x0,
        label_base,
        label,
        winui::MUTED(),
        label_end,
        ("preview-label", salt),
    );
    let color = match how {
        Value::Warning => winui::ERROR(),
        _ => ink(),
    };
    match how {
        Value::Data => {
            let data = crate::i18n::ltr(value);
            let rc = winui::fit_middle(
                ui,
                &p,
                value_x,
                value_base,
                &data,
                color,
                c.x1,
                ("preview-value", salt),
            );
            winui::copyable(ui, rc, ("preview-copy", salt), value, false);
        }
        _ => {
            winui::fit(
                ui,
                &p,
                value_x,
                value_base,
                value,
                color,
                c.x1,
                ("preview-value", salt),
            );
        }
    }
    let rc = winui::rect(Rect::from_min_max(
        pos2(c.x0, y),
        pos2(c.x1, value_base + 4.0),
    ));
    winui::readable(
        ui,
        rc,
        ("preview-field", salt),
        &format!("{} {value}", label.trim()),
    );
    value_base + 4.0 + if c.value_x.is_some() { 5.0 } else { 8.0 }
}

/// The picture's box for an inside `room` wide: a picture's (three quarters as tall as wide,
/// within bounds), or an icon's.
pub fn picture_box(room: f32, picture: bool) -> Rect {
    let w = room.max(1.0);
    let h = if picture {
        (w * 0.75).clamp(72.0, 240.0)
    } else {
        ICON + 8.0
    };
    Rect::from_min_size(Pos2::ZERO, vec2(w, h))
}

/// A file's icon in the pane, when it has no picture (points).
const ICON: f32 = 96.0;

/// One download: its picture, its name and type, then what MDM knows of it.
fn one(ui: &mut Ui, x0: f32, x1: f32, y0: f32, item: &Item) -> f32 {
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let complete = item.status == Status::Complete;
    // Whether its file is there: only a finished one's (the others are a part file still).
    let (presence, modified) = if complete {
        presence(&ctx, &item.dest)
    } else {
        (Presence::Looking, None)
    };
    let boxed = picture_box(x1 - x0, true).translate(vec2(x0, y0));
    let ppp = ctx.pixels_per_point();
    let px =
        ((boxed.width().max(boxed.height()) * ppp / 64.0).ceil() * 64.0).clamp(64.0, 1024.0) as u32;
    let pic = match presence {
        Presence::Found { len, .. } if complete => picture(&ctx, &item.dest, len, modified, px),
        _ => None,
    };
    let boxed = picture_box(x1 - x0, pic.as_ref().is_some_and(|s| !s.icon)).translate(vec2(x0, y0));
    let dir = matches!(presence, Presence::Found { dir: true, .. })
        || item.gallery.is_some()
        || item.torrent.is_some() && Path::new(&item.file_name).extension().is_none();
    match pic {
        Some(s) if !s.icon => {
            // The picture as large as the box allows, its shape kept, never larger than made.
            let (w, h) = (s.size[0] as f32 / ppp, s.size[1] as f32 / ppp);
            let k = (boxed.width() / w).min(boxed.height() / h).min(1.0);
            let size = vec2((w * k).round(), (h * k).round());
            let rc = Rect::from_center_size(boxed.center(), size);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            // Placed, not mirrored: a picture keeps its look.
            p.image(s.texture.id(), winui::rect(rc), uv, Color32::WHITE);
            p.rect_stroke(
                winui::rect(rc.expand(0.5)),
                0.0,
                egui::Stroke::new(1.0, crate::theme::colors().tree_frame),
                egui::StrokeKind::Outside,
            );
        }
        Some(s) => {
            // A program's own icon, at an icon's size.
            let side = ICON;
            let rc = Rect::from_center_size(boxed.center(), vec2(side, side));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            p.image(s.texture.id(), winui::rect(rc), uv, Color32::WHITE);
        }
        None => {
            // The type's icon, large (the theme's where its icon set has one, as the list's).
            let side = ICON;
            let rc = Rect::from_center_size(boxed.center(), vec2(side, side));
            let kind = if dir {
                Kind::Folder
            } else {
                Kind::File(&item.file_name)
            };
            fileicons::paint(ui, &p, kind, winui::rect(rc));
        }
    }
    let mut y = boxed.bottom() + 14.0;

    // Its name, bold, on up to three lines broken at its own separators, its extension whole;
    // its type under it. Each line a text of its own direction (an Arabic name in an English
    // window reads right to left, a Latin one in an Arabic window left to right).
    let font = winui::tahoma_bold();
    let lines = winui::name_lines_in(&p, font.clone(), &item.file_name, x1 - x0, 3);
    let top = y - 11.0;
    for l in &lines {
        winui::text_font(&p, font.clone(), x0, y, &crate::i18n::isolate(l), ink());
        y += LINE;
    }
    let name_rc = winui::rect(Rect::from_min_max(pos2(x0, top), pos2(x1, y - LINE + 4.0)));
    winui::copyable(
        ui,
        name_rc,
        "preview-name",
        &item.file_name,
        lines.len() > 1,
    );
    winui::readable(ui, name_rc, "preview-name-said", &item.file_name);
    let kind = type_name(&item.file_name);
    winui::fit(ui, &p, x0, y, &kind, winui::MUTED(), x1, "preview-type");
    winui::readable(
        ui,
        winui::rect(Rect::from_min_max(pos2(x0, y - 11.0), pos2(x1, y + 4.0))),
        "preview-type-said",
        &kind,
    );
    y += 14.0;
    winui::etched_h(&p, x0, y - 4.0, x1 - x0);
    y += 6.0;

    // The details, each value beside its label where the pane is wide enough.
    let labels = [
        "Status:",
        "Size:",
        "File:",
        "Save To:",
        "Address:",
        "Web page:",
        "Added:",
        "Finished:",
    ]
    .map(crate::i18n::t);
    let c = Cols::new(&p, x0, x1, &labels);
    // Status, and how far it is while it isn't finished.
    let mut status = item.status_text();
    if item.status == Status::Error
        && let Some(e) = &item.error
    {
        status = format!("{status}: {e}");
    }
    let how = if item.status == Status::Error {
        Value::Warning
    } else {
        Value::Plain
    };
    y = field(ui, c, y, crate::i18n::t("Status:"), &status, how, "status");
    if !complete && let Some(f) = item.fraction() {
        let bar = Rect::from_min_max(pos2(c.value_x(), y - 3.0), pos2(x1, y + 3.0));
        winui::progress_bar(&p, bar, Some(f as f32), ctx.input(|i| i.time));
        y += 9.0;
    }
    // Size: the file's, or how much of it has come.
    let size = match (item.size, complete) {
        (Some(s), true) => crate::i18n::ltr(&crate::model::size(s)),
        (Some(s), false) => crate::i18n::fmt(
            // i18n: the preview pane: how much of a download has come; {done} and {total} sizes, e.g. "12.5 MB of 40.0 MB"
            crate::i18n::t("{done} of {total}"),
            &[
                (
                    "done",
                    &crate::i18n::ltr(&crate::model::size(item.downloaded)),
                ),
                ("total", &crate::i18n::ltr(&crate::model::size(s))),
            ],
        ),
        (None, _) if item.downloaded > 0 => crate::i18n::ltr(&crate::model::size(item.downloaded)),
        // i18n: the preview pane: the size of a download is not known yet
        (None, _) => crate::i18n::t("Not known yet").to_owned(),
    };
    y = field(
        ui,
        c,
        y,
        crate::i18n::t("Size:"),
        &size,
        Value::Plain,
        "size",
    );
    // Whether its file is still there (#1368's second idea).
    if complete {
        let (text, how) = match presence {
            // i18n: the preview pane: a finished download's file is still where it was saved
            Presence::Found { .. } => (crate::i18n::t("Still there"), Value::Plain),
            // i18n: the preview pane: a finished download's file is no longer where it was saved
            Presence::Gone => (crate::i18n::t("Moved or deleted"), Value::Warning),
            Presence::Unreachable => (
                // i18n: the preview pane: neither the file nor its folder can be found now (an unplugged drive, a network folder away)
                crate::i18n::t("Its folder can't be reached now"),
                Value::Warning,
            ),
            // i18n: the preview pane: still looking whether the file is there
            Presence::Looking => (crate::i18n::t("Looking..."), Value::Plain),
        };
        // i18n: label in the preview pane: whether the downloaded file is still where it was saved
        y = field(ui, c, y, crate::i18n::t("File:"), text, how, "presence");
    }
    let folder = item
        .dest
        .parent()
        .map_or_else(String::new, |d| d.display().to_string());
    y = field(
        ui,
        c,
        y,
        crate::i18n::t("Save To:"),
        &folder,
        Value::Data,
        "folder",
    );
    y = field(
        ui,
        c,
        y,
        crate::i18n::t("Address:"),
        item.file_address().as_str(),
        Value::Data,
        "address",
    );
    if let Some(page) = item.web_page()
        && page != item.file_address().as_str()
    {
        y = field(
            ui,
            c,
            y,
            // i18n: label in the preview pane: the web page the download was found on
            crate::i18n::t("Web page:"),
            page,
            Value::Data,
            "page",
        );
    }
    if let Some(added) = &item.added {
        let when = crate::model::date_text(added);
        y = field(
            ui,
            c,
            y,
            // i18n: label in the preview pane: when the download was added to the list
            crate::i18n::t("Added:"),
            &when,
            Value::Plain,
            "added",
        );
    }
    if complete && let Some(done) = &item.finished {
        let when = crate::model::date_text(done);
        y = field(
            ui,
            c,
            y,
            // i18n: label in the preview pane: when the download finished
            crate::i18n::t("Finished:"),
            &when,
            Value::Plain,
            "finished",
        );
    }
    y - 8.0
}

/// The file type's name ("PDF File", "Compressed (zipped) Folder"), once per extension.
fn type_name(file_name: &str) -> String {
    use std::cell::RefCell;
    thread_local! {
        static NAMES: RefCell<HashMap<(String, &'static str), String>> = RefCell::default();
    }
    let ext = Path::new(file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    // Keyed by the interface language too (MDM's own "{ext} File" is translated).
    let key = (ext.clone(), crate::i18n::code());
    NAMES.with(|n| {
        n.borrow_mut()
            .entry(key)
            .or_insert_with(|| crate::properties::file_type(file_name, &ext))
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_made_smaller_never_larger_and_keeps_its_shape() {
        assert_eq!(fitted(4000, 3000, 256), (256, 192));
        assert_eq!(fitted(300, 1200, 256), (64, 256));
        assert_eq!(fitted(100, 50, 256), (100, 50));
        assert_eq!(fitted(10_000, 1, 256), (256, 1));
        let (w, h, px) = fit_within(4, 2, vec![255; 4 * 2 * 4], false, 2);
        assert_eq!((w, h, px.len()), (2, 1, 2 * 4));
    }

    /// The Thumbnail Managing Standard's names, as GLib writes a file's address (the standard's
    /// own example: `file:///home/jens/photos/me.png` → `c6ee772d9e49320e97ec29a7eb5b1697.png`).
    #[test]
    fn thumbnails_are_found_by_the_standards_names() {
        assert_eq!(
            thumbnail_uri(b"/home/jens/photos/me.png"),
            "file:///home/jens/photos/me.png"
        );
        assert_eq!(
            thumbnail_name("file:///home/jens/photos/me.png"),
            "c6ee772d9e49320e97ec29a7eb5b1697.png"
        );
        // Spaces and letters outside ASCII as their bytes, as g_filename_to_uri writes them.
        assert_eq!(
            thumbnail_uri("/tmp/a b/é#1.jpg".as_bytes()),
            "file:///tmp/a%20b/%C3%A9%231.jpg"
        );
    }

    #[test]
    fn a_thumbnails_date_is_read_from_its_text() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0; 4]);
            c
        };
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"tEXt", b"Thumb::URI\x00file:///a.png"));
        png.extend(chunk(b"tEXt", b"Thumb::MTime\x001700000000"));
        png.extend(chunk(b"IDAT", &[0; 4]));
        assert_eq!(
            png_text(&png, "Thumb::MTime").as_deref(),
            Some("1700000000")
        );
        assert_eq!(
            png_text(&png, "Thumb::URI").as_deref(),
            Some("file:///a.png")
        );
        assert_eq!(png_text(&png, "Thumb::Size"), None);
        assert_eq!(png_text(b"not a png", "Thumb::MTime"), None);
        // Cut short: no answer, no panic.
        assert_eq!(png_text(&png[..20], "Thumb::MTime"), None);
    }

    #[test]
    fn several_selected_add_up_their_sizes() {
        let url = url::Url::parse("https://example.com/a.zip").unwrap();
        let mut a = Item::new(
            1,
            url.clone(),
            PathBuf::from("a.zip"),
            crate::model::Category::General,
        );
        a.size = Some(1000);
        a.status = Status::Complete;
        let mut b = Item::new(
            2,
            url.clone(),
            PathBuf::from("b.zip"),
            crate::model::Category::General,
        );
        b.size = Some(24);
        let c = Item::new(
            3,
            url,
            PathBuf::from("c.zip"),
            crate::model::Category::General,
        );
        let s = Summary::of([&a, &b, &c]);
        assert_eq!(
            s,
            Summary {
                n: 3,
                bytes: 1024,
                unknown: 1,
                complete: 1
            }
        );
    }

    /// A file deleted while it is shown is seen gone; one whose folder is gone too, out of
    /// reach (a drive unplugged): looked at on the disk's thread, never the window's.
    #[test]
    fn the_disk_is_looked_at_beside_the_window() {
        let dir = std::env::temp_dir().join(format!("mdm-preview-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("photo.jpg");
        std::fs::write(&file, b"12345").unwrap();
        let ctx = egui::Context::default();
        let wait = |path: &Path| {
            let start = Instant::now();
            loop {
                let (p, _) = presence(&ctx, path);
                if p != Presence::Looking || start.elapsed() > Duration::from_secs(10) {
                    return p;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        assert_eq!(wait(&file), Presence::Found { len: 5, dir: false });
        assert_eq!(wait(&dir.join("gone.jpg")), Presence::Gone);
        assert_eq!(
            wait(&dir.join("no-folder").join("x.jpg")),
            Presence::Unreachable
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

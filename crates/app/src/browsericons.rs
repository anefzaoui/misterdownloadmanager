//! Browsers' logos, before their names (the browser extension's window, the first start's setup,
//! Options → General's browser list): each browser's own artwork, unchanged in shape and colour
//! (assets/browsers/, its sources in SOURCES.txt; the logos are trademarks of their owners, shown
//! only to name the browsers). A name MDM doesn't know, or a browser whose artwork MDM has no
//! source for, gets a neutral globe (Lucide's) in the text's colour: never another browser's logo.
//!
//! Drawn at the screen's pixel size on whole pixels (an SVG rasterized for that size; Yandex's
//! PNGs, which have no SVG, at their own size or resampled from the next larger one), so they
//! stay sharp at every scale, and at their own proportions.

use std::path::Path;

use egui::emath::GuiRounding as _;
use egui::{Color32, ImageSource, Painter, Rect, Ui, pos2, vec2};

/// A browser MDM can show a logo for, or [`Browser::Other`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Browser {
    Chrome,
    Chromium,
    Edge,
    EdgeLegacy,
    InternetExplorer,
    Brave,
    Vivaldi,
    Opera,
    OperaGx,
    Yandex,
    Helium,
    Thorium,
    Firefox,
    Zen,
    LibreWolf,
    Floorp,
    Waterfox,
    Safari,
    /// Unknown, or no artwork to show (Arc): the neutral globe.
    Other,
}

/// The names each browser goes by in MDM: mdm_ipc::browsers' table (the extension's window),
/// Options' list (settings::BROWSERS, capture::browser_named), and what Windows' registry calls
/// one MDM finds by its Start menu entry only ("Opera GX Stable", "Yandex"). Matched whole,
/// ignoring case, or followed by " stable" or " browser" only: "Google Chrome Beta" is another
/// logo, so it gets the globe rather than Chrome's.
const NAMES: &[(&str, Browser)] = &[
    ("google chrome", Browser::Chrome),
    ("chrome", Browser::Chrome),
    ("chromium", Browser::Chromium),
    ("microsoft edge legacy", Browser::EdgeLegacy),
    ("microsoft edge", Browser::Edge),
    ("internet explorer", Browser::InternetExplorer),
    ("brave", Browser::Brave),
    ("vivaldi", Browser::Vivaldi),
    ("opera gx", Browser::OperaGx),
    ("opera", Browser::Opera),
    ("yandex", Browser::Yandex),
    ("helium", Browser::Helium),
    ("thorium", Browser::Thorium),
    ("mozilla firefox", Browser::Firefox),
    ("firefox", Browser::Firefox),
    ("zen", Browser::Zen),
    ("librewolf", Browser::LibreWolf),
    ("floorp", Browser::Floorp),
    ("waterfox", Browser::Waterfox),
    ("apple safari", Browser::Safari),
    ("safari", Browser::Safari),
];

impl Browser {
    /// The browser named `name` (as MDM lists it), [`Browser::Other`] when it isn't one MDM
    /// has a logo for.
    pub fn named(name: &str) -> Browser {
        let n = name.trim().to_lowercase();
        NAMES
            .iter()
            .find(|(known, _)| {
                n.strip_prefix(known)
                    .is_some_and(|rest| matches!(rest, "" | " stable" | " browser"))
            })
            .map_or(Browser::Other, |(_, b)| *b)
    }

    /// The browser named `name` whose program is `program`: Windows names every `opera.exe`
    /// "Opera" (mdm_ipc::browsers::windows_named), and Opera GX's is in a folder of its own.
    pub fn of(name: &str, program: Option<&Path>) -> Browser {
        let named = Browser::named(name);
        let gx = program.is_some_and(|p| {
            p.to_string_lossy()
                .to_lowercase()
                .replace('\\', "/")
                .contains("/opera gx/")
        });
        if named == Browser::Opera && gx {
            Browser::OperaGx
        } else {
            named
        }
    }

    /// Its SVG (assets/browsers/), none for Yandex (PNGs) and Other (the globe).
    fn svg(self) -> Option<(&'static str, &'static [u8])> {
        macro_rules! svg {
            ($n:literal) => {
                Some((
                    $n,
                    include_bytes!(concat!("../assets/browsers/", $n, ".svg")),
                ))
            };
        }
        match self {
            Browser::Chrome => svg!("chrome"),
            Browser::Chromium => svg!("chromium"),
            Browser::Edge => svg!("edge"),
            Browser::EdgeLegacy => svg!("edge-legacy"),
            Browser::InternetExplorer => svg!("internet-explorer"),
            Browser::Brave => svg!("brave"),
            Browser::Vivaldi => svg!("vivaldi"),
            Browser::Opera => svg!("opera"),
            Browser::OperaGx => svg!("opera-gx"),
            Browser::Helium => svg!("helium"),
            Browser::Thorium => svg!("thorium"),
            Browser::Firefox => svg!("firefox"),
            Browser::Zen => svg!("zen"),
            Browser::LibreWolf => svg!("librewolf"),
            Browser::Floorp => svg!("floorp"),
            Browser::Waterfox => svg!("waterfox"),
            Browser::Safari => svg!("safari"),
            Browser::Yandex | Browser::Other => None,
        }
    }
}

/// Yandex's logo has no SVG: its PNGs, by side in pixels (smallest first).
const YANDEX: &[(u32, &[u8])] = &[
    (16, include_bytes!("../assets/browsers/yandex-16.png")),
    (24, include_bytes!("../assets/browsers/yandex-24.png")),
    (32, include_bytes!("../assets/browsers/yandex-32.png")),
    (48, include_bytes!("../assets/browsers/yandex-48.png")),
    (64, include_bytes!("../assets/browsers/yandex-64.png")),
    (128, include_bytes!("../assets/browsers/yandex-128.png")),
];

/// Lucide's "globe" (v1.63.0, ISC), drawn with the ink asked for.
const GLOBE: &str = include_str!("../assets/icons/lucide/globe.svg");

/// Draw `browser`'s logo in `rc` (on screen) through `painter` (so it keeps to a list's clip),
/// at its own proportions, centred, on whole pixels at the screen's pixel size. `ink` colours
/// the globe of [`Browser::Other`] (the row's text colour, so it reads on a selected row too).
pub fn paint(ui: &Ui, painter: &Painter, browser: Browser, rc: Rect, ink: Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let rc = rc.round_to_pixels(ppp);
    let px = (rc.width().min(rc.height()) * ppp)
        .round()
        .clamp(1.0, 512.0) as u32;
    let src = source(browser, px, ink);
    let img = egui::Image::new(src)
        .fit_to_exact_size(rc.size())
        .texture_options(egui::TextureOptions::LINEAR);
    if let Ok(egui::load::TexturePoll::Ready { texture }) = img.load_for_size(ui.ctx(), rc.size()) {
        // The texture is the logo's own shape at this size (Waterfox's is wider than tall):
        // centred in the square on whole pixels, never stretched.
        let (w, h) = (texture.size.x, texture.size.y);
        let scale = (rc.width() / w)
            .min(rc.height() / h)
            .min(1.0 / ppp)
            .max(f32::EPSILON);
        let size = vec2(w, h) * scale;
        let at = Rect::from_center_size(rc.center(), size).round_to_pixels(ppp);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(texture.id, at, uv, Color32::WHITE);
    }
}

/// The image for `browser` at `px` pixels.
fn source(browser: Browser, px: u32, ink: Color32) -> ImageSource<'static> {
    if let Some((name, bytes)) = browser.svg() {
        return ImageSource::Bytes {
            uri: format!("bytes://mdm/browser/{name}.svg").into(),
            bytes: egui::load::Bytes::Static(bytes),
        };
    }
    match browser {
        Browser::Yandex => yandex(px),
        _ => globe(ink),
    }
}

/// Yandex's PNG at `px`: its own when it has that size, else the next larger one resampled
/// (Lanczos) to it, made once per size.
fn yandex(px: u32) -> ImageSource<'static> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static MADE: RefCell<HashMap<u32, ImageSource<'static>>> = RefCell::default();
    }
    if let Some((_, bytes)) = YANDEX.iter().find(|(side, _)| *side == px) {
        return ImageSource::Bytes {
            uri: format!("bytes://mdm/browser/yandex-{px}.png").into(),
            bytes: egui::load::Bytes::Static(bytes),
        };
    }
    MADE.with(|made| {
        made.borrow_mut()
            .entry(px)
            .or_insert_with(|| {
                let (_, from) = YANDEX
                    .iter()
                    .find(|(side, _)| *side >= px)
                    .unwrap_or(&YANDEX[YANDEX.len() - 1]);
                ImageSource::Bytes {
                    uri: format!("bytes://mdm/browser/yandex-{px}-made.png").into(),
                    bytes: egui::load::Bytes::Shared(resampled(from, px).into()),
                }
            })
            .clone()
    })
}

/// The PNG `png` resampled to `px` square, as a PNG (the original when that fails).
fn resampled(png: &'static [u8], px: u32) -> Vec<u8> {
    let made = image::load_from_memory_with_format(png, image::ImageFormat::Png).and_then(|img| {
        let small = image::imageops::resize(
            &img.to_rgba8(),
            px,
            px,
            image::imageops::FilterType::Lanczos3,
        );
        let mut out = std::io::Cursor::new(Vec::new());
        small.write_to(&mut out, image::ImageFormat::Png)?;
        Ok(out.into_inner())
    });
    made.unwrap_or_else(|_| png.to_vec())
}

/// The globe in `ink`, made once per ink.
fn globe(ink: Color32) -> ImageSource<'static> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static MADE: RefCell<HashMap<[u8; 4], ImageSource<'static>>> = RefCell::default();
    }
    MADE.with(|made| {
        made.borrow_mut()
            .entry(ink.to_array())
            .or_insert_with(|| {
                let hex = crate::palette::to_hex(ink);
                ImageSource::Bytes {
                    uri: format!(
                        "bytes://mdm/browser/globe-{}.svg",
                        hex.trim_start_matches('#')
                    )
                    .into(),
                    bytes: egui::load::Bytes::Shared(
                        GLOBE.replace("currentColor", &hex).into_bytes().into(),
                    ),
                }
            })
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Browser] = &[
        Browser::Chrome,
        Browser::Chromium,
        Browser::Edge,
        Browser::EdgeLegacy,
        Browser::InternetExplorer,
        Browser::Brave,
        Browser::Vivaldi,
        Browser::Opera,
        Browser::OperaGx,
        Browser::Yandex,
        Browser::Helium,
        Browser::Thorium,
        Browser::Firefox,
        Browser::Zen,
        Browser::LibreWolf,
        Browser::Floorp,
        Browser::Waterfox,
        Browser::Safari,
        Browser::Other,
    ];

    #[test]
    fn every_browser_mdm_names_has_its_logo() {
        // The extension window's rows (Arc excepted: no artwork to show, the globe).
        for b in mdm_ipc::browsers::BROWSERS {
            let got = Browser::named(b.name);
            if b.name == "Arc" {
                assert_eq!(got, Browser::Other);
            } else {
                assert_ne!(got, Browser::Other, "{}", b.name);
            }
        }
        // Options' list.
        for name in crate::settings::BROWSERS {
            assert_ne!(Browser::named(name), Browser::Other, "{name}");
        }
        assert_eq!(Browser::named("Apple Safari"), Browser::Safari);
        assert_eq!(Browser::named("Brave Browser"), Browser::Brave);
        assert_eq!(Browser::named("Microsoft Edge Legacy"), Browser::EdgeLegacy);
        assert_eq!(Browser::named("Microsoft Edge"), Browser::Edge);
        assert_eq!(Browser::named("Zen Browser"), Browser::Zen);
        assert_eq!(Browser::named("Mozilla Firefox"), Browser::Firefox);
    }

    #[test]
    fn a_browser_named_by_the_registry_only() {
        assert_eq!(Browser::named("Opera Stable"), Browser::Opera);
        assert_eq!(Browser::named("Opera GX Stable"), Browser::OperaGx);
        assert_eq!(Browser::named("opera gx"), Browser::OperaGx);
        assert_eq!(Browser::named("Yandex"), Browser::Yandex);
        assert_eq!(Browser::named(" YANDEX browser "), Browser::Yandex);
    }

    #[test]
    fn an_unknown_name_never_gets_another_browsers_logo() {
        for name in [
            "",
            "Your browser",
            "Arc",
            "Google Chrome Beta",
            "Google Chrome Canary",
            "Microsoft Edge Dev",
            "Firefox Nightly",
            "Opera Neon",
            "Operator",
            "Chromebook",
            "Pale Moon",
            "Internet",
            "Zenith",
        ] {
            assert_eq!(Browser::named(name), Browser::Other, "{name:?}");
        }
    }

    #[test]
    fn opera_gx_is_told_apart_by_its_folder() {
        let gx = Path::new(r"C:\Users\a\AppData\Local\Programs\Opera GX\opera.exe");
        let opera = Path::new(r"C:\Users\a\AppData\Local\Programs\Opera\opera.exe");
        assert_eq!(Browser::of("Opera", Some(gx)), Browser::OperaGx);
        assert_eq!(Browser::of("Opera", Some(opera)), Browser::Opera);
        assert_eq!(Browser::of("Opera", None), Browser::Opera);
        // Only Opera's own program: another browser in such a folder keeps its name's logo.
        assert_eq!(Browser::of("Vivaldi", Some(gx)), Browser::Vivaldi);
    }

    /// Every logo is drawn by the loaders the app installs, at each size a 16-point icon takes
    /// (100 %, 125 %, 150 %, 200 %, 300 %), as large as asked on its longer side, never larger,
    /// and with something to see.
    #[test]
    fn every_logo_draws_at_every_scale() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        for &b in ALL {
            for px in [16u32, 20, 24, 32, 48] {
                let src = source(b, px, Color32::from_rgb(0x20, 0x20, 0x20));
                let ImageSource::Bytes { uri, bytes } = src else {
                    panic!("{b:?}: not bytes")
                };
                ctx.include_bytes(uri.clone(), bytes);
                let hint = egui::load::SizeHint::Size {
                    width: px,
                    height: px,
                    maintain_aspect_ratio: true,
                };
                // An SVG is drawn at once; a PNG is decoded on the loader's own thread, as in
                // the app (drawn the frame after): asked again until it is, within a deadline
                // no decode of a few kilobytes comes near.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                let img = loop {
                    match ctx.try_load_image(&uri, hint) {
                        Ok(egui::load::ImagePoll::Ready { image }) => break image,
                        Ok(_) if std::time::Instant::now() < deadline => std::thread::yield_now(),
                        Ok(_) => panic!("{b:?} at {px}: not decoded in 20 s"),
                        Err(e) => panic!("{b:?} at {px}: {e}"),
                    }
                };
                let [w, h] = img.size;
                assert_eq!(w.max(h), px as usize, "{b:?} at {px}: {w}x{h}");
                // Something to see: at least two rows' worth of solid pixels (Opera GX's is a
                // thin ring, Zen's and the globe's thin lines).
                let seen = img.pixels.iter().filter(|c| c.a() > 128).count();
                assert!(
                    seen >= 2 * px as usize,
                    "{b:?} at {px}: only {seen} pixels drawn"
                );
            }
        }
    }

    #[test]
    fn yandexs_pngs_are_the_size_their_names_say() {
        for (side, bytes) in YANDEX {
            let img = image::load_from_memory(bytes).expect("png");
            assert_eq!((img.width(), img.height()), (*side, *side));
        }
        let made = image::load_from_memory(&resampled(YANDEX[1].1, 20)).expect("png");
        assert_eq!((made.width(), made.height()), (20, 20));
    }
}

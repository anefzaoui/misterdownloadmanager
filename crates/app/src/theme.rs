//! IDM 6.42's dark look (notes/UI.md "Live reference"), as egui visuals.

use egui::{Color32, CornerRadius, Stroke, Visuals};

use std::sync::atomic::{AtomicBool, Ordering};

static DARK: AtomicBool = AtomicBool::new(true);

/// Is the UI in dark mode? IDM: dark when "View → Dark Mode support" is on and Windows' app
/// mode is dark (IDM 6.37); light otherwise. Painting code reads this every frame.
pub fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// Match egui's glyph rendering to the mode: dark mode thickens light-on-dark text
/// (`TwoCoverageMinusCoverageSq`), which makes black-on-white text look heavy, so light mode
/// uses plain coverage. The glyph atlas is shared, so this follows the main window's mode.
pub fn apply_mode(ctx: &egui::Context) {
    let tf = if dark() {
        egui::epaint::FontColorTransferFunction::DARK_MODE_DEFAULT
    } else {
        egui::epaint::FontColorTransferFunction::LIGHT_MODE_DEFAULT
    };
    ctx.all_styles_mut(|s| s.visuals.text_options.color_transfer_function = tf);
    // eframe applies egui's theme preference to every window's caption; keep it in step so it
    // doesn't reset the captions to the system theme.
    ctx.set_theme(if dark() {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    // The window's own theme (caption, DWM): otherwise winit follows the system theme.
    ctx.send_viewport_cmd_to(
        egui::ViewportId::ROOT,
        egui::ViewportCommand::SetTheme(system_theme(dark())),
    );
}

/// The winit window theme for a dark (true) or light caption.
pub fn system_theme(dark: bool) -> egui::SystemTheme {
    if dark {
        egui::SystemTheme::Dark
    } else {
        egui::SystemTheme::Light
    }
}

/// Decide the mode from the setting and the system's: Windows' app mode, elsewhere the light or
/// dark appearance the window system reports (light when it says nothing). True if it changed.
pub fn update(dark_mode_support: bool, ctx: &egui::Context) -> bool {
    #[cfg(windows)]
    let system_dark = {
        let _ = ctx;
        crate::win32::apps_use_dark()
    };
    #[cfg(not(windows))]
    let system_dark = ctx.system_theme() == Some(egui::Theme::Dark);
    let want = dark_mode_support && system_dark;
    DARK.swap(want, Ordering::Relaxed) != want
}

/// Colours that differ between IDM's dark and light modes: each becomes a function of the
/// current mode, `dark()` (main-window.md, menus.md; light-main.md, light-dialogs.md).
macro_rules! themed {
    ($($(#[$m:meta])* $vis:vis $name:ident = $dark:expr, $light:expr;)*) => {$(
        $(#[$m])*
        #[allow(non_snake_case)]
        $vis fn $name() -> Color32 {
            if $crate::theme::dark() { $dark } else { $light }
        }
    )*};
}
pub(crate) use themed;

pub const WINDOW: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
pub const LIST_ALT: Color32 = Color32::from_rgb(0x3e, 0x3e, 0x3e);
pub const TEXT: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);

/// The system's fonts for the scripts the UI font lacks (D-019; nothing bundled): a key, the
/// files to try (in the fonts folder), the interface languages that need it. CJK fonts are large
/// (13–20 MB), so only the interface language's are loaded, and all of them while the language
/// picker shows every language by its own name ([`all_scripts`]). Windows: Segoe UI and Tahoma
/// already cover Arabic, Hebrew, Armenian and Georgian.
#[cfg(windows)]
const SCRIPT_FONTS: &[(&str, &[&str], &[&str])] = &[
    ("han-sc", &["msyh.ttc"], &["zh-hans"]),
    ("han-tc", &["msjh.ttc"], &["zh-hant"]),
    (
        "japanese",
        &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"],
        &["ja"],
    ),
    ("hangul", &["malgun.ttf"], &["ko"]),
    (
        "indic",
        &["Nirmala.ttc", "Nirmala.ttf"],
        &[
            "as", "bn", "gu", "hi", "kn", "kok", "ml", "mr", "ne", "or", "pa", "ta", "te",
        ],
    ),
    ("southeast-asian", &["LeelawUI.ttf"], &["km", "lo", "th"]),
    ("ethiopic", &["ebrima.ttf"], &["am"]),
    ("cherokee", &["gadugi.ttf"], &["chr"]),
];

/// macOS's fonts, in /System/Library/Fonts: the specific ones first, then Arial Unicode for
/// every other script the system font lacks.
#[cfg(target_os = "macos")]
const SCRIPT_FONTS: &[(&str, &[&str], &[&str])] = &[
    (
        "han",
        &["PingFang.ttc", "Supplemental/Songti.ttc"],
        &["zh-hans", "zh-hant"],
    ),
    ("japanese", &["ヒラギノ角ゴシック W3.ttc"], &["ja"]),
    ("hangul", &["AppleSDGothicNeo.ttc"], &["ko"]),
    ("khmer", &["Supplemental/Khmer Sangam MN.ttf"], &["km"]),
    ("lao", &["Supplemental/Lao Sangam MN.ttf"], &["lo"]),
    ("ethiopic", &["Supplemental/Kefa.ttc", "Kefa.ttc"], &["am"]),
    (
        "cherokee",
        &["Supplemental/Plantagenet Cherokee.ttf"],
        &["chr"],
    ),
    (
        "unicode",
        &["Supplemental/Arial Unicode.ttf"],
        &[
            "ar", "fa", "ur", "ug", "he", "hy", "ka", "as", "bn", "gu", "hi", "kn", "kok", "ml",
            "mr", "ne", "or", "pa", "ta", "te", "th", "ja", "ko", "zh-hans", "zh-hant",
        ],
    ),
];

/// Linux: fontconfig names the font for each language (`fc-match :lang=…`), whatever the
/// distribution installs; these are the languages whose script needs one, as fontconfig names
/// them (one per script is enough for the picker's list of all languages).
#[cfg(not(any(windows, target_os = "macos")))]
const SCRIPT_LANGS: &[(&str, &str)] = &[
    ("ar", "ar"),
    ("fa", "fa"),
    ("ur", "ur"),
    ("ug", "ug"),
    ("he", "he"),
    ("hy", "hy"),
    ("ka", "ka"),
    ("am", "am"),
    ("chr", "chr"),
    ("km", "km"),
    ("lo", "lo"),
    ("th", "th"),
    ("ja", "ja"),
    ("ko", "ko"),
    ("zh-hans", "zh-cn"),
    ("zh-hant", "zh-tw"),
    ("hi", "hi"),
    ("mr", "mr"),
    ("ne", "ne"),
    ("kok", "kok"),
    ("as", "as"),
    ("bn", "bn"),
    ("gu", "gu"),
    ("pa", "pa"),
    ("or", "or"),
    ("ta", "ta"),
    ("te", "te"),
    ("kn", "kn"),
    ("ml", "ml"),
];

/// The folder the font tables name files in.
#[cfg(any(windows, target_os = "macos"))]
fn font_dir() -> std::path::PathBuf {
    #[cfg(windows)]
    return std::env::var_os("WINDIR")
        .map(|w| std::path::PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows\Fonts"));
    #[cfg(target_os = "macos")]
    return std::path::PathBuf::from("/System/Library/Fonts");
}

/// The interface font: Segoe UI as IDM has it on Windows, the system's own elsewhere.
fn ui_font() -> Option<Vec<u8>> {
    #[cfg(windows)]
    return std::fs::read(font_dir().join("segoeui.ttf")).ok();
    #[cfg(target_os = "macos")]
    return ["SFNS.ttf", "Helvetica.ttc"]
        .iter()
        .find_map(|f| std::fs::read(font_dir().join(f)).ok());
    #[cfg(not(any(windows, target_os = "macos")))]
    return fc_match("sans-serif").and_then(|f| std::fs::read(f).ok());
}

/// The script fonts for `code` (every language's with `all`): (key, bytes), in fallback order.
fn script_fonts(code: &str, all: bool) -> Vec<(String, Vec<u8>)> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let dir = font_dir();
        SCRIPT_FONTS
            .iter()
            .filter(|(_, _, langs)| all || langs.contains(&code))
            // The first file there is used (older systems name some differently); a .ttc's
            // first face is the regular one.
            .filter_map(|(key, files, _)| {
                let bytes = files.iter().find_map(|f| std::fs::read(dir.join(f)).ok())?;
                Some(((*key).to_owned(), bytes))
            })
            .collect()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let mut seen = std::collections::HashSet::new();
        SCRIPT_LANGS
            .iter()
            .filter(|(ours, _)| all || *ours == code)
            .filter_map(|(_, fc)| fc_match(&format!(":lang={fc}")))
            // Several languages share a font (Devanagari, the CJK collection): loaded once.
            .filter(|f| seen.insert(f.clone()))
            .filter_map(|f| {
                let bytes = std::fs::read(&f).ok()?;
                Some((format!("fc:{}", f.display()), bytes))
            })
            .collect()
    }
}

/// The font file fontconfig picks for `pattern`.
#[cfg(not(any(windows, target_os = "macos")))]
fn fc_match(pattern: &str) -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    let file = String::from_utf8(out.stdout).ok()?;
    (out.status.success() && !file.trim().is_empty()).then(|| file.trim().into())
}

/// The script fonts loaded now: the interface language's, or all of them.
static LOADED: std::sync::Mutex<Option<(String, bool)>> = std::sync::Mutex::new(None);

/// Load the fonts the interface language needs (after it changed).
pub fn language_fonts(ctx: &egui::Context) {
    let all = LOADED.lock().unwrap().as_ref().is_some_and(|l| l.1);
    load_fonts(ctx, all);
}

/// Load every script font, for a list of all languages by their own names; they stay loaded.
pub fn all_scripts(ctx: &egui::Context) {
    load_fonts(ctx, true);
}

fn load_fonts(ctx: &egui::Context, all: bool) {
    let code = crate::i18n::code().to_owned();
    let mut loaded = LOADED.lock().unwrap();
    // Only the script fonts depend on the language: rebuild when that set changes.
    let needs = |code: &str| -> Vec<&str> {
        #[cfg(any(windows, target_os = "macos"))]
        return SCRIPT_FONTS
            .iter()
            .filter(|(_, _, langs)| all || langs.contains(&code))
            .map(|(key, ..)| *key)
            .collect();
        #[cfg(not(any(windows, target_os = "macos")))]
        return SCRIPT_LANGS
            .iter()
            .filter(|(ours, _)| all || *ours == code)
            .map(|(ours, _)| *ours)
            .collect();
    };
    if let Some((old, old_all)) = loaded.as_ref()
        && *old_all == all
        && needs(old) == needs(&code)
    {
        *loaded = Some((code, all));
        return;
    }
    fonts(ctx, &code, all);
    *loaded = Some((code, all));
}

/// IDM draws with Segoe UI 9 pt (12 px at 100 %): the system's own copy, nothing bundled
/// (elsewhere the system's interface font, [`ui_font`]; else egui's default font). Every family
/// falls back to it (Tahoma has no Armenian or Georgian), then to the script fonts for `code`
/// (all with `all`).
fn fonts(ctx: &egui::Context, code: &str, all: bool) {
    let mut defs = egui::FontDefinitions::default();
    let ui = ui_font();
    if let Some(bytes) = &ui {
        defs.font_data.insert(
            "segoe-ui".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes.clone())),
        );
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families
                .entry(fam)
                .or_default()
                .insert(0, "segoe-ui".into());
        }
    }
    #[cfg(windows)]
    crate::winui::install_fonts(&mut defs, Some(&font_dir()));
    #[cfg(not(windows))]
    crate::winui::install_fonts(&mut defs, None);
    // A fallback's glyphs sit on the baseline of the family's first font (Japanese next to
    // Latin digits, Georgian in a Tahoma line), not centred in its row.
    let on_baseline = |bytes: Vec<u8>| {
        let mut tweak = egui::FontTweak::default();
        tweak.set_baseline_fallback(true);
        std::sync::Arc::new(egui::FontData::from_owned(bytes).tweak(tweak))
    };
    let mut fallbacks: Vec<String> = Vec::new();
    if let Some(bytes) = ui {
        defs.font_data
            .insert("segoe-ui-fallback".into(), on_baseline(bytes));
        fallbacks.push("segoe-ui-fallback".into());
    }
    for (key, bytes) in script_fonts(code, all) {
        defs.font_data.insert(key.clone(), on_baseline(bytes));
        fallbacks.push(key);
    }
    for list in defs.families.values_mut() {
        for f in &fallbacks {
            // Segoe UI already leads the two built-in families (as itself, not a fallback).
            if f == "segoe-ui-fallback" && list.first().is_some_and(|l| l == "segoe-ui") {
                continue;
            }
            if !list.contains(f) {
                list.push(f.clone());
            }
        }
    }
    ctx.set_fonts(defs);
    ctx.all_styles_mut(|s| {
        use egui::{FontId, TextStyle};
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(12.0)),
            (TextStyle::Button, FontId::proportional(12.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
            (TextStyle::Heading, FontId::proportional(16.0)),
        ]
        .into();
    });
}

pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx, false);
    egui_extras::install_image_loaders(ctx);
    let mut v = Visuals::dark();
    v.panel_fill = WINDOW;
    v.window_fill = WINDOW;
    v.extreme_bg_color = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    v.faint_bg_color = LIST_ALT;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = crate::winui::SELECTION;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    // Square-ish Windows controls rather than egui's rounded ones.
    let r = CornerRadius::same(2);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3a);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3a);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x6a, 0x6a, 0x6a));
    v.window_corner_radius = CornerRadius::same(0);
    ctx.set_visuals(v);
}

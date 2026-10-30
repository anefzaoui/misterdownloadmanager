//! The interface language (M16, D-019). craft-i18n reads MDM's catalogs (`i18n/<locale>.tsv`,
//! embedded at build time) and looks the English source text up: a missing translation is the
//! English itself. The language follows Windows' display language until the user picks one in
//! View → Language; numbers follow the Windows region (craft-i18n's format).
//!
//! `t("Options")` for a label, `tc("menu", "&Options")` where the same English needs different
//! translations, `tn(n, "{n} file", "{n} files")` for counts. Text that is data (file names,
//! addresses, paths) is never looked up; inside a translated sentence it is wrapped in `ltr()` so
//! it keeps its own direction in a right-to-left language.

use std::sync::{OnceLock, RwLock};

pub use craft_i18n::Lang;
use craft_i18n::{Embedded, I18n};

static CATALOGS: &Embedded = include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

/// The catalogs, indexed once.
pub fn i18n() -> &'static I18n {
    static I: OnceLock<I18n> = OnceLock::new();
    I.get_or_init(|| {
        let offered: Vec<&str> = CATALOGS.iter().map(|(code, _, _)| *code).collect();
        let (i, problems) = I18n::new(CATALOGS, &offered);
        for p in problems {
            eprintln!("translations: {p}");
        }
        i
    })
}

static CURRENT: RwLock<Option<Lang>> = RwLock::new(None);

/// The interface language: the one chosen, else Windows' display language (English when no
/// catalog matches it).
pub fn lang() -> Lang {
    if let Some(l) = *CURRENT.read().unwrap() {
        return l;
    }
    let l = i18n().system_lang();
    *CURRENT.write().unwrap() = Some(l);
    l
}

/// Use `code` (a catalog's locale, e.g. `ar`, `pt-br`), or follow Windows again with `None`.
pub fn set_lang(code: Option<&str>) {
    let l = code
        .and_then(|c| i18n().from_code(c))
        .unwrap_or_else(|| i18n().system_lang());
    *CURRENT.write().unwrap() = Some(l);
}

/// The interface language's code (`en`, `ar`…).
pub fn code() -> &'static str {
    i18n().code(lang())
}

/// Is the interface written right to left (Arabic, Persian, Hebrew, Urdu, Uyghur)? Then every
/// window is laid out from the right (winui's surfaces).
pub fn rtl() -> bool {
    i18n().rtl(lang())
}

/// Marks `source` for the catalog (tools/i18n/extract.py) where the lookup happens later, e.g.
/// a menu label chosen here and translated when the menu is drawn. Returns it unchanged.
pub const fn mark(source: &'static str) -> &'static str {
    source
}

/// The translation of `source` (English); `source` itself when there is none.
pub fn t(source: &str) -> &str {
    i18n().t(lang(), source)
}

/// The translation of `source` in `context` (the same English with different meanings).
pub fn tc<'a>(context: &str, source: &'a str) -> &'a str {
    i18n().tc(lang(), context, source)
}

/// A count: `one` / `other` templates with `{n}` (plural rules of the language).
pub fn tn(n: u64, one: &str, other: &str) -> String {
    i18n().trn(lang(), n, one, other)
}

/// Fill a translated template's `{name}` slots.
pub fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    craft_i18n::fmt(template, args)
}

/// Data that reads left to right in every language (a path, an address, a size, a date, a
/// speed): a left-to-right isolate, inside a translated sentence or alone.
pub fn ltr(value: &str) -> String {
    craft_i18n::ltr(value)
}

/// Data with a direction of its own (a file name, a description) shown alone where interface
/// text goes: a first-strong isolate, so an Arabic name reads right to left in an English window
/// and a Latin one left to right in an Arabic window (interface text is laid out in the
/// interface's direction).
pub fn isolate(value: &str) -> String {
    craft_i18n::isolate(value)
}

/// A whole number as the interface language writes it (English: plain digits, as IDM).
pub fn integer(n: u64) -> String {
    if code() == "en" {
        n.to_string()
    } else {
        i18n().format().integer(n)
    }
}

/// Every language offered, English first, each by its own name, for View → Language.
pub fn languages() -> Vec<(Lang, &'static str, &'static str)> {
    let i = i18n();
    i.languages()
        .into_iter()
        .map(|l| {
            (
                l,
                i.code(l),
                info(i.code(l)).map_or(i.name(l), |x| x.native),
            )
        })
        .collect()
}

/// A row of `i18n/languages.tsv`: the code, the English name, the language's own name and its
/// flag (lipis/flag-icons, SVG).
pub struct Info {
    pub code: &'static str,
    pub english: &'static str,
    pub native: &'static str,
    pub flag: &'static [u8],
}

type Row = (&'static str, &'static str, &'static str, &'static [u8]);
static TABLE: &[Row] = include!(concat!(env!("OUT_DIR"), "/languages.rs"));

/// The table's row for `code` (a catalog's locale, any case).
pub fn info(code: &str) -> Option<Info> {
    TABLE
        .iter()
        .find(|r| r.0.eq_ignore_ascii_case(code))
        .map(|&(code, english, native, flag)| Info {
            code,
            english,
            native,
            flag,
        })
}

/// The code of Windows' display language as MDM offers it (English when none fits).
pub fn system_code() -> &'static str {
    i18n().code(i18n().system_lang())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LRI: char = '\u{2066}';
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';

    /// Every embedded catalog parses with no problem (craft-i18n's own checks: placeholders,
    /// plural forms, the trailing ellipsis…), and every language of i18n/languages.tsv has one.
    #[test]
    fn every_catalog_is_clean_and_offered() {
        let problems = i18n().check_all();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        for row in TABLE.iter().filter(|r| r.0 != "en") {
            assert!(
                i18n().from_code(row.0).is_some(),
                "no catalog for {}",
                row.0
            );
        }
    }

    #[test]
    fn data_goes_in_as_isolates() {
        // A path or an address reads left to right; a file name takes its own direction.
        assert_eq!(ltr(r"C:\dl\a.zip"), format!(r"{LRI}C:\dl\a.zip{PDI}"));
        assert_eq!(isolate("ملف.zip"), format!("{FSI}ملف.zip{PDI}"));
        // In a right-to-left sentence the values stay isolated (one isolate is kept, not
        // doubled), so a Latin name stays where the translator put it.
        let s = fmt("حذف \"{name}\"؟", &[("name", &isolate("report.zip"))]);
        assert_eq!(s, format!("حذف \"{FSI}report.zip{PDI}\"؟"));
        // English text with English values gets no marks; an Arabic name in an English
        // sentence is isolated, so it can't reorder the sentence.
        assert_eq!(
            fmt("Delete \"{name}\"?", &[("name", "a.zip")]),
            "Delete \"a.zip\"?"
        );
        assert_eq!(
            fmt("Delete \"{name}\"?", &[("name", "ملف.zip")]),
            format!("Delete \"{FSI}ملف.zip{PDI}\"?")
        );
    }

    #[test]
    fn right_to_left_languages() {
        let i = i18n();
        for code in ["ar", "fa", "he", "ur", "ug"] {
            if let Some(l) = i.from_code(code) {
                assert!(i.rtl(l), "{code}");
            }
        }
        assert!(!i.rtl(Lang::EN));
    }
}

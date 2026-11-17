//! Options → Save to → Sites (#385): the downloads of one category that come from one site, in
//! a folder of their own.
//!
//! A rule names a category (its key), a site, and the folder. The site is a host, or a `*.` form
//! covering it and its subdomains — `*.example.com` covers `example.com` and `cdn.example.com`,
//! as the browser's own rules do. When several rules name the site, the most specific wins
//! (`www.example.com` over `*.example.com`), so a site and a whole family can sit together.
//!
//! The site is the *page* the download was started from (a browser's Referer), not the file's own
//! address: a video's address is `googlevideo.com` and a file host hands out a CDN link, so a rule
//! for `*.youtube.com` would never name the file. The file's host is read only when there is no
//! page.

use std::path::{Path, PathBuf};

use crate::model::Category;

/// A rule: the category it belongs to (its key), the site, and the folder.
pub type Rule = (String, String, String);

/// Whether `host` is the one `rule` names: the same host, or a `*.` form covering it. Case is
/// ignored, as is a trailing dot (a fully qualified name's).
pub fn host_matches(rule: &str, host: &str) -> bool {
    let rule = rule.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if rule.is_empty() || host.is_empty() {
        return false;
    }
    match rule.strip_prefix("*.") {
        Some(suffix) => host == suffix || host.ends_with(&format!(".{suffix}")),
        None => host == rule,
    }
}

/// How specific a rule's site is: its length without the `*.` (a longer one names more of the
/// host, so it wins).
fn specificity(rule: &str) -> usize {
    rule.trim().trim_start_matches("*.").len()
}

/// The folder a download of `category` goes to: the most specific site rule for the page it was
/// started from (`page`), else for the file's own `host` when there is no page, else the
/// category's own folder (Options → Save to, or the default under `downloads`).
pub fn folder_for(
    category: Category,
    host: Option<&str>,
    page: Option<&str>,
    downloads: &Path,
    dirs: &[(String, String)],
    sites: &[Rule],
) -> PathBuf {
    let key = category.key();
    let site = |h: &str| {
        sites
            .iter()
            .filter(|(c, rule, folder)| {
                *c == *key && !folder.trim().is_empty() && host_matches(rule, h)
            })
            .max_by_key(|(_, rule, _)| specificity(rule))
    };
    // The page decides when it is known; the file's own host only when it isn't.
    let best = match page {
        Some(page) => site(page),
        None => host.and_then(site),
    };
    if let Some((_, _, folder)) = best {
        return crate::dialogs::resolve_path(folder);
    }
    crate::model::category_folder(category, downloads, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> Vec<(String, String)> {
        vec![("Video".to_owned(), "/chosen/video".to_owned())]
    }

    fn rule(category: &str, host: &str, folder: &str) -> Rule {
        (category.to_owned(), host.to_owned(), folder.to_owned())
    }

    #[test]
    fn a_site_is_its_host_or_a_star_form_of_it() {
        assert!(host_matches("example.com", "example.com"));
        assert!(host_matches("Example.COM", "example.com."));
        assert!(
            !host_matches("example.com", "cdn.example.com"),
            "an exact host names itself only"
        );
        assert!(host_matches("*.example.com", "cdn.example.com"));
        assert!(
            host_matches("*.example.com", "example.com"),
            "the family's own host too"
        );
        assert!(
            !host_matches("*.example.com", "notexample.com"),
            "no false suffix"
        );
        assert!(!host_matches("", "example.com"));
        assert!(!host_matches("*.", "example.com"));
    }

    #[test]
    fn the_most_specific_rule_wins() {
        let sites = [
            rule("Video", "*.example.com", "/family"),
            rule("Video", "www.example.com", "/only-www"),
            rule("Video", "*.com", "/everything"),
        ];
        let got = folder_for(
            Category::Video,
            Some("www.example.com"),
            None,
            Path::new("/dl"),
            &[],
            &sites,
        );
        assert_eq!(got, crate::dialogs::resolve_path("/only-www"));
        let got = folder_for(
            Category::Video,
            Some("cdn.example.com"),
            None,
            Path::new("/dl"),
            &[],
            &sites,
        );
        assert_eq!(got, crate::dialogs::resolve_path("/family"));
    }

    /// The page a download was started from is the site a person means: a video's own address is
    /// `googlevideo.com`, a file host hands out a CDN link. The file's host is read only when no
    /// page is known.
    #[test]
    fn a_rule_names_the_page_not_the_file() {
        let sites = [
            rule("Video", "*.youtube.com", "/youtube"),
            rule("Video", "cdn.example.net", "/example"),
        ];
        // Started from YouTube, the file comes from googlevideo.com: the page matches.
        let got = folder_for(
            Category::Video,
            Some("rr1---sn.googlevideo.com"),
            Some("www.youtube.com"),
            Path::new("/dl"),
            &[],
            &sites,
        );
        assert_eq!(got, crate::dialogs::resolve_path("/youtube"));
        // A page no rule names: the file's CDN host is not the site, so the category decides.
        let got = folder_for(
            Category::Video,
            Some("cdn.example.net"),
            Some("news.test"),
            Path::new("/dl"),
            &[],
            &sites,
        );
        assert_eq!(
            got,
            crate::model::category_folder(Category::Video, Path::new("/dl"), &[])
        );
        // No page at all (a link handed over without one): the file's host is read.
        let got = folder_for(
            Category::Video,
            Some("cdn.example.net"),
            None,
            Path::new("/dl"),
            &[],
            &sites,
        );
        assert_eq!(got, crate::dialogs::resolve_path("/example"));
    }

    #[test]
    fn a_rule_belongs_to_its_category_and_beats_its_folder() {
        let sites = [rule("Music", "example.com", "/music")];
        // Another category's rule is not this one's: the category's own folder decides.
        let got = folder_for(
            Category::Video,
            Some("example.com"),
            None,
            Path::new("/dl"),
            &dirs(),
            &sites,
        );
        assert_eq!(
            got,
            crate::model::category_folder(Category::Video, Path::new("/dl"), &dirs())
        );
        // The same rule for this category beats it.
        let sites = [rule("Video", "example.com", "/from-site")];
        let got = folder_for(
            Category::Video,
            Some("example.com"),
            None,
            Path::new("/dl"),
            &dirs(),
            &sites,
        );
        assert_eq!(got, crate::dialogs::resolve_path("/from-site"));
    }

    #[test]
    fn without_a_site_or_a_rule_the_category_decides() {
        let sites = [rule("Video", "example.com", "/from-site")];
        let chosen = crate::model::category_folder(Category::Video, Path::new("/dl"), &dirs());
        // An address with no host (a magnet) never matches.
        assert_eq!(
            folder_for(
                Category::Video,
                None,
                None,
                Path::new("/dl"),
                &dirs(),
                &sites
            ),
            chosen
        );
        // A host no rule names: the category's folder, or the default when it has none.
        assert_eq!(
            folder_for(
                Category::Video,
                Some("other.test"),
                None,
                Path::new("/dl"),
                &dirs(),
                &sites
            ),
            chosen
        );
        assert_eq!(
            folder_for(
                Category::Video,
                Some("other.test"),
                None,
                Path::new("/dl"),
                &[],
                &sites
            ),
            crate::model::category_folder(Category::Video, Path::new("/dl"), &[])
        );
        // A rule whose folder is empty is not one.
        let sites = [rule("Video", "example.com", "   ")];
        assert_eq!(
            folder_for(
                Category::Video,
                Some("example.com"),
                None,
                Path::new("/dl"),
                &dirs(),
                &sites
            ),
            chosen
        );
    }
}

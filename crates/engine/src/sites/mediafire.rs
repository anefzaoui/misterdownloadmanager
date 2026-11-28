//! MediaFire (#952). A file's page (`www.mediafire.com/file/<key>/<name>/file`) holds its
//! download button, whose address (`download<N>.mediafire.com/<ticket>/<key>/<name>`) gives the
//! file for a few hours only. Asked later, that address redirects to MediaFire's "repair" page
//! (`www.mediafire.com/download_repair.php?…&qkey=<key>`), a web page: a download taken from the
//! browser and resumed after a pause, a broken connection or a night failed with "a web page
//! instead of the file". The file's page is such an address's origin
//! (`download::Ctx::refresh_target`): asked again, it gives today's address, which the download
//! goes on from. Pure functions over addresses and page text, asked by `download` through `sites`.
//!
//! Learned from MediaFire's public pages (a public file, without signing in). Pages are matched
//! on their structure (the button's `id`, the addresses' shapes), never on their words.

use url::Url;

use super::PageSays;
use crate::html;

/// The site's name, and its pages' host.
const SITE: &str = "mediafire.com";
const WWW: &str = "www.mediafire.com";
/// The page an expired file address redirects to.
const REPAIR: &str = "/download_repair.php";
/// The file page's download button (`<a id="downloadButton" href="…">`).
const BUTTON: &str = "downloadButton";

/// MediaFire's own hosts: `mediafire.com` and its subdomains (never a look-alike such as
/// `mediafire.com.example` or `notmediafire.com`), over http(s).
pub(crate) fn is_site(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|h| {
            let h = h.to_ascii_lowercase();
            h == SITE || h.ends_with(".mediafire.com")
        })
}

/// One of the hosts files come from: `download<N>.mediafire.com`.
fn is_file_host(url: &Url) -> bool {
    is_site(url)
        && url.host_str().is_some_and(|h| {
            let h = h.to_ascii_lowercase();
            h.strip_suffix(".mediafire.com")
                .and_then(|sub| sub.strip_prefix("download"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// A file's key: letters and digits (15 today).
fn is_key(s: &str) -> bool {
    (6..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The file's page for its key (what MediaFire's own repair page links to), on its pages' host,
/// as `from` is asked (its scheme and port: https in the wild, the lab's port in tests).
fn file_page(from: &Url, key: &str) -> Option<Url> {
    let mut page = from.clone();
    page.set_host(Some(WWW)).ok()?;
    page.set_path(&format!("/file/{key}"));
    page.set_query(None);
    page.set_fragment(None);
    Some(page)
}

/// A file address's key: `/<ticket>/<key>/<name>` on a file host.
fn file_key(url: &Url) -> Option<&str> {
    if !is_file_host(url) {
        return None;
    }
    let mut parts = url.path_segments()?;
    let (ticket, key, name) = (parts.next()?, parts.next()?, parts.next()?);
    (!ticket.is_empty() && is_key(key) && !name.is_empty()).then_some(key)
}

/// The file's page for its address on a file host, which expires: the address to ask for
/// today's (`sites::origin`). None for any other address.
pub(crate) fn origin(url: &Url) -> Option<Url> {
    file_page(url, file_key(url)?)
}

/// `at` is MediaFire's address for a file (a file host's, a file's page, the repair page): a web
/// page from it without a file name is the site talking, never the file.
pub(crate) fn expects_file(at: &Url) -> bool {
    if !is_site(at) {
        return false;
    }
    if is_file_host(at) {
        return true;
    }
    let first = at.path_segments().and_then(|mut s| s.next()).unwrap_or("");
    at.path() == REPAIR || matches!(first, "file" | "file_premium" | "download")
}

/// What MediaFire's page `page`, answered 200 at `at`, says:
/// 1. the repair page (an expired address's): the file's page, by the key in its address;
/// 2. a file's page: its download button's address, on a file host;
/// 3. nothing known (a removed file's page, any other page).
pub(crate) fn read_page(at: &Url, page: &str) -> PageSays {
    if !is_site(at) || is_file_host(at) {
        return PageSays::Nothing;
    }
    if at.path() == REPAIR {
        return at
            .query_pairs()
            .find(|(k, _)| k == "qkey")
            .filter(|(_, v)| is_key(v))
            .and_then(|(_, key)| file_page(at, &key))
            .map_or(PageSays::Nothing, PageSays::Next);
    }
    html::tags(page, "a")
        .filter(|(_, tag)| html::attribute(tag, "id").is_some_and(|id| html::decode(&id) == BUTTON))
        .filter_map(|(_, tag)| html::attribute(tag, "href"))
        .filter_map(|href| at.join(html::decode(&href).trim()).ok())
        .find(|next| file_key(next).is_some())
        .map_or(PageSays::Nothing, PageSays::Next)
}

/// A file page's key: `/file/<key>[/<name>[/file]]`, `/file_premium/<key>…`, `/download/<key>…`
/// on MediaFire's pages' host.
fn page_key(url: &Url) -> Option<&str> {
    if !is_site(url) || is_file_host(url) {
        return None;
    }
    let mut parts = url.path_segments()?;
    let kind = parts.next()?;
    let key = parts.next()?;
    (matches!(kind, "file" | "file_premium" | "download") && is_key(key)).then_some(key)
}

/// The file's name a file page's address carries (`/file/<key>/<name>/file`,
/// `/file_premium/<key>/<name>`), decoded: what its row is called before any look-up, and when one
/// fails, instead of the address's last part (`file`). None for any other address.
pub(crate) fn name_in_address(url: &Url) -> Option<String> {
    page_key(url)?;
    let mut parts = url.path_segments()?;
    let kind = parts.next()?;
    let name = parts.nth(1)?;
    let name = percent_encoding::percent_decode_str(name).decode_utf8_lossy();
    let name = name.trim();
    (matches!(kind, "file" | "file_premium") && !name.is_empty() && name != "file")
        .then(|| name.to_owned())
}

/// MediaFire's public API (`api/1.5/file/get_info.php`, documented at
/// <https://www.mediafire.com/developers/core_api/1.5/>) for a file page's name and size: one
/// small JSON answer instead of the page, which is what the site's bot check watches
/// (JDownloader's MediaFire plugin looks links up the same way). None for any other address.
pub(crate) fn info_api(url: &Url) -> Option<Url> {
    let key = page_key(url)?;
    let mut api = url.clone();
    api.set_host(Some(WWW)).ok()?;
    api.set_path("/api/1.5/file/get_info.php");
    api.set_query(Some(&format!("quick_key={key}&response_format=json")));
    api.set_fragment(None);
    Some(api)
}

/// What the API says of the file: Some((name, size)) when it has it, Err(()) when MediaFire
/// says there is no such file, None when the answer isn't understood (the page is asked then).
pub(crate) fn read_info(answer: &[u8]) -> Option<Result<(String, Option<u64>), ()>> {
    let v: serde_json::Value = serde_json::from_slice(answer).ok()?;
    let r = v.get("response")?;
    let info = r
        .get("file_info")
        .or_else(|| r.get("file_infos").and_then(|l| l.get(0)))
        .filter(|i| i.is_object());
    match info {
        Some(i) => {
            let name = i.get("filename")?.as_str()?.trim();
            if name.is_empty() || name.contains(['/', '\\']) {
                return None;
            }
            let size = i.get("size").and_then(|s| {
                s.as_u64()
                    .or_else(|| s.as_str().and_then(|s| s.parse().ok()))
            });
            Some(Ok((name.to_owned(), size)))
        }
        // "Unknown or invalid QuickKey" (error 110), or success with nothing for the key.
        None if r.get("error").and_then(|e| e.as_u64()) == Some(110)
            || r.get("result").and_then(|e| e.as_str()) == Some("Success") =>
        {
            Some(Err(()))
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    const FILE: &str = "https://download850.mediafire.com/w6kt4imij6hgYO_vTSXBgkKJTdz0bgmrxu/k4makp2pk726bmb/Fonts%20and%20more.zip";

    /// A file page's button as MediaFire's are written: attributes on their own lines, other
    /// links (the sign-in, the app) before it, written here (not copied from MediaFire).
    fn file_page_html(href: &str) -> String {
        format!(
            r#"<!DOCTYPE html><html><head><title>Fonts - MediaFire</title></head><body>
<a href="/login/">Log in</a>
<a class="download_link" href="https://app.mediafire.example/get">Get the app</a>
<div class="download_link"><a class="input popsok"
           aria-label="Download file"
           href="{href}"           id="downloadButton"
           rel="nofollow">Download (1.2MB)</a></div>
</body></html>"#
        )
    }

    #[test]
    fn a_file_address_has_its_page_as_origin() {
        assert_eq!(
            origin(&url(FILE)),
            Some(url("https://www.mediafire.com/file/k4makp2pk726bmb"))
        );
        // Asked as the address is (http, a port: the lab's tests).
        assert_eq!(
            origin(&url(
                "http://DOWNLOAD1589.MediaFire.com:8080/c89ag42y1swg7/k4makp2pk726bmb/a.bin?x=1#y"
            )),
            Some(url("http://www.mediafire.com:8080/file/k4makp2pk726bmb"))
        );
        for other in [
            // Its pages, the site itself: not a file's address.
            "https://www.mediafire.com/file/k4makp2pk726bmb/Fonts.zip/file",
            "https://mediafire.com/",
            // Not a file host: no number, another name, a look-alike.
            "https://download.mediafire.com/t/k4makp2pk726bmb/a.zip",
            "https://downloadx1.mediafire.com/t/k4makp2pk726bmb/a.zip",
            "https://download850.mediafire.com.example/t/k4makp2pk726bmb/a.zip",
            "https://download850.notmediafire.com/t/k4makp2pk726bmb/a.zip",
            // Not a file's path.
            "https://download850.mediafire.com/t/k4makp2pk726bmb",
            "https://download850.mediafire.com/t/k4m/a.zip",
            "https://download850.mediafire.com/t/key-with-dash/a.zip",
            "ftp://download850.mediafire.com/t/k4makp2pk726bmb/a.zip",
        ] {
            assert_eq!(origin(&url(other)), None, "{other}");
        }
    }

    #[test]
    fn a_file_page_gives_its_button() {
        let at = url("https://www.mediafire.com/file/k4makp2pk726bmb/Fonts.zip/file");
        assert_eq!(
            read_page(&at, &file_page_html(FILE)),
            PageSays::Next(url(FILE))
        );
        // Entities in the address, and the other ways to a file's page.
        let amp = FILE.replace("Fonts%20and%20more.zip", "a.zip?b=1&amp;c=2");
        for at in [
            "https://www.mediafire.com/file_premium/k4makp2pk726bmb/Fonts.zip",
            "https://www.mediafire.com/download/k4makp2pk726bmb",
        ] {
            assert_eq!(
                read_page(&url(at), &file_page_html(&amp)),
                PageSays::Next(url(&FILE.replace("Fonts%20and%20more.zip", "a.zip?b=1&c=2"))),
                "{at}"
            );
        }
        // A button that doesn't lead to a file host, a page without one, another site's page:
        // nothing known.
        for (at, page) in [
            (
                at.as_str(),
                file_page_html("https://www.mediafire.com/upgrade/"),
            ),
            (at.as_str(), file_page_html("javascript:void(0)")),
            (
                at.as_str(),
                "<html><a href=\"/login/\">Log in</a></html>".to_owned(),
            ),
            ("https://example.com/file/x", file_page_html(FILE)),
        ] {
            assert_eq!(read_page(&url(at), &page), PageSays::Nothing, "{at}");
        }
    }

    /// The page an expired address redirects to leads back to the file's page.
    #[test]
    fn the_repair_page_leads_to_the_files_page() {
        let repair = url(
            "https://www.mediafire.com/download_repair.php?flag=1&dkey=w6kt4imij6hg&qkey=k4makp2pk726bmb&ip=192%2E0%2E2%2E1&ref=3",
        );
        assert_eq!(
            read_page(&repair, "<html><a href=\"/help/\">Help</a></html>"),
            PageSays::Next(url("https://www.mediafire.com/file/k4makp2pk726bmb"))
        );
        for bad in [
            "https://www.mediafire.com/download_repair.php?flag=1",
            "https://www.mediafire.com/download_repair.php?qkey=..%2F..",
        ] {
            assert_eq!(read_page(&url(bad), ""), PageSays::Nothing, "{bad}");
        }
    }

    #[test]
    fn its_pages_and_file_hosts_expect_files() {
        for at in [
            FILE,
            "https://www.mediafire.com/file/k4makp2pk726bmb/Fonts.zip/file",
            "https://www.mediafire.com/file_premium/k4makp2pk726bmb/Fonts.zip",
            "https://www.mediafire.com/download/k4makp2pk726bmb",
            "https://www.mediafire.com/download_repair.php?qkey=k4makp2pk726bmb",
        ] {
            assert!(expects_file(&url(at)), "{at}");
        }
        for at in [
            "https://www.mediafire.com/",
            "https://www.mediafire.com/folder/abc123/Stuff",
            "https://blog.mediafire.com/post",
            "https://example.com/file/k4makp2pk726bmb",
        ] {
            assert!(!expects_file(&url(at)), "{at}");
        }
    }

    #[test]
    fn file_pages_are_looked_up_through_the_api() {
        assert_eq!(
            info_api(&url(
                "https://www.mediafire.com/file/bh3c2xcde57eqpf/a.part01.rar/file"
            )),
            Some(url(
                "https://www.mediafire.com/api/1.5/file/get_info.php?quick_key=bh3c2xcde57eqpf&response_format=json"
            ))
        );
        for other in [
            FILE,
            "https://www.mediafire.com/folder/abc123/x",
            "https://example.com/file/bh3c2xcde57eqpf/a",
        ] {
            assert_eq!(info_api(&url(other)), None, "{other}");
        }
        let one = br#"{"response":{"action":"file/get_info","file_info":{"quickkey":"k","filename":"a.part01.rar","size":"5315022028"},"result":"Success"}}"#;
        assert_eq!(
            read_info(one),
            Some(Ok(("a.part01.rar".into(), Some(5315022028))))
        );
        let list = br#"{"response":{"file_infos":[{"quickkey":"k","filename":"b.rar","size":"12"}],"result":"Success"}}"#;
        assert_eq!(read_info(list), Some(Ok(("b.rar".into(), Some(12)))));
        let gone = br#"{"response":{"action":"file/get_info","message":"Unknown or invalid QuickKey","error":110,"result":"Error"}}"#;
        assert_eq!(read_info(gone), Some(Err(())));
        for odd in [
            &b"<html>"[..],
            br#"{"response":{"result":"Error","error":1}}"#,
            br#"{"response":{"file_info":{"filename":"../x"}}}"#,
        ] {
            assert_eq!(read_info(odd), None);
        }
    }

    /// A file page's address names its file: the row's name before (and without) a look-up.
    #[test]
    fn a_file_pages_address_names_its_file() {
        let name = |u: &str| name_in_address(&url(u));
        assert_eq!(
            name("https://www.mediafire.com/file/k4makp2pk726bmb/Fonts%20and%20more.zip/file")
                .as_deref(),
            Some("Fonts and more.zip")
        );
        assert_eq!(
            name("https://www.mediafire.com/file_premium/k4makp2pk726bmb/a.part1.rar").as_deref(),
            Some("a.part1.rar")
        );
        for none in [
            "https://www.mediafire.com/file/k4makp2pk726bmb",
            "https://www.mediafire.com/file/k4makp2pk726bmb/file",
            "https://www.mediafire.com/download/k4makp2pk726bmb",
            FILE,
            "https://example.com/file/k4makp2pk726bmb/a.zip/file",
        ] {
            assert_eq!(name(none), None, "{none}");
        }
    }
}

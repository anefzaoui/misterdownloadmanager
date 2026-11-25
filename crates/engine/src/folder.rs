//! Folders for Link grabber (#1774): the files of an `ftp://` folder, or of the listing page a
//! web server makes of a folder ("Index of /…": Apache, nginx, lighttpd and the like, IIS's
//! "host - /path/", Python's "Directory listing for /…"), and of their subfolders, each file with
//! the subfolders it is in.
//!
//! A listing is the server's word, so it is read defensively. An entry is a name *in* the
//! folder, matched by name: a link to another site, to the folder above (`../`), to the folder
//! itself, with a query (a listing's sort order), or whose name isn't a plain name (`a/b`, `..`,
//! control characters) is no entry, and an FTP folder's links (symbolic links) aren't followed.
//! Subfolders are listed down to the depth asked, at most `MAX_DEPTH`, at most `MAX_FOLDERS` of
//! them and `MAX_FILES` files in all, each listing read up to `LISTING_MAX` bytes. A subfolder
//! whose listing comes from another address (a redirect elsewhere) is left out.

use std::collections::{HashSet, VecDeque};

use percent_encoding::percent_decode_str;
use reqwest::header::HeaderMap;
use url::Url;

use crate::download::{Error, Options, PAGE_LINKS_MAX, web_page};
use crate::html;

/// The deepest subfolders listed, below the folder given.
pub const MAX_DEPTH: u8 = 10;
/// The most files a folder lists, its subfolders' included.
pub const MAX_FILES: usize = 10_000;
/// The most folders listed for one folder given (it, and its subfolders).
pub const MAX_FOLDERS: usize = 500;
/// The most of one listing read (a page, an FTP folder's list).
pub(crate) const LISTING_MAX: usize = PAGE_LINKS_MAX;

/// One name a listing gives: a file (with its size when the listing says it) or a folder, and
/// its address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: Option<u64>,
    pub url: Url,
}

/// One file of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderFile {
    pub url: Url,
    /// The listing it is in (its folder's address).
    pub listing: Url,
    /// The subfolders it is in, below the folder given, each a plain name.
    pub folders: Vec<String>,
    /// As the listing says it (FTP's), else None.
    pub size: Option<u64>,
}

/// A folder's files, its subfolders' included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folder {
    /// Its own name (the address's last folder, else the server's name), a plain name.
    pub name: String,
    pub files: Vec<FolderFile>,
    /// More files or folders than were listed (`MAX_FILES`, `MAX_FOLDERS`).
    pub capped: bool,
}

/// What an address given to Link grabber turned out to be.
#[derive(Debug)]
pub enum Read {
    /// A web page, with its links (#705) and the address it came from.
    Page(Url, Vec<Url>),
    Folder(Folder),
}

/// Is `name` a plain name, one a folder can hold: no `/` or `\`, no control characters, and not
/// `.`, `..` or nothing, also as Windows reads a name, without its trailing dots and spaces
/// (`...`, `.. `)?
pub fn plain_name(name: &str) -> bool {
    !name.trim_end_matches(['.', ' ']).is_empty()
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

/// Is `url` an FTP folder's address (its path empty or ending in `/`)?
pub fn is_ftp_folder(url: &Url) -> bool {
    crate::ftp::is_ftp(url) && url.path().ends_with('/')
}

/// The address given read for Link grabber: an FTP folder (`is_ftp_folder`) or a web server's
/// listing page is a [`Folder`] with its subfolders down to `depth` (at most `MAX_DEPTH`) and
/// the files whose names `keep` takes; any other web page its links (`html::links`), as
/// `download::page_links` reads them. An answer that isn't a web page has no links.
pub async fn read(
    url: Url,
    headers: HeaderMap,
    opts: &Options,
    depth: u8,
    keep: impl Fn(&str) -> bool,
) -> Result<Read, Error> {
    if crate::ftp::is_ftp(&url) {
        let mut lister = crate::ftp::Lister::open(&url, &headers, opts).await?;
        let first = lister.list(&url).await?;
        let mut list = Lister::Ftp(Box::new(lister));
        let folder = walk(&mut list, url, first, depth, &keep).await;
        if let Lister::Ftp(l) = list {
            l.close().await;
        }
        return Ok(Read::Folder(folder));
    }
    let (at, page) = web_page(url, headers.clone(), opts, PAGE_LINKS_MAX).await?;
    let Some(page) = page else {
        return Ok(Read::Page(at, Vec::new()));
    };
    match index_entries(&page, &at) {
        Some(first) => {
            let mut list = Lister::Http { headers, opts };
            Ok(Read::Folder(walk(&mut list, at, first, depth, &keep).await))
        }
        None => {
            let links = html::links(&page, &at);
            Ok(Read::Page(at, links))
        }
    }
}

/// Where the next listings come from.
enum Lister<'a> {
    Http {
        headers: HeaderMap,
        opts: &'a Options,
    },
    Ftp(Box<crate::ftp::Lister>),
}

impl Lister<'_> {
    /// The subfolder `url`'s entries; None when it can't be listed, isn't a listing, or its
    /// listing came from another address.
    async fn list(&mut self, url: &Url) -> Option<Vec<Entry>> {
        match self {
            Lister::Ftp(l) => l.list(url).await.ok(),
            Lister::Http { headers, opts } => {
                let (at, page) = web_page(url.clone(), headers.clone(), opts, LISTING_MAX)
                    .await
                    .ok()?;
                if at != *url {
                    return None;
                }
                index_entries(&page?, &at)
            }
        }
    }
}

/// The files of the folder at `start` (its entries `first`), and of its subfolders down to
/// `depth`, breadth first: the folder's own files come first.
async fn walk(
    list: &mut Lister<'_>,
    start: Url,
    first: Vec<Entry>,
    depth: u8,
    keep: &impl Fn(&str) -> bool,
) -> Folder {
    let depth = usize::from(depth.min(MAX_DEPTH));
    let mut out = Folder {
        name: folder_name(&start),
        ..Folder::default()
    };
    let mut queue: VecDeque<(Url, Vec<String>)> = VecDeque::new();
    let mut listed = 1;
    let mut next = Some((start, Vec::new(), first));
    while let Some((at, folders, entries)) = next.take() {
        for e in entries {
            if e.dir {
                if folders.len() < depth {
                    let mut inside = folders.clone();
                    inside.push(e.name);
                    queue.push_back((e.url, inside));
                }
            } else if keep(&e.name) {
                if out.files.len() >= MAX_FILES {
                    out.capped = true;
                    return out;
                }
                out.files.push(FolderFile {
                    url: e.url,
                    listing: at.clone(),
                    folders: folders.clone(),
                    size: e.size,
                });
            }
        }
        while let Some((url, folders)) = queue.pop_front() {
            if listed >= MAX_FOLDERS {
                out.capped = true;
                return out;
            }
            listed += 1;
            if let Some(entries) = list.list(&url).await {
                next = Some((url, folders, entries));
                break;
            }
        }
    }
    out
}

/// A folder's own name: its address's last folder, decoded, else the server's name; a plain
/// name either way.
fn folder_name(url: &Url) -> String {
    let last = url
        .path_segments()
        .and_then(|mut s| s.rfind(|s| !s.is_empty()))
        .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
        .filter(|n| plain_name(n));
    last.or_else(|| url.host_str().map(str::to_owned))
        .filter(|n| plain_name(n))
        .unwrap_or_else(|| "folder".to_owned())
}

/// The entries of the listing page `page` that came from `at` (a folder's address, ending in
/// `/`): None when it isn't a web server's listing of a folder. Each link that names something
/// in the folder (`child`), once.
pub fn index_entries(page: &str, at: &Url) -> Option<Vec<Entry>> {
    if !at.path().ends_with('/') || !is_listing(page, at) {
        return None;
    }
    let mut seen = HashSet::new();
    Some(
        html::links(page, at)
            .iter()
            .filter_map(|u| child(at, u))
            .filter(|e| seen.insert(e.name.clone()))
            .collect(),
    )
}

/// Does `page` say it lists the folder `at`, as servers title their listings: "Index of /pub/"
/// (Apache, nginx, lighttpd, LiteSpeed, Caddy's "Index of"), "Directory listing for /pub/"
/// (Python), "host - /pub/" (IIS)?
fn is_listing(page: &str, at: &Url) -> bool {
    let path = percent_decode_str(at.path()).decode_utf8_lossy();
    ["title", "h1"].iter().any(|name| {
        let Some(text) = html::element_text(page, name) else {
            return false;
        };
        let lower = text.to_lowercase();
        lower.starts_with("index of ")
            || lower.starts_with("directory listing for ")
            || text
                .rsplit_once(" - ")
                .is_some_and(|(_, p)| p.trim() == path || p.trim() == path.trim_end_matches('/'))
    })
}

/// What the link `u` on the listing of folder `dir` (ending in `/`) names in it: a file
/// (`dir/name`) or a folder (`dir/name/`) on the same server, without a query, whose name is a
/// plain name once decoded. None for anything else: another site, the folder above or itself,
/// a deeper path, a sort order.
fn child(dir: &Url, u: &Url) -> Option<Entry> {
    let same = u.scheme() == dir.scheme()
        && u.host_str() == dir.host_str()
        && u.port_or_known_default() == dir.port_or_known_default()
        && u.username() == dir.username();
    if !same || u.query().is_some() {
        return None;
    }
    let rest = u.path().strip_prefix(dir.path())?;
    let (raw, is_dir) = match rest.strip_suffix('/') {
        Some(r) => (r, true),
        None => (rest, false),
    };
    // A name in another encoding than UTF-8 (an old server's) keeps its address; only its
    // shown name has the replacement character.
    let name = percent_decode_str(raw).decode_utf8_lossy().into_owned();
    plain_name(&name).then(|| Entry {
        name,
        dir: is_dir,
        size: None,
        url: u.clone(),
    })
}

/// The address of `name` in the folder `dir` (ending in `/`), a folder's ending in `/` too.
pub(crate) fn child_url(dir: &Url, name: &str, is_dir: bool) -> Option<Url> {
    let mut u = dir.clone();
    {
        let mut segments = u.path_segments_mut().ok()?;
        segments.pop_if_empty().push(name);
        if is_dir {
            segments.push("");
        }
    }
    Some(u)
}

/// One line of an FTP folder's list: `MLSD`'s (`type=file;size=12; name`) when `mlsd`, else
/// `LIST`'s (Unix `ls -l` or DOS lines), as an entry of the folder `dir`. None for anything else:
/// a symbolic link, the folder itself or its parent (`type=cdir`, `pdir`), a summary line
/// (`total 12`), a name that isn't plain.
pub(crate) fn ftp_entry(line: &str, mlsd: bool, dir: &Url) -> Option<Entry> {
    let (name, is_dir, size) = if mlsd {
        // RFC 3659 §7.2: facts, each ending in `;`, then a space and the name, which may hold
        // spaces and `;` itself.
        let (facts, name) = line.split_once("; ")?;
        let fact = |key: &str| {
            facts.split(';').find_map(|f| {
                let (k, v) = f.split_once('=')?;
                k.trim().eq_ignore_ascii_case(key).then(|| v.trim())
            })
        };
        let is_dir = match fact("type")?.to_ascii_lowercase().as_str() {
            "file" => false,
            "dir" => true,
            _ => return None,
        };
        let size = fact("size").and_then(|s| s.parse().ok());
        (name.to_owned(), is_dir, size)
    } else {
        use suppaftp::list::ListParser;
        let f = ListParser::parse_posix(line)
            .or_else(|_| ListParser::parse_dos(line))
            .ok()?;
        if f.is_symlink() {
            return None;
        }
        let size = (!f.is_directory()).then(|| f.size() as u64);
        (f.name().to_owned(), f.is_directory(), size)
    };
    if !plain_name(&name) {
        return None;
    }
    Some(Entry {
        url: child_url(dir, &name, is_dir)?,
        name,
        dir: is_dir,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| format!("{}{}", e.name, if e.dir { "/" } else { "" }))
            .collect()
    }

    #[test]
    fn plain_names_only() {
        for good in ["a.bin", "my file (1).iso", "ünï.txt", ".hidden", "a..b"] {
            assert!(plain_name(good), "{good}");
        }
        for bad in [
            "", ".", "..", "...", ".. ", " ", "a/b", "..\\x", "c:\\x", "/etc", "a\nb", "a\0b",
        ] {
            assert!(!plain_name(bad), "{bad:?}");
        }
    }

    /// Apache's listing (its "fancy" table): the sort links, the parent and the icons aren't
    /// entries; files and folders are.
    #[test]
    fn apache_lists_its_folder() {
        let page = r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2 Final//EN">
<html><head><title>Index of /pub/linux</title></head><body>
<h1>Index of /pub/linux</h1>
<table><tr><th valign="top"><img src="/icons/blank.gif" alt="[ICO]"></th><th><a href="?C=N;O=D">Name</a></th><th><a href="?C=M;O=A">Last modified</a></th><th><a href="?C=S;O=A">Size</a></th></tr>
<tr><td valign="top"><img src="/icons/back.gif" alt="[PARENTDIR]"></td><td><a href="/pub/">Parent Directory</a></td><td>&nbsp;</td><td align="right">  - </td></tr>
<tr><td valign="top"><img src="/icons/folder.gif" alt="[DIR]"></td><td><a href="iso/">iso/</a></td><td align="right">2026-11-20 10:00  </td><td align="right">  - </td></tr>
<tr><td valign="top"><img src="/icons/compressed.gif" alt="[   ]"></td><td><a href="kernel-6.1.tar.xz">kernel-6.1.tar.xz</a></td><td align="right">2026-11-20 10:00  </td><td align="right">130M</td></tr>
<tr><td valign="top"><img src="/icons/text.gif" alt="[TXT]"></td><td><a href="read%20me.txt">read me.txt</a></td><td align="right">2026-11-20 10:00  </td><td align="right">1.2K</td></tr>
</table><address>Apache/2.4.62 Server at mirror.example Port 80</address></body></html>"#;
        let dir = at("http://mirror.example/pub/linux/");
        let e = index_entries(page, &dir).expect("a listing");
        assert_eq!(names(&e), ["iso/", "kernel-6.1.tar.xz", "read me.txt"]);
        assert_eq!(e[0].url.as_str(), "http://mirror.example/pub/linux/iso/");
        assert_eq!(
            e[2].url.as_str(),
            "http://mirror.example/pub/linux/read%20me.txt"
        );
    }

    #[test]
    fn nginx_and_iis_and_python_list_theirs() {
        let nginx = "<html>\r\n<head><title>Index of /files/</title></head>\r\n<body>\r\n<h1>Index of /files/</h1><hr><pre><a href=\"../\">../</a>\r\n<a href=\"docs/\">docs/</a>                                              20-Nov-2026 10:00       -\r\n<a href=\"a.bin\">a.bin</a>                                              20-Nov-2026 10:00   65536\r\n</pre><hr></body>\r\n</html>\r\n";
        let e = index_entries(nginx, &at("http://h:8080/files/")).unwrap();
        assert_eq!(names(&e), ["docs/", "a.bin"]);
        let iis = r#"<html><head><title>localhost - /files/</title></head><body><H1>localhost - /files/</H1><hr>
<pre><A HREF="/">[To Parent Directory]</A><br><br> 11/20/2026 10:00 AM        &lt;dir&gt; <A HREF="/files/docs/">docs</A><br> 11/20/2026 10:00 AM        65536 <A HREF="/files/a.bin">a.bin</A><br></pre><hr></body></html>"#;
        let e = index_entries(iis, &at("http://localhost/files/")).unwrap();
        assert_eq!(names(&e), ["docs/", "a.bin"]);
        let python = r#"<!DOCTYPE HTML><html lang="en"><head><meta charset="utf-8"><title>Directory listing for /files/</title></head><body><h1>Directory listing for /files/</h1><hr><ul><li><a href="docs/">docs/</a></li><li><a href="a.bin">a.bin</a></li></ul><hr></body></html>"#;
        let e = index_entries(python, &at("http://127.0.0.1:8000/files/")).unwrap();
        assert_eq!(names(&e), ["docs/", "a.bin"]);
    }

    /// The hostile listing: what leaves the folder, goes to another site or isn't a plain name
    /// is no entry, whatever the page says.
    #[test]
    fn a_hostile_listing_stays_in_its_folder() {
        let page = r#"<title>Index of /files/</title><base href="http://h/files/">
<a href="../">up</a> <a href="../../etc/passwd">passwd</a> <a href="/">root</a>
<a href="/other/x.bin">elsewhere</a> <a href="http://evil.example/files/x.bin">evil</a>
<a href="//evil.example/y.bin">evil too</a> <a href="https://h/files/z.bin">other scheme</a>
<a href="http://h:81/files/z.bin">other port</a> <a href="http://user@h/files/u.bin">login</a>
<a href="sub/../../escape.bin">escape</a> <a href="%2e%2e/dots.bin">dots</a>
<a href="a%2fb.bin">slash</a> <a href="a%5cb.bin">backslash</a> <a href="%2e%2e">dotdot</a>
<a href="deep/er/x.bin">deeper</a> <a href="./">itself</a> <a href="?C=S;O=A">sort</a>
<a href="ok.bin?x=1">query</a> <a href="bad%0d%0a.bin">newline</a> <a href="nul%00.bin">nul</a>
<a href="ok.bin">ok</a> <a href="ok.bin#frag">ok again</a> <a href="sub/">sub</a>"#;
        let e = index_entries(page, &at("http://h/files/")).unwrap();
        assert_eq!(names(&e), ["ok.bin", "sub/"]);
    }

    /// A page that isn't a listing (a site's own page with "Index" in its text) is read for its
    /// links as before, and a listing's address without its `/` lists nothing it could misplace.
    #[test]
    fn other_pages_are_no_listings() {
        let page = "<title>Downloads</title><h2>Index of our files</h2><a href=\"a.bin\">a</a>";
        assert_eq!(index_entries(page, &at("http://h/files/")), None);
        let listing = "<title>Index of /files</title><a href=\"a.bin\">a</a>";
        assert_eq!(index_entries(listing, &at("http://h/files")), None);
    }

    #[test]
    fn ftp_lines() {
        let dir = at("ftp://h/pub/");
        let mlsd = |l: &str| ftp_entry(l, true, &dir).map(|e| (e.name, e.dir, e.size));
        assert_eq!(
            mlsd("type=file;size=1234;modify=20261120100000; a file.bin"),
            Some(("a file.bin".into(), false, Some(1234)))
        );
        assert_eq!(
            mlsd("Type=dir;Modify=20261120100000; docs"),
            Some(("docs".into(), true, None))
        );
        assert_eq!(
            mlsd("type=file;size=x; semi;colon.txt"),
            Some(("semi;colon.txt".into(), false, None))
        );
        for bad in [
            "type=cdir; .",
            "type=pdir; ..",
            "type=OS.unix=slink:/etc; link",
            "type=file;size=1; ../up.bin",
            "type=file;size=1; /etc/passwd",
            "type=file;size=1; a/b.bin",
            "type=dir; ..",
            "no facts at all",
        ] {
            assert_eq!(mlsd(bad), None, "{bad}");
        }
        let list = |l: &str| ftp_entry(l, false, &dir).map(|e| (e.name, e.dir, e.size));
        assert_eq!(
            list("-rw-r--r--    1 ftp      ftp         65536 Nov 20 10:00 a.bin"),
            Some(("a.bin".into(), false, Some(65536)))
        );
        assert_eq!(
            list("drwxr-xr-x    2 ftp      ftp          4096 Nov 20 10:00 docs"),
            Some(("docs".into(), true, None))
        );
        assert_eq!(
            list("lrwxrwxrwx    1 ftp      ftp            11 Nov 20 10:00 up -> ../../etc"),
            None
        );
        assert_eq!(list("total 12"), None);
        assert_eq!(
            list("11-20-26  10:00AM       <DIR>          docs"),
            Some(("docs".into(), true, None))
        );
        assert_eq!(
            list("11-20-26  10:00AM                65536 a.bin"),
            Some(("a.bin".into(), false, Some(65536)))
        );
        let e = ftp_entry("type=dir; my docs", true, &dir).unwrap();
        assert_eq!(e.url.as_str(), "ftp://h/pub/my%20docs/");
        let e = ftp_entry("type=file; 100% #1?.bin", true, &dir).unwrap();
        assert_eq!(e.url.as_str(), "ftp://h/pub/100%25%20%231%3F.bin");
    }

    #[test]
    fn a_folders_own_name() {
        assert_eq!(folder_name(&at("http://h/pub/linux%20iso/")), "linux iso");
        assert_eq!(
            folder_name(&at("ftp://ftp.example.org/")),
            "ftp.example.org"
        );
        assert_eq!(folder_name(&at("http://h/%2e%2e/")), "h");
    }
}

//! Folders for Link grabber's folder tests (#1774): made-up trees of folders and generated
//! files, listed as web servers list a folder (`/idx/{style}/{tree}/…`, style `apache`, `nginx`
//! or `iis`) and over FTP (`/{tree}/…`, `MLSD` and `LIST`), each file the generator's bytes.
//!
//! - `tree`: a few files and folders four deep, a name with a space in it.
//! - `hostile`: a listing that tries to lead out of its folder: `../`, absolute paths, other
//!   sites, names with `/` or `\`, `..` as a folder, a symbolic link; only `ok.bin` and
//!   `sub/inner.bin` are really in it.
//! - `huge`: one folder of `HUGE` files, more than a reader lists.

use std::fmt::Write as _;

/// The files in `huge`.
pub const HUGE: usize = 25_000;

/// `tree`'s folders (ending in `/`) and files, with their sizes.
const TREE: &[(&str, u64)] = &[
    ("a.bin", 64 << 10),
    ("notes.txt", 1 << 10),
    ("docs/", 0),
    ("docs/guide.pdf", 128 << 10),
    ("docs/deep/", 0),
    ("docs/deep/notes.txt", 2 << 10),
    ("docs/deep/deeper/", 0),
    ("docs/deep/deeper/last.bin", 4 << 10),
    ("my files/", 0),
    ("my files/photo 1.jpg", 32 << 10),
];

/// `hostile`'s own folders and files, those really in it.
const HOSTILE: &[(&str, u64)] = &[("ok.bin", 1 << 10), ("sub/", 0), ("sub/inner.bin", 2 << 10)];

/// One thing a folder holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

/// What a path of a tree is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// A folder and what it holds (`hostile` adds its tricks when listed).
    Dir(Vec<Item>),
    /// A file: the generator's seed and its size.
    File { seed: u64, size: u64 },
}

fn listed(tree: &str) -> Option<&'static [(&'static str, u64)]> {
    match tree {
        "tree" => Some(TREE),
        "hostile" => Some(HOSTILE),
        _ => None,
    }
}

/// The seed of a file's bytes: its path's hash (FNV-1a), the same on every request.
pub fn seed_of(path: &str) -> u64 {
    path.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// The name of `huge`'s file `n` (from 1).
pub fn huge_name(n: usize) -> String {
    format!("f{n:05}.bin")
}

/// What `path` (decoded, `/`-separated, a folder's ending in `/`, "" the tree itself) is in the
/// tree `tree`.
pub fn node(tree: &str, path: &str) -> Option<Node> {
    if tree == "huge" {
        if path.is_empty() {
            return Some(Node::Dir(
                (1..=HUGE)
                    .map(|n| Item {
                        name: huge_name(n),
                        dir: false,
                        size: 1 << 10,
                    })
                    .collect(),
            ));
        }
        let n: usize = path.strip_prefix('f')?.strip_suffix(".bin")?.parse().ok()?;
        return (1..=HUGE).contains(&n).then(|| Node::File {
            seed: seed_of(path),
            size: 1 << 10,
        });
    }
    let all = listed(tree)?;
    if path.is_empty() || path.ends_with('/') {
        if !path.is_empty() && !all.iter().any(|(p, _)| *p == path) {
            return None;
        }
        let items = all
            .iter()
            .filter_map(|(p, size)| {
                let rest = p.strip_prefix(path)?;
                let (name, dir) = match rest.strip_suffix('/') {
                    Some(n) => (n, true),
                    None => (rest, false),
                };
                (!name.is_empty() && !name.contains('/')).then(|| Item {
                    name: name.to_owned(),
                    dir,
                    size: *size,
                })
            })
            .collect();
        return Some(Node::Dir(items));
    }
    all.iter()
        .find(|(p, _)| *p == path)
        .map(|(_, size)| Node::File {
            seed: seed_of(path),
            size: *size,
        })
}

/// Is `path` a folder of `tree` written without its `/` (to redirect, as servers do)?
pub fn is_dir_without_slash(tree: &str, path: &str) -> bool {
    !path.is_empty()
        && listed(tree).is_some_and(|all| all.iter().any(|(p, _)| *p == format!("{path}/")))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `name` as a link's path segment.
fn href(name: &str) -> String {
    percent_encoding::utf8_percent_encode(name, SEGMENT).to_string()
}

const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b'\\');

/// The links `hostile`'s listing adds, as `(href, text)`: every way out of the folder a page
/// can write.
fn hostile_links(style: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = [
        ("../", "Parent Directory"),
        ("../../escape.bin", "escape.bin"),
        ("/gen/1K/outside.bin", "outside.bin"),
        ("/idx/apache/tree/a.bin", "a.bin"),
        ("http://evil.invalid/x.bin", "x.bin"),
        ("//evil.invalid/y.bin", "y.bin"),
        ("sub/../../escape2.bin", "escape2.bin"),
        ("%2e%2e/dots.bin", "dots.bin"),
        ("%2e%2e/", "dots"),
        ("a%2fb.bin", "a/b.bin"),
        ("a%5cb.bin", "a\\b.bin"),
        ("sub/inner.bin", "inner.bin, from the folder above"),
        ("?C=N;O=D", "Name"),
        ("ok.bin?download=1", "ok.bin again"),
    ]
    .iter()
    .map(|(h, t)| ((*h).to_owned(), (*t).to_owned()))
    .collect();
    // IIS writes absolute paths: one outside the folder, as it would write it.
    if style == "iis" {
        out.push(("/".into(), "[To Parent Directory]".into()));
    }
    out
}

/// The listing page of the folder `path` of `tree` (its items `items`) as `style` writes one.
/// `base` is the folder's address path (`/idx/apache/tree/docs/`).
pub fn page(style: &str, tree: &str, base: &str, items: &[Item]) -> String {
    let title_path = base;
    let mut links: Vec<(String, String, Option<u64>)> = Vec::new();
    if style != "iis" {
        links.push(("../".into(), "../".into(), None));
    }
    for i in items {
        let h = if i.dir {
            format!("{}/", href(&i.name))
        } else {
            href(&i.name)
        };
        let h = if style == "iis" {
            format!("{base}{h}")
        } else {
            h
        };
        let text = if i.dir && style != "iis" {
            format!("{}/", i.name)
        } else {
            i.name.clone()
        };
        links.push((h, text, (!i.dir).then_some(i.size)));
    }
    if tree == "hostile" {
        links.extend(
            hostile_links(style)
                .into_iter()
                .map(|(h, t)| (h, t, Some(1024))),
        );
    }
    let mut s = String::new();
    match style {
        "nginx" => {
            let _ = write!(
                s,
                "<html>\r\n<head><title>Index of {0}</title></head>\r\n<body>\r\n<h1>Index of {0}</h1><hr><pre>",
                esc(title_path)
            );
            for (h, t, size) in &links {
                let size = size.map_or("-".to_owned(), |n| n.to_string());
                let _ = write!(
                    s,
                    "<a href=\"{}\">{}</a>{:>50} {size:>19}\r\n",
                    esc(h),
                    esc(t),
                    "20-Nov-2026 10:00"
                );
            }
            s.push_str("</pre><hr></body>\r\n</html>\r\n");
        }
        "iis" => {
            let _ = write!(
                s,
                "<html><head><title>localhost - {0}</title></head><body><H1>localhost - {0}</H1><hr>\r\n\r\n<pre>",
                esc(title_path)
            );
            if base.trim_end_matches('/').contains('/') {
                let up = base
                    .trim_end_matches('/')
                    .rsplit_once('/')
                    .map_or("/", |(u, _)| u);
                let _ = write!(
                    s,
                    "<A HREF=\"{}/\">[To Parent Directory]</A><br><br>",
                    esc(up)
                );
            }
            for (h, t, size) in &links {
                let size = size.map_or("&lt;dir&gt;".to_owned(), |n| n.to_string());
                let _ = write!(
                    s,
                    " 11/20/2026 10:00 AM {size:>12} <A HREF=\"{}\">{}</A><br>",
                    esc(h),
                    esc(t)
                );
            }
            s.push_str("</pre><hr></body></html>");
        }
        // Apache's "fancy" index, its sort links and icons too.
        _ => {
            let _ = write!(
                s,
                "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 3.2 Final//EN\">\n<html>\n <head>\n  <title>Index of {0}</title>\n </head>\n <body>\n<h1>Index of {0}</h1>\n  <table>\n   <tr><th valign=\"top\"><img src=\"/icons/blank.gif\" alt=\"[ICO]\"></th><th><a href=\"?C=N;O=D\">Name</a></th><th><a href=\"?C=M;O=A\">Last modified</a></th><th><a href=\"?C=S;O=A\">Size</a></th></tr>\n",
                esc(title_path)
            );
            for (h, t, size) in &links {
                let size = size.map_or("  - ".to_owned(), |n| format!("{}K", n.div_ceil(1024)));
                let _ = writeln!(
                    s,
                    "<tr><td valign=\"top\"><img src=\"/icons/unknown.gif\" alt=\"[   ]\"></td><td><a href=\"{}\">{}</a></td><td align=\"right\">2026-11-20 10:00  </td><td align=\"right\">{size}</td></tr>",
                    esc(h),
                    esc(t)
                );
            }
            s.push_str("</table>\n<address>Lab Server</address>\n</body></html>\n");
        }
    }
    s
}

/// The lines an FTP server lists the folder's `items` in: `MLSD`'s facts, or `LIST`'s Unix
/// lines; `hostile` adds its tricks.
pub fn ftp_lines(tree: &str, items: &[Item], mlsd: bool) -> String {
    let mut s = String::new();
    if mlsd {
        s.push_str("type=cdir;modify=20261120100000; .\r\ntype=pdir;modify=20261120100000; ..\r\n");
    } else {
        s.push_str("total 8\r\n");
    }
    let line = |s: &mut String, name: &str, dir: bool, size: u64| {
        if mlsd {
            if dir {
                let _ = write!(s, "type=dir;modify=20261120100000; {name}\r\n");
            } else {
                let _ = write!(s, "type=file;size={size};modify=20261120100000; {name}\r\n");
            }
        } else {
            let (kind, size) = if dir { ('d', 4096) } else { ('-', size) };
            let _ = write!(
                s,
                "{kind}rw-r--r--    1 ftp      ftp      {size:>10} Nov 20 10:00 {name}\r\n"
            );
        }
    };
    for i in items {
        line(&mut s, &i.name, i.dir, i.size);
    }
    if tree == "hostile" {
        for name in [
            "../escape.bin",
            "/etc/passwd",
            "a/b.bin",
            "..\\up.bin",
            "...",
        ] {
            line(&mut s, name, false, 1024);
        }
        for name in ["..", "../../up", "."] {
            line(&mut s, name, true, 0);
        }
        if mlsd {
            s.push_str("type=OS.unix=slink:/;modify=20261120100000; root\r\n");
        } else {
            s.push_str("lrwxrwxrwx    1 ftp      ftp               1 Nov 20 10:00 root -> /\r\n");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trees_hold_their_files() {
        let Some(Node::Dir(top)) = node("tree", "") else {
            panic!("a folder")
        };
        let names: Vec<_> = top.iter().map(|i| (i.name.as_str(), i.dir)).collect();
        assert_eq!(
            names,
            [
                ("a.bin", false),
                ("notes.txt", false),
                ("docs", true),
                ("my files", true)
            ]
        );
        assert!(matches!(
            node("tree", "docs/deep/deeper/last.bin"),
            Some(Node::File { size: 4096, .. })
        ));
        assert_eq!(node("tree", "nothing/"), None);
        assert_eq!(node("tree", "../a.bin"), None);
        assert!(is_dir_without_slash("tree", "docs"));
        assert!(matches!(
            node("huge", "f25000.bin"),
            Some(Node::File { .. })
        ));
        assert_eq!(node("huge", "f25001.bin"), None);
    }
}

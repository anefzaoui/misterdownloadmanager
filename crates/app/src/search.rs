//! Torrent search (beyond IDM): finding a magnet or a .torrent for something you want, from
//! sources the user names.
//!
//! MDM has no catalogue of its own and never will: an indexer is a service, not a download
//! manager. So a *provider* is a small JSON file — how to ask a site for a search and where in its
//! answer the name, size and torrent sit — and the app ships one that works out of the box
//! (Internet Archive's torrent collection) plus the format, so any other site can be added by
//! dropping a file in the data folder's `search` directory. Nothing is fetched until the user
//! searches, and every result is added by hand.
//!
//! ```json
//! {
//!   "name": "Internet Archive",
//!   "url": "https://archive.org/advancedsearch.php?q={query}&output=json",
//!   "results": "response.docs",
//!   "fields": {
//!     "name": "title",
//!     "size": "item_size",
//!     "date": "publicdate",
//!     "torrent": "https://archive.org/download/{identifier}/{identifier}_archive.torrent",
//!     "details": "https://archive.org/details/{identifier}"
//!   }
//! }
//! ```
//!
//! `url` gets `{query}`, URL-encoded. `results` and the `fields` are dotted paths into the answer
//! (`a.b.0.c` reads an array's first entry). `torrent`, `magnet` and `details` are templates over
//! the row's own keys: `{identifier}` is that row's `identifier`.
//!
//! An RSS/Atom answer can be searched too: `"results": "rss"` reads the feed's items through the
//! app's own feed parser, its `fields` then naming the item's `title`, `link`, `torrent`, `date`
//! and, when the feed gives one, `size`.

use std::path::PathBuf;

use serde::Deserialize;

/// How many results one provider may give (a search is a window, not an index).
pub const MAX_HITS: usize = 100;

/// How large an answer is read (a search's JSON is small; a runaway one isn't read whole).
pub const MAX_ANSWER: u64 = 8 * 1024 * 1024;

/// One source of torrents.
#[derive(Clone, Debug)]
pub struct Provider {
    /// Its file's name without `.json`, or the built-in's id.
    pub id: String,
    /// What the window calls it (the file's `name`, else its id).
    pub name: String,
    /// What its search asks.
    pub url: String,
    /// Where the results are in the answer: a dotted path, or `rss` for a feed.
    pub results: String,
    pub fields: Fields,
}

/// A provider's name and what it answers: the file's own keys.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    pub name: String,
    pub size: String,
    pub seeds: String,
    pub peers: String,
    pub date: String,
    pub magnet: String,
    pub torrent: String,
    pub details: String,
}

impl Default for Provider {
    fn default() -> Self {
        Provider {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            results: "results".into(),
            fields: Fields::default(),
        }
    }
}

/// A provider as its file writes it.
#[derive(Deserialize)]
struct File {
    name: Option<String>,
    url: String,
    #[serde(default)]
    results: Option<String>,
    #[serde(default)]
    fields: FileFields,
}

#[derive(Deserialize, Default)]
struct FileFields {
    name: Option<String>,
    size: Option<String>,
    seeds: Option<String>,
    peers: Option<String>,
    date: Option<String>,
    magnet: Option<String>,
    torrent: Option<String>,
    details: Option<String>,
}

/// One line of a search's answer, as the app shows and adds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hit {
    pub name: String,
    pub size: Option<u64>,
    pub seeds: Option<u32>,
    pub peers: Option<u32>,
    pub date: String,
    /// The magnet link, when the source gives one.
    pub magnet: Option<String>,
    /// A .torrent's address, when it gives that instead.
    pub torrent: Option<String>,
    /// The page it was found on, for the user to look at.
    pub details: Option<String>,
    /// The provider it came from (its name).
    pub source: String,
}

impl Hit {
    /// What adding it hands the app: the magnet link, else the .torrent's address.
    pub fn link(&self) -> Option<&str> {
        self.magnet.as_deref().or(self.torrent.as_deref())
    }
}

/// Where a provider file goes: the data folder's `search` directory.
pub fn dir() -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join("search"))
}

/// The providers: the built-in one, then the user's files (a file named as the built-in is
/// skipped).
pub fn providers() -> Vec<Provider> {
    let mut out = builtins();
    let mut files: Vec<(String, PathBuf)> = dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            Some((path.file_stem()?.to_str()?.to_owned(), path))
        })
        .collect();
    files.sort();
    for (id, path) in files {
        if out.iter().any(|p| p.id == id) {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| read(&id, &t))
        {
            Ok(p) => out.push(p),
            Err(e) => tracing::warn!("search: {}: {e}", path.display()),
        }
    }
    out
}

/// A provider file's text read into a provider (the built-in ones call it with their own JSON).
fn read(id: &str, text: &str) -> Result<Provider, String> {
    let f: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if f.url.trim().is_empty() {
        return Err("no url".into());
    }
    let mut p = Provider {
        id: id.to_owned(),
        name: f.name.unwrap_or_else(|| id.to_owned()),
        url: f.url,
        results: f.results.unwrap_or_else(|| "results".into()),
        fields: Fields {
            name: f.fields.name.unwrap_or_default(),
            size: f.fields.size.unwrap_or_default(),
            seeds: f.fields.seeds.unwrap_or_default(),
            peers: f.fields.peers.unwrap_or_default(),
            date: f.fields.date.unwrap_or_default(),
            magnet: f.fields.magnet.unwrap_or_default(),
            torrent: f.fields.torrent.unwrap_or_default(),
            details: f.fields.details.unwrap_or_default(),
        },
    };
    if p.fields.name.is_empty() {
        p.fields.name = "name".into();
    }
    Ok(p)
}

/// The provider that works without a file of the user's: Internet Archive's torrents (its items
/// carry a `.torrent` beside the files, which needs no tracker of ours).
pub fn builtins() -> Vec<Provider> {
    // Names are proper nouns (not translated); the ids name them in the window's settings.
    let archive = |id: &str, name: &str, media: &str| {
        read(
            id,
            &format!(
                r#"{{
                  "name": "{name}",
                  "url": "https://archive.org/advancedsearch.php?q={{query}}+AND+mediatype%3A{media}&fl%5B%5D=identifier&fl%5B%5D=title&fl%5B%5D=item_size&fl%5B%5D=publicdate&rows=50&page=1&output=json",
                  "results": "response.docs",
                  "fields": {{
                    "name": "title",
                    "size": "item_size",
                    "date": "publicdate",
                    "torrent": "https://archive.org/download/{{identifier}}/{{identifier}}_archive.torrent",
                    "details": "https://archive.org/details/{{identifier}}"
                  }}
                }}"#
            ),
        )
        .expect("a built-in provider parses")
    };
    vec![
        archive("internet-archive", "Internet Archive — films", "movies"),
        archive(
            "internet-archive-audio",
            "Internet Archive — audio",
            "audio",
        ),
    ]
}

/// The address to ask: the provider's `url` with `{query}` filled in.
pub fn query_url(provider: &Provider, query: &str) -> String {
    let q = percent_encoding::utf8_percent_encode(query.trim(), percent_encoding::NON_ALPHANUMERIC)
        .to_string();
    provider.url.replace("{query}", &q)
}

/// A search's answer read into hits, in the order the source gave them.
pub fn parse(provider: &Provider, body: &str) -> Result<Vec<Hit>, String> {
    if provider.results.eq_ignore_ascii_case("rss") {
        return parse_feed(provider, body);
    }
    // A Torznab indexer (Jackett, Prowlarr, every indexer they hold): the `/api?t=search` answer,
    // which is RSS with the details search needs in `torznab:attr` elements.
    if provider.results.eq_ignore_ascii_case("torznab") {
        return parse_torznab(provider, body);
    }
    let answer: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
    let rows = path(&answer, &provider.results).ok_or_else(|| {
        // i18n: torrent search: the source's answer has no list where the provider says
        format!(
            "{}: {}",
            crate::i18n::t("the answer has no results where the provider says"),
            provider.results
        )
    })?;
    let rows = rows.as_array().ok_or("results is not a list")?;
    let mut out = Vec::new();
    for row in rows.iter().take(MAX_HITS) {
        let text = |key: &str| -> Option<String> {
            if key.is_empty() {
                return None;
            }
            path(row, key).and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
        };
        let name = text(&provider.fields.name)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if name.is_empty() {
            continue;
        }
        out.push(Hit {
            name,
            size: text(&provider.fields.size).and_then(|s| bytes(&s)),
            seeds: text(&provider.fields.seeds).and_then(|s| s.trim().parse().ok()),
            peers: text(&provider.fields.peers).and_then(|s| s.trim().parse().ok()),
            date: text(&provider.fields.date)
                .map(|d| d.chars().take(10).collect())
                .unwrap_or_default(),
            magnet: template(&provider.fields.magnet, row),
            torrent: template(&provider.fields.torrent, row),
            details: template(&provider.fields.details, row),
            source: provider.name.clone(),
        });
    }
    Ok(out)
}

/// A Torznab answer (Jackett, Prowlarr): one `<item>` per result, its name in `<title>`, its
/// torrent in `<link>`/`<enclosure>`, and size, seeders, peers and a magnet in `torznab:attr`.
fn parse_torznab(provider: &Provider, body: &str) -> Result<Vec<Hit>, String> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: 200_000,
    };
    let doc = roxmltree::Document::parse_with_options(body, opts).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let child = |name: &str| -> Option<String> {
            item.children()
                .find(|c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
                .and_then(|c| c.text())
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
        };
        let attr = |name: &str| -> Option<String> {
            item.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "attr")
                .find(|c| c.attribute("name") == Some(name))
                .and_then(|c| c.attribute("value"))
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let Some(name) = child("title") else { continue };
        let link = child("link")
            .or_else(|| {
                item.children()
                    .find(|c| c.has_tag_name("enclosure"))
                    .and_then(|c| c.attribute("url"))
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let magnet =
            attr("magneturl").or_else(|| link.starts_with("magnet:").then(|| link.clone()));
        let torrent = (!link.is_empty() && !link.starts_with("magnet:"))
            .then(|| link.clone())
            .or_else(|| {
                item.children()
                    .find(|c| c.has_tag_name("enclosure"))
                    .and_then(|c| c.attribute("url"))
                    .map(str::to_owned)
                    .filter(|u| !u.starts_with("magnet:"))
            });
        if magnet.is_none() && torrent.is_none() {
            continue;
        }
        out.push(Hit {
            name,
            size: attr("size")
                .and_then(|v| v.parse().ok())
                .or_else(|| child("size").and_then(|v| bytes(&v))),
            seeds: attr("seeders").and_then(|v| v.parse().ok()),
            peers: attr("peers").and_then(|v| v.parse().ok()),
            date: child("pubDate")
                .map(|d| d.chars().take(16).collect())
                .unwrap_or_default(),
            magnet,
            torrent,
            details: child("comments").or_else(|| child("guid")),
            source: provider.name.clone(),
        });
        if out.len() >= MAX_HITS {
            break;
        }
    }
    Ok(out)
}

/// A feed's items read as hits: the item's title, its torrent (a magnet or a `.torrent`'s
/// address), its size and date.
fn parse_feed(provider: &Provider, body: &str) -> Result<Vec<Hit>, String> {
    let parsed = crate::rss::parse(body)?;
    Ok(parsed
        .articles
        .into_iter()
        .filter(|a| !a.torrent.is_empty())
        .map(|a| Hit {
            name: a.title.clone(),
            size: a.size,
            date: a.date.chars().take(10).collect(),
            magnet: a.torrent.starts_with("magnet:").then(|| a.torrent.clone()),
            torrent: (!a.torrent.starts_with("magnet:")).then(|| a.torrent.clone()),
            details: (!a.page.is_empty()).then_some(a.page),
            source: provider.name.clone(),
            ..Hit::default()
        })
        .take(MAX_HITS)
        .collect())
}

/// `a.b.0.c` into a value: an object's key, an array's index.
fn path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut at = value;
    for part in path.split('.') {
        at = match at {
            serde_json::Value::Object(map) => map.get(part)?,
            serde_json::Value::Array(list) => list.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(at)
}

/// A template over a row's own keys: `…/{identifier}/…` with that row's `identifier`. Empty when
/// a key is missing, so a row with no torrent is a hit that can't be added rather than a wrong one.
fn template(template: &str, row: &serde_json::Value) -> Option<String> {
    let template = template.trim();
    if template.is_empty() {
        return None;
    }
    // No braces: it is the row's own key, and its value is taken as it is (a magnet link is not
    // a URL to be encoded part by part).
    if !template.contains('{') {
        return path(row, template)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty());
    }
    // A template into a web address encodes what it fills in; into a magnet, it does not.
    let encode = template.starts_with("http://") || template.starts_with("https://");
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}').map(|e| start + e) else {
            break;
        };
        out.push_str(&rest[..start]);
        let key = &rest[start + 1..end];
        let value = path(row, key).and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        });
        match value {
            Some(v) if !v.is_empty() => {
                if encode {
                    out.push_str(
                        &percent_encoding::utf8_percent_encode(
                            &v,
                            percent_encoding::NON_ALPHANUMERIC,
                        )
                        .to_string(),
                    );
                } else {
                    out.push_str(&v);
                }
            }
            _ => return None,
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// A size as a number of bytes: `12345`, `1.2 GB`, `340 MiB`.
fn bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Ok(n) = text.parse::<u64>() {
        return Some(n);
    }
    let mut parts = text.split_whitespace();
    let number: f64 = parts.next()?.replace(',', ".").parse().ok()?;
    let unit = parts.next().unwrap_or("B").to_ascii_lowercase();
    let mul = match unit.trim_end_matches('b').trim_end_matches('i') {
        "k" | "kib" => 1024.0,
        "m" | "mib" => 1024.0 * 1024.0,
        "g" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    Some((number * mul) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_reads_its_file() {
        let p = read(
            "example",
            r#"{"name":"Example","url":"https://x.test/s?q={query}","results":"data.hits",
                "fields":{"name":"title","size":"bytes","seeds":"seeders","magnet":"magnet"}}"#,
        )
        .unwrap();
        assert_eq!(p.id, "example");
        assert_eq!(p.results, "data.hits");
        assert_eq!(p.fields.name, "title");
    }

    #[test]
    fn the_query_is_encoded_into_the_address() {
        let p = Provider {
            url: "https://x.test/s?q={query}&n=20".into(),
            ..Provider::default()
        };
        let url = query_url(&p, "rick astley & friends");
        assert!(url.starts_with("https://x.test/s?q="));
        assert!(!url.contains(' '));
        assert!(url.ends_with("&n=20"));
    }

    #[test]
    fn a_json_answer_gives_hits_with_templated_links() {
        let p = read(
            "example",
            r#"{"url":"https://x.test/{query}","results":"data.hits",
                "fields":{"name":"title","size":"bytes","seeds":"seeders","date":"added",
                          "magnet":"magnet","torrent":"https://x.test/{id}.torrent",
                          "details":"https://x.test/item/{id}"}}"#,
        )
        .unwrap();
        let body = r#"{"data":{"hits":[
            {"title":"A film","bytes":"12345","seeders":12,"added":"2020-05-01T10:00:00Z","id":"abc","magnet":"magnet:?xt=urn:btih:abc"},
            {"title":"","bytes":"1","id":"skip"},
            {"title":"A disc","bytes":"1.2 GB","id":"def"}
        ]}}"#;
        let hits = parse(&p, body).unwrap();
        assert_eq!(hits.len(), 2, "the nameless row is skipped");
        assert_eq!(hits[0].name, "A film");
        assert_eq!(hits[0].size, Some(12345));
        assert_eq!(hits[0].seeds, Some(12));
        assert_eq!(hits[0].date, "2020-05-01");
        assert_eq!(hits[0].magnet.as_deref(), Some("magnet:?xt=urn:btih:abc"));
        assert_eq!(
            hits[0].torrent.as_deref(),
            Some("https://x.test/abc.torrent")
        );
        assert_eq!(hits[0].details.as_deref(), Some("https://x.test/item/abc"));
        assert_eq!(hits[0].link(), Some("magnet:?xt=urn:btih:abc"));
        // No magnet, a .torrent: that is what adding it hands the app.
        assert_eq!(hits[1].link(), Some("https://x.test/def.torrent"));
        assert!(hits[1].magnet.is_none());
    }

    #[test]
    fn a_row_without_its_key_gives_no_link_rather_than_a_wrong_one() {
        let p = read(
            "example",
            r#"{"url":"https://x.test/{query}","results":"hits",
                "fields":{"name":"title","torrent":"https://x.test/{missing}.torrent"}}"#,
        )
        .unwrap();
        let hits = parse(&p, r#"{"hits":[{"title":"No id here"}]}"#).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].torrent.is_none());
        assert!(hits[0].link().is_none());
    }

    #[test]
    fn sizes_read_as_bytes_or_as_text() {
        assert_eq!(bytes("12345"), Some(12345));
        assert_eq!(bytes("1.5 KB"), Some(1536));
        assert_eq!(bytes("2 GiB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(bytes("340 MiB"), Some(340 * 1024 * 1024));
        assert_eq!(bytes("nonsense"), None);
    }

    #[test]
    fn a_torznab_answer_gives_hits_with_magnets_and_seeders() {
        let p = read(
            "jackett",
            r#"{"name":"Jackett","url":"http://127.0.0.1:9117/api/v2.0/indexers/all/results/torznab/api?apikey=KEY&t=search&q={query}","results":"torznab"}"#,
        )
        .unwrap();
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
            <rss xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
              <item><title>Some.Thing.1080p</title><link>magnet:?xt=urn:btih:abc</link>
                <torznab:attr name="size" value="1234567890"/>
                <torznab:attr name="seeders" value="42"/>
                <torznab:attr name="peers" value="7"/>
                <pubDate>Mon, 09 Nov 2026 10:00:00 +0000</pubDate></item>
              <item><title>No link at all</title></item>
            </channel></rss>"#;
        let hits = parse(&p, body).unwrap();
        assert_eq!(hits.len(), 1, "the item with no torrent is skipped");
        assert_eq!(hits[0].name, "Some.Thing.1080p");
        assert_eq!(hits[0].size, Some(1234567890));
        assert_eq!(hits[0].seeds, Some(42));
        assert_eq!(hits[0].peers, Some(7));
        assert_eq!(hits[0].link(), Some("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn the_built_in_provider_is_a_working_file() {
        let all = builtins();
        assert_eq!(all.len(), 2, "films and audio");
        assert!(all.iter().all(|p| p.name.contains("Internet Archive")));
        assert!(query_url(&all[0], "nights").contains("archive.org"));
    }
}

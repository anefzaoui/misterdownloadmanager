//! Whole-file checksums (beyond IDM, D-015): verify a finished download when its hash is known —
//! given by the user, published by the file's site, or announced by the server (`Digest`,
//! `Repr-Digest`, `Content-MD5`, `x-goog-hash`). The overlap check catches a *changed* file; this
//! catches a *corrupt* one.
//!
//! Where a hash came from decides what a file that doesn't match it is ([`Origin`], #2064): the
//! user's or a published one must match, or the download fails; the server's own announcement
//! is only a warning, the file kept. Servers announce hashes of what they store, which is not
//! always what they send (an object stored encoded and decoded on the way out).

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Algo {
    Crc32,
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Crc32 => "CRC32",
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha256 => "SHA-256",
            Algo::Sha512 => "SHA-512",
        }
    }
}

/// Where an expected hash came from (#2064): it decides what a finished file that doesn't match
/// it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Typed or pasted by the user (Verify checksum..., the command line's `--checksum`, an
    /// add's checksum): a file that doesn't match fails. A hash saved before origins were kept
    /// is one of these, except in a resume state ([`state_hash`]).
    #[default]
    User,
    /// Published by the file's site and fetched by MDM itself (Hugging Face's or GitHub's digest,
    /// a helper's checksum list): as binding as the user's.
    Published,
    /// Announced by the server in the download's own answer (`Content-MD5`, `Digest`,
    /// `Repr-Digest`, `x-goog-hash`): a file that doesn't match is kept, with a warning. It can
    /// describe the object as the server stores it rather than the bytes it sent.
    Server,
}

/// An expected hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hash {
    pub algo: Algo,
    #[serde(with = "hex_bytes")]
    pub value: Vec<u8>,
    /// Who said the file has it.
    #[serde(default)]
    pub origin: Origin,
}

impl Hash {
    /// A hash typed or pasted by the user: hex, optionally `algo:` / `algo=` first
    /// (`sha256:…`, `md5=…`); without a name the length decides (8 CRC32, 32 MD5, 40 SHA-1,
    /// 64 SHA-256, 128 SHA-512). Spaces and a trailing file name (`… *file.iso`) are ignored.
    pub fn parse(text: &str) -> Option<Hash> {
        let text = text.trim();
        let (name, rest) = match text.split_once([':', '=']) {
            Some((n, r)) if n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                (Some(n.to_ascii_lowercase().replace('-', "")), r)
            }
            _ => (None, text),
        };
        let digits: String = rest
            .split_whitespace()
            .next()?
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let value = hex::decode(&digits).ok()?;
        let algo = match name.as_deref() {
            Some("crc32") => Algo::Crc32,
            Some("md5") => Algo::Md5,
            Some("sha1") => Algo::Sha1,
            Some("sha256") => Algo::Sha256,
            Some("sha512") => Algo::Sha512,
            Some(_) => return None,
            None => match value.len() {
                4 => Algo::Crc32,
                16 => Algo::Md5,
                20 => Algo::Sha1,
                32 => Algo::Sha256,
                64 => Algo::Sha512,
                _ => return None,
            },
        };
        (value.len() == digest_len(algo)).then_some(Hash {
            algo,
            value,
            origin: Origin::User,
        })
    }

    /// The same hash, published by the file's site (a hub's digest, a release's checksum list).
    pub fn published(self) -> Hash {
        Hash {
            origin: Origin::Published,
            ..self
        }
    }

    /// A file that doesn't match it fails (the user's, a published one); else it is kept with a
    /// warning (the server's).
    pub fn binding(&self) -> bool {
        self.origin != Origin::Server
    }

    /// The strongest hash a full (200) response announces for its content, if any: the server's
    /// ([`Origin::Server`]). `Content-MD5` and RFC 3230's `Digest` describe the body as sent, so
    /// they are left out when the answer says its body was transformed ([`transformed`]): a
    /// content coding, or an object stored encoded and decoded as it is served (Alibaba Cloud
    /// OSS's `x-oss-meta-encode`, #2064). RFC 9530's `Repr-Digest` describes the representation,
    /// and `x-goog-hash` the stored object, unless it was decompressed on the way out.
    pub fn from_headers(h: &HeaderMap) -> Option<Hash> {
        let b64 = |s: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(s.trim().trim_matches(':'))
                .ok()
        };
        let server = |algo, value| Hash {
            algo,
            value,
            origin: Origin::Server,
        };
        let as_sent = !transformed(h);
        let mut found: Vec<Hash> = Vec::new();
        // RFC 9530 `Repr-Digest: sha-256=:…:`, RFC 3230 `Digest: SHA-256=…`.
        let names: &[&str] = if as_sent {
            &["repr-digest", "digest"]
        } else {
            &["repr-digest"]
        };
        for name in names.iter().copied() {
            for v in h.get_all(name).iter().filter_map(|v| v.to_str().ok()) {
                for part in v.split(',') {
                    let Some((algo, value)) = part.trim().split_once('=') else {
                        continue;
                    };
                    let algo = match algo.trim().to_ascii_lowercase().as_str() {
                        "sha-512" => Algo::Sha512,
                        "sha-256" => Algo::Sha256,
                        "sha" | "sha-1" => Algo::Sha1,
                        "md5" => Algo::Md5,
                        _ => continue,
                    };
                    if let Some(value) = b64(value).filter(|v| v.len() == digest_len(algo)) {
                        found.push(server(algo, value));
                    }
                }
            }
        }
        if let Some(value) = h
            .get("content-md5")
            .filter(|_| as_sent)
            .and_then(|v| v.to_str().ok())
            .and_then(b64)
            .filter(|v| v.len() == 16)
        {
            found.push(server(Algo::Md5, value));
        }
        // Google Cloud Storage: `x-goog-hash: crc32c=…, md5=…` (crc32c is not CRC32), the stored
        // object's: not the bytes sent when it is stored compressed and served decompressed.
        let coded = |v: &str| !v.eq_ignore_ascii_case("identity");
        let decompressed = header_says(h, "x-goog-stored-content-encoding", coded)
            && !header_says(h, "content-encoding", coded);
        for v in h
            .get_all("x-goog-hash")
            .iter()
            .filter(|_| !decompressed)
            .filter_map(|v| v.to_str().ok())
        {
            for part in v.split(',') {
                if let Some(value) = part.trim().strip_prefix("md5=").and_then(b64)
                    && value.len() == 16
                {
                    found.push(server(Algo::Md5, value));
                }
            }
        }
        found.into_iter().max_by_key(|h| strength(h.algo))
    }

    /// The hash as text (`sha256:…`).
    pub fn display(&self) -> String {
        format!(
            "{}:{}",
            self.algo.name().to_ascii_lowercase().replace('-', ""),
            hex::encode(&self.value)
        )
    }
}

/// Whether an answer says its body is not the object as the server stores and hashes it: a
/// content coding other than `identity` (a hash of the coded bytes, or of the bytes before a
/// proxy coded them), Alibaba Cloud OSS's `x-oss-meta-encode` (an object stored encoded and
/// decoded as it is served: PikPak's, #2064), or a cache's `Warning: 214` (Transformation
/// Applied). Then `Content-MD5` and `Digest` are no promise about the bytes that arrive.
pub fn transformed(h: &HeaderMap) -> bool {
    header_says(h, "content-encoding", |v| {
        v.split(',')
            .map(str::trim)
            .any(|c| !c.is_empty() && !c.eq_ignore_ascii_case("identity"))
    }) || header_says(h, "x-oss-meta-encode", |v| {
        !(v.is_empty() || v == "0" || v.eq_ignore_ascii_case("false"))
    }) || header_says(h, "warning", |v| v.starts_with("214"))
}

/// Whether any `name` header's value, trimmed, passes `test`.
fn header_says(h: &HeaderMap, name: &str, test: impl Fn(&str) -> bool) -> bool {
    h.get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| test(v.trim()))
}

fn digest_len(a: Algo) -> usize {
    match a {
        Algo::Crc32 => 4,
        Algo::Md5 => 16,
        Algo::Sha1 => 20,
        Algo::Sha256 => 32,
        Algo::Sha512 => 64,
    }
}

fn strength(a: Algo) -> u8 {
    match a {
        Algo::Crc32 => 0,
        Algo::Md5 => 1,
        Algo::Sha1 => 2,
        Algo::Sha256 => 3,
        Algo::Sha512 => 4,
    }
}

/// The file's hash with `algo` (reads the whole file; call off the async threads).
pub fn digest(path: &Path, algo: Algo) -> std::io::Result<Vec<u8>> {
    digest_counted(path, algo, &AtomicU64::new(0))
}

/// [`digest`], adding each part read to `read` as it goes (#736: how far a check has come).
pub fn digest_counted(path: &Path, algo: Algo, read: &AtomicU64) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    macro_rules! run {
        ($h:expr) => {{
            let mut h = $h;
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
                read.fetch_add(n as u64, Ordering::Relaxed);
            }
            h.finalize().to_vec()
        }};
    }
    Ok(match algo {
        Algo::Crc32 => {
            let mut h = crc32fast::Hasher::new();
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
                read.fetch_add(n as u64, Ordering::Relaxed);
            }
            h.finalize().to_be_bytes().to_vec()
        }
        Algo::Md5 => run!(md5::Md5::new()),
        Algo::Sha1 => run!(sha1::Sha1::new()),
        Algo::Sha256 => run!(sha2::Sha256::new()),
        Algo::Sha512 => run!(sha2::Sha512::new()),
    })
}

/// Whether the file matches `hash`.
pub fn matches(path: &Path, hash: &Hash) -> std::io::Result<bool> {
    matches_counted(path, hash, &AtomicU64::new(0))
}

/// [`matches`], counting the bytes checked into `read` as it goes.
pub fn matches_counted(path: &Path, hash: &Hash, read: &AtomicU64) -> std::io::Result<bool> {
    Ok(digest_counted(path, hash.algo, read)? == hash.value)
}

/// A resume state's hash (`ResumeState::checksum`). One saved before hashes said where they
/// came from is taken as the server's: the app and the command line give the user's again as
/// they resume (`Options::checksum`, which comes first), so a saved hash on its own was the
/// server's, and a download that failed its server's check before #2064 now finishes.
pub(crate) mod state_hash {
    use super::{Algo, Hash, Origin};
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    struct Saved {
        algo: Algo,
        #[serde(with = "super::hex_bytes")]
        value: Vec<u8>,
        #[serde(default)]
        origin: Option<Origin>,
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Hash>, D::Error> {
        Ok(Option::<Saved>::deserialize(d)?.map(|s| Hash {
            algo: s.algo,
            value: s.value,
            origin: s.origin.unwrap_or(Origin::Server),
        }))
    }
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_hashes_by_length_or_name() {
        let md5 = Hash::parse("9e107d9d372bb6826bd81d3542a419d6").unwrap();
        assert_eq!(md5.algo, Algo::Md5);
        let sha = Hash::parse(
            "SHA256: d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592  *x.iso",
        )
        .unwrap();
        assert_eq!(sha.algo, Algo::Sha256);
        assert!(Hash::parse("md5:abc").is_none());
        assert!(Hash::parse("not a hash").is_none());
    }

    #[test]
    fn server_announced_hashes() {
        let mut h = HeaderMap::new();
        h.insert("content-md5", "nhB9nTcrtoJr2B01QqQZ1g==".parse().unwrap());
        h.insert(
            "repr-digest",
            "sha-256=:16j7swfXgJRpypq8sAguT41WUeRtPNt2LQLQvzfJ5ZI=:"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            Hash::from_headers(&h).unwrap().algo,
            Algo::Sha256,
            "the strongest"
        );
        h.remove("repr-digest");
        assert_eq!(Hash::from_headers(&h).unwrap().algo, Algo::Md5);
    }

    /// #2064: the user's and a site's published hash bind; the server's own doesn't.
    #[test]
    fn where_a_hash_came_from() {
        let typed = Hash::parse("9e107d9d372bb6826bd81d3542a419d6").unwrap();
        assert_eq!(typed.origin, Origin::User);
        assert!(typed.binding());
        let published = typed.clone().published();
        assert_eq!(published.origin, Origin::Published);
        assert!(published.binding());
        assert_eq!(published.value, typed.value);
        let mut h = HeaderMap::new();
        h.insert("content-md5", "nhB9nTcrtoJr2B01QqQZ1g==".parse().unwrap());
        let announced = Hash::from_headers(&h).unwrap();
        assert_eq!(announced.origin, Origin::Server);
        assert!(!announced.binding());
        assert_eq!(announced.value, typed.value);
        // Saved with the list: its origin goes with it, and a hash saved before origins were
        // kept is the user's (the list only ever held the user's or a published one).
        let json = serde_json::to_string(&published).unwrap();
        assert!(json.contains("\"origin\":\"published\""), "{json}");
        assert_eq!(serde_json::from_str::<Hash>(&json).unwrap(), published);
        let old = r#"{"algo":"Md5","value":"9e107d9d372bb6826bd81d3542a419d6"}"#;
        assert_eq!(serde_json::from_str::<Hash>(old).unwrap(), typed);
    }

    /// A resume state's hash saved before origins were kept is taken as the server's: the
    /// user's is given again as the download resumes.
    #[test]
    fn a_resume_states_old_hash_is_the_servers() {
        #[derive(serde::Deserialize)]
        struct State {
            #[serde(default, deserialize_with = "state_hash::deserialize")]
            checksum: Option<Hash>,
        }
        let read = |json: &str| serde_json::from_str::<State>(json).unwrap().checksum;
        let old = read(r#"{"checksum":{"algo":"Md5","value":"9e107d9d372bb6826bd81d3542a419d6"}}"#);
        assert_eq!(old.unwrap().origin, Origin::Server);
        let user = read(
            r#"{"checksum":{"algo":"Md5","value":"9e107d9d372bb6826bd81d3542a419d6","origin":"user"}}"#,
        );
        assert_eq!(user.unwrap().origin, Origin::User);
        assert_eq!(read(r#"{"checksum":null}"#), None);
        assert_eq!(read("{}"), None);
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (name, value) in pairs {
            h.append(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        h
    }

    const MD5: (&str, &str) = ("Content-MD5", "nhB9nTcrtoJr2B01QqQZ1g==");
    const SHA256: &str = "16j7swfXgJRpypq8sAguT41WUeRtPNt2LQLQvzfJ5ZI=";

    /// #2064: PikPak's storage (Alibaba Cloud OSS) sends the MD5 of the object as stored,
    /// encoded, beside `X-Oss-Meta-Encode: 1`; the body is decoded. No check at all.
    #[test]
    fn an_object_stored_encoded_is_not_checked_by_its_content_md5() {
        let pikpak = headers(&[
            MD5,
            ("ETag", "\"9E107D9D372BB6826BD81D3542A419D6\""),
            ("X-Oss-Meta-Encode", "1"),
            ("x-oss-hash-crc64ecma", "1234567890"),
        ]);
        assert!(transformed(&pikpak));
        assert_eq!(Hash::from_headers(&pikpak), None);
        // Any case, any value but an explicit no.
        assert!(transformed(&headers(&[("x-OSS-meta-ENCODE", "True")])));
        for no in ["0", "false", "FALSE", ""] {
            assert!(
                !transformed(&headers(&[("x-oss-meta-encode", no)])),
                "{no:?}"
            );
        }
        // Without it, the same Content-MD5 is checked.
        assert_eq!(
            Hash::from_headers(&headers(&[MD5])).map(|h| h.algo),
            Some(Algo::Md5)
        );
    }

    /// A content coding: `Content-MD5` and RFC 3230's `Digest` are of the coded instance, not
    /// what is saved; RFC 9530's `Repr-Digest` describes the representation and stays.
    #[test]
    fn a_coded_body_is_not_checked_by_its_content_md5_or_digest() {
        let digest = format!("SHA-256={SHA256}");
        for coding in ["gzip", "br", "identity, gzip", "x-custom"] {
            let h = headers(&[MD5, ("Digest", &digest), ("Content-Encoding", coding)]);
            assert!(transformed(&h), "{coding}");
            assert_eq!(Hash::from_headers(&h), None, "{coding}");
        }
        let repr = format!("sha-256=:{SHA256}:");
        let h = headers(&[MD5, ("Content-Encoding", "gzip"), ("Repr-Digest", &repr)]);
        let kept = Hash::from_headers(&h).unwrap();
        assert_eq!((kept.algo, kept.origin), (Algo::Sha256, Origin::Server));
        // `identity` is no coding at all.
        let plain = headers(&[MD5, ("Digest", &digest), ("Content-Encoding", "identity")]);
        assert!(!transformed(&plain));
        assert_eq!(Hash::from_headers(&plain).unwrap().algo, Algo::Sha256);
        // A cache that says it changed the body (Warning: 214 Transformation Applied).
        let cached = headers(&[MD5, ("Warning", "214 proxy \"Transformation Applied\"")]);
        assert_eq!(Hash::from_headers(&cached), None);
        let stale = headers(&[MD5, ("Warning", "110 proxy \"Response is Stale\"")]);
        assert!(Hash::from_headers(&stale).is_some());
    }

    /// Google Cloud Storage's hashes are the stored object's: kept, unless it is stored
    /// compressed and served decompressed (decompressive transcoding).
    #[test]
    fn google_storage_hashes_unless_decompressed_on_the_way() {
        let goog = (
            "x-goog-hash",
            "crc32c=n03x6A==, md5=nhB9nTcrtoJr2B01QqQZ1g==",
        );
        assert_eq!(
            Hash::from_headers(&headers(&[goog])).map(|h| h.algo),
            Some(Algo::Md5)
        );
        let stored_gzip = ("x-goog-stored-content-encoding", "gzip");
        assert_eq!(Hash::from_headers(&headers(&[goog, stored_gzip])), None);
        // Sent as stored, still gzip: the hash is of those bytes.
        let sent_gzip = headers(&[goog, stored_gzip, ("Content-Encoding", "gzip")]);
        assert!(Hash::from_headers(&sent_gzip).is_some());
        let stored_plain = ("x-goog-stored-content-encoding", "identity");
        assert!(Hash::from_headers(&headers(&[goog, stored_plain])).is_some());
    }

    #[test]
    fn digests_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fox.txt");
        std::fs::write(&p, b"The quick brown fox jumps over the lazy dog").unwrap();
        let sha = Hash::parse("d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592")
            .unwrap();
        assert!(matches(&p, &sha).unwrap());
        let md5 = Hash::parse("9e107d9d372bb6826bd81d3542a419d6").unwrap();
        assert!(matches(&p, &md5).unwrap());
        let crc = Hash::parse("crc32:414fa339").unwrap();
        assert!(matches(&p, &crc).unwrap());
        let wrong = Hash::parse("00000000000000000000000000000000").unwrap();
        assert!(!matches(&p, &wrong).unwrap());
        // #736: every byte checked is counted.
        let read = AtomicU64::new(0);
        assert!(matches_counted(&p, &sha, &read).unwrap());
        assert_eq!(read.load(Ordering::Relaxed), 43);
    }
}

//! What a web page posts to Click'n'Load (`mdm_cnl`: any page can post to it): the key in its
//! `jk` script, its `crypted` links opened with that key, and the links in what they decrypt to.
//!
//! The input's first line is `jk`; the rest is `crypted` as posted, and also, encrypted here with
//! a key the way a link site encrypts its links, the links themselves, so that the fuzzer reaches
//! the link list's reader.
#![no_main]

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use base64::Engine;
use libfuzzer_sys::fuzz_target;

const KEY_HEX: &str = "31323334353637383930313233343536";
const KEY: [u8; 16] = *b"1234567890123456";

/// `plain` as a link site posts it: zero-padded, AES-128-CBC with the key as IV too, Base64.
fn crypted(plain: &[u8]) -> String {
    let cipher = aes::Aes128::new(&KEY.into());
    let mut padded = plain.to_vec();
    padded.resize(plain.len().div_ceil(16).max(1) * 16, 0);
    let mut prev = KEY;
    let mut out = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(16) {
        let mut block = aes::Block::default();
        for (b, (c, p)) in block.iter_mut().zip(chunk.iter().zip(prev)) {
            *b = c ^ p;
        }
        cipher.encrypt_block(&mut block);
        prev.copy_from_slice(&block);
        out.extend_from_slice(&block);
    }
    base64::engine::general_purpose::STANDARD.encode(out)
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (jk, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let _ = mdm_cnl::key_of(jk);
    let _ = mdm_cnl::decrypt(rest, jk);

    let jk = format!("function f(){{ return '{KEY_HEX}'; }}");
    let links = mdm_cnl::decrypt(&crypted(rest.as_bytes()), &jk).expect("our own post opens");
    let mut seen = std::collections::HashSet::new();
    for link in &links {
        // Web links only, each once, none to this computer or its network by name.
        let u = url::Url::parse(link).expect("a link that parses");
        assert!(matches!(u.scheme(), "http" | "https" | "ftp"), "{link}");
        assert!(u.host_str() != Some("localhost"), "{link}");
        assert!(seen.insert(link.clone()), "{link} twice");
    }
});

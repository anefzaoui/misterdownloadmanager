//! A .torrent file (from a page, a folder MDM watches, or the remote control) and a magnet link:
//! its file list and names checked (`mdm_torrent::files_from_bytes`), its magnet link, and its
//! trackers read, filtered and added to (`mdm_torrent::trackers`), which rewrite the file around
//! its info dictionary.
#![no_main]

use std::path::Component;

use libfuzzer_sys::fuzz_target;
use mdm_torrent::librqbit;
use mdm_torrent::trackers;

fn info_hash(bytes: &[u8]) -> Option<String> {
    librqbit::torrent_from_bytes(bytes)
        .ok()
        .map(|m| m.info_hash.as_string())
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Some(source) = mdm_torrent::Source::magnet(&text) {
        let mdm_torrent::Source::Magnet(link) = source else {
            unreachable!()
        };
        let kept = trackers::in_magnet(&link);
        for proxied in [false, true] {
            let filtered = trackers::filter_magnet(&link, proxied);
            // Filtering only takes trackers away.
            for t in trackers::in_magnet(&filtered) {
                assert!(kept.contains(&t), "{t} came from nowhere");
            }
            // What librqbit reads from it is only what MDM may contact.
            if let Ok(m) = librqbit::Magnet::parse(&filtered) {
                for t in m.trackers {
                    assert!(trackers::allowed(&t, proxied), "{t} was kept");
                }
            }
        }
    }

    if let Ok(files) = mdm_torrent::files_from_bytes(data) {
        // Every name a torrent may write was checked: each path is relative, of plain names
        // (padding files are never written).
        for f in files.iter().filter(|f| !f.padding) {
            assert!(f.path.components().count() > 0, "{:?}", f.path);
            for c in f.path.components() {
                let Component::Normal(part) = c else {
                    panic!("{:?} is not a plain relative path", f.path);
                };
                let part = part.to_string_lossy();
                assert!(!part.contains(['/', '\\', '\0']), "{:?}", f.path);
            }
        }
    }
    let _ = mdm_torrent::magnet_link(data);

    let hash = info_hash(data);
    let listed = trackers::listed(data);
    for proxied in [false, true] {
        let filtered = trackers::filter_torrent(data, proxied);
        // A torrent librqbit reads is one MDM can filter (one it can't is refused).
        if hash.is_some() {
            assert!(
                filtered.is_some(),
                "a torrent librqbit reads wasn't filtered"
            );
        }
        if let Some(out) = filtered {
            // The same torrent: its info dictionary is kept byte for byte.
            if hash.is_some() {
                assert_eq!(info_hash(&out), hash, "filtering changed the info hash");
            }
            for t in trackers::listed(&out) {
                assert!(listed.contains(&t), "{t} came from nowhere");
            }
            // What librqbit reads from it is only what MDM may contact.
            if let Ok(meta) = librqbit::torrent_from_bytes(&out) {
                for t in meta.iter_announce() {
                    let t = std::str::from_utf8(t.as_ref()).expect("kept trackers are text");
                    assert!(trackers::allowed(t, proxied), "{t} was kept");
                }
            }
        }
    }
    let more = [
        "udp://tracker.example.org:1337/announce".to_owned(),
        "https://t.example.net/announce".to_owned(),
    ];
    if let Some(out) = trackers::add(data, &more)
        && hash.is_some()
    {
        assert_eq!(
            info_hash(&out),
            hash,
            "adding trackers changed the info hash"
        );
    }

    // The public tracker list (#1776): never added to a private torrent (BEP 27), whatever
    // librqbit reads as private MDM reads so too, and the hash stays.
    let private = trackers::private(data);
    if let Ok(meta) = librqbit::torrent_from_bytes(data)
        && meta.info.data.private
    {
        assert!(private, "librqbit reads it as private, MDM didn't");
    }
    let list = mdm_torrent::public_trackers::parse(
        b"udp://tracker.example.org:1337/announce\nhttp://10.0.0.1/announce\n",
    );
    if let Some(out) = mdm_torrent::public_trackers::add_to(data, &list) {
        assert!(!private, "a public list was added to a private torrent");
        if hash.is_some() {
            assert_eq!(
                info_hash(&out),
                hash,
                "the public list changed the info hash"
            );
        }
        for t in trackers::listed(&out) {
            assert!(
                listed.contains(&t) || list.contains(&t),
                "{t} came from nowhere"
            );
        }
    }
});

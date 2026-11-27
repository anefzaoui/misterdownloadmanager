//! A ZIP's table of contents as a server sends it (#1775, `mdm_engine::remotezip`): the input
//! is the archive's last bytes, read as the end record, the ZIP64 record and the table they
//! point to, then each entry's local header. Nothing may panic or overflow, nothing past the
//! limits is taken, and every name offered is a plain relative path whose data lies before the
//! table and apart from every other entry's.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mdm_engine::remotezip::{
    self, LOCAL_FIXED, MAX_DIRECTORY, MAX_ENTRIES, Tail, directory, local_header, tail, zip64_end,
};

fuzz_target!(|data: &[u8]| {
    // The input is the whole archive: the reader sees its last bytes as a server sends them.
    let size = data.len() as u64;
    let from = size.saturating_sub(remotezip::TAIL);
    let Ok(t) = tail(&data[from as usize..], from, size) else {
        return;
    };
    let end = match t {
        Tail::End(end) => end,
        Tail::Zip64 { at, before } => {
            let Some(rec) = data.get(at as usize..) else {
                return;
            };
            match zip64_end(rec, before, at) {
                Ok(end) => end,
                Err(_) => return,
            }
        }
    };
    assert!(end.directory_size <= MAX_DIRECTORY && end.entries <= MAX_ENTRIES);
    let Some(dir) = end
        .directory
        .checked_add(end.directory_size)
        .and_then(|e| data.get(end.directory as usize..e as usize))
    else {
        return;
    };
    let Ok((entries, left_out)) = directory(dir, &end) else {
        return;
    };
    assert!(entries.len() + left_out <= end.entries as usize);
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for e in &entries {
        // A plain relative path: no root, no `..`, no drive or stream, no empty part.
        assert!(!e.name.starts_with('/') && !e.name.contains(':'), "{:?}", e.name);
        for part in e.parts() {
            assert!(part != ".." && part != "." && !part.contains('\\'), "{:?}", e.name);
            assert!(!part.chars().any(char::is_control), "{:?}", e.name);
        }
        assert!(e.offset < e.limit && e.limit <= end.directory);
        spans.push((e.offset, e.limit));
        let _ = e.modified();
        let _ = e.refused();
        if let Some(head) = data.get(e.offset as usize..) {
            if let Ok((name, extra)) = local_header(head, e) {
                let data_end = e.offset + (LOCAL_FIXED + name + extra) as u64 + e.compressed;
                assert!(data_end <= e.limit);
            }
        }
    }
    spans.sort_unstable();
    assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "entries overlap");
});

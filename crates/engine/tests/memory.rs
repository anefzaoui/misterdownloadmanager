//! What a download keeps in memory while the disk holds its writes is bounded by what is
//! allocated, not only by what the engine counts (`HOLD_MAX`, 256 MiB): every allocation of this
//! test's process is counted (its own binary, so no other test allocates meanwhile).

use mdm_engine::{Destination, Download, Options, Request};
use mdm_labserver::{Config, Faults, LabServer, content};

#[global_allocator]
static ALLOC: cap::Cap<std::alloc::System> = cap::Cap::new(std::alloc::System, usize::MAX);

/// The engine's `HOLD_MAX`.
const HOLD_MAX: usize = 256 << 20;

/// A flush that holds every write for 5 s while 8 connections receive as fast as the lab server
/// sends (no rate: its chunks come in all sizes, as from a fast real server): their memory fills
/// to `HOLD_MAX` and they wait for the disk. Before, the engine counted 256 MiB there and
/// allocated more: a batch grew past its 256 KiB when a chunk overran it (Vec doubling), and a
/// write joined its batches into one buffer, grown in steps up to 4 MiB. Measured here: 309 MiB
/// allocated before, 268 MiB after (0.5.2, one buffer per connection grown by doubling to its
/// share of the cap, did worse).
#[tokio::test]
async fn memory_held_for_the_disk_stays_within_its_cap() {
    const SIZE: u64 = 512 << 20;
    let s = LabServer::start(Config {
        faults: Faults::parse("").unwrap(),
        ..Config::default()
    })
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let slow = mdm_engine::slowdisk::SlowDisk::slow_flush(std::time::Duration::from_secs(5));
    let gate = slow.gate.clone();
    let before = ALLOC.allocated();
    let d = Download::start(
        Request {
            url: s.url("/gen/512M/a.bin?seed=946").parse().unwrap(),
            headers: Default::default(),
            dest: Destination::Dir(dir.path().to_owned()),
        },
        Options {
            retry_delay: std::time::Duration::from_millis(50),
            save_interval: std::time::Duration::from_millis(100),
            slow_disk: Some(slow),
            ..Options::default()
        },
    );
    // The most allocated while the slow flush held the writes (sampled), and overall.
    let mut flushed = false;
    let mut during = 0;
    loop {
        let v = d.snapshot();
        if gate.try_read().is_err() {
            flushed = true;
            during = during.max(ALLOC.allocated().saturating_sub(before));
        }
        use mdm_engine::download::State;
        if matches!(v.state, State::Done | State::Failed | State::Cancelled) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let out = d.wait().await.expect("download succeeds");
    let peak = ALLOC.max_allocated().saturating_sub(before);
    eprintln!(
        "allocated beyond the start: {} MiB at most, {} MiB while the flush held the writes",
        peak >> 20,
        during >> 20
    );
    assert!(flushed, "a slow flush held the writes");
    // The memory did fill up (else this proves nothing)...
    assert!(during >= HOLD_MAX * 3 / 4, "{} MiB held", during >> 20);
    // ...and no further than the cap, with room for the connections' own receive buffers, the
    // lab server's, and a chunk past the cap per connection.
    assert!(
        peak <= HOLD_MAX + (24 << 20),
        "{} MiB allocated at most, for {} MiB held",
        peak >> 20,
        HOLD_MAX >> 20
    );
    // The file is whole (read in pieces: this test counts memory).
    assert_eq!(out.size, SIZE);
    let mut f = std::fs::File::open(&out.path).unwrap();
    let (mut piece, mut want) = (vec![0; 4 << 20], vec![0; 4 << 20]);
    let mut at = 0;
    while at < SIZE {
        std::io::Read::read_exact(&mut f, &mut piece).unwrap();
        content::fill(946, at, &mut want);
        assert!(piece == want, "content mismatch at {at}");
        at += piece.len() as u64;
    }
}

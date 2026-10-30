//! Crash safety, end to end: kill the CLI process mid-download (no clean shutdown, like a
//! crash or power cut for the process), resume from the state file, verify every byte.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mdm_engine::state::{ResumeState, state_path};
use mdm_labserver::{Config, Faults, LabServer, content};

const CLI: &str = env!("CARGO_BIN_EXE_misterdownloadmanager-cli");

#[tokio::test(flavor = "multi_thread")]
async fn killed_download_resumes_correctly() {
    let s = LabServer::start(Config {
        faults: Faults::parse("rate=1M").unwrap(),
        ..Config::default()
    })
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("crash.bin");
    let size: u64 = 24 << 20;

    let mut child = Command::new(CLI)
        .args(["get", &s.url("/gen/24M?seed=11"), "-o"])
        .arg(&dest)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    // Wait until saved progress shows real data, then kill without warning.
    let state = state_path(&dest);
    let deadline = Instant::now() + Duration::from_secs(30);
    let saved_before = loop {
        assert!(Instant::now() < deadline, "no progress saved");
        if let Ok(st) = ResumeState::load(&state)
            && st.downloaded() > 4 << 20
        {
            break st.downloaded();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(!dest.exists(), "not complete yet");

    let status = Command::new(CLI)
        .arg("resume")
        .arg(&state)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());

    let data = std::fs::read(&dest).unwrap();
    let mut want = vec![0; size as usize];
    content::fill(11, 0, &mut want);
    assert!(data == want, "content mismatch after crash + resume");
    assert!(!state.exists());
    assert!(saved_before < size);
}

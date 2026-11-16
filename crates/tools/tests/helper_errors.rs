//! #816: a helper's download that fails says which server it couldn't reach and how
//! (`Error::network`), and every request of it, the release list's first, goes through the proxy
//! Options sets (here one that isn't there: nothing leaves this computer).

use mdm_engine::Proxy;
use mdm_engine::fault::Fault;
use tokio::net::TcpSocket;

/// Options whose proxy is a port on this computer that refuses every connection, and the socket
/// that keeps it so: bound, never listening, held until the test ends. A port bound and let go
/// at once was free for anything to take before the request came: another program's server, or
/// this test's own connection, whose source port the system may pick (a connection to itself).
/// Something then answered there, the tunnel failed with words of its own, and the failure was
/// taken for `Unreachable` (seen once on Windows on ARM).
fn through_a_missing_proxy() -> (TcpSocket, mdm_engine::Options) {
    let held = TcpSocket::new_v4().unwrap();
    held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = held.local_addr().unwrap().port();
    let net = mdm_tools::net(Proxy::Manual {
        url: format!("http://127.0.0.1:{port}"),
        login: None,
        http: true,
        https: true,
    });
    (held, net)
}

#[tokio::test]
async fn ytdlp_names_the_server_and_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (_held, net) = through_a_missing_proxy();
    let e = mdm_tools::install_latest(dir.path(), &net, None)
        .await
        .expect_err("no proxy there");
    assert_eq!(
        e.network(),
        Some(("api.github.com".to_owned(), Fault::Refused)),
        "{e}"
    );
}

/// gallery-dl has a build of its own for Windows and Linux on x86-64 only.
#[cfg(any(windows, all(target_os = "linux", target_arch = "x86_64")))]
#[tokio::test]
async fn gallery_dl_names_the_server_and_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (_held, net) = through_a_missing_proxy();
    let e = mdm_tools::gallery::install_latest(dir.path(), &net, None)
        .await
        .expect_err("no proxy there");
    assert_eq!(e.network().map(|n| n.1), Some(Fault::Refused), "{e}");
}

/// ffmpeg is installed by MDM on 64-bit Windows only (elsewhere the system's is used, or one
/// chosen in Helpers: BtbN builds none for 32-bit Windows).
#[cfg(all(windows, any(target_arch = "x86_64", target_arch = "aarch64")))]
#[tokio::test]
async fn ffmpeg_names_the_server_and_the_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (_held, net) = through_a_missing_proxy();
    let e = mdm_tools::ffmpeg::install_latest(dir.path(), &net, None)
        .await
        .expect_err("no proxy there");
    assert_eq!(
        e.network(),
        Some(("api.github.com".to_owned(), Fault::Refused)),
        "{e}"
    );
}

/// 32-bit Windows: no ffmpeg of MDM's (BtbN builds none), said at once, without asking a server.
#[cfg(all(windows, target_arch = "x86"))]
#[tokio::test]
async fn ffmpeg_on_32_bit_windows_says_it_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let (_held, net) = through_a_missing_proxy();
    let e = mdm_tools::ffmpeg::install_latest(dir.path(), &net, None)
        .await
        .expect_err("no build");
    assert!(matches!(e, mdm_tools::Error::Missing(_)), "{e}");
    assert!(!mdm_tools::ffmpeg::available());
}

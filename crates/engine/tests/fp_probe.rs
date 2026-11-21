//! Temporary measurement probe (not for merging): the browser-like connection's fingerprint as
//! an echo service sees it. Run with MDM_FP_OUT=<dir> and MDM_FP_URL; ignored otherwise.
#![cfg(feature = "browser-connection")]

use mdm_engine::Options;
use mdm_engine::browserlike::Connection;
use reqwest::header::HeaderMap;
use tokio_util::sync::CancellationToken;
use url::Url;

const AGENTS: &[(&str, &str)] = &[
    (
        "chrome157",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36",
    ),
    (
        "edge157",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36 Edg/157.0.0.0",
    ),
    (
        "chrome158",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/158.0.0.0 Safari/537.36",
    ),
];

#[tokio::test]
#[ignore]
async fn fp_probe() {
    let Ok(out) = std::env::var("MDM_FP_OUT") else {
        return;
    };
    let url: Url = std::env::var("MDM_FP_URL")
        .unwrap_or_else(|_| "https://tls.peet.ws/api/all".to_owned())
        .parse()
        .unwrap();
    for (name, agent) in AGENTS {
        for run in 0..2 {
            let opts = Options {
                user_agent: (*agent).to_owned(),
                ..Options::default()
            };
            let conn = Connection::new(&opts, &url, &HeaderMap::new()).unwrap();
            let mut answer = conn
                .get(url.clone(), HeaderMap::new(), &CancellationToken::new())
                .await
                .unwrap();
            let mut body = Vec::new();
            while let Some(b) = answer.chunk().await.unwrap() {
                body.extend_from_slice(&b);
            }
            println!("{name} run {run}: {} {} bytes", answer.status, body.len());
            std::fs::write(format!("{out}/mdm-{name}-{run}.json"), &body).unwrap();
        }
    }
}

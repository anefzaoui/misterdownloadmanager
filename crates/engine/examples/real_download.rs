//! Test MDM's download engine against real live URLs.
//! Demonstrates the difference between static connection count vs learned/adaptive host limits.
//!
//! Usage:
//!   cargo run -p mdm-engine --example real_download -- <url> [max_connections]

use std::sync::Arc;
use std::time::{Duration, Instant};

use mdm_engine::download::{ConnStatus, State};
use mdm_engine::hosts::HostLimits;
use mdm_engine::{Destination, Download, Options, Request};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("Mister Download Manager (MDM) - Real URL Download Benchmark");
        println!("Usage: cargo run -p mdm-engine --example real_download -- <url> [max_connections]");
        println!("\nExamples:");
        println!("  # Cloudflare 25MB test file:");
        println!("  cargo run -p mdm-engine --example real_download -- \"https://speed.cloudflare.com/__down?bytes=25000000\"");
        println!("  # OVH 10MB test file:");
        println!("  cargo run -p mdm-engine --example real_download -- \"https://proof.ovh.net/files/10Mb.dat\"");
        return;
    }

    let url_str = &args[0];
    let max_conns: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(16);

    let parsed_url: url::Url = url_str.parse().expect("Invalid URL format");
    let host = parsed_url.host_str().unwrap_or("unknown").to_string();

    println!("==========================================================================");
    println!("🌐 Target URL : {}", url_str);
    println!("🏢 Host       : {}", host);
    println!("⚙️ Max Conns  : {}", max_conns);
    println!("==========================================================================");

    // Run 1: Static / Unconstrained run (Simulating default 16 connections)
    println!("\n▶️  [Pass 1] Running with Default Static Connections (Cap: {})...", max_conns);
    let fresh_limits = Arc::new(HostLimits::default());
    let res1 = run_download(url_str, max_conns, fresh_limits.clone()).await;

    // Run 2: Learned / Optimal Run (Simulating learned host profile from Issue #1240)
    // For CDNs (like Cloudflare / OVH), throughput saturates at 2-3 connections.
    let adaptive_limits = Arc::new(HostLimits::default());
    adaptive_limits.record_optimal(&host, 2); // Learned optimal for fast single pipe

    println!("\n▶️  [Pass 2] Running with Learned Adaptive Host Limit (Optimal: 2 Conns)...");
    let res2 = run_download(url_str, max_conns, adaptive_limits.clone()).await;

    // Print Side-by-Side Telemetry Table
    println!("\n==========================================================================");
    println!("📊 Real URL Download Benchmark Results");
    println!("==========================================================================");
    println!("{:<28} | {:<10} | {:<12} | {:<11} | {:<12}",
             "Strategy", "Time (s)", "Speed (MB/s)", "Final Conns", "Learned Cap");
    println!("--------------------------------------------------------------------------");

    println!("{:<28} | {:<10.2} | {:<12.2} | {:<11} | {:<12}",
             "Pass 1: Static (16 conns)",
             res1.duration.as_secs_f64(),
             res1.avg_speed_mb_s,
             res1.max_observed_conns,
             fresh_limits.limit_for(&host, max_conns));

    println!("{:<28} | {:<10.2} | {:<12.2} | {:<11} | {:<12}",
             "Pass 2: Learned (2 conns)",
             res2.duration.as_secs_f64(),
             res2.avg_speed_mb_s,
             res2.max_observed_conns,
             adaptive_limits.limit_for(&host, max_conns));

    let speed_diff = ((res2.avg_speed_mb_s - res1.avg_speed_mb_s) / res1.avg_speed_mb_s) * 100.0;
    let sign = if speed_diff >= 0.0 { "+" } else { "" };
    println!("👉 Speed Delta: {}{:.1}% | Socket Streams Saved: {}",
             sign, speed_diff, res1.max_observed_conns.saturating_sub(res2.max_observed_conns));
    println!("==========================================================================");
}

struct DownloadMetrics {
    duration: Duration,
    avg_speed_mb_s: f64,
    max_observed_conns: usize,
}

async fn run_download(url: &str, max_conns: u32, limits: Arc<HostLimits>) -> DownloadMetrics {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
    let opts = Options {
        max_connections: max_conns,
        host_limits: Some(limits),
        ..Options::default()
    };

    let download = Download::start(
        Request {
            url: url.parse().expect("Invalid URL"),
            headers: Default::default(),
            dest: Destination::Dir(temp_dir.path().to_owned()),
        },
        opts,
    );

    let start = Instant::now();
    let mut max_conns_seen = 0;
    let mut total_downloaded;

    print!("Progress: [");
    let mut last_pct = 0;

    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let snap = download.snapshot();

        let active_conns = snap
            .connections
            .iter()
            .filter(|c| matches!(c.status, ConnStatus::Receiving | ConnStatus::Connecting))
            .count();

        if active_conns > max_conns_seen {
            max_conns_seen = active_conns;
        }

        total_downloaded = snap.downloaded;

        if let Some(total_size) = snap.size {
            if total_size > 0 {
                let pct = (total_downloaded as f64 / total_size as f64 * 100.0) as usize;
                while last_pct < pct {
                    if last_pct % 5 == 0 {
                        print!("#");
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                    }
                    last_pct += 1;
                }
            }
        }

        if matches!(snap.state, State::Done) {
            println!("] 100% DONE ({:.2} MB)", total_downloaded as f64 / 1024.0 / 1024.0);
            break;
        }

        if matches!(snap.state, State::Failed | State::Cancelled) {
            println!("] FAILED / CANCELLED");
            break;
        }
    }

    let elapsed = start.elapsed();
    let speed_mb_s = (total_downloaded as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64().max(0.001);

    DownloadMetrics {
        duration: elapsed,
        avg_speed_mb_s: speed_mb_s,
        max_observed_conns: max_conns_seen,
    }
}

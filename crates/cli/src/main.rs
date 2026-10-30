use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Parser, Subcommand};
use mdm_engine::download::{ConnStatus, State};
use mdm_engine::{Destination, Download, Options, Request};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

#[derive(Parser)]
#[command(
    version,
    about = "Mister Download Manager command line",
    long_about = "Mister Download Manager command line: downloads, videos and torrents without the \
                  window, and the running app's automation API. Guide: \
                  https://github.com/anefzaoui/misterdownloadmanager/blob/main/docs/command-line.md"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download a URL. Ctrl+C pauses; continue later with `resume`.
    Get {
        /// The address to download (http or https).
        url: url::Url,
        /// Folder to save into (name from the server), or a file path ending in a name.
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Maximum connections.
        #[arg(short, long, default_value_t = Options::default().max_connections)]
        connections: u32,
        /// Extra request header, e.g. "Referer: https://example.com/" (repeatable).
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
        /// The finished file must have this hash: hex MD5 / SHA-1 / SHA-256 / SHA-512 / CRC32,
        /// optionally named ("sha256:…").
        #[arg(long, value_parser = parse_checksum)]
        checksum: Option<mdm_engine::verify::Hash>,
    },
    /// The running app's downloads, as JSON (automation API; turn it on with `api on`).
    Status,
    /// Install (or update) yt-dlp for video downloads: its checksum list is verified against
    /// yt-dlp's signing key, the program against that list.
    InstallYtdlp,
    /// Install (or update) ffmpeg, for the video qualities that merge video and audio: BtbN's
    /// LGPL build, checked against its release's SHA-256 list (Windows; elsewhere the system's).
    InstallFfmpeg,
    /// What video a page holds, and the choices to download (needs `install-ytdlp`).
    VideoInfo {
        /// The page's address (YouTube, Vimeo, ...).
        url: String,
    },
    /// Download a video with yt-dlp (needs `install-ytdlp`).
    VideoGet {
        /// The page's address.
        url: String,
        /// Which quality: a choice's selector from `video-info` (e.g. "b[height=360]"); "b" is
        /// the best single file.
        #[arg(short, long, default_value = "b")]
        format: String,
        /// Folder to save into.
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
    },
    /// Make a .torrent of a file or folder (no trackers; peers find it through the DHT).
    TorrentCreate {
        /// The file or folder to share.
        path: PathBuf,
        /// The .torrent file to write.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// What a torrent holds: its name, files (numbered) and size.
    TorrentInfo {
        /// A magnet link or a .torrent file.
        source: String,
    },
    /// Download a torrent.
    TorrentGet {
        /// A magnet link or a .torrent file.
        source: String,
        /// Folder to save into.
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Only these files, by their numbers in `torrent-info` (e.g. 0,2); all by default.
        #[arg(long, value_delimiter = ',')]
        files: Option<Vec<usize>>,
        /// Stop after this many seconds (a test run).
        #[arg(long)]
        seconds: Option<u64>,
    },
    /// Pause downloads in the running app, by their ids from `status` (automation API).
    Pause {
        /// Download ids.
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Start (or resume) downloads in the running app, by id (automation API).
    Start {
        /// Download ids.
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Take downloads off the running app's list, keeping their files (automation API).
    Remove {
        /// Download ids.
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Take downloads off the running app's list and delete their files (automation API).
    Delete {
        /// Download ids.
        #[arg(required = true)]
        ids: Vec<u64>,
    },
    /// Turn the running app's automation API on or off (off by default).
    Api {
        /// on or off.
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Continue a paused or interrupted download from its `.mdm.json` state file.
    Resume {
        /// The `<file>.mdm.json` next to the unfinished download.
        state: PathBuf,
        /// Maximum connections.
        #[arg(short, long, default_value_t = Options::default().max_connections)]
        connections: u32,
        /// Extra request header, e.g. "Cookie: id=1" (repeatable).
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
    },
}

/// An automation API action on the running app's downloads; prints its answer.
fn act(action: &str, ids: Vec<u64>) -> anyhow::Result<()> {
    let cmd = mdm_ipc::Command {
        action: Some(action.into()),
        ids,
        ..Default::default()
    };
    println!("{}", mdm_ipc::request(&cmd).map_err(anyhow::Error::msg)?);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Get {
            url,
            output,
            connections,
            headers,
            checksum,
        } => {
            let dest = if output.is_dir() {
                Destination::Dir(output)
            } else {
                Destination::File(output)
            };
            let req = Request {
                url,
                headers: parse_headers(&headers)?,
                dest,
            };
            let opts = Options {
                checksum,
                ..opts(connections)
            };
            watch(Download::start(req, opts)).await
        }
        Cmd::InstallYtdlp => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let i = mdm_tools::install_latest(&dir).await?;
            println!(
                "yt-dlp {} verified and installed: {}",
                i.version,
                i.exe.display()
            );
            Ok(())
        }
        Cmd::InstallFfmpeg => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let i = mdm_tools::ffmpeg::install_latest(&dir).await?;
            println!(
                "ffmpeg {} verified and installed: {}",
                i.version,
                i.exe.display()
            );
            Ok(())
        }
        Cmd::VideoInfo { url } => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let tool = mdm_tools::installed(&dir)
                .ok_or_else(|| anyhow::anyhow!("yt-dlp is not installed (install-ytdlp)"))?;
            let v = mdm_tools::resolve::resolve(&tool.exe, &url)?;
            println!("{}", v.title);
            let merge = mdm_tools::ffmpeg::installed(&dir).is_some();
            for c in mdm_tools::resolve::choices(&v, merge) {
                let size = c
                    .size
                    .map_or(String::new(), |s| format!("  ~{} MB", s / 1_000_000));
                println!("  {}{size}    -f \"{}\"", c.label, c.selector);
            }
            Ok(())
        }
        Cmd::TorrentCreate { path, output } => {
            let spawner = mdm_torrent::librqbit::spawn_utils::BlockingSpawner::new(1);
            let made = mdm_torrent::librqbit::create_torrent(
                &path,
                mdm_torrent::librqbit::CreateTorrentOptions {
                    name: None,
                    trackers: Vec::new(),
                    piece_length: None,
                },
                &spawner,
            )
            .await?;
            std::fs::write(&output, made.as_bytes()?)?;
            println!("{}  {}", made.info_hash().as_string(), output.display());
            Ok(())
        }
        Cmd::TorrentInfo { source } => {
            let (t, _dir) = torrents().await?;
            let m = t
                .metadata(&torrent_source(&source)?, Duration::from_secs(180))
                .await?;
            println!("{}  {}  {} bytes", m.name, m.info_hash, m.total());
            for (i, f) in m.files.iter().enumerate().filter(|(_, f)| !f.padding) {
                println!("{i:>4}  {:>14}  {}", f.size, f.path.display());
            }
            t.stop().await;
            Ok(())
        }
        Cmd::TorrentGet {
            source,
            output,
            files,
            seconds,
        } => {
            let (t, _dir) = torrents().await?;
            let m = t
                .metadata(&torrent_source(&source)?, Duration::from_secs(180))
                .await?;
            let folder = m.folder_in(&output);
            let hash = t
                .add(&m.torrent, &folder, files, m.peers.clone(), false)
                .await?;
            let started = std::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let s = t.stats(&hash)?;
                eprint!(
                    "\r{:?} {} / {}  {:.0} KB/s  peers {}/{}      ",
                    s.phase,
                    s.done,
                    s.total,
                    s.down_speed / 1024.0,
                    s.peers_live,
                    s.peers_seen
                );
                if s.phase == mdm_torrent::Phase::Finished {
                    eprintln!();
                    println!("saved in {}", folder.display());
                    break;
                }
                if let Some(e) = s.error {
                    anyhow::bail!(e);
                }
                if seconds.is_some_and(|n| started.elapsed().as_secs() >= n) {
                    eprintln!();
                    t.pause(&hash).await?;
                    println!(
                        "stopped after {} s; {} bytes there",
                        started.elapsed().as_secs(),
                        s.done
                    );
                    break;
                }
            }
            t.stop().await;
            Ok(())
        }
        Cmd::VideoGet {
            url,
            format,
            output,
        } => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let tool = mdm_tools::installed(&dir)
                .ok_or_else(|| anyhow::anyhow!("yt-dlp is not installed (install-ytdlp)"))?;
            let v = mdm_tools::resolve::resolve(&tool.exe, &url)?;
            let stem = mdm_engine::naming::sanitize(&v.title);
            let ffmpeg = mdm_tools::ffmpeg::installed(&dir).map(|f| f.exe);
            let job = mdm_tools::video::VideoJob::start(
                &tool.exe,
                &url,
                &format,
                &output,
                &stem,
                None,
                ffmpeg.as_deref(),
            )?;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let p = job.progress();
                eprint!(
                    "\r{} / {}  {:.0} KB/s      ",
                    p.downloaded,
                    p.total.map_or("?".into(), |t| t.to_string()),
                    p.speed / 1024.0
                );
                match p.state {
                    mdm_tools::video::State::Running => {}
                    mdm_tools::video::State::Done(f) => {
                        eprintln!();
                        println!("saved {}", f.display());
                        return Ok(());
                    }
                    other => anyhow::bail!("{other:?}"),
                }
            }
        }
        Cmd::Status => {
            let cmd = mdm_ipc::Command {
                query: Some("list".into()),
                ..Default::default()
            };
            println!("{}", mdm_ipc::request(&cmd).map_err(anyhow::Error::msg)?);
            Ok(())
        }
        Cmd::Pause { ids } => act("pause", ids),
        Cmd::Start { ids } => act("start", ids),
        Cmd::Remove { ids } => act("remove", ids),
        Cmd::Delete { ids } => act("delete", ids),
        Cmd::Api { state } => {
            let cmd = mdm_ipc::Command {
                api: Some(state == "on"),
                ..Default::default()
            };
            println!("{}", mdm_ipc::request(&cmd).map_err(anyhow::Error::msg)?);
            Ok(())
        }
        Cmd::Resume {
            state,
            connections,
            headers,
        } => {
            let dl = Download::resume(state, parse_headers(&headers)?, opts(connections))?;
            watch(dl).await
        }
    }
}

fn opts(connections: u32) -> Options {
    Options {
        max_connections: connections,
        ..Options::default()
    }
}

fn parse_checksum(s: &str) -> Result<mdm_engine::verify::Hash, String> {
    mdm_engine::verify::Hash::parse(s)
        .ok_or_else(|| "not a checksum (hex MD5, SHA-1, SHA-256, SHA-512 or CRC32)".into())
}

fn parse_headers(raw: &[String]) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    for h in raw {
        let (k, v) = h
            .split_once(':')
            .with_context(|| format!("header `{h}` needs `Name: value`"))?;
        headers.insert(
            HeaderName::try_from(k.trim())?,
            HeaderValue::try_from(v.trim())?,
        );
    }
    Ok(headers)
}

/// Print progress until the download ends; Ctrl+C pauses it (state is saved).
async fn watch(dl: Download) -> anyhow::Result<()> {
    let started = Instant::now();
    let mut last = (Instant::now(), 0u64);
    let mut speed = 0.0;
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = tokio::signal::ctrl_c() => {
                dl.cancel();
                println!("\npausing…");
            }
        }
        let s = dl.snapshot();
        let dt = last.0.elapsed().as_secs_f64().max(1e-3);
        let inst = s.downloaded.saturating_sub(last.1) as f64 / dt;
        speed = if speed == 0.0 {
            inst
        } else {
            0.7 * speed + 0.3 * inst
        };
        last = (Instant::now(), s.downloaded);
        let active = s
            .connections
            .iter()
            .filter(|c| matches!(c.status, ConnStatus::Receiving))
            .count();
        let pct = s.size.map_or(String::new(), |t| {
            format!("{:5.1}% ", 100.0 * s.downloaded as f64 / t.max(1) as f64)
        });
        print!(
            "\r{pct}{} / {}  {}/s  conns {active}  segs {}   ",
            mb(s.downloaded),
            s.size.map_or("?".into(), mb),
            mb(speed as u64),
            s.segments.len()
        );
        std::io::stdout().flush().ok();
        if !matches!(
            s.state,
            State::Probing | State::Downloading | State::Completing
        ) {
            break;
        }
    }
    println!();
    let file = dl.snapshot().file;
    match dl.wait().await {
        Ok(out) => {
            println!(
                "saved {} ({}) in {:.1}s",
                out.path.display(),
                mb(out.size),
                started.elapsed().as_secs_f64()
            );
            Ok(())
        }
        Err(mdm_engine::Error::Cancelled) => {
            if let Some(f) = file {
                println!(
                    "paused. continue with:\n  misterdownloadmanager-cli resume \"{}\"",
                    mdm_engine::state::state_path(&f).display()
                );
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn mb(b: u64) -> String {
    format!("{:.2} MB", b as f64 / (1 << 20) as f64)
}

/// A throwaway torrent session for the command line (its own state folder, nothing kept).
async fn torrents() -> anyhow::Result<(mdm_torrent::Torrents, tempfile::TempDir)> {
    let dir = tempfile::tempdir()?;
    let t = mdm_torrent::Torrents::start(mdm_torrent::Config {
        state_dir: dir.path().to_owned(),
        listen: true,
        dht: true,
        offline: false,
    })
    .await?;
    Ok((t, dir))
}

fn torrent_source(s: &str) -> anyhow::Result<mdm_torrent::Source> {
    match mdm_torrent::Source::magnet(s) {
        Some(m) => Ok(m),
        None => Ok(mdm_torrent::Source::File(std::fs::read(s)?)),
    }
}

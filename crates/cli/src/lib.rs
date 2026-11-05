//! `misterdownloadmanager-cli`'s commands. The app's program carries them (#25: one download
//! engine shipped, not two): `misterdownloadmanager-cli` (main.rs) is a small launcher that runs
//! `misterdownloadmanager --cli <arguments>`, and the app's `main` hands those to [`main`].

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Parser, Subcommand};
use mdm_engine::download::{ConnStatus, MAX_CONNECTIONS_UNLOCKED, State};
use mdm_engine::{Destination, Download, Options, Proxy, Request};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::json;

#[derive(Parser)]
#[command(
    version,
    about = "Mister Download Manager command line",
    long_about = "Mister Download Manager command line: downloads, videos and torrents without the \
                  window, and the running app's automation API. Guide: \
                  https://github.com/anefzaoui/misterdownloadmanager/blob/main/docs/command-line.md"
)]
struct Cli {
    /// Print JSON for programs: one object per line (`progress` events, then `done`, `paused`
    /// or `stopped`, for a download; one object for a question), `{"error": …}` on failure.
    #[arg(long, global = true)]
    json: bool,
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
        /// Maximum connections (1 to 128).
        #[arg(short, long, default_value_t = Options::default().max_connections,
              value_parser = clap::value_parser!(u32).range(1..=MAX_CONNECTIONS_UNLOCKED as i64))]
        connections: u32,
        /// Extra request header, e.g. "Referer: https://example.com/" (repeatable).
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
        /// The finished file must have this hash: hex MD5 / SHA-1 / SHA-256 / SHA-512 / CRC32,
        /// optionally named ("sha256:…").
        #[arg(long, value_parser = parse_checksum)]
        checksum: Option<mdm_engine::verify::Hash>,
        /// The DNS that looks the names up: "system" (the default); a service and how to ask it,
        /// "cloudflare-https", "quad9-tls", "google-plain", "adguard-https", "nextdns-tls"; DNS
        /// servers' addresses, "9.9.9.9,149.112.112.112"; or a DNS-over-HTTPS address,
        /// "https://dns.example/dns-query". No falling back to the system's DNS.
        #[arg(long, default_value = "system", value_parser = parse_dns)]
        dns: mdm_engine::DnsChoice,
        /// The network adapter every connection goes through (#35): its name ("Ethernet",
        /// "wg0") or one of its addresses. When it isn't there or has no address, nothing is
        /// sent and the download fails.
        #[arg(long)]
        adapter: Option<String>,
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
        /// the best single file; "mp3:320", "mp3:256", "mp3:192" or "mp3:128" the sound as an
        /// MP3 at that bitrate, "mp3:0" at its best variable one (MP3 needs ffmpeg).
        #[arg(short, long, default_value = "b")]
        format: String,
        /// Folder to save into.
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Subtitles to save beside the video: language codes from `video-info`, separated by
        /// commas ("en,fr"), "all" for every one the uploader gave, or "none".
        #[arg(long, default_value = "none")]
        subs: String,
        /// Take the subtitles from the site's automatic captions instead of the uploader's.
        #[arg(long)]
        auto_subs: bool,
        /// Put the subtitles into the video file instead of beside it (needs ffmpeg; MP4/MKV).
        #[arg(long)]
        embed_subs: bool,
        /// The network adapter yt-dlp goes through (#35), as `get --adapter`: Windows only
        /// (elsewhere video pages are refused while an adapter is chosen).
        #[arg(long)]
        adapter: Option<String>,
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
        /// Use the torrent's files already in the folder: what matches the torrent is kept, the
        /// rest downloaded into them. Without it, such files stop the download.
        #[arg(long)]
        use_existing: bool,
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
        /// Maximum connections (1 to 128).
        #[arg(short, long, default_value_t = Options::default().max_connections,
              value_parser = clap::value_parser!(u32).range(1..=MAX_CONNECTIONS_UNLOCKED as i64))]
        connections: u32,
        /// Extra request header, e.g. "Cookie: id=1" (repeatable).
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
    },
}

/// One JSON line on stdout (`--json`).
fn emit(v: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// A failure already printed (the app's own `{"error"}` answer): exit 1, print nothing more.
#[derive(Debug)]
struct Reported;

impl std::fmt::Display for Reported {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("reported")
    }
}

impl std::error::Error for Reported {}

/// Ask the running app (automation API) and print its answer, which is JSON already; an
/// `{"error"}` answer fails the command.
fn ask(cmd: mdm_ipc::Command) -> anyhow::Result<()> {
    let answer = mdm_ipc::request(&cmd)
        .map_err(|e| anyhow::anyhow!("Mister Download Manager isn't running ({e})"))?;
    println!("{answer}");
    if serde_json::from_str::<mdm_ipc::api::ErrorAnswer>(&answer).is_ok() {
        return Err(Reported.into());
    }
    Ok(())
}

/// An automation API action on the running app's downloads; prints its answer.
fn act(action: &str, ids: Vec<u64>) -> anyhow::Result<()> {
    ask(mdm_ipc::Command {
        action: Some(action.into()),
        ids,
        ..Default::default()
    })
}

/// The first argument the launcher starts the app's program with: run these commands, not the
/// window. On Windows the launcher adds its process id (`--cli=1234`), see [`Launch`].
pub const FLAG: &str = "--cli";

/// The launcher's name, which the commands' help and messages show.
pub const NAME: &str = "misterdownloadmanager-cli";

/// The app's program, started for the command line.
pub struct Launch {
    /// Windows: the launcher's process id, `--cli=<id>`. The program ends if the launcher is
    /// killed, as one process would: a script that kills it (a time limit) doesn't leave a
    /// download going on unseen.
    pub launcher: Option<u32>,
    /// The command line's arguments, without the program's name.
    pub args: Vec<OsString>,
}

/// Was the app's program started for the command line? `args` are its own (`std::env::args_os`):
/// [`FLAG`] first, or on Linux and macOS a program named [`NAME`] (a link to the app's program
/// works as the launcher there; on Windows the app is a windowed program, which a console
/// doesn't wait for, so it needs the launcher).
pub fn launched(args: Vec<OsString>) -> Option<Launch> {
    let mut args = args.into_iter();
    let program = args.next()?;
    let rest: Vec<OsString> = args.collect();
    if let Some(first) = rest.first().and_then(|a| a.to_str()) {
        let launcher = if first == FLAG {
            Some(None)
        } else {
            first
                .strip_prefix(FLAG)
                .and_then(|r| r.strip_prefix('='))
                .and_then(|id| id.parse().ok())
                .map(Some)
        };
        if let Some(launcher) = launcher {
            return Some(Launch {
                launcher,
                args: rest[1..].to_vec(),
            });
        }
    }
    let by_name = std::path::Path::new(&program)
        .file_name()
        .is_some_and(|n| n == NAME);
    (cfg!(unix) && by_name).then_some(Launch {
        launcher: None,
        args: rest,
    })
}

/// Run the command line `args` (without the program's name): what `misterdownloadmanager-cli`
/// does, its output, and its exit code (1 when a command fails).
pub fn main(args: Vec<OsString>) -> ExitCode {
    let cli = Cli::parse_from(std::iter::once(OsString::from(NAME)).chain(args));
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let json = cli.json;
    match rt.block_on(run(cli.cmd, json)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.is::<Reported>() => ExitCode::FAILURE,
        Err(e) => {
            if json {
                emit(json!({ "error": format!("{e:#}") }));
            } else {
                eprintln!("Error: {e:?}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn run(cmd: Cmd, json: bool) -> anyhow::Result<()> {
    match cmd {
        Cmd::Get {
            url,
            output,
            connections,
            headers,
            checksum,
            dns,
            adapter,
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
                dns,
                // Worked out once, as the app does each second (a missing adapter: Down).
                route: adapter.map_or(mdm_engine::Route::Any, |a| {
                    mdm_engine::net::route_for(&a, &mdm_engine::net::adapters())
                }),
                ..opts(connections)
            };
            watch(Download::start(req, opts), json).await
        }
        Cmd::InstallYtdlp => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let i = mdm_tools::install_latest(&dir, &mdm_tools::net(Proxy::System), None).await?;
            if json {
                emit(json!({ "tool": "yt-dlp", "version": i.version, "path": i.exe }));
            } else {
                println!(
                    "yt-dlp {} verified and installed: {}",
                    i.version,
                    i.exe.display()
                );
            }
            Ok(())
        }
        Cmd::InstallFfmpeg => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let i = mdm_tools::ffmpeg::install_latest(&dir, &mdm_tools::net(Proxy::System), None)
                .await?;
            if json {
                emit(json!({ "tool": "ffmpeg", "version": i.version, "path": i.exe }));
            } else {
                println!(
                    "ffmpeg {} verified and installed: {}",
                    i.version,
                    i.exe.display()
                );
            }
            Ok(())
        }
        Cmd::VideoInfo { url } => {
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let tool = mdm_tools::installed(&dir)
                .ok_or_else(|| anyhow::anyhow!("yt-dlp is not installed (install-ytdlp)"))?;
            let v = mdm_tools::resolve::resolve(&tool.exe, &url, &Default::default())?;
            let merge = mdm_tools::ffmpeg::installed(&dir).is_some();
            let choices = mdm_tools::resolve::choices(&v, merge);
            // The subtitles a person would be offered (the automatic ones: the spoken
            // language's and English).
            let subs = mdm_tools::resolve::sub_choices(&v, "en");
            if json {
                let choices: Vec<_> = choices
                    .iter()
                    .map(|c| json!({ "label": c.label, "format": c.selector, "size": c.size }))
                    .collect();
                let subs: Vec<_> = subs
                    .iter()
                    .map(|s| json!({ "code": s.code, "name": s.name, "automatic": s.automatic }))
                    .collect();
                emit(json!({ "title": v.title, "choices": choices, "subtitles": subs }));
                return Ok(());
            }
            println!("{}", v.title);
            for c in choices {
                let size = c
                    .size
                    .map_or(String::new(), |s| format!("  ~{} MB", s / 1_000_000));
                println!("  {}{size}    -f \"{}\"", c.label, c.selector);
            }
            if !subs.is_empty() {
                println!("subtitles:");
                for s in subs {
                    let auto = if s.automatic { "  --auto-subs" } else { "" };
                    println!("  {}    --subs {}{auto}", s.name, s.code);
                }
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
            if json {
                emit(json!({ "info_hash": made.info_hash().as_string(), "path": output }));
            } else {
                println!("{}  {}", made.info_hash().as_string(), output.display());
            }
            Ok(())
        }
        Cmd::TorrentInfo { source } => {
            let (t, _dir) = torrents().await?;
            let m = t
                .metadata(&torrent_source(&source)?, Duration::from_secs(180))
                .await?;
            let files = m.files.iter().enumerate().filter(|(_, f)| !f.padding);
            if json {
                let files: Vec<_> = files
                    .map(|(i, f)| json!({ "index": i, "size": f.size, "path": f.path }))
                    .collect();
                emit(json!({
                    "name": m.name,
                    "info_hash": m.info_hash,
                    "size": m.total(),
                    "files": files,
                }));
            } else {
                println!("{}  {}  {} bytes", m.name, m.info_hash, m.total());
                for (i, f) in files {
                    println!("{i:>4}  {:>14}  {}", f.size, f.path.display());
                }
            }
            t.stop().await;
            Ok(())
        }
        Cmd::TorrentGet {
            source,
            output,
            files,
            seconds,
            use_existing,
        } => {
            let (t, _dir) = torrents().await?;
            let m = t
                .metadata(&torrent_source(&source)?, Duration::from_secs(180))
                .await?;
            // The app asks first; here, the flag says yes (#40).
            let there = m.existing_in(&output);
            if let (false, Some(first)) = (use_existing, there.first()) {
                t.stop().await;
                anyhow::bail!(
                    "files of the torrent are already there ({}, first: {}); add --use-existing \
                     to use them, or choose another folder with -o",
                    there.len(),
                    first.display()
                );
            }
            let folder = m.folder_in(&output);
            let hash = t
                .add(&m.torrent, &folder, files, m.peers.clone(), false)
                .await?;
            let started = std::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let s = t.stats(&hash)?;
                if json {
                    emit(json!({
                        "event": "progress",
                        "phase": format!("{:?}", s.phase),
                        "downloaded": s.done,
                        "size": s.total,
                        "speed": s.down_speed as u64,
                        "peers": s.peers_live,
                        "peers_seen": s.peers_seen,
                    }));
                } else {
                    eprint!(
                        "\r{:?} {} / {}  {:.0} KB/s  peers {}/{}      ",
                        s.phase,
                        s.done,
                        s.total,
                        s.down_speed / 1024.0,
                        s.peers_live,
                        s.peers_seen
                    );
                }
                if s.phase == mdm_torrent::Phase::Finished {
                    if json {
                        emit(json!({ "event": "done", "path": folder, "size": s.total }));
                    } else {
                        eprintln!();
                        println!("saved in {}", folder.display());
                    }
                    break;
                }
                if let Some(e) = s.error {
                    anyhow::bail!(e);
                }
                if seconds.is_some_and(|n| started.elapsed().as_secs() >= n) {
                    t.pause(&hash).await?;
                    let secs = started.elapsed().as_secs();
                    if json {
                        emit(json!({ "event": "stopped", "downloaded": s.done, "seconds": secs }));
                    } else {
                        eprintln!();
                        println!("stopped after {secs} s; {} bytes there", s.done);
                    }
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
            subs,
            auto_subs,
            embed_subs,
            adapter,
        } => {
            if let Some(a) = adapter {
                mdm_tools::proc::set_route(mdm_engine::net::route_for(
                    &a,
                    &mdm_engine::net::adapters(),
                ));
            }
            let subs = subs_arg(&subs, auto_subs, embed_subs);
            let dir = mdm_tools::tools_dir().ok_or_else(|| anyhow::anyhow!("no data folder"))?;
            let tool = mdm_tools::installed(&dir)
                .ok_or_else(|| anyhow::anyhow!("yt-dlp is not installed (install-ytdlp)"))?;
            let v = mdm_tools::resolve::resolve(&tool.exe, &url, &Default::default())?;
            // A stem no file in the folder has, whatever the extension: yt-dlp takes any
            // `<stem>.*` there as its own (#32). The extension only sizes the name.
            let ext = if mdm_tools::resolve::mp3_quality(&format).is_some() {
                "mp3"
            } else {
                "mp4"
            };
            let name = format!("{}.{ext}", mdm_engine::naming::sanitize(&v.title));
            let free = mdm_engine::naming::stem_destination(&output, &name, |_| false);
            let stem = free
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "video".into());
            let ffmpeg = mdm_tools::ffmpeg::installed(&dir).map(|f| f.exe);
            let job = mdm_tools::video::VideoJob::start(
                &tool.exe,
                &url,
                &format,
                &output,
                &stem,
                None,
                ffmpeg.as_deref(),
                // No size shown here: the progress steadies itself (video::Meter).
                None,
                &Default::default(),
                &subs,
            )?;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let p = job.progress();
                if json {
                    emit(json!({
                        "event": "progress",
                        "downloaded": p.downloaded,
                        "size": p.total,
                        "speed": p.speed as u64,
                    }));
                } else {
                    eprint!(
                        "\r{} / {}  {:.0} KB/s      ",
                        p.downloaded,
                        p.total.map_or("?".into(), |t| t.to_string()),
                        p.speed / 1024.0
                    );
                }
                match p.state {
                    mdm_tools::video::State::Running => {}
                    mdm_tools::video::State::Done(f) => {
                        // Subtitles the site refused: the video is saved all the same (#68).
                        let problems: Vec<(String, String, String, bool)> = p
                            .sub_problems
                            .iter()
                            .map(|s| {
                                let why = match &s.reason {
                                    mdm_tools::video::SubReason::TooManyRequests => {
                                        "the site refused (too many requests)".to_owned()
                                    }
                                    mdm_tools::video::SubReason::Other(w) => w.clone(),
                                };
                                (s.code.clone(), s.name.clone(), why, s.beside)
                            })
                            .collect();
                        if json {
                            let problems: Vec<_> = problems
                                .iter()
                                .map(|(code, name, why, beside)| {
                                    json!({ "code": code, "name": name, "reason": why, "beside": beside })
                                })
                                .collect();
                            emit(json!({
                                "event": "done",
                                "path": f,
                                "subtitles": p.side_files,
                                "subtitle_problems": problems,
                            }));
                        } else {
                            eprintln!();
                            println!("saved {}", f.display());
                            for s in &p.side_files {
                                println!("saved {}", s.display());
                            }
                            for (code, name, why, beside) in problems {
                                if beside {
                                    println!(
                                        "subtitles in {name} ({code}) came on a second try: beside the video, not in it"
                                    );
                                } else {
                                    println!(
                                        "subtitles in {name} ({code}) couldn't be downloaded: {why}"
                                    );
                                }
                            }
                        }
                        return Ok(());
                    }
                    other => anyhow::bail!("{other:?}"),
                }
            }
        }
        Cmd::Status => ask(mdm_ipc::Command {
            query: Some("list".into()),
            ..Default::default()
        }),
        Cmd::Pause { ids } => act("pause", ids),
        Cmd::Start { ids } => act("start", ids),
        Cmd::Remove { ids } => act("remove", ids),
        Cmd::Delete { ids } => act("delete", ids),
        Cmd::Api { state } => ask(mdm_ipc::Command {
            api: Some(state == "on"),
            ..Default::default()
        }),
        Cmd::Resume {
            state,
            connections,
            headers,
        } => {
            let dl = Download::resume(state, parse_headers(&headers)?, opts(connections))?;
            watch(dl, json).await
        }
    }
}

/// `video-get --subs` (#68): "none" (or nothing), "all", or language codes separated by commas.
fn subs_arg(langs: &str, automatic: bool, embed: bool) -> mdm_tools::video::Subs {
    let langs: Vec<String> = langs
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("none"))
        .map(|l| {
            if l.eq_ignore_ascii_case(mdm_tools::video::ALL_UPLOADERS) {
                mdm_tools::video::ALL_UPLOADERS.to_owned()
            } else {
                l.to_owned()
            }
        })
        .collect();
    mdm_tools::video::Subs {
        langs,
        automatic,
        embed,
    }
}

fn opts(connections: u32) -> Options {
    Options {
        max_connections: connections,
        ..Options::default()
    }
}

/// `--dns`: see [`Cmd::Get`].
fn parse_dns(s: &str) -> Result<mdm_engine::DnsChoice, String> {
    use mdm_engine::DnsChoice;
    use mdm_engine::dns::{Dns, Mode, Preset, parse_servers};
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("system") {
        return Ok(DnsChoice::default());
    }
    if s.starts_with("https://") {
        return Ok(DnsChoice {
            dns: Dns::Https {
                url: s.to_owned(),
                bootstrap: Vec::new(),
            },
            fallback: false,
            label: s.to_owned(),
        });
    }
    if let Some((id, how)) = s.rsplit_once('-')
        && let Some(p) = Preset::by_id(&id.to_ascii_lowercase())
    {
        let mode = match how.to_ascii_lowercase().as_str() {
            "plain" => Mode::Plain,
            "tls" => Mode::Tls,
            "https" => Mode::Https,
            _ => return Err(format!("`{how}`: plain, tls or https")),
        };
        if !p.offers(mode) {
            return Err(format!("{} isn't offered as {how}", p.name));
        }
        return Ok(p.choice(mode, "", false, format!("{} ({how})", p.name)));
    }
    let servers = parse_servers(s, 53).map_err(|bad| {
        format!("`{bad}`: not a DNS server's address, a service or an https:// address")
    })?;
    Ok(DnsChoice {
        dns: Dns::Plain(servers),
        fallback: false,
        label: s.to_owned(),
    })
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

/// Print progress until the download ends; Ctrl+C pauses it (state is saved). `json`: one
/// `progress` line per tick, then `done` or `paused`.
async fn watch(dl: Download, json: bool) -> anyhow::Result<()> {
    let started = Instant::now();
    let mut last = (Instant::now(), 0u64);
    let mut speed = 0.0;
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = tokio::signal::ctrl_c() => {
                dl.cancel();
                if !json {
                    println!("\npausing…");
                }
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
        if json {
            emit(json!({
                "event": "progress",
                "downloaded": s.downloaded,
                "size": s.size,
                "speed": speed as u64,
                "connections": active,
                "segments": s.segments.len(),
            }));
        } else {
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
        }
        if !matches!(
            s.state,
            State::Probing | State::Downloading | State::Completing
        ) {
            break;
        }
    }
    if !json {
        println!();
    }
    let file = dl.snapshot().file;
    match dl.wait().await {
        Ok(out) => {
            let secs = started.elapsed().as_secs_f64();
            if json {
                emit(
                    json!({ "event": "done", "path": out.path, "size": out.size, "seconds": secs }),
                );
            } else {
                println!(
                    "saved {} ({}) in {secs:.1}s",
                    out.path.display(),
                    mb(out.size)
                );
            }
            Ok(())
        }
        Err(mdm_engine::Error::Cancelled) => {
            let state = file.map(|f| mdm_engine::state::state_path(&f));
            if json {
                emit(json!({ "event": "paused", "state": state }));
            } else if let Some(state) = state {
                println!(
                    "paused. continue with:\n  {NAME} resume \"{}\"",
                    state.display()
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
    let t = mdm_torrent::Torrents::start(mdm_torrent::Config::new(dir.path().to_owned())).await?;
    Ok((t, dir))
}

fn torrent_source(s: &str) -> anyhow::Result<mdm_torrent::Source> {
    match mdm_torrent::Source::magnet(s) {
        Some(m) => Ok(m),
        None => Ok(mdm_torrent::Source::File(mdm_torrent::read_file(
            std::path::Path::new(s),
        )?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_launcher_flag_starts_the_command_line() {
        let l = launched(args(&["mdm", "--cli", "get", "x"])).unwrap();
        assert_eq!((l.launcher, l.args), (None, args(&["get", "x"])));
        let l = launched(args(&["mdm", "--cli=42", "status"])).unwrap();
        assert_eq!((l.launcher, l.args), (Some(42), args(&["status"])));
        // The app's own command line, and look-alikes, start the app.
        assert!(launched(args(&["mdm"])).is_none());
        assert!(launched(args(&["mdm", "/d", "--cli"])).is_none());
        assert!(launched(args(&["mdm", "--cli=x"])).is_none());
        assert!(launched(args(&["mdm", "--client"])).is_none());
        // A link named as the launcher (Linux and macOS).
        let named = launched(args(&["/usr/bin/misterdownloadmanager-cli", "status"]));
        assert_eq!(named.is_some(), cfg!(unix));
    }
}

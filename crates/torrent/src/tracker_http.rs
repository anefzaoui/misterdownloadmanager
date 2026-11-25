//! The torrent session's HTTP client (HTTP trackers, and UPnP to the router): MDM's TLS setup
//! (`mdm_engine::tls`), and the rule of `trackers` kept for every address it connects to, not
//! only the one a torrent names. A tracker's redirect is followed only to a tracker MDM may
//! contact; a name is looked up and only its addresses on the internet are used; and no request
//! says where it came from (`Referer`).
//!
//! The router's address (UPnP) is never a name, so it isn't looked up here; a redirect from it
//! stays on it. A proxy's own name (Torrent settings', or the system's) is looked up as it is.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::trackers;

/// The most redirects one request follows (reqwest's own default is 10; trackers seldom have one).
const MAX_REDIRECTS: usize = 5;

/// What the session's HTTP client keeps to.
#[derive(Clone)]
pub(crate) struct Guard {
    /// Through Torrent settings' proxy (`trackers::allowed`).
    proxied: bool,
    /// The proxies' names (lowercase), whose addresses may be anywhere: a proxy on this computer
    /// or the local network is the person's choice.
    proxies: Vec<String>,
    /// Which resolved addresses may be used: `trackers::public`. Tests pass another one, with
    /// their trackers on loopback.
    judge: fn(IpAddr) -> bool,
    /// Tests: names answered from here instead of the system's DNS.
    names: Option<Arc<HashMap<String, Vec<IpAddr>>>>,
}

impl Guard {
    /// For a session with `proxy` (Torrent settings', `socks5://…`), and the system's proxies
    /// for HTTP and HTTPS as reqwest finds them.
    pub(crate) fn new(proxy: Option<&str>) -> Guard {
        let mut proxies: Vec<String> = Vec::new();
        let probes = ["http://tracker.invalid/", "https://tracker.invalid/"];
        let system = probes
            .iter()
            .filter_map(|p| url::Url::parse(p).ok())
            .filter_map(|u| mdm_engine::net::system_proxy(&u));
        for p in proxy.map(str::to_owned).into_iter().chain(system) {
            if let Some(host) = url::Url::parse(&p)
                .ok()
                .as_ref()
                .and_then(url::Url::host_str)
            {
                let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
                if !proxies.contains(&host) {
                    proxies.push(host);
                }
            }
        }
        Guard {
            proxied: proxy.is_some(),
            proxies,
            judge: trackers::public,
            names: None,
        }
    }

    /// `builder` as the session's clients are built: MDM's TLS, no `Referer`, redirects and
    /// names checked.
    pub(crate) fn configure(&self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        let proxied = self.proxied;
        mdm_engine::tls::configure(builder)
            .referer(false)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > MAX_REDIRECTS {
                    return attempt.error("too many redirects");
                }
                let first = attempt.previous().first();
                if first.is_some_and(|first| may_follow(first, attempt.url(), proxied)) {
                    attempt.follow()
                } else {
                    let to = attempt.url().to_string();
                    attempt.error(format!(
                        "a redirect to {to}, which MDM doesn't contact for torrents, wasn't followed"
                    ))
                }
            }))
            .dns_resolver(Arc::new(self.clone()))
    }
}

/// Whether a request that began at `first` may follow a redirect to `next`: to a tracker MDM may
/// contact (`trackers::allowed`), as a torrent naming it would be. A request MDM began at an
/// address it would refuse a torrent (UPnP's, to the router) stays on that one.
fn may_follow(first: &url::Url, next: &url::Url, proxied: bool) -> bool {
    if trackers::allowed(first.as_str(), proxied) {
        return trackers::allowed(next.as_str(), proxied);
    }
    matches!(next.scheme(), "http" | "https")
        && next.host() == first.host()
        && next.port_or_known_default() == first.port_or_known_default()
}

impl Resolve for Guard {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().trim_matches(['[', ']']).to_ascii_lowercase();
        let any = self.proxies.contains(&host);
        let (judge, names) = (self.judge, self.names.clone());
        Box::pin(async move {
            // An address written as one (reqwest asks here for those too, through a SOCKS proxy
            // that leaves the looking up to this computer).
            let found: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
                vec![SocketAddr::new(ip, 0)]
            } else if let Some(names) = names {
                let ips = names.get(&host).ok_or("no such name")?;
                ips.iter().map(|ip| SocketAddr::new(*ip, 0)).collect()
            } else {
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect()
            };
            let kept: Vec<SocketAddr> =
                found.into_iter().filter(|a| any || judge(a.ip())).collect();
            if kept.is_empty() {
                return Err(format!(
                    "{host} leads only to this computer or the local network, which MDM doesn't \
                     contact for torrents"
                )
                .into());
            }
            Ok::<Addrs, Box<dyn std::error::Error + Send + Sync>>(Box::new(kept.into_iter()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server on loopback that answers each request with `answer(path)` and keeps the
    /// requests it got, whole.
    struct Stub {
        port: u16,
        got: Arc<Mutex<Vec<String>>>,
    }

    impl Stub {
        async fn start(answer: impl Fn(&str) -> String + Send + Sync + 'static) -> Stub {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let port = listener.local_addr().unwrap().port();
            let got: Arc<Mutex<Vec<String>>> = Default::default();
            let (seen, answer) = (got.clone(), Arc::new(answer));
            tokio::spawn(async move {
                while let Ok((mut s, _)) = listener.accept().await {
                    let (seen, answer) = (seen.clone(), answer.clone());
                    tokio::spawn(async move {
                        let mut req = Vec::new();
                        let mut buf = [0u8; 4096];
                        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                            match s.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => req.extend_from_slice(&buf[..n]),
                            }
                        }
                        let req = String::from_utf8_lossy(&req).into_owned();
                        let path = req.split(' ').nth(1).unwrap_or_default().to_owned();
                        seen.lock().unwrap().push(req);
                        let _ = s.write_all(answer(&path).as_bytes()).await;
                        let _ = s.shutdown().await;
                    });
                }
            });
            Stub { port, got }
        }

        fn got(&self) -> Vec<String> {
            self.got.lock().unwrap().clone()
        }
    }

    fn redirect(to: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    }

    fn announced() -> String {
        let body = "d8:intervali60e5:peers0:e";
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// A client as the session's, with `names` answered without DNS; `loopback_counts`: this
    /// computer's 127.0.0.1 stands in for the internet (a test's "public" tracker is on it),
    /// while the rest of loopback ([`ELSEWHERE`]) is still this computer.
    fn client(names: &[(&str, IpAddr)], loopback_counts: bool) -> reqwest::Client {
        let mut map: HashMap<String, Vec<IpAddr>> = HashMap::new();
        for (n, ip) in names {
            map.entry(n.to_string()).or_default().push(*ip);
        }
        let guard = Guard {
            names: Some(Arc::new(map)),
            judge: if loopback_counts {
                |ip: IpAddr| ip == LOOPBACK || trackers::public(ip)
            } else {
                trackers::public
            },
            ..Guard::new(None)
        };
        guard
            .configure(reqwest::Client::builder().no_proxy())
            .build()
            .unwrap()
    }

    const LOOPBACK: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    /// Another address of this computer, never connected to.
    const ELSEWHERE: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 2));

    /// A tracker on the internet that redirects to this computer, by address or by a name
    /// for it: the second request is never sent, and the first said nothing of where it came
    /// from. A redirect to another tracker MDM may contact is followed.
    #[tokio::test]
    async fn a_redirect_into_this_computer_is_not_followed() {
        let target = Stub::start(|_| announced()).await;
        for to in [
            format!("http://127.0.0.1:{}/admin?reboot=1", target.port),
            format!("http://localhost:{}/admin", target.port),
            format!("http://[::1]:{}/admin", target.port),
            format!("http://10.0.0.1:{}/admin", target.port),
            format!("http://router:{}/admin", target.port),
            "file:///etc/passwd".to_owned(),
        ] {
            let to2 = to.clone();
            let tracker = Stub::start(move |_| redirect(&to2)).await;
            let c = client(&[("tracker.example", LOOPBACK)], true);
            let url = format!(
                "http://tracker.example:{}/announce?info_hash=x",
                tracker.port
            );
            // Refused by the rule, or (not http) never followed by reqwest at all.
            match c.get(&url).send().await {
                Err(err) => assert!(err.is_redirect(), "{to}: {err:?}"),
                Ok(r) => assert!(
                    r.status().is_redirection() && to.starts_with("file:"),
                    "{to}"
                ),
            }
            assert_eq!(tracker.got().len(), 1, "{to}");
        }
        assert!(target.got().is_empty(), "{:?}", target.got());

        // To another tracker by a name: followed, and neither request carries a Referer.
        let moved = format!("http://tracker2.example:{}/announce", target.port);
        let tracker = Stub::start(move |_| redirect(&moved)).await;
        let c = client(
            &[
                ("tracker.example", LOOPBACK),
                ("tracker2.example", LOOPBACK),
            ],
            true,
        );
        let url = format!("http://tracker.example:{}/announce", tracker.port);
        let r = c.get(&url).send().await.unwrap();
        assert!(r.status().is_success());
        let got = target.got();
        assert_eq!(got.len(), 1);
        assert!(
            !got[0].to_ascii_lowercase().contains("referer"),
            "{}",
            got[0]
        );
    }

    /// A redirect to a name that resolves to this computer: the name isn't used, so nothing is
    /// sent there.
    #[tokio::test]
    async fn a_redirect_to_a_name_for_this_computer_is_not_followed() {
        let target = Stub::start(|_| announced()).await;
        let to = format!("http://here.example:{}/admin", target.port);
        let tracker = Stub::start(move |_| redirect(&to)).await;
        let c = client(
            &[("tracker.example", LOOPBACK), ("here.example", ELSEWHERE)],
            true,
        );
        let url = format!("http://tracker.example:{}/announce", tracker.port);
        let err = c.get(&url).send().await.unwrap_err();
        assert!(format!("{err:?}").contains("doesn't contact"), "{err:?}");
        assert_eq!(tracker.got().len(), 1);
        assert!(target.got().is_empty(), "{:?}", target.got());
    }

    /// A tracker's name that resolves to this computer or the local network isn't asked; one
    /// that resolves to the internet is, and a name with both kinds keeps only the public ones.
    #[tokio::test]
    async fn names_that_lead_to_this_computer_are_not_asked() {
        let tracker = Stub::start(|_| announced()).await;
        let local: IpAddr = "192.168.1.1".parse().unwrap();
        let c = client(
            &[("localtest.example", LOOPBACK), ("nas.example", local)],
            false,
        );
        for name in ["localtest.example", "nas.example"] {
            let url = format!("http://{name}:{}/announce", tracker.port);
            let err = c.get(&url).send().await.unwrap_err();
            assert!(
                format!("{err:?}").contains("doesn't contact"),
                "{name}: {err:?}"
            );
        }
        assert!(tracker.got().is_empty(), "{:?}", tracker.got());

        // The guard itself, with addresses that are on the internet: those pass, the local one
        // of a mixed answer is left out.
        let public: IpAddr = "8.8.8.8".parse().unwrap();
        let guard = Guard {
            names: Some(Arc::new(HashMap::from([(
                "mixed.example".to_owned(),
                vec![LOOPBACK, public, local],
            )]))),
            ..Guard::new(None)
        };
        let got: Vec<IpAddr> = guard
            .resolve("mixed.example".parse().unwrap())
            .await
            .unwrap()
            .map(|a| a.ip())
            .collect();
        assert_eq!(got, [public]);
        // A proxy's name may lead anywhere: it is the person's own.
        let guard = Guard {
            names: Some(Arc::new(HashMap::from([(
                "localhost".to_owned(),
                vec![LOOPBACK],
            )]))),
            ..Guard::new(Some("socks5://localhost:1080"))
        };
        assert!(guard.resolve("localhost".parse().unwrap()).await.is_ok());
        assert!(guard.resolve("127.0.0.1".parse().unwrap()).await.is_err());
    }

    /// A public-style tracker still works: a name resolving to the internet (loopback stands in
    /// for it here) is asked, its answer read.
    #[tokio::test]
    async fn a_tracker_on_the_internet_is_asked() {
        let tracker = Stub::start(|_| announced()).await;
        let c = client(&[("tracker.example", LOOPBACK)], true);
        let url = format!(
            "http://tracker.example:{}/announce?info_hash=x",
            tracker.port
        );
        let r = c.get(&url).send().await.unwrap();
        assert!(r.status().is_success());
        assert_eq!(r.text().await.unwrap(), "d8:intervali60e5:peers0:e");
        assert_eq!(tracker.got().len(), 1);
    }

    /// Which redirects are followed, by address: a tracker MDM may contact, and from the
    /// router's address (UPnP) only the same address.
    #[test]
    fn redirects_follow_the_tracker_rule() {
        let u = |s: &str| url::Url::parse(s).unwrap();
        let public = u("http://tracker.example.org/announce");
        for ok in [
            "http://t2.example.org/announce",
            "https://t2.example.org:8443/a",
            "http://8.8.8.8/a",
        ] {
            assert!(may_follow(&public, &u(ok), false), "{ok}");
        }
        for bad in [
            "http://127.0.0.1/a",
            "http://localhost/a",
            "http://192.168.1.1/a",
            "http://[::1]/a",
            "http://[fd00::1]/a",
            "http://169.254.169.254/latest",
            "http://2130706433/a",
            "http://router/a",
            "http://nas.lan/a",
            "ftp://t2.example.org/a",
            "file:///etc/passwd",
        ] {
            assert!(!may_follow(&public, &u(bad), false), "{bad}");
        }
        // The router's own address: only itself.
        let router = u("http://192.168.1.1:5000/rootDesc.xml");
        assert!(may_follow(
            &router,
            &u("http://192.168.1.1:5000/other.xml"),
            false
        ));
        assert!(!may_follow(&router, &u("http://192.168.1.1:80/a"), false));
        assert!(!may_follow(&router, &u("http://192.168.1.2:5000/a"), false));
        assert!(!may_follow(&router, &u("file:///etc/passwd"), false));
    }
}

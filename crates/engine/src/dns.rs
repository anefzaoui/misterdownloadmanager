//! Name resolution for downloads: a dead address must not stall every new connection.
//!
//! hyper tries a host's addresses of one family one after another, each with an equal share of
//! the connect timeout, so a host whose first address is dead costs 15 s (of 30) per new
//! connection. When a host has several addresses, this resolver races TCP connects to them,
//! staggered as in RFC 8305, and puts the first that answers in front; the winner is remembered
//! for a while so later connections don't race again. A single address is passed through as is.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio::task::JoinSet;

/// Head start each address gets before the next one is tried too (RFC 8305: 250 ms).
const STAGGER: Duration = Duration::from_millis(250);
/// The whole race; after it the addresses keep the resolver's order.
const RACE_BUDGET: Duration = Duration::from_secs(3);
/// How long a winner is preferred without racing again.
const REMEMBER: Duration = Duration::from_secs(300);

pub(crate) struct Resolver {
    /// Fixed addresses for host names, like hosts-file entries (`Options::hosts`).
    hosts: HashMap<String, Vec<IpAddr>>,
    /// The port the race connects to: the download's (redirects usually keep the scheme).
    port: u16,
}

impl Resolver {
    pub(crate) fn new(hosts: &[(String, Vec<IpAddr>)], port: u16) -> Self {
        Resolver {
            hosts: hosts
                .iter()
                .map(|(h, ips)| (h.to_ascii_lowercase(), ips.clone()))
                .collect(),
            port,
        }
    }
}

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_ascii_lowercase();
        let fixed = self.hosts.get(&host).cloned();
        let port = self.port;
        Box::pin(async move {
            let ips: Vec<IpAddr> = match fixed {
                Some(ips) => ips,
                None => tokio::net::lookup_host((host.as_str(), port))
                    .await?
                    .map(|a| a.ip())
                    .collect(),
            };
            let ips = reachable_first(&host, ips, port).await;
            // Port 0: reqwest then uses the URL's port (a non-zero port here would win).
            let addrs: Addrs = Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

fn winners() -> &'static Mutex<HashMap<String, (IpAddr, Instant)>> {
    static WINNERS: OnceLock<Mutex<HashMap<String, (IpAddr, Instant)>>> = OnceLock::new();
    WINNERS.get_or_init(Default::default)
}

/// `ips` with an address that accepts connections first (unchanged when none answers in time).
async fn reachable_first(host: &str, mut ips: Vec<IpAddr>, port: u16) -> Vec<IpAddr> {
    if ips.len() < 2 {
        return ips;
    }
    let remembered = winners()
        .lock()
        .unwrap()
        .get(host)
        .filter(|(_, at)| at.elapsed() < REMEMBER)
        .map(|(ip, _)| *ip);
    let first = match remembered.and_then(|ip| ips.iter().position(|x| *x == ip)) {
        Some(i) => Some(i),
        None => race(&ips, port).await,
    };
    if let Some(i) = first {
        winners()
            .lock()
            .unwrap()
            .insert(host.to_owned(), (ips[i], Instant::now()));
        let ip = ips.remove(i);
        ips.insert(0, ip);
    }
    ips
}

/// Index of the first address that accepts a TCP connection. Each attempt gets a `STAGGER`
/// head start; a failed attempt starts the next one at once. The probe sockets are closed.
async fn race(ips: &[IpAddr], port: u16) -> Option<usize> {
    let deadline = tokio::time::Instant::now() + RACE_BUDGET;
    let mut attempts = JoinSet::new();
    let mut next = 0;
    loop {
        if next < ips.len() {
            let addr = SocketAddr::new(ips[next], port);
            let i = next;
            attempts
                .spawn(async move { tokio::net::TcpStream::connect(addr).await.ok().map(|_| i) });
            next += 1;
        } else if attempts.is_empty() {
            return None;
        }
        let wait = if next < ips.len() {
            STAGGER
        } else {
            deadline.saturating_duration_since(tokio::time::Instant::now())
        };
        match tokio::time::timeout(wait, attempts.join_next()).await {
            Ok(Some(Ok(Some(i)))) => return Some(i),
            // Failed (or nothing left): go on with the next address at once.
            Ok(_) => {}
            // The head start is over: add the next address to the race.
            Err(_) if next < ips.len() => {}
            Err(_) => return None,
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_listening_address_wins_over_a_dead_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // 192.0.2.1 is TEST-NET-1: never routed (silently dropped or unreachable).
        let dead: IpAddr = "192.0.2.1".parse().unwrap();
        let live: IpAddr = "127.0.0.1".parse().unwrap();
        let started = Instant::now();
        let ips = reachable_first("race.test", vec![dead, live], port).await;
        assert_eq!(ips, vec![live, dead]);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn one_address_is_not_raced() {
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let started = Instant::now();
        assert_eq!(reachable_first("single.test", vec![ip], 9).await, vec![ip]);
        assert!(started.elapsed() < Duration::from_millis(50));
    }
}

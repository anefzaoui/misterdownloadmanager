//! Learned per-host connection limits (beyond IDM, D-012): when a host refuses extra connections
//! while others to it are receiving data, it allows at least those — the most ever seen working
//! at a refusal is the cap (counting connections still connecting, or keeping the smallest
//! count, learned 1 for a host that allows 2: benchmark 2026-10-30). After two such observations the
//! cap applies — to this download and to later ones — instead of every extra connection using
//! up its retries (wasted time, and a risk of being banned). Forgotten after `DECAY`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a learned cap lasts without being seen again.
pub const DECAY: Duration = Duration::from_secs(10 * 60);
/// Observations before a cap applies (one refusal can be a hiccup).
const NEEDED: u32 = 2;

#[derive(Debug, Default)]
pub struct HostLimits {
    hosts: Mutex<HashMap<String, Learned>>,
}

#[derive(Debug, Clone, Copy)]
struct Learned {
    cap: u32,
    seen: u32,
    at: Instant,
    optimal: Option<u32>,
}

impl HostLimits {
    /// The connections `host` is known to allow, if learned.
    pub fn cap(&self, host: &str) -> Option<u32> {
        let mut hosts = self.hosts.lock().unwrap();
        match hosts.get(host) {
            Some(l) if l.at.elapsed() >= DECAY => {
                hosts.remove(host);
                None
            }
            Some(l) if l.seen >= NEEDED => Some(l.cap),
            _ => None,
        }
    }

    /// The throughput-optimal connection count for `host`, if learned (#1240).
    pub fn optimal(&self, host: &str) -> Option<u32> {
        let mut hosts = self.hosts.lock().unwrap();
        match hosts.get(host) {
            Some(l) if l.at.elapsed() >= DECAY => {
                hosts.remove(host);
                None
            }
            Some(l) => l.optimal,
            None => None,
        }
    }

    /// Record a learned throughput-optimal connection count for `host` (#1240).
    pub fn record_optimal(&self, host: &str, optimal: u32) {
        let optimal = optimal.max(1);
        let mut hosts = self.hosts.lock().unwrap();
        let l = hosts.entry(host.to_owned()).or_insert(Learned {
            cap: 0,
            seen: 0,
            at: Instant::now(),
            optimal: Some(optimal),
        });
        if l.at.elapsed() >= DECAY {
            *l = Learned {
                cap: 0,
                seen: 0,
                at: Instant::now(),
                optimal: Some(optimal),
            };
        }
        l.optimal = Some(if l.seen >= NEEDED {
            optimal.min(l.cap)
        } else {
            optimal
        });
        l.at = Instant::now();
    }

    /// The effective connection limit for `host`: bounded by hard refusal cap, learned optimal, and `wanted`.
    pub fn limit_for(&self, host: &str, wanted: u32) -> u32 {
        let mut hosts = self.hosts.lock().unwrap();
        match hosts.get(host) {
            Some(l) if l.at.elapsed() >= DECAY => {
                hosts.remove(host);
                wanted
            }
            Some(l) => {
                let mut limit = wanted;
                if l.seen >= NEEDED {
                    limit = limit.min(l.cap);
                }
                if let Some(opt) = l.optimal {
                    limit = limit.min(opt);
                }
                limit.max(1)
            }
            None => wanted,
        }
    }

    /// An extra connection to `host` was refused while `working` others were receiving data.
    /// Returns the cap once it applies.
    pub fn refused(&self, host: &str, working: u32) -> Option<u32> {
        let working = working.max(1);
        let mut hosts = self.hosts.lock().unwrap();
        let l = hosts.entry(host.to_owned()).or_insert(Learned {
            cap: working,
            seen: 0,
            at: Instant::now(),
            optimal: None,
        });
        if l.at.elapsed() >= DECAY {
            *l = Learned {
                cap: working,
                seen: 0,
                at: Instant::now(),
                optimal: None,
            };
        }
        if l.seen == 0 {
            l.cap = working;
        } else {
            l.cap = l.cap.max(working);
        }
        l.seen += 1;
        l.at = Instant::now();
        if l.seen >= NEEDED && let Some(opt) = l.optimal {
            l.optimal = Some(opt.min(l.cap));
        }
        (l.seen >= NEEDED).then_some(l.cap)
    }

    /// `working` connections to `host` are receiving data at once: a learned cap below that
    /// was too low (refusals can arrive before an accepted connection's first byte) — raise it.
    pub fn working(&self, host: &str, working: u32) {
        let mut hosts = self.hosts.lock().unwrap();
        if let Some(l) = hosts.get_mut(host)
            && working > l.cap
        {
            l.cap = working;
            if let Some(opt) = l.optimal {
                l.optimal = Some(opt.max(working));
            }
        }
    }

    /// Everything learned, for display: (host, effective_limit).
    pub fn learned(&self) -> Vec<(String, u32)> {
        let hosts = self.hosts.lock().unwrap();
        let mut v: Vec<(String, u32)> = hosts
            .iter()
            .filter(|(_, l)| l.at.elapsed() < DECAY)
            .filter_map(|(h, l)| {
                let cap = (l.seen >= NEEDED).then_some(l.cap);
                match (cap, l.optimal) {
                    (Some(c), Some(o)) => Some((h.clone(), c.min(o))),
                    (Some(c), None) => Some((h.clone(), c)),
                    (None, Some(o)) => Some((h.clone(), o)),
                    (None, None) => None,
                }
            })
            .collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_teach_the_most_that_worked() {
        let h = HostLimits::default();
        assert_eq!(h.refused("x.org", 1), None, "one refusal can be a hiccup");
        assert_eq!(h.cap("x.org"), None);
        // A refusal while only one other was receiving doesn't hide that two worked before.
        assert_eq!(h.refused("x.org", 2), Some(2));
        assert_eq!(h.refused("x.org", 1), Some(2));
        assert_eq!(h.cap("x.org"), Some(2));
        // More seen working later raises it; fewer never lowers it.
        h.working("x.org", 3);
        h.working("x.org", 1);
        assert_eq!(h.cap("x.org"), Some(3));
        h.working("y.org", 5);
        assert_eq!(h.cap("y.org"), None, "nothing refused there: no cap");
        assert_eq!(h.cap("other.org"), None);
        assert_eq!(h.learned(), vec![("x.org".to_owned(), 3)]);
    }

    #[test]
    fn throughput_optimal_limits_learning() {
        let h = HostLimits::default();
        assert_eq!(h.optimal("cdn.example.com"), None);
        assert_eq!(h.limit_for("cdn.example.com", 16), 16);

        // Learning an optimal connection count (e.g. 2 for a fast CDN)
        h.record_optimal("cdn.example.com", 2);
        assert_eq!(h.optimal("cdn.example.com"), Some(2));
        assert_eq!(h.limit_for("cdn.example.com", 16), 2);
        assert_eq!(h.limit_for("cdn.example.com", 1), 1, "bounded by wanted");

        // Optimal counts are displayed in learned list
        assert_eq!(h.learned(), vec![("cdn.example.com".to_owned(), 2)]);

        // If a refusal cap is also learned, limit_for respects the stricter of the two
        h.refused("cdn.example.com", 1);
        h.refused("cdn.example.com", 1);
        assert_eq!(h.cap("cdn.example.com"), Some(1));
        assert_eq!(h.limit_for("cdn.example.com", 16), 1);
    }
}

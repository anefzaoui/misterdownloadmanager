//! Fault injection settings. One representation for both the server default (`--faults`) and
//! per-URL overrides (query string): `rate=100000&max_conns=2&drop_after=1M`.

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Faults {
    /// Per-response speed cap, bytes/s.
    #[serde(deserialize_with = "size_opt")]
    pub rate: Option<u64>,
    /// Every Nth request is throttled to `slow_rate` (simulates one slow connection).
    pub slow_every: Option<u64>,
    #[serde(deserialize_with = "size_opt")]
    pub slow_rate: Option<u64>,
    /// Max concurrent responses per client IP; extra requests get 503.
    pub max_conns: Option<u32>,
    /// Abort the connection after sending this many body bytes.
    #[serde(deserialize_with = "size_opt")]
    pub drop_after: Option<u64>,
    /// Apply `drop_after` only to every Nth request (default: every request).
    pub drop_every: Option<u64>,
    /// Every Nth request fails with 503.
    pub fail_every: Option<u64>,
    /// The first N requests fail with 503 (a server busy when the download starts).
    pub fail_first: Option<u64>,
    /// Ignore `Range` and always send the whole file with 200.
    pub no_range: Option<bool>,
    /// Ignore `Range` on every Nth request only (a CDN edge without range support); the file
    /// and its validators stay the same.
    pub no_range_every: Option<u64>,
    /// Answer with a small web page (a login wall) instead of the file.
    pub html: Option<bool>,
    /// ETag / Last-Modified change every N requests (resume validation).
    pub etag_change_every: Option<u64>,
    /// ETag style: `weak` (W/"…"), `per-request` (a new strong ETag on every answer, same file,
    /// same Last-Modified — CDN nodes), `none`. Default: one strong ETag.
    pub etag: Option<String>,
    /// Delay before sending response headers, ms.
    pub latency_ms: Option<u64>,
}

impl Faults {
    pub fn parse(query: &str) -> Result<Self, serde_urlencoded::de::Error> {
        serde_urlencoded::from_str(query)
    }

    /// `self` with every field that `over` sets replaced.
    pub fn overridden_by(&self, over: &Faults) -> Faults {
        Faults {
            rate: over.rate.or(self.rate),
            slow_every: over.slow_every.or(self.slow_every),
            slow_rate: over.slow_rate.or(self.slow_rate),
            max_conns: over.max_conns.or(self.max_conns),
            drop_after: over.drop_after.or(self.drop_after),
            drop_every: over.drop_every.or(self.drop_every),
            fail_every: over.fail_every.or(self.fail_every),
            fail_first: over.fail_first.or(self.fail_first),
            no_range: over.no_range.or(self.no_range),
            no_range_every: over.no_range_every.or(self.no_range_every),
            html: over.html.or(self.html),
            etag_change_every: over.etag_change_every.or(self.etag_change_every),
            etag: over.etag.clone().or(self.etag.clone()),
            latency_ms: over.latency_ms.or(self.latency_ms),
        }
    }
}

/// True when request number `n` (1-based) is selected by "every Nth".
pub fn every(n: u64, nth: Option<u64>) -> bool {
    matches!(nth, Some(k) if k > 0 && n.is_multiple_of(k))
}

fn size_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let s = String::deserialize(d)?;
    crate::content::parse_size(&s)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom(format!("invalid size `{s}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_override() {
        let base = Faults::parse("rate=1M&max_conns=2").unwrap();
        assert_eq!(base.rate, Some(1 << 20));
        assert_eq!(base.max_conns, Some(2));
        let over = Faults::parse("max_conns=4&no_range=true").unwrap();
        let f = base.overridden_by(&over);
        assert_eq!(f.rate, Some(1 << 20));
        assert_eq!(f.max_conns, Some(4));
        assert_eq!(f.no_range, Some(true));
    }

    #[test]
    fn rejects_unknown_and_bad_sizes() {
        assert!(Faults::parse("speed=1").is_err());
        assert!(Faults::parse("rate=fast").is_err());
    }

    #[test]
    fn every_nth() {
        assert!(every(4, Some(2)));
        assert!(!every(3, Some(2)));
        assert!(!every(3, None));
        assert!(!every(3, Some(0)));
    }
}

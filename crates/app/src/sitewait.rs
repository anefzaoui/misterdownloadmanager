//! A site that refused a download with its bot check gets a rest, the whole site and not only
//! that file (JDownloader's "temporarily unavailable" for a host does the same).
//!
//! Without it, a queue whose files are all on one site (a DLC container's parts on MediaFire)
//! went through them as fast as they were refused: each refusal took under a second, the queue
//! started the next file of the same site at once, then retried each one every 30 s. The site's
//! bot check saw a steady stream of requests and never let up. Now the first refusal stops
//! every download of that site from starting until its rest is over: 2 minutes, then 5, 15 and
//! 30 for each refusal in a row. A file of that site finishing ends the rest.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The rests, one more step for each refusal in a row; the last one repeats.
pub const RESTS: [Duration; 4] = [
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
    Duration::from_secs(30 * 60),
];

/// The sites resting, each until when and after how many refusals in a row.
#[derive(Default)]
pub struct SiteWaits {
    sites: HashMap<String, (Instant, u32)>,
}

/// The site a download's address is on: its host, lower case, without `www.`.
pub fn site_of(url: &url::Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_owned())
}

impl SiteWaits {
    /// `site` refused a download with its bot check at `now`: it rests. A refusal while it
    /// already rests (a download that was running then) doesn't make the rest longer.
    pub fn refused(&mut self, site: &str, now: Instant) -> Duration {
        let (until, strikes) = self.sites.entry(site.to_owned()).or_insert((now, 0));
        if *until > now {
            return *until - now;
        }
        let rest = RESTS[(*strikes as usize).min(RESTS.len() - 1)];
        *strikes += 1;
        *until = now + rest;
        rest
    }

    /// A download of `site` finished: the site lets files through again.
    pub fn worked(&mut self, site: &str) {
        self.sites.remove(site);
    }

    /// Whether downloads of `site` wait at `now`.
    pub fn resting(&self, site: &str, now: Instant) -> bool {
        self.sites.get(site).is_some_and(|(until, _)| *until > now)
    }

    /// The soonest a resting site's rest is over, to look at the queues again then.
    pub fn next_end(&self, now: Instant) -> Option<Instant> {
        self.sites
            .values()
            .map(|(until, _)| *until)
            .filter(|u| *u > now)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rests_grow_and_end_when_a_file_comes_through() {
        let mut w = SiteWaits::default();
        let t = Instant::now();
        assert!(!w.resting("mediafire.com", t));
        assert_eq!(w.refused("mediafire.com", t), RESTS[0]);
        assert!(w.resting("mediafire.com", t + Duration::from_secs(60)));
        assert!(!w.resting("other.example", t));
        // Refused again while resting (another download that was running): no longer rest.
        assert_eq!(
            w.refused("mediafire.com", t + Duration::from_secs(60)),
            Duration::from_secs(60)
        );
        // After it, the next refusal rests longer, up to the last step.
        let mut at = t + RESTS[0];
        assert!(!w.resting("mediafire.com", at));
        for want in &RESTS[1..] {
            assert_eq!(w.refused("mediafire.com", at), *want);
            at += *want;
        }
        assert_eq!(w.refused("mediafire.com", at), RESTS[3]);
        assert_eq!(w.next_end(at), Some(at + RESTS[3]));
        w.worked("mediafire.com");
        assert!(!w.resting("mediafire.com", at));
        assert_eq!(w.refused("mediafire.com", at), RESTS[0]);
    }

    #[test]
    fn a_site_is_its_host_without_www() {
        let s = |u: &str| site_of(&url::Url::parse(u).unwrap());
        assert_eq!(
            s("https://www.MediaFire.com/file/abc/x/file").as_deref(),
            Some("mediafire.com")
        );
        assert_eq!(
            s("https://download850.mediafire.com/t/abc/x").as_deref(),
            Some("download850.mediafire.com")
        );
    }
}

//! Per-download speed limiter (IDM: "Maximum download speed for one file", notes/ENGINE.md §4).
//! A token bucket shared by all of a download's connections; the limit can change while the
//! download runs. Throttling the socket reads makes TCP slow the server down.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Bytes per second; 0 = unlimited. One per download, and optionally one shared by all of them
/// (the global cap, `Options::global`).
#[derive(Debug)]
pub struct Limiter {
    rate: AtomicU64,
    bucket: Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    /// Bytes that may be read right now (may go negative: debt to wait off).
    tokens: f64,
    last: Instant,
}

/// Burst allowance: up to one second's budget accumulates while no connection is reading (e.g.
/// all between requests), so short gaps don't lower the average below the limit.
const BURST_SECS: f64 = 1.0;

impl Limiter {
    pub fn new(rate: Option<u64>) -> Self {
        Limiter {
            rate: AtomicU64::new(rate.unwrap_or(0)),
            bucket: Mutex::new(Bucket {
                tokens: 0.0,
                last: Instant::now(),
            }),
        }
    }

    pub fn set(&self, rate: Option<u64>) {
        self.rate.store(rate.unwrap_or(0), Ordering::Relaxed);
    }

    pub fn get(&self) -> Option<u64> {
        Some(self.rate.load(Ordering::Relaxed)).filter(|r| *r > 0)
    }

    /// Account for `n` received bytes; returns how long the caller should wait first.
    fn take(&self, n: u64) -> Duration {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return Duration::ZERO;
        }
        let rate = rate as f64;
        let mut b = self.bucket.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(b.last).as_secs_f64();
        b.last = now;
        b.tokens = (b.tokens + elapsed * rate).min(rate * BURST_SECS);
        b.tokens -= n as f64;
        if b.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-b.tokens / rate)
        }
    }

    /// Give back budget for `n` bytes that were paid for but not kept.
    pub fn refund(&self, n: u64) {
        if self.rate.load(Ordering::Relaxed) > 0 {
            self.bucket.lock().unwrap().tokens += n as f64;
        }
    }

    /// Wait until `n` more bytes fit under the limit.
    pub async fn acquire(&self, n: u64) {
        let wait = self.take(n);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limits_average_rate() {
        let l = Limiter::new(Some(400 * 1024));
        let t = Instant::now();
        for _ in 0..20 {
            l.acquire(10 * 1024).await; // 200 KB at 400 KB/s ≈ 0.5 s
        }
        let secs = t.elapsed().as_secs_f64();
        assert!((0.4..0.75).contains(&secs), "{secs:.2}s");
    }

    #[tokio::test]
    async fn unlimited_and_changeable() {
        let l = Limiter::new(None);
        let t = Instant::now();
        l.acquire(100 << 20).await;
        assert!(t.elapsed() < Duration::from_millis(20));
        l.set(Some(1024));
        assert_eq!(l.get(), Some(1024));
        l.set(None);
        assert_eq!(l.get(), None);
    }
}

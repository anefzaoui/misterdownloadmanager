//! A shell query may wait indefinitely for Explorer. Keep one worker, never wait for it on
//! the window thread, and stop trusting an old answer if the shell stops responding.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub(super) const INTERVAL: Duration = Duration::from_secs(1);
const MAX_AGE: Duration = Duration::from_secs(2);

pub(super) struct Probe<T> {
    answer: Arc<Mutex<Option<(Instant, T)>>>,
    // Dropping this wakes an idle worker. Never join: it may be inside a hung shell call.
    _stop: mpsc::Sender<()>,
}

impl<T: Copy + Send + 'static> Probe<T> {
    pub(super) fn new(
        mut query: impl FnMut() -> T + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let answer = Arc::new(Mutex::new(None));
        let output = answer.clone();
        let (stop, stopped) = mpsc::channel();
        std::thread::Builder::new()
            .name("tray visibility".into())
            .spawn(move || {
                loop {
                    // Date the start, not the end: a slow reply is already stale on arrival.
                    let started = Instant::now();
                    let value = query();
                    if let Ok(mut answer) = output.lock() {
                        *answer = Some((started, value));
                    }
                    wake();
                    if stopped.recv_timeout(INTERVAL) != Err(mpsc::RecvTimeoutError::Timeout) {
                        break;
                    }
                }
            })?;
        Ok(Self {
            answer,
            _stop: stop,
        })
    }

    pub(super) fn latest(&self) -> Option<T> {
        // Even the short publish above must never hold up the window thread.
        let answer = self.answer.try_lock().ok()?;
        answer
            .filter(|(started, _)| started.elapsed() < MAX_AGE)
            .map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hung_shell_never_blocks_the_caller_or_drop() {
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let probe = Probe::new(
            move || {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                true
            },
            move || {
                let _ = finished.send(());
            },
        )
        .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        // The worker stays blocked throughout; these reads and Drop must all return.
        for _ in 0..10_000 {
            assert_eq!(probe.latest(), None);
        }
        drop(probe);
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn stale_or_busy_answers_cannot_hide_a_window() {
        let (stop, _stopped) = mpsc::channel();
        let probe = Probe {
            answer: Arc::new(Mutex::new(Some((Instant::now(), true)))),
            _stop: stop,
        };
        assert_eq!(probe.latest(), Some(true));
        *probe.answer.lock().unwrap() = Some((Instant::now() - MAX_AGE, true));
        assert_eq!(probe.latest(), None);
        let _publishing = probe.answer.lock().unwrap();
        assert_eq!(probe.latest(), None);
    }

    #[test]
    fn publishes_a_reply_and_wakes_the_window() {
        let (ready, received) = mpsc::channel();
        let probe = Probe::new(
            || true,
            move || {
                let _ = ready.send(());
            },
        )
        .unwrap();
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(probe.latest(), Some(true));
    }
}

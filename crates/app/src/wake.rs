//! Scheduler → "Wake the computer to run this queue" (#142): the computer, asleep, is woken
//! shortly before a queue's start time, and kept awake while that queue runs.
//!
//! The times are worked out here, the same on every system; the waking itself is the system's:
//! - Windows: a waitable timer that resumes the computer (`win32::wake_at`), owned by MDM's
//!   process, as the Scheduler's starts are (MDM has to be running, in the tray if need be).
//!   Windows' power options must allow wake timers; when they don't, the Scheduler says so.
//! - Linux: the clock chip's wake alarm (`/sys/class/rtc/rtc0/wakealarm`), which only an
//!   administrator may set unless they let users set it (the guide shows how). A systemd user
//!   timer can't wake the system (`WakeSystem=` needs the system manager), nor can `rtcwake`
//!   or a timer on an alarm clock without root or CAP_WAKE_ALARM.
//! - macOS: `pmset schedule wake` and IOKit's scheduled power events need an administrator;
//!   MDM never asks for one by itself, so the option says why.
//!
//! While a queue with the option runs, the computer is kept from sleeping (Windows: a power
//! request; Linux: logind's sleep and idle inhibitor; macOS: `caffeinate`), so a computer woken
//! for it doesn't fall asleep again before the downloads are done. Afterwards the system's own
//! sleep settings apply again, and the queue's "Turn off computer when done" can put it to
//! sleep at once.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDateTime, TimeZone, Utc};

use crate::queue::{Queue, Schedule};

/// The computer is woken this long before the start, so the network is back by then.
pub const LEAD: Duration = Duration::seconds(60);

/// Whether this computer can be woken by MDM, and if not, why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    // macOS never can (see unix::wake_support).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Ready,
    /// Windows' power options don't allow wake timers now (on battery: `battery`).
    TimersOff { battery: bool },
    /// Windows allows wake timers on mains power only, and this computer has a battery.
    NotOnBattery,
    /// Linux: the wake alarm is the administrator's (the guide shows how to allow it).
    NeedsAdmin,
    /// Linux: no clock chip with a wake alarm.
    NoAlarm,
    /// macOS: only an administrator can schedule a wake.
    MacAdmin,
}

impl Support {
    /// The checkbox can be ticked: waking is possible here, or becomes so in the system's own
    /// settings without an administrator (Windows' wake timers are a power option).
    pub fn can_tick(&self) -> bool {
        matches!(
            self,
            Support::Ready | Support::TimersOff { .. } | Support::NotOnBattery
        )
    }
}

/// When `s` next starts after `now` (local time): its "Once at" date at the start time, or
/// the next ticked weekday's. None without a start time, or when it can never come.
pub fn next_start(s: &Schedule, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let (h, m, sec) = s.start_at?;
    let time = chrono::NaiveTime::from_hms_opt(h.into(), m.into(), sec.into())?;
    if let Some((y, mo, d)) = s.once {
        let at = chrono::NaiveDate::from_ymd_opt(y, mo, d)?.and_time(time);
        return (at > now).then_some(at);
    }
    (0..=7)
        .map(|n| (now.date() + Duration::days(n)).and_time(time))
        .find(|at| *at > now && s.days[at.weekday().num_days_from_sunday() as usize])
}

/// When to wake for a start at `start`: [`LEAD`] before it, or at the start itself when that
/// is already past.
pub fn wake_for(start: NaiveDateTime, now: NaiveDateTime) -> NaiveDateTime {
    let early = start - LEAD;
    if early > now { early } else { start }
}

/// A wake the queues want: when (local time), for which start, of which queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wake {
    pub at: NaiveDateTime,
    pub start: NaiveDateTime,
    pub queue: String,
}

/// The earliest wake the queues want after `now`.
pub fn next_wake(queues: &[Queue], now: NaiveDateTime) -> Option<Wake> {
    queues
        .iter()
        .filter(|q| q.schedule.wake)
        .filter_map(|q| {
            let start = next_start(&q.schedule, now)?;
            Some(Wake {
                at: wake_for(start, now),
                start,
                queue: q.name.clone(),
            })
        })
        .min_by_key(|w| w.at)
}

/// A local time as UTC. A time the clocks skip (summer time) is taken an hour later, as the
/// clock reads then; a time they pass twice, the first time.
pub fn to_utc(local: NaiveDateTime) -> Option<DateTime<Utc>> {
    Local
        .from_local_datetime(&local)
        .earliest()
        .or_else(|| {
            Local
                .from_local_datetime(&(local + Duration::hours(1)))
                .earliest()
        })
        .map(|t| t.with_timezone(&Utc))
}

/// Whether this computer can be woken now, asked of the system at most every 2 s.
pub fn support() -> Support {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<(Instant, Support)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, s)) = &*last
        && at.elapsed() < Duration::from_secs(2)
    {
        return s.clone();
    }
    let s = test_support().unwrap_or_else(ask_support);
    *last = Some((Instant::now(), s.clone()));
    s
}

/// An off-screen test run (MDM_OFFSCREEN) may play another system's answer with MDM_TEST_WAKE
/// (`timers-off`, `battery-off`, `not-on-battery`, `needs-admin`, `no-alarm`, `mac-admin`), so
/// every state of the Scheduler's line can be shown and checked on one computer.
fn test_support() -> Option<Support> {
    if !crate::offscreen() {
        return None;
    }
    Some(match std::env::var("MDM_TEST_WAKE").ok()?.as_str() {
        "timers-off" => Support::TimersOff { battery: false },
        "battery-off" => Support::TimersOff { battery: true },
        "not-on-battery" => Support::NotOnBattery,
        "needs-admin" => Support::NeedsAdmin,
        "no-alarm" => Support::NoAlarm,
        "mac-admin" => Support::MacAdmin,
        _ => return None,
    })
}

#[cfg(windows)]
fn ask_support() -> Support {
    // 0 off, 1 on, 2 "important wake timers only" (Windows' own, not an app's).
    let w = crate::win32::wake_timers();
    let now = if w.on_battery { w.dc } else { w.ac };
    if now != 1 {
        Support::TimersOff {
            battery: w.on_battery,
        }
    } else if w.has_battery && w.dc != 1 {
        Support::NotOnBattery
    } else {
        Support::Ready
    }
}

#[cfg(unix)]
fn ask_support() -> Support {
    crate::unix::wake_support()
}

/// What the app keeps: the wake asked of the system, and whether it keeps the computer awake.
#[derive(Default)]
pub struct Waker {
    /// The wake last asked for: (UTC, the queue, the start it is for).
    wanted: Option<(DateTime<Utc>, String, NaiveDateTime)>,
    /// Whether the system has a wake of ours now.
    armed: bool,
    /// Why the last attempt failed (shown in the Scheduler), if it did.
    pub error: Option<String>,
    /// The wake is asked for again at least every minute (the system may have dropped it: a
    /// Linux alarm used up by another wake, an alarm another program set has gone off).
    checked: Option<std::time::Instant>,
    awake: bool,
    /// When keeping the computer awake was last refused, and how often in a row (asked again
    /// 5 s later, after a minute of refusals once a minute).
    refused: Option<(std::time::Instant, u32)>,
}

impl Waker {
    /// Set, move or cancel the system's wake for `queues` (called with the Scheduler's clock;
    /// it asks the system only when the wake changes, and once a minute).
    pub fn sync(&mut self, queues: &[Queue]) {
        let now = Local::now().naive_local();
        let mut want = next_wake(queues, now).and_then(|w| Some((to_utc(w.at)?, w.queue, w.start)));
        // A start keeps the wake set for it: in the minute before the start, the wake worked out
        // now would be the start itself, moving a timer that is just going off.
        if let (Some(w), Some(set)) = (&mut want, &self.wanted)
            && (&w.1, w.2) == (&set.1, set.2)
        {
            *w = set.clone();
        }
        let stale = self
            .checked
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60));
        // Once its time has come, a wake isn't set again (a Linux alarm in the past is refused).
        let passed = want.as_ref().is_some_and(|w| w.0 <= Utc::now());
        if want == self.wanted && (!stale || passed) {
            return;
        }
        self.checked = Some(std::time::Instant::now());
        let changed = want != self.wanted;
        self.wanted = want.clone();
        // Nothing to set where waking isn't possible (Linux, macOS without the rights).
        if want.is_some() && !support().can_tick() {
            if std::mem::take(&mut self.armed) {
                let _ = arm(None);
            }
            self.error = None;
            return;
        }
        if want.is_none() && !self.armed {
            self.error = None;
            return;
        }
        match arm(want.as_ref().map(|(at, q, _)| (*at, q.as_str()))) {
            Ok(()) => {
                if changed {
                    match &want {
                        Some((at, q, _)) => tracing::info!(
                            "wake: the computer wakes at {} UTC for the queue {q:?}",
                            at.format("%Y-%m-%d %H:%M:%S")
                        ),
                        None => tracing::info!("wake: no wake set"),
                    }
                }
                self.armed = want.is_some();
                self.error = None;
            }
            Err(e) => {
                if self.error.as_deref() != Some(e.as_str()) {
                    tracing::warn!("wake: could not set the wake: {e}");
                }
                self.armed = false;
                self.error = Some(e);
            }
        }
    }

    /// Keep the computer awake while `on` (a queue with the option is running).
    pub fn keep_awake(&mut self, on: bool, queue: &str) {
        if !on {
            self.refused = None;
            if std::mem::take(&mut self.awake) {
                tracing::info!("wake: no longer keeping the computer awake");
                hold_awake(None);
            }
            return;
        }
        // A refusal is asked again a little later: logind refuses while the sleep the computer
        // just woke from is still ending, which is when a woken queue starts.
        let wait = |n: u32| std::time::Duration::from_secs(if n < 12 { 5 } else { 60 });
        if self.awake || self.refused.is_some_and(|(t, n)| t.elapsed() < wait(n)) {
            return;
        }
        let reason = crate::i18n::fmt(
            // i18n: why the computer isn't going to sleep, in the system's list of programs that keep it awake; {queue} = the queue's name
            crate::i18n::t("Mister Download Manager is running the queue \"{queue}\""),
            &[("queue", crate::i18n::t(queue))],
        );
        if hold_awake(Some(&reason)) {
            tracing::info!("wake: keeping the computer awake");
            self.awake = true;
            self.refused = None;
        } else {
            let n = self.refused.map_or(0, |(_, n)| n);
            self.refused = Some((std::time::Instant::now(), n + 1));
        }
    }

    /// Cancel the wake (MDM is quitting: Linux's alarm would outlive it).
    pub fn clear(&mut self) {
        self.wanted = None;
        if std::mem::take(&mut self.armed) {
            let _ = arm(None);
        }
        if self.awake {
            self.awake = false;
            hold_awake(None);
        }
    }
}

/// Set the system's wake for `at` (UTC), or cancel it (None).
fn arm(at: Option<(DateTime<Utc>, &str)>) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::win32::wake_at(at.map(|(t, q)| (t.timestamp(), q)))
    }
    #[cfg(unix)]
    {
        crate::unix::wake_at(at.map(|(t, _)| t.timestamp()))
    }
}

/// Keep the computer awake (`reason`) or let it sleep (None); false when the system refused.
fn hold_awake(reason: Option<&str>) -> bool {
    #[cfg(windows)]
    {
        crate::win32::keep_awake(reason)
    }
    #[cfg(unix)]
    {
        crate::unix::keep_awake(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap()
    }

    fn daily(t: (u8, u8, u8)) -> Schedule {
        Schedule {
            start_at: Some(t),
            wake: true,
            ..Schedule::default()
        }
    }

    #[test]
    fn daily_starts_today_or_on_the_next_ticked_day() {
        // Friday 6 November 2026.
        let now = at(2026, 11, 6, 22, 0, 0);
        let s = daily((23, 0, 0));
        assert_eq!(next_start(&s, now), Some(at(2026, 11, 6, 23, 0, 0)));
        // Passed today: tomorrow.
        let s = daily((7, 30, 0));
        assert_eq!(next_start(&s, now), Some(at(2026, 11, 7, 7, 30, 0)));
        // Only on Mondays (1): the next Monday.
        let mut s = daily((23, 0, 0));
        s.days = [false, true, false, false, false, false, false];
        assert_eq!(next_start(&s, now), Some(at(2026, 11, 9, 23, 0, 0)));
        // Only on Fridays, already passed today: a week later.
        s.days = [false, false, false, false, false, true, false];
        assert_eq!(
            next_start(&s, at(2026, 11, 6, 23, 30, 0)),
            Some(at(2026, 11, 13, 23, 0, 0))
        );
        // No day ticked, or no start time: never.
        s.days = [false; 7];
        assert_eq!(next_start(&s, now), None);
        assert_eq!(next_start(&Schedule::default(), now), None);
    }

    #[test]
    fn once_at_a_date_until_it_has_passed() {
        let mut s = daily((6, 0, 0));
        s.once = Some((2026, 11, 8));
        assert_eq!(
            next_start(&s, at(2026, 11, 6, 22, 0, 0)),
            Some(at(2026, 11, 8, 6, 0, 0))
        );
        assert_eq!(next_start(&s, at(2026, 11, 8, 6, 0, 0)), None, "passed");
        assert_eq!(next_start(&s, at(2026, 12, 1, 0, 0, 0)), None);
    }

    #[test]
    fn the_wake_comes_a_minute_early_unless_that_has_passed() {
        let start = at(2026, 11, 6, 23, 0, 0);
        assert_eq!(
            wake_for(start, at(2026, 11, 6, 22, 0, 0)),
            at(2026, 11, 6, 22, 59, 0)
        );
        // 30 s before the start: at the start itself.
        assert_eq!(wake_for(start, at(2026, 11, 6, 22, 59, 30)), start);
    }

    #[test]
    fn the_earliest_queue_with_the_option_wins() {
        let now = at(2026, 11, 6, 12, 0, 0);
        let q = |name: &str, s: Schedule| Queue {
            schedule: s,
            ..Queue::new(name, false)
        };
        let mut off = daily((13, 0, 0));
        off.wake = false;
        let queues = vec![
            q("early but not waking", off),
            q("night", daily((23, 0, 0))),
            q("evening", daily((19, 0, 0))),
            q(
                "no start",
                Schedule {
                    wake: true,
                    ..Schedule::default()
                },
            ),
        ];
        assert_eq!(
            next_wake(&queues, now),
            Some(Wake {
                at: at(2026, 11, 6, 18, 59, 0),
                start: at(2026, 11, 6, 19, 0, 0),
                queue: "evening".to_owned()
            })
        );
        assert_eq!(next_wake(&queues[..1], now), None);
    }

    #[test]
    fn a_local_time_has_a_utc_time() {
        let local = at(2026, 11, 6, 23, 0, 0);
        let utc = to_utc(local).unwrap();
        assert_eq!(utc.with_timezone(&Local).naive_local(), local);
    }
}

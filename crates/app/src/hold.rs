//! Pop-ups that wait (#1773): while a full-screen game or app, a presentation or a full-screen
//! video is on, or the screen is locked, the windows MDM would put forward by itself (Download
//! complete, Downloads complete, a new version, the download limit's warning, an archive's
//! test or password, the browser extension's offer, a link copied to the clipboard) wait, and
//! show when the user is back, one after the other in the order they came. The downloads
//! themselves go on as usual; only their windows wait. The crash report's window, in a process
//! of its own (crashui.rs), waits the same way.
//!
//! What answers the user's own action on this computer is never held: a link clicked in the
//! browser opens its Download File Info (or Download video, Link grabber, Add torrent) at once,
//! as does every window opened from MDM's menus. Nor is the Turn off countdown: it warns that
//! the computer is about to turn off, and its Cancel must stay within reach.
//!
//! Where the system can't tell (an unknown answer, a desktop without the state, macOS), nothing
//! waits: the windows show as they did before. Options → Downloads turns it off.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Why pop-ups wait now, as the system tells. A game and presentation mode are Windows' own
/// states; macOS tells none ([`system`]).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    /// A full-screen app or video is in front (Windows: `QUNS_BUSY`; X11: the active window is
    /// full screen).
    FullScreen,
    /// A full-screen Direct3D game (`QUNS_RUNNING_D3D_FULL_SCREEN`).
    Game,
    /// Windows' presentation mode (`QUNS_PRESENTATION_MODE`).
    Presentation,
    /// The session is locked, its screen saver shows, or another user's session is in front.
    Away,
}

impl Busy {
    /// For the log.
    pub fn says(self) -> &'static str {
        match self {
            Busy::FullScreen => "a full-screen app",
            Busy::Game => "a full-screen game",
            Busy::Presentation => "presentation mode",
            Busy::Away => "the locked screen",
        }
    }
}

/// A window that can wait: what it is, unique among those open at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PopUp {
    /// Download complete, for a download (its id).
    Complete(u64),
    /// Downloads complete, for a group (its id).
    Summary(u64),
    /// The automatic update check found a new version.
    Update,
    /// Scheduler → Download limits: the limit is reached, downloads stopped.
    LimitReached,
    /// Test archive's outcome, when it found no damage.
    ArchiveTested,
    /// A damaged archive.
    ArchiveDamaged,
    /// An archive asks for its password.
    ArchivePassword,
    /// An archive needs 7-Zip.
    SevenZip,
    /// The browser extension's offer, once per start.
    Extension,
    /// A link copied to the clipboard (its number): its Download File Info.
    CopiedLink(u64),
}

/// How often the system is asked again while something waits (and at most, while nothing does).
pub const LOOK_EVERY: Duration = Duration::from_secs(1);
/// How long the user must have been back before the windows show: a game's loading screen or
/// an Alt+Tab past the desktop is not "back".
pub const SETTLE: Duration = Duration::from_secs(1);
/// The time between two windows shown in a row, so that each comes up after (above) the one
/// before it, in the order they came.
pub const STEP: Duration = Duration::from_millis(250);

/// The windows waiting, those let through, and what the system said last.
#[derive(Debug)]
pub struct Hold {
    /// Options → Downloads → "Don't show pop-ups while a full-screen app or presentation is
    /// running". Off: everything shows at once, as before.
    pub on: bool,
    busy: Option<Busy>,
    looked: Option<Instant>,
    /// Since when the system has said the user is back (none: busy, or not looked yet).
    free_since: Option<Instant>,
    /// Waiting, in the order they came.
    waiting: Vec<PopUp>,
    /// Let through, and still asked for (open).
    shown: HashSet<PopUp>,
    /// Asked for in this pass (the others are gone: closed, or their download deleted).
    asked: HashSet<PopUp>,
    /// When the last waiting window was let through.
    stepped: Option<Instant>,
    /// What the last log line said: the windows began to wait (and why).
    told: Option<Busy>,
}

impl Default for Hold {
    fn default() -> Self {
        Hold {
            on: true,
            busy: None,
            looked: None,
            free_since: None,
            waiting: Vec::new(),
            shown: HashSet::new(),
            asked: HashSet::new(),
            stepped: None,
            told: None,
        }
    }
}

impl Hold {
    /// Waiting while `on` (the setting).
    pub fn new(on: bool) -> Self {
        Hold {
            on,
            ..Default::default()
        }
    }

    /// The start of a pass that may show pop-ups: the system is asked (`ask`, at most every
    /// [`LOOK_EVERY`]) why they should wait, if they should.
    pub fn begin(&mut self, now: Instant, ask: impl FnOnce() -> Option<Busy>) {
        self.asked.clear();
        if !self.on {
            self.busy = None;
            self.free_since = None;
            return;
        }
        if self
            .looked
            .is_none_or(|t| now.saturating_duration_since(t) >= LOOK_EVERY)
        {
            self.looked = Some(now);
            let busy = ask();
            if busy.is_some() {
                self.free_since = None;
            } else if self.busy.is_some() || self.free_since.is_none() {
                self.free_since = Some(now);
            }
            self.busy = busy;
        }
    }

    /// Whether the window `p` may show now. Asked in every pass while it would be open: one no
    /// longer asked for is gone. A new one waits while the user is busy, or behind the ones
    /// already waiting; those are let through one by one ([`STEP`]) once the user has been back
    /// for [`SETTLE`]. One already shown stays.
    pub fn lets(&mut self, p: PopUp, now: Instant) -> bool {
        self.asked.insert(p);
        if !self.on || self.shown.contains(&p) {
            self.waiting.retain(|w| *w != p);
            self.shown.insert(p);
            return true;
        }
        if !self.waiting.contains(&p) {
            if self.busy.is_none() && self.waiting.is_empty() {
                self.shown.insert(p);
                return true;
            }
            self.waiting.push(p);
            if let Some(why) = self.busy
                && self.told != Some(why)
            {
                self.told = Some(why);
                tracing::info!("pop-ups wait: {} is on", why.says());
            }
            return false;
        }
        if self.waiting.first() != Some(&p) || !self.back(now) {
            return false;
        }
        if self
            .stepped
            .is_some_and(|t| now.saturating_duration_since(t) < STEP)
        {
            return false;
        }
        if let Some(why) = self.told.take() {
            tracing::info!(
                "pop-ups shown: {} is over, {} waited",
                why.says(),
                self.waiting.len()
            );
        }
        self.waiting.remove(0);
        self.stepped = Some(now);
        self.shown.insert(p);
        true
    }

    /// The end of the pass: windows no longer asked for leave both lists.
    pub fn end(&mut self) {
        let asked = &self.asked;
        self.shown.retain(|p| asked.contains(p));
        self.waiting.retain(|p| asked.contains(p));
    }

    /// The user is busy now (as last asked): new pop-ups wait.
    pub fn busy(&self) -> Option<Busy> {
        if self.on { self.busy } else { None }
    }

    /// Whether `p` is waiting.
    pub fn is_waiting(&self, p: &PopUp) -> bool {
        self.waiting.contains(p)
    }

    /// How many windows wait (the tray icon's tip says so).
    pub fn waiting(&self) -> usize {
        if self.on { self.waiting.len() } else { 0 }
    }

    /// Back long enough for the waiting windows to show.
    fn back(&self, now: Instant) -> bool {
        self.busy.is_none()
            && self
                .free_since
                .is_some_and(|t| now.saturating_duration_since(t) >= SETTLE)
    }

    /// When the next pass that may show pop-ups is due, while windows wait (none: nothing
    /// waits, the passes come as they do): the next look at the system while the user is busy,
    /// the next window's turn once back.
    pub fn next_pass(&self, now: Instant) -> Option<Duration> {
        if self.waiting() == 0 {
            return None;
        }
        let after = |t: Option<Instant>, wait: Duration| {
            t.map_or(Duration::ZERO, |t| {
                wait.saturating_sub(now.saturating_duration_since(t))
            })
        };
        Some(if self.busy.is_some() {
            after(self.looked, LOOK_EVERY)
        } else if !self.back(now) {
            after(self.free_since, SETTLE).max(after(self.looked, LOOK_EVERY))
        } else {
            after(self.stepped, STEP)
        })
    }
}

/// Why pop-ups should wait now, as the system tells (none: they show; also where it can't tell).
/// Off-screen test runs never wait: their windows are drawn for screenshots and tests, whatever
/// the computer they run on shows.
pub fn system() -> Option<Busy> {
    if crate::offscreen() {
        return None;
    }
    #[cfg(windows)]
    return crate::win32::pop_ups_wait();
    #[cfg(target_os = "linux")]
    return crate::unix::pop_ups_wait();
    // macOS: no public call tells whether another program is full screen or presenting. It needs
    // none for those: a full-screen app has a Space of its own, and MDM's windows (none of which
    // joins every Space) open on the desktop's, never over it; the locked screen covers every
    // window. So they already wait there, and are where they were when the user goes back.
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// One pass: the system says `busy`, the windows `asked` ask to show; those let through.
    fn pass(h: &mut Hold, now: Instant, busy: Option<Busy>, asked: &[PopUp]) -> Vec<PopUp> {
        h.begin(now, || busy);
        let out = asked.iter().copied().filter(|p| h.lets(*p, now)).collect();
        h.end();
        out
    }

    #[test]
    fn nothing_waits_while_the_user_is_free_or_the_system_cannot_tell() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let one = PopUp::Complete(1);
        assert_eq!(pass(&mut h, t0, None, &[one]), [one]);
        assert_eq!(h.waiting(), 0);
        assert_eq!(h.next_pass(t0), None);
    }

    #[test]
    fn a_window_waits_while_busy_and_shows_once_back_for_a_moment() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let one = PopUp::Complete(1);
        let game = Some(Busy::Game);
        assert!(pass(&mut h, t0, game, &[one]).is_empty());
        assert!(h.is_waiting(&one));
        assert_eq!(h.next_pass(t0), Some(LOOK_EVERY));
        // Still in the game a second later.
        assert!(pass(&mut h, at(t0, 1000), game, &[one]).is_empty());
        // Back: not at once (a loading screen, an Alt+Tab), after SETTLE.
        assert!(pass(&mut h, at(t0, 2000), None, &[one]).is_empty());
        assert_eq!(h.next_pass(at(t0, 2000)), Some(SETTLE));
        assert_eq!(pass(&mut h, at(t0, 3000), None, &[one]), [one]);
        // Shown, it stays shown, even if a game starts again.
        assert_eq!(pass(&mut h, at(t0, 4000), game, &[one]), [one]);
        assert_eq!(h.waiting(), 0);
    }

    #[test]
    fn waiting_windows_show_one_by_one_in_the_order_they_came() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let (a, b, c) = (PopUp::Update, PopUp::Complete(7), PopUp::Summary(3));
        let busy = Some(Busy::FullScreen);
        // They come in this order, a pass apart; each pass asks them in code order (c, b, a).
        pass(&mut h, t0, busy, &[a]);
        pass(&mut h, at(t0, 100), busy, &[b, a]);
        pass(&mut h, at(t0, 200), busy, &[c, b, a]);
        assert_eq!(h.waiting(), 3);
        let all = [c, b, a];
        pass(&mut h, at(t0, 1000), None, &all);
        let mut shown = Vec::new();
        let mut now = 2000;
        while shown.len() < 3 && now < 5000 {
            // Each window once, as it comes up (one shown stays let through).
            for p in pass(&mut h, at(t0, now), None, &all) {
                if !shown.contains(&p) {
                    shown.push(p);
                }
            }
            now += 50;
        }
        assert_eq!(shown, [a, b, c]);
    }

    #[test]
    fn a_new_window_waits_behind_those_already_waiting() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let (a, b) = (PopUp::Complete(1), PopUp::Complete(2));
        pass(&mut h, t0, Some(Busy::Away), &[a]);
        pass(&mut h, at(t0, 1000), None, &[a]);
        // Back, but before SETTLE: b comes and goes behind a.
        assert!(pass(&mut h, at(t0, 1500), None, &[b, a]).is_empty());
        assert_eq!(pass(&mut h, at(t0, 2000), None, &[b, a]), [a]);
        // a stays up; b's turn comes a STEP later.
        assert_eq!(pass(&mut h, at(t0, 2100), None, &[b, a]), [a]);
        assert_eq!(pass(&mut h, at(t0, 2300), None, &[b, a]), [b, a]);
    }

    #[test]
    fn a_window_gone_while_it_waited_leaves_the_line() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let (a, b) = (PopUp::Complete(1), PopUp::Complete(2));
        pass(&mut h, t0, Some(Busy::Game), &[a, b]);
        // a's download was deleted meanwhile: only b is asked for.
        pass(&mut h, at(t0, 500), Some(Busy::Game), &[b]);
        assert!(!h.is_waiting(&a));
        pass(&mut h, at(t0, 1000), None, &[b]);
        assert_eq!(pass(&mut h, at(t0, 2000), None, &[b]), [b]);
    }

    #[test]
    fn turned_off_everything_shows_at_once_as_before() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let (a, b) = (PopUp::Complete(1), PopUp::Update);
        pass(&mut h, t0, Some(Busy::Presentation), &[a]);
        assert_eq!(h.waiting(), 1);
        h.on = false;
        assert_eq!(
            pass(&mut h, at(t0, 10), Some(Busy::Presentation), &[a, b]),
            [a, b]
        );
        assert_eq!(h.busy(), None);
        // Nothing left in line for when it is turned on again.
        h.on = true;
        assert_eq!(h.waiting(), 0);
    }

    #[test]
    fn the_system_is_asked_at_most_every_second() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        let mut asked = 0;
        for ms in (0..2000).step_by(100) {
            h.begin(at(t0, ms), || {
                asked += 1;
                None
            });
        }
        assert_eq!(asked, 2);
    }
}

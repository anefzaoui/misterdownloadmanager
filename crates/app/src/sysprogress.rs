//! The progress the system shows on MDM's own button or icon: the taskbar button on Windows
//! (win32.rs, ITaskbarList3), the launcher on Linux docks and MDM's icon in the macOS Dock
//! (unix.rs `dock_progress`, #1777). One cache for all of them: the value is sent in thousandths,
//! only when it changes, and a send that failed is sent again on the next tick.

/// The indicator's resolution: thousandths, the steps the Windows taskbar was always sent.
pub const STEPS: u32 = 1000;

/// What one indicator last showed. `None` means nothing sent yet (or forgotten: the button was
/// recreated, the dock restarted), so the next value always goes out, a cleared one included.
#[derive(Default)]
pub struct Shown {
    shown: Option<Option<u32>>,
}

impl Shown {
    pub const fn new() -> Self {
        Self { shown: None }
    }

    /// The system lost what it showed: send the next value again, even if unchanged. Windows'
    /// taskbar buttons are recreated; a dock gets its value again on its own (unix.rs).
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub fn forget(&mut self) {
        self.shown = None;
    }

    /// `progress` (0 to 1, `None` to clear) in [`STEPS`], handed to `send` only when it differs
    /// from what was shown; `send` says whether it got through, and a failed one is retried.
    pub fn update(&mut self, progress: Option<f64>, send: impl FnOnce(Option<u32>) -> bool) {
        let value = progress.map(steps);
        if self.shown != Some(value) && send(value) {
            self.shown = Some(value);
        }
    }
}

/// A fraction in [`STEPS`], clamped (a size that shrank must not draw past the end).
pub fn steps(fraction: f64) -> u32 {
    (fraction.clamp(0.0, 1.0) * f64::from(STEPS)).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changes_are_sent_and_a_failed_send_is_retried() {
        let mut shown = Shown::new();
        let mut sent = Vec::new();
        // The first value goes out, a cleared one too (an indicator left by a crash).
        shown.update(None, |v| {
            sent.push(v);
            true
        });
        shown.update(None, |v| {
            sent.push(v);
            true
        });
        shown.update(Some(0.25), |v| {
            sent.push(v);
            true
        });
        // The same thousandth again: nothing to send.
        shown.update(Some(0.2501), |v| {
            sent.push(v);
            true
        });
        // A failed send is tried again next time.
        shown.update(Some(0.5), |_| false);
        shown.update(Some(0.5), |v| {
            sent.push(v);
            true
        });
        // Forgotten (the dock restarted): the same value goes out again.
        shown.forget();
        shown.update(Some(0.5), |v| {
            sent.push(v);
            true
        });
        shown.update(None, |v| {
            sent.push(v);
            true
        });
        assert_eq!(
            sent,
            [None, Some(250), Some(500), Some(500), None],
            "values sent"
        );
    }

    #[test]
    fn fractions_are_clamped_to_the_bar() {
        assert_eq!(steps(-0.5), 0);
        assert_eq!(steps(0.0), 0);
        assert_eq!(steps(0.4996), 500);
        assert_eq!(steps(1.0), STEPS);
        assert_eq!(steps(1.7), STEPS);
    }
}

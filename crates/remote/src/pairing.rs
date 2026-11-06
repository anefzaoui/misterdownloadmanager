//! Pairing codes. Options → Remote → Pair a device shows an 8-digit code, valid for ten minutes
//! and for one device; closing that window ends it. A code takes five wrong tries, then it is
//! void (a new one must be shown), and tries come one a second at most, whoever sends them: a
//! guesser gets five chances in a hundred million per code shown.

use std::time::{Duration, Instant};

/// How long a code shown stays valid.
pub const VALID: Duration = Duration::from_secs(10 * 60);
/// Wrong tries a code takes before it is void.
pub const TRIES: u8 = 5;
/// The least time between two tries.
pub const PACE: Duration = Duration::from_secs(1);

struct Active {
    code: String,
    until: Instant,
    left: u8,
}

#[derive(Default)]
pub struct Pairing {
    active: Option<Active>,
    last_try: Option<Instant>,
    /// The device the code shown paired (the window says so and closes).
    paired: Option<String>,
}

/// A try's outcome.
#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    Right,
    /// Wrong; `left` tries remain for this code (0: it is void now).
    Wrong {
        left: u8,
    },
    /// No code is shown (none asked for, timed out, or used up).
    NoCode,
    /// Too soon after the last try.
    TooSoon,
}

impl Pairing {
    /// Show a new code (the one before is void).
    pub fn start(&mut self) -> String {
        let n = u64::from_le_bytes(crate::devices::random(8).try_into().unwrap_or([0; 8]))
            % 100_000_000;
        let code = format!("{n:08}");
        self.active = Some(Active {
            code: code.clone(),
            until: Instant::now() + VALID,
            left: TRIES,
        });
        self.paired = None;
        code
    }

    /// The pairing window closed: its code is void.
    pub fn stop(&mut self) {
        self.active = None;
        self.paired = None;
    }

    /// The code shown now and the time it has left, if any.
    pub fn shown(&self) -> Option<(String, Duration)> {
        let a = self.active.as_ref()?;
        let left = a.until.checked_duration_since(Instant::now())?;
        Some((a.code.clone(), left))
    }

    /// The device the code shown paired, once it has.
    pub fn paired(&self) -> Option<String> {
        self.paired.clone()
    }

    /// A device tries `given` (spaces and dashes ignored).
    pub fn check(&mut self, given: &str) -> Check {
        let now = Instant::now();
        if self.last_try.is_some_and(|t| now.duration_since(t) < PACE) {
            return Check::TooSoon;
        }
        self.last_try = Some(now);
        let Some(a) = self.active.as_mut().filter(|a| a.until > now && a.left > 0) else {
            self.active = None;
            return Check::NoCode;
        };
        let given: String = given.chars().filter(char::is_ascii_digit).collect();
        let right = given.len() == a.code.len()
            && given
                .bytes()
                .zip(a.code.bytes())
                .fold(0u8, |acc, (x, y)| acc | (x ^ y))
                == 0;
        if right {
            // One device per code.
            self.active = None;
            return Check::Right;
        }
        a.left -= 1;
        let left = a.left;
        if left == 0 {
            self.active = None;
        }
        Check::Wrong { left }
    }

    /// The code shown paired `name`.
    pub fn done(&mut self, name: &str) {
        self.paired = Some(name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait() {
        std::thread::sleep(PACE + Duration::from_millis(20));
    }

    #[test]
    fn a_code_pairs_once() {
        let mut p = Pairing::default();
        assert_eq!(p.check("12345678"), Check::NoCode, "none shown");
        let code = p.start();
        assert_eq!(code.len(), 8);
        wait();
        let spaced = format!("{} {}", &code[..4], &code[4..]);
        assert_eq!(p.check(&spaced), Check::Right);
        wait();
        assert_eq!(p.check(&code), Check::NoCode, "used");
    }

    #[test]
    fn wrong_tries_void_the_code_and_are_paced() {
        let mut p = Pairing::default();
        let code = p.start();
        let wrong = if code == "00000000" {
            "11111111"
        } else {
            "00000000"
        };
        assert_eq!(p.check(wrong), Check::Wrong { left: TRIES - 1 });
        assert_eq!(p.check(&code), Check::TooSoon, "one try a second");
        for left in (0..TRIES - 1).rev() {
            wait();
            assert_eq!(p.check(wrong), Check::Wrong { left });
        }
        wait();
        assert_eq!(p.check(&code), Check::NoCode, "void after five wrong tries");
    }
}

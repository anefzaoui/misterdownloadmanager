//! The first start's setup in one window (#1839): the interface language, the video helpers
//! (yt-dlp, ffmpeg and a JavaScript runtime) and the browser extension, as the steps of one
//! window with Back, Next and the steps on top, instead of a window each, one after the other.
//! A step this start doesn't need is left out (the installer chose the language; the helpers are
//! there), every step can be passed with Next without doing anything, and nothing is downloaded
//! until one of the helpers' Install buttons is pressed. Closing the window or Finish ends the
//! setup for good: the app marks the language and the helpers asked (settings.rs), as the
//! separate windows did. The extension's own window still offers it at later starts, until a
//! browser has it (extension.rs).
//!
//! Choosing a language lays the window out in it at once (right to left for Arabic, Hebrew,
//! Persian and Urdu), and every other window with it: the app applies it as View → Language does.

use egui::vec2;

use crate::dialogs::Outcome;
use crate::winui::{self, ButtonKind, r};

/// The window's width (the helpers' and the extension's windows were as wide).
const W: f32 = 520.0;
/// The banner's planned height (winui::banner).
const BANNER: f32 = 84.0;
/// The pages' planned height: the tallest page grows the window ([`crate::dialogs::grow_px`]).
const PAGE: f32 = 350.0;
/// The buttons' row under the pages, with its separator.
const FOOTER: f32 = 50.0;

/// A step of the setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Language,
    Helpers,
    Extension,
}

impl Step {
    /// The step's name over its page (translated where shown).
    fn name(self) -> &'static str {
        match self {
            // i18n: window title (the interface language)
            Step::Language => crate::i18n::mark("Language"),
            // i18n: a step of the first start's setup window: yt-dlp, ffmpeg and Node.js, the programs that download videos
            Step::Helpers => crate::i18n::mark("Video helpers"),
            // i18n: a step of the first start's setup window: the extension that hands the browser's downloads to MDM
            Step::Extension => crate::i18n::mark("Browser extension"),
        }
    }

    /// The step `/show setup:<name>` opens at (screenshots, tests).
    pub fn named(name: &str) -> Option<Step> {
        match name {
            "language" => Some(Step::Language),
            "helpers" => Some(Step::Helpers),
            "extension" => Some(Step::Extension),
            _ => None,
        }
    }
}

/// What this start still needs, as the app finds it: the language not chosen yet (no
/// installer said), the helpers never offered and one missing, the extension worth offering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Need {
    pub language: bool,
    pub helpers: bool,
    pub extension: bool,
}

impl Need {
    /// Every step (`/show setup`).
    pub const ALL: Need = Need {
        language: true,
        helpers: true,
        extension: true,
    };
}

/// Whether the setup is due at this start: the language or the helpers were never asked (a
/// first start, or one that ended before the setup did). A setup finished or closed once never
/// comes back; the extension alone is offered by its own window (extension.rs).
pub fn due(language_asked: bool, helpers_asked: bool) -> bool {
    !language_asked || !helpers_asked
}

/// The steps the setup goes through and the one shown: the window's logic, apart from drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flow {
    steps: Vec<Step>,
    at: usize,
}

impl Flow {
    /// The steps `need` asks for, in their order, at the first; None when there is none (no
    /// window then).
    pub fn new(need: Need) -> Option<Flow> {
        let steps: Vec<Step> = [
            (need.language, Step::Language),
            (need.helpers, Step::Helpers),
            (need.extension, Step::Extension),
        ]
        .into_iter()
        .filter_map(|(wanted, step)| wanted.then_some(step))
        .collect();
        (!steps.is_empty()).then_some(Flow { steps, at: 0 })
    }

    /// The step shown.
    pub fn step(&self) -> Step {
        self.steps[self.at]
    }

    /// The steps, in order.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The step shown's place, from 0.
    pub fn at(&self) -> usize {
        self.at
    }

    /// Whether Back has a step to go back to.
    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    /// Whether the step shown is the last: its Next is Finish.
    pub fn is_last(&self) -> bool {
        self.at + 1 == self.steps.len()
    }

    /// Back to the step before (none before the first).
    pub fn back(&mut self) {
        self.at = self.at.saturating_sub(1);
    }

    /// On to the next step; true when the step shown was the last (Finish: the setup is done).
    pub fn next(&mut self) -> bool {
        if self.is_last() {
            return true;
        }
        self.at += 1;
        false
    }

    /// Shown at `step`, when it is one of its steps.
    pub fn go_to(&mut self, step: Step) {
        if let Some(at) = self.steps.iter().position(|s| *s == step) {
            self.at = at;
        }
    }
}

/// The setup's window.
pub struct Setup {
    flow: Flow,
    /// The language list (made only when the step is there).
    picker: Option<crate::langpick::Picker>,
    /// The language in use, as the setup last set it (the one the list preselects at first:
    /// the system's, which is the one in use then).
    shown: Option<&'static str>,
    /// A language the person chose in this frame, for the app to apply ([`Setup::take_language`]).
    chosen: Option<Option<&'static str>>,
    helpers: Option<crate::helpers::Offer>,
    extension: Option<crate::extension::Extension>,
    /// Every script's font is loaded for the language list: let go once it is gone.
    all_fonts: bool,
}

impl Setup {
    /// The setup for `need`; None when it needs no step.
    pub fn new(need: Need) -> Option<Setup> {
        let flow = Flow::new(need)?;
        let picker = need
            .language
            .then(|| crate::langpick::Picker::new(true, None));
        Some(Setup {
            shown: picker.as_ref().and_then(|p| p.selected()),
            picker,
            chosen: None,
            helpers: need.helpers.then(crate::helpers::Offer::new),
            extension: need.extension.then(crate::extension::Extension::new),
            flow,
            all_fonts: false,
        })
    }

    /// Shown at `step` (`/show setup:helpers`).
    pub fn at(mut self, step: Step) -> Setup {
        self.flow.go_to(step);
        self
    }

    /// The language chosen since the last call, for the app to apply: `Some(code)`.
    pub fn take_language(&mut self) -> Option<Option<&'static str>> {
        self.chosen.take()
    }

    /// Open while shown; Done on Finish, Closed when closed before (Close, Escape, the window's
    /// ×). Either way the setup is over.
    pub fn show(&mut self, ctx: &egui::Context, rt: &tokio::runtime::Handle) -> Outcome<()> {
        let step = self.flow.step();
        let banner = crate::dialogs::grow_extra(ctx, "setup-banner");
        let page = PAGE + crate::dialogs::grow_extra(ctx, "setup-page");
        let h = BANNER + banner + winui::STEPS_H + page + FOOTER;
        let out = crate::dialogs::window_of_main(
            ctx,
            "setup",
            // i18n: title of the first start's setup window (the language, the video helpers, the browser extension)
            "Welcome to Mister Download Manager",
            [W, h],
            false,
            winui::BG(),
            |ui, o| self.body(ui, o, page, rt),
        );
        if let Some(ext) = &mut self.extension {
            ext.steps_window(ctx);
        }
        // The language list let every script's font load: back to the interface's alone once it
        // is gone.
        let listing = self.flow.step() == Step::Language && matches!(out, Outcome::Open);
        if self.all_fonts && !listing {
            crate::theme::interface_fonts(ctx);
        }
        self.all_fonts = listing;
        // A new step: the window's text boxes and focus start afresh on it.
        if self.flow.step() != step {
            ctx.request_repaint();
        }
        out
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        o: egui::Pos2,
        page: f32,
        rt: &tokio::runtime::Handle,
    ) -> Outcome<()> {
        let p = ui.painter().clone();
        let step = self.flow.step();
        let (headline, subline) = match step {
            Step::Language => (
                crate::i18n::t("Choose a language"),
                // i18n: the first start's setup window, under "Choose a language"
                crate::i18n::t(
                    "Mister Download Manager shows every window in it. View → Language changes it later.",
                ),
            ),
            Step::Helpers => match &self.helpers {
                Some(h) => (h.headline(), h.subline()),
                None => ("", ""),
            },
            Step::Extension => (crate::extension::headline(), crate::extension::subline()),
        };
        let extra = winui::banner(ui, &p, o, W, "setup-banner", headline, subline);
        let top = BANNER + extra;
        let names: Vec<&str> = self
            .flow
            .steps()
            .iter()
            .map(|s| crate::i18n::t(s.name()))
            .collect();
        let step_of = crate::i18n::fmt(
            // i18n: what a screen reader says of the first start's setup window's steps; {n} = this step's number, {total} = how many, {name} = its name (Language, Video helpers, Browser extension)
            crate::i18n::t("Step {n} of {total}: {name}"),
            &[
                ("n", &crate::i18n::integer(self.flow.at() as u64 + 1)),
                ("total", &crate::i18n::integer(names.len() as u64)),
                ("name", names[self.flow.at()]),
            ],
        );
        winui::steps(ui, &p, o, W, top, &names, self.flow.at(), &step_of);
        let page_o = o + vec2(0.0, top + winui::STEPS_H);

        // The page.
        let mut next = false;
        let need = match step {
            Step::Language => {
                let picker = self
                    .picker
                    .get_or_insert_with(|| crate::langpick::Picker::new(true, None));
                let picked = picker.list(ui, page_o, [18.0, 6.0, W - 36.0, page - 18.0]);
                // A row the person picked: the window, and every other, in that language now.
                if picked.moved && picker.selected() != self.shown {
                    self.shown = picker.selected();
                    self.chosen = Some(self.shown);
                }
                next |= picked.chosen && !picked.none_match;
                PAGE
            }
            Step::Helpers => match &mut self.helpers {
                Some(offer) => offer.page(ui, page_o, W, rt),
                None => PAGE,
            },
            Step::Extension => match &mut self.extension {
                Some(ext) => ext.page(ui, page_o, W),
                None => PAGE,
            },
        };
        crate::dialogs::grow_px(ui.ctx(), "setup-page", (need - PAGE).max(0.0));

        // Back, Next (Finish on the last step) and Close, as a wizard's (IDM's own Site Grabber
        // has < Back, Next > and Close), as wide as their labels in this language (#192).
        let y = top + winui::STEPS_H + page;
        winui::etched_h(&p, o.x + 16.0, o.y + y + 4.0, W - 32.0);
        let y = y + 15.0;
        let last = self.flow.is_last();
        let forward = if last { "Finish" } else { "Next" };
        let mut row: Vec<(&str, f32)> = vec![("Back", 80.0), (forward, 80.0)];
        if !last {
            row.push(("Close", 80.0));
        }
        let placed = winui::buttons_to_right(&p, W - 18.0, 8.0, &row, 160.0);
        // Close stands apart from the two that move between steps.
        let shift = if last { 0.0 } else { 10.0 };
        let back = winui::button(
            ui,
            r(o, placed[0].0 - shift, y, placed[0].1, 25.0),
            // i18n: button of the first start's setup window: the step before
            "Back",
            if self.flow.can_back() {
                ButtonKind::Normal
            } else {
                ButtonKind::Disabled
            },
            false,
            None,
        );
        let forward_b = winui::button(
            ui,
            r(o, placed[1].0 - shift, y, placed[1].1, 25.0),
            if last {
                // i18n: button of the first start's setup window, on its last step: the setup is done
                "Finish"
            } else {
                // i18n: button of the first start's setup window: the next step
                "Next"
            },
            ButtonKind::Default,
            false,
            None,
        );
        let close = (!last).then(|| {
            winui::button(
                ui,
                r(o, placed[2].0, y, placed[2].1, 25.0),
                "Close",
                ButtonKind::Normal,
                false,
                None,
            )
        });
        // The window opens with the focus on Next where no box has it (the language's search box
        // takes it there): a screen reader says it, not just the title (#1407).
        if ui.memory(|m| m.focused().is_none()) && step != Step::Language {
            forward_b.request_focus();
        }
        next |= forward_b.clicked() || winui::enter_pressed(ui);
        if back.clicked() && self.flow.can_back() {
            self.flow.back();
            ui.memory_mut(|m| m.surrender_focus(forward_b.id));
            return Outcome::Open;
        }
        if close.is_some_and(|c| c.clicked()) || winui::escape_pressed(ui) {
            return Outcome::Closed;
        }
        if next {
            // Leaving the language: the one selected is the one used (a search may have selected
            // a row the person didn't click).
            if step == Step::Language
                && let Some(picker) = &self.picker
                && picker.selected() != self.shown
            {
                self.shown = picker.selected();
                self.chosen = Some(self.shown);
            }
            if self.flow.next() {
                return Outcome::Done(());
            }
            ui.memory_mut(|m| m.surrender_focus(forward_b.id));
        }
        Outcome::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A first start with nothing chosen yet: the three steps in order; Next goes through them,
    /// Finish on the last ends the setup, and nothing in the flow downloads or applies anything.
    #[test]
    fn next_goes_through_every_step_then_finishes() {
        let mut flow = Flow::new(Need::ALL).unwrap();
        assert_eq!(
            flow.steps(),
            [Step::Language, Step::Helpers, Step::Extension]
        );
        assert_eq!(flow.step(), Step::Language);
        assert!(!flow.can_back() && !flow.is_last());
        assert!(!flow.next());
        assert_eq!(flow.step(), Step::Helpers);
        assert!(!flow.next());
        assert_eq!(flow.step(), Step::Extension);
        assert!(flow.is_last());
        assert!(flow.next(), "Finish on the last step ends the setup");
    }

    /// Back returns to the step before, never before the first; Next after it goes on again.
    #[test]
    fn back_returns_to_the_step_before() {
        let mut flow = Flow::new(Need::ALL).unwrap();
        flow.back();
        assert_eq!(flow.step(), Step::Language, "no step before the first");
        flow.next();
        flow.next();
        assert_eq!(flow.step(), Step::Extension);
        flow.back();
        assert_eq!(flow.step(), Step::Helpers);
        assert!(flow.can_back());
        flow.back();
        assert_eq!(flow.step(), Step::Language);
        assert!(!flow.can_back());
        assert!(!flow.next());
        assert_eq!(flow.step(), Step::Helpers);
    }

    /// Every step can be passed: Next alone, from the first to Finish, takes as many presses as
    /// there are steps, whatever was needed.
    #[test]
    fn every_step_can_be_skipped() {
        for language in [false, true] {
            for helpers in [false, true] {
                for extension in [false, true] {
                    let need = Need {
                        language,
                        helpers,
                        extension,
                    };
                    let Some(mut flow) = Flow::new(need) else {
                        continue;
                    };
                    let n = flow.steps().len();
                    let presses = (1..).find(|_| flow.next()).unwrap();
                    assert_eq!(presses, n, "{need:?}");
                }
            }
        }
    }

    /// Only what this start needs is a step: the installer chose the language (Windows), or the
    /// helpers are there already; the order stays.
    #[test]
    fn only_the_steps_needed_are_shown() {
        let flow = Flow::new(Need {
            language: false,
            ..Need::ALL
        })
        .unwrap();
        assert_eq!(flow.steps(), [Step::Helpers, Step::Extension]);
        let flow = Flow::new(Need {
            helpers: false,
            ..Need::ALL
        })
        .unwrap();
        assert_eq!(flow.steps(), [Step::Language, Step::Extension]);
        let flow = Flow::new(Need {
            language: true,
            ..Need::default()
        })
        .unwrap();
        assert_eq!(flow.steps(), [Step::Language]);
        // One step: no Back, and its Next is Finish.
        assert!(!flow.can_back() && flow.is_last());
        // Nothing needed: no window at all.
        assert_eq!(Flow::new(Need::default()), None);
    }

    /// A setup finished or closed once (both asked) never comes back; one that a start ended
    /// before it did (the language chosen, the helpers not yet offered) comes back for the rest.
    #[test]
    fn a_finished_setup_never_comes_back() {
        assert!(!due(true, true));
        assert!(due(false, false));
        assert!(due(true, false));
        assert!(due(false, true));
    }

    /// `/show setup:<step>` opens at that step when it is one of the setup's; another name or a
    /// step left out keeps the first.
    #[test]
    fn a_setup_opens_at_a_named_step() {
        let mut flow = Flow::new(Need::ALL).unwrap();
        flow.go_to(Step::named("extension").unwrap());
        assert_eq!(flow.step(), Step::Extension);
        assert_eq!(Step::named("nothing"), None);
        let mut flow = Flow::new(Need {
            helpers: false,
            ..Need::ALL
        })
        .unwrap();
        flow.go_to(Step::Helpers);
        assert_eq!(flow.step(), Step::Language);
    }
}

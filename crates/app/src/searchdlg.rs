//! The Torrent search window (beyond IDM): type what you are after, the providers answer, and what
//! they found is a list. Adding a result opens Add torrent on it, exactly as any magnet link or
//! .torrent address does — nothing is added without the user's yes.
//!
//! The window owns nothing of the app's: it asks (`Act`) and the app does (fetches, opens Add
//! torrent). Results arrive through [`SearchWindow::found`], one provider at a time.

use egui::{Align2, FontId, Sense, Stroke, Ui};

use crate::dialogs::{Outcome, window};
use crate::search::{Hit, Provider};
use crate::winui::{self, ButtonKind, r};

/// What the window asks of the app.
pub enum Act {
    /// Ask these providers for this query.
    Search(String, Vec<Provider>),
    /// Open Add torrent on this result's link (a magnet or a .torrent's address).
    Download(String),
    /// Show the folder provider files go in, so one can be added.
    OpenSources,
}

const ROW_H: f32 = 19.0;
const ROWS: usize = 19;

pub struct SearchWindow {
    pub query: String,
    providers: Vec<Provider>,
    hits: Vec<Hit>,
    selected: Option<usize>,
    first: usize,
    /// How many providers are still to answer.
    waiting: usize,
    /// One line per provider: how many it gave, or what went wrong.
    notes: Vec<String>,
    searched: bool,
}

impl SearchWindow {
    pub fn new(providers: Vec<Provider>) -> Self {
        SearchWindow {
            query: String::new(),
            providers,
            hits: Vec::new(),
            selected: None,
            first: 0,
            waiting: 0,
            notes: Vec::new(),
            searched: false,
        }
    }

    /// A provider's answer, as it comes in: its hits appended and a line saying what it gave.
    pub fn found(&mut self, provider: &str, result: Result<Vec<Hit>, String>) {
        self.waiting = self.waiting.saturating_sub(1);
        match result {
            Ok(hits) => {
                // i18n: torrent search: how many results a source gave ("Internet Archive: 12 results")
                let count = crate::i18n::tn(hits.len() as u64, "{n} result", "{n} results");
                self.notes.push(format!("{provider}: {count}"));
                self.hits.extend(hits);
            }
            Err(e) => self.notes.push(format!("{provider}: {e}")),
        }
    }

    /// The providers are asking: the old answer is gone.
    pub fn searching(&mut self, count: usize) {
        self.hits.clear();
        self.selected = None;
        self.first = 0;
        self.notes.clear();
        self.waiting = count;
        self.searched = true;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<Act> {
        let colors = *crate::theme::colors();
        window(
            ctx,
            "torrent-search",
            // i18n: torrent search window: its title
            "Torrent search",
            [760.0, 500.0],
            true,
            colors.window,
            |ui, o| {
                let p = ui.painter().clone();
                // ---- the query ----
                // i18n: torrent search: the label beside its search box
                winui::text(
                    &p,
                    o.x + 12.0,
                    o.y + 31.0,
                    crate::i18n::t("Search for:"),
                    colors.text,
                );
                let edit_rect = r(o, 84.0, 18.0, 760.0 - 84.0 - 96.0 - 24.0, 22.0);
                let edit = winui::edit(ui, edit_rect, &mut self.query, true, false);
                // i18n: torrent search: the button that asks the sources
                let go = winui::button(
                    ui,
                    r(o, 760.0 - 96.0 - 12.0, 18.0, 96.0, 22.0),
                    "Search",
                    ButtonKind::Default,
                    true,
                    None,
                )
                .clicked();
                let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (go || enter) && !self.query.trim().is_empty() {
                    return Outcome::Done(Act::Search(
                        self.query.trim().to_owned(),
                        self.providers.clone(),
                    ));
                }

                // ---- the results ----
                // Column headers, in the ink a column heading has.
                let head = o.y + 56.0;
                // i18n: torrent search: the results list's columns
                winui::text(&p, o.x + 18.0, head, crate::i18n::t("Name"), colors.muted);
                p.text(
                    egui::pos2(o.x + 736.0 - 236.0, head),
                    Align2::RIGHT_CENTER,
                    crate::i18n::t("Size"),
                    FontId::proportional(12.0),
                    colors.muted,
                );
                // i18n: torrent search: the column naming where a result came from
                winui::text(
                    &p,
                    o.x + 736.0 - 190.0,
                    head,
                    crate::i18n::t("Source"),
                    colors.muted,
                );
                let list = r(o, 12.0, 64.0, 736.0, ROWS as f32 * ROW_H);
                p.rect_filled(list, 0.0, colors.window);
                p.rect_stroke(
                    list,
                    0.0,
                    Stroke::new(1.0, colors.group_line),
                    egui::StrokeKind::Inside,
                );
                self.list(ui, list);

                // ---- what each provider said ----
                let note = if self.waiting > 0 {
                    // i18n: torrent search: the sources are still being asked
                    format!("{}…", crate::i18n::t("Asking the sources"))
                } else if self.notes.is_empty() && !self.searched {
                    // i18n: torrent search: what to do before anything is searched
                    crate::i18n::t("Type what you are looking for and press Search.").to_owned()
                } else {
                    self.notes.join("    ")
                };
                winui::text(&p, o.x + 12.0, list.bottom() + 19.0, &note, colors.muted);

                // ---- the buttons ----
                // i18n: torrent search: opens Add torrent on the chosen result
                let add = winui::button(
                    ui,
                    r(o, 560.0, 466.0, 100.0, 22.0),
                    "Download",
                    if self
                        .selected
                        .and_then(|i| self.hits.get(i))
                        .and_then(|h| h.link())
                        .is_some()
                    {
                        ButtonKind::Default
                    } else {
                        ButtonKind::Disabled
                    },
                    false,
                    None,
                )
                .clicked();
                // i18n: torrent search: shows the folder a provider file goes in
                let folder = winui::button(
                    ui,
                    r(o, 12.0, 466.0, 136.0, 22.0),
                    "Add a source...",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                // i18n: the dialog's Close button
                let close = winui::button(
                    ui,
                    r(o, 668.0, 466.0, 80.0, 22.0),
                    "Close",
                    ButtonKind::Normal,
                    false,
                    None,
                )
                .clicked();
                if close || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if folder {
                    Outcome::Done(Act::OpenSources)
                } else if add {
                    match self
                        .selected
                        .and_then(|i| self.hits.get(i))
                        .and_then(|h| h.link())
                    {
                        Some(link) => Outcome::Done(Act::Download(link.to_owned())),
                        None => Outcome::Open,
                    }
                } else {
                    Outcome::Open
                }
            },
        )
    }

    /// The list: one row per hit, the chosen one filled, the wheel scrolling through the rest.
    fn list(&mut self, ui: &mut Ui, area: egui::Rect) {
        let colors = *crate::theme::colors();
        let p = ui.painter().clone();
        if ui.rect_contains_pointer(area) {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            let rows = (wheel / ROW_H) as i32;
            if rows != 0 {
                let last = (self.hits.len() as i32 - ROWS as i32).max(0);
                self.first = (self.first as i32 - rows).clamp(0, last) as usize;
            }
        }
        let shown: Vec<(usize, Hit)> = self
            .hits
            .iter()
            .skip(self.first)
            .take(ROWS)
            .cloned()
            .enumerate()
            .map(|(i, h)| (self.first + i, h))
            .collect();
        for (k, (index, hit)) in shown.iter().enumerate() {
            let top = area.top() + k as f32 * ROW_H;
            let row = egui::Rect::from_min_size(
                egui::pos2(area.left(), top),
                egui::vec2(area.width(), ROW_H),
            );
            let response =
                ui.interact(row, egui::Id::unique(("search-hit", index)), Sense::click());
            if self.selected == Some(*index) {
                p.rect_filled(row, 0.0, colors.selection);
            } else if response.hovered() {
                p.rect_filled(row, 0.0, colors.selection_inactive);
            }
            if response.clicked() {
                self.selected = Some(*index);
            }
            if response.double_clicked()
                && let Some(link) = hit.link()
            {
                // The list's own double-click, as every list in the app: act on the row.
                self.selected = Some(*index);
                let _ = link;
            }
            let chosen = self.selected == Some(*index);
            let text_color = if chosen {
                colors.selection_text
            } else {
                colors.text
            };
            let middle = top + ROW_H / 2.0;
            let name = fit(&p, &hit.name, area.width() - 300.0);
            p.text(
                egui::pos2(area.left() + 6.0, middle),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(12.0),
                text_color,
            );
            let size = hit.size.map(crate::model::size).unwrap_or_default();
            p.text(
                egui::pos2(area.right() - 236.0, middle),
                Align2::RIGHT_CENTER,
                size,
                FontId::proportional(12.0),
                if chosen {
                    colors.selection_muted
                } else {
                    colors.muted
                },
            );
            p.text(
                egui::pos2(area.right() - 190.0, middle),
                Align2::LEFT_CENTER,
                fit(&p, &hit.source, 178.0),
                FontId::proportional(12.0),
                if chosen {
                    colors.selection_muted
                } else {
                    colors.muted
                },
            );
        }
        if self.hits.len() > ROWS {
            let track = egui::Rect::from_min_max(
                egui::pos2(area.right() - 8.0, area.top()),
                egui::pos2(area.right() - 4.0, area.bottom()),
            );
            p.rect_filled(track, 0.0, colors.etch_shadow);
            let frac = ROWS as f32 / self.hits.len() as f32;
            let h = (track.height() * frac).max(20.0);
            let at = (self.first as f32 / self.hits.len() as f32) * track.height();
            p.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(track.left(), track.top() + at),
                    egui::vec2(track.width(), h),
                ),
                0.0,
                colors.group_line,
            );
        }
    }
}

/// `text` cut to fit `width` px, with an ellipsis.
fn fit(p: &egui::Painter, text: &str, width: f32) -> String {
    let font = FontId::proportional(12.0);
    let width_of = |s: &str| {
        p.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .rect
            .width()
    };
    if width_of(text) <= width {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    while !out.is_empty() && width_of(&format!("{out}…")) > width {
        out.pop();
    }
    format!("{out}…")
}

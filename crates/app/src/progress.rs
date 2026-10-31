//! The per-download progress dialog, painted to IDM 6.42's pixels
//! (notes/ui-spec/progress-dialog.md). Coordinates are client pixels from the spec.

use egui::{Color32, Pos2, Rect, Sense, Ui, ViewportBuilder, ViewportId, pos2, vec2};
use mdm_engine::Snapshot;
use mdm_engine::download::ConnStatus;

use crate::dialogs::display_path;
use crate::model::{self, Item, Status, size3};
// Laid out in English pixels: `fill` and `px` make each one where it is on screen (bars fill
// from the right in a right-to-left interface), `text` starts at an English pen.
use crate::winui::{self, ButtonKind, fill_l as fill, px_l as px, r, text};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}
const HILIGHT: Color32 = rgb(0xff, 0xff, 0xff);
const LIGHT: Color32 = rgb(0xe3, 0xe3, 0xe3);
const SHADOW: Color32 = rgb(0xa0, 0xa0, 0xa0);
const DKSHADOW: Color32 = rgb(0x69, 0x69, 0x69);
const TAB_CORNER: Color32 = rgb(0xf0, 0xf0, 0xf0);
const DISABLED: Color32 = rgb(0x80, 0x80, 0x80);

/// Segment bar interior rows (top → bottom): not downloaded, downloaded, start marker.
const SEG_ROWS: [(u32, u32, u32); 13] = [
    (0xfafafc, 0xbfc6f3, 0xf67566),
    (0xf7f8fa, 0xa4aae7, 0xf46a5e),
    (0xf5f6fa, 0x898edd, 0xf25e56),
    (0xf2f4f9, 0x7176d3, 0xef514c),
    (0xecf0f6, 0x686bcf, 0xea4240),
    (0xdbe0ee, 0x1a3cb7, 0xbb0205),
    (0xdce1f0, 0x0f43b8, 0xbe0205),
    (0xdee3f1, 0x1155bf, 0xc60b09),
    (0xdfe4f1, 0x1267c7, 0xd21913),
    (0xe1e5f1, 0x117acd, 0xde281b),
    (0xe2e7f3, 0x128ed6, 0xeb3724),
    (0xe4e8f4, 0x1aafe3, 0xf1462c),
    (0xe6e9f5, 0x2dd3f1, 0xf64f30),
];

fn hex(v: u32) -> Color32 {
    rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Status,
    Limiter,
    Completion,
}

/// A torrent's details tabs (M15, D-014), in the place of the connections table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TTab {
    Files,
    Peers,
    Trackers,
    Pieces,
    General,
}

pub struct Progress {
    pub id: u64,
    details: bool,
    tab: Tab,
    ttab: TTab,
    limiter: bool,
    /// KBytes/sec typed on the Speed Limiter tab.
    limit_kb: String,
    /// The tab has been initialised from the download's current limit.
    limit_init: bool,
    /// A limit change to report to the app (`Action::Limit`), and whether to remember it.
    pending_limit: Option<(Option<u64>, bool)>,
    remember: bool,
    show_complete: bool,
    /// Options on completion, used when "Show download complete dialog" is off.
    hang_up: bool,
    exit: bool,
    turn_off_on: bool,
    turn_off: usize,
    force: bool,
}

pub enum Action {
    None,
    Pause,
    Resume,
    /// Stop the download and close the dialog.
    Cancel,
    /// Set this download's speed limit (bytes/s, None = off).
    /// This download's limit (None: unlimited); `true`: remember it for the file.
    Limit(Option<u64>, bool),
    /// Close the dialog, keep downloading.
    Close,
}

impl Progress {
    pub fn new(id: u64) -> Self {
        Progress {
            id,
            details: true,
            tab: Tab::Status,
            ttab: TTab::Files,
            limiter: false,
            limit_kb: "10".into(),
            limit_init: false,
            pending_limit: None,
            remember: false,
            show_complete: true,
            hang_up: false,
            exit: false,
            turn_off_on: false,
            turn_off: 0,
            force: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, item: &Item, snap: Option<&Snapshot>) -> Action {
        let frac = item
            .size
            .filter(|t| *t > 0)
            .map_or(0.0, |t| (item.downloaded as f64 / t as f64).clamp(0.0, 1.0));
        // IDM truncates the percentage in the title (44.51 % → "44%").
        // The name keeps its own direction (an Arabic one doesn't pull the percentage after it).
        let title = format!(
            "{}% {}",
            (frac * 100.0).floor() as u32,
            crate::i18n::isolate(&item.file_name)
        );
        let client = if self.details {
            [548.0, 412.0]
        } else {
            [548.0, 241.0]
        };
        let key = egui::Id::unique(("progress-pos", self.id));
        let title_now = title.clone();
        // Hidden until dialogs::keep_theme has given it its theme.
        let mut builder = crate::quiet(ViewportBuilder::default())
            .with_visible(false)
            .with_title(title)
            .with_inner_size(client)
            // IDM's progress window has the sizing-frame caption (31 px) but 2 px borders and a
            // fixed size. On Windows the sizing frame is asked for and win32::thin_frame trims
            // its borders and disables resizing; elsewhere the window is simply not resizable
            // (its contents are laid out for one size: Show/Hide details switches between two).
            .with_resizable(cfg!(windows))
            .with_icon(crate::app_icon())
            .with_maximize_button(false);
        if let Some(p) = crate::dialogs::place(ctx, key, [548.0, 412.0]) {
            builder = builder.with_position(p);
        }
        let action = ctx.show_viewport_immediate(
            ViewportId::from_hash_of(("progress", self.id)),
            builder,
            |ui, _| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    return Action::Close;
                }
                let action = egui::CentralPanel::no_frame()
                    .frame(egui::Frame::new().fill(winui::BG()))
                    .show(ui, |ui| {
                        let o = ui.max_rect().min;
                        // The client area, laid out from the right in a right-to-left interface.
                        let _surface = winui::Surface::enter(o.x, o.x + client[0]);
                        self.body(ui, o, item, snap, frac)
                    })
                    .inner;
                // Escape hides the window (the download goes on), as the × does.
                if matches!(action, Action::None) && winui::escape_pressed(ui) {
                    Action::Close
                } else {
                    action
                }
            },
        );
        // Apply IDM's thin frame once, and again whenever the client size changes (details).
        #[cfg(windows)]
        {
            let applied = key.with("framed");
            let scale = ctx.input_for(ViewportId::from_hash_of(("progress", self.id)), |i| {
                i.pixels_per_point
            });
            if ctx.data(|d| d.get_temp::<[f32; 2]>(applied)) != Some(client)
                && crate::win32::thin_frame(&title_now, client, scale)
            {
                ctx.data_mut(|d| d.insert_temp(applied, client));
            }
        }
        let closing = matches!(action, Action::Close | Action::Cancel);
        crate::dialogs::keep_theme(
            ctx,
            ViewportId::from_hash_of(("progress", self.id)),
            key,
            &title_now,
            crate::theme::dark(),
            !closing,
        );
        let action = match (action, self.pending_limit.take()) {
            (Action::None, Some((limit, remember))) => Action::Limit(limit, remember),
            (a, _) => a,
        };
        if matches!(action, Action::Close | Action::Cancel) {
            crate::dialogs::forget_place(ctx, key);
            ctx.data_mut(|d| d.remove::<[f32; 2]>(key.with("framed")));
        }
        action
    }

    fn body(
        &mut self,
        ui: &mut Ui,
        o: Pos2,
        item: &Item,
        snap: Option<&Snapshot>,
        frac: f64,
    ) -> Action {
        let p = ui.painter().clone();
        self.tabs(ui, o);
        match self.tab {
            Tab::Status => status_page(ui, o, item, snap, frac),
            Tab::Limiter => {
                if self.limiter_page(ui, o, item) {
                    self.tab = Tab::Status;
                }
            }
            Tab::Completion => {
                if self.completion_page(ui, o, item) {
                    self.tab = Tab::Status;
                }
            }
        }

        // Progress bar: #bcbcbc border, #e6e6e6 track, #0f7b0f fill of floor(f × 511) px.
        let bar = r(o, 17.0, 180.0, 513.0, 16.0);
        p.rect_stroke(
            bar,
            0.0,
            egui::Stroke::new(1.0, rgb(0xbc, 0xbc, 0xbc)),
            egui::StrokeKind::Inside,
        );
        fill(
            &p,
            o.x + 18.0,
            o.y + 181.0,
            o.x + 528.0,
            o.y + 194.0,
            rgb(0xe6, 0xe6, 0xe6),
        );
        let w = (frac * 511.0).floor() as f32;
        if w > 0.0 {
            fill(
                &p,
                o.x + 18.0,
                o.y + 181.0,
                o.x + 17.0 + w,
                o.y + 194.0,
                rgb(0x0f, 0x7b, 0x0f),
            );
        }

        let mut action = Action::None;
        let details_label = if self.details {
            crate::i18n::mark("<< Hide details")
        } else {
            crate::i18n::mark("Show details >>")
        };
        let other_tab = self.tab != Tab::Status;
        let kind = if other_tab {
            ButtonKind::Default
        } else {
            ButtonKind::Normal
        };
        if winui::button(
            ui,
            r(o, 30.0, 206.0, 138.0, 22.0),
            details_label,
            kind,
            false,
            None,
        )
        .clicked()
        {
            self.details = !self.details;
        }
        let running = item.status == Status::Downloading;
        let can_resume = matches!(item.status, Status::Stopped | Status::Error);
        let (label, pkind) = if running {
            // i18n: verb: pause the download
            (crate::i18n::mark("Pause"), ButtonKind::Normal)
        } else if can_resume {
            // i18n: verb: continue the download
            (crate::i18n::mark("Resume"), ButtonKind::Normal)
        } else {
            (crate::i18n::mark("Pause"), ButtonKind::Disabled)
        };
        if winui::button(
            ui,
            r(o, 311.0, 206.0, 82.0, 22.0),
            label,
            pkind,
            false,
            None,
        )
        .clicked()
        {
            action = if running {
                Action::Pause
            } else {
                Action::Resume
            };
        }
        if winui::button(
            ui,
            r(o, 422.0, 206.0, 73.0, 22.0),
            "Cancel",
            ButtonKind::Normal,
            false,
            None,
        )
        .clicked()
        {
            action = Action::Cancel;
        }

        if self.details && item.torrent.is_some() {
            self.torrent_details(ui, &p, o, item);
        } else if self.details {
            winui::text_in_window(
                ui,
                &p,
                o.x + 137.0,
                o.y + 242.0,
                crate::i18n::t("Start positions and download progress by connections"),
                winui::TEXT(),
            );
            segment_bar(&p, o, item, snap);
            connections(ui, &p, o, snap);
        }
        action
    }

    /// Classic 3-D tabs over a raised page frame (spec "Tab strip").
    fn tabs(&mut self, ui: &mut Ui, o: Pos2) {
        let order = [Tab::Status, Tab::Limiter, Tab::Completion];
        let current = order.iter().position(|t| *t == self.tab).unwrap_or(0);
        if let Some(i) = winui::tab_keys(ui, order.len(), current) {
            self.tab = order[i];
        }
        let p = ui.painter().clone();
        let tabs = [
            // i18n: tab title: how the download is going
            (Tab::Status, crate::i18n::t("Download status"), 20.0, 111.0),
            // i18n: tab title: the tool that limits the download speed
            (Tab::Limiter, crate::i18n::t("Speed Limiter"), 112.0, 187.0),
            // i18n: tab title: what happens when the download finishes
            (
                Tab::Completion,
                crate::i18n::t("Options on completion"),
                188.0,
                305.0,
            ),
        ];
        // A translation sizes the tabs to their labels, within the page (cut with "..." only
        // when the row can't hold them, whole on hover).
        let english = crate::i18n::code() == "en";
        let row = if english {
            tabs.map(|(_, label, x0, x1)| (x0, x1, label.to_owned()))
                .to_vec()
        } else {
            winui::tab_row(&p, &tabs.map(|t| t.1), 20.0, 525.0, 1.0, 6.0)
        };
        for ((_, full, ..), (x0, x1, shown)) in tabs.iter().zip(&row) {
            if shown != full {
                let tab = r(o, *x0, 8.0, x1 - x0 + 1.0, 20.0);
                winui::full_on_hover(ui, tab, ("progress-tab-label", *x0 as i32), full);
            }
        }
        let tabs: [(Tab, &str, f32, f32); 3] = std::array::from_fn(|i| {
            let (x0, x1, shown) = &row[i];
            (tabs[i].0, shown.as_str(), *x0, *x1)
        });
        if !crate::theme::dark() {
            self.tabs_light(ui, o, &tabs, english);
            return;
        }
        // Page frame (EDGE_RAISED), x 18..529, y 28..171.
        let (l, t, rr, b) = (o.x + 18.0, o.y + 28.0, o.x + 529.0, o.y + 171.0);
        fill(&p, l, t, rr, t, HILIGHT);
        fill(&p, l + 1.0, t + 1.0, rr - 1.0, t + 1.0, LIGHT);
        fill(&p, l, t, l, b, HILIGHT);
        fill(&p, l + 1.0, t + 1.0, l + 1.0, b - 1.0, LIGHT);
        fill(&p, rr, t, rr, b, DKSHADOW);
        fill(&p, rr - 1.0, t + 1.0, rr - 1.0, b - 1.0, SHADOW);
        fill(&p, l, b, rr, b, DKSHADOW);
        fill(&p, l + 1.0, b - 1.0, rr - 1.0, b - 1.0, SHADOW);
        for (tab, label, x0, x1) in tabs {
            let sel = tab == self.tab;
            let (x0, x1, y0) = if sel {
                (o.x + x0 - 2.0, o.x + x1 + 2.0, o.y + 8.0)
            } else {
                (o.x + x0, o.x + x1, o.y + 10.0)
            };
            let y1 = o.y + 27.0;
            let hit = winui::rect(Rect::from_min_max(pos2(x0, y0), pos2(x1 + 1.0, y1 + 1.0)));
            let resp = ui.interact(
                hit,
                egui::Id::unique(("progress-tab", label)),
                Sense::click(),
            );
            // An unselected tab under the pointer: lighter (the torrent tabs' hover grey).
            let body = if !sel && resp.hovered() {
                rgb(0x45, 0x45, 0x45)
            } else {
                winui::BG()
            };
            fill(&p, x0, y0, x1, y1 + if sel { 2.0 } else { 0.0 }, body);
            px(&p, x0, y0, TAB_CORNER);
            px(&p, x0 + 1.0, y0, TAB_CORNER);
            fill(&p, x0 + 2.0, y0, x1 - 2.0, y0, HILIGHT);
            px(&p, x1 - 1.0, y0, TAB_CORNER);
            px(&p, x1, y0, TAB_CORNER);
            px(&p, x0, y0 + 1.0, TAB_CORNER);
            px(&p, x0 + 1.0, y0 + 1.0, HILIGHT);
            fill(&p, x0 + 2.0, y0 + 1.0, x1 - 2.0, y0 + 1.0, LIGHT);
            px(&p, x1 - 1.0, y0 + 1.0, DKSHADOW);
            px(&p, x1, y0 + 1.0, TAB_CORNER);
            let bottom = y1 + if sel { 1.0 } else { 0.0 };
            fill(&p, x0, y0 + 2.0, x0, bottom, HILIGHT);
            fill(&p, x0 + 1.0, y0 + 2.0, x0 + 1.0, bottom, LIGHT);
            fill(&p, x1 - 1.0, y0 + 2.0, x1 - 1.0, y1, SHADOW);
            fill(&p, x1, y0 + 2.0, x1, y1, DKSHADOW);
            let base_x = if sel { x0 + 2.0 } else { x0 };
            text(
                &p,
                base_x + 5.0,
                o.y + if sel { 22.0 } else { 21.0 },
                label,
                winui::TEXT(),
            );
            winui::show_focus(ui, &resp, hit.shrink(3.0));
            if resp.clicked() {
                self.tab = tab;
            }
        }
    }

    /// Light mode's themed tabs (light-dialogs.md §6): flat #e5e5e5 borders with small rounded
    /// top corners, selected #f9f9f9 (inflated 2 px left, open into the page), unselected
    /// #f3f3f3; tabs share borders at x 113 and 187; text x as in dark, baselines selected 21 /
    /// unselected 23.
    fn tabs_light(
        &mut self,
        ui: &mut Ui,
        o: Pos2,
        tabs: &[(Tab, &str, f32, f32); 3],
        english: bool,
    ) {
        let p = ui.painter().clone();
        let border = rgb(0xe5, 0xe5, 0xe5);
        let (sel_fill, unsel_fill) = (rgb(0xf9, 0xf9, 0xf9), rgb(0xf3, 0xf3, 0xf3));
        // Page: 1 px border x 18 / 527, y 28 / 170, body #f9f9f9, #f3f3f3 shadow right (2 px)
        // and bottom (1 px).
        let (l, t, rr, b) = (o.x + 18.0, o.y + 28.0, o.x + 527.0, o.y + 170.0);
        fill(&p, l, t, rr, b, sel_fill);
        fill(&p, l, t, rr, t, border);
        fill(&p, l, t, l, b, border);
        fill(&p, rr, t, rr, b, border);
        fill(&p, l, b, rr, b, border);
        fill(&p, rr + 1.0, t, rr + 2.0, b + 1.0, unsel_fill);
        fill(&p, l, b + 1.0, rr + 2.0, b + 1.0, unsel_fill);
        // Tab boundaries (shared borders); a translation's from its tabs.
        let edges = if english {
            [20.0, 113.0, 187.0, 303.0]
        } else {
            [tabs[0].2, tabs[1].2, tabs[2].2, tabs[2].3]
        };
        let draw = |i: usize, sel: bool| {
            let x0 = o.x + edges[i] - if sel { 2.0 } else { 0.0 };
            let x1 = o.x + edges[i + 1];
            let y0 = o.y + if sel { 8.0 } else { 10.0 };
            let y1 = o.y + 27.0;
            let bottom = if sel { o.y + 28.0 } else { y1 };
            fill(
                &p,
                x0 + 1.0,
                y0 + 1.0,
                x1 - 1.0,
                bottom,
                if sel { sel_fill } else { unsel_fill },
            );
            fill(&p, x0 + 2.0, y0, x1 - 2.0, y0, border);
            fill(&p, x0, y0 + 2.0, x0, bottom, border);
            fill(&p, x1, y0 + 2.0, x1, bottom, border);
            for (x, inward) in [(x0, 1.0), (x1, -1.0)] {
                px(&p, x, y0, rgb(0xec, 0xec, 0xec));
                px(&p, x + inward, y0, rgb(0xe6, 0xe6, 0xe6));
                px(&p, x, y0 + 1.0, rgb(0xe6, 0xe6, 0xe6));
                px(&p, x + inward, y0 + 1.0, rgb(0xf7, 0xf7, 0xf7));
            }
        };
        let selected = tabs.iter().position(|t| t.0 == self.tab).unwrap_or(0);
        for i in 0..3 {
            if i != selected {
                draw(i, false);
            }
        }
        draw(selected, true);
        for (i, (tab, label, dark_x0, _)) in tabs.iter().enumerate() {
            let sel = i == selected;
            let hit = winui::rect(Rect::from_min_max(
                pos2(o.x + edges[i], o.y + 8.0),
                pos2(o.x + edges[i + 1] + 1.0, o.y + 28.0),
            ));
            let resp = ui.interact(
                hit,
                egui::Id::unique(("progress-tab", *label)),
                Sense::click(),
            );
            // An unselected tab under the pointer: lighter, as the shared light tabs.
            if !sel && resp.hovered() {
                fill(
                    &p,
                    o.x + edges[i] + 1.0,
                    o.y + 11.0,
                    o.x + edges[i + 1] - 1.0,
                    o.y + 27.0,
                    rgb(0xfd, 0xfd, 0xfd),
                );
            }
            // Text x is the same as in dark mode (25 / 117 / 193).
            let tx = o.x + dark_x0 + 5.0;
            text(
                &p,
                tx,
                o.y + if sel { 21.0 } else { 23.0 },
                label,
                winui::TEXT(),
            );
            winui::show_focus(ui, &resp, hit.shrink(3.0));
            if resp.clicked() {
                self.tab = *tab;
            }
        }
    }

    /// The Speed Limiter tab, live for this download. Returns true when "Hide tab" is pressed.
    fn limiter_page(&mut self, ui: &mut Ui, o: Pos2, item: &Item) -> bool {
        let p = ui.painter().clone();
        if !self.limit_init {
            if let Some(rate) = item.live.as_ref().and_then(|l| l.dl.speed_limit()) {
                self.limiter = true;
                self.limit_kb = (rate / 1024).max(1).to_string();
            }
            // A remembered limit: the box is ticked (and the limit shows even while stopped).
            if let Some(rate) = item.own_limit {
                self.limiter = true;
                self.limit_kb = (rate / 1024).max(1).to_string();
                self.remember = true;
            }
            self.limit_init = true;
        }
        text(
            &p,
            o.x + 33.0,
            o.y + 51.0,
            crate::i18n::t("Transfer rate"),
            winui::TEXT(),
        );
        let rate = if item.status == Status::Downloading {
            model::rate(item.rate())
        } else {
            String::new()
        };
        text(&p, o.x + 136.0, o.y + 51.0, &rate, winui::TEXT());
        let was_on = self.limiter;
        winui::checkbox(
            ui,
            o + vec2(33.0, 64.0),
            &mut self.limiter,
            "Use Speed Limiter",
            true,
        );
        let label = if self.limiter {
            winui::TEXT()
        } else {
            DISABLED
        };
        text(
            &p,
            o.x + 33.0,
            o.y + 92.0,
            crate::i18n::t("Maximum download speed:"),
            label,
        );
        let rate_box = winui::edit_right(
            ui,
            r(o, 33.0, 100.0, 77.0, 23.0),
            &mut self.limit_kb,
            self.limiter,
        );
        self.limit_kb.retain(|c| c.is_ascii_digit());
        // A typed rate applies once typed (focus leaves, Enter), not at each digit (typing 100
        // would throttle to 1, then 10 KB/s on the way).
        let typed = rate_box.lost_focus();
        // i18n: unit after an input box: kilobytes per second
        text(
            &p,
            o.x + 118.0,
            o.y + 113.0,
            crate::i18n::t("KBytes/sec"),
            label,
        );
        let remembered = self.remember;
        winui::checkbox(ui, o + vec2(33.0, 134.0), &mut self.remember, "", true);
        // Its caption is two lines drawn below; a click on them ticks it too.
        winui::caption_click(
            ui,
            r(o, 51.0, 127.0, 450.0, 27.0),
            "remember-limit",
            &mut self.remember,
            true,
        );
        // An empty or zero rate isn't "unlimited" while the box is ticked: the limit stays
        // as it was until a rate is typed.
        let kb: u64 = self.limit_kb.parse().unwrap_or(0);
        if (was_on != self.limiter || (self.limiter && typed) || remembered != self.remember)
            && !(self.limiter && kb == 0)
        {
            let limit = self.limiter.then_some(kb * 1024);
            self.pending_limit = Some((limit, self.remember));
        }
        text(
            &p,
            o.x + 51.0,
            o.y + 137.0,
            crate::i18n::t("Remember Speed Limiter settings for this file"),
            winui::TEXT(),
        );
        text(
            &p,
            o.x + 51.0,
            o.y + 150.0,
            crate::i18n::t("on download stop/resume"),
            winui::TEXT(),
        );
        hide_tab(ui, o)
    }

    /// Options on completion → "Show download complete dialog" for this file.
    pub fn shows_complete(&self) -> bool {
        self.show_complete
    }

    /// Options on completion's other settings, when they apply (the complete dialog is off).
    pub fn when_done(&self) -> Option<crate::queue::WhenDone> {
        (!self.show_complete).then(|| crate::queue::WhenDone {
            hang_up: self.hang_up,
            exit: self.exit,
            turn_off: self
                .turn_off_on
                .then(|| crate::queue::TurnOff::ALL[self.turn_off]),
            force: self.turn_off_on && self.force,
        })
    }

    /// Returns true when "Hide tab" is pressed.
    fn completion_page(&mut self, ui: &mut Ui, o: Pos2, item: &Item) -> bool {
        let p = ui.painter().clone();
        text(
            &p,
            o.x + 33.0,
            o.y + 45.0,
            crate::i18n::t("Save To:"),
            winui::TEXT(),
        );
        text(
            &p,
            o.x + 90.0,
            o.y + 45.0,
            &display_path(&item.dest),
            winui::TEXT(),
        );
        winui::checkbox(
            ui,
            o + vec2(33.0, 55.0),
            &mut self.show_complete,
            "Show download complete dialog",
            true,
        );
        fill(
            &p,
            o.x + 33.0,
            o.y + 74.0,
            o.x + 505.0,
            o.y + 74.0,
            winui::ETCH_SHADOW(),
        );
        fill(
            &p,
            o.x + 33.0,
            o.y + 75.0,
            o.x + 506.0,
            o.y + 75.0,
            winui::ETCH_LIGHT(),
        );
        text(
            &p,
            o.x + 33.0,
            o.y + 89.0,
            crate::i18n::t(
                "These settings are unavailable when \"Show download complete dialog\" is turned on",
            ),
            winui::TEXT(),
        );
        // Used when the complete dialog is off (it asks what to do itself).
        let on = !self.show_complete;
        winui::checkbox(
            ui,
            o + vec2(33.0, 96.0),
            &mut self.hang_up,
            "Hang up modem when done",
            on,
        );
        winui::checkbox(
            ui,
            o + vec2(33.0, 114.0),
            &mut self.exit,
            "Exit Mister Download Manager when done",
            on,
        );
        winui::checkbox_until(
            ui,
            o + vec2(33.0, 132.0),
            &mut self.turn_off_on,
            "Turn off computer when done",
            on,
            Some(o.x + 267.0),
        );
        winui::checkbox(
            ui,
            o + vec2(52.0, 148.0),
            &mut self.force,
            "Force processes to terminate",
            on && self.turn_off_on,
        );
        if let Some(i) = winui::combo_light(
            ui,
            r(o, 267.0, 134.0, 119.0, 21.0),
            &crate::queue::TurnOff::LABELS,
            self.turn_off,
            on && self.turn_off_on,
            "progress-turn-off",
        ) {
            self.turn_off = i;
        }
        hide_tab(ui, o)
    }
}

/// "Hide tab" (401,135,96,21) with its 1 px #f9f9f9 focus ring.
fn hide_tab(ui: &mut Ui, o: Pos2) -> bool {
    let p = ui.painter().clone();
    let ring = r(o, 400.0, 134.0, 98.0, 23.0);
    p.rect_stroke(
        ring,
        0.0,
        egui::Stroke::new(1.0, winui::LIGHT_PAGE()),
        egui::StrokeKind::Inside,
    );
    winui::button(
        ui,
        r(o, 401.0, 135.0, 96.0, 21.0),
        "Hide tab",
        ButtonKind::Normal,
        false,
        None,
    )
    .clicked()
}

/// Right end for text on the tab page (its frame is x 18..529).
const PAGE_TEXT_RIGHT: f32 = 520.0;

fn status_page(ui: &Ui, o: Pos2, item: &Item, snap: Option<&Snapshot>, frac: f64) {
    let p = &ui.painter().clone();
    // Long values are cut with "..." at the page edge and shown whole on hover.
    let fit = |key: &str, x: f32, base: f32, s: &str, color| {
        let (rect, cut) = winui::text_fit(p, o.x + x, o.y + base, s, color, o.x + PAGE_TEXT_RIGHT);
        winui::copyable(ui, rect, key, s, cut);
    };
    let status = match item.status {
        Status::Downloading => {
            // A video (yt-dlp) has no engine connections: data arriving is the sign.
            let receiving = snap.is_some_and(|s| {
                s.connections
                    .iter()
                    .any(|c| matches!(c.status, ConnStatus::Receiving))
            }) || ((item.video.is_some() || item.torrent.is_some())
                && item.rate() > 0.0);
            if receiving {
                crate::i18n::t("Receiving data...")
            } else {
                crate::i18n::t("Connecting...")
            }
            .to_owned()
        }
        // i18n: a download's status (state it is in, not a command): finished
        Status::Complete => crate::i18n::t("Complete").to_owned(),
        // i18n: a download's status (state it is in, not a command): stopped for now
        Status::Stopped => crate::i18n::t("Paused").to_owned(),
        // i18n: a download's status (state it is in, not a command): waiting in a queue
        Status::Queued => crate::i18n::t("Waiting").to_owned(),
        Status::Error => crate::i18n::fmt(
            // i18n: a download's status: it failed, with the reason
            crate::i18n::t("Error: {error}"),
            &[(
                "error",
                // i18n: the reason a download failed is not known
                item.error.as_deref().unwrap_or(crate::i18n::t("unknown")),
            )],
        ),
    };
    fit(
        "url",
        33.0,
        47.0,
        &crate::i18n::ltr(item.url.as_str()),
        winui::TEXT(),
    );
    let rows: [(&str, f32, f32, String); 6] = [
        // i18n: noun: label before the download's state
        (crate::i18n::mark("Status"), 64.0, 102.0, status),
        (
            crate::i18n::mark("File size"),
            86.0,
            135.0,
            item.size.map_or(String::new(), size3),
        ),
        (
            // i18n: label before the amount received so far
            crate::i18n::mark("Downloaded"),
            102.0,
            135.0,
            format!("{} ( {:.2} % )", size3(item.downloaded), frac * 100.0),
        ),
        (
            crate::i18n::mark("Transfer rate"),
            118.0,
            135.0,
            if item.status == Status::Downloading {
                model::rate(item.rate())
            } else {
                String::new()
            },
        ),
        (
            crate::i18n::mark("Time left"),
            134.0,
            135.0,
            item.time_left(),
        ),
        (
            // i18n: label: whether the server lets a stopped download continue (Yes / No)
            crate::i18n::mark("Resume capability"),
            151.0,
            213.0,
            match item.resumable {
                Some(true) => crate::i18n::t("Yes"),
                Some(false) => crate::i18n::t("No"),
                None => "",
            }
            .to_owned(),
        ),
    ];
    // A value starts 6 px after its label at the least (a long translation); the four rows that
    // share IDM's column at 135 keep sharing one.
    let after = |label: &str| 33.0 + winui::text_width(p, crate::i18n::t(label)).ceil() + 6.0;
    let column = rows
        .iter()
        .filter(|r| r.2 == 135.0)
        .map(|r| after(r.0))
        .fold(135.0, f32::max);
    for (label, base, vx, value) in rows {
        let vx = if vx == 135.0 {
            column
        } else {
            vx.max(after(label))
        };
        text(
            p,
            o.x + 33.0,
            o.y + base,
            crate::i18n::t(label),
            winui::TEXT(),
        );
        // Light mode shows the status value in pure blue (light-dialogs.md).
        let vc = if label == "Status" && !crate::theme::dark() {
            rgb(0x00, 0x00, 0xff)
        } else {
            winui::TEXT()
        };
        fit(label, vx, base, &value, vc);
    }
}

impl Progress {
    /// A torrent's details: a tab strip where the connections caption is, the piece map in
    /// the segment bar, and the tab's table below.
    fn torrent_details(&mut self, ui: &mut Ui, p: &egui::Painter, o: Pos2, item: &Item) {
        let d = match &item.live {
            Some(crate::model::Live {
                dl: crate::model::Job::Torrent(run),
                ..
            }) => Some(run.details()),
            _ => None,
        };
        let tabs = [
            // i18n: tab title: the torrent's files
            (TTab::Files, crate::i18n::mark("Files")),
            // i18n: tab title: the other computers sharing the torrent
            (TTab::Peers, crate::i18n::mark("Peers")),
            // i18n: tab title: the servers that list the peers
            (TTab::Trackers, crate::i18n::mark("Trackers")),
            // i18n: tab title: the parts the torrent is made of
            (TTab::Pieces, crate::i18n::mark("Pieces")),
            // i18n: tab title: general information
            (TTab::General, crate::i18n::mark("General")),
        ];
        // The usual tab control (the Scheduler's and Options' look): tabs sized to their
        // labels from the leading edge, the page under them holding the piece map and the
        // table.
        let english: Vec<&str> = tabs.iter().map(|t| t.1).collect();
        let light: Vec<winui::LightTab> = winui::tab_row(p, &english, 16.0, 530.0, 2.0, 8.0)
            .into_iter()
            .zip(tabs.iter())
            .map(|((x0, x1, _), (_, label))| winui::LightTab {
                label,
                sel: (x0 - 2.0, x1 + 2.0),
                unsel: (x0, x1),
            })
            .collect();
        let current = tabs.iter().position(|t| t.0 == self.ttab).unwrap_or(0);
        if let Some(i) = winui::tabs_light(ui, o, r(o, 14.0, 226.0, 521.0, 180.0), &light, current)
        {
            self.ttab = tabs[i].0;
        }
        let pieces = d.as_ref().map(|d| d.pieces.as_slice()).unwrap_or(&[]);
        piece_bar(p, o, pieces);
        let Some(d) = &d else {
            table(ui, p, o, "t-none", &[(25.0, "")], &[], &[]);
            text(
                p,
                o.x + 25.0,
                o.y + 312.0,
                crate::i18n::t("Shown while the torrent is downloading."),
                winui::TEXT(),
            );
            return;
        };
        match self.ttab {
            TTab::Files => {
                let rows: Vec<Vec<String>> = d
                    .files
                    .iter()
                    .map(|(f, on, done)| {
                        let pct = if !on {
                            // i18n: a torrent file the user chose not to download
                            crate::i18n::t("Skipped").to_owned()
                        } else if f.size == 0 {
                            "100 %".to_owned()
                        } else {
                            format!("{:.1} %", 100.0 * *done as f64 / f.size as f64)
                        };
                        vec![f.path.display().to_string(), model::size(f.size), pct]
                    })
                    .collect();
                table(
                    ui,
                    p,
                    o,
                    "t-files",
                    // i18n: column titles: File (name), Size, Done (how much is downloaded)
                    &[
                        (25.0, crate::i18n::mark("File")),
                        (335.0, crate::i18n::mark("Size")),
                        (430.0, crate::i18n::mark("Done")),
                    ],
                    &[330.0, 425.0],
                    &rows,
                );
            }
            TTab::Peers => {
                let rows: Vec<Vec<String>> = d
                    .peers
                    .iter()
                    .map(|q| {
                        vec![
                            q.addr.clone(),
                            q.client.clone().unwrap_or_default(),
                            model::size(q.fetched),
                            model::size(q.uploaded),
                        ]
                    })
                    .collect();
                table(
                    ui,
                    p,
                    o,
                    "t-peers",
                    &[
                        // i18n: column title: the web address (URL)
                        (25.0, crate::i18n::mark("Address")),
                        // i18n: column title: the peer's torrent program
                        (190.0, crate::i18n::mark("Client")),
                        // i18n: column title: how much came from the peer
                        (340.0, crate::i18n::mark("Received")),
                        // i18n: column title: how much went to the peer
                        (425.0, crate::i18n::mark("Sent")),
                    ],
                    &[185.0, 335.0, 420.0],
                    &rows,
                );
            }
            TTab::Trackers => {
                let rows: Vec<Vec<String>> = if d.trackers.is_empty() {
                    vec![vec![
                        crate::i18n::t("None: peers come from the DHT and other peers").into(),
                    ]]
                } else {
                    d.trackers.iter().map(|t| vec![t.clone()]).collect()
                };
                // i18n: column title: a server that lists the peers
                let head = [(25.0, crate::i18n::mark("Tracker"))];
                table(ui, p, o, "t-trackers", &head, &[], &rows);
            }
            TTab::Pieces => {
                let have = pieces.iter().filter(|b| **b).count();
                let head = crate::i18n::fmt(
                    // i18n: {have} and {total} = numbers of pieces; {size} = one piece's size
                    crate::i18n::t("{have} of {total} pieces ({size} each)"),
                    &[
                        ("have", &crate::i18n::integer(have as u64)),
                        ("total", &crate::i18n::integer(pieces.len() as u64)),
                        ("size", &model::size(u64::from(d.piece_length))),
                    ],
                );
                table(ui, p, o, "t-pieces", &[(25.0, head.as_str())], &[], &[]);
                piece_legend(p, o);
                piece_grid(p, o, pieces);
            }
            TTab::General => {
                let st = d.stats.as_ref();
                let have = pieces.iter().filter(|b| **b).count();
                let ratio = st.filter(|s| s.done > 0).map_or("0.00".into(), |s| {
                    format!("{:.2}", s.uploaded as f64 / s.done as f64)
                });
                let hash = item
                    .torrent
                    .as_ref()
                    .map_or(String::new(), |t| t.hash.clone());
                let folder = item
                    .torrent
                    .as_ref()
                    .map_or(String::new(), |t| t.folder.display().to_string());
                let t = |s: &str| crate::i18n::t(s).to_owned();
                let rows = vec![
                    // i18n: the torrent's identifier
                    vec![t("Info hash"), hash],
                    vec![
                        t("Pieces"),
                        crate::i18n::fmt(
                            // i18n: {have} and {total} = numbers of pieces; {size} = one piece's size
                            crate::i18n::t("{have} of {total} ({size} each)"),
                            &[
                                ("have", &crate::i18n::integer(have as u64)),
                                ("total", &crate::i18n::integer(pieces.len() as u64)),
                                ("size", &model::size(u64::from(d.piece_length))),
                            ],
                        ),
                    ],
                    vec![
                        t("Peers"),
                        st.map_or(String::new(), |s| {
                            crate::i18n::fmt(
                                // i18n: {live} = peers connected now; {known} = peers heard of
                                crate::i18n::t("{live} connected, {known} known"),
                                &[
                                    ("live", &crate::i18n::integer(s.peers_live as u64)),
                                    ("known", &crate::i18n::integer(s.peers_seen as u64)),
                                ],
                            )
                        }),
                    ],
                    vec![
                        // i18n: how much was sent to other peers
                        t("Sent"),
                        st.map_or(String::new(), |s| model::size(s.uploaded)),
                    ],
                    // i18n: sent divided by received
                    vec![t("Share ratio"), ratio],
                    // i18n: label before the folder the torrent is saved in
                    vec![t("Saved in"), folder],
                    vec![
                        // i18n: noun: sharing the torrent with others after it is downloaded
                        t("Sharing"),
                        t("Stops when the download finishes"),
                    ],
                ];
                table(
                    ui,
                    p,
                    o,
                    "t-general",
                    &[
                        (25.0, crate::i18n::mark("Property")),
                        (130.0, crate::i18n::mark("Value")),
                    ],
                    &[125.0],
                    &rows,
                );
            }
        }
    }
}

/// The segment bar as a piece map: a column is filled when the pieces under it are there.
fn piece_bar(p: &egui::Painter, o: Pos2, pieces: &[bool]) {
    let (x0, y0) = (o.x + 17.0, o.y + 249.0);
    let border = rgb(0x65, 0x6b, 0x7b);
    fill(p, x0, y0, x0 + 512.0, y0, border);
    fill(p, x0, y0 + 14.0, x0 + 512.0, y0 + 14.0, border);
    fill(p, x0, y0, x0, y0 + 14.0, border);
    fill(p, x0 + 512.0, y0, x0 + 512.0, y0 + 14.0, border);
    let ix = o.x + 18.0;
    // Runs of equal columns, painted as one rectangle per row.
    let have = |c: usize| {
        if pieces.is_empty() {
            return false;
        }
        let a = c * pieces.len() / 511;
        let b = ((c + 1) * pieces.len() / 511).max(a + 1).min(pieces.len());
        pieces[a..b].iter().all(|x| *x)
    };
    let mut c = 0usize;
    while c < 511 {
        let on = have(c);
        let mut e = c;
        while e + 1 < 511 && have(e + 1) == on {
            e += 1;
        }
        for (row, cols) in SEG_ROWS.iter().enumerate() {
            let v = if on { cols.1 } else { cols.0 };
            let y = y0 + 1.0 + row as f32;
            fill(p, ix + c as f32, y, ix + e as f32, y, hex(v));
        }
        c = e + 1;
    }
}

/// The Pieces tab's key, at the header's right: have / some / missing.
fn piece_legend(p: &egui::Painter, o: Pos2) {
    let (done, partial, missing) = piece_colors();
    let mut x = o.x + 505.0;
    for (c, label) in [
        // i18n: legend: pieces not downloaded yet
        (missing, crate::i18n::mark("Missing")),
        // i18n: legend: pieces partly downloaded
        (partial, crate::i18n::mark("Some")),
        // i18n: legend: pieces downloaded
        (done, crate::i18n::mark("Have")),
    ] {
        let label = crate::i18n::t(label);
        x -= winui::text_width(p, label);
        text(p, x, o.y + 290.0, label, winui::TEXT());
        x -= 10.0;
        p.rect_filled(
            winui::rect(egui::Rect::from_min_size(
                egui::pos2(x, o.y + 282.0),
                egui::vec2(7.0, 7.0),
            )),
            1.0,
            c,
        );
        x -= 14.0;
    }
}

/// Pieces there, partly there (a square standing for several), missing.
fn piece_colors() -> (Color32, Color32, Color32) {
    if crate::theme::dark() {
        (
            rgb(0x2e, 0x9d, 0x52),
            rgb(0x3a, 0x6e, 0x4b),
            rgb(0x45, 0x45, 0x45),
        )
    } else {
        (
            rgb(0x2e, 0x9d, 0x52),
            rgb(0x9c, 0xd2, 0xae),
            rgb(0xd6, 0xd6, 0xd6),
        )
    }
}

/// The Pieces tab: one square per piece (or per group of pieces when there are many).
fn piece_grid(p: &egui::Painter, o: Pos2, pieces: &[bool]) {
    if pieces.is_empty() {
        return;
    }
    let (left, top, w, h) = (o.x + 22.0, o.y + 302.0, 498.0, 94.0);
    let cols = (w / 7.0) as usize;
    let rows = (h / 7.0) as usize;
    let cells = (cols * rows).min(pieces.len());
    let (done, partial, missing) = piece_colors();
    for k in 0..cells {
        let a = k * pieces.len() / cells;
        let b = ((k + 1) * pieces.len() / cells)
            .max(a + 1)
            .min(pieces.len());
        let got = pieces[a..b].iter().filter(|x| **x).count();
        let c = if got == b - a {
            done
        } else if got > 0 {
            partial
        } else {
            missing
        };
        let (x, y) = (
            left + (k % cols) as f32 * 7.0,
            top + (k / cols) as f32 * 7.0,
        );
        p.rect_filled(
            winui::rect(egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(6.0, 6.0),
            )),
            1.0,
            c,
        );
    }
}

/// A table in the connections table's frame: headers at x, column lines at `seps`, rows of
/// text cut to their column (whole on hover), scrolled by the wheel.
fn table(
    ui: &mut Ui,
    p: &egui::Painter,
    o: Pos2,
    id: &str,
    heads: &[(f32, &str)],
    seps: &[f32],
    rows: &[Vec<String>],
) {
    let outer = r(o, 17.0, 273.0, 513.0, 128.0);
    winui::solid(p, outer, rgb(0x82, 0x87, 0x90));
    let dark = crate::theme::dark();
    let (bg, head, line) = if dark {
        (winui::BG(), rgb(0x19, 0x19, 0x19), rgb(0x63, 0x63, 0x63))
    } else {
        (
            rgb(0xff, 0xff, 0xff),
            rgb(0xf3, 0xf3, 0xf3),
            rgb(0xe5, 0xe5, 0xe5),
        )
    };
    fill(p, o.x + 18.0, o.y + 274.0, o.x + 528.0, o.y + 399.0, bg);
    fill(p, o.x + 19.0, o.y + 275.0, o.x + 527.0, o.y + 298.0, head);
    for x in seps {
        fill(p, o.x + x, o.y + 275.0, o.x + x, o.y + 298.0, line);
    }
    for (x, h) in heads {
        text(p, o.x + x, o.y + 290.0, crate::i18n::t(h), winui::TEXT());
    }
    let visible = 6usize;
    // Per window: each progress window scrolls its own tables.
    let key = egui::Id::unique(("torrent-table", id, ui.ctx().viewport_id()));
    let max_first = rows.len().saturating_sub(visible);
    let mut first = ui
        .ctx()
        .data(|d| d.get_temp::<usize>(key))
        .unwrap_or(0)
        .min(max_first);
    let body = r(o, 19.0, 299.0, 508.0, 100.0);
    if ui.rect_contains_pointer(body) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy < 0.0 {
            first = (first + 1).min(max_first);
        } else if dy > 0.0 {
            first = first.saturating_sub(1);
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, first));
    }
    let clip = p.with_clip_rect(body);
    let right_of = |c: usize| seps.get(c).map_or(o.x + 520.0, |x| o.x + x - 4.0);
    for (k, row) in rows.iter().skip(first).take(visible + 1).enumerate() {
        let top = o.y + 299.0 + 17.0 * k as f32;
        for (c, cell) in row.iter().enumerate() {
            let x = heads.get(c).map_or(o.x + 25.0, |h| o.x + h.0);
            let (rect, cut) =
                winui::text_fit(&clip, x, top + 12.0, cell, winui::TEXT(), right_of(c));
            // Cells hold info hashes, folders, trackers, peers: copyable (whole on hover).
            winui::copyable(ui, rect.intersect(body), (id, first + k, c), cell, cut);
        }
    }
    if rows.len() > visible {
        let track = 124.0 - 17.0;
        let len = (track * visible as f32 / rows.len() as f32).max(17.0);
        let y = o.y + 292.0 + (track - len) * first as f32 / max_first.max(1) as f32;
        let thumb = if dark {
            rgb(0x95, 0x95, 0x95)
        } else {
            rgb(0x85, 0x85, 0x85)
        };
        fill(p, o.x + 524.0, y, o.x + 525.0, y + len - 1.0, thumb);
    }
}

/// The custom-drawn segment bar (spec "Segment bar"): 13-row gradients, red start columns.
fn segment_bar(p: &egui::Painter, o: Pos2, item: &Item, snap: Option<&Snapshot>) {
    let (x0, y0) = (o.x + 17.0, o.y + 249.0);
    let border = rgb(0x65, 0x6b, 0x7b);
    fill(p, x0, y0, x0 + 512.0, y0, border);
    fill(p, x0, y0 + 14.0, x0 + 512.0, y0 + 14.0, border);
    fill(p, x0, y0, x0, y0 + 14.0, border);
    fill(p, x0 + 512.0, y0, x0 + 512.0, y0 + 14.0, border);
    for (x, y) in [
        (x0, y0),
        (x0 + 512.0, y0),
        (x0, y0 + 14.0),
        (x0 + 512.0, y0 + 14.0),
    ] {
        px(p, x, y, rgb(0xa2, 0xa5, 0xaf));
    }
    // Paint a run of columns [a, b] with one of the three row profiles.
    let run = |a: f32, b: f32, which: usize| {
        if b < a {
            return;
        }
        for (row, cols) in SEG_ROWS.iter().enumerate() {
            let c = match which {
                0 => cols.0,
                1 => cols.1,
                _ => cols.2,
            };
            fill(
                p,
                a,
                y0 + 1.0 + row as f32,
                b,
                y0 + 1.0 + row as f32,
                hex(c),
            );
        }
    };
    let ix = o.x + 18.0;
    run(ix, ix + 510.0, 0);
    let Some(total) = item.size.filter(|t| *t > 0) else {
        return;
    };
    let col = |v: u64| (v as f64 / total as f64 * 511.0).floor() as f32;
    let segments: Vec<(u64, u64)> = match snap {
        Some(s) if !s.segments.is_empty() => s
            .segments
            .iter()
            .map(|g| (g.start, g.cursor - g.start))
            .collect(),
        _ => vec![(0, item.downloaded)],
    };
    for (start, done) in segments {
        let sx = ix + col(start);
        let n = col(done);
        if n > 1.0 {
            run(sx + 1.0, (sx + n - 1.0).min(ix + 510.0), 1);
        }
        run(sx, sx, 2);
    }
}

/// The connections table (spec "Connections table"): N. · Downloaded · Info.
fn connections(ui: &mut Ui, p: &egui::Painter, o: Pos2, snap: Option<&Snapshot>) {
    let outer = r(o, 17.0, 273.0, 513.0, 128.0);
    winui::solid(p, outer, rgb(0x82, 0x87, 0x90));
    fill(
        p,
        o.x + 18.0,
        o.y + 274.0,
        o.x + 528.0,
        o.y + 399.0,
        // Light: the list background is mauve #beb4be (light-dialogs.md).
        if crate::theme::dark() {
            winui::BG()
        } else {
            rgb(0xbe, 0xb4, 0xbe)
        },
    );
    fill(
        p,
        o.x + 19.0,
        o.y + 275.0,
        o.x + 510.0,
        o.y + 298.0,
        if crate::theme::dark() {
            rgb(0x19, 0x19, 0x19)
        } else {
            rgb(0xff, 0xff, 0xff)
        },
    );
    for x in [48.0, 148.0, 498.0] {
        fill(
            p,
            o.x + x,
            o.y + 275.0,
            o.x + x,
            o.y + 298.0,
            if crate::theme::dark() {
                rgb(0x63, 0x63, 0x63)
            } else {
                rgb(0xe5, 0xe5, 0xe5)
            },
        );
    }
    // Column titles end in "..." before their column's line, whole on hover; a long "N." starts
    // at the list's edge.
    let head = |x: f32, right: f32, label: &str| {
        let (rc, cut) = winui::text_fit(p, o.x + x, o.y + 290.0, label, winui::TEXT(), o.x + right);
        if cut {
            winui::full_on_hover(ui, rc, ("connections-head", x as i32), label);
        }
    };
    // i18n: column title: connection number (abbreviation of Number)
    let number = crate::i18n::t("N.");
    let x = if winui::text_width(p, number) > 21.0 {
        21.0
    } else {
        25.0
    };
    head(x, 46.0, number);
    // i18n: column title: bytes this connection received
    head(55.0, 146.0, crate::i18n::t("Downloaded"));
    // i18n: column title: what the connection is doing
    head(155.0, 496.0, crate::i18n::t("Info"));
    // Body column lines: dark mode only.
    let lines: &[(f32, f32)] = if crate::theme::dark() {
        &[(47.0, 48.0), (147.0, 148.0), (497.0, 498.0)]
    } else {
        &[]
    };
    for &(a, b) in lines {
        fill(
            p,
            o.x + a,
            o.y + 299.0,
            o.x + a,
            o.y + 398.0,
            rgb(0x57, 0x57, 0x57),
        );
        fill(
            p,
            o.x + b,
            o.y + 299.0,
            o.x + b,
            o.y + 398.0,
            rgb(0x35, 0x3e, 0x48),
        );
    }
    // Listed by connection number, like IDM (the engine reports them in arrival order).
    let mut conns: Vec<_> = snap
        .map(|s| s.connections.iter().collect())
        .unwrap_or_default();
    conns.sort_by_key(|c| c.id);
    let key = egui::Id::unique(("conn-scroll", ui.ctx().viewport_id()));
    let visible = 6usize;
    let max_first = conns.len().saturating_sub(visible);
    let mut first = ui
        .ctx()
        .data(|d| d.get_temp::<usize>(key))
        .unwrap_or(0)
        .min(max_first);
    let body = r(o, 19.0, 299.0, 492.0, 100.0);
    if ui.rect_contains_pointer(body) {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy < 0.0 {
            first = (first + 1).min(max_first);
        } else if dy > 0.0 {
            first = first.saturating_sub(1);
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, first));
    }
    let clip = p.with_clip_rect(body);
    for (k, c) in conns.iter().skip(first).take(visible + 1).enumerate() {
        let top = o.y + 299.0 + 17.0 * k as f32;
        winui::text_right(
            &clip,
            o.x + 33.0,
            top + 12.0,
            &(c.id + 1).to_string(),
            winui::TEXT(),
        );
        text(
            &clip,
            o.x + 56.0,
            top + 12.0,
            &size3(c.downloaded),
            winui::TEXT(),
        );
        let info = match &c.status {
            ConnStatus::Connecting => crate::i18n::t("Connecting...").to_owned(),
            ConnStatus::Receiving => crate::i18n::t("Receiving data...").to_owned(),
            ConnStatus::Retrying(e) => crate::i18n::fmt(
                // i18n: a connection's state: it failed and tries again, with the reason
                crate::i18n::t("Retrying: {error}"),
                &[("error", &crate::errors::describe(e))],
            ),
            ConnStatus::Finished => String::new(),
            ConnStatus::Failed(e) => crate::i18n::fmt(
                crate::i18n::t("Error: {error}"),
                &[("error", &crate::errors::describe(e))],
            ),
            ConnStatus::Retired { host, cap } => crate::i18n::fmt(
                // i18n: a connection closed because the server allows only that many
                &crate::i18n::tn(
                    u64::from(*cap),
                    "{host} allows {n} connection",
                    "{host} allows {n} connections",
                ),
                &[("host", &crate::i18n::ltr(host))],
            ),
        };
        // The Info column ends at the scrollbar.
        let (rect, cut) = winui::text_fit(
            &clip,
            o.x + 155.0,
            top + 12.0,
            &info,
            winui::TEXT(),
            o.x + 507.0,
        );
        if cut {
            winui::full_on_hover(ui, rect.intersect(body), ("conn", c.id), &info);
        }
    }
    // Thin scrollbar: #171717 track, 2 px #959595 thumb (light: #f0f0f0 / #858585).
    fill(
        p,
        o.x + 511.0,
        o.y + 275.0,
        o.x + 527.0,
        o.y + 398.0,
        if crate::theme::dark() {
            rgb(0x17, 0x17, 0x17)
        } else {
            rgb(0xf0, 0xf0, 0xf0)
        },
    );
    if conns.len() > visible {
        let track = 124.0 - 17.0;
        let len = (track * visible as f32 / conns.len() as f32).max(17.0);
        let top = o.y + 292.0 + (track - len) * first as f32 / max_first.max(1) as f32;
        fill(
            p,
            o.x + 519.0,
            top,
            o.x + 520.0,
            top + len - 1.0,
            if crate::theme::dark() {
                rgb(0x95, 0x95, 0x95)
            } else {
                rgb(0x85, 0x85, 0x85)
            },
        );
    }
}

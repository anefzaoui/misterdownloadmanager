//! The main window in a phone's layout (Android, on a screen narrower than [`PHONE_WIDTH`]): one
//! column, the download list first. A bar at the top names the view and opens the categories (a
//! drawer) and the rest of the commands (a menu); the list's rows are tall enough for a finger,
//! each with its name, its progress and what it is doing; a bar at the bottom holds the commands
//! used most. Everything a command does is the desktop's ([`App::run`]): only the layout differs.
//!
//! `MDM_PHONE=1` lays the main window out so on a desktop too, to look at it (a test copy's
//! window, as `MDM_ZOOM`).

use super::*;

/// Narrower than this (in points), the main window takes the phone's layout.
pub const PHONE_WIDTH: f32 = 600.0;
/// The top and bottom bars' height: Android's 48 dp touch target and its margin.
const BAR_H: f32 = 56.0;
/// A row of the list: two lines of text and a progress bar, with room for a finger.
const ROW_H: f32 = 80.0;
/// The home screen's summary and its views' chips, under the top bar ([`App::home_header`]).
const HEADER_H: f32 = 116.0;
/// The space around the bars' and rows' contents.
const PAD: f32 = 16.0;

/// The width a sheet's contents can use ([`sheet`]): the window's.
pub fn sheet_width(ctx: &egui::Context) -> f32 {
    ctx.content_rect().width()
}

/// The height a sheet's contents can use ([`sheet`]): the window's, less its top bar.
pub fn sheet_height(ctx: &egui::Context) -> f32 {
    ctx.content_rect().height() - BAR_H
}

/// A window on a phone (a dialog, a download's progress): a page over the whole main window,
/// as Android's apps show one, with its title in a top bar whose × closes it as the desktop
/// window's × does. It isn't an egui window: those are placed and framed by egui, can be dragged,
/// and give a screen reader's root to whichever was drawn last. `body` lays out the page under
/// the bar, in its own width ([`sheet_width`]). Returns its answer, and whether × was tapped.
pub(crate) fn sheet<R>(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    bg: egui::Color32,
    body: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let room = ctx.content_rect();
    // Opening, it comes in from the trailing side and fades in, as Android's pages do.
    let shown = entrance(ctx, id, 0.25);
    let from = room.width() * 0.3 * (1.0 - shown);
    ctx.set_transform_layer(
        egui::LayerId::new(egui::Order::Foreground, id),
        egui::emath::TSTransform::from_translation(egui::vec2(
            if winui::rtl() { -from } else { from },
            0.0,
        )),
    );
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(room.min)
        .movable(false)
        .constrain(false)
        .fade_in(true)
        .show(ctx, |ui| {
            ui.set_min_size(room.size());
            ui.set_max_size(room.size());
            let full = room;
            // The page takes the taps that miss its controls (the list under it gets none), and
            // is a window for a screen reader, named by its title.
            let page = ui.interact(full, id.with("page"), egui::Sense::click());
            let name = title.to_owned();
            page.widget_info(move || egui::WidgetInfo::labeled(egui::Role::Window, true, &name));
            ui.painter().rect_filled(full, 0.0, bg);
            let _surface = winui::Surface::enter(full.min.x, full.max.x);
            let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), BAR_H));
            let p = ui.painter();
            p.rect_filled(winui::rect(top), 0.0, colors().menubar);
            p.hline(
                winui::rect(top).x_range(),
                top.max.y - 0.5,
                egui::Stroke::new(1.0, colors().toolbar_line),
            );
            let close_at = egui::Rect::from_min_size(top.min, egui::vec2(BAR_H, BAR_H));
            // A page within the window ([`sheet_back`]): ←, which goes back (as Back does).
            let back = ctx
                .data_mut(|d| d.remove_temp::<bool>(sheet_back_key()))
                .is_some();
            let tapped = bar_button(
                ui,
                close_at,
                id.with(("close", back)),
                if back { Glyph::Back } else { Glyph::Close },
            );
            if tapped && back {
                ui.ctx().input_mut(|i| {
                    i.events.push(egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                });
            }
            let closed = tapped && !back;
            let tx = close_at.max.x + 4.0;
            let shown = elide(ui, title, font_bold(18.0), top.max.x - PAD - tx);
            ui.painter().text(
                winui::rect(egui::Rect::from_min_size(
                    pos2(tx, top.center().y),
                    egui::Vec2::ZERO,
                ))
                .min,
                if winui::rtl() {
                    egui::Align2::RIGHT_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                shown,
                font_bold(18.0),
                colors().text,
            );
            let below = egui::Rect::from_min_max(pos2(full.min.x, top.max.y), full.max);
            let mut rest = ui.new_child(egui::UiBuilder::new().max_rect(below));
            (body(&mut rest), closed)
        })
        .inner
}

/// How far the entrance of what `id` names has come, from 0 to 1, eased out (fast, then
/// settling) over `secs`: counted from the first pass it is drawn in after one it wasn't, so it
/// plays each time it opens. Asks for the frames it needs.
fn entrance(ctx: &egui::Context, id: egui::Id, secs: f64) -> f32 {
    let now = ctx.input(|i| i.time);
    let pass = ctx.cumulative_pass_nr();
    let key = id.with("entrance");
    let since = match ctx.data(|d| d.get_temp::<(f64, u64)>(key)) {
        Some((since, last)) if last + 1 >= pass => since,
        _ => now,
    };
    ctx.data_mut(|d| d.insert_temp(key, (since, pass)));
    let t = ((now - since) / secs).clamp(0.0, 1.0) as f32;
    if t < 1.0 {
        ctx.request_repaint();
    }
    1.0 - (1.0 - t).powi(3)
}

/// A big button's height on a phone's page ([`button_rects`]): 40 points, a finger's.
pub(crate) const BUTTON_H: f32 = 40.0;

/// Where a phone's page puts its `n` buttons, from English y `top` of a page `width` wide (both
/// relative to the page's origin `o`, as `winui::r` takes them): big buttons across the page,
/// the first alone on its row when `lead` (the page's main action), the rest two by two. The
/// screen rects in the buttons' order, and the height the rows take.
pub(crate) fn button_rects(
    o: Pos2,
    top: f32,
    width: f32,
    n: usize,
    lead: bool,
) -> (Vec<egui::Rect>, f32) {
    const MARGIN: f32 = 11.0;
    const GAP: f32 = 8.0;
    let full = width - 2.0 * MARGIN;
    let half = (full - GAP) / 2.0;
    let mut rects = Vec::with_capacity(n);
    let mut y = top;
    let mut k = 0;
    if lead && n > 0 {
        rects.push(winui::r(o, MARGIN, y, full, BUTTON_H));
        y += BUTTON_H + GAP;
        k = 1;
    }
    while k < n {
        if k + 1 < n {
            rects.push(winui::r(o, MARGIN, y, half, BUTTON_H));
            rects.push(winui::r(o, MARGIN + half + GAP, y, half, BUTTON_H));
            k += 2;
        } else {
            rects.push(winui::r(o, MARGIN, y, full, BUTTON_H));
            k += 1;
        }
        y += BUTTON_H + GAP;
    }
    (rects, (y - GAP - top).max(0.0))
}

/// How much taller a window is on a phone for `rows` rows of big buttons ([`button_rects`])
/// where the desktop has one row of 21 pt buttons; nothing elsewhere.
pub(crate) fn phone_buttons_extra(ctx: &egui::Context, rows: usize) -> f32 {
    if active(ctx) {
        rows as f32 * (BUTTON_H + 8.0) - 21.0
    } else {
        0.0
    }
}

/// A window's two buttons: big across its `width` from English y `top` on a phone ([`button_rects`]),
/// the desktop's `a` and `b` elsewhere.
pub(crate) fn pair_or(
    ctx: &egui::Context,
    o: Pos2,
    top: f32,
    width: f32,
    a: egui::Rect,
    b: egui::Rect,
) -> (egui::Rect, egui::Rect) {
    if active(ctx) {
        let (r, _) = button_rects(o, top, width, 2, false);
        (r[0], r[1])
    } else {
        (a, b)
    }
}

/// The width a window lays itself out in: the phone's page's on a phone (its controls take the
/// room it has), `desktop` elsewhere.
pub(crate) fn layout_width(ctx: &egui::Context, desktop: f32) -> f32 {
    if active(ctx) {
        sheet_width(ctx)
    } else {
        desktop
    }
}

/// A page laid out wider than the phone (a desktop window's layout: Download video, the
/// Scheduler...) drawn smaller to fit its width, so every control and button is on the screen:
/// in a layer of its own, scaled (input too) from the top left of `ui`'s room. `width` is the
/// layout's; `body` gets a room that wide, and as much taller as it was made smaller.
pub(crate) fn fitted<R>(
    ui: &mut Ui,
    id: egui::Id,
    width: f32,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    let view = ui.max_rect();
    let s = (view.width() / width).min(1.0);
    if s > 0.995 {
        return body(ui);
    }
    let layer = egui::LayerId::new(ui.layer_id().order, id.with("fitted"));
    let parent = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or(egui::emath::TSTransform::IDENTITY);
    let fit = egui::emath::TSTransform::new(view.min.to_vec2() * (1.0 - s), s);
    let to_screen = parent * fit;
    ui.ctx().set_transform_layer(layer, to_screen);
    ui.ctx().set_sublayer(ui.layer_id(), layer);
    let room = egui::Rect::from_min_size(view.min, view.size() / s);
    let mut inner = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(room));
    inner.set_clip_rect(room);
    ui.ctx().data_mut(|d| d.insert_temp(fit_key(), to_screen));
    let out = body(&mut inner);
    ui.ctx()
        .data_mut(|d| d.remove::<egui::emath::TSTransform>(fit_key()));
    out
}

fn fit_key() -> egui::Id {
    egui::Id::unique("phone-fitted")
}

/// `p`, a point of the page being drawn, on the screen: scaled with it while it is drawn
/// smaller ([`fitted`]); for what is drawn in a layer of its own over it (a drop-down's list).
pub(crate) fn to_screen(ctx: &egui::Context, p: Pos2) -> Pos2 {
    ctx.data(|d| d.get_temp::<egui::emath::TSTransform>(fit_key()))
        .map_or(p, |t| t * p)
}

fn sheet_back_key() -> egui::Id {
    egui::Id::unique("phone-sheet-back")
}

/// The next [`sheet`] drawn shows ← instead of ×: a page within its window (an Options
/// section's), which goes back to where it came from (it gets Escape, as from Back).
pub(crate) fn sheet_back(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(sheet_back_key(), true));
}

/// Whether the main window takes the phone's layout now.
pub fn active(ctx: &egui::Context) -> bool {
    (cfg!(target_os = "android") && ctx.content_rect().width() < PHONE_WIDTH)
        || std::env::var_os("MDM_PHONE").is_some()
}

fn colors() -> &'static crate::palette::Palette {
    crate::theme::colors()
}

/// The phone's text sizes: Android's body (16) and caption (13) sizes, in MDM's own font.
fn font(size: f32) -> egui::FontId {
    egui::FontId::new(size, winui::tahoma().family)
}

fn font_bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, winui::tahoma_bold().family)
}

/// Where the drawer is (kept with the window's memory, not in the app's settings).
fn drawer_id() -> egui::Id {
    egui::Id::unique("phone-drawer")
}

impl App {
    /// The main window in the phone's layout ([`active`]).
    pub(super) fn paint_phone(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let _surface = winui::Surface::enter(full.min.x, full.max.x);
        let mut chosen: Option<(Cmd, Option<u64>)> = None;
        ui.painter().rect_filled(full, 0.0, colors().list);

        let top = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), BAR_H));
        let bottom = egui::Rect::from_min_max(pos2(full.min.x, full.max.y - BAR_H), full.max);
        // Under the top bar: how the downloads go, and the views at a tap ([`App::home_header`]).
        let header = egui::Rect::from_min_size(
            pos2(full.min.x, top.max.y),
            egui::vec2(full.width(), HEADER_H),
        );
        let body = egui::Rect::from_min_max(
            pos2(full.min.x, header.max.y),
            pos2(full.max.x, bottom.min.y),
        );
        if let Some(view) = self.home_header(ui, header) {
            self.view = view;
        }

        // The list.
        let (ids, rows) = self.list_rows();
        self.selection.retain(&ids);
        // Selection mode (a long press starts it), as Android's lists have it; out of it nothing
        // stays picked: a tap opens, and the commands act on what they name.
        let mut picking = ctx
            .data(|d| d.get_temp::<bool>(picking_id()))
            .unwrap_or(false);
        if picking && self.selection.ids().is_empty() {
            picking = false;
        }
        if !picking && !self.selection.ids().is_empty() && self.popup.is_none() {
            self.selection.apply(crate::selection::Pick::Clear, &ids);
        }
        let (selected, _) = self.selection.rows(&ids);
        if self.list_view != self.view {
            self.list.scroll = 0.0;
            self.list_view = self.view;
        }
        // Each row's own button: Pause while it runs, Resume when it can go on.
        let states: Vec<RowState> = ids
            .iter()
            .map(|id| match self.items.iter().find(|i| i.id == *id) {
                Some(i) if i.live.is_some() => RowState::Running,
                Some(i) if i.can_start() => RowState::Stopped,
                _ => RowState::Done,
            })
            .collect();
        let mut list = ui.new_child(egui::UiBuilder::new().max_rect(body));
        let picked = phone_list(
            &mut list,
            &rows,
            &selected,
            picking,
            &states,
            &mut self.icons,
        );
        if let Some((i, held)) = picked.tapped {
            if picking {
                // A tap (or a long press) adds the row or takes it away.
                self.selection
                    .apply(crate::selection::Pick::Toggle(i), &ids);
            } else if held {
                // A long press picks the row and starts selection mode.
                self.selection.apply(crate::selection::Pick::Only(i), &ids);
                picking = true;
            } else {
                // A tap opens it, as Android's lists do: a download that isn't done shows its
                // details (its progress page), a finished one opens its file.
                self.open_item(ids[i]);
            }
        }
        if let Some((i, at)) = picked.menu {
            // A right-click (the desktop's preview): its menu, as on the desktop.
            if !selected.get(i).copied().unwrap_or(false) {
                self.selection.apply(crate::selection::Pick::Only(i), &ids);
            }
            self.open_popup_asked(Popup::Item(Some(ids[i])), at, &ctx);
        }
        if let Some((i, pause)) = picked.action {
            chosen = Some((if pause { Cmd::Stop } else { Cmd::Resume }, Some(ids[i])));
        }
        // Back ends selection mode (a menu or the drawer over it closes first).
        let overlay = self.popup.is_some()
            || ctx
                .data(|d| d.get_temp::<bool>(drawer_id()))
                .unwrap_or(false);
        if picking
            && !overlay
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            picking = false;
            self.selection.apply(crate::selection::Pick::Clear, &ids);
        }
        if ids.is_empty()
            && let Some(cmd) = self.empty_state(ui, winui::rect(body))
        {
            chosen = Some((cmd, None));
        }

        // The top bar: the categories, the view's name, the rest of the commands.
        let p = ui.painter();
        p.rect_filled(winui::rect(top), 0.0, colors().menubar);
        p.hline(
            winui::rect(top).x_range(),
            top.max.y - 0.5,
            egui::Stroke::new(1.0, colors().toolbar_line),
        );
        let side = BAR_H;
        let menu_at = egui::Rect::from_min_size(top.min, egui::vec2(side, side));
        let more_at =
            egui::Rect::from_min_size(pos2(top.max.x - side, top.min.y), egui::vec2(side, side));
        let all_at = more_at.translate(egui::vec2(-side, 0.0));
        let picked_n = self.selection.ids().len() as u64;
        // Search: the file names that have what is typed (the desktop's File Name filter).
        let mut searching = !picking
            && ctx
                .data(|d| d.get_temp::<bool>(search_id()))
                .unwrap_or(false);
        let (lead, title) = if picking {
            (
                Glyph::Close,
                crate::i18n::tn(picked_n, "{n} selected", "{n} selected"),
            )
        } else if searching {
            (Glyph::Close, String::new())
        } else {
            (Glyph::Menu, self.view_title())
        };
        if bar_button(ui, menu_at, ("phone-lead", picking, searching), lead) {
            if picking {
                picking = false;
                self.selection.apply(crate::selection::Pick::Clear, &ids);
            } else if searching {
                searching = false;
            } else {
                ctx.data_mut(|d| d.insert_temp(drawer_id(), true));
            }
        }
        let tx = menu_at.max.x + 4.0;
        if searching {
            let mut query = match self.list.filters.first() {
                Some(Some(list::Filter::Text(t))) => t.clone(),
                _ => String::new(),
            };
            let field = winui::rect(egui::Rect::from_min_max(
                pos2(tx, top.min.y + 10.0),
                pos2(top.max.x - PAD, top.max.y - 10.0),
            ));
            let edit = ui.put(
                field,
                egui::TextEdit::singleline(&mut query)
                    .font(font(18.0))
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::i18n::t("Find"))
                    .id(egui::Id::unique("phone-search-box")),
            );
            if ctx
                .data_mut(|d| d.remove_temp::<bool>(search_id().with("focus")))
                .is_some()
            {
                edit.request_focus();
            }
            ui.painter().hline(
                field.x_range(),
                field.max.y,
                egui::Stroke::new(1.0, colors().selection),
            );
            set_name_filter(&mut self.list, &query);
        } else {
            let title_room = all_at.min.x - PAD - tx;
            let title = elide(ui, &title, font_bold(18.0), title_room);
            lead_text(
                ui,
                pos2(tx, top.center().y),
                &title,
                font_bold(18.0),
                colors().text,
            );
        }
        if picking && bar_button(ui, all_at, "phone-select-all", Glyph::SelectAll) {
            self.selection.apply(crate::selection::Pick::All, &ids);
        }
        // Out of selection and search: search and sort (the desktop's Arrange files).
        if !picking && !searching {
            if bar_button(ui, all_at, "phone-search", Glyph::Search) {
                searching = true;
                ctx.data_mut(|d| d.insert_temp(search_id().with("focus"), true));
            }
            if bar_button(ui, more_at, "phone-sort", Glyph::Sort) {
                self.open_popup_asked(Popup::PhoneMore, winui::rect(more_at).left_bottom(), &ctx);
                if let Some((_, _, serial)) = self.popup {
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::unique(("phone-menu-path", serial)), vec![0usize]);
                        d.insert_temp(egui::Id::unique(("phone-menu-base", serial)), 1usize);
                    });
                }
            }
        }
        // ⋮ in selection mode: the picked downloads' menu (the rest of the commands are at the
        // drawer's bottom).
        if picking && bar_button(ui, more_at, "phone-more", Glyph::More) {
            let at = winui::rect(more_at).left_bottom();
            // (A picked download's menu acts on every one picked.)
            self.open_popup_asked(Popup::Item(self.selection.ids().first().copied()), at, &ctx);
        }
        // Back ends the search (a menu over it closes first), and the search ends with it.
        if searching
            && self.popup.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            searching = false;
        }
        if !searching
            && ctx
                .data(|d| d.get_temp::<bool>(search_id()))
                .unwrap_or(false)
        {
            set_name_filter(&mut self.list, "");
        }
        ctx.data_mut(|d| d.insert_temp(search_id(), searching));
        ctx.data_mut(|d| d.insert_temp(picking_id(), picking));

        // The bottom bar: the commands used most; in selection mode, those for the downloads
        // picked.
        let picked_items = self.chosen(None);
        let running = picked_items.iter().any(|i| i.live.is_some());
        let resumable = picked_items.iter().any(|i| i.can_start());
        let deletable = picked_items.iter().any(|i| i.live.is_none());
        let any_running = self.items.iter().any(|i| i.live.is_some());
        let buttons: Vec<(Icon, &str, bool, Cmd)> = if picking {
            vec![
                (
                    Icon::Resume,
                    crate::i18n::tc("toolbar", "Resume"),
                    resumable,
                    Cmd::Resume,
                ),
                (
                    Icon::Stop,
                    crate::i18n::tc("toolbar", "Stop"),
                    running,
                    Cmd::Stop,
                ),
                (
                    Icon::Delete,
                    crate::i18n::tc("toolbar", "Delete"),
                    deletable,
                    Cmd::Remove,
                ),
            ]
        } else {
            vec![
                (
                    Icon::AddUrl,
                    crate::i18n::tc("toolbar", "Add URL"),
                    true,
                    Cmd::AddUrl,
                ),
                (
                    Icon::HelperVideo,
                    crate::i18n::t("Video"),
                    true,
                    Cmd::VideoPage,
                ),
                (
                    Icon::StopAll,
                    crate::i18n::tc("toolbar", "Stop All"),
                    any_running,
                    Cmd::StopAll,
                ),
                (
                    Icon::Scheduler,
                    crate::i18n::tc("toolbar", "Scheduler"),
                    true,
                    Cmd::Scheduler,
                ),
            ]
        };
        let p = ui.painter();
        p.rect_filled(winui::rect(bottom), 0.0, colors().menubar);
        p.hline(
            winui::rect(bottom).x_range(),
            bottom.min.y + 0.5,
            egui::Stroke::new(1.0, colors().toolbar_line),
        );
        let w = bottom.width() / buttons.len() as f32;
        for (k, (icon, label, enabled, cmd)) in buttons.into_iter().enumerate() {
            let at = egui::Rect::from_min_size(
                pos2(bottom.min.x + w * k as f32, bottom.min.y),
                egui::vec2(w, BAR_H),
            );
            if bottom_button(ui, at, icon, label, enabled) {
                chosen = Some((cmd, None));
            }
        }

        // The drawer over the list: drawn while it slides in and out.
        let drawer_open = ctx
            .data(|d| d.get_temp::<bool>(drawer_id()))
            .unwrap_or(false);
        let slide = ctx.animate_bool_with_time(drawer_id().with("slide"), drawer_open, 0.22);
        if slide > 0.0 {
            let shown = 1.0 - (1.0 - slide).powi(3);
            let (view, cmd, sub) = self.drawer(ui, full, shown, drawer_open);
            if let Some(view) = view {
                self.view = view;
            }
            if let Some(cmd) = cmd {
                chosen = Some((cmd, None));
            }
            // A submenu (Start queue, Stop queue): the menu sheet, open on it.
            if let Some(k) = sub {
                self.open_popup_asked(Popup::PhoneMore, full.center(), &ctx);
                if let Some((_, _, serial)) = self.popup {
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::unique(("phone-menu-path", serial)), vec![k]);
                        d.insert_temp(egui::Id::unique(("phone-menu-base", serial)), 1usize);
                    });
                }
            }
        }

        // A row's menu, the "more" menu, a node's: the desktop's entries, in a sheet from the
        // bottom with rows a finger can hit ([`menu_sheet`]).
        if let Some((kind, _, serial)) = self.popup {
            // Closing, it slides down for a moment (taking no touches); what was chosen runs
            // at once.
            let leave_key = egui::Id::unique(("phone-menu-leave", serial));
            let now = ctx.input(|i| i.time);
            let leaving = ctx
                .data(|d| d.get_temp::<f64>(leave_key))
                .map(|since| ((now - since) / MENU_LEAVE) as f32);
            if leaving.is_some_and(|t| t >= 1.0) {
                self.popup = None;
                ctx.data_mut(|d| d.remove::<f64>(leave_key));
                ctx.request_repaint();
            } else {
                let (entries, target) = self.popup_entries(kind);
                let leave = |ctx: &egui::Context| {
                    ctx.data_mut(|d| d.insert_temp(leave_key, now));
                    ctx.request_repaint();
                };
                match menu_sheet(ui, full, &entries, serial, leaving.unwrap_or(0.0)) {
                    PopupResult::Open | PopupResult::Neighbor(_) => {}
                    PopupResult::Chosen(c) => {
                        chosen = Some((c, target));
                        leave(&ctx);
                    }
                    PopupResult::Dismissed => leave(&ctx),
                }
            }
        }
        if let Some((c, target)) = chosen {
            self.run(c, target, &ctx);
        }
    }

    /// Under the top bar, at English `at`: a card of how the downloads go (their speed together,
    /// how many run and how far they are, as the desktop's status bar and tray tip say it; or
    /// how many there are), then chips for All, Unfinished and Finished with their counts. The
    /// view a chip asks for.
    fn home_header(&mut self, ui: &mut Ui, at: egui::Rect) -> Option<View> {
        let live: Vec<&Item> = self.items.iter().filter(|i| i.live.is_some()).collect();
        let speed: f64 = live.iter().map(|i| i.rate()).sum();
        let (got, of) = live
            .iter()
            .filter_map(|i| i.size.map(|s| (i.downloaded, s)))
            .fold((0u64, 0u64), |(a, b), (d, s)| (a + d, b + s));
        let card = egui::Rect::from_min_size(
            pos2(at.min.x + 12.0, at.min.y + 10.0),
            egui::vec2(at.width() - 24.0, 58.0),
        );
        let screen = winui::rect(card);
        // The theme's own window colour and line, as the desktop's panels.
        ui.painter().rect_filled(screen, 14.0, winui::BG());
        ui.painter().rect_stroke(
            screen,
            14.0,
            egui::Stroke::new(1.0, colors().toolbar_line),
            egui::StrokeKind::Inside,
        );
        let (big, small) = if live.is_empty() {
            (
                crate::i18n::tn(self.items.len() as u64, "{n} download", "{n} downloads"),
                None,
            )
        } else {
            (
                model::rate(speed),
                Some(crate::i18n::tn(
                    live.len() as u64,
                    "{n} downloading",
                    "{n} downloading",
                )),
            )
        };
        let big_y = if small.is_some() { 22.0 } else { 29.0 };
        lead_text(
            ui,
            pos2(card.min.x + 16.0, card.min.y + big_y),
            &big,
            font_bold(20.0),
            colors().text,
        );
        if let Some(small) = small {
            lead_text(
                ui,
                pos2(card.min.x + 16.0, card.min.y + 44.0),
                &small,
                font(13.0),
                colors().muted,
            );
        }
        // How far the running ones are together: a ring with its percent.
        if of > 0 && !live.is_empty() {
            let frac = (got as f64 / of as f64).clamp(0.0, 1.0);
            let shown =
                ui.ctx()
                    .animate_value_with_time(egui::Id::unique("phone-ring"), frac as f32, 0.35);
            let c = winui::rect(egui::Rect::from_center_size(
                pos2(card.max.x - 36.0, card.center().y),
                egui::vec2(40.0, 40.0),
            ))
            .center();
            ring(ui, c, 19.0, shown);
            ui.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                crate::i18n::percent(frac, 0),
                font_bold(11.0),
                colors().text,
            );
        }
        // The views: All, Unfinished, Finished, with how many each has.
        let finished = self
            .items
            .iter()
            .filter(|i| i.status == model::Status::Complete)
            .count();
        let counts = [
            ("All Downloads", self.items.len()),
            ("Unfinished", self.items.len() - finished),
            ("Finished", finished),
        ];
        let rows = self.tree_rows();
        let mut x = at.min.x + 12.0;
        let y = card.max.y + 10.0;
        let mut picked = None;
        for (label, n) in counts {
            let Some((row, Some(view), _)) = rows.iter().find(|(r, ..)| r.label == label) else {
                continue;
            };
            // "All Downloads" is "All" on its chip: the top bar names the view.
            let name = if label == "All Downloads" {
                crate::i18n::t("All")
            } else {
                crate::i18n::t(label)
            };
            let text = format!("{name}  {}", crate::i18n::integer(n as u64));
            let w = ui
                .painter()
                .layout_no_wrap(text.clone(), font(14.0), colors().text)
                .size()
                .x
                + 28.0;
            let chip = egui::Rect::from_min_size(pos2(x, y), egui::vec2(w, 34.0));
            let cs = winui::rect(chip);
            let r = ui.interact(
                cs,
                egui::Id::unique(("phone-chip", label)),
                egui::Sense::click(),
            );
            let on = row.selected;
            let fill = ui.ctx().animate_bool_with_time(r.id.with("on"), on, 0.15);
            ui.painter()
                .rect_filled(cs, 17.0, colors().selection_inactive.gamma_multiply(fill));
            ui.painter().rect_stroke(
                cs,
                17.0,
                egui::Stroke::new(
                    1.0,
                    if on {
                        colors().selection
                    } else {
                        colors().menu_sep
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if r.is_pointer_button_down_on() {
                ui.painter().rect_filled(cs, 17.0, colors().row_pressed);
            }
            ui.painter().text(
                cs.center(),
                egui::Align2::CENTER_CENTER,
                &text,
                font(14.0),
                colors().text,
            );
            winui::expose(&r, egui::Role::Button, &text, true, None);
            if r.clicked() && !on {
                picked = Some(*view);
            }
            x += w + 8.0;
        }
        ui.painter().hline(
            winui::rect(at).x_range(),
            at.max.y - 0.5,
            egui::Stroke::new(1.0, colors().list_grid),
        );
        picked
    }

    /// Whether the drawer, a menu sheet or selection mode is over the list (Back closes it).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn phone_overlay(&self, ctx: &egui::Context) -> bool {
        self.popup.is_some()
            || ctx
                .data(|d| d.get_temp::<bool>(drawer_id()))
                .unwrap_or(false)
            || ctx
                .data(|d| d.get_temp::<bool>(picking_id()))
                .unwrap_or(false)
            || ctx
                .data(|d| d.get_temp::<bool>(search_id()))
                .unwrap_or(false)
    }

    /// The view's name for the top bar: the category's, the queue's, or All Downloads.
    fn view_title(&self) -> String {
        self.tree_rows()
            .into_iter()
            .find(|(r, ..)| r.selected)
            .map_or_else(
                || crate::i18n::t("All Downloads").to_owned(),
                |(r, ..)| crate::i18n::t(&r.label).to_owned(),
            )
    }

    /// The drawer, from the leading side: the app's name, the categories and queues, then the
    /// rest of the commands (the desktop's menus' a phone has: what the ⋮ menu had). A tap on a
    /// view shows it, on a command runs it, on a submenu opens it in the menu sheet; each, a tap
    /// outside it or Back closes the drawer. `shown`: how far it is in (0 to 1) as it opens and
    /// closes; `open`: whether it takes touches (not while it slides out).
    fn drawer(
        &mut self,
        ui: &mut Ui,
        full: egui::Rect,
        shown: f32,
        open: bool,
    ) -> (Option<View>, Option<Cmd>, Option<usize>) {
        let ctx = ui.ctx().clone();
        let close = || {
            ctx.data_mut(|d| d.insert_temp(drawer_id(), false));
            ctx.request_repaint();
        };
        let width = (full.width() - BAR_H).min(320.0);
        let pane = egui::Rect::from_min_size(
            pos2(full.min.x - width * (1.0 - shown), full.min.y),
            egui::vec2(width, full.height()),
        );
        // The rest of the window, dimmed: a tap there closes the drawer.
        let scrim =
            open.then(|| ui.interact(full, egui::Id::unique("phone-scrim"), egui::Sense::click()));
        ui.painter().rect_filled(
            full,
            0.0,
            egui::Color32::from_black_alpha((110.0 * shown) as u8),
        );
        let pane_screen = winui::rect(pane);
        ui.painter().rect_filled(pane_screen, 0.0, colors().tree);
        let mut view = None;
        let mut cmd = None;
        let mut sub = None;
        let rows = self.tree_rows();
        let entries = self.phone_more_entries();
        let sense = if open {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(pane_screen));
        inner.set_clip_rect(pane_screen.intersect(ui.clip_rect()));
        let icons = &mut self.icons;
        let mut toggled = None;
        let mut node_menu = None;
        egui::ScrollArea::vertical()
            .id_salt("phone-drawer")
            .auto_shrink([false, false])
            .show(&mut inner, |ui| {
                let w = pane_screen.width();
                // The header: the app's icon and name.
                let (h, _) = ui.allocate_exact_size(egui::vec2(w, 72.0), egui::Sense::hover());
                let at = winui::rect(h);
                egui::Image::from_bytes(
                    "bytes://mdm-48.png",
                    include_bytes!("../assets/icons/app/mdm-48.png"),
                )
                .paint_at(
                    ui,
                    winui::rect(egui::Rect::from_min_size(
                        pos2(at.min.x + PAD, at.min.y + 16.0),
                        egui::vec2(40.0, 40.0),
                    )),
                );
                lead_text(
                    ui,
                    pos2(at.min.x + PAD + 52.0, at.center().y),
                    crate::i18n::t("Mister Download Manager"),
                    font_bold(16.0),
                    colors().text,
                );
                ui.painter().hline(
                    h.x_range(),
                    h.max.y - 0.5,
                    egui::Stroke::new(1.0, colors().menu_sep),
                );
                ui.add_space(8.0);
                // The categories and queues.
                for (row, node, branch) in &rows {
                    let (screen, r) = ui.allocate_exact_size(egui::vec2(w, 48.0), sense);
                    let at = winui::rect(screen);
                    if row.selected {
                        ui.painter().rect_filled(
                            screen.shrink2(egui::vec2(8.0, 2.0)),
                            24.0,
                            colors().selection_inactive,
                        );
                    } else if r.is_pointer_button_down_on() {
                        ui.painter().rect_filled(
                            screen.shrink2(egui::vec2(8.0, 2.0)),
                            24.0,
                            colors().row_pressed,
                        );
                    }
                    let indent = PAD + 20.0 * row.level as f32;
                    // An English point: the painter places it (right to left, from the right).
                    icons.paint(
                        ui,
                        row.icon.clone(),
                        pos2(at.min.x + indent, at.center().y - 8.0),
                    );
                    lead_text(
                        ui,
                        pos2(at.min.x + indent + 28.0, at.center().y),
                        crate::i18n::t(&row.label),
                        font(16.0),
                        colors().text,
                    );
                    if r.clicked() {
                        match node {
                            Some(v) => view = Some(*v),
                            None => toggled = Some(*branch),
                        }
                    }
                    if r.secondary_clicked() || held_long(ui, &r) {
                        let p = r.interact_pointer_pos().unwrap_or(r.rect.center());
                        if node.is_some() || *branch == Branch::Queues {
                            node_menu = Some((*node, p));
                        }
                    }
                }
                ui.add_space(4.0);
                let (sep, _) = ui.allocate_exact_size(egui::vec2(w, 9.0), egui::Sense::hover());
                ui.painter().hline(
                    sep.shrink2(egui::vec2(PAD, 0.0)).x_range(),
                    sep.center().y,
                    egui::Stroke::new(1.0, colors().menu_sep),
                );
                // The rest of the commands.
                for (k, e) in entries.iter().enumerate() {
                    let (label, enabled, has_sub) = match e {
                        MenuEntry::Sep => {
                            let (sep, _) =
                                ui.allocate_exact_size(egui::vec2(w, 9.0), egui::Sense::hover());
                            ui.painter().hline(
                                sep.shrink2(egui::vec2(PAD, 0.0)).x_range(),
                                sep.center().y,
                                egui::Stroke::new(1.0, colors().menu_sep),
                            );
                            continue;
                        }
                        MenuEntry::Item(l, _, en, _) | MenuEntry::Radio(l, _, en, _) => {
                            (*l, *en, false)
                        }
                        MenuEntry::Sub(l, en, _) => (*l, *en, true),
                    };
                    let (screen, r) = ui.allocate_exact_size(
                        egui::vec2(w, MENU_ROW_H),
                        if enabled { sense } else { egui::Sense::hover() },
                    );
                    menu_row(ui, screen, &menu_label(label), enabled, false, has_sub, &r);
                    if enabled && r.clicked() {
                        match e {
                            MenuEntry::Item(_, c, ..) | MenuEntry::Radio(_, c, ..) => {
                                cmd = Some(*c);
                            }
                            MenuEntry::Sub(..) => sub = Some(k),
                            MenuEntry::Sep => {}
                        }
                    }
                }
                ui.add_space(8.0);
            });
        if let Some(b) = toggled {
            self.toggle(b);
        }
        if let Some((node, p)) = node_menu {
            self.open_popup_asked(Popup::Node(node), p, &ctx);
        }
        if let Some(scrim) = scrim
            && scrim.clicked()
            && !scrim
                .interact_pointer_pos()
                .is_some_and(|p| pane_screen.contains(p))
        {
            close();
        }
        // Back closes it first (Esc on the desktop's keys).
        if open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close();
        }
        if view.is_some() || cmd.is_some() || sub.is_some() {
            close();
        }
        (view, cmd, sub)
    }
}

/// What a touch on the list asked for.
#[derive(Default)]
struct ListTouch {
    /// A row tapped: its index, and whether it was held (a long press).
    tapped: Option<(usize, bool)>,
    /// A row's menu asked for (its ⋮, a right-click in the desktop's preview), where.
    menu: Option<(usize, Pos2)>,
    /// A row's own button tapped: true for Pause, false for Resume.
    action: Option<(usize, bool)>,
}

/// What a row's own button does ([`phone_list`]).
#[derive(Clone, Copy, PartialEq)]
enum RowState {
    /// Downloading: Pause.
    Running,
    /// Stopped, failed or waiting: Resume.
    Stopped,
    /// Complete (or can't start): no button.
    Done,
}

/// Whether the top bar is a search box (kept with the window's memory).
fn search_id() -> egui::Id {
    egui::Id::unique("phone-search")
}

/// The list's File Name filter (the desktop's, `list::Filter::Text`): `query`, or none when
/// empty.
fn set_name_filter(list: &mut list::ListState, query: &str) {
    if list.filters.is_empty() {
        list.filters.resize(list::COLS, None);
    }
    list.filters[0] = (!query.is_empty()).then(|| list::Filter::Text(query.to_owned()));
}

/// Whether the list is in selection mode (kept with the window's memory).
fn picking_id() -> egui::Id {
    egui::Id::unique("phone-picking")
}

/// The download list, one row per download: its icon, its name, then (not done) its progress
/// bar, and a line of what it is doing: its status in its colour, its size, speed and time left
/// (done: its size and when it finished), in the list's own words ([`list::Row`]). At its end its
/// own button (Pause, Resume; [`RowState`]) and its menu (⋮). In selection mode (`picking`) a
/// check circle takes the icon's place and the picked rows are tinted.
fn phone_list(
    ui: &mut Ui,
    rows: &[list::Row],
    selected: &[bool],
    picking: bool,
    states: &[RowState],
    icons: &mut chrome::ShellIcons,
) -> ListTouch {
    let mut touch = ListTouch::default();
    let width = ui.max_rect().width();
    egui::ScrollArea::vertical()
        .id_salt("phone-list")
        .auto_shrink([false, false])
        .show_rows(ui, ROW_H, rows.len(), |ui, range| {
            for i in range {
                let row = &rows[i];
                let (screen, r) =
                    ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
                // English, mirrored where drawn.
                let at = winui::rect(screen);
                let state = states.get(i).copied().unwrap_or(RowState::Done);
                // At its end: its menu (⋮), and before it its own button (not in selection mode).
                let more = (!picking).then(|| {
                    egui::Rect::from_min_size(
                        pos2(at.max.x - 8.0 - 40.0, at.center().y - 20.0),
                        egui::vec2(40.0, 40.0),
                    )
                });
                let button = (!picking && state != RowState::Done).then(|| {
                    egui::Rect::from_min_size(
                        pos2(at.max.x - 8.0 - 40.0 - 44.0, at.center().y - 20.0),
                        egui::vec2(40.0, 40.0),
                    )
                });
                let picked = picking && selected.get(i).copied().unwrap_or(false);
                if picked {
                    ui.painter()
                        .rect_filled(screen, 0.0, colors().selection_inactive);
                }
                ripple(ui, screen, &r);
                if picking {
                    check_circle(
                        ui,
                        r.id,
                        winui::rect(egui::Rect::from_center_size(
                            pos2(at.min.x + PAD + 16.0, at.min.y + 28.0),
                            egui::vec2(24.0, 24.0),
                        )),
                        picked,
                    );
                } else {
                    // A file's icon at 32 points (its type's); an English point, the painter
                    // places it (right to left, from the right).
                    let (icon, dy) = match &row.icon {
                        chrome::IconKey::Ext(e) => (chrome::IconKey::ExtLarge(e.clone()), 0.0),
                        other => (other.clone(), 8.0),
                    };
                    icons.paint(ui, icon, pos2(at.min.x + PAD + dy, at.min.y + 12.0 + dy));
                }
                let text_x = PAD + 32.0 + 14.0;
                let end = match (button.is_some(), more.is_some()) {
                    (true, _) => 8.0 + 40.0 + 44.0 + 4.0,
                    (false, true) => 8.0 + 40.0 + 4.0,
                    _ => PAD,
                };
                let text_w = width - text_x - end;
                let name = elide(ui, &row.cells[0], font(16.0), text_w);
                lead_text(
                    ui,
                    pos2(at.min.x + text_x, at.min.y + 22.0),
                    &name,
                    font(16.0),
                    colors().text,
                );
                // Its status in its colour, then its figures.
                // MDM's own colours: the progress window's status colour while it runs, the
                // theme's error colour when it failed, the list's muted text otherwise.
                let status_color = match row.status_group {
                    "Downloading" => colors().status_value,
                    "Error" => colors().error,
                    _ => colors().muted,
                };
                let figures: Vec<&str> = if state == RowState::Done {
                    vec![row.cells[2].as_str(), row.cells[6].as_str()]
                } else {
                    vec![
                        row.cells[2].as_str(),
                        row.cells[list::RATE].as_str(),
                        row.cells[4].as_str(),
                    ]
                };
                let figures = figures
                    .into_iter()
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("  ·  ");
                let line_y = if state == RowState::Done { 50.0 } else { 62.0 };
                status_line(
                    ui,
                    pos2(at.min.x + text_x, at.min.y + line_y),
                    text_w,
                    &row.cells[list::STATUS],
                    status_color,
                    &figures,
                );
                // The progress bar (not done, the size known), moving smoothly to where it is.
                if let Some(done) = row.progress.filter(|_| state != RowState::Done) {
                    let shown = ui.ctx().animate_value_with_time(
                        r.id.with("bar"),
                        done.clamp(0.0, 1.0) as f32,
                        0.35,
                    );
                    let bar = egui::Rect::from_min_size(
                        pos2(at.min.x + text_x, at.min.y + 37.0),
                        egui::vec2(text_w, 9.0),
                    );
                    // The desktop's progress bar (its theme's meter).
                    winui::meter(ui.painter(), winui::rect(bar), shown);
                }
                ui.painter().hline(
                    winui::rect(at.with_min_x(at.min.x + text_x)).x_range(),
                    screen.max.y - 0.5,
                    egui::Stroke::new(1.0, colors().list_grid),
                );
                // Its own buttons, over the row (a tap on one isn't one on the row).
                let mut on_button = false;
                if let Some(b) = button {
                    let bs = winui::rect(b);
                    let br = ui.interact(bs, r.id.with("own"), egui::Sense::click());
                    on_button |= br.hovered() || br.is_pointer_button_down_on();
                    row_button(ui, bs, state == RowState::Running, &br);
                    if br.clicked() {
                        touch.action = Some((i, state == RowState::Running));
                    }
                }
                if let Some(m) = more {
                    let ms = winui::rect(m);
                    let mr = ui.interact(ms, r.id.with("more"), egui::Sense::click());
                    on_button |= mr.hovered() || mr.is_pointer_button_down_on();
                    if mr.is_pointer_button_down_on() {
                        ui.painter()
                            .circle_filled(ms.center(), 20.0, colors().toolbar_pressed);
                    }
                    for dy in [-6.0, 0.0, 6.0] {
                        ui.painter().circle_filled(
                            ms.center() + egui::vec2(0.0, dy),
                            2.0,
                            colors().muted,
                        );
                    }
                    winui::expose(&mr, egui::Role::Button, &row.cells[0], true, None);
                    if mr.clicked() {
                        touch.menu = Some((i, ms.left_bottom()));
                    }
                }
                let held = !on_button && held_long(ui, &r);
                // The release that ends a long press is no tap: it picked the row already.
                let after_hold = r.clicked()
                    && ui.data_mut(|d| d.remove_temp::<bool>(r.id.with("held-done")).is_some());
                if r.secondary_clicked() {
                    let p = r.interact_pointer_pos().unwrap_or(screen.center());
                    touch.menu = Some((i, p));
                } else if held {
                    touch.tapped = Some((i, true));
                    ui.data_mut(|d| d.insert_temp(r.id.with("held-done"), true));
                } else if r.clicked() && !after_hold && !on_button {
                    touch.tapped = Some((i, false));
                }
            }
        });
    touch
}

/// A row's line at English point `at` (its left middle), `width` wide: `status` in `color`, then
/// `figures` muted, cut with "…" when it doesn't fit.
fn status_line(ui: &Ui, at: Pos2, width: f32, status: &str, color: egui::Color32, figures: &str) {
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat {
        font_id: font(13.0),
        color,
        ..Default::default()
    };
    if !status.trim().is_empty() {
        job.append(status, 0.0, format(color));
    }
    if !figures.is_empty() {
        let lead = if job.text.is_empty() { "" } else { "  ·  " };
        job.append(&format!("{lead}{figures}"), 0.0, format(colors().muted));
    }
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    let x = if winui::rtl() {
        winui::mx(at.x) - galley.size().x
    } else {
        at.x
    };
    let pos = pos2(x, at.y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, colors().muted);
}

/// Where a finger pressed `r` (at `screen`): a circle spreading from it while it is down, fading
/// once let go, as Android's lists answer a touch.
fn ripple(ui: &Ui, screen: egui::Rect, r: &egui::Response) {
    let key = r.id.with("ripple");
    let now = ui.input(|i| i.time);
    let down = r.is_pointer_button_down_on();
    let mut state = ui.data(|d| d.get_temp::<(Pos2, f64, Option<f64>)>(key));
    if down {
        if state.is_none_or(|(_, _, up)| up.is_some()) {
            let at = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(screen.center());
            state = Some((at, now, None));
        }
    } else if let Some((at, start, None)) = state {
        state = Some((at, start, Some(now)));
    }
    let Some((at, start, up)) = state else {
        return;
    };
    let grow = ((now - start) / 0.35).min(1.0) as f32;
    let fade = up.map_or(1.0, |u| 1.0 - ((now - u) / 0.25).min(1.0) as f32);
    if fade <= 0.0 {
        ui.data_mut(|d| d.remove::<(Pos2, f64, Option<f64>)>(key));
        return;
    }
    ui.data_mut(|d| d.insert_temp(key, (at, start, up)));
    let reach = screen.width().max(screen.height());
    let eased = 1.0 - (1.0 - grow).powi(3);
    let p = ui
        .painter()
        .with_clip_rect(screen.intersect(ui.clip_rect()));
    p.circle_filled(at, reach * eased, colors().row_pressed.gamma_multiply(fade));
    ui.ctx().request_repaint();
}

/// A ring at `c` of `radius`: its track, and `frac` (0 to 1) of it filled from the top,
/// clockwise.
fn ring(ui: &Ui, c: Pos2, radius: f32, frac: f32) {
    let p = ui.painter();
    p.circle_stroke(c, radius, egui::Stroke::new(4.0, colors().meter_track));
    if frac <= 0.0 {
        return;
    }
    let steps = (64.0 * frac).ceil().max(2.0) as usize;
    let points: Vec<Pos2> = (0..=steps)
        .map(|k| {
            let a = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::TAU * frac * k as f32 / steps as f32;
            c + egui::vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    p.add(egui::Shape::line(
        points,
        egui::Stroke::new(4.0, colors().meter_fill),
    ));
}

/// Selection mode's check circle at `screen`: filled with a tick when `on` (growing in), a ring
/// when not.
fn check_circle(ui: &Ui, id: egui::Id, screen: egui::Rect, on: bool) {
    let grow = ui.ctx().animate_bool_with_time(id.with("check"), on, 0.15);
    let p = ui.painter();
    let c = screen.center();
    p.circle_stroke(c, 10.0, egui::Stroke::new(1.5, colors().muted));
    if grow > 0.0 {
        p.circle_filled(c, 11.0 * grow, colors().selection);
        if grow > 0.6 {
            let ink = colors().selection_text;
            let tick = [
                c + egui::vec2(-4.5, 0.0),
                c + egui::vec2(-1.5, 3.5),
                c + egui::vec2(5.0, -3.5),
            ];
            p.line_segment([tick[0], tick[1]], egui::Stroke::new(2.0, ink));
            p.line_segment([tick[1], tick[2]], egui::Stroke::new(2.0, ink));
        }
    }
}

/// A row's own round button at `screen`: the toolbar's Stop (while it runs) or Resume icon.
fn row_button(ui: &Ui, screen: egui::Rect, pause: bool, r: &egui::Response) {
    let c = screen.center();
    if r.is_pointer_button_down_on() {
        ui.painter()
            .circle_filled(c, 20.0, colors().toolbar_pressed);
    }
    let icon = if pause { Icon::Stop } else { Icon::Resume };
    icon.image(true)
        .paint_at(ui, egui::Rect::from_center_size(c, egui::vec2(24.0, 24.0)));
    winui::expose(
        r,
        egui::Role::Button,
        if pause {
            // i18n: verb: a download's own button in the phone's list
            crate::i18n::t("Pause")
        } else {
            // i18n: verb: a download's own button in the phone's list
            crate::i18n::t("Resume")
        },
        true,
        None,
    );
}

/// How long a row is held still before its menu opens: Android's long press.
const LONG_PRESS: f64 = 0.5;

/// Whether `r` has just been held still for [`LONG_PRESS`] (true once per press). MDM's own,
/// not egui's `long_touched`: that needs the press reported as a touch and a frame drawn between
/// 0.8 s and the release, and neither held on the emulator; this one counts any press (a mouse's
/// too, in the desktop's preview) and asks for the frames it needs.
fn held_long(ui: &Ui, r: &egui::Response) -> bool {
    let key = r.id.with("held");
    if !r.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<(f64, bool)>(key));
        return false;
    }
    let now = ui.input(|i| i.time);
    let (start, fired) = ui
        .data(|d| d.get_temp::<(f64, bool)>(key))
        .unwrap_or((now, false));
    // Moved away from where it pressed: a scroll, not a long press.
    let moved = ui.input(|i| {
        i.pointer
            .press_origin()
            .zip(i.pointer.latest_pos())
            .is_some_and(|(a, b)| a.distance(b) > 8.0)
    });
    let fire = !fired && !moved && now - start >= LONG_PRESS;
    ui.data_mut(|d| d.insert_temp(key, (start, fired || fire || moved)));
    if !fired && !moved {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    fire
}

/// `text` from English point `at` (its left middle; mirrored, and its right middle, right to
/// left).
fn lead_text(ui: &Ui, at: Pos2, text: &str, font: egui::FontId, color: egui::Color32) {
    ui.painter().text(
        winui::rect(egui::Rect::from_min_size(at, egui::Vec2::ZERO)).min,
        if winui::rtl() {
            egui::Align2::RIGHT_CENTER
        } else {
            egui::Align2::LEFT_CENTER
        },
        text,
        font,
        color,
    );
}

/// `text` cut with "…" to fit `width` in `font`.
fn elide(ui: &Ui, text: &str, font: egui::FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&s) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// The top bar's buttons' glyphs, drawn (no font has them all).
#[derive(Clone, Copy)]
enum Glyph {
    /// Three lines: the categories.
    Menu,
    /// Three dots: the rest of the commands.
    More,
    /// A cross: closes the page ([`sheet`]).
    Close,
    /// An arrow pointing back: a page within a window goes back ([`sheet_back`]).
    Back,
    /// A box with a tick in it: Select all (selection mode).
    SelectAll,
    /// A magnifying glass: search the list.
    Search,
    /// Three lines, shorter and shorter: sort the list (Arrange files).
    Sort,
}

/// A square button of the top bar at English `at`; true when tapped.
fn bar_button(
    ui: &mut Ui,
    at: egui::Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    glyph: Glyph,
) -> bool {
    let screen = winui::rect(at);
    let r = ui.interact(screen, egui::Id::unique(id), egui::Sense::click());
    let p = ui.painter();
    if r.is_pointer_button_down_on() {
        p.circle_filled(screen.center(), 20.0, colors().toolbar_pressed);
    }
    let c = screen.center();
    let ink = colors().text;
    match glyph {
        Glyph::Menu => {
            for dy in [-6.0, 0.0, 6.0] {
                p.line_segment(
                    [c + egui::vec2(-9.0, dy), c + egui::vec2(9.0, dy)],
                    egui::Stroke::new(2.0, ink),
                );
            }
        }
        Glyph::More => {
            for dy in [-6.0, 0.0, 6.0] {
                p.circle_filled(c + egui::vec2(0.0, dy), 2.0, ink);
            }
        }
        Glyph::Back => {
            let stroke = egui::Stroke::new(2.0, ink);
            // Pointing the way back: left, or right in a right-to-left interface.
            let d = if winui::rtl() { -1.0 } else { 1.0 };
            p.line_segment(
                [c + egui::vec2(-8.0 * d, 0.0), c + egui::vec2(8.0 * d, 0.0)],
                stroke,
            );
            p.line_segment(
                [
                    c + egui::vec2(-8.0 * d, 0.0),
                    c + egui::vec2(-1.5 * d, -6.5),
                ],
                stroke,
            );
            p.line_segment(
                [c + egui::vec2(-8.0 * d, 0.0), c + egui::vec2(-1.5 * d, 6.5)],
                stroke,
            );
        }
        Glyph::Search => {
            let stroke = egui::Stroke::new(2.0, ink);
            p.circle_stroke(c + egui::vec2(-2.0, -2.0), 6.5, stroke);
            p.line_segment([c + egui::vec2(3.0, 3.0), c + egui::vec2(8.0, 8.0)], stroke);
        }
        Glyph::Sort => {
            let stroke = egui::Stroke::new(2.0, ink);
            for (dy, w) in [(-6.0, 9.0), (0.0, 6.0), (6.0, 3.0)] {
                p.line_segment(
                    [c + egui::vec2(-9.0, dy), c + egui::vec2(-9.0 + 2.0 * w, dy)],
                    stroke,
                );
            }
        }
        Glyph::SelectAll => {
            let stroke = egui::Stroke::new(2.0, ink);
            p.rect_stroke(
                egui::Rect::from_center_size(c, egui::vec2(16.0, 16.0)),
                2.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            p.line_segment(
                [c + egui::vec2(-4.0, 0.0), c + egui::vec2(-1.0, 3.5)],
                stroke,
            );
            p.line_segment(
                [c + egui::vec2(-1.0, 3.5), c + egui::vec2(4.5, -3.5)],
                stroke,
            );
        }
        Glyph::Close => {
            let stroke = egui::Stroke::new(2.0, ink);
            p.line_segment(
                [c + egui::vec2(-7.0, -7.0), c + egui::vec2(7.0, 7.0)],
                stroke,
            );
            p.line_segment(
                [c + egui::vec2(-7.0, 7.0), c + egui::vec2(7.0, -7.0)],
                stroke,
            );
        }
    }
    r.clicked()
}

/// A button of the bottom bar at English `at`: the toolbar's icon over its label; true when
/// tapped.
fn bottom_button(ui: &mut Ui, at: egui::Rect, icon: Icon, label: &str, enabled: bool) -> bool {
    let screen = winui::rect(at);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let r = ui.interact(screen, egui::Id::unique(("phone-bottom", label)), sense);
    if r.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(screen.shrink(4.0), 12.0, colors().toolbar_pressed);
    }
    let img = egui::Rect::from_center_size(
        pos2(screen.center().x, screen.min.y + 18.0),
        egui::vec2(22.0, 22.0),
    );
    icon.image(enabled).paint_at(ui, img);
    ui.painter().text(
        pos2(screen.center().x, screen.min.y + 42.0),
        egui::Align2::CENTER_CENTER,
        label,
        font(12.0),
        if enabled {
            colors().text
        } else {
            colors().text_disabled
        },
    );
    enabled && r.clicked()
}

/// How long the menu sheet takes to slide away as it closes (seconds).
const MENU_LEAVE: f64 = 0.18;

/// A menu row's height in the sheet: Android's list item.
const MENU_ROW_H: f32 = 48.0;

/// `entries` (a popup menu of the desktop's) as a sheet over the bottom of `full`, one row a
/// finger can hit for each, under a dimmed window: a tap runs an item, opens a submenu in the
/// sheet's place (a first row goes back), or outside the sheet closes it, as Back does. Which
/// submenu is open is kept for menu `serial`.
fn menu_sheet<C: Clone>(
    ui: &mut Ui,
    full: egui::Rect,
    entries: &[MenuEntry<C>],
    serial: u64,
    leaving: f32,
) -> PopupResult<C> {
    let ctx = ui.ctx().clone();
    let path_id = egui::Id::unique(("phone-menu-path", serial));
    // Opened on a submenu (the sort button, a submenu from the drawer): this many levels in,
    // where going back closes it.
    let base_id = egui::Id::unique(("phone-menu-base", serial));
    let base: usize = ctx.data(|d| d.get_temp(base_id)).unwrap_or(0);
    let mut path: Vec<usize> = ctx.data(|d| d.get_temp(path_id)).unwrap_or_default();
    // The entries of the open submenu, and its title.
    let mut here = entries;
    let mut title: Option<&str> = None;
    for &k in &path {
        match here.get(k) {
            Some(MenuEntry::Sub(label, _, children)) => {
                title = Some(label);
                here = children;
            }
            _ => break,
        }
    }
    let rows = here.len() + usize::from(title.is_some());
    let height = (rows as f32 * MENU_ROW_H + 16.0).min(full.height() * 0.85);
    // Opening, it rises from the bottom as the window behind it dims.
    // Closing (`leaving` from 0 to 1): it slides back down as the window behind brightens,
    // and takes no touches.
    let shown = entrance(&ctx, egui::Id::unique(("phone-menu", serial)), 0.22)
        * (1.0 - (leaving.clamp(0.0, 1.0)).powi(2));
    let live = leaving <= 0.0;
    let sheet = egui::Rect::from_min_max(pos2(full.min.x, full.max.y - height), full.max)
        .translate(egui::vec2(0.0, height * (1.0 - shown)));
    let scrim = ui.interact(
        full,
        egui::Id::unique(("phone-menu-scrim", serial)),
        if live {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    ui.painter().rect_filled(
        full,
        0.0,
        egui::Color32::from_black_alpha((110.0 * shown) as u8),
    );
    ui.painter().rect_filled(
        sheet,
        egui::CornerRadius {
            nw: 16,
            ne: 16,
            sw: 0,
            se: 0,
        },
        colors().menu,
    );
    let mut result = PopupResult::Open;
    let mut inside =
        ui.new_child(egui::UiBuilder::new().max_rect(sheet.shrink2(egui::vec2(0.0, 8.0))));
    egui::ScrollArea::vertical()
        .id_salt(("phone-menu", serial, path.len()))
        .auto_shrink([false, false])
        .show(&mut inside, |ui| {
            let width = ui.max_rect().width();
            if let Some(title) = title {
                let (at, r) = ui.allocate_exact_size(
                    egui::vec2(width, MENU_ROW_H),
                    if live {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    },
                );
                menu_row(
                    ui,
                    at,
                    &format!("‹  {}", menu_label(title)),
                    true,
                    false,
                    false,
                    &r,
                );
                if r.clicked() {
                    if path.len() > base {
                        path.pop();
                    } else {
                        result = PopupResult::Dismissed;
                    }
                }
            }
            for (k, e) in here.iter().enumerate() {
                if let MenuEntry::Sep = e {
                    let (at, _) =
                        ui.allocate_exact_size(egui::vec2(width, 9.0), egui::Sense::hover());
                    ui.painter().hline(
                        at.shrink2(egui::vec2(PAD, 0.0)).x_range(),
                        at.center().y,
                        egui::Stroke::new(1.0, colors().menu_sep),
                    );
                    continue;
                }
                let (label, enabled, checked, sub) = match e {
                    MenuEntry::Item(l, _, en, ch) | MenuEntry::Radio(l, _, en, ch) => {
                        (*l, *en, *ch, false)
                    }
                    MenuEntry::Sub(l, en, _) => (*l, *en, false, true),
                    MenuEntry::Sep => unreachable!(),
                };
                let sense = if enabled && live {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (at, r) = ui.allocate_exact_size(egui::vec2(width, MENU_ROW_H), sense);
                menu_row(ui, at, &menu_label(label), enabled, checked, sub, &r);
                if enabled && r.clicked() {
                    match e {
                        MenuEntry::Item(_, c, ..) | MenuEntry::Radio(_, c, ..) => {
                            result = PopupResult::Chosen(c.clone());
                        }
                        MenuEntry::Sub(..) => path.push(k),
                        MenuEntry::Sep => {}
                    }
                }
            }
        });
    let outside = scrim.clicked()
        && !scrim
            .interact_pointer_pos()
            .is_some_and(|p| sheet.contains(p));
    // Back: out of the submenu, else the menu closes.
    let back = live && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if back && path.len() > base {
        path.pop();
    } else if back || outside {
        result = PopupResult::Dismissed;
    }
    if matches!(result, PopupResult::Open) {
        ctx.data_mut(|d| d.insert_temp(path_id, path));
    } else {
        ctx.data_mut(|d| {
            d.remove::<Vec<usize>>(path_id);
            d.remove::<usize>(base_id);
        });
        // A frame without the sheet now: a touch is matched to the last frame's widgets, and
        // the sheet's backdrop would take the next one.
        ctx.request_repaint();
    }
    result
}

/// A menu's label in the interface language, without its access letter's `&` or the keyboard
/// shortcut some labels carry ("Move/Rename (Ctrl-M)"): a phone has no keyboard for it.
fn menu_label(label: &str) -> String {
    chrome::without_shortcut(&chrome::plain(crate::i18n::tc("menu", label)).0).to_owned()
}

/// One row of [`menu_sheet`] at `at` (screen): its label, a check mark when checked, an arrow
/// when it opens a submenu.
fn menu_row(
    ui: &Ui,
    at: egui::Rect,
    label: &str,
    enabled: bool,
    checked: bool,
    sub: bool,
    r: &egui::Response,
) {
    let p = ui.painter();
    if enabled && r.is_pointer_button_down_on() {
        p.rect_filled(at, 0.0, colors().menu_hot);
    }
    let ink = if enabled {
        colors().menu_text
    } else {
        colors().menu_text_disabled
    };
    let rtl = winui::rtl();
    let lead = if rtl { at.max.x - PAD } else { at.min.x + PAD };
    let trail = if rtl { at.min.x + PAD } else { at.max.x - PAD };
    let dir = if rtl { -1.0 } else { 1.0 };
    let text_x = lead + dir * 28.0;
    p.text(
        pos2(text_x, at.center().y),
        if rtl {
            egui::Align2::RIGHT_CENTER
        } else {
            egui::Align2::LEFT_CENTER
        },
        label,
        font(16.0),
        ink,
    );
    let stroke = egui::Stroke::new(2.0, ink);
    if checked {
        let c = pos2(lead + dir * 9.0, at.center().y);
        p.line_segment(
            [
                c + egui::vec2(-6.0 * dir, 0.0),
                c + egui::vec2(-2.0 * dir, 4.0),
            ],
            stroke,
        );
        p.line_segment(
            [
                c + egui::vec2(-2.0 * dir, 4.0),
                c + egui::vec2(6.0 * dir, -5.0),
            ],
            stroke,
        );
    }
    if sub {
        let c = pos2(trail - dir * 4.0, at.center().y);
        p.line_segment([c + egui::vec2(-4.0 * dir, -6.0), c], stroke);
        p.line_segment([c, c + egui::vec2(-4.0 * dir, 6.0)], stroke);
    }
}

/// A download as a phone's notification shows it (android.rs `Notices`): running, with how far
/// it is and its figures, or just completed.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Notice {
    pub id: u64,
    pub name: String,
    /// How far it is (0 to 100), when its size is known.
    pub percent: Option<u8>,
    /// Its figures as the list says them: "45% · 1.25 MB/sec · 2 min".
    pub line: String,
    /// The file, for a completed one's Open.
    pub path: String,
}

impl App {
    /// The downloads running now, for their notifications (android.rs).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn running_notices(&self) -> Vec<Notice> {
        self.items
            .iter()
            .filter(|i| i.live.is_some())
            .map(|i| {
                let frac = i.fraction();
                let mut parts = Vec::new();
                if let Some(f) = frac {
                    parts.push(crate::i18n::percent(f, 0));
                }
                let rate = i.rate();
                if rate > 0.0 {
                    parts.push(model::rate(rate));
                }
                let left = i.time_left();
                if !left.is_empty() {
                    parts.push(left);
                }
                Notice {
                    id: i.id,
                    name: i.file_name.clone(),
                    percent: frac.map(|f| (f * 100.0).clamp(0.0, 100.0) as u8),
                    line: parts.join("  ·  "),
                    path: i.dest.display().to_string(),
                }
            })
            .collect()
    }

    /// Download `id`, when it has completed: for its "Download complete" notification.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn completed_notice(&self, id: u64) -> Option<Notice> {
        self.items
            .iter()
            .find(|i| i.id == id && i.status == Status::Complete)
            .map(|i| Notice {
                id: i.id,
                name: i.file_name.clone(),
                percent: Some(100),
                line: model::size3(i.size.unwrap_or(i.downloaded)),
                path: i.dest.display().to_string(),
            })
    }
}

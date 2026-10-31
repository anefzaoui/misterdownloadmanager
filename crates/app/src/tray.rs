//! The notification-area icon (IDM: "IDM menu pops up when you right click on IDM icon"
//! [IDM-doc]). Left click brings the window back; right click shows the menu. The menu's
//! contents are ours ([unknown] IDM's exact items, lab). Windows, macOS (menu bar) and Linux
//! (StatusNotifierItem over D-Bus: KDE, GNOME with its AppIndicator extension).

use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// What the tray asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCmd {
    Show,
    AddUrl,
    Batch,
    LimiterOn,
    LimiterOff,
    Scheduler,
    StartQueue,
    StopQueue,
    Options,
    Exit,
}

pub struct Tray {
    limiter_on: CheckMenuItem,
    limiter_off: CheckMenuItem,
    /// Start and Stop main download queue: one of them enabled, as the queue runs.
    start_queue: MenuItem,
    stop_queue: MenuItem,
    queue_running: std::cell::Cell<Option<bool>>,
    tray: TrayIcon,
    /// The tip shown and when, so the shell is told at most once a second.
    tip: std::cell::RefCell<(String, Option<std::time::Instant>)>,
    rx: Receiver<TrayCmd>,
    /// Every entry with its English text, to translate again when the language changes.
    labels: Vec<(Entry, &'static str)>,
}

/// A menu entry whose text is translated.
enum Entry {
    Item(MenuItem),
    Check(CheckMenuItem),
    Sub(Submenu),
}

impl Tray {
    /// The icon and its menu; events wake `ctx`. None if the shell refuses the icon.
    pub fn new(ctx: &egui::Context, rgba: Vec<u8>, size: u32, limiter: bool) -> Option<Tray> {
        let menu = Menu::new();
        let mut items = Vec::new();
        let mut labels = Vec::new();
        // Labels are translated here and again by `relabel` when the language changes.
        let mut add = |m: &Menu, label: &'static str, cmd: TrayCmd| {
            let item = MenuItem::new(crate::i18n::t(label), true, None);
            items.push((item.id().clone(), cmd));
            m.append(&item).ok();
            labels.push((Entry::Item(item.clone()), label));
            item
        };
        add(
            &menu,
            crate::i18n::mark("Open Mister Download Manager"),
            TrayCmd::Show,
        );
        menu.append(&PredefinedMenuItem::separator()).ok();
        let mark = crate::i18n::mark;
        add(&menu, mark("Add new download"), TrayCmd::AddUrl);
        add(&menu, mark("Add batch download"), TrayCmd::Batch);
        menu.append(&PredefinedMenuItem::separator()).ok();
        let limiter_label = mark("Speed Limiter");
        let limiter_menu = Submenu::new(crate::i18n::t(limiter_label), true);
        // i18n: verb: turn the Speed Limiter on
        let on_label = mark("Turn on");
        // i18n: verb: turn the Speed Limiter off
        let off_label = mark("Turn off");
        let limiter_on = CheckMenuItem::new(crate::i18n::t(on_label), true, limiter, None);
        let limiter_off = CheckMenuItem::new(crate::i18n::t(off_label), true, !limiter, None);
        limiter_menu.append(&limiter_on).ok();
        limiter_menu.append(&limiter_off).ok();
        menu.append(&limiter_menu).ok();
        // i18n: noun: opens the Scheduler (the tool that starts queues on time)
        add(&menu, mark("Scheduler"), TrayCmd::Scheduler);
        let start_queue = add(
            &menu,
            mark("Start main download queue"),
            TrayCmd::StartQueue,
        );
        let stop_queue = add(&menu, mark("Stop main download queue"), TrayCmd::StopQueue);
        // i18n: plural noun: opens the Options window (settings)
        add(&menu, mark("Options"), TrayCmd::Options);
        menu.append(&PredefinedMenuItem::separator()).ok();
        // i18n: verb: quit the app
        add(&menu, mark("Exit"), TrayCmd::Exit);
        drop(add);
        items.push((limiter_on.id().clone(), TrayCmd::LimiterOn));
        items.push((limiter_off.id().clone(), TrayCmd::LimiterOff));

        let icon = tray_icon::Icon::from_rgba(rgba, size, size).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip(crate::MAIN_TITLE)
            .with_icon(icon)
            .build()
            .ok()?;

        let (tx, rx) = channel();
        let wake = |tx: &Sender<TrayCmd>, ctx: &egui::Context| (tx.clone(), ctx.clone());
        let (icon_tx, icon_ctx) = wake(&tx, ctx);
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = icon_tx.send(TrayCmd::Show);
                icon_ctx.request_repaint();
            }
        }));
        let (menu_tx, menu_ctx) = wake(&tx, ctx);
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some((_, cmd)) = items
                .iter()
                .find(|(id, _): &&(MenuId, TrayCmd)| *id == e.id)
            {
                let _ = menu_tx.send(*cmd);
                menu_ctx.request_repaint();
            }
        }));
        labels.push((Entry::Sub(limiter_menu), limiter_label));
        labels.push((Entry::Check(limiter_on.clone()), on_label));
        labels.push((Entry::Check(limiter_off.clone()), off_label));
        Some(Tray {
            tray,
            tip: Default::default(),
            limiter_on,
            limiter_off,
            start_queue,
            stop_queue,
            queue_running: std::cell::Cell::new(None),
            rx,
            labels,
        })
    }

    /// The commands since the last call.
    pub fn poll(&self) -> Vec<TrayCmd> {
        self.rx.try_iter().collect()
    }

    /// Translate the menu again (the interface language changed).
    pub fn relabel(&self) {
        for (entry, source) in &self.labels {
            let text = crate::i18n::t(source);
            match entry {
                Entry::Item(i) => i.set_text(text),
                Entry::Check(i) => i.set_text(text),
                Entry::Sub(i) => i.set_text(text),
            }
        }
    }

    /// Keep Start / Stop main download queue in step with the queue (the menu is only told
    /// when it changes).
    pub fn set_queue(&self, running: bool) {
        if self.queue_running.replace(Some(running)) != Some(running) {
            self.start_queue.set_enabled(!running);
            self.stop_queue.set_enabled(running);
        }
    }

    /// The icon's tip: the app's name, and while downloads run how many and how fast.
    pub fn set_tooltip(&self, tip: &str) {
        let mut last = self.tip.borrow_mut();
        let due = last
            .1
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if last.0 != tip && due {
            let _ = self.tray.set_tooltip(Some(tip));
            *last = (tip.to_owned(), Some(std::time::Instant::now()));
        }
    }

    /// Keep the Speed Limiter checks in step with the app.
    pub fn set_limiter(&self, on: bool) {
        self.limiter_on.set_checked(on);
        self.limiter_off.set_checked(!on);
    }
}

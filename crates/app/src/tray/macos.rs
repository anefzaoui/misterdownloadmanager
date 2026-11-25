//! The macOS status menu uses the same commands and menu items as the desktop tray.
//! Running downloads occupy its first rows, so each progress window is one click away.

use super::TrayCmd;
use crate::settings::MenuBarSettings;
use std::cell::Cell;
use tray_icon::TrayIcon;
use tray_icon::menu::{Menu, MenuId, MenuItem, MenuItemKind, PredefinedMenuItem};

pub(super) struct StatusMenu {
    menu: Menu,
    commands: Vec<(MenuItemKind, Option<TrayCmd>)>,
    last: Cell<Option<(MenuBarSettings, bool)>>,
    divider: Cell<bool>,
}

impl StatusMenu {
    pub fn new(menu: Menu, ids: &[(MenuId, TrayCmd)], limiter: &MenuId) -> Self {
        let commands = menu
            .items()
            .into_iter()
            .map(|row| {
                let cmd = if row.id() == limiter {
                    Some(TrayCmd::LimiterOn)
                } else {
                    ids.iter()
                        .find(|(id, _)| id == row.id())
                        .map(|(_, cmd)| *cmd)
                };
                (row, cmd)
            })
            .collect();
        Self {
            menu,
            commands,
            last: Cell::new(None),
            divider: Cell::new(false),
        }
    }

    pub fn configure(&self, tray: &TrayIcon, settings: MenuBarSettings, panel: bool) -> bool {
        let last = self.last.get();
        if last == Some((settings, panel)) {
            return false;
        }
        if last.is_none_or(|(s, _)| s.enabled != settings.enabled)
            && let Err(err) = tray.set_visible(settings.enabled)
        {
            tracing::warn!("menu bar visibility: {err}");
            return false;
        }
        tray.set_show_menu_on_left_click(!panel);
        self.last.set(Some((settings, panel)));
        while self.menu.remove_at(0).is_some() {}
        self.divider.set(false);
        let mut separator = false;
        let mut any = false;
        for (row, cmd) in &self.commands {
            let Some(cmd) = cmd else {
                separator = any;
                continue;
            };
            if !visible(*cmd, settings) {
                continue;
            }
            if separator {
                self.menu.append(&PredefinedMenuItem::separator()).ok();
                separator = false;
            }
            match row {
                MenuItemKind::MenuItem(i) => self.menu.append(i),
                MenuItemKind::Submenu(s) => self.menu.append(s),
                _ => continue,
            }
            .ok();
            any = true;
        }
        true
    }

    pub fn update_downloads(&self, running: &[(u64, String)], ids: &mut Vec<(MenuId, u64)>) {
        if ids
            .iter()
            .map(|(_, id)| id)
            .eq(running.iter().map(|(id, _)| id))
        {
            for (row, (_, label)) in self.menu.items().iter().zip(running) {
                if let Some(item) = row.as_menuitem() {
                    item.set_text(label);
                }
            }
            return;
        }
        for _ in 0..ids.len() + usize::from(self.divider.replace(false)) {
            self.menu.remove_at(0);
        }
        ids.clear();
        let has_commands = !self.menu.items().is_empty();
        for (id, name) in running {
            let item = MenuItem::new(name, true, None);
            if self.menu.insert(&item, ids.len()).is_ok() {
                ids.push((item.id().clone(), *id));
            }
        }
        if !ids.is_empty() && has_commands {
            self.divider.set(
                self.menu
                    .insert(&PredefinedMenuItem::separator(), ids.len())
                    .is_ok(),
            );
        }
    }
}

fn visible(cmd: TrayCmd, settings: MenuBarSettings) -> bool {
    match cmd {
        TrayCmd::Show => settings.main_window,
        TrayCmd::AddUrl => settings.add_url,
        TrayCmd::Batch => settings.batch,
        TrayCmd::BatchClipboard => settings.batch_clipboard,
        TrayCmd::LimiterOn | TrayCmd::LimiterOff => settings.limiter,
        TrayCmd::Scheduler => settings.scheduler,
        TrayCmd::StartQueue | TrayCmd::StopQueue => settings.queues,
        TrayCmd::Options => settings.options,
        TrayCmd::Exit => settings.quit,
        TrayCmd::Click(_) | TrayCmd::DoubleClick | TrayCmd::ShowDownload(_) => false,
    }
}

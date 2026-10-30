//! Tasks → Show drop target (D-021): IDM's small floating box that takes links dragged onto
//! it. Always on top, moved by dragging it, double-click brings the main window back,
//! right-click hides it. Browsers hand dragged links over through OLE drag and drop (the
//! "UniformResourceLocatorW" format, else text), which egui doesn't expose (it only takes
//! dropped files): the window gets its own `IDropTarget` (`win32::ole`, D-008; elsewhere it
//! shows but takes nothing yet).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};

use egui::{Pos2, Sense, ViewportBuilder, ViewportCommand, ViewportId, pos2, vec2};

/// The window's title (its OLE target is found by it).
const TITLE: &str = "Mister Download Manager drop target"; // not translated: an id
const SIZE: f32 = 56.0;

/// What was dropped: an address or a file path.
pub type Dropped = String;

pub enum Event {
    /// Something to download (an address, or a .torrent file's path).
    Drop(Dropped),
    /// Double-click: the main window.
    ShowMain,
    /// Right-click at this screen point: the app shows the drop target's menu there.
    Context(Pos2),
    /// Moved: where it is now (screen, logical points).
    Moved(Pos2),
}

pub struct DropTarget {
    rx: Receiver<Dropped>,
    /// For OLE drag and drop, how links from a browser arrive on Windows (winit passes only
    /// files on macOS and Linux).
    #[cfg_attr(not(windows), allow(dead_code))]
    tx: Sender<Dropped>,
    /// Something is being dragged over it (highlight).
    over: Arc<AtomicBool>,
    #[cfg(windows)]
    ole: Option<crate::win32::ole::Registration>,
    pos: Pos2,
}

impl DropTarget {
    pub fn new(pos: Option<Pos2>) -> Self {
        let (tx, rx) = channel();
        DropTarget {
            rx,
            tx,
            over: Arc::new(AtomicBool::new(false)),
            #[cfg(windows)]
            ole: None,
            pos: pos.unwrap_or(pos2(200.0, 200.0)),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, icon: &egui::ImageSource<'static>) -> Vec<Event> {
        let mut events: Vec<Event> = self.rx.try_iter().map(Event::Drop).collect();
        let builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_drag_and_drop(false)
            .with_position(self.pos)
            .with_inner_size(vec2(SIZE, SIZE));
        let over = self.over.load(Ordering::Relaxed);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("drop-target"), builder, |ui, _| {
            let rect = ui.max_rect();
            let p = ui.painter();
            let (fill, edge) = if over {
                (
                    egui::Color32::from_rgb(0xe5, 0xf3, 0xff),
                    egui::Color32::from_rgb(0x00, 0x78, 0xd4),
                )
            } else {
                (
                    crate::winui::BG(),
                    egui::Color32::from_rgb(0x8d, 0x8d, 0x8d),
                )
            };
            p.rect_filled(rect, 0.0, fill);
            p.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, edge),
                egui::StrokeKind::Inside,
            );
            egui::Image::new(icon.clone()).paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), vec2(40.0, 40.0)),
            );
            let resp = ui
                .interact(
                    rect,
                    ui.scope_id().with("drop-target"),
                    Sense::click_and_drag(),
                )
                .on_hover_text(crate::i18n::t("Drag links here to download them"));
            if resp.drag_started() {
                ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
            }
            if resp.double_clicked() {
                events.push(Event::ShowMain);
            }
            // The menu is the app's (a 56 px window can't hold one).
            if resp.secondary_clicked()
                && let (Some(at), Some(win)) = (
                    resp.interact_pointer_pos(),
                    ui.ctx().input(|i| i.viewport().inner_rect),
                )
            {
                events.push(Event::Context(win.min + at.to_vec2()));
            }
            if let Some(r) = ui.ctx().input(|i| i.viewport().outer_rect)
                && (r.min - self.pos).length() > 0.5
            {
                self.pos = r.min;
                events.push(Event::Moved(r.min));
            }
        });
        #[cfg(windows)]
        if self.ole.is_none() {
            self.ole =
                crate::win32::ole::Registration::new(TITLE, self.tx.clone(), self.over.clone());
        }
        events
    }
}

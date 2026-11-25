//! Zip preview (#1775): the files inside a ZIP on its server, listed from the archive's table of
//! contents (read with a few range requests, `mdm_engine::remotezip`), to tick those to
//! download instead of the whole archive. Opened by Preview in Download File Info, for a `.zip`
//! address whose server sends parts of files. Its columns: File Name (the path in the archive),
//! Packed, Size, Modified. OK hands the ticked files back to Download File Info, whose Start
//! adds them as one download: each fetched alone, checked against its CRC-32 and written under
//! a folder named after the archive, keeping its path there.
//!
//! Only files MDM can take out on their own are listed: stored or Deflate, not encrypted, not a
//! link, a name that stays in its folder; the others are counted under the list.

use std::sync::{Arc, Mutex};

use mdm_engine::remotezip::Listing;

use crate::dialogs::{self, Outcome};
use crate::winui::{self, ButtonKind, LIGHT_BG};

/// Rows the list shows at once.
const SHOWN: usize = 15;
/// The progress window's table sits at y 273; this window's starts near the top.
const TABLE_DY: f32 = 36.0 - 273.0;

/// What Zip preview's OK hands back: the files ticked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chosen {
    /// Their paths in the archive.
    pub files: Vec<String>,
    /// Their sizes together, unpacked.
    pub size: u64,
    /// The archive's size, as its table was read.
    pub archive_size: u64,
}

/// The table as read, or why it couldn't be (in the interface language).
type Read = Arc<Mutex<Option<Result<Listing, String>>>>;

/// The listed files: each row's cells, its path in the archive, its size, and ticked or not.
struct Rows {
    cells: Vec<Vec<String>>,
    names: Vec<String>,
    sizes: Vec<u64>,
    ticks: Vec<bool>,
    /// Files that can't be taken out on their own (encrypted, another method).
    refused: usize,
    /// Entries left out for names that would leave their folder, and links.
    left_out: usize,
    archive_size: u64,
    heading: String,
}

pub struct ZipPreview {
    /// Its window's id (one per Download File Info window).
    id: String,
    /// The archive's file name (the first line).
    archive: String,
    read: Read,
    rows: Option<Result<Rows, String>>,
    /// The files ticked when it opened (an earlier OK's).
    ticked: Vec<String>,
    /// The first files to tick once the table is read (`/show zip-preview:N`).
    tick_first: usize,
}

impl ZipPreview {
    /// For the archive `archive` (its name) at `url`: `listing` its table when Download File
    /// Info's look at its end already brought it, else the table is read now, as a download
    /// of it asks (`request`), its size `size` when known. `ticked`: the files ticked before.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        archive: String,
        url: url::Url,
        request: &crate::app::RequestParts,
        size: Option<u64>,
        listing: Option<Listing>,
        ticked: Vec<String>,
        rt: &tokio::runtime::Handle,
        ctx: &egui::Context,
    ) -> Self {
        let read: Read = Arc::new(Mutex::new(listing.map(Ok)));
        if read.lock().unwrap().is_none() {
            let (out, ctx) = (read.clone(), ctx.clone());
            let (headers, opts) = (request.headers.clone(), request.opts.clone());
            rt.spawn(async move {
                let got = mdm_engine::remotezip::list(&url, &headers, &opts, size)
                    .await
                    .map_err(|e| {
                        tracing::info!("zip preview: the table couldn't be read: {e}");
                        crate::errors::describe(&e)
                    });
                *out.lock().unwrap() = Some(got);
                ctx.request_repaint();
            });
        }
        ZipPreview {
            id,
            archive,
            read,
            rows: None,
            ticked,
            tick_first: 0,
        }
    }

    /// The table, once read, made rows (once: a table may hold 100 000 entries).
    fn take_read(&mut self) {
        if self.rows.is_some() {
            return;
        }
        let Some(got) = self.read.lock().unwrap().take() else {
            return;
        };
        let first = self.tick_first;
        self.rows = Some(got.map(|l| {
            let mut rows = rows_of(&l, &self.archive, &self.ticked);
            rows.ticks.iter_mut().take(first).for_each(|t| *t = true);
            rows
        }));
    }

    /// Tick the first `n` files once the table is read (`/show zip-preview:N`).
    pub fn tick_first(&mut self, n: usize) {
        self.tick_first = n;
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Outcome<Chosen> {
        self.take_read();
        let out = dialogs::window(
            ctx,
            &self.id,
            // i18n: title of the window that lists the files inside a ZIP on its server, to tick those to download (#1775)
            "Zip preview",
            [548.0, 440.0],
            false,
            LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let line = |ui: &mut egui::Ui, y: f32, id: &str, s: &str| {
                    let (rect, cut) =
                        winui::text_fit(&p, o.x + 18.0, o.y + y, s, winui::TEXT(), o.x + 528.0);
                    winui::copyable(ui, rect, (id, &self.id), s, cut);
                };
                let mut ok = false;
                match &mut self.rows {
                    None => line(
                        ui,
                        24.0,
                        "zip-preview-heading",
                        // i18n: Zip preview, while the ZIP's list of files is being read from its server
                        crate::i18n::t("Reading the ZIP's list of files..."),
                    ),
                    Some(Err(why)) => line(ui, 24.0, "zip-preview-heading", why),
                    Some(Ok(rows)) => {
                        line(ui, 24.0, "zip-preview-heading", &rows.heading);
                        let table_o = o + egui::vec2(0.0, TABLE_DY);
                        let body_bottom =
                            table_o.y + 299.0 + crate::progress::table_body_height(SHOWN);
                        if let Some(k) = crate::progress::zip_table(
                            ui,
                            &p,
                            table_o,
                            &format!("zip-preview-{}", self.id),
                            &rows.cells,
                            &rows.ticks,
                            SHOWN,
                        ) {
                            rows.ticks[k] = !rows.ticks[k];
                        }
                        let mut all = !rows.ticks.is_empty() && rows.ticks.iter().all(|t| *t);
                        let response = winui::checkbox(
                            ui,
                            egui::pos2(o.x + 20.0, body_bottom + 12.0),
                            &mut all,
                            crate::i18n::t("Select all"),
                            !rows.ticks.is_empty(),
                        );
                        if response.changed() {
                            rows.ticks.iter_mut().for_each(|t| *t = all);
                        }
                        let (n, size) = rows
                            .ticks
                            .iter()
                            .zip(&rows.sizes)
                            .filter(|(t, _)| **t)
                            .fold((0u64, 0u64), |(n, s), (_, z)| (n + 1, s + z));
                        let picked = crate::i18n::fmt(
                            // i18n: Zip preview, under the list: {files} how many files are ticked ("3 files"), {size} their size together ("12.4 MB")
                            crate::i18n::t("Ticked: {files}, {size}"),
                            &[
                                ("files", &crate::i18n::tn(n, "{n} file", "{n} files")),
                                ("size", &crate::model::size(size)),
                            ],
                        );
                        line(ui, body_bottom + 46.0 - o.y, "zip-preview-ticked", &picked);
                        if rows.refused + rows.left_out > 0 {
                            let note = crate::i18n::fmt(
                                // i18n: Zip preview, under the list: {n} files of the ZIP aren't listed because MDM can't take them out on their own: encrypted ones, ones packed in a way MDM can't unpack from the server, links, or names that would leave their folder; the whole archive still downloads
                                crate::i18n::t(
                                    "{n} more can't be taken out on their own (encrypted, packed another way, or links)",
                                ),
                                &[(
                                    "n",
                                    &crate::i18n::integer((rows.refused + rows.left_out) as u64),
                                )],
                            );
                            line(ui, body_bottom + 66.0 - o.y, "zip-preview-note", &note);
                        }
                        ok = n > 0;
                    }
                }
                let row = winui::buttons_light_right(
                    ui,
                    o,
                    528.0,
                    407.0,
                    &[
                        (
                            crate::i18n::mark("OK"),
                            if ok {
                                ButtonKind::Default
                            } else {
                                ButtonKind::Disabled
                            },
                        ),
                        (crate::i18n::mark("Cancel"), ButtonKind::Normal),
                    ],
                );
                if row[1].clicked() || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if ok && (row[0].clicked() || winui::enter_pressed(ui)) {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        );
        match out {
            Outcome::Open => Outcome::Open,
            Outcome::Closed => Outcome::Closed,
            Outcome::Done(()) => Outcome::Done(self.chosen()),
        }
    }

    /// The files ticked now.
    fn chosen(&self) -> Chosen {
        let Some(Ok(rows)) = &self.rows else {
            return Chosen::default();
        };
        let mut c = Chosen {
            archive_size: rows.archive_size,
            ..Chosen::default()
        };
        for ((name, size), t) in rows.names.iter().zip(&rows.sizes).zip(&rows.ticks) {
            if *t {
                c.files.push(name.clone());
                c.size += size;
            }
        }
        c
    }
}

/// The rows of `l`'s files MDM can take out, `ticked` ticked; the heading says the archive's
/// name, its files and their size.
fn rows_of(l: &Listing, archive: &str, ticked: &[String]) -> Rows {
    let mut rows = Rows {
        cells: Vec::new(),
        names: Vec::new(),
        sizes: Vec::new(),
        ticks: Vec::new(),
        refused: 0,
        left_out: l.left_out,
        archive_size: l.size,
        heading: String::new(),
    };
    for e in &l.entries {
        if e.dir {
            continue;
        }
        if e.link {
            rows.left_out += 1;
            continue;
        }
        if e.refused().is_some() {
            rows.refused += 1;
            continue;
        }
        let modified = e
            .modified()
            .and_then(|(y, mo, d, h, mi, s)| {
                chrono::NaiveDate::from_ymd_opt(i32::from(y), u32::from(mo), u32::from(d))?
                    .and_hms_opt(u32::from(h), u32::from(mi), u32::from(s))
            })
            .map(crate::regional::short_date_time)
            .unwrap_or_default();
        rows.cells.push(vec![
            e.name.clone(),
            crate::model::size(e.compressed),
            crate::model::size(e.size),
            modified,
        ]);
        rows.ticks.push(ticked.contains(&e.name));
        rows.names.push(e.name.clone());
        rows.sizes.push(e.size);
    }
    let total: u64 = rows.sizes.iter().sum();
    rows.heading = format!(
        "{}  ({}, {})",
        crate::i18n::isolate(archive),
        crate::i18n::tn(rows.names.len() as u64, "{n} file", "{n} files"),
        crate::model::size(total)
    );
    rows
}

//! Add or edit a category (D-021): the user's own (name, file types, folder) or a built-in
//! one's folder. Opened from Options → Save to (New, Edit...) and the Categories right-click
//! menu (Add category, Properties); Delete removes a user's category.

use std::path::Path;

use crate::dialogs::{Outcome, window};
use crate::model::{Category, CustomCategory};
use crate::settings::Settings;
use crate::winui::{self, ButtonKind, LIGHT_BG, LIGHT_DISABLED, LIGHT_TEXT, r};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    New,
    /// A category of the user's, or a built-in one (only its folder changes).
    Edit(Category),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CategoryEdit {
    pub target: Target,
    pub name: String,
    pub types: String,
    /// The folder typed (empty: the default one).
    pub folder: String,
}

pub struct CategoryDialog {
    target: Target,
    name: String,
    types: String,
    folder: String,
}

impl CategoryDialog {
    pub fn new() -> Self {
        CategoryDialog {
            target: Target::New,
            name: String::new(),
            types: String::new(),
            folder: String::new(),
        }
    }

    /// Edit `cat`, whose folder is `folder` now.
    pub fn edit(cat: Category, folder: &Path) -> Self {
        CategoryDialog {
            target: Target::Edit(cat),
            name: cat.label().into_owned(),
            types: match cat {
                Category::Custom(id) => crate::model::custom(id)
                    .map(|c| c.types)
                    .unwrap_or_default(),
                c => c.types_text(),
            },
            folder: folder.display().to_string(),
        }
    }

    /// `names`: every category's name now (a new name must be another).
    pub fn show(&mut self, ctx: &egui::Context, names: &[String]) -> Outcome<CategoryEdit> {
        let built_in = matches!(self.target, Target::Edit(c) if !c.is_custom());
        let title = match self.target {
            Target::New => crate::i18n::mark("Add category"),
            Target::Edit(_) => crate::i18n::mark("Category properties"),
        };
        let own = match self.target {
            Target::Edit(c) => c.label().into_owned(),
            Target::New => String::new(),
        };
        let out = window(
            ctx,
            "category-edit",
            title,
            [420.0, 196.0],
            false,
            LIGHT_BG(),
            |ui, o| {
                let p = ui.painter().clone();
                let label = |y: f32, s: &str| {
                    winui::text_right(&p, o.x + 90.0, o.y + y, crate::i18n::t(s), LIGHT_TEXT())
                };
                label(26.0, crate::i18n::mark("Name:"));
                winui::edit_light(
                    ui,
                    r(o, 98.0, 12.0, 310.0, 23.0),
                    &mut self.name,
                    !built_in,
                    false,
                );
                label(57.0, crate::i18n::mark("File types:"));
                winui::edit_light(
                    ui,
                    r(o, 98.0, 43.0, 310.0, 23.0),
                    &mut self.types,
                    !built_in,
                    false,
                );
                winui::text(
                    &p,
                    o.x + 98.0,
                    o.y + 80.0,
                    crate::i18n::t("Extensions separated by spaces, for example MP4 MKV R0*"),
                    LIGHT_DISABLED(),
                );
                label(112.0, crate::i18n::mark("Folder:"));
                winui::edit_light(
                    ui,
                    r(o, 98.0, 98.0, 229.0, 23.0),
                    &mut self.folder,
                    true,
                    false,
                );
                if winui::button_light(
                    ui,
                    r(o, 335.0, 99.0, 73.0, 21.0),
                    "Browse...",
                    ButtonKind::Normal,
                    false,
                )
                .clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_directory(&self.folder)
                        .pick_folder()
                {
                    self.folder = dir.display().to_string();
                }
                let name = self.name.trim();
                // An empty name only greys OK; a name in use says so.
                let taken = !built_in
                    && names
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(name) && !n.eq_ignore_ascii_case(&own));
                if taken {
                    let e = crate::i18n::t("There is already a category with this name");
                    winui::text(&p, o.x + 98.0, o.y + 142.0, e, winui::ERROR());
                }
                let valid = built_in || (!name.is_empty() && !taken);
                let ok = winui::button_light(
                    ui,
                    r(o, 254.0, 164.0, 73.0, 21.0),
                    "OK",
                    if valid {
                        ButtonKind::Default
                    } else {
                        ButtonKind::Disabled
                    },
                    false,
                )
                .clicked();
                let cancel = winui::button_light(
                    ui,
                    r(o, 335.0, 164.0, 73.0, 21.0),
                    "Cancel",
                    ButtonKind::Normal,
                    false,
                )
                .clicked();
                if cancel || winui::escape_pressed(ui) {
                    Outcome::Closed
                } else if valid && (ok || winui::enter_pressed(ui)) {
                    Outcome::Done(())
                } else {
                    Outcome::Open
                }
            },
        );
        match out {
            Outcome::Open => Outcome::Open,
            Outcome::Closed => Outcome::Closed,
            Outcome::Done(()) => Outcome::Done(CategoryEdit {
                target: self.target,
                name: self.name.trim().to_owned(),
                types: self.types.split_whitespace().collect::<Vec<_>>().join(" "),
                folder: self.folder.trim().to_owned(),
            }),
        }
    }
}

/// Every category's name now (for the dialog's "already exists" check).
pub fn names() -> Vec<String> {
    Category::all()
        .iter()
        .map(|c| c.label().into_owned())
        .collect()
}

/// Apply an edit to `settings` (the app's, or Options' draft) and to the registry; returns the
/// category. A folder equal to the default one is stored as "none" (it follows Downloads).
pub fn apply(settings: &mut Settings, e: CategoryEdit, downloads: &Path) -> Category {
    let cat = match e.target {
        Target::Edit(c) if !c.is_custom() => c,
        Target::Edit(Category::Custom(id)) => {
            if let Some(c) = settings.custom_categories.iter_mut().find(|c| c.id == id) {
                c.name = e.name.clone();
                c.types = e.types.clone();
            }
            Category::Custom(id)
        }
        _ => {
            let id = settings
                .custom_categories
                .iter()
                .map(|c| c.id + 1)
                .max()
                .unwrap_or(1);
            settings.custom_categories.push(CustomCategory {
                id,
                name: e.name.clone(),
                types: e.types.clone(),
                folder: None,
            });
            Category::Custom(id)
        }
    };
    crate::model::set_custom(&settings.custom_categories);
    let typed = e.folder.trim_end_matches('\\');
    let default = cat.default_folder(downloads).display().to_string();
    let own =
        (!typed.is_empty() && typed != default.trim_end_matches('\\')).then(|| typed.to_owned());
    settings.set_category_dir(cat, own);
    crate::model::set_custom(&settings.custom_categories);
    cat
}

/// Remove a user's category; its downloads become General (`items`), files stay put.
pub fn delete(settings: &mut Settings, id: u32, items: &mut [crate::model::Item]) {
    settings.custom_categories.retain(|c| c.id != id);
    crate::model::set_custom(&settings.custom_categories);
    for i in items {
        if i.category == Category::Custom(id) {
            i.category = Category::General;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_category_takes_its_files_and_folder() {
        let mut s = Settings::default();
        let downloads = Path::new(r"C:\Users\u\Downloads");
        let e = CategoryEdit {
            target: Target::New,
            name: "Books".into(),
            types: "EPUB MOBI".into(),
            folder: String::new(),
        };
        let cat = apply(&mut s, e, downloads);
        assert!(cat.is_custom());
        assert_eq!(Category::of("novel.epub"), cat, "its file types come first");
        assert_eq!(Category::of("a.zip"), Category::Compressed);
        assert_eq!(cat.label(), "Books");
        assert_eq!(cat.folder(downloads), downloads.join("Books"));
        // A folder of its own.
        let e = CategoryEdit {
            target: Target::Edit(cat),
            name: "Books".into(),
            types: "EPUB".into(),
            folder: r"D:\Library".into(),
        };
        apply(&mut s, e, downloads);
        assert_eq!(cat.folder(downloads), Path::new(r"D:\Library"));
        assert_eq!(
            Category::of("x.mobi"),
            Category::General,
            "MOBI was taken off"
        );
        let mut items: Vec<crate::model::Item> = Vec::new();
        let Category::Custom(id) = cat else {
            unreachable!()
        };
        delete(&mut s, id, &mut items);
        assert_eq!(Category::of("novel.epub"), Category::General);
        crate::model::set_custom(&[]);
    }

    #[test]
    fn torrents_have_a_folder_but_no_file_types() {
        let downloads = Path::new(r"C:\d");
        assert_eq!(
            Category::Torrents.folder(downloads),
            downloads.join("Torrents")
        );
        assert_eq!(Category::of("ubuntu.iso.torrent"), Category::General);
        assert!(Category::all().contains(&Category::Torrents));
    }
}

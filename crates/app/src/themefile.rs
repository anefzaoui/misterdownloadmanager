//! Theme files (#45): TOML or JSON in the data folder's `themes` folder, read into a
//! [`Theme`](crate::palette::Theme). A file names only what it changes; the rest comes from the
//! theme it `inherits` (a built-in's id or another file's name; the classic look by default).
//!
//! ```toml
//! name = "My theme"
//! inherits = "classic"
//! icons = "mono"            # color | mono | accent
//!
//! [sizes]
//! radius = 4
//!
//! [colors]                  # both variants
//! accent = "#2f7d32"
//!
//! [light]                   # the light variant only
//! window = "#f3f3f3"
//!
//! [dark]                    # the dark variant only
//! window = "#202020"
//! "segments.done" = "#3fa864"   # one colour, or a list of 13 (top to bottom)
//! ```
//!
//! Unknown keys and values that can't be read are skipped and reported as warnings, so a file
//! written for a newer MDM still loads; a file that isn't TOML or JSON is an error.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::palette::{self, IconStyle, Palette, Theme};

/// A theme read from a file, with what in it was skipped.
#[derive(Debug)]
pub struct Loaded {
    pub theme: Theme,
    pub warnings: Vec<String>,
}

/// Where theme files are: the data folder's `themes` (portable copies' next to the program,
/// `MDM_DATA_DIR`'s in tests). None when there is no data folder (built-ins only).
pub fn dir() -> Option<PathBuf> {
    mdm_ipc::data_dir().map(|d| d.join("themes"))
}

/// A theme the user can pick: a built-in, or a file in [`dir`].
// The theme picker (#45, next) lists them.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// The file, for a theme from one.
    pub path: Option<PathBuf>,
}

/// The built-in themes, then the theme files in [`dir`] (by name), each once: a file named as a
/// built-in is skipped (the built-in keeps its id).
#[allow(dead_code)]
pub fn list() -> Vec<Entry> {
    let mut out: Vec<Entry> = palette::builtins()
        .iter()
        .map(|t| Entry {
            id: t.id.to_string(),
            name: t.name.to_string(),
            path: None,
        })
        .collect();
    let mut files: Vec<(String, PathBuf)> = dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let id = theme_id(&path)?;
            Some((id, path))
        })
        .collect();
    files.sort();
    for (id, path) in files {
        if out.iter().any(|e| e.id == id) {
            continue;
        }
        // The name the file gives itself, else its file name.
        let name = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| read_value(&path, &t).ok())
            .and_then(|v| v.get("name").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| id.clone());
        out.push(Entry {
            id,
            name,
            path: Some(path),
        });
    }
    out
}

/// A theme file's id: its name without `.toml` or `.json`; None for any other file.
fn theme_id(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "toml" && ext != "json" {
        return None;
    }
    Some(path.file_stem()?.to_str()?.to_owned())
}

/// The theme with this id: a built-in, or the file of that name in [`dir`].
pub fn load(id: &str) -> Result<Loaded, String> {
    let folder = dir().unwrap_or_default();
    load_in(&folder, id, 0)
}

/// The theme in this file (the themes it inherits from beside it).
pub fn load_file(path: &Path) -> Result<Loaded, String> {
    let id =
        theme_id(path).ok_or_else(|| format!("{}: not a .toml or .json file", path.display()))?;
    load_in(path.parent().unwrap_or(Path::new(".")), &id, 0)
}

/// [`load`] from `folder`, `depth` themes down an `inherits` chain.
fn load_in(folder: &Path, id: &str, depth: usize) -> Result<Loaded, String> {
    if let Some(t) = palette::builtins().into_iter().find(|t| t.id == id) {
        return Ok(Loaded {
            theme: t.clone(),
            warnings: Vec::new(),
        });
    }
    if depth > 8 {
        return Err(format!("{id}: too many themes inherit one another"));
    }
    let path = ["toml", "json"]
        .iter()
        .map(|ext| folder.join(format!("{id}.{ext}")))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{id}: no such theme"))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let value = read_value(&path, &text)?;
    parse(id, &value, |base| load_in(folder, base, depth + 1))
}

/// A file's contents as one value tree: TOML, or JSON for a `.json` file.
fn read_value(path: &Path, text: &str) -> Result<Value, String> {
    let json = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if json {
        serde_json::from_str(text).map_err(|e| format!("{}: {e}", path.display()))
    } else {
        toml::from_str(text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// A theme from a file's value tree; `base` loads the theme it inherits.
pub fn parse(
    id: &str,
    v: &Value,
    base: impl FnOnce(&str) -> Result<Loaded, String>,
) -> Result<Loaded, String> {
    let top = v
        .as_object()
        .ok_or_else(|| format!("{id}: a theme is a table of keys"))?;
    let mut warnings = Vec::new();
    let inherits = match top.get("inherits") {
        None => "classic",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(format!("{id}: inherits names a theme")),
    };
    if inherits == id {
        return Err(format!("{id}: a theme can't inherit itself"));
    }
    let Loaded {
        mut theme,
        warnings: inherited,
    } = base(inherits)?;
    warnings.extend(inherited);
    theme.id = id.to_owned().into();
    theme.name = match top.get("name").and_then(Value::as_str) {
        Some(n) => n.to_owned().into(),
        None => id.to_owned().into(),
    };
    for (key, value) in top {
        match key.as_str() {
            "name" | "inherits" => {}
            "icons" => match value.as_str().and_then(IconStyle::from_key) {
                Some(s) => theme.icons = s,
                None => warnings.push(format!("icons: {value} is not color, mono or accent")),
            },
            "sizes" => sizes(value, &mut theme, &mut warnings),
            "colors" | "colours" => {
                colors(key, value, &mut theme.light, &mut warnings);
                colors(key, value, &mut theme.dark, &mut warnings);
            }
            "light" => colors(key, value, &mut theme.light, &mut warnings),
            "dark" => colors(key, value, &mut theme.dark, &mut warnings),
            _ => warnings.push(format!("{key}: not a theme key")),
        }
    }
    Ok(Loaded { theme, warnings })
}

fn sizes(v: &Value, theme: &mut Theme, warnings: &mut Vec<String>) {
    let Some(table) = v.as_object() else {
        warnings.push("sizes: not a table".into());
        return;
    };
    for (key, value) in table {
        match value.as_f64() {
            // A size from 0 to 400 points (a tooltip's width is the largest).
            Some(n) if (0.0..=400.0).contains(&n) => {
                if !theme.metrics.set(key, n as f32) {
                    warnings.push(format!("sizes.{key}: not a size"));
                }
            }
            _ => warnings.push(format!("sizes.{key}: {value} is not a size")),
        }
    }
}

/// A `[colors]`, `[light]` or `[dark]` table into `pal`.
fn colors(table_name: &str, v: &Value, pal: &mut Palette, warnings: &mut Vec<String>) {
    let Some(table) = v.as_object() else {
        warnings.push(format!("{table_name}: not a table"));
        return;
    };
    for (key, value) in table {
        let flag = match key.as_str() {
            "dark" => Some(&mut pal.dark),
            "focus-line-straight" => Some(&mut pal.focus_line_straight),
            "access-keys-always" => Some(&mut pal.access_keys_always),
            "tree-chevrons" => Some(&mut pal.tree_chevrons),
            _ => None,
        };
        if let Some(flag) = flag {
            match value.as_bool() {
                Some(b) => *flag = b,
                None => warnings.push(format!("{table_name}.{key}: true or false")),
            }
            continue;
        }
        let rows = match key.as_str() {
            "segments.missing" => Some(&mut pal.segments.missing),
            "segments.done" => Some(&mut pal.segments.done),
            "segments.start" => Some(&mut pal.segments.start),
            _ => None,
        };
        if let Some(rows) = rows {
            if !gradient(value, rows) {
                warnings.push(format!(
                    "{table_name}.{key}: one colour or a list of 13 colours"
                ));
            }
            continue;
        }
        match value.as_str().and_then(palette::parse_hex) {
            Some(c) => {
                if !pal.set_role(key, c) {
                    warnings.push(format!("{table_name}.{key}: not a colour role"));
                }
            }
            None => warnings.push(format!(
                "{table_name}.{key}: {value} is not a colour (#rrggbb or #rrggbbaa)"
            )),
        }
    }
}

/// One colour for every row, or 13 colours top to bottom.
fn gradient(v: &Value, rows: &mut [egui::Color32; 13]) -> bool {
    if let Some(c) = v.as_str().and_then(palette::parse_hex) {
        *rows = [c; 13];
        return true;
    }
    let Some(list) = v.as_array().filter(|l| l.len() == 13) else {
        return false;
    };
    let parsed: Option<Vec<_>> = list
        .iter()
        .map(|c| c.as_str().and_then(palette::parse_hex))
        .collect();
    match parsed {
        Some(cs) => {
            rows.copy_from_slice(&cs);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::CLASSIC;
    use egui::Color32;

    fn toml_theme(id: &str, text: &str) -> Result<Loaded, String> {
        let v: Value = toml::from_str(text).map_err(|e| e.to_string())?;
        parse(id, &v, |base| load_in(Path::new("nowhere"), base, 1))
    }

    #[test]
    fn a_file_changes_only_what_it_names() {
        let l = toml_theme(
            "mine",
            r##"
            name = "Mine"
            icons = "mono"
            [sizes]
            radius = 5
            "list.row" = 20
            [colors]
            accent = "#2f7d32"
            [dark]
            window = "202020"
            "segments.done" = "#3fa864"
            "##,
        )
        .unwrap();
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        let t = l.theme;
        assert_eq!((t.id.as_ref(), t.name.as_ref()), ("mine", "Mine"));
        assert_eq!(t.icons, IconStyle::Mono);
        assert_eq!(t.metrics.control_radius, 5.0);
        assert_eq!(t.metrics.list_row, 20.0);
        assert_eq!(t.metrics.font_dialog, CLASSIC.metrics.font_dialog);
        let green = Color32::from_rgb(0x2f, 0x7d, 0x32);
        assert_eq!((t.light.accent, t.dark.accent), (green, green));
        assert_eq!(t.dark.window, Color32::from_gray(0x20));
        assert_eq!(t.light.window, CLASSIC.light.window);
        assert_eq!(
            t.dark.segments.done,
            [Color32::from_rgb(0x3f, 0xa8, 0x64); 13]
        );
        assert_eq!(t.dark.text, CLASSIC.dark.text);
    }

    #[test]
    fn json_reads_the_same() {
        let v: Value = serde_json::from_str(
            r##"{ "name": "J", "light": { "accent": "#ff0000", "dark": true } }"##,
        )
        .unwrap();
        let l = parse("j", &v, |b| load_in(Path::new("nowhere"), b, 1)).unwrap();
        assert_eq!(l.theme.light.accent, Color32::RED);
        assert!(l.theme.light.dark);
    }

    #[test]
    fn unknown_keys_and_bad_values_are_warnings() {
        let l = toml_theme(
            "w",
            r##"
            future = 1
            icons = "sparkly"
            [sizes]
            nope = 3
            radius = -1
            [light]
            "no.such.role" = "#000000"
            text = "black"
            "segments.missing" = ["#000000"]
            dark = "yes"
            "##,
        )
        .unwrap();
        assert_eq!(l.warnings.len(), 8, "{:?}", l.warnings);
        assert_eq!(l.theme.light, CLASSIC.light);
        assert_eq!(l.theme.metrics, CLASSIC.metrics);
    }

    #[test]
    fn broken_files_and_loops_are_errors() {
        assert!(toml_theme("x", "name = ").is_err());
        assert!(toml_theme("x", r#"inherits = "x""#).is_err());
        assert!(toml_theme("x", r#"inherits = "not-there""#).is_err());
    }

    #[test]
    fn files_in_the_folder_inherit_one_another() {
        let dir = std::env::temp_dir().join(format!("mdm-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base.toml"), "[colors]\naccent = \"#112233\"\n").unwrap();
        std::fs::write(
            dir.join("child.json"),
            r##"{ "inherits": "base", "dark": { "text": "#eeeeee" } }"##,
        )
        .unwrap();
        std::fs::write(dir.join("a.toml"), "inherits = \"b\"").unwrap();
        std::fs::write(dir.join("b.toml"), "inherits = \"a\"").unwrap();
        let t = load_in(&dir, "child", 0).unwrap().theme;
        assert_eq!(t.light.accent, Color32::from_rgb(0x11, 0x22, 0x33));
        assert_eq!(t.dark.text, Color32::from_gray(0xee));
        assert_eq!(load_file(&dir.join("child.json")).unwrap().theme, t);
        assert!(load_file(&dir.join("child.txt")).is_err());
        assert!(load_in(&dir, "a", 0).is_err());
        assert_eq!(load_in(&dir, "classic", 0).unwrap().theme, CLASSIC);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

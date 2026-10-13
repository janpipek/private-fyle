//! Config file loading, creation and comment-preserving settings persistence.
//! Blocking operations here belong in background work when called from the GUI.
//! Config paths use XDG on Linux, APPDATA on Windows and Application Support on macOS.

use std::path::{Path, PathBuf};

use super::{EXAMPLE, KeyContext, KeymapEntry, Settings};
use crate::grid::ViewMode;
use crate::workspace::LayoutMode;

/// `<config dir>/fyle/config.toml`, if the platform has a config dir.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("fyle").join("config.toml"))
}

/// Loads the settings file. A missing file gives defaults; an unreadable
/// or invalid one gives defaults plus an error message.
pub fn load() -> (Settings, Option<String>) {
    match path() {
        Some(path) => load_from(&path),
        None => (Settings::default(), None),
    }
}

/// Loads a supplied file, returning defaults and a path-qualified error on failure.
pub fn load_from(path: &Path) -> (Settings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => {
            return (
                Settings::default(),
                Some(format!("{}: {e}", path.display())),
            );
        }
    };
    match Settings::parse(&text) {
        Ok(settings) => (settings, None),
        Err(e) => (
            Settings::default(),
            Some(format!("{}: {e}", path.display())),
        ),
    }
}

/// Writes `theme = "<name>"` into the settings file, keeping the rest of the
/// file (comments, formatting, other keys) intact. Creates the file if needed.
pub fn save_theme(name: &str) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_theme_to(&path, name)?;
    Ok(path)
}

/// Saves the theme at a supplied config path, creating parent directories if needed.
pub fn save_theme_to(path: &Path, name: &str) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    doc["theme"] = toml_edit::value(name);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

/// Saves the custom title bar preference while keeping other settings intact.
pub fn save_custom_titlebar(enabled: bool) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_custom_titlebar_to(&path, enabled)?;
    Ok(path)
}

/// Saves the custom title bar preference at a supplied config path.
pub fn save_custom_titlebar_to(path: &Path, enabled: bool) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    match doc.get_mut("window") {
        Some(item) => {
            if let Some(window) = item.as_table_mut() {
                set_table_bool(window, "custom_titlebar", enabled);
            } else if let Some(window) = item
                .as_value_mut()
                .and_then(toml_edit::Value::as_inline_table_mut)
            {
                if let Some(value) = window.get_mut("custom_titlebar") {
                    replace_bool_value(value, enabled);
                } else {
                    window.insert("custom_titlebar", enabled.into());
                }
            } else {
                return Err(error(&"window settings must be a table"));
            }
        }
        None => {
            doc["window"] = toml_edit::table();
            doc["window"]["custom_titlebar"] = toml_edit::value(enabled);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

fn set_table_bool(table: &mut toml_edit::Table, key: &str, enabled: bool) {
    if let Some(value) = table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
        replace_bool_value(value, enabled);
    } else {
        table[key] = toml_edit::value(enabled);
    }
}

fn replace_bool_value(value: &mut toml_edit::Value, enabled: bool) {
    let decor = value.decor().clone();
    *value = enabled.into();
    *value.decor_mut() = decor;
}

/// Writes the edited keymap to the config file, preserving unrelated settings.
pub fn save_keymap(entries: &[KeymapEntry]) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_keymap_to(&path, entries)?;
    Ok(path)
}

/// Writes keymap overrides while retaining existing table comments where rows remain.
pub fn save_keymap_to(path: &Path, entries: &[KeymapEntry]) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    if !doc
        .get("keymap")
        .is_some_and(toml_edit::Item::is_array_of_tables)
    {
        doc["keymap"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let tables = doc["keymap"]
        .as_array_of_tables_mut()
        .expect("keymap set to array of tables");
    while tables.len() > entries.len() {
        tables.remove(tables.len() - 1);
    }
    for (index, entry) in entries.iter().enumerate() {
        if index == tables.len() {
            tables.push(toml_edit::Table::new());
        }
        let table = tables
            .get_mut(index)
            .expect("table was just added or already exists");
        set_table_string(table, "keys", &entry.keys);
        set_table_string(table, "action", &entry.action);
        if entry.context == KeyContext::Panel {
            table.remove("context");
        } else {
            set_table_string(
                table,
                "context",
                match entry.context {
                    KeyContext::Panel => "panel",
                    KeyContext::Global => "global",
                    KeyContext::Editor => "editor",
                },
            );
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

fn set_table_string(table: &mut toml_edit::Table, key: &str, value: &str) {
    if let Some(existing) = table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
        let decor = existing.decor().clone();
        let toml_edit::Item::Value(replacement) = toml_edit::value(value) else {
            unreachable!("toml_edit::value creates a value item")
        };
        *existing = replacement;
        *existing.decor_mut() = decor;
    } else {
        table[key] = toml_edit::value(value);
    }
}

/// Writes `font_size = <size>` into the settings file, keeping the rest of
/// the file intact. Creates the file if needed.
pub fn save_font_size(size: f32) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_font_size_to(&path, size)?;
    Ok(path)
}

/// Saves the font size at a supplied config path without changing other keys.
pub fn save_font_size_to(path: &Path, size: f32) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    doc["font_size"] = toml_edit::value(f64::from(size));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

/// The settings file, created from [`super::EXAMPLE`] if it does not exist yet
/// (so Help › Configuration always has something to open).
pub fn ensure_file() -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    ensure_file_at(&path)?;
    Ok(path)
}

/// Creates the example at a supplied path only when the file does not exist.
pub fn ensure_file_at(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    let error = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(error)?;
    }
    std::fs::write(path, EXAMPLE).map_err(error)
}

/// Writes `[panels] columns` into the settings file, keeping the rest of
/// the file intact. Creates the file if needed.
pub fn save_columns(columns: &[crate::columns::ColumnKind]) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_columns_to(&path, columns)?;
    Ok(path)
}

/// Saves visible columns at a supplied config path without changing other panel keys.
pub fn save_columns_to(path: &Path, columns: &[crate::columns::ColumnKind]) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    if !doc.contains_table("panels") {
        doc["panels"] = toml_edit::table();
    }
    let ids: toml_edit::Array = columns.iter().map(|c| c.id()).collect();
    doc["panels"]["columns"] = toml_edit::value(ids);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

/// Writes `[panels] left_view` or `right_view` (panel `ix`) into the
/// settings file, keeping the rest of the file intact.
pub fn save_panel_view(ix: usize, mode: ViewMode) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_panel_view_to(&path, ix, mode)?;
    Ok(path)
}

/// Saves a panel view mode at a supplied config path.
pub fn save_panel_view_to(path: &Path, ix: usize, mode: ViewMode) -> Result<(), String> {
    let key = if ix == 0 { "left_view" } else { "right_view" };
    save_panel_value_to(path, key, mode.id().into())
}

/// Writes `[panels] icon_size` into the settings file, keeping the rest of
/// the file intact.
pub fn save_icon_size(size: u32) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_icon_size_to(&path, size)?;
    Ok(path)
}

/// Saves the icon size at a supplied config path.
pub fn save_icon_size_to(path: &Path, size: u32) -> Result<(), String> {
    save_panel_value_to(path, "icon_size", i64::from(size).into())
}

/// Saves the chosen workspace layout while preserving other settings.
pub fn save_workspace_layout(layout: LayoutMode) -> Result<PathBuf, String> {
    let path = path().ok_or("No config directory on this platform")?;
    save_workspace_layout_to(&path, layout)?;
    Ok(path)
}

/// Writes `[workspace] layout` and keeps the other settings and comments.
pub fn save_workspace_layout_to(path: &Path, layout: LayoutMode) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    match doc.get_mut("workspace") {
        Some(item) => {
            if let Some(workspace) = item.as_table_mut() {
                if let Some(value) = workspace
                    .get_mut("layout")
                    .and_then(toml_edit::Item::as_value_mut)
                {
                    replace_layout_value(value, layout);
                } else {
                    workspace["layout"] = toml_edit::value(layout.id());
                }
            } else if let Some(workspace) = item
                .as_value_mut()
                .and_then(toml_edit::Value::as_inline_table_mut)
            {
                if let Some(value) = workspace.get_mut("layout") {
                    replace_layout_value(value, layout);
                } else {
                    workspace.insert("layout", layout.id().into());
                }
            } else {
                return Err(format!("{}: [workspace] must be a table", path.display()));
            }
        }
        None => {
            doc["workspace"] = toml_edit::table();
            let workspace = doc["workspace"]
                .as_table_mut()
                .expect("workspace was just set to a table");
            workspace["layout"] = toml_edit::value(layout.id());
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

fn replace_layout_value(value: &mut toml_edit::Value, layout: LayoutMode) {
    let decor = value.decor().clone();
    *value = layout.id().into();
    *value.decor_mut() = decor;
}

fn save_panel_value_to(path: &Path, key: &str, value: toml_edit::Value) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(&e)),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| error(&e))?;
    if !doc.contains_table("panels") {
        doc["panels"] = toml_edit::table();
    }
    doc["panels"][key] = toml_edit::Item::Value(value);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| error(&e))?;
    }
    std::fs::write(path, doc.to_string()).map_err(|e| error(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::Location;

    #[test]
    fn load_failures_return_defaults_with_path_qualified_errors() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        for contents in [b"theme = [".as_slice(), b"theme = 1", b"\xff"] {
            std::fs::write(&path, contents).unwrap();
            let (settings, error) = load_from(&path);
            assert_eq!(settings, Settings::default());
            assert!(error.unwrap().starts_with(&format!("{}: ", path.display())));
            assert_eq!(std::fs::read(&path).unwrap(), contents);
        }
        let (settings, error) = load_from(temp.path());
        assert_eq!(settings, Settings::default());
        assert!(
            error
                .unwrap()
                .starts_with(&format!("{}: ", temp.path().display()))
        );
    }

    #[test]
    fn all_savers_leave_invalid_config_files_untouched() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        type Save = fn(&Path) -> Result<(), String>;
        let savers: [Save; 8] = [
            |path| save_theme_to(path, "dark"),
            |path| save_custom_titlebar_to(path, false),
            |path| save_keymap_to(path, &[]),
            |path| save_font_size_to(path, 16.),
            |path| save_columns_to(path, &[]),
            |path| save_panel_view_to(path, 0, ViewMode::Icons),
            |path| save_icon_size_to(path, 64),
            |path| save_workspace_layout_to(path, LayoutMode::TwoPanelsInspector),
        ];
        for contents in [b"theme = [".as_slice(), b"\xff"] {
            std::fs::write(&path, contents).unwrap();
            for save in savers {
                let error = save(&path).unwrap_err();
                assert!(error.starts_with(&format!("{}: ", path.display())));
                assert_eq!(std::fs::read(&path).unwrap(), contents);
            }
        }
    }

    #[test]
    fn saving_preserves_unknown_settings_as_well_as_comments() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        let unrelated = "future_setting = 'keep me' # user setting\n";
        std::fs::write(&path, format!("{unrelated}theme = \"light\"\n")).unwrap();
        save_theme_to(&path, "dark").unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains(unrelated));
        assert!(saved.contains("theme = \"dark\""));
    }

    #[test]
    fn saving_custom_titlebar_preserves_comments_and_other_window_settings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep this\n[window]\nwidth = 900 # keep width comment\ncustom_titlebar = true # keep titlebar comment\n",
        )
        .expect("write config");
        save_custom_titlebar_to(&path, false).expect("save preference");
        let saved = std::fs::read_to_string(&path).expect("read config");
        assert!(saved.contains("# keep this"));
        assert!(saved.contains("width = 900 # keep width comment"));
        assert!(saved.contains("custom_titlebar = false # keep titlebar comment"));
        assert!(!load_from(&path).0.window.custom_titlebar);
    }

    #[test]
    fn saving_custom_titlebar_supports_inline_window_tables() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "window = { width = 900, custom_titlebar = true }\n")
            .expect("write config");
        save_custom_titlebar_to(&path, false).expect("save preference");
        let settings = load_from(&path).0;
        assert_eq!(settings.window.width, 900.);
        assert!(!settings.window.custom_titlebar);
    }

    #[test]
    fn saving_custom_titlebar_rejects_non_table_window_without_changing_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        let contents = "window = \"invalid\" # retain this file\n";
        std::fs::write(&path, contents).expect("write config");
        let error = save_custom_titlebar_to(&path, false).expect_err("window is not a table");
        assert!(error.starts_with(&format!("{}: ", path.display())));
        assert_eq!(
            std::fs::read_to_string(path).expect("read config"),
            contents
        );
    }

    #[test]
    fn save_keymap_keeps_other_settings_and_row_comments() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# retain me\ntheme = \"dark\"\n\n# shortcut comment\n[[keymap]]\nkeys = \"ctrl-c\"\naction = \"Copy\"\n").expect("write config");
        save_keymap_to(
            &path,
            &[KeymapEntry {
                keys: "ctrl-alt-c".into(),
                action: "Copy".into(),
                context: KeyContext::Panel,
            }],
        )
        .expect("save keymap");
        let saved = std::fs::read_to_string(path).expect("read config");
        assert!(saved.contains("# retain me"));
        assert!(saved.contains("# shortcut comment"));
        assert!(saved.contains("theme = \"dark\""));
        assert!(saved.contains("keys = \"ctrl-alt-c\""));
    }

    #[test]
    fn saving_empty_keymap_removes_overrides_and_keeps_config_comments() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep this\ntheme = \"dark\"\n\n[[keymap]]\nkeys = \"ctrl-c\"\naction = \"Copy\"\n",
        )
        .expect("write config");
        save_keymap_to(&path, &[]).expect("save empty keymap");
        let saved = std::fs::read_to_string(&path).expect("read config");
        assert!(saved.contains("# keep this"));
        assert!(saved.contains("theme = \"dark\""));
        let parsed = Settings::parse(&saved).expect("parse saved config");
        assert_eq!(parsed.theme, "dark");
        assert!(parsed.keymap.is_empty());
    }

    #[test]
    fn saving_first_keymap_entry_creates_the_table() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        save_keymap_to(
            &path,
            &[KeymapEntry {
                keys: "ctrl-alt-o".into(),
                action: "Open".into(),
                context: KeyContext::Panel,
            }],
        )
        .expect("save first shortcut");
        let parsed = load_from(&path).0;
        assert_eq!(parsed.keymap.len(), 1);
        assert_eq!(parsed.keymap[0].keys, "ctrl-alt-o");
    }

    #[test]
    fn saves_workspace_layout_without_losing_other_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            "# Keep this\n[workspace]\nlayout = \"two_panels\" # layout\n[editor]\ncommand = []\n",
        )
        .unwrap();
        save_workspace_layout_to(&path, LayoutMode::OnePanelInspector).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Keep this"));
        assert!(text.contains("# layout"));
        assert!(text.contains("command = []"));
        assert_eq!(
            Settings::parse(&text).unwrap().workspace.layout,
            LayoutMode::OnePanelInspector
        );
    }

    #[test]
    fn saves_workspace_layout_when_config_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/config.toml");
        save_workspace_layout_to(&path, LayoutMode::TwoPanelsInspector).unwrap();
        assert_eq!(
            load_from(&path).0.workspace.layout,
            LayoutMode::TwoPanelsInspector
        );
    }

    #[test]
    fn saves_workspace_layout_in_an_inline_table() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            "show_hidden = true\nworkspace = { layout = \"two_panels\" }\n",
        )
        .unwrap();
        save_workspace_layout_to(&path, LayoutMode::OnePanelInspector).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("show_hidden = true"));
        assert_eq!(
            Settings::parse(&text).unwrap().workspace.layout,
            LayoutMode::OnePanelInspector
        );
    }

    #[test]
    fn save_theme_keeps_comments_and_other_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("fyle").join("config.toml");

        save_theme_to(&path, "Ayu Dark").unwrap(); // creates file and dirs
        assert_eq!(load_from(&path).0.theme, "Ayu Dark");

        std::fs::write(
            &path,
            "# mine\nshow_hidden = true # keep\ntheme = \"dark\"\n",
        )
        .unwrap();
        save_theme_to(&path, "Tokyo Night").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine") && text.contains("show_hidden = true # keep"));
        let settings = Settings::parse(&text).unwrap();
        assert_eq!(settings.theme, "Tokyo Night");
        assert!(settings.show_hidden);
    }

    #[test]
    fn save_font_size_keeps_other_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("config.toml");
        save_font_size_to(&path, 17.5).unwrap(); // no file yet
        assert_eq!(load_from(&path).0.font_size, 17.5);

        std::fs::write(&path, "theme = \"dark\" # mine\nfont_size = 16\n").unwrap();
        save_font_size_to(&path, 13.0).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("theme = \"dark\" # mine"), "{text}");
        assert_eq!(Settings::parse(&text).unwrap().font_size, 13.0);
    }

    #[test]
    fn ensure_file_creates_the_example_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("fyle").join("config.toml");
        ensure_file_at(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
        std::fs::write(&path, "theme = \"dark\"\n").unwrap();
        ensure_file_at(&path).unwrap(); // existing file untouched
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme = \"dark\"\n"
        );
    }

    #[test]
    fn save_columns_keeps_other_panel_settings() {
        use crate::columns::{ColumnKind, ExifField};
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("config.toml");
        let columns = [
            ColumnKind::Name,
            ColumnKind::Size,
            ColumnKind::Exif(ExifField::Iso),
        ];
        save_columns_to(&path, &columns).unwrap(); // no file yet
        assert_eq!(load_from(&path).0.panels.columns, columns);

        std::fs::write(
            &path,
            "[panels]\ntree = true # mine\ncolumns = [\"name\"]\n",
        )
        .unwrap();
        save_columns_to(&path, &columns).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("tree = true # mine"), "{text}");
        assert!(text.contains(r#""exif.iso""#), "{text}");
        assert_eq!(Settings::parse(&text).unwrap().panels.columns, columns);
    }

    #[test]
    fn save_view_and_icon_size_keep_other_panel_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        save_panel_view_to(&path, 1, ViewMode::Icons).unwrap(); // no file yet
        save_icon_size_to(&path, 160).unwrap();
        let panels = load_from(&path).0.panels;
        assert_eq!(
            (panels.view(0), panels.view(1)),
            (ViewMode::List, ViewMode::Icons)
        );
        assert_eq!(panels.icon_size, 160);

        std::fs::write(
            &path,
            "[panels]\ntree = true # mine\nleft_view = \"list\"\nicon_size = 96\n",
        )
        .unwrap();
        save_panel_view_to(&path, 0, ViewMode::Icons).unwrap();
        save_icon_size_to(&path, 64).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("tree = true # mine"), "{text}");
        let panels = Settings::parse(&text).unwrap().panels;
        assert_eq!(panels.view(0), ViewMode::Icons);
        assert_eq!(panels.icon_size, 64);
        assert!(panels.tree);
        assert!(Settings::parse("[panels]\nleft_view = \"grid\"").is_err());
    }

    #[test]
    fn bookmarks_are_added_once_and_parsed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("config.toml");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(&path, "# mine\ntheme = \"dark\"\n").unwrap();
        let a = Location::local(dir.join("a")).unwrap();
        let b = Location::local(dir.join("b")).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &a).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &b).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &a).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine"));
        let settings = Settings::parse(&text).unwrap();
        assert_eq!(settings.bookmarks, [a, b]);
        assert_eq!(settings.theme, "dark");
    }

    #[test]
    fn encoded_bookmarks_preserve_native_bytes_and_legacy_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let legacy = Location::local(tmp.path().join("legacy")).unwrap();
        let legacy_value = toml_edit::value(legacy.panel_path().to_string_lossy().as_ref());
        std::fs::write(
            &path,
            format!("# keep this comment\nbookmarks = [{legacy_value}]\n"),
        )
        .unwrap();
        #[cfg(unix)]
        let unusual = {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from("/tmp").join(OsString::from_vec(vec![b'a', 0xff, b'\t', b'\n']))
        };
        #[cfg(windows)]
        let unusual = PathBuf::from("C:\\tmp\\a\tb\nc");

        let unusual = Location::local(unusual).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &unusual).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &unusual).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep this comment"));
        assert!(text.contains(crate::location_codec::PREFIX));
        assert_eq!(Settings::parse(&text).unwrap().bookmarks, [legacy, unusual]);
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let (settings, error) = load_from(&temp.path().join("missing/config.toml"));
        assert_eq!(settings, Settings::default());
        assert_eq!(error, None);
    }

    #[test]
    fn rejects_non_table_workspace_when_saving_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "workspace = false\n").unwrap();
        let error = save_workspace_layout_to(&path, LayoutMode::OnePanelInspector).unwrap_err();
        assert!(error.contains("[workspace] must be a table"));
    }
}

//! User settings, read from `config.toml` in the platform's config directory:
//!
//! - Linux: `$XDG_CONFIG_HOME/fyle/config.toml` (default `~/.config/fyle/config.toml`)
//! - Windows: `%APPDATA%\fyle\config.toml`
//! - macOS: `~/Library/Application Support/fyle/config.toml`
//!
//! Every key is optional; missing keys take the defaults below. Unknown keys
//! are rejected so typos do not go unnoticed.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::fs_ops::SortKey;
use crate::grid::ViewMode;
use crate::location::Location;
use crate::model::Sort;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub show_hidden: bool,
    /// "system", "light", "dark", or the name of a bundled or user theme.
    pub theme: String,
    /// UI font size in pixels; zooming (Ctrl-+ / Ctrl--) saves it.
    pub font_size: f32,
    /// Local and remote folders offered first in Go to (Ctrl-G); Ctrl-D adds
    /// one. Read from legacy path strings or versioned location values.
    #[serde(deserialize_with = "deserialize_bookmarks")]
    pub bookmarks: Vec<Location>,
    /// Programs opening files (Enter uses the first match; Shift-Enter lists all).
    #[serde(rename = "opener")]
    pub openers: Vec<crate::openers::Opener>,
    /// Extra key bindings, applied after (and so overriding) the defaults.
    pub keymap: Vec<KeymapEntry>,
    pub window: WindowSettings,
    pub panels: PanelSettings,
    pub preview: PreviewSettings,
    pub editor: EditorSettings,
    pub terminal: TerminalSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalSettings {
    /// Terminal for "Open terminal here", e.g. `["kitty", "-d", "{dir}"]`;
    /// it also starts in the directory. Empty: `$TERMINAL`, else the first
    /// known terminal installed.
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct EditorSettings {
    /// External editor command, e.g. `["code"]` or `["gnome-text-editor"]`;
    /// the file path is appended. Empty: the built-in editor.
    pub command: Vec<String>,
}

/// `[[keymap]] keys = "ctrl-shift-c", action = "Copy", context = "panel"`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeymapEntry {
    /// Keystrokes as gpui writes them, e.g. `"ctrl-k ctrl-c"`.
    pub keys: String,
    /// Action name (`"Copy"`, `"ToggleHidden"`, …) or `"none"` to unbind.
    pub action: String,
    #[serde(default)]
    pub context: KeyContext,
}

/// Where a binding applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyContext {
    /// While a panel has focus (most commands).
    #[default]
    Panel,
    /// Anywhere in the window.
    Global,
    /// In the F4 editor.
    Editor,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowSettings {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PanelSettings {
    /// Start directory of the left panel; default: current directory.
    pub left: Option<PathBuf>,
    /// Start directory of the right panel; default: home directory.
    pub right: Option<PathBuf>,
    pub tree: bool,
    /// How each panel starts: "list" or "icons" (no tree mode in icons).
    pub left_view: ViewMode,
    pub right_view: ViewMode,
    /// Icon size in pixels in icon view; snaps to 48, 64, 96, 128 or 160.
    pub icon_size: u32,
    /// Columns shown, in order; Name is always first.
    pub columns: Vec<crate::columns::ColumnKind>,
    pub sort: SortSetting,
    pub sort_descending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortSetting {
    #[default]
    Name,
    Ext,
    Size,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewSettings {
    /// Open the F3 quick preview at startup.
    pub open_on_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_hidden: false,
            theme: "system".into(),
            font_size: crate::zoom::DEFAULT,
            bookmarks: Vec::new(),
            keymap: Vec::new(),
            openers: Vec::new(),
            window: WindowSettings::default(),
            panels: PanelSettings::default(),
            preview: PreviewSettings::default(),
            editor: EditorSettings::default(),
            terminal: TerminalSettings::default(),
        }
    }
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            width: 1200.,
            height: 760.,
        }
    }
}

impl Default for PanelSettings {
    fn default() -> Self {
        Self {
            left: None,
            right: None,
            tree: false,
            left_view: ViewMode::List,
            right_view: ViewMode::List,
            icon_size: crate::grid::DEFAULT_SIZE,
            columns: crate::columns::ColumnKind::DEFAULT.to_vec(),
            sort: SortSetting::Name,
            sort_descending: false,
        }
    }
}

impl PanelSettings {
    /// The start view mode of panel `ix` (0 left, 1 right).
    pub fn view(&self, ix: usize) -> ViewMode {
        if ix == 0 {
            self.left_view
        } else {
            self.right_view
        }
    }

    pub fn sort(&self) -> Sort {
        Sort {
            key: match self.sort {
                SortSetting::Name => SortKey::Name,
                SortSetting::Ext => SortKey::Ext,
                SortSetting::Size => SortKey::Size,
                SortSetting::Modified => SortKey::Modified,
            },
            descending: self.sort_descending,
        }
    }
}

impl Settings {
    /// `<config dir>/fyle/config.toml`, if the platform has a config dir.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("fyle").join("config.toml"))
    }

    /// Loads the settings file. A missing file gives defaults; an unreadable
    /// or invalid one gives defaults plus an error message.
    pub fn load() -> (Self, Option<String>) {
        match Self::path() {
            Some(path) => Self::load_from(&path),
            None => (Self::default(), None),
        }
    }

    pub fn load_from(path: &Path) -> (Self, Option<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(e) => return (Self::default(), Some(format!("{}: {e}", path.display()))),
        };
        match Self::parse(&text) {
            Ok(settings) => (settings, None),
            Err(e) => (Self::default(), Some(format!("{}: {e}", path.display()))),
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut settings: Self = toml::from_str(text).map_err(|e| e.message().to_string())?;
        settings.panels.left = settings.panels.left.map(expand_home);
        settings.panels.right = settings.panels.right.map(expand_home);
        Ok(settings)
    }
}

/// Reads bookmark entries: versioned typed locations (local or remote) or
/// legacy local path strings (`~` expands to the home directory).
fn deserialize_bookmarks<'de, D>(deserializer: D) -> Result<Vec<Location>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|value| {
            if value.starts_with(crate::location_codec::PREFIX) {
                match crate::location_codec::decode(&value) {
                    Ok(location @ (Location::Local(_) | Location::Remote(_))) => Ok(location),
                    Ok(Location::Archive { .. }) => Err(D::Error::custom(
                        "bookmarks must name folders, not archive members",
                    )),
                    Err(error) => Err(D::Error::custom(format!("invalid bookmark: {error}"))),
                }
            } else {
                Location::local(expand_home(PathBuf::from(&value)))
                    .map_err(|error| D::Error::custom(format!("invalid bookmark {value}: {error}")))
            }
        })
        .collect()
}

/// Writes `theme = "<name>"` into the settings file, keeping the rest of the
/// file (comments, formatting, other keys) intact. Creates the file if needed.
pub fn save_theme(name: &str) -> Result<PathBuf, String> {
    let path = Settings::path().ok_or("No config directory on this platform")?;
    save_theme_to(&path, name)?;
    Ok(path)
}

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

/// Writes `font_size = <size>` into the settings file, keeping the rest of
/// the file intact. Creates the file if needed.
pub fn save_font_size(size: f32) -> Result<PathBuf, String> {
    let path = Settings::path().ok_or("No config directory on this platform")?;
    save_font_size_to(&path, size)?;
    Ok(path)
}

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

/// The commented example settings, shipped with the program.
pub const EXAMPLE: &str = include_str!("../config.example.toml");

/// The settings file, created from [`EXAMPLE`] if it does not exist yet
/// (so Help › Configuration always has something to open).
pub fn ensure_file() -> Result<PathBuf, String> {
    let path = Settings::path().ok_or("No config directory on this platform")?;
    ensure_file_at(&path)?;
    Ok(path)
}

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
    let path = Settings::path().ok_or("No config directory on this platform")?;
    save_columns_to(&path, columns)?;
    Ok(path)
}

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
    let path = Settings::path().ok_or("No config directory on this platform")?;
    save_panel_view_to(&path, ix, mode)?;
    Ok(path)
}

pub fn save_panel_view_to(path: &Path, ix: usize, mode: ViewMode) -> Result<(), String> {
    let key = if ix == 0 { "left_view" } else { "right_view" };
    save_panel_value_to(path, key, mode.id().into())
}

/// Writes `[panels] icon_size` into the settings file, keeping the rest of
/// the file intact.
pub fn save_icon_size(size: u32) -> Result<PathBuf, String> {
    let path = Settings::path().ok_or("No config directory on this platform")?;
    save_icon_size_to(&path, size)?;
    Ok(path)
}

pub fn save_icon_size_to(path: &Path, size: u32) -> Result<(), String> {
    save_panel_value_to(path, "icon_size", i64::from(size).into())
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

/// Expands a leading `~` to the home directory.
fn expand_home(path: PathBuf) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path;
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn parses_all_keys() {
        let settings = Settings::parse(
            r#"
            show_hidden = true
            theme = "dark"

            [window]
            width = 800
            height = 600

            [panels]
            left = "/tmp"
            right = "~/projects"
            tree = true
            left_view = "icons"
            icon_size = 128
            sort = "size"
            sort_descending = true

            [preview]
            open_on_start = true

            [terminal]
            command = ["kitty", "-d", "{dir}"]
            "#,
        )
        .unwrap();
        assert!(settings.show_hidden);
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.terminal.command, ["kitty", "-d", "{dir}"]);
        assert_eq!(settings.window.width, 800.);
        assert_eq!(settings.panels.left, Some(PathBuf::from("/tmp")));
        let right = settings.panels.right.as_ref().unwrap();
        assert!(right.ends_with("projects") && !right.starts_with("~"));
        assert!(settings.panels.tree);
        assert_eq!(settings.panels.view(0), ViewMode::Icons);
        assert_eq!(settings.panels.view(1), ViewMode::List);
        assert_eq!(settings.panels.icon_size, 128);
        assert_eq!(
            settings.panels.sort(),
            Sort {
                key: SortKey::Size,
                descending: true
            }
        );
        assert!(settings.preview.open_on_start);
    }

    #[test]
    fn partial_sections_keep_other_defaults() {
        let settings = Settings::parse("[window]\nwidth = 900").unwrap();
        assert_eq!(settings.window.width, 900.);
        assert_eq!(settings.window.height, 760.);
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(Settings::parse("show_hiden = true").is_err());
        assert!(Settings::parse("theme = 1").is_err());
        assert!(Settings::parse("[panels]\nsort = \"color\"").is_err());
    }

    #[test]
    fn example_file_matches_defaults() {
        let example = include_str!("../config.example.toml");
        assert_eq!(Settings::parse(example).unwrap(), Settings::default());
    }

    #[test]
    fn save_theme_keeps_comments_and_other_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("fyle").join("config.toml");

        save_theme_to(&path, "Ayu Dark").unwrap(); // creates file and dirs
        assert_eq!(Settings::load_from(&path).0.theme, "Ayu Dark");

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
        assert_eq!(Settings::load_from(&path).0.font_size, 17.5);

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
        assert_eq!(Settings::load_from(&path).0.panels.columns, columns);

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
        let panels = Settings::load_from(&path).0.panels;
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
        let loc = |p: &str| Location::local(p.into()).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &loc("/srv/a")).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &loc("/srv/b")).unwrap();
        crate::bookmarks::add_bookmark_to(&path, &loc("/srv/a")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine"));
        let settings = Settings::parse(&text).unwrap();
        assert_eq!(settings.bookmarks, [loc("/srv/a"), loc("/srv/b")]);
        assert_eq!(settings.theme, "dark");
    }

    #[test]
    fn encoded_bookmarks_preserve_native_bytes_and_legacy_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "# keep this comment\nbookmarks = [\"/legacy\"]\n").unwrap();
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
        assert_eq!(
            Settings::parse(&text).unwrap().bookmarks,
            [Location::local("/legacy".into()).unwrap(), unusual]
        );
    }

    #[test]
    fn bookmark_codec_reads_remotes_and_rejects_malformed_entries() {
        let remote = Location::Remote(crate::remote::Location::root("nas").unwrap());
        let value = toml_edit::value(crate::location_codec::encode(&remote).unwrap());
        let settings = Settings::parse(&format!("bookmarks = [{value}]")).unwrap();
        assert_eq!(settings.bookmarks, std::slice::from_ref(&remote));
        let member = Location::archive(Location::local("/a.zip".into()).unwrap(), "x").unwrap();
        let value = toml_edit::value(crate::location_codec::encode(&member).unwrap());
        assert!(Settings::parse(&format!("bookmarks = [{value}]")).is_err());
        assert!(Settings::parse("bookmarks = [\"fyle-location:v1:{\"]").is_err());
    }

    #[test]
    fn parses_keymap() {
        let settings = Settings::parse(
            "[[keymap]]\nkeys = \"ctrl-c\"\naction = \"Copy\"\n\
             [[keymap]]\nkeys = \"f9\"\naction = \"none\"\ncontext = \"global\"\n",
        )
        .unwrap();
        assert_eq!(settings.keymap.len(), 2);
        assert_eq!(settings.keymap[0].context, KeyContext::Panel);
        assert_eq!(settings.keymap[1].context, KeyContext::Global);
        assert!(
            Settings::parse("[[keymap]]\nkeys = \"x\"\naction = \"y\"\ncontext = \"z\"").is_err()
        );
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let (settings, error) = Settings::load_from(Path::new("/nonexistent/fyle/config.toml"));
        assert_eq!(settings, Settings::default());
        assert_eq!(error, None);
    }
}

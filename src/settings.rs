//! User settings schema, defaults and parsing; file I/O lives in [`persistence`].
//!
//! Every key is optional; missing keys take the defaults below. Unknown keys
//! are rejected so typos do not go unnoticed.

/// Config file loading, creation and comment-preserving saves.
pub mod persistence;

use std::path::PathBuf;

use serde::Deserialize;

use crate::entry::Sort;
use crate::entry::SortKey;
use crate::grid::ViewMode;
use crate::location::Location;
use crate::workspace::LayoutMode;

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
    pub workspace: WorkspaceSettings,
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

/// Which workspace layout opens at startup.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceSettings {
    /// The visible folder panels and optional inspector.
    pub layout: LayoutMode,
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
            workspace: WorkspaceSettings::default(),
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
    /// Parses and validates settings, applying defaults and expanding home paths.
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

/// The commented example settings, shipped with the program.
pub const EXAMPLE: &str = include_str!("../config.example.toml");

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

    #[cfg(unix)]
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

            [workspace]
            layout = "one_panel_inspector"

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
        assert_eq!(settings.workspace.layout, LayoutMode::OnePanelInspector);
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
        assert!(Settings::parse("[workspace]\nlayout = \"three_panels\"").is_err());
        assert!(Settings::parse("[preview]\nopen_on_start = true").is_err());
    }

    #[test]
    fn example_file_matches_defaults() {
        let example = include_str!("../config.example.toml");
        assert_eq!(Settings::parse(example).unwrap(), Settings::default());
    }

    #[test]
    fn bookmark_codec_reads_remotes_and_rejects_malformed_entries() {
        let remote = Location::Remote(crate::remote::Location::root("nas").unwrap());
        let value = toml_edit::value(crate::location_codec::encode(&remote).unwrap());
        let settings = Settings::parse(&format!("bookmarks = [{value}]")).unwrap();
        assert_eq!(settings.bookmarks, std::slice::from_ref(&remote));
        let tmp = tempfile::tempdir().unwrap();
        let member =
            Location::archive(Location::local(tmp.path().join("a.zip")).unwrap(), "x").unwrap();
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
}

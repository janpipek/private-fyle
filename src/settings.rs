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
use crate::model::Sort;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub show_hidden: bool,
    pub theme: ThemeSetting,
    pub window: WindowSettings,
    pub panels: PanelSettings,
    pub preview: PreviewSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSetting {
    #[default]
    System,
    Light,
    Dark,
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
            theme: ThemeSetting::System,
            window: WindowSettings::default(),
            panels: PanelSettings::default(),
            preview: PreviewSettings::default(),
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
            sort: SortSetting::Name,
            sort_descending: false,
        }
    }
}

impl PanelSettings {
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
            sort = "size"
            sort_descending = true

            [preview]
            open_on_start = true
            "#,
        )
        .unwrap();
        assert!(settings.show_hidden);
        assert_eq!(settings.theme, ThemeSetting::Dark);
        assert_eq!(settings.window.width, 800.);
        assert_eq!(settings.panels.left, Some(PathBuf::from("/tmp")));
        let right = settings.panels.right.as_ref().unwrap();
        assert!(right.ends_with("projects") && !right.starts_with("~"));
        assert!(settings.panels.tree);
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
        assert!(Settings::parse("theme = \"blue\"").is_err());
        assert!(Settings::parse("[panels]\nsort = \"color\"").is_err());
    }

    #[test]
    fn example_file_matches_defaults() {
        let example = include_str!("../config.example.toml");
        assert_eq!(Settings::parse(example).unwrap(), Settings::default());
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let (settings, error) = Settings::load_from(Path::new("/nonexistent/fyle/config.toml"));
        assert_eq!(settings, Settings::default());
        assert_eq!(error, None);
    }
}

//! Colour themes in gpui-component's format.
//!
//! Bundled themes come from `themes/` (see `themes/README.md` for their
//! origin and licences). Users can add more by placing `*.json` files in
//! `<config dir>/fyle/themes/`. `config.toml`'s `theme` key selects one by
//! name, or follows the system / forces the built-in light or dark theme.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::*;

/// `(file name, contents)` of the bundled theme files.
pub const BUNDLED: &[(&str, &str)] = &[
    ("adventure.json", include_str!("../themes/adventure.json")),
    ("alduin.json", include_str!("../themes/alduin.json")),
    ("asciinema.json", include_str!("../themes/asciinema.json")),
    ("aurora.json", include_str!("../themes/aurora.json")),
    ("ayu.json", include_str!("../themes/ayu.json")),
    ("catppuccin.json", include_str!("../themes/catppuccin.json")),
    ("everforest.json", include_str!("../themes/everforest.json")),
    ("fahrenheit.json", include_str!("../themes/fahrenheit.json")),
    ("flexoki.json", include_str!("../themes/flexoki.json")),
    ("gruvbox.json", include_str!("../themes/gruvbox.json")),
    ("harper.json", include_str!("../themes/harper.json")),
    ("hybrid.json", include_str!("../themes/hybrid.json")),
    ("jellybeans.json", include_str!("../themes/jellybeans.json")),
    ("kibble.json", include_str!("../themes/kibble.json")),
    (
        "macos-classic.json",
        include_str!("../themes/macos-classic.json"),
    ),
    (
        "mellifluous.json",
        include_str!("../themes/mellifluous.json"),
    ),
    ("molokai.json", include_str!("../themes/molokai.json")),
    ("solarized.json", include_str!("../themes/solarized.json")),
    ("spaceduck.json", include_str!("../themes/spaceduck.json")),
    ("tokyonight.json", include_str!("../themes/tokyonight.json")),
    ("twilight.json", include_str!("../themes/twilight.json")),
];

/// What the `theme` setting asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeChoice {
    /// Built-in light or dark theme, following the OS appearance.
    System,
    Light,
    Dark,
    /// A registered theme, matched by name case-insensitively.
    Named(String),
}

impl ThemeChoice {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "" | "system" => Self::System,
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::Named(value.trim().to_string()),
        }
    }
}

/// `<config dir>/fyle/themes`.
pub fn user_themes_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("fyle").join("themes"))
}

/// Registers bundled and user themes. User themes are registered first so a
/// user file can replace a bundled theme of the same name. Returns errors for
/// files that could not be loaded.
pub fn register_all(cx: &mut App) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(dir) = user_themes_dir() {
        errors.extend(register_dir(&dir, cx));
    }
    let registry = ThemeRegistry::global_mut(cx);
    for (name, json) in BUNDLED {
        if let Err(e) = registry.load_themes_from_str(json) {
            errors.push(format!("bundled theme {name}: {e}"));
        }
    }
    errors
}

fn register_dir(dir: &Path, cx: &mut App) -> Vec<String> {
    let Ok(items) = std::fs::read_dir(dir) else {
        return Vec::new(); // No user themes directory.
    };
    let mut paths: Vec<_> = items
        .filter_map(|i| i.ok().map(|i| i.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    let registry = ThemeRegistry::global_mut(cx);
    let mut errors = Vec::new();
    for path in paths {
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|json| {
                registry
                    .load_themes_from_str(&json)
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = result {
            errors.push(format!("{}: {e}", path.display()));
        }
    }
    errors
}

/// Applies the chosen theme to the app.
pub fn apply(choice: &ThemeChoice, window: &mut Window, cx: &mut App) -> Result<(), String> {
    match choice {
        ThemeChoice::System => Theme::sync_system_appearance(Some(window), cx),
        ThemeChoice::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        ThemeChoice::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        ThemeChoice::Named(name) => {
            let config = find(name, cx).ok_or_else(|| format!("Unknown theme \"{name}\""))?;
            Theme::update(cx, |theme| theme.apply_config(&config));
        }
    }
    Ok(())
}

fn find(name: &str, cx: &App) -> Option<Rc<ThemeConfig>> {
    ThemeRegistry::global(cx)
        .themes()
        .values()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .cloned()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob export would shadow `#[test]`.
    use super::{BUNDLED, ThemeChoice};
    use gpui_kit::component::ThemeSet;
    use std::collections::HashSet;

    fn theme_names(json: &str) -> Result<Vec<String>, String> {
        let set: ThemeSet = serde_json::from_str(json).map_err(|e| e.to_string())?;
        Ok(set.themes.iter().map(|t| t.name.to_string()).collect())
    }

    #[test]
    fn bundled_themes_parse_with_unique_names() {
        let mut names = HashSet::new();
        for (file, json) in BUNDLED {
            let themes = theme_names(json).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(!themes.is_empty(), "{file}");
            for name in themes {
                assert!(names.insert(name.to_lowercase()), "duplicate {name}");
            }
        }
        assert!(names.contains("tokyo night"));
        assert_eq!(BUNDLED.len(), 21);
    }

    #[test]
    fn parses_choice() {
        assert_eq!(ThemeChoice::parse("System"), ThemeChoice::System);
        assert_eq!(ThemeChoice::parse("dark"), ThemeChoice::Dark);
        assert_eq!(
            ThemeChoice::parse(" Tokyo Night "),
            ThemeChoice::Named("Tokyo Night".into())
        );
    }
}

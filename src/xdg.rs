//! The XDG base directories and desktop environment names that decide where
//! MIME data, desktop entries and `mimeapps.list` files live. Built from
//! explicit values so tests never read the real environment.

use std::path::{Path, PathBuf};

/// Directories and names from the environment, resolved with the defaults
/// of the XDG base directory spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub config_home: Option<PathBuf>,
    pub config_dirs: Vec<PathBuf>,
    pub data_home: Option<PathBuf>,
    pub data_dirs: Vec<PathBuf>,
    /// `XDG_CURRENT_DESKTOP`, lowercased, one entry per colon-separated part.
    pub desktops: Vec<String>,
    /// Locale keys for localized names, most specific first
    /// (`de_DE`, `de`).
    pub locales: Vec<String>,
}

impl Env {
    /// Reads the real environment.
    pub fn from_env() -> Self {
        Self::from_vars(|k| std::env::var(k).ok(), dirs::home_dir())
    }

    /// Resolves from `var` (an environment lookup) and the home directory.
    /// Empty and relative paths are ignored, as the spec demands.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>, home: Option<PathBuf>) -> Self {
        let absolute = |key: &str| -> Option<PathBuf> {
            var(key).map(PathBuf::from).filter(|p| p.is_absolute())
        };
        let list = |key: &str, default: &[&str]| -> Vec<PathBuf> {
            let parsed: Vec<PathBuf> = var(key)
                .map(|v| {
                    v.split(':')
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute())
                        .collect()
                })
                .unwrap_or_default();
            if parsed.is_empty() {
                default.iter().map(PathBuf::from).collect()
            } else {
                parsed
            }
        };
        let desktops = var("XDG_CURRENT_DESKTOP")
            .map(|v| {
                v.split(':')
                    .filter(|d| !d.is_empty())
                    .map(str::to_lowercase)
                    .collect()
            })
            .unwrap_or_default();
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| var(k).filter(|v| !v.is_empty()))
            .unwrap_or_default();
        Self {
            config_home: absolute("XDG_CONFIG_HOME")
                .or_else(|| home.as_ref().map(|h| h.join(".config"))),
            config_dirs: list("XDG_CONFIG_DIRS", &["/etc/xdg"]),
            data_home: absolute("XDG_DATA_HOME").or_else(|| home.map(|h| h.join(".local/share"))),
            data_dirs: list("XDG_DATA_DIRS", &["/usr/local/share", "/usr/share"]),
            desktops,
            locales: locale_keys(&locale),
        }
    }

    /// Data directories in precedence order: the user's, then the system's.
    pub fn all_data_dirs(&self) -> Vec<PathBuf> {
        self.data_home
            .iter()
            .chain(&self.data_dirs)
            .cloned()
            .collect()
    }

    /// Every place a `mimeapps.list` may be, most important first: per
    /// directory the desktop-specific files, then the generic one; config
    /// home, config dirs, then `applications/` in the data directories.
    pub fn mimeapps_paths(&self) -> Vec<PathBuf> {
        let in_dir = |dir: &Path| -> Vec<PathBuf> {
            self.desktops
                .iter()
                .map(|d| dir.join(format!("{d}-mimeapps.list")))
                .chain(std::iter::once(dir.join("mimeapps.list")))
                .collect()
        };
        self.config_home
            .iter()
            .chain(&self.config_dirs)
            .cloned()
            .chain(
                self.all_data_dirs()
                    .into_iter()
                    .map(|d| d.join("applications")),
            )
            .flat_map(|dir| in_dir(&dir))
            .collect()
    }
}

/// `de_DE.UTF-8@euro` becomes `["de_DE", "de"]`; `C` and `POSIX` give none.
fn locale_keys(locale: &str) -> Vec<String> {
    let base = locale.split(['.', '@']).next().unwrap_or_default();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return Vec::new();
    }
    let mut keys = vec![base.to_string()];
    if let Some((lang, _)) = base.split_once('_') {
        keys.push(lang.to_string());
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> Env {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Env::from_vars(|k| map.get(k).cloned(), Some(PathBuf::from("/home/u")))
    }

    #[test]
    fn defaults_follow_the_spec() {
        let e = env(&[]);
        assert_eq!(e.config_home, Some(PathBuf::from("/home/u/.config")));
        assert_eq!(e.data_home, Some(PathBuf::from("/home/u/.local/share")));
        assert_eq!(e.config_dirs, [PathBuf::from("/etc/xdg")]);
        assert_eq!(e.data_dirs.len(), 2);
        assert!(e.desktops.is_empty() && e.locales.is_empty());
    }

    #[test]
    fn ignores_relative_and_empty_values() {
        let e = env(&[
            ("XDG_CONFIG_HOME", "rel"),
            ("XDG_DATA_DIRS", "x:/a:/b"),
            ("XDG_CONFIG_DIRS", ""),
        ]);
        assert_eq!(e.config_home, Some(PathBuf::from("/home/u/.config")));
        assert_eq!(e.data_dirs, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(e.config_dirs, [PathBuf::from("/etc/xdg")]);
    }

    #[test]
    fn mimeapps_order_and_desktops() {
        let e = env(&[
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_CONFIG_DIRS", "/etc/x"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_DATA_DIRS", "/usr/share"),
            ("XDG_CURRENT_DESKTOP", "KDE:Plasma"),
            ("LANG", "de_DE.UTF-8"),
        ]);
        assert_eq!(e.locales, ["de_DE", "de"]);
        let paths: Vec<String> = e
            .mimeapps_paths()
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert_eq!(
            paths,
            [
                "/c/kde-mimeapps.list",
                "/c/plasma-mimeapps.list",
                "/c/mimeapps.list",
                "/etc/x/kde-mimeapps.list",
                "/etc/x/plasma-mimeapps.list",
                "/etc/x/mimeapps.list",
                "/d/applications/kde-mimeapps.list",
                "/d/applications/plasma-mimeapps.list",
                "/d/applications/mimeapps.list",
                "/usr/share/applications/kde-mimeapps.list",
                "/usr/share/applications/plasma-mimeapps.list",
                "/usr/share/applications/mimeapps.list",
            ]
        );
    }
}

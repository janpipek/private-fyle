//! Which program opens a file: user rules from `[[opener]]` in config.toml,
//! matched by extension, file kind or name; UI-independent.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::icons::{self, FileIcon};

/// ```toml
/// [[opener]]
/// label = "GIMP"
/// command = ["gimp"]            # the path is appended, or put "{path}"
/// kinds = ["image"]             # and/or exts = ["xcf"], names = ["Makefile"]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opener {
    pub label: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub exts: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<FileIcon>,
    #[serde(default)]
    pub names: Vec<String>,
}

impl Opener {
    /// Matches if the file has one of the extensions, kinds or names; a rule
    /// without any of them matches every file.
    pub fn matches(&self, path: &Path) -> bool {
        self.matches_entry(path, false)
    }

    /// Whether this rule matches a file or directory at `path`.
    pub fn matches_entry(&self, path: &Path, is_dir: bool) -> bool {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let unrestricted = self.exts.is_empty() && self.kinds.is_empty() && self.names.is_empty();
        unrestricted
            || self.exts.iter().any(|e| e.eq_ignore_ascii_case(&ext))
            || self
                .kinds
                .contains(&icons::icon_for(&name, is_dir, false, false))
            || self.names.contains(&name)
    }

    /// Program and arguments to open `path`: `{path}` is substituted, else
    /// the path is appended.
    pub fn command_for(&self, path: &Path) -> Option<(PathBuf, Vec<String>)> {
        let (program, args) = self.command.split_first()?;
        let path = path.display().to_string();
        let mut substituted = false;
        let mut args: Vec<String> = args
            .iter()
            .map(|a| {
                if a.contains("{path}") {
                    substituted = true;
                    a.replace("{path}", &path)
                } else {
                    a.clone()
                }
            })
            .collect();
        if !substituted {
            args.push(path);
        }
        Some((PathBuf::from(program), args))
    }
}

/// Rules matching `path`, in configuration order.
pub fn matching<'a>(openers: &'a [Opener], path: &Path) -> Vec<&'a Opener> {
    openers.iter().filter(|o| o.matches(path)).collect()
}

/// Rules matching a file or directory, in configuration order.
pub fn matching_entry<'a>(openers: &'a [Opener], path: &Path, is_dir: bool) -> Vec<&'a Opener> {
    openers
        .iter()
        .filter(|o| o.matches_entry(path, is_dir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opener(label: &str, command: &[&str], exts: &[&str], kinds: &[FileIcon]) -> Opener {
        Opener {
            label: label.into(),
            command: command.iter().map(|s| s.to_string()).collect(),
            exts: exts.iter().map(|s| s.to_string()).collect(),
            kinds: kinds.to_vec(),
            names: Vec::new(),
        }
    }

    #[test]
    fn matches_by_ext_kind_or_anything() {
        let rules = [
            opener("Zathura", &["zathura"], &["pdf"], &[]),
            opener("GIMP", &["gimp"], &[], &[FileIcon::Image]),
            opener("Any", &["xdg-open"], &[], &[]),
        ];
        let labels = |p: &str| {
            matching(&rules, Path::new(p))
                .iter()
                .map(|o| o.label.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels("/d/Doc.PDF"), ["Zathura", "Any"]);
        assert_eq!(labels("/d/photo.jpg"), ["GIMP", "Any"]);
        assert_eq!(labels("/d/notes.txt"), ["Any"]);
    }

    #[test]
    fn builds_commands() {
        let append = opener("x", &["gimp", "--new"], &[], &[]);
        assert_eq!(
            append.command_for(Path::new("/a b.png")),
            Some((
                PathBuf::from("gimp"),
                vec!["--new".into(), "/a b.png".into()]
            ))
        );
        let substitute = opener("x", &["code", "--goto", "{path}:1"], &[], &[]);
        assert_eq!(
            substitute.command_for(Path::new("/f.rs")).unwrap().1,
            ["--goto", "/f.rs:1"]
        );
        assert_eq!(
            opener("x", &[], &[], &[]).command_for(Path::new("/f")),
            None
        );
    }

    #[test]
    fn directory_kind_matches_directories_only() {
        let rules = [opener("Folder", &["fm"], &[], &[FileIcon::Folder])];
        assert!(matching_entry(&rules, Path::new("/x/notes"), false).is_empty());
        assert_eq!(
            matching_entry(&rules, Path::new("/x/notes"), true)[0].label,
            "Folder"
        );
    }
}

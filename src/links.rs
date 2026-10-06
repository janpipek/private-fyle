//! Symbolic link state, UI-independent: what a link points at and whether
//! it works. Views draw a badge from it and show [`LinkState::tooltip`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `ELOOP`: too many levels of symbolic links (`ErrorKind::FilesystemLoop`
/// is not stable yet).
#[cfg(any(target_os = "linux", target_os = "android"))]
const ELOOP: i32 = 40;
/// macOS and the BSDs.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const ELOOP: i32 = 62;

/// Why a link does not lead anywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Broken {
    /// The target does not exist.
    Dangling,
    /// Links lead in a circle (or are nested too deeply).
    Loop,
    /// The target exists but cannot be examined (e.g. permissions).
    Unreadable,
}

/// Whether an entry is a symbolic link, and where it leads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LinkState {
    #[default]
    NotLink,
    /// A working link; whether it leads to a folder is the entry's `is_dir`.
    Link {
        /// As stored in the link, possibly relative (empty when unknown,
        /// e.g. a zip entry).
        target: PathBuf,
    },
    Broken {
        target: PathBuf,
        reason: Broken,
    },
}

impl LinkState {
    /// State of the symlink at `path`, given the result of following it
    /// (`fs::metadata`), which listings fetch anyway. Costs one `read_link`;
    /// the resolved path is only computed on demand ([`Self::resolved`]).
    /// `path` must be a link.
    pub fn probe(path: &Path, followed: &io::Result<fs::Metadata>) -> Self {
        let Ok(target) = fs::read_link(path) else {
            return Self::Broken {
                target: PathBuf::new(),
                reason: Broken::Unreadable,
            };
        };
        match followed {
            Ok(_) => Self::Link { target },
            Err(e) => Self::Broken {
                reason: match e.kind() {
                    io::ErrorKind::NotFound => Broken::Dangling,
                    _ if e.raw_os_error() == Some(ELOOP) => Broken::Loop,
                    _ => Broken::Unreadable,
                },
                target,
            },
        }
    }

    /// State of whatever is at `path` (touches the disk).
    pub fn of(path: &Path) -> Self {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => Self::probe(path, &fs::metadata(path)),
            _ => Self::NotLink,
        }
    }

    pub fn is_broken(&self) -> bool {
        matches!(self, Self::Broken { .. })
    }

    /// Absolute path after following every link, for a working link at
    /// `path` (touches the disk; call it for a tooltip, not per listed row).
    pub fn resolved(&self, path: &Path) -> Option<PathBuf> {
        match self {
            Self::Link { .. } => fs::canonicalize(path).ok(),
            _ => None,
        }
    }

    /// Tooltip text for the entry at `path`: `→ target`, plus the resolved
    /// path when it differs; `Broken link → target` with the reason for
    /// broken ones.
    pub fn tooltip(&self, path: &Path) -> Option<String> {
        match self {
            Self::NotLink => None,
            Self::Link { target } if target.as_os_str().is_empty() => Some("Symbolic link".into()),
            Self::Link { target } => {
                let mut text = format!("→ {}", target.display());
                if let Some(resolved) = self.resolved(path)
                    && resolved != *target
                {
                    text.push_str(&format!("\n{}", resolved.display()));
                }
                Some(text)
            }
            Self::Broken { target, reason } => Some(format!(
                "Broken link{} → {}",
                match reason {
                    Broken::Dangling => " (target missing)",
                    Broken::Loop => " (loop)",
                    Broken::Unreadable => " (target unreadable)",
                },
                target.display()
            )),
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::{Broken, LinkState};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/file.rs"), "x").unwrap();
        dir
    }

    fn real(dir: &TempDir, rel: &str) -> PathBuf {
        fs::canonicalize(dir.path().join(rel)).unwrap()
    }

    #[test]
    fn regular_entries_are_not_links() {
        let dir = dir();
        assert_eq!(LinkState::of(&dir.path().join("real")), LinkState::NotLink);
        assert_eq!(
            LinkState::of(&dir.path().join("real/file.rs")),
            LinkState::NotLink
        );
        assert_eq!(
            LinkState::of(&dir.path().join("missing")),
            LinkState::NotLink
        );
        assert_eq!(LinkState::NotLink.tooltip(dir.path()), None);
    }

    #[test]
    fn relative_link_to_file() {
        let dir = dir();
        symlink("real/file.rs", dir.path().join("l")).unwrap();
        let state = LinkState::of(&dir.path().join("l"));
        assert_eq!(
            state,
            LinkState::Link {
                target: "real/file.rs".into(),
            }
        );
        let path = dir.path().join("l");
        assert_eq!(state.resolved(&path), Some(real(&dir, "real/file.rs")));
        let tip = state.tooltip(&path).unwrap();
        assert!(tip.starts_with("→ real/file.rs\n"), "{tip}");
        assert!(tip.ends_with("file.rs"));
    }

    #[test]
    fn absolute_link_to_dir_shows_target_once() {
        let dir = dir();
        let target = real(&dir, "real");
        symlink(&target, dir.path().join("l")).unwrap();
        let state = LinkState::of(&dir.path().join("l"));
        assert_eq!(
            state.tooltip(&dir.path().join("l")),
            Some(format!("→ {}", target.display()))
        );
    }

    #[test]
    fn link_to_link_resolves_fully() {
        let dir = dir();
        symlink("real", dir.path().join("a")).unwrap();
        symlink("a", dir.path().join("b")).unwrap();
        let state = LinkState::of(&dir.path().join("b"));
        let LinkState::Link { target } = &state else {
            panic!("expected a working link");
        };
        assert_eq!(target, Path::new("a"));
        assert_eq!(
            state.resolved(&dir.path().join("b")),
            Some(real(&dir, "real"))
        );
    }

    #[test]
    fn dangling_link() {
        let dir = dir();
        symlink("../nowhere", dir.path().join("l")).unwrap();
        let state = LinkState::of(&dir.path().join("l"));
        assert_eq!(
            state,
            LinkState::Broken {
                target: "../nowhere".into(),
                reason: Broken::Dangling
            }
        );
        assert!(state.is_broken());
        assert_eq!(
            state.tooltip(&dir.path().join("l")).unwrap(),
            "Broken link (target missing) → ../nowhere"
        );
    }

    #[test]
    fn loops_are_broken() {
        let dir = dir();
        symlink("y", dir.path().join("x")).unwrap();
        symlink("x", dir.path().join("y")).unwrap();
        symlink("self", dir.path().join("self")).unwrap();
        for name in ["x", "y", "self"] {
            let state = LinkState::of(&dir.path().join(name));
            assert!(
                matches!(
                    state,
                    LinkState::Broken {
                        reason: Broken::Loop,
                        ..
                    }
                ),
                "{name}: {state:?}"
            );
        }
    }

    #[test]
    fn listing_marks_links_and_keeps_target_kind() {
        let dir = dir();
        symlink("real", dir.path().join("to_dir")).unwrap();
        symlink("real/file.rs", dir.path().join("to_file")).unwrap();
        symlink("gone", dir.path().join("dangling")).unwrap();
        let entries = crate::fs_ops::read_dir(dir.path(), false).unwrap();
        let find = |name: &str| entries.iter().find(|e| e.name == name).unwrap();
        // A link to a folder is a folder (so Enter enters it) and keeps the
        // link path.
        assert!(find("to_dir").is_dir);
        assert_eq!(find("to_dir").path, dir.path().join("to_dir"));
        assert!(matches!(find("to_dir").link, LinkState::Link { .. }));
        assert!(!find("to_file").is_dir);
        assert!(matches!(find("to_file").link, LinkState::Link { .. }));
        assert!(find("dangling").link.is_broken());
        assert!(!find("dangling").is_dir);
        assert_eq!(find("real").link, LinkState::NotLink);
    }
}

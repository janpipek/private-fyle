//! Pure archive member paths and address helpers, independent of archive I/O.

use std::path::{Component, Path};

/// A normalized path within an archive, with an empty path for its root.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArchiveMember(String);

impl ArchiveMember {
    /// Validates a slash-separated archive member path without normalization.
    pub fn new(path: &str) -> std::io::Result<Self> {
        let valid = match clean(path) {
            Cleaned::Root => path.is_empty(),
            Cleaned::Path(cleaned) => cleaned == path,
            Cleaned::Unsafe => false,
        };
        if valid {
            Ok(Self(path.to_owned()))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid archive member path",
            ))
        }
    }
    /// Returns the normalized path, empty at the archive root.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Result of cleaning an entry name.
#[derive(Debug, PartialEq, Eq)]
pub enum Cleaned {
    /// The archive root itself (`./`, `/` is *not* this: see `Unsafe`).
    Root,
    /// A relative path of normal components joined by `/`.
    Path(String),
    /// Absolute, or containing `..`, a drive prefix or NUL: never used.
    Unsafe,
}

/// Cleans an entry name: drops `.` and empty components and treats `\` as
/// a separator; rejects absolute names and `..`.
pub fn clean(raw: &str) -> Cleaned {
    if raw.contains('\0') || raw.starts_with('/') || raw.starts_with('\\') {
        return Cleaned::Unsafe;
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Cleaned::Unsafe,
            p if p.len() >= 2
                && p.as_bytes()[1] == b':'
                && p.as_bytes()[0].is_ascii_alphabetic() =>
            {
                return Cleaned::Unsafe;
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        Cleaned::Root
    } else {
        Cleaned::Path(parts.join("/"))
    }
}

/// `path` relative to `file` as an inner path (`a/b`); None if it is not
/// below it or has odd components.
pub fn inner_of(file: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(file).ok()?;
    let mut parts = Vec::new();
    for component in rest.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::{ArchiveMember, Cleaned, clean, inner_of};
    use std::path::Path;

    #[test]
    fn validates_members_without_normalizing_them() {
        assert!(ArchiveMember::new("").is_ok());
        assert!(ArchiveMember::new("a/b").is_ok());
        for bad in [
            "../x", "/x", "a//b", "a/./b", "a\\b", "a/../b", "C:/x", "a\0b",
        ] {
            assert!(ArchiveMember::new(bad).is_err(), "{bad:?}");
        }
        assert_eq!(clean("./"), Cleaned::Root);
        assert_eq!(clean("a/b"), Cleaned::Path("a/b".into()));
        assert_eq!(clean("./a//b/"), Cleaned::Path("a/b".into()));
        assert_eq!(clean("a\\b"), Cleaned::Path("a/b".into()));
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a/..",
            "\\x",
            "C:/x",
            "a\0b",
        ] {
            assert_eq!(clean(bad), Cleaned::Unsafe, "{bad:?}");
        }
    }

    #[test]
    fn inner_paths_reject_parent_and_prefix_mismatches() {
        assert_eq!(
            inner_of(Path::new("/tmp/a.zip"), Path::new("/tmp/a.zip/folder/item")),
            Some("folder/item".into())
        );
        assert_eq!(
            inner_of(Path::new("/tmp/a.zip"), Path::new("/tmp/a.zip")),
            Some(String::new())
        );
        assert_eq!(
            inner_of(Path::new("/tmp/a.zip"), Path::new("/tmp/other")),
            None
        );
        assert_eq!(
            inner_of(
                Path::new("/tmp/a.zip"),
                &Path::new("/tmp/a.zip").join("a/../escape")
            ),
            None
        );
    }
}

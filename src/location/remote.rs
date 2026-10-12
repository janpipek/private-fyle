//! Pure remote address parsing and identity, independent of providers.

use std::io;
use std::path::{Path, PathBuf};

/// A folder or item on a remote, kept distinct from local paths. Shown in
/// panels as `<remote>:/a/b`; the root is `<remote>:/`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteLocation {
    remote: String,
    parts: Vec<String>,
}

impl RemoteLocation {
    /// The root of `remote`.
    pub fn root(remote: &str) -> io::Result<Self> {
        if !validate_remote_name(remote) {
            return Err(invalid("invalid remote name"));
        }
        Ok(Self {
            remote: remote.into(),
            parts: Vec::new(),
        })
    }

    /// Parses a panel path. `Ok(None)` means a local path; an error means a
    /// remote path that is malformed or escapes the remote's root.
    pub fn parse(path: &Path) -> io::Result<Option<Self>> {
        let Some((remote, rest)) = split_remote(path) else {
            return Ok(None);
        };
        if !rest.is_empty() && !rest.starts_with('/') {
            return Err(invalid(&format!("use {remote}:/folder")));
        }
        let mut location = Self::root(remote)?;
        for part in rest.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    location = location
                        .parent()
                        .ok_or_else(|| invalid("remote path cannot leave its root"))?
                }
                name => location = location.child(name)?,
            }
        }
        Ok(Some(location))
    }

    /// The remote's name, which selects its provider.
    pub fn remote(&self) -> &str {
        &self.remote
    }
    /// The folder names below the root.
    pub fn parts(&self) -> &[String] {
        &self.parts
    }
    /// The last name; `None` at the root.
    pub fn name(&self) -> Option<&str> {
        self.parts.last().map(String::as_str)
    }
    /// The path below the root, without a leading slash (empty at the root).
    pub fn relative(&self) -> String {
        self.parts.join("/")
    }
    /// The panel address, never a local download or cache path.
    pub fn panel_path(&self) -> PathBuf {
        PathBuf::from(format!("{}:/{}", self.remote, self.relative()))
    }
    /// Resolves a single entry name without permitting traversal.
    pub fn child(&self, name: &str) -> io::Result<Self> {
        validate_entry_name(name)?;
        let mut child = self.clone();
        child.parts.push(name.into());
        Ok(child)
    }
    /// The parent folder; the root has none.
    pub fn parent(&self) -> Option<Self> {
        let mut parent = self.clone();
        parent.parts.pop()?;
        Some(parent)
    }
}

/// Whether a path is in the remote namespace (`<remote>:...`), even if
/// malformed, so it is never treated as a local path.
pub fn is_remote(path: &Path) -> bool {
    split_remote(path).is_some()
}

/// Remote names are at least two characters (so Windows drive letters are
/// never remotes) from a conservative set that needs no quoting.
/// Reports whether a name can identify a configured remote.
pub fn validate_remote_name(name: &str) -> bool {
    name.len() >= 2
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '@'))
        && !name.starts_with(['-', '.'])
}
fn split_remote(path: &Path) -> Option<(&str, &str)> {
    let (remote, rest) = path.to_str()?.split_once(':')?;
    validate_remote_name(remote).then_some((remote, rest))
}
/// Validates one entry name before adding it below a remote address.
pub fn validate_entry_name(name: &str) -> io::Result<()> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
        return Err(invalid("invalid remote entry name"));
    }
    Ok(())
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::{RemoteLocation, is_remote};
    use std::path::Path;

    #[test]
    fn parses_and_normalizes_remote_addresses() {
        let root = RemoteLocation::root("gdrive").unwrap();
        let child = root.child("résumé 2026").unwrap();
        assert_eq!(child.panel_path(), Path::new("gdrive:/résumé 2026"));
        assert_eq!(root.panel_path(), Path::new("gdrive:/"));
        assert_eq!(
            RemoteLocation::parse(&child.panel_path()).unwrap(),
            Some(child.clone())
        );
        assert_eq!(
            RemoteLocation::parse(Path::new("gdrive:")).unwrap(),
            Some(root.clone())
        );
        assert_eq!(child.parent(), Some(root.clone()));
        assert_eq!(root.parent(), None);
        assert_eq!(child.relative(), "résumé 2026");
        let location = RemoteLocation::parse(Path::new("nas:/docs/./reports/../2026"))
            .unwrap()
            .unwrap();
        assert_eq!(location.panel_path(), Path::new("nas:/docs/2026"));
        assert_eq!(location.parent().unwrap().relative(), "docs");
        assert_eq!(
            RemoteLocation::parse(Path::new("local/file")).unwrap(),
            None
        );
        assert_eq!(RemoteLocation::parse(Path::new("C:/Users")).unwrap(), None);
        assert_eq!(
            RemoteLocation::parse(Path::new("/tmp/nas:/x")).unwrap(),
            None
        );
    }

    #[test]
    fn remote_namespace_recognizes_malformed_paths() {
        assert!(is_remote(Path::new("proton:invalid")));
        assert!(!is_remote(Path::new("/home/a:b")));
        assert!(RemoteLocation::parse(Path::new("nas:/../../escape")).is_err());
        let b = RemoteLocation::root("proton").unwrap().child("b").unwrap();
        assert_eq!(
            RemoteLocation::parse(Path::new("proton:/a/../b/.")).unwrap(),
            Some(b)
        );
        assert!(RemoteLocation::parse(Path::new("proton:/..")).is_err());
        assert!(RemoteLocation::parse(Path::new("proton:/a/../..")).is_err());
        assert!(RemoteLocation::parse(Path::new("nas:/a\\b")).is_err());
        assert!(RemoteLocation::parse(Path::new("nas:a")).is_err());
        for name in ["", ".", "..", "../escape", "/absolute", "a\\b", "a\0b"] {
            assert!(
                RemoteLocation::root("x1").unwrap().child(name).is_err(),
                "{name:?}"
            );
        }
        for remote in ["", "x", "-flag", ".x", "a b", "a/b"] {
            assert!(RemoteLocation::root(remote).is_err(), "{remote:?}");
        }
    }
}

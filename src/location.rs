//! Typed addresses for local files, remote items, and archive members.
//! Construction does not open a file or connect to a provider.

use std::io;
use std::path::{Path, PathBuf};

use crate::{archive, remote};

/// An absolute native path. Its original spelling and non-UTF-8 bytes are kept.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LocalPath(PathBuf);

impl LocalPath {
    /// Accepts an absolute path without resolving symlinks or touching disk.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        if path.is_absolute() {
            Ok(Self(path))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected an absolute local path",
            ))
        }
    }

    /// Returns the native path for local filesystem operations.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// A normalized path within an archive, with an empty path for its root.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArchiveMember(String);

impl ArchiveMember {
    /// Validates a slash-separated archive member path without normalization.
    pub fn new(path: &str) -> io::Result<Self> {
        let valid = match archive::clean(path) {
            archive::Cleaned::Root => path.is_empty(),
            archive::Cleaned::Path(cleaned) => cleaned == path,
            archive::Cleaned::Unsafe => false,
        };
        if valid {
            Ok(Self(path.to_owned()))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid archive member path",
            ))
        }
    }

    /// Returns the normalized path, empty at the archive root.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An address independent of a live connection, listing, or file metadata.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Location {
    /// A native filesystem path.
    Local(LocalPath),
    /// A configured remote and a path in its namespace.
    Remote(remote::Location),
    /// A member of an archive at `source`. Browsing currently supports local sources.
    Archive {
        source: Box<Location>,
        member: ArchiveMember,
    },
}

/// A panel navigation destination, including the platform's places overview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelTarget {
    /// A local, remote, or archive address.
    Location(Location),
    /// The platform's top-level places view (currently Windows drives).
    Places,
}

impl PanelTarget {
    /// Adapts the target to the legacy panel path used by existing views.
    pub fn panel_path(&self) -> PathBuf {
        match self {
            Self::Location(location) => location.panel_path(),
            Self::Places => PathBuf::new(),
        }
    }

    /// Returns the file address when this is a directory target.
    pub fn location(&self) -> Option<&Location> {
        match self {
            Self::Location(location) => Some(location),
            Self::Places => None,
        }
    }

    /// Consumes the target and returns its file address, if it has one.
    pub fn into_location(self) -> Option<Location> {
        match self {
            Self::Location(location) => Some(location),
            Self::Places => None,
        }
    }

    /// Parses a panel path while preserving the special places overview.
    pub fn from_panel_path(path: &Path, archive_source: Option<&Path>) -> io::Result<Self> {
        if path.as_os_str().is_empty() {
            Ok(Self::Places)
        } else {
            Location::from_panel_path(path, archive_source).map(Self::Location)
        }
    }
}

impl Location {
    /// Constructs a native location without resolving its spelling.
    pub fn local(path: PathBuf) -> io::Result<Self> {
        Ok(Self::Local(LocalPath::new(path)?))
    }

    /// Constructs an archive address; the member is validated before use.
    pub fn archive(source: Location, member: &str) -> io::Result<Self> {
        Ok(Self::Archive {
            source: Box::new(source),
            member: ArchiveMember::new(member)?,
        })
    }

    /// Returns a native path only for native locations.
    pub fn as_local_path(&self) -> Option<&Path> {
        match self {
            Self::Local(path) => Some(path.as_path()),
            _ => None,
        }
    }

    /// The existing panel spelling, for address input and older UI callers.
    pub fn panel_path(&self) -> PathBuf {
        match self {
            Self::Local(path) => path.as_path().to_path_buf(),
            Self::Remote(path) => path.panel_path(),
            Self::Archive { source, member } if member.as_str().is_empty() => source.panel_path(),
            Self::Archive { source, member } => source.panel_path().join(member.as_str()),
        }
    }

    /// Parses an existing panel path. The optional archive source removes
    /// ambiguity when a listed member has the same spelling as a local path.
    pub fn from_panel_path(path: &Path, archive_source: Option<&Path>) -> io::Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty location",
            ));
        }
        if let Some(remote) = remote::Location::parse(path)? {
            return Ok(Self::Remote(remote));
        }
        if let Some(source) = archive_source
            && let Some(inner) = archive::inner_of(source, path)
        {
            return Self::archive(Self::local(source.to_path_buf())?, &inner);
        }
        Self::local(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::{ArchiveMember, Location, PanelTarget};
    use std::path::PathBuf;

    #[test]
    fn local_paths_keep_bytes_and_symlink_spelling() {
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            let path = PathBuf::from("/link").join(OsString::from_vec(vec![b'f', 0xff]));
            let location = Location::local(path.clone()).unwrap();
            assert_eq!(location.as_local_path(), Some(path.as_path()));
            assert_eq!(location.panel_path(), path);
        }
        #[cfg(unix)]
        let spelled = PathBuf::from("/link/../file");
        #[cfg(windows)]
        let spelled = PathBuf::from(r"C:\link\..\file");
        assert_eq!(
            Location::local(spelled.clone()).unwrap().panel_path(),
            spelled
        );
        assert!(Location::local(PathBuf::from("relative")).is_err());
    }

    #[test]
    fn remote_and_archive_addresses_never_expose_native_paths() {
        let remote = Location::from_panel_path(PathBuf::from("nas:/docs").as_path(), None).unwrap();
        assert!(remote.as_local_path().is_none());
        let archive_file = std::env::current_dir().unwrap().join("x.zip");
        let archive =
            Location::archive(Location::local(archive_file.clone()).unwrap(), "a/b").unwrap();
        assert!(archive.as_local_path().is_none());
        assert_eq!(archive.panel_path(), archive_file.join("a").join("b"));
        assert_eq!(ArchiveMember::new("").unwrap().as_str(), "");
        assert_eq!(
            Location::archive(Location::local(archive_file.clone()).unwrap(), "")
                .unwrap()
                .panel_path(),
            archive_file
        );
        assert!(Location::from_panel_path(std::path::Path::new(""), None).is_err());
        for bad in ["../x", "/x", "a//b", "a/./b", "a\\b", "a/../b"] {
            assert!(ArchiveMember::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn places_is_a_panel_target_not_a_file_location() {
        let target = PanelTarget::from_panel_path(std::path::Path::new(""), None).unwrap();
        assert_eq!(target, PanelTarget::Places);
        assert!(target.location().is_none());
        assert!(target.into_location().is_none());

        let root = std::env::current_dir().unwrap();
        let local = PanelTarget::from_panel_path(&root, None).unwrap();
        assert_eq!(local.panel_path(), root);
        assert!(matches!(local, PanelTarget::Location(Location::Local(_))));
    }
}

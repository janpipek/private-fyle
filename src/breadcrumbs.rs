//! UI-independent breadcrumb paths and editable location parsing.

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::location::PanelTarget;

/// The visual role of one clickable part of a panel path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A native filesystem component.
    Normal,
    /// The archive file at the boundary of native and virtual paths.
    Archive,
    /// A directory component inside an archive.
    ArchiveInner,
    /// The root of a remote, labelled with its service.
    Remote,
}

/// One clickable breadcrumb and the location it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Text shown in the header.
    pub label: String,
    /// Location opened when the segment is chosen.
    pub path: PathBuf,
    /// How the view should distinguish this segment.
    pub kind: Kind,
}

/// Identifies the panel state a background navigation started from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationGuard {
    target: PanelTarget,
    generation: u64,
}

impl NavigationGuard {
    /// Captures the path and listing generation before background work starts.
    pub fn new(target: PanelTarget, generation: u64) -> Self {
        Self { target, generation }
    }

    /// Whether a result can still replace the panel without overwriting a
    /// newer load or navigation.
    pub fn matches(&self, target: &PanelTarget, generation: u64) -> bool {
        &self.target == target && self.generation == generation
    }
}

/// Builds clickable segments, keeping an archive file separate from its
/// internal path. A remote path starts at its remote's root, which shows
/// `remote_label` (the service's name; the remote's own name if unknown).
pub fn segments(
    path: &Path,
    archive_file: Option<&Path>,
    inner: Option<&str>,
    remote_label: Option<&str>,
) -> Vec<Segment> {
    if crate::remote::is_remote(path) {
        return remote_segments(path, remote_label);
    }
    let Some(file) = archive_file else {
        return path_segments(path, Kind::Normal);
    };
    let mut result = path_segments(file, Kind::Normal);
    if let Some(last) = result.last_mut() {
        last.kind = Kind::Archive;
    }
    let mut target = file.to_path_buf();
    for part in inner
        .unwrap_or_default()
        .split('/')
        .filter(|p| !p.is_empty())
    {
        target.push(part);
        result.push(Segment {
            label: part.to_string(),
            path: target.clone(),
            kind: Kind::ArchiveInner,
        });
    }
    result
}

/// Never touches the disk. A malformed remote path is one unclickable-ish
/// segment that reopens itself.
fn remote_segments(path: &Path, label: Option<&str>) -> Vec<Segment> {
    let Ok(Some(location)) = crate::remote::Location::parse(path) else {
        return vec![Segment {
            label: path.display().to_string(),
            path: path.to_path_buf(),
            kind: Kind::Remote,
        }];
    };
    let mut target = crate::remote::Location::root(location.remote())
        .expect("a parsed location has a valid remote name");
    let mut result = vec![Segment {
        label: label.unwrap_or(location.remote()).to_string(),
        path: target.panel_path(),
        kind: Kind::Remote,
    }];
    for part in location.parts() {
        target = target.child(part).expect("parsed parts are valid names");
        result.push(Segment {
            label: part.clone(),
            path: target.panel_path(),
            kind: Kind::Normal,
        });
    }
    result
}

fn path_segments(path: &Path, kind: Kind) -> Vec<Segment> {
    let mut result: Vec<Segment> = Vec::new();
    let mut target = PathBuf::new();
    for component in path.components() {
        target.push(component.as_os_str());
        // A drive's root joins its prefix: one `C:\` segment, not `C:` and `\`.
        if let (Component::RootDir, Some(prefix)) = (component, result.last_mut()) {
            prefix.label.push(std::path::MAIN_SEPARATOR);
            prefix.path = target.clone();
            continue;
        }
        let label = match component {
            Component::RootDir => std::path::MAIN_SEPARATOR.to_string(),
            Component::Prefix(prefix) => prefix.as_os_str().to_string_lossy().into_owned(),
            Component::CurDir => ".".into(),
            Component::ParentDir => "..".into(),
            Component::Normal(part) => part.to_string_lossy().into_owned(),
        };
        result.push(Segment {
            label,
            path: target.clone(),
            kind,
        });
    }
    if result.is_empty() {
        result.push(Segment {
            label: "Drives".into(),
            path: PathBuf::new(),
            kind,
        });
    }
    result
}

/// Text shown in the editable path field and copied to the clipboard.
pub fn editable_path(path: &Path) -> String {
    path.display().to_string()
}

/// Turns typed or pasted location text into a model path. Relative paths are
/// resolved against the current directory; `~` expands to the user's home.
pub fn parse_location(
    text: &str,
    current: &Path,
    home: Option<&Path>,
    archive_file: Option<&Path>,
) -> io::Result<PathBuf> {
    if text.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "enter a location",
        ));
    }
    // `<remote>:/path` is never a local path and must not be resolved
    // against (or checked on) the disk.
    if crate::remote::is_remote(Path::new(text)) {
        return remote_path(Path::new(text));
    }
    let path = expand_home(text, home)?;
    let path = if path.is_absolute() || path.as_os_str().is_empty() {
        path
    } else {
        current.join(path)
    };
    if crate::remote::is_remote(&path) {
        // Relative to a remote folder.
        return remote_path(&path);
    }
    match archive_file.filter(|file| path.starts_with(file)) {
        Some(file) => normalize_archive_path(file, &path),
        None => Ok(path),
    }
}

/// The normalized panel path of a remote location (no `.` or `..`).
fn remote_path(path: &Path) -> io::Result<PathBuf> {
    match crate::remote::Location::parse(path)? {
        Some(location) => Ok(location.panel_path()),
        None => Ok(path.to_path_buf()),
    }
}

fn normalize_archive_path(file: &Path, path: &Path) -> io::Result<PathBuf> {
    let rest = path
        .strip_prefix(file)
        .expect("archive prefix was checked before normalization");
    let mut inner = Vec::new();
    for component in rest.components() {
        match component {
            Component::Normal(part) => inner.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir if inner.pop().is_some() => {}
            Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "archive path cannot leave the archive",
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid archive path",
                ));
            }
        }
    }
    let mut normalized = file.to_path_buf();
    normalized.extend(inner);
    Ok(normalized)
}

fn expand_home(text: &str, home: Option<&Path>) -> io::Result<PathBuf> {
    if text == "~" {
        return home.map(Path::to_path_buf).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable")
        });
    }
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return home.map(|home| home.join(rest)).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable")
        });
    }
    Ok(PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use crate::location::PanelTarget;
    use std::path::Path;

    use super::{Kind, NavigationGuard, editable_path, parse_location, segments};

    #[test]
    fn ordinary_path_has_clickable_ancestors() {
        let crumbs = segments(Path::new("/home/u/work"), None, None, None);
        assert_eq!(
            crumbs.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["/", "home", "u", "work"]
        );
        assert_eq!(crumbs[2].path, Path::new("/home/u"));
        assert!(crumbs.iter().all(|c| c.kind == Kind::Normal));
    }

    #[cfg(windows)]
    #[test]
    fn drive_and_root_are_one_segment() {
        let crumbs = segments(Path::new(r"C:\Users\u"), None, None, None);
        assert_eq!(
            crumbs.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            [r"C:\", "Users", "u"]
        );
        assert_eq!(crumbs[0].path, Path::new(r"C:\"));
    }

    #[test]
    fn archive_and_internal_path_are_distinct() {
        let file = Path::new("/tmp/photos.zip");
        let crumbs = segments(
            Path::new("/tmp/photos.zip/2025/raw"),
            Some(file),
            Some("2025/raw"),
            None,
        );
        assert_eq!(crumbs[2].kind, Kind::Archive);
        assert_eq!(crumbs[3].kind, Kind::ArchiveInner);
        assert_eq!(crumbs[3].path, Path::new("/tmp/photos.zip/2025"));
        assert_eq!(
            editable_path(Path::new("/tmp/photos.zip/2025/raw")),
            "/tmp/photos.zip/2025/raw"
        );
    }

    #[test]
    fn typed_locations_support_home_relative_and_archive_paths() {
        let home = Path::new("/home/u");
        assert_eq!(
            parse_location("~/work", Path::new("/tmp"), Some(home), None).unwrap(),
            Path::new("/home/u/work")
        );
        assert_eq!(
            parse_location("child", Path::new("/tmp"), Some(home), None).unwrap(),
            Path::new("/tmp/child")
        );
        assert_eq!(
            parse_location("/tmp/a.zip/x/y", Path::new("/"), Some(home), None).unwrap(),
            Path::new("/tmp/a.zip/x/y")
        );
        assert_eq!(
            parse_location(" name ", Path::new("/tmp"), Some(home), None).unwrap(),
            Path::new("/tmp/ name ")
        );
    }

    #[test]
    fn normalizes_parent_components_only_inside_current_archive() {
        let file = Path::new("/tmp/a.zip");
        assert_eq!(
            parse_location("../y", Path::new("/tmp/a.zip/x"), None, Some(file)).unwrap(),
            Path::new("/tmp/a.zip/y")
        );
        assert!(parse_location("../../y", Path::new("/tmp/a.zip/x"), None, Some(file)).is_err());
        assert_eq!(
            parse_location("a/../b", Path::new("/tmp"), None, None).unwrap(),
            Path::new("/tmp/a/../b")
        );
    }

    #[test]
    fn remote_paths_parse_and_show_without_the_disk() {
        let current = Path::new("/tmp");
        assert_eq!(
            parse_location("gdrive:/Docs/../Pics/", current, None, None).unwrap(),
            Path::new("gdrive:/Pics")
        );
        assert_eq!(
            parse_location("gdrive:", current, None, None).unwrap(),
            Path::new("gdrive:/")
        );
        assert_eq!(
            parse_location("sub", Path::new("gdrive:/a"), None, None).unwrap(),
            Path::new("gdrive:/a/sub")
        );
        assert_eq!(
            parse_location("/etc", Path::new("gdrive:/a"), None, None).unwrap(),
            Path::new("/etc")
        );
        assert!(parse_location("gdrive:/..", current, None, None).is_err());
        let crumbs = segments(
            Path::new("gdrive:/Docs/2026"),
            None,
            None,
            Some("Google Drive"),
        );
        assert_eq!(
            crumbs.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["Google Drive", "Docs", "2026"]
        );
        assert_eq!(crumbs[0].kind, Kind::Remote);
        assert_eq!(crumbs[0].path, Path::new("gdrive:/"));
        assert_eq!(crumbs[2].path, Path::new("gdrive:/Docs/2026"));
        let unknown = segments(Path::new("other:/"), None, None, None);
        assert_eq!(unknown[0].label, "other");
    }

    #[test]
    fn navigation_guard_rejects_stale_results() {
        let one = PanelTarget::from_panel_path(Path::new("/one"), None).unwrap();
        let two = PanelTarget::from_panel_path(Path::new("/two"), None).unwrap();
        let guard = NavigationGuard::new(one.clone(), 4);
        assert!(guard.matches(&one, 4));
        assert!(!guard.matches(&two, 4));
        assert!(!guard.matches(&one, 5));

        let file = crate::location::Location::local("/tmp/a.zip".into()).unwrap();
        let archive = crate::location::Location::archive(file.clone(), "").unwrap();
        assert_eq!(file.panel_path(), archive.panel_path());
        let local_guard = NavigationGuard::new(PanelTarget::Location(file), 8);
        assert!(!local_guard.matches(&PanelTarget::Location(archive), 8));
    }
}

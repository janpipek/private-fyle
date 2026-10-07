//! Remote locations: the provider interface, `<remote>:/path` addresses and
//! the registry of configured remotes, independent of panel rendering.

pub mod connections;
pub mod hostkey;
pub mod jobs;
pub mod process;
pub mod proton;
pub mod rclone;
pub mod remotefs;

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use crate::fs_ops::{Attrs, Entry};

/// How a provider removes items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Into the service's trash, where it can be restored.
    Trash,
    /// For good.
    Permanent,
}

/// What a provider supports beyond listing and downloading, so the UI can
/// offer only working commands and explain the others.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub upload: bool,
    /// `None` when items cannot be removed.
    pub removal: Option<Removal>,
    pub create_folder: bool,
    /// Renaming in place.
    pub rename: bool,
    /// Moving to another folder on the same remote.
    pub move_within: bool,
    /// [`Provider::web_url`] works, so items can open in the browser.
    pub web: bool,
}

/// Cancellation and progress for one provider call. Providers report what
/// they can: per byte, per item, or nothing at all.
pub struct Control<'a> {
    pub cancel: &'a AtomicBool,
    /// Called with the item being transferred and the bytes done so far
    /// in this call (not per item).
    pub progress: &'a (dyn Fn(&str, u64) + Sync),
}

impl<'a> Control<'a> {
    /// Cancellable, without progress reports.
    pub fn new(cancel: &'a AtomicBool) -> Self {
        Self {
            cancel,
            progress: &|_, _| {},
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The error a provider returns when it stops because of [`Self::cancel`].
    pub fn cancelled_error() -> io::Error {
        io::Error::new(io::ErrorKind::Interrupted, "cancelled")
    }
}

/// A remote file service. Every method may block for a long time: call
/// them only from a background worker, through [`Control`] to cancel.
///
/// Transfers never overwrite: an item whose name already exists at the
/// destination is skipped, so callers check conflicts first (from a
/// listing) and ask the user. Optional operations default to an
/// `Unsupported` error and are announced in [`Capabilities`].
pub trait Provider: Send + Sync {
    /// Remote name used in panel paths (`<name>:/...`).
    fn name(&self) -> &str;
    /// The service, for Locations and messages (e.g. "Google Drive").
    fn label(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    /// Lists one folder, without a ".." entry.
    fn list(&self, dir: &Location, control: &Control) -> io::Result<Listing>;
    /// Downloads `source` (a file or a whole folder) into the local folder
    /// `destination`, keeping its name.
    fn download(
        &self,
        source: &Location,
        is_dir: bool,
        destination: &Path,
        control: &Control,
    ) -> io::Result<()>;
    /// Uploads a local file or folder into the remote folder `destination`.
    fn upload(
        &self,
        _source: &Path,
        _destination: &Location,
        _control: &Control,
    ) -> io::Result<()> {
        Err(unsupported(self.label(), "uploading"))
    }
    /// Removes a file or folder as [`Capabilities::removal`] says.
    fn remove(&self, _target: &Location, _is_dir: bool, _control: &Control) -> io::Result<()> {
        Err(unsupported(self.label(), "removing items"))
    }
    /// Creates the folder `name` in `parent`.
    fn create_folder(&self, _parent: &Location, _name: &str, _control: &Control) -> io::Result<()> {
        Err(unsupported(self.label(), "creating folders"))
    }
    /// Renames `target` in place.
    fn rename(&self, _target: &Location, _new_name: &str, _control: &Control) -> io::Result<()> {
        Err(unsupported(self.label(), "renaming"))
    }
    /// Moves `target` into the folder `destination` on the same remote.
    fn move_to(
        &self,
        _target: &Location,
        _destination: &Location,
        _control: &Control,
    ) -> io::Result<()> {
        Err(unsupported(self.label(), "moving"))
    }
    /// The address of `target` in the service's private web view, which
    /// needs the user's own login. Never a public or shared link. May run a
    /// provider call, so call it in the background; announced by
    /// [`Capabilities::web`].
    fn web_url(&self, _target: &Location, _control: &Control) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{} has no web view", self.label()),
        ))
    }
}

/// A folder or item on a remote, kept distinct from local paths. Shown in
/// panels as `<remote>:/a/b`; the root is `<remote>:/`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    remote: String,
    parts: Vec<String>,
}

impl Location {
    /// The root of `remote`.
    pub fn root(remote: &str) -> io::Result<Self> {
        if !valid_remote(remote) {
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
                        .ok_or_else(|| invalid("remote path cannot leave its root"))?;
                }
                name => location = location.child(name)?,
            }
        }
        Ok(Some(location))
    }

    /// The remote's name, which selects its [`Provider`].
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

    /// The path below the root, without a leading slash ("" at the root).
    pub fn relative(&self) -> String {
        self.parts.join("/")
    }

    /// The panel address, never a local download or cache path.
    pub fn panel_path(&self) -> PathBuf {
        PathBuf::from(format!("{}:/{}", self.remote, self.relative()))
    }

    /// Resolves a single entry name without permitting traversal.
    pub fn child(&self, name: &str) -> io::Result<Self> {
        validate_name(name)?;
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
fn valid_remote(name: &str) -> bool {
    name.len() >= 2
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '@'))
        && !name.starts_with(['-', '.'])
}

fn split_remote(path: &Path) -> Option<(&str, &str)> {
    let (remote, rest) = path.to_str()?.split_once(':')?;
    valid_remote(remote).then_some((remote, rest))
}

fn validate_name(name: &str) -> io::Result<()> {
    if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
        return Err(invalid("invalid remote entry name"));
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn unsupported(label: &str, what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{label} does not support {what}"),
    )
}

/// One listed item, as providers report it.
pub struct Item {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

/// A folder listing and what had to be left out of it.
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// Problems worth showing once (hidden duplicates, unusable names); the
    /// folder still opens.
    pub warnings: Vec<String>,
}

/// Turns provider items into panel entries below `dir`. A panel path must
/// name exactly one item, so of items sharing a name only the first is kept,
/// and items with unsafe names are dropped; both are reported as warnings.
pub fn entries(dir: &Location, items: Vec<Item>) -> Listing {
    entries_with_hint(dir, items, "")
}

/// Like [`entries`]; `hint` (e.g. "; run rclone dedupe") ends the warning
/// about duplicate names.
pub fn entries_with_hint(dir: &Location, items: Vec<Item>, hint: &str) -> Listing {
    let mut seen = HashSet::new();
    let (mut duplicates, mut unsafe_names) = (0, 0);
    let mut listing = Vec::new();
    for item in items {
        let Ok(child) = dir.child(&item.name) else {
            unsafe_names += 1;
            continue;
        };
        if !seen.insert(item.name.clone()) {
            duplicates += 1;
            continue;
        }
        listing.push(Entry {
            name: item.name,
            path: child.panel_path(),
            is_dir: item.is_dir,
            is_parent: false,
            size: item.size,
            modified: item.modified,
            attrs: Attrs {
                created: item.created,
                ..Default::default()
            },
            link: Default::default(),
        });
    }
    let shown = dir.panel_path().display().to_string();
    let plural = |n: usize| if n == 1 { "item" } else { "items" };
    let mut warnings = Vec::new();
    if duplicates > 0 {
        warnings.push(format!(
            "{shown}: {duplicates} {} {} a name with another item and {} hidden{hint}",
            plural(duplicates),
            if duplicates == 1 { "shares" } else { "share" },
            if duplicates == 1 { "is" } else { "are" },
        ));
    }
    if unsafe_names > 0 {
        warnings.push(format!(
            "{shown}: {unsafe_names} {} with unusable names skipped",
            plural(unsafe_names),
        ));
    }
    Listing {
        entries: listing,
        warnings,
    }
}

/// Percent-encodes `text` for a URL path segment or query value: only
/// ASCII letters, digits and `-_.~` stay as they are.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Whether `url` is only a service's start page (no path), which a provider
/// returns when it cannot address the exact item.
pub fn is_start_page(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    !rest.trim_end_matches('/').contains(['/', '?'])
}

/// Parses an RFC 3339 timestamp as services report them.
pub fn timestamp(text: Option<&str>) -> Option<SystemTime> {
    text.and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(Into::into)
}

/// The configured remotes, by name.
#[derive(Clone, Default)]
pub struct Registry {
    providers: Vec<Arc<dyn Provider>>,
}

impl Registry {
    /// Keeps the first provider of each name.
    pub fn new(providers: Vec<Arc<dyn Provider>>) -> Self {
        let mut names = HashSet::new();
        let providers = providers
            .into_iter()
            .filter(|p| valid_remote(p.name()) && names.insert(p.name().to_owned()))
            .collect();
        Self { providers }
    }

    /// Finds the configured services: Proton Drive through its official
    /// CLI, the remotes in `remotes.toml` (see [`connections`]), then every
    /// rclone remote; on a name clash the first wins. Runs programs and
    /// reads files, so call it in the background. Problems are returned,
    /// never fatal.
    pub fn discover(cancel: &AtomicBool) -> (Self, Vec<io::Error>) {
        let mut providers: Vec<Arc<dyn Provider>> = Vec::new();
        let mut errors = Vec::new();
        let proton = proton::Proton::default();
        if proton.is_installed() {
            providers.push(Arc::new(proton));
        }
        let (configs, config_errors) = connections::load_default();
        errors.extend(config_errors);
        providers.extend(
            configs.into_iter().map(|config| {
                Arc::new(remotefs::RemoteFsProvider::new(config)) as Arc<dyn Provider>
            }),
        );
        if rclone::Rclone::is_installed() {
            match rclone::Rclone::remotes(cancel) {
                Ok(remotes) => providers.extend(
                    remotes
                        .into_iter()
                        .map(|remote| Arc::new(remote) as Arc<dyn Provider>),
                ),
                Err(error) => errors.push(error),
            }
        }
        (Self::new(providers), errors)
    }

    pub fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.name() == name)
    }

    /// The provider and location for a panel path; `Ok(None)` for local paths.
    pub fn resolve(&self, path: &Path) -> io::Result<Option<(Arc<dyn Provider>, Location)>> {
        let Some(location) = Location::parse(path)? else {
            return Ok(None);
        };
        let provider = self.get(location.remote()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no remote named {} is configured", location.remote()),
            )
        })?;
        Ok(Some((provider.clone(), location)))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Capabilities, Control, Item, Listing, Location, Provider, Registry, entries,
        entries_with_hint, is_remote,
    };
    use std::io;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    struct Fake(&'static str);

    impl Provider for Fake {
        fn name(&self) -> &str {
            self.0
        }
        fn label(&self) -> &str {
            "Fake"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        fn list(&self, _: &Location, _: &Control) -> io::Result<Listing> {
            Ok(Listing::default())
        }
        fn download(&self, _: &Location, _: bool, _: &Path, _: &Control) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn locations_round_trip_and_stay_distinct_from_local_paths() {
        let root = Location::root("gdrive").unwrap();
        let child = root.child("résumé 2026").unwrap();
        assert_eq!(child.panel_path(), Path::new("gdrive:/résumé 2026"));
        assert_eq!(root.panel_path(), Path::new("gdrive:/"));
        assert_eq!(
            Location::parse(&child.panel_path()).unwrap(),
            Some(child.clone())
        );
        assert_eq!(
            Location::parse(Path::new("gdrive:")).unwrap(),
            Some(root.clone())
        );
        assert_eq!(child.parent(), Some(root.clone()));
        assert_eq!(root.parent(), None);
        assert_eq!(child.relative(), "résumé 2026");
        assert_eq!(Location::parse(Path::new("/tmp/gdrive:/x")).unwrap(), None);
        assert_eq!(Location::parse(Path::new("C:/Users")).unwrap(), None);
        assert!(is_remote(Path::new("proton:invalid")));
        assert!(!is_remote(Path::new("/home/a:b")));
    }

    #[test]
    fn normalizes_paths_without_escaping_the_root() {
        let b = Location::root("proton").unwrap().child("b").unwrap();
        assert_eq!(
            Location::parse(Path::new("proton:/a/../b/.")).unwrap(),
            Some(b)
        );
        for path in ["proton:/..", "proton:/a/../..", "proton:a", "proton:/a\\b"] {
            assert!(Location::parse(Path::new(path)).is_err(), "{path}");
        }
        for name in ["", ".", "..", "../escape", "/absolute", "a\\b", "a\0b"] {
            assert!(
                Location::root("x1").unwrap().child(name).is_err(),
                "{name:?}"
            );
        }
        for remote in ["", "x", "-flag", ".x", "a b", "a/b"] {
            assert!(Location::root(remote).is_err(), "{remote:?}");
        }
    }

    #[test]
    fn entries_hide_duplicates_and_skip_unsafe_names_with_warnings() {
        let root = Location::root("gdrive").unwrap();
        let item = |name: &str, size| Item {
            name: name.into(),
            is_dir: false,
            size,
            modified: None,
            created: None,
        };
        let listed = entries(&root, vec![item("a", 1), item("b", 2)]);
        assert_eq!(listed.entries[1].path, Path::new("gdrive:/b"));
        assert!(listed.warnings.is_empty());

        let listed = entries_with_hint(
            &root,
            vec![
                item("a", 1),
                item("b", 2),
                item("a", 3),
                item("a", 4),
                item("../x", 5),
            ],
            "; run rclone dedupe",
        );
        let names: Vec<_> = listed
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.size))
            .collect();
        assert_eq!(names, [("a", 1), ("b", 2)]);
        assert_eq!(
            listed.warnings,
            [
                "gdrive:/: 2 items share a name with another item and are hidden; run rclone dedupe",
                "gdrive:/: 1 item with unusable names skipped"
            ]
        );
        let one = entries(&root, vec![item("a", 1), item("a", 1)]);
        assert_eq!(
            one.warnings,
            ["gdrive:/: 1 item shares a name with another item and is hidden"]
        );
    }

    #[test]
    fn registry_resolves_by_name_and_keeps_the_first_of_each() {
        let registry = Registry::new(vec![
            Arc::new(Fake("proton")),
            Arc::new(Fake("proton")),
            Arc::new(Fake("gdrive")),
            Arc::new(Fake("x")),
        ]);
        assert_eq!(registry.providers().len(), 2);
        let (provider, location) = registry.resolve(Path::new("gdrive:/a")).unwrap().unwrap();
        assert_eq!(provider.name(), "gdrive");
        assert_eq!(location.relative(), "a");
        assert!(registry.resolve(Path::new("/tmp")).unwrap().is_none());
        assert!(registry.resolve(Path::new("other:/")).is_err());
        let cancel = AtomicBool::new(false);
        let control = Control::new(&cancel);
        let upload = provider.upload(Path::new("/tmp/a"), &location, &control);
        assert_eq!(upload.unwrap_err().kind(), io::ErrorKind::Unsupported);
        let folder = provider.create_folder(&location, "x", &control);
        assert_eq!(folder.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
}

//! Background-ready directory listing service for typed panel targets.
//!
//! The service owns filesystem, archive-index, and remote-provider reads. It
//! returns plain rows that a panel model can cache and filter without I/O.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use crate::archive;
use crate::entry::{Attrs, Entry, sort_entries};
use crate::fs_ops;
use crate::links::LinkState;
use crate::location::{Location, PanelTarget};
use crate::remote::{self, Control, Registry};
/// Shared listing request and result values used by the panel model.
pub mod contracts;
use contracts::{ListingOutcome, ListingRequest, ListingRow, ListingSnapshot};

/// Reads local, remote, archive, and places targets without a UI dependency.
#[derive(Clone, Default)]
pub struct ListingService {
    remotes: Arc<Registry>,
    remote_cache: Arc<RemoteListingCache>,
}

/// Bounded LRU cache of provider listing data, kept outside panel state.
#[derive(Default)]
pub struct RemoteListingCache {
    entries: Mutex<Vec<(remote::Location, CachedRemoteListing)>>,
}

#[derive(Clone)]
struct CachedRemoteListing {
    entries: Vec<Entry>,
    warnings: Vec<String>,
}

impl RemoteListingCache {
    /// Drops every cached provider response, for example after rediscovery.
    pub fn clear(&self) {
        self.entries
            .lock()
            .expect("remote listing cache lock is never poisoned")
            .clear();
    }

    fn get(&self, location: &remote::Location) -> Option<CachedRemoteListing> {
        let mut entries = self
            .entries
            .lock()
            .expect("remote listing cache lock is never poisoned");
        let ix = entries.iter().position(|(path, _)| path == location)?;
        let hit = entries.remove(ix);
        let listing = hit.1.clone();
        entries.push(hit);
        Some(listing)
    }

    fn insert(&self, location: remote::Location, listing: CachedRemoteListing) {
        let mut entries = self
            .entries
            .lock()
            .expect("remote listing cache lock is never poisoned");
        entries.retain(|(path, _)| path != &location);
        entries.push((location, listing));
        if entries.len() > 32 {
            entries.remove(0);
        }
    }
}

impl ListingService {
    /// Creates a service using the configured remote providers.
    #[cfg(test)]
    pub fn new(remotes: Arc<Registry>) -> Self {
        Self {
            remotes,
            remote_cache: Arc::default(),
        }
    }

    /// Creates a service sharing a panel's bounded remote-listing cache.
    pub fn with_cache(remotes: Arc<Registry>, remote_cache: Arc<RemoteListingCache>) -> Self {
        Self {
            remotes,
            remote_cache,
        }
    }

    /// Reads one target. Call this only from a background worker.
    pub fn list(
        &self,
        request: &ListingRequest,
        cancel: &AtomicBool,
    ) -> io::Result<ListingSnapshot> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(remote::Control::cancelled_error());
        }
        let mut snapshot = ListingSnapshot {
            target: request.target.clone(),
            rows: Vec::new(),
            warnings: Vec::new(),
            search_results: request.search_hits.is_some(),
            remote_origin: request.remote_origin.clone(),
        };
        if let Some(hits) = &request.search_hits {
            for row in hits {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(remote::Control::cancelled_error());
                }
                if row.entry.is_parent
                    || row
                        .location
                        .as_local_path()
                        .is_some_and(|path| std::fs::symlink_metadata(path).is_ok())
                {
                    snapshot.rows.push(row.clone());
                }
            }
            return Ok(snapshot);
        }
        match &request.target {
            PanelTarget::Places => {
                let mut entries = fs_ops::read_dir_cancellable(Path::new(""), true, Some(cancel))?;
                sort_entries(&mut entries, request.sort.key, request.sort.descending);
                snapshot.rows = entries
                    .into_iter()
                    .filter_map(|entry| {
                        let location = Location::from_panel_path(&entry.path, None).ok()?;
                        Some(ListingRow {
                            entry,
                            location,
                            depth: 0,
                            hit_line: None,
                        })
                    })
                    .collect();
            }
            PanelTarget::Location(location) => {
                self.list_directory(location, request, cancel, 0, &mut snapshot)?;
            }
        }
        snapshot.remote_origin =
            if matches!(snapshot.target, PanelTarget::Location(Location::Remote(_))) {
                request.remote_origin.clone()
            } else {
                None
            };
        Ok(snapshot)
    }

    /// Lists the nearest readable local ancestor while retaining the first
    /// failure for the status line. An unreadable local archive falls back to
    /// the directory containing its source file.
    pub fn list_with_ancestor_fallback(
        &self,
        request: &ListingRequest,
        cancel: &AtomicBool,
    ) -> io::Result<ListingOutcome> {
        let requested = request.target.clone();
        if !request.allow_ancestor_fallback {
            let snapshot = self.list(request, cancel)?;
            return Ok(ListingOutcome {
                requested,
                first_error: None,
                snapshot,
            });
        }
        let mut candidate = request.clone();
        let mut first_error = None;
        loop {
            match self.list(&candidate, cancel) {
                Ok(snapshot) => {
                    return Ok(ListingOutcome {
                        requested,
                        first_error,
                        snapshot,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
                Err(error) => {
                    first_error.get_or_insert(error);
                    let Some(fallback) = candidate
                        .target
                        .clone()
                        .into_location()
                        .and_then(fallback_location)
                    else {
                        return Err(first_error.expect("an error was recorded"));
                    };
                    candidate.target = PanelTarget::Location(fallback);
                }
            }
        }
    }

    fn list_directory(
        &self,
        location: &Location,
        request: &ListingRequest,
        cancel: &AtomicBool,
        depth: usize,
        snapshot: &mut ListingSnapshot,
    ) -> io::Result<()> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(remote::Control::cancelled_error());
        }
        if let Location::Local(local) = location
            && archive::format_of(local.as_path()).is_some()
            && std::fs::metadata(local.as_path()).is_ok_and(|metadata| metadata.is_file())
        {
            let archive = Location::archive(location.clone(), "")?;
            snapshot.target = PanelTarget::Location(archive.clone());
            snapshot.remote_origin = None;
            return self.list_directory(&archive, request, cancel, depth, snapshot);
        }
        let mut entries = match location {
            Location::Local(local) => {
                fs_ops::read_dir_cancellable(local.as_path(), true, Some(cancel))?
            }
            Location::Remote(remote_location) => {
                let provider = self.remotes.get(remote_location.remote()).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("no remote named {} is configured", remote_location.remote()),
                    )
                })?;
                let listing = if !request.force_remote_refresh {
                    self.remote_cache.get(remote_location)
                } else {
                    None
                };
                let listing = match listing {
                    Some(listing) => listing,
                    None => {
                        let control = Control::new(cancel);
                        let listing = provider.list(remote_location, &control)?;
                        if control.cancelled() {
                            return Err(Control::cancelled_error());
                        }
                        let cached = CachedRemoteListing {
                            entries: listing.entries.clone(),
                            warnings: listing.warnings.clone(),
                        };
                        self.remote_cache.insert(remote_location.clone(), cached);
                        CachedRemoteListing {
                            entries: listing.entries,
                            warnings: listing.warnings,
                        }
                    }
                };
                snapshot.warnings.extend(listing.warnings);
                let mut entries = parent_location(location, request.remote_origin.as_ref())
                    .map(parent_entry)
                    .into_iter()
                    .collect::<Vec<_>>();
                for mut entry in listing.entries {
                    if entry.is_parent {
                        continue;
                    }
                    match remote_location.child(&entry.name) {
                        Ok(child) => {
                            entry.path = child.panel_path();
                            entries.push(entry);
                        }
                        Err(error) => snapshot.warnings.push(format!(
                            "{}: skipped remote entry {:?}: {error}",
                            remote_location.panel_path().display(),
                            entry.name
                        )),
                    }
                }
                entries
            }
            Location::Archive { source, member } => {
                let file = source.as_local_path().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Unsupported, "archive source must be local")
                })?;
                let index = archive::Index::cached_cancellable(file, cancel)?;
                let inner = member.as_str();
                if !index.is_dir(inner) {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "not a folder in the archive",
                    ));
                }
                let parent = archive_parent(source, inner)?;
                let mut archive_entries = Vec::new();
                if let Some(parent) = parent {
                    archive_entries.push(parent_entry(parent));
                }
                archive_entries.extend(index.list(inner).map(|node| {
                    let child = Location::archive((**source).clone(), &node.path)
                        .expect("archive index paths are normalized");
                    Entry {
                        name: node.name.clone(),
                        path: child.panel_path(),
                        is_dir: node.is_dir(),
                        is_parent: false,
                        size: Some(node.size),
                        modified: node.modified,
                        attrs: Attrs {
                            mode: node.mode,
                            ..Default::default()
                        },
                        link: match &node.kind {
                            archive::Kind::Symlink(target) => LinkState::Link {
                                target: target.into(),
                            },
                            _ => LinkState::NotLink,
                        },
                    }
                }));
                archive_entries
            }
        };
        if depth > 0 {
            entries.retain(|entry| !entry.is_parent);
        }
        sort_entries(&mut entries, request.sort.key, request.sort.descending);
        for entry in entries {
            let child = if entry.is_parent {
                parent_location(location, request.remote_origin.as_ref())
                    .or_else(|| entry.path.as_os_str().is_empty().then(|| location.clone()))
                    .map(Ok)
                    .unwrap_or_else(|| Location::from_panel_path(&entry.path, None))?
            } else {
                entry_location(location, &entry)?
            };
            let descend = request.tree
                && matches!(location, Location::Local(_))
                && !entry.is_parent
                && entry.is_dir
                && request.expanded.contains(&child);
            let path = child.clone();
            snapshot.rows.push(ListingRow {
                entry,
                location: child,
                depth,
                hit_line: None,
            });
            if descend {
                // A failed expanded child remains a visible row and has no
                // children, matching the panel's prior best-effort tree read.
                if let Err(error) = self.list_directory(&path, request, cancel, depth + 1, snapshot)
                    && error.kind() == io::ErrorKind::Interrupted
                {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

/// Returns the typed parent represented by a directory's `..` entry.
fn parent_location(location: &Location, remote_origin: Option<&Location>) -> Option<Location> {
    match location {
        Location::Local(local) => {
            fs_ops::parent_of(local.as_path()).and_then(|path| Location::local(path).ok())
        }
        Location::Remote(remote_location) => remote_location
            .parent()
            .map(Location::Remote)
            .or_else(|| remote_origin.cloned()),
        Location::Archive { source, member } => match member.as_str().rsplit_once('/') {
            Some((parent, _)) => Location::archive((**source).clone(), parent).ok(),
            None if member.as_str().is_empty() => source
                .as_local_path()
                .and_then(fs_ops::parent_of)
                .and_then(|path| Location::local(path).ok()),
            None => Location::archive((**source).clone(), "").ok(),
        },
    }
}

fn archive_parent(source: &Location, inner: &str) -> io::Result<Option<Location>> {
    match inner.rsplit_once('/') {
        Some((parent, _)) => Ok(Some(Location::archive(source.clone(), parent)?)),
        None if inner.is_empty() => source
            .as_local_path()
            .and_then(fs_ops::parent_of)
            .map(Location::local)
            .transpose(),
        None => Ok(Some(Location::archive(source.clone(), "")?)),
    }
}

fn parent_entry(location: Location) -> Entry {
    Entry {
        name: "..".into(),
        path: location.panel_path(),
        is_dir: true,
        is_parent: true,
        size: Some(0),
        modified: None,
        attrs: Attrs::default(),
        link: LinkState::NotLink,
    }
}

fn entry_location(parent: &Location, entry: &Entry) -> io::Result<Location> {
    match parent {
        Location::Local(_) => Location::local(entry.path.clone()),
        Location::Remote(_) => remote::Location::parse(&entry.path)?
            .map(Location::Remote)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "provider row path is not remote",
                )
            }),
        Location::Archive { source, .. } => {
            let inner = crate::location::archive::inner_of(
                source.as_local_path().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::Unsupported, "archive source must be local")
                })?,
                &entry.path,
            )
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "archive row is outside its source",
                )
            })?;
            Location::archive((**source).clone(), &inner)
        }
    }
}

fn fallback_location(location: Location) -> Option<Location> {
    match location {
        Location::Local(local) => {
            fs_ops::parent_of(local.as_path()).and_then(|path| Location::local(path).ok())
        }
        Location::Archive { source, .. } => source
            .as_local_path()
            .and_then(fs_ops::parent_of)
            .and_then(|path| Location::local(path).ok()),
        Location::Remote(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ListingRequest, ListingService};
    use crate::entry::{Attrs, Entry, Sort, SortKey};
    use crate::location::{Location, PanelTarget};
    use crate::remote::{self, Capabilities, Control, Item, Listing, Provider};
    use std::collections::HashSet;
    use std::fs;
    use std::io::{self, Write};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn request(location: Location) -> ListingRequest {
        ListingRequest {
            target: PanelTarget::Location(location),
            tree: false,
            expanded: HashSet::new(),
            sort: Sort {
                key: SortKey::Name,
                descending: false,
            },
            remote_origin: None,
            allow_ancestor_fallback: false,
            search_hits: None,
            force_remote_refresh: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn local_snapshot_keeps_typed_rows_and_symlink_spelling() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("actual")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("actual", temp.path().join("alias")).unwrap();
        let spelling = temp.path().join("alias");
        let service = ListingService::default();
        let snapshot = service
            .list(
                &request(Location::local(spelling.clone()).unwrap()),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            snapshot.target,
            PanelTarget::Location(Location::local(spelling.clone()).unwrap())
        );
        assert!(
            snapshot
                .rows
                .iter()
                .all(|row| matches!(row.location, Location::Local(_)))
        );
    }

    #[test]
    fn local_failure_falls_back_and_preserves_first_error() {
        let temp = TempDir::new().unwrap();
        let requested = temp.path().join("missing").join("child");
        let service = ListingService::default();
        let outcome = service
            .list_with_ancestor_fallback(
                &ListingRequest {
                    allow_ancestor_fallback: true,
                    ..request(Location::local(requested.clone()).unwrap())
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(outcome.first_error.is_some());
        assert_eq!(
            outcome.snapshot.target,
            PanelTarget::Location(Location::local(temp.path().to_path_buf()).unwrap())
        );
    }

    #[test]
    fn cancellation_interrupts_before_reading() {
        let temp = TempDir::new().unwrap();
        let cancel = AtomicBool::new(true);
        let error = ListingService::default()
            .list(
                &request(Location::local(temp.path().to_path_buf()).unwrap()),
                &cancel,
            )
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    }

    #[test]
    fn expanded_tree_omits_parent_rows_below_the_root() {
        let temp = TempDir::new().unwrap();
        let child = temp.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("inside.txt"), "contents").unwrap();
        let root = Location::local(temp.path().to_path_buf()).unwrap();
        let child_location = Location::local(child).unwrap();
        let mut listing_request = request(root);
        listing_request.tree = true;
        listing_request.expanded.insert(child_location);

        let snapshot = ListingService::default()
            .list(&listing_request, &AtomicBool::new(false))
            .unwrap();
        assert!(snapshot.rows.iter().any(|row| {
            row.depth == 1 && row.entry.name == "inside.txt" && !row.entry.is_parent
        }));
        assert!(
            snapshot
                .rows
                .iter()
                .all(|row| row.depth == 0 || !row.entry.is_parent)
        );
    }

    #[test]
    fn unreadable_archive_falls_back_to_its_containing_directory() {
        let temp = TempDir::new().unwrap();
        let source = Location::local(temp.path().join("gone.zip")).unwrap();
        let target = Location::archive(source, "a").unwrap();
        let outcome = ListingService::default()
            .list_with_ancestor_fallback(
                &ListingRequest {
                    allow_ancestor_fallback: true,
                    ..request(target)
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(outcome.first_error.is_some());
        assert_eq!(
            outcome.snapshot.target,
            PanelTarget::Location(Location::local(temp.path().to_path_buf()).unwrap())
        );
    }

    #[test]
    fn archive_root_and_member_rows_keep_archive_identity() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("bundle.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("root.txt", options).unwrap();
        zip.write_all(b"root").unwrap();
        zip.start_file("docs/guide.txt", options).unwrap();
        zip.write_all(b"guide").unwrap();
        zip.finish().unwrap();

        let source = Location::local(archive_path.clone()).unwrap();
        let root = Location::archive(source.clone(), "").unwrap();
        let service = ListingService::default();
        let root_snapshot = service
            .list(&request(root.clone()), &AtomicBool::new(false))
            .unwrap();
        assert!(root_snapshot.rows.iter().any(|row| {
            row.entry.name == ".."
                && row.location == Location::local(temp.path().to_path_buf()).unwrap()
        }));
        assert!(root_snapshot.rows.iter().any(|row| {
            row.entry.name == "docs"
                && row.location == Location::archive(source.clone(), "docs").unwrap()
        }));

        let docs = Location::archive(source.clone(), "docs").unwrap();
        let docs_snapshot = service
            .list(&request(docs), &AtomicBool::new(false))
            .unwrap();
        assert!(
            docs_snapshot
                .rows
                .iter()
                .any(|row| { row.entry.name == ".." && row.location == root })
        );
        assert!(docs_snapshot.rows.iter().any(|row| {
            row.entry.name == "guide.txt"
                && row.location == Location::archive(source.clone(), "docs/guide.txt").unwrap()
        }));
    }

    struct MockProvider;

    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }
        fn label(&self) -> &str {
            "Mock"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        fn list(&self, dir: &remote::Location, _control: &Control) -> io::Result<Listing> {
            let mut entries = remote::entries(
                dir,
                vec![Item {
                    name: "folder".into(),
                    is_dir: true,
                    size: Some(0),
                    modified: None,
                    created: None,
                }],
            )
            .entries;
            entries.push(Entry {
                name: "../escape".into(),
                path: Path::new("/tmp/escape").to_path_buf(),
                is_dir: true,
                is_parent: false,
                size: Some(0),
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            });
            Ok(Listing {
                entries,
                warnings: Vec::new(),
            })
        }
        fn download(
            &self,
            _source: &remote::Location,
            _is_dir: bool,
            _destination: &Path,
            _control: &Control,
        ) -> io::Result<()> {
            Ok(())
        }
    }

    struct CountingProvider {
        calls: Arc<AtomicUsize>,
        cancel_next: AtomicBool,
    }

    impl Provider for CountingProvider {
        fn name(&self) -> &str {
            "counting"
        }

        fn label(&self) -> &str {
            "Counting"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn list(&self, dir: &remote::Location, control: &Control) -> io::Result<Listing> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.cancel_next.swap(false, Ordering::Relaxed) {
                control.cancel.store(true, Ordering::Relaxed);
            }
            Ok(remote::entries(
                dir,
                vec![Item {
                    name: "item.txt".into(),
                    is_dir: false,
                    size: Some(1),
                    modified: None,
                    created: None,
                }],
            ))
        }

        fn download(
            &self,
            _source: &remote::Location,
            _is_dir: bool,
            _destination: &Path,
            _control: &Control,
        ) -> io::Result<()> {
            Ok(())
        }
    }

    fn counting_service(calls: Arc<AtomicUsize>, cancel_next: bool) -> ListingService {
        ListingService::new(Arc::new(remote::Registry::new(vec![Arc::new(
            CountingProvider {
                calls,
                cancel_next: AtomicBool::new(cancel_next),
            },
        )])))
    }

    #[test]
    fn remote_cache_reuses_entries_and_refresh_can_bypass_it() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = counting_service(calls.clone(), false);
        let target = Location::Remote(remote::Location::root("counting").unwrap());
        let listing = request(target);
        service.list(&listing, &AtomicBool::new(false)).unwrap();
        service.list(&listing, &AtomicBool::new(false)).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        let mut refresh = listing;
        refresh.force_remote_refresh = true;
        service.list(&refresh, &AtomicBool::new(false)).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn remote_cache_is_bounded_and_keeps_recently_requested_listings() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = counting_service(calls.clone(), false);
        let root = remote::Location::root("counting").unwrap();
        let read = |index: usize| {
            service
                .list(
                    &request(Location::Remote(root.child(&index.to_string()).unwrap())),
                    &AtomicBool::new(false),
                )
                .unwrap();
        };
        for index in 0..32 {
            read(index);
        }
        assert_eq!(calls.load(Ordering::Relaxed), 32);
        read(0);
        read(32);
        assert_eq!(calls.load(Ordering::Relaxed), 33);
        read(0);
        assert_eq!(calls.load(Ordering::Relaxed), 33);
        read(1);
        assert_eq!(calls.load(Ordering::Relaxed), 34);
    }

    #[test]
    fn cancelled_remote_listing_is_not_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = counting_service(calls.clone(), true);
        let listing = request(Location::Remote(
            remote::Location::root("counting").unwrap(),
        ));
        let first = service.list(&listing, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(first.kind(), io::ErrorKind::Interrupted);
        service.list(&listing, &AtomicBool::new(false)).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn remote_listing_uses_provider_and_typed_parent_rows() {
        let service = ListingService::new(Arc::new(remote::Registry::new(vec![Arc::new(
            MockProvider,
        )])));
        let target = Location::Remote(remote::Location::root("mock").unwrap());
        let snapshot = service
            .list(&request(target), &AtomicBool::new(false))
            .unwrap();
        assert!(snapshot.rows.iter().any(|row| {
            row.entry.name == "folder"
                && row.location
                    == Location::Remote(
                        remote::Location::root("mock")
                            .unwrap()
                            .child("folder")
                            .unwrap(),
                    )
        }));
        assert!(snapshot.rows.iter().all(|row| !row.entry.is_parent));
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|warning| warning.contains("../escape"))
        );
        assert!(
            !snapshot
                .rows
                .iter()
                .any(|row| row.entry.name == "../escape")
        );

        let child = Location::Remote(
            remote::Location::root("mock")
                .unwrap()
                .child("folder")
                .unwrap(),
        );
        let mut child_request = request(child);
        child_request.remote_origin =
            Some(Location::local(std::env::temp_dir().join("from")).unwrap());
        let child_snapshot = service
            .list(&child_request, &AtomicBool::new(false))
            .unwrap();
        assert!(child_snapshot.rows.iter().any(|row| row.entry.is_parent
            && row.location == Location::Remote(remote::Location::root("mock").unwrap())));
    }
}

//! Cached remote listings and navigation metadata; never starts remote I/O.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::io;
#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Arc;

use super::Folder;
#[cfg(test)]
use super::{PanelContent, Row};
#[cfg(test)]
use crate::entry::{Entry, sort_entries};
#[cfg(test)]
use crate::fs_ops;
use crate::location::Location;
#[cfg(test)]
use crate::location::RemoteLocation;
use crate::remote;

/// Folders whose listings a panel keeps, so Back, Forward and Up are instant.
#[cfg(test)]
const LISTING_CAP: usize = 32;

/// The most recently loaded remote listings of one panel, by panel path.
/// Clones share the listings, because panels are cloned for background work.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct ListingCache {
    listings: HashMap<RemoteLocation, Arc<Vec<Entry>>>,
    /// Paths from least to most recently stored.
    order: Vec<RemoteLocation>,
}

#[cfg(test)]
impl ListingCache {
    /// Stores (or replaces) a listing; beyond the cap the oldest other than
    /// `keep` (the folder on show) is dropped.
    pub fn insert(
        &mut self,
        path: RemoteLocation,
        entries: Vec<Entry>,
        keep: Option<&RemoteLocation>,
    ) {
        self.order.retain(|p| p != &path);
        self.order.push(path.clone());
        self.listings.insert(path, Arc::new(entries));
        while self.order.len() > LISTING_CAP {
            let Some(ix) = self.order.iter().position(|p| Some(p) != keep) else {
                break;
            };
            let old = self.order.remove(ix);
            self.listings.remove(&old);
        }
    }

    pub fn get(&self, path: &RemoteLocation) -> Option<&Arc<Vec<Entry>>> {
        self.listings.get(path)
    }

    pub fn contains(&self, path: &RemoteLocation) -> bool {
        self.get(path).is_some()
    }
}

impl Folder {
    /// Whether this panel displays a remote folder.
    pub fn is_remote(&self) -> bool {
        matches!(self.location(), Some(Location::Remote(_)))
    }

    /// Installs a background result before navigation or history traversal.
    #[cfg(test)]
    pub fn cache_remote(&mut self, path: PathBuf, entries: Vec<Entry>) {
        if !self.is_remote() {
            self.remote_origin = self.location().cloned();
        }
        let path = RemoteLocation::parse(&path)
            .expect("background result has a validated address")
            .expect("background result names a remote");
        let keep = match self.location() {
            Some(Location::Remote(current)) => Some(current.clone()),
            _ => None,
        };
        self.remote_listings.insert(path, entries, keep.as_ref());
    }

    /// Whether showing `path` needs a background listing first: it is
    /// remote and not cached.
    #[cfg(test)]
    pub fn remote_needed(&self, path: &Path) -> bool {
        RemoteLocation::parse(path)
            .ok()
            .flatten()
            .is_some_and(|location| !self.remote_listings.contains(&location))
    }

    /// Shows an empty listing of the remote folder `target` while it is
    /// listed elsewhere (see [`Folder::remote_needed`]). The previous state
    /// comes back with [`Folder::end_archive_load`], which also cancels.
    #[cfg(test)]
    pub fn begin_remote_load(&mut self, target: PathBuf) {
        self.end_archive_load(false);
        let saved = Box::new(self.clone());
        self.entries.clear();
        self.all.clear();
        self.marked.clear();
        self.filter.clear();
        self.cursor = 0;
        self.mount = None;
        self.content = PanelContent::Directory(
            Location::from_panel_path(&target, None).expect("remote load receives a valid address"),
        );
        self.generation += 1;
        self.loading = Some(saved);
    }

    /// Parent of the current folder, including the return location at remote root.
    #[cfg(test)]
    pub fn parent_location(&self) -> Option<PathBuf> {
        match self.location() {
            Some(Location::Remote(location)) => location
                .parent()
                .map(|p| p.panel_path())
                .or_else(|| self.remote_origin.as_ref().map(Location::panel_path)),
            Some(Location::Local(path)) => fs_ops::parent_of(path.as_path()),
            Some(Location::Archive { .. }) => fs_ops::parent_of(&self.path()),
            None => None,
        }
    }

    /// The remote items for typed panel targets, with
    /// whether each is a folder as listed; paths that are not remote or not
    /// listed are skipped.
    pub fn remote_items(&self, locations: &[Location]) -> Vec<remote::jobs::RemoteItem> {
        locations
            .iter()
            .filter_map(|location| {
                let row = self.all.iter().find(|row| &row.location == location)?;
                let Location::Remote(location) = &row.location else {
                    return None;
                };
                Some(remote::jobs::RemoteItem {
                    location: location.clone(),
                    is_dir: row.is_dir,
                })
            })
            .collect()
    }

    /// Lists the cached remote folder `location` with a ".." row; fails with
    /// `WouldBlock` when it was not loaded in the background first.
    #[cfg(test)]
    pub fn list_remote(&self, location: &RemoteLocation, out: &mut Vec<Row>) -> io::Result<()> {
        let cached = self.remote_listings.get(location).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "remote folder must be loaded in the background",
            )
        })?;
        let parent = location
            .parent()
            .map(|p| p.panel_path())
            .or_else(|| self.remote_origin.as_ref().map(Location::panel_path));
        let mut entries: Vec<Entry> = parent
            .into_iter()
            .map(|path| Entry {
                name: "..".into(),
                path,
                is_dir: true,
                is_parent: true,
                size: Some(0),
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            })
            .collect();
        entries.extend(
            cached
                .iter()
                .filter(|e| self.show_hidden || !e.name.starts_with('.'))
                .cloned(),
        );
        sort_entries(&mut entries, self.sort.key, self.sort.descending);
        out.extend(entries.into_iter().map(|entry| {
            Row {
                location: Location::from_panel_path(&entry.path, None)
                    .expect("remote listing contains validated addresses"),
                entry,
                depth: 0,
                hit_line: None,
            }
        }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Folder, remote};
    use crate::entry::{Entry, Sort};
    use crate::location::Location as PanelLocation;
    use crate::location::RemoteLocation;
    use crate::remote::Item;

    fn root() -> RemoteLocation {
        RemoteLocation::root("proton").unwrap()
    }

    fn listing() -> Vec<Entry> {
        let item = Item {
            name: "a.txt".into(),
            is_dir: false,
            size: Some(1),
            modified: None,
            created: None,
        };
        remote::entries(&root(), vec![item]).entries
    }

    #[test]
    fn remote_navigation_sort_and_history_use_supplied_listings() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut folder = Folder::new(dir.path().into()).unwrap();
        folder.load().unwrap();
        folder.cache_remote(root().panel_path(), listing());
        folder.navigate(root().panel_path()).unwrap();
        assert!(folder.is_remote());
        assert!(matches!(folder.location(), Some(PanelLocation::Remote(_))));
        assert!(matches!(
            folder.entries()[1].location,
            PanelLocation::Remote(_)
        ));
        assert_eq!(folder.parent_location().as_deref(), Some(dir.path()));
        assert_eq!(folder.entries().len(), 2);
        folder.set_cursor(1);
        folder.toggle_mark();
        folder.set_sort(Sort::default());
        assert_eq!(folder.marked_count(), 1);
        assert!(folder.go_back().unwrap());
        assert!(!folder.is_remote());
        assert!(matches!(folder.location(), Some(PanelLocation::Local(_))));
        assert!(folder.go_forward().unwrap());
        assert!(folder.is_remote());
    }

    #[test]
    fn listing_cache_keeps_the_newest_and_the_folder_on_show() {
        use super::{LISTING_CAP, ListingCache};
        let mut cache = ListingCache::default();
        let shown = root().child("shown").unwrap();
        cache.insert(shown.clone(), listing(), Some(&shown));
        for n in 0..LISTING_CAP + 5 {
            cache.insert(
                root().child(&n.to_string()).unwrap(),
                listing(),
                Some(&shown),
            );
        }
        assert_eq!(cache.listings.len(), LISTING_CAP);
        assert!(cache.contains(&shown));
        assert!(!cache.contains(&root().child("0").unwrap()));
        assert!(cache.contains(&root().child(&(LISTING_CAP + 4).to_string()).unwrap()));
        // Replacing a listing does not grow the cache.
        cache.insert(shown.clone(), Vec::new(), Some(&shown));
        assert_eq!(cache.listings.len(), LISTING_CAP);
        assert!(cache.get(&shown).unwrap().is_empty());
    }

    #[test]
    fn remote_items_know_which_targets_are_folders() {
        let mut folder = Folder::new(root().panel_path()).unwrap();
        let items = vec![
            Item {
                name: "d".into(),
                is_dir: true,
                size: Some(0),
                modified: None,
                created: None,
            },
            Item {
                name: "f".into(),
                is_dir: false,
                size: Some(1),
                modified: None,
                created: None,
            },
        ];
        folder.cache_remote(root().panel_path(), remote::entries(&root(), items).entries);
        folder.navigate(root().panel_path()).unwrap();
        let locations = [
            PanelLocation::Remote(root().child("d").unwrap()),
            PanelLocation::Remote(root().child("f").unwrap()),
            PanelLocation::local(std::env::temp_dir().join("local")).unwrap(),
        ];
        let found = folder.remote_items(&locations);
        assert_eq!(found.len(), 2);
        assert!(found[0].is_dir && !found[1].is_dir);
    }

    #[test]
    fn remote_load_placeholder_restores_the_previous_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut folder = Folder::new(dir.path().into()).unwrap();
        folder.load().unwrap();
        assert!(!folder.remote_needed(dir.path()));
        assert!(folder.remote_needed(&root().panel_path()));
        folder.begin_remote_load(root().panel_path());
        assert!(folder.is_loading() && folder.entries().is_empty());
        assert_eq!(folder.path(), root().panel_path());
        folder.end_archive_load(true);
        assert!(!folder.is_loading());
        assert_eq!(folder.path(), dir.path());
        folder.cache_remote(root().panel_path(), listing());
        assert!(!folder.remote_needed(&root().panel_path()));
    }

    #[test]
    fn unloaded_remote_does_not_fall_back_to_local_paths() {
        let mut folder = Folder::new(root().panel_path()).unwrap();
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), root().panel_path());
    }

    fn dir_item(name: &str) -> Item {
        Item {
            name: name.into(),
            is_dir: true,
            size: Some(0),
            modified: None,
            created: None,
        }
    }

    /// Folder showing `proton:/a/b` (reached from a local folder through the
    /// drive root and `a`), with listings of every level cached.
    fn nested_folder(local: &std::path::Path) -> Folder {
        let a = root().child("a").unwrap();
        let b = a.child("b").unwrap();
        let mut folder = Folder::new(local.into()).unwrap();
        folder.load().unwrap();
        for (dir, items) in [
            (&root(), vec![dir_item("a")]),
            (&a, vec![dir_item("b"), dir_item("c")]),
            (&b, vec![dir_item("deep")]),
        ] {
            folder.cache_remote(dir.panel_path(), remote::entries(dir, items).entries);
        }
        for dir in [root(), a, b] {
            folder.navigate(dir.panel_path()).unwrap();
        }
        folder
    }

    #[test]
    fn remote_subfolders_have_a_parent_row_and_go_up_to_their_parent() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut folder = nested_folder(dir.path());
        let a = root().child("a").unwrap();
        let b = a.child("b").unwrap();
        // In a nested folder: `..` exists and leads to the parent folder.
        assert_eq!(folder.location(), Some(&PanelLocation::Remote(b.clone())));
        assert!(folder.entries()[0].is_parent);
        assert_eq!(folder.entries()[0].path, a.panel_path());
        assert_eq!(
            folder.parent_target().and_then(|t| t.into_location()),
            Some(PanelLocation::Remote(a.clone()))
        );
        // Leaving puts the cursor on the folder we came from, even when the
        // cursor was on another row.
        folder.set_cursor(1);
        let target = folder.parent_target().unwrap();
        assert_eq!(
            folder.selection_for_navigation(&target),
            Some(PanelLocation::Remote(b.clone()))
        );
        folder.go_up().unwrap();
        assert_eq!(folder.location(), Some(&PanelLocation::Remote(a.clone())));
        assert_eq!(folder.current().unwrap().name, "b");
        // One level below the root: `..` is the drive root.
        assert_eq!(folder.entries()[0].path, root().panel_path());
        let target = folder.parent_target().unwrap();
        assert_eq!(
            folder.selection_for_navigation(&target),
            Some(PanelLocation::Remote(a))
        );
        folder.go_up().unwrap();
        assert_eq!(folder.location(), Some(&PanelLocation::Remote(root())));
        assert_eq!(folder.current().unwrap().name, "a");
    }

    #[test]
    fn remote_root_parent_row_returns_to_where_the_panel_came_from() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut folder = nested_folder(dir.path());
        folder.navigate(root().panel_path()).unwrap();
        // Same as every other remote: `..` at the root is the local folder
        // the panel came from, and the cursor stays on the remote root.
        assert!(folder.entries()[0].is_parent);
        assert_eq!(folder.entries()[0].path, dir.path());
        let target = folder.parent_target().unwrap();
        assert_eq!(
            target.clone().into_location(),
            Some(PanelLocation::local(dir.path().into()).unwrap())
        );
        assert_eq!(
            folder.selection_for_navigation(&target),
            Some(PanelLocation::Remote(root()))
        );
        folder.go_up().unwrap();
        assert!(!folder.is_remote());
        assert_eq!(folder.path(), dir.path());
    }

    #[test]
    fn remote_root_without_a_known_origin_has_no_parent_row() {
        // Opened directly (e.g. `fyle proton:/`): nothing to return to.
        let mut folder = Folder::new(root().panel_path()).unwrap();
        folder.cache_remote(root().panel_path(), listing());
        folder.navigate(root().panel_path()).unwrap();
        assert!(folder.parent_target().is_none());
        assert!(folder.entries().iter().all(|row| !row.is_parent));
    }
}

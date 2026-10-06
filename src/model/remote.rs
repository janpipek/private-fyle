//! Cached remote listings and navigation metadata; never starts remote I/O.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Entry, Folder, Row};
use crate::{fs_ops, remote};

/// Folders whose listings a panel keeps, so Back, Forward and Up are instant.
const LISTING_CAP: usize = 32;

/// The most recently loaded remote listings of one panel, by panel path.
/// Clones share the listings, because panels are cloned for background work.
#[derive(Clone, Default)]
pub struct ListingCache {
    listings: HashMap<PathBuf, Arc<Vec<Entry>>>,
    /// Paths from least to most recently stored.
    order: Vec<PathBuf>,
}

impl ListingCache {
    /// Stores (or replaces) a listing; beyond the cap the oldest other than
    /// `keep` (the folder on show) is dropped.
    pub fn insert(&mut self, path: PathBuf, entries: Vec<Entry>, keep: &Path) {
        self.order.retain(|p| p != &path);
        self.order.push(path.clone());
        self.listings.insert(path, Arc::new(entries));
        while self.order.len() > LISTING_CAP {
            let Some(ix) = self.order.iter().position(|p| p != keep) else {
                break;
            };
            let old = self.order.remove(ix);
            self.listings.remove(&old);
        }
    }

    pub fn get(&self, path: &Path) -> Option<&Arc<Vec<Entry>>> {
        self.listings.get(path)
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.listings.contains_key(path)
    }
}

impl Folder {
    /// Whether this panel displays a remote folder.
    pub fn is_remote(&self) -> bool {
        remote::is_remote(self.path())
    }

    /// Installs a background result before navigation or history traversal.
    pub fn cache_remote(&mut self, path: PathBuf, entries: Vec<Entry>) {
        if !self.is_remote() {
            self.remote_origin = Some(self.path.clone());
        }
        self.remote_listings.insert(path, entries, &self.path);
    }

    /// Whether showing `path` needs a background listing first: it is
    /// remote and not cached.
    pub fn remote_needed(&self, path: &Path) -> bool {
        remote::is_remote(path) && !self.remote_listings.contains(path)
    }

    /// Shows an empty listing of the remote folder `target` while it is
    /// listed elsewhere (see [`Folder::remote_needed`]). The previous state
    /// comes back with [`Folder::end_archive_load`], which also cancels.
    pub fn begin_remote_load(&mut self, target: PathBuf) {
        self.end_archive_load(false);
        let saved = Box::new(self.clone());
        self.entries.clear();
        self.all.clear();
        self.marked.clear();
        self.filter.clear();
        self.results = None;
        self.cursor = 0;
        self.mount = None;
        self.path = target;
        self.generation += 1;
        self.loading = Some(saved);
    }

    /// Parent of the current folder, including the return location at remote root.
    pub fn parent_location(&self) -> Option<PathBuf> {
        match remote::Location::parse(self.path()).ok().flatten() {
            Some(location) => location
                .parent()
                .map(|p| p.panel_path())
                .or_else(|| self.remote_origin.clone()),
            None => fs_ops::parent_of(self.path()),
        }
    }

    /// The remote items for panel paths (e.g. [`Folder::targets`]), with
    /// whether each is a folder as listed; paths that are not remote or not
    /// listed are skipped.
    pub fn remote_items(&self, paths: &[PathBuf]) -> Vec<remote::jobs::RemoteItem> {
        paths
            .iter()
            .filter_map(|path| {
                let location = remote::Location::parse(path).ok().flatten()?;
                let row = self.all.iter().find(|row| &row.path == path)?;
                Some(remote::jobs::RemoteItem {
                    location,
                    is_dir: row.is_dir,
                })
            })
            .collect()
    }

    /// Lists the cached remote folder `path` with a ".." row; fails with
    /// `WouldBlock` when it was not loaded in the background first.
    pub fn list_remote(&self, path: &Path, out: &mut Vec<Row>) -> io::Result<()> {
        let cached = self.remote_listings.get(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "remote folder must be loaded in the background",
            )
        })?;
        let location =
            remote::Location::parse(path)?.expect("cached remote path is validated before listing");
        let parent = location
            .parent()
            .map(|p| p.panel_path())
            .or_else(|| self.remote_origin.clone());
        let mut entries: Vec<Entry> = parent
            .into_iter()
            .map(|path| Entry {
                name: "..".into(),
                path,
                is_dir: true,
                is_parent: true,
                size: 0,
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
        fs_ops::sort_entries(&mut entries, self.sort.key, self.sort.descending);
        out.extend(entries.into_iter().map(|entry| Row {
            entry,
            depth: 0,
            hit_line: None,
        }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Folder, remote};
    use crate::model::Sort;
    use crate::remote::{Item, Location};

    fn root() -> Location {
        Location::root("proton").unwrap()
    }

    fn listing() -> Vec<crate::fs_ops::Entry> {
        let item = Item {
            name: "a.txt".into(),
            is_dir: false,
            size: 1,
            modified: None,
            created: None,
        };
        remote::entries(&root(), "Test", vec![item]).unwrap()
    }

    #[test]
    fn remote_navigation_sort_and_history_use_supplied_listings() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut folder = Folder::new(dir.path().into());
        folder.load().unwrap();
        folder.cache_remote(root().panel_path(), listing());
        folder.navigate(root().panel_path()).unwrap();
        assert!(folder.is_remote());
        assert_eq!(folder.parent_location().as_deref(), Some(dir.path()));
        assert_eq!(folder.entries().len(), 2);
        folder.set_cursor(1);
        folder.toggle_mark();
        folder.set_sort(Sort::default());
        assert_eq!(folder.marked_count(), 1);
        assert!(folder.go_back().unwrap());
        assert!(!folder.is_remote());
        assert!(folder.go_forward().unwrap());
        assert!(folder.is_remote());
    }

    #[test]
    fn listing_cache_keeps_the_newest_and_the_folder_on_show() {
        use super::{LISTING_CAP, ListingCache};
        use std::path::{Path, PathBuf};
        let mut cache = ListingCache::default();
        let shown = Path::new("proton:/shown");
        cache.insert(shown.into(), listing(), shown);
        for n in 0..LISTING_CAP + 5 {
            cache.insert(PathBuf::from(format!("proton:/{n}")), listing(), shown);
        }
        assert_eq!(cache.listings.len(), LISTING_CAP);
        assert!(cache.contains(shown));
        assert!(!cache.contains(Path::new("proton:/0")));
        assert!(cache.contains(Path::new(&format!("proton:/{}", LISTING_CAP + 4))));
        // Replacing a listing does not grow the cache.
        cache.insert(shown.into(), Vec::new(), shown);
        assert_eq!(cache.listings.len(), LISTING_CAP);
        assert!(cache.get(shown).unwrap().is_empty());
    }

    #[test]
    fn remote_items_know_which_targets_are_folders() {
        let mut folder = Folder::new(root().panel_path());
        let items = vec![
            Item {
                name: "d".into(),
                is_dir: true,
                size: 0,
                modified: None,
                created: None,
            },
            Item {
                name: "f".into(),
                is_dir: false,
                size: 1,
                modified: None,
                created: None,
            },
        ];
        folder.cache_remote(
            root().panel_path(),
            remote::entries(&root(), "Test", items).unwrap(),
        );
        folder.navigate(root().panel_path()).unwrap();
        let paths = [
            root().child("d").unwrap().panel_path(),
            root().child("f").unwrap().panel_path(),
            "/local".into(),
        ];
        let found = folder.remote_items(&paths);
        assert_eq!(found.len(), 2);
        assert!(found[0].is_dir && !found[1].is_dir);
    }

    #[test]
    fn remote_load_placeholder_restores_the_previous_folder() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut folder = Folder::new(dir.path().into());
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
        let mut folder = Folder::new(root().panel_path());
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), root().panel_path());
    }
}

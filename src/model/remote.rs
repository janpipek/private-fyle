//! Cached remote listings and navigation metadata; never starts remote I/O.

use std::io;
use std::path::{Path, PathBuf};

use super::{Entry, Folder, Row};
use crate::{fs_ops, remote};

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
        self.remote_listing = Some((path, entries));
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

    /// Lists the cached remote folder `path` with a ".." row; fails with
    /// `WouldBlock` when it was not loaded in the background first.
    pub fn list_remote(&self, path: &Path, out: &mut Vec<Row>) -> io::Result<()> {
        let (cached_path, cached) = self
            .remote_listing
            .as_ref()
            .filter(|(cached, _)| cached == path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "remote folder must be loaded in the background",
                )
            })?;
        let location = remote::Location::parse(cached_path)?
            .expect("cached remote path is validated before listing");
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
    fn unloaded_remote_does_not_fall_back_to_local_paths() {
        let mut folder = Folder::new(root().panel_path());
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), root().panel_path());
    }
}

//! Remote selection metadata; listing execution belongs to the listing service.

use super::Folder;
use crate::location::Location;
use crate::remote;

impl Folder {
    /// Whether this panel displays a remote folder.
    pub fn is_remote(&self) -> bool {
        matches!(self.location(), Some(Location::Remote(_)))
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
}

#[cfg(test)]
mod tests {
    use super::remote;
    use crate::entry::{Entry, Sort};
    use crate::location::Location as PanelLocation;
    use crate::location::RemoteLocation;
    use crate::model::test_support::TestFolder;
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
        let mut folder = TestFolder::new(dir.path().into()).unwrap();
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
    fn remote_items_know_which_targets_are_folders() {
        let mut folder = TestFolder::new(root().panel_path()).unwrap();
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
    fn unloaded_remote_does_not_fall_back_to_local_paths() {
        let mut folder = TestFolder::new(root().panel_path()).unwrap();
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
    fn nested_folder(local: &std::path::Path) -> TestFolder {
        let a = root().child("a").unwrap();
        let b = a.child("b").unwrap();
        let mut folder = TestFolder::new(local.into()).unwrap();
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
        let mut folder = TestFolder::new(root().panel_path()).unwrap();
        folder.cache_remote(root().panel_path(), listing());
        folder.navigate(root().panel_path()).unwrap();
        assert!(folder.parent_target().is_none());
        assert!(folder.entries().iter().all(|row| !row.is_parent));
    }
}

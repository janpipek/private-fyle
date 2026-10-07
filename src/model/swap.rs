//! Exchanging the contents of two panels.

use super::Folder;

impl Folder {
    /// Exchanges everything the two panels hold: location or search results,
    /// rows, cursor, marks, filter, sort, tree state, sizes and navigation
    /// history. Listing and size generations move to a value newer than
    /// either panel has shown, so per-panel caches (metadata, thumbnails, Git
    /// status) and work still in flight for the old content are dropped
    /// rather than applied to the new one.
    pub fn swap_with(&mut self, other: &mut Folder) {
        let generation = self.generation.max(other.generation) + 1;
        let epoch = self.size_epoch.max(other.size_epoch) + 1;
        std::mem::swap(self, other);
        for folder in [self, other] {
            folder.generation = generation;
            folder.size_epoch = epoch;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use crate::location::Location;
    use crate::model::test_support::{fixture, loaded, names};
    use crate::model::{Folder, PanelContent, Sort};
    use crate::remote::{self, Item};

    fn two_folders() -> (tempfile::TempDir, tempfile::TempDir, Folder, Folder) {
        let (da, db) = (fixture(), fixture());
        fs::write(db.path().join("only-in-b.txt"), "x").unwrap();
        let (a, b) = (loaded(&da), loaded(&db));
        (da, db, a, b)
    }

    #[test]
    fn swaps_local_content_cursor_marks_sort_and_history() {
        let (da, db, mut a, mut b) = two_folders();
        a.set_cursor(2);
        a.toggle_mark();
        a.set_sort(Sort {
            descending: true,
            ..Sort::default()
        });
        b.navigate(db.path().join("sub")).unwrap();
        b.set_filter("x");
        let (a_names, b_names) = (
            names(&a).into_iter().map(String::from).collect::<Vec<_>>(),
            names(&b).into_iter().map(String::from).collect::<Vec<_>>(),
        );
        let (a_cursor, a_marks) = (a.cursor(), a.marked_count());

        a.swap_with(&mut b);

        assert_eq!(a.path(), db.path().join("sub"));
        assert_eq!(b.path(), da.path());
        assert_eq!(names(&a), b_names);
        assert_eq!(names(&b), a_names);
        assert_eq!(a.filter(), "x");
        assert_eq!((b.cursor(), b.marked_count()), (a_cursor, a_marks));
        assert!(b.sort().descending && !a.sort().descending);
        // History travels with its content: a can go back to db, b cannot.
        assert!(a.can_go_back() && !b.can_go_back());
        assert!(a.go_back().unwrap());
        assert_eq!(a.path(), db.path());
    }

    #[test]
    fn swap_is_its_own_inverse() {
        let (_da, _db, mut a, mut b) = two_folders();
        a.set_cursor(3);
        let (pa, pb) = (a.path(), b.path());
        a.swap_with(&mut b);
        a.swap_with(&mut b);
        assert_eq!((a.path(), b.path()), (pa, pb));
        assert_eq!(a.cursor(), 3);
    }

    #[test]
    fn swap_makes_generations_newer_than_both() {
        let (_da, _db, mut a, mut b) = two_folders();
        let before = (
            a.generation(),
            b.generation(),
            a.size_epoch(),
            b.size_epoch(),
        );
        a.swap_with(&mut b);
        for folder in [&a, &b] {
            assert!(folder.generation() > before.0.max(before.1));
            assert!(folder.size_epoch() > before.2.max(before.3));
        }
    }

    #[test]
    fn swaps_search_results_and_keeps_the_root() {
        let (da, _db, mut a, mut b) = two_folders();
        a.show_results(da.path().to_path_buf(), "found".into());
        a.add_results(vec![crate::search::Hit {
            entry: crate::fs_ops::Entry {
                name: "a.txt".into(),
                path: da.path().join("a.txt"),
                is_dir: false,
                is_parent: false,
                size: Some(1),
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            },
            line: None,
        }]);
        a.swap_with(&mut b);
        assert_eq!(b.results(), Some("found"));
        assert_eq!(b.path(), da.path());
        assert_eq!(b.entries().len(), 2);
        assert_eq!(a.results(), None);
        assert!(matches!(a.content(), PanelContent::Directory(_)));
    }

    #[test]
    fn swaps_remote_and_archive_locations() {
        let (da, _db, mut a, mut b) = two_folders();
        // Remote on one side (listing supplied, no network).
        let root = remote::Location::root("proton").unwrap();
        let item = Item {
            name: "r.txt".into(),
            is_dir: false,
            size: Some(1),
            modified: None,
            created: None,
        };
        b.cache_remote(
            root.panel_path(),
            remote::entries(&root, vec![item]).entries,
        );
        b.navigate(root.panel_path()).unwrap();
        // Archive on the other.
        let zip = da.path().join("pack.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        writer
            .start_file("in.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"t").unwrap();
        writer.finish().unwrap();
        crate::model::test_support::reload(&mut a);
        a.select_path(Some(&zip));
        a.activate().unwrap();
        assert!(a.archive().is_some() && b.is_remote());

        a.swap_with(&mut b);

        assert!(a.is_remote());
        assert!(matches!(a.location(), Some(Location::Remote(_))));
        assert_eq!(names(&a), ["..", "r.txt"]);
        assert_eq!(b.archive(), Some(zip.as_path()));
        assert!(matches!(b.location(), Some(Location::Archive { .. })));
        assert_eq!(names(&b), ["..", "in.txt"]);
    }

    #[test]
    fn swaps_places_with_a_folder() {
        let (_da, _db, mut a, mut b) = two_folders();
        b.replace_places().unwrap();
        a.swap_with(&mut b);
        assert_eq!(a.content(), &PanelContent::Places);
        assert!(matches!(b.content(), PanelContent::Directory(_)));
    }
}

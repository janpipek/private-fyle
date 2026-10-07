//! Sorting, tree expansion, the name filter and hidden files.

use std::path::Path;

use super::{Entry, Folder, Row, Sort};
use crate::fs_ops;

impl Folder {
    /// Re-sorts, keeping the cursor on the same entry.
    pub fn set_sort(&mut self, sort: Sort) {
        self.sort = sort;
        if self.results.is_some() {
            let selected = self.current().map(|e| e.path.clone());
            let mut lines = std::collections::HashMap::new();
            let mut entries: Vec<Entry> = self
                .all
                .drain(..)
                .map(|r| {
                    if let Some(line) = r.hit_line {
                        lines.insert(r.entry.path.clone(), line);
                    }
                    r.entry
                })
                .collect();
            fs_ops::sort_entries(&mut entries, sort.key, sort.descending);
            self.all = entries
                .into_iter()
                .map(|entry| Row {
                    hit_line: lines.remove(&entry.path),
                    entry,
                    depth: 0,
                })
                .collect();
            self.apply_filter();
            self.select_path(selected.as_deref());
            return;
        }
        self.load_or_ancestor();
    }

    /// Switches between flat listing and tree, keeping the cursor entry.
    pub fn set_tree(&mut self, tree: bool) -> Option<String> {
        self.tree = tree;
        self.load_or_ancestor()
    }

    /// Expands or collapses the directory at `path` (tree mode only).
    pub fn toggle_expanded(&mut self, path: &Path) -> Option<String> {
        if !self.tree {
            return None;
        }
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
        self.load_or_ancestor()
    }

    /// Right arrow: expand the cursor directory, or step into its first child.
    pub fn expand(&mut self) -> Option<String> {
        let row = self.current().filter(|r| r.is_dir && !r.is_parent)?;
        if !self.tree {
            return None;
        }
        if self.expanded.contains(&row.path) {
            let depth = row.depth;
            if self
                .entries
                .get(self.cursor + 1)
                .is_some_and(|r| r.depth > depth)
            {
                self.cursor += 1;
            }
            return None;
        }
        let path = row.path.clone();
        self.toggle_expanded(&path)
    }

    /// Left arrow: collapse the cursor directory, or jump to its parent row.
    pub fn collapse(&mut self) -> Option<String> {
        let row = self.current().filter(|_| self.tree)?;
        if self.expanded.contains(&row.path) {
            let path = row.path.clone();
            return self.toggle_expanded(&path);
        }
        if row.depth > 0 {
            let parent = row.path.parent().map(Path::to_path_buf);
            self.select_path(parent.as_deref());
        }
        None
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Number of rows before filtering (excluding "..").
    pub fn unfiltered_count(&self) -> usize {
        self.all.iter().filter(|r| !r.is_parent).count()
    }

    /// Shows only rows whose name contains `filter` (case-insensitive); ".."
    /// stays. Keeps the cursor on the same entry if it is still shown, else
    /// moves it to the first match.
    pub fn set_filter(&mut self, filter: &str) {
        let selected = self.current().map(|e| e.path.clone());
        self.filter = filter.to_string();
        self.apply_filter();
        if !self.select_path(selected.as_deref()) {
            let first = self.entries.iter().position(|r| !r.is_parent);
            self.cursor = first.unwrap_or(0);
        }
    }

    pub(super) fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.entries = self
            .all
            .iter()
            .filter(|r| r.is_parent || needle.is_empty() || r.name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
    }

    pub fn set_show_hidden(&mut self, show_hidden: bool) -> Option<String> {
        self.end_archive_load(false);
        self.show_hidden = show_hidden;
        self.load_or_ancestor()
    }
}

#[cfg(test)]
mod tests {

    use crate::fs_ops::SortKey;
    use crate::model::Sort;
    use crate::model::test_support::{fixture, loaded, names};
    use std::fs;

    #[test]
    fn sort_keeps_cursor_on_entry() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.set_cursor(2); // a.txt
        folder.set_sort(Sort {
            key: SortKey::Size,
            descending: true,
        });
        assert_eq!(names(&folder), ["..", "sub", "b.txt", "a.txt"]);
        assert_eq!(folder.current().unwrap().name, "a.txt");
    }

    #[test]
    fn tree_expand_collapse() {
        let dir = fixture();
        fs::write(dir.path().join("sub").join("inner.txt"), "").unwrap();
        let mut folder = loaded(&dir);
        folder.set_cursor(1); // sub
        assert_eq!(folder.expand(), None);
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]); // flat: no-op

        folder.set_tree(true);
        assert_eq!(folder.current().unwrap().name, "sub");
        folder.expand();
        assert_eq!(names(&folder), ["..", "sub", "inner.txt", "a.txt", "b.txt"]);
        assert_eq!(folder.entries()[2].depth, 1);
        assert_eq!(folder.file_count(), 3);

        folder.expand(); // already expanded: step into first child
        assert_eq!(folder.current().unwrap().name, "inner.txt");
        folder.collapse(); // on a child: jump to parent row
        assert_eq!(folder.current().unwrap().name, "sub");
        folder.collapse();
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]);

        // Expansion survives toggling tree mode off and on.
        folder.toggle_expanded(&dir.path().join("sub"));
        folder.set_tree(false);
        assert_eq!(folder.entries().len(), 4);
        folder.set_tree(true);
        assert_eq!(folder.entries().len(), 5);
    }

    #[test]
    fn filter_narrows_rows_until_directory_changes() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.set_cursor(3); // b.txt
        folder.set_filter("B.T");
        assert_eq!(names(&folder), ["..", "b.txt"]);
        assert_eq!(folder.current().unwrap().name, "b.txt");
        assert_eq!(folder.unfiltered_count(), 3);

        folder.set_filter("su"); // cursor entry hidden: first match
        assert_eq!(names(&folder), ["..", "sub"]);
        assert_eq!(folder.current().unwrap().name, "sub");

        folder.load().unwrap(); // reload keeps the filter
        assert_eq!(names(&folder), ["..", "sub"]);
        folder.activate().unwrap(); // entering a directory clears it
        assert_eq!(folder.filter(), "");

        folder.go_up().unwrap();
        folder.set_filter("nothing");
        assert_eq!(names(&folder), [".."]);
        folder.set_filter("");
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]);
    }
}

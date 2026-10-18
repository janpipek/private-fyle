//! Sorting, tree expansion, the name filter and hidden files.

use super::Folder;
use crate::entry::Sort;
use crate::location::Location;

impl Folder {
    /// Re-sorts, keeping the cursor on the same entry.
    pub fn set_sort(&mut self, sort: Sort) {
        let selected = self.current().map(|row| row.location.clone());
        self.sort = sort;
        self.sort_cached();
        self.select_location(selected.as_ref());
    }

    /// Switches between flat listing and tree, keeping the cursor entry.
    pub fn set_tree(&mut self, tree: bool) -> Option<String> {
        self.tree = tree;
        None
    }

    /// Expands or collapses the directory at `path` (tree mode only).
    pub fn toggle_expanded(&mut self, location: &Location) -> Option<String> {
        if !self.tree {
            return None;
        }
        if !self.expanded.remove(location) {
            self.expanded.insert(location.clone());
        }
        None
    }

    /// Right arrow: expand the cursor directory, or step into its first child.
    pub fn expand(&mut self) -> Option<String> {
        let row = self.current().filter(|r| r.is_dir && !r.is_parent)?;
        if !self.tree {
            return None;
        }
        if self.expanded.contains(&row.location) {
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
        let location = row.location.clone();
        self.toggle_expanded(&location)
    }

    /// Left arrow: collapse the cursor directory, or jump to its parent row.
    pub fn collapse(&mut self) -> Option<String> {
        let row = self.current().filter(|_| self.tree)?;
        if self.expanded.contains(&row.location) {
            let location = row.location.clone();
            return self.toggle_expanded(&location);
        }
        if row.depth > 0
            && let Some(parent) = self.all.iter().find(|candidate| {
                candidate.depth + 1 == row.depth
                    && row
                        .location
                        .as_local_path()
                        .is_some_and(|path| path.parent() == candidate.location.as_local_path())
            })
        {
            let location = parent.location.clone();
            self.select_location(Some(&location));
        }
        None
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Number of rows before filtering (excluding "..").
    pub fn unfiltered_count(&self) -> usize {
        let mut hidden_depth = None;
        let mut count = 0;
        for row in &self.all {
            if row.is_parent {
                continue;
            }
            if hidden_depth.is_some_and(|depth| row.depth > depth) {
                continue;
            }
            hidden_depth = None;
            if !self.show_hidden && row.name.starts_with('.') {
                hidden_depth = Some(row.depth);
            } else {
                count += 1;
            }
        }
        count
    }

    /// Shows only rows whose name contains `filter` (case-insensitive); ".."
    /// stays. Keeps the cursor on the same entry if it is still shown, else
    /// moves it to the first match.
    pub fn set_filter(&mut self, filter: &str) {
        let selected = self.current().map(|row| row.location.clone());
        self.filter = filter.to_string();
        self.apply_filter();
        if !self.select_location(selected.as_ref()) {
            let first = self.entries.iter().position(|r| !r.is_parent);
            self.cursor = first.unwrap_or(0);
        }
    }

    pub fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        let mut hidden_depth = None;
        self.entries.clear();
        for row in &self.all {
            if row.is_parent {
                self.entries.push(row.clone());
                continue;
            }
            if hidden_depth.is_some_and(|depth| row.depth > depth) {
                continue;
            }
            hidden_depth = None;
            if !self.show_hidden && row.name.starts_with('.') {
                hidden_depth = Some(row.depth);
                continue;
            }
            if (needle.is_empty() || row.name.to_lowercase().contains(&needle))
                && self.column_filter.accepts(&row.location)
            {
                self.entries.push(row.clone());
            }
        }
    }

    pub fn set_show_hidden(&mut self, show_hidden: bool) -> Option<String> {
        let selected = self.current().map(|row| row.location.clone());
        self.show_hidden = show_hidden;
        self.apply_filter();
        self.select_location(selected.as_ref());
        None
    }
}

#[cfg(test)]
mod tests {

    use crate::column_filter::{ColumnRule, MatchMode, RowOutcome, ScanIdentity};
    use crate::columns::{ColumnId, ColumnKind};
    use crate::entry::Sort;
    use crate::entry::SortKey;
    use crate::location::Location;
    use crate::model::test_support::{fixture, loaded, names, reload};
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
    fn column_filter_and_name_filter_keep_parent_and_restore_cursor() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.set_cursor(2);
        folder.set_filter(".txt");
        let name = ColumnId::from(ColumnKind::Name);
        folder.set_column_filters(vec![ColumnRule {
            column: name.clone(),
            mode: MatchMode::Contains("a.txt".into()),
        }]);
        assert_eq!(names(&folder), [".."]);
        let identity: ScanIdentity = folder.column_filter_identity();
        let rows = folder
            .all_entries()
            .iter()
            .map(|row| {
                let outcome = if row.is_parent || row.name == "a.txt" {
                    RowOutcome::Match
                } else {
                    RowOutcome::NoMatch
                };
                (row.location.clone(), outcome)
            })
            .collect();
        assert!(folder.apply_column_filter_batch(identity, rows));
        assert_eq!(names(&folder), ["..", "a.txt"]);
        assert_eq!(folder.current().map(|row| row.name.as_str()), Some("a.txt"));
    }

    #[test]
    fn column_filter_is_rowwise_for_directories_and_keeps_parent() {
        let dir = fixture();
        fs::write(dir.path().join("sub").join("inner.txt"), "").unwrap();
        let mut folder = loaded(&dir);
        folder.set_tree(true);
        reload(&mut folder);
        folder.toggle_expanded(&Location::local(dir.path().join("sub")).unwrap());
        reload(&mut folder);
        let name = ColumnId::from(ColumnKind::Name);
        folder.set_column_filters(vec![ColumnRule {
            column: name.clone(),
            mode: MatchMode::Contains("inner".into()),
        }]);
        let rows = folder
            .all_entries()
            .iter()
            .map(|row| {
                (
                    row.location.clone(),
                    if row.name == "inner.txt" || row.is_parent {
                        RowOutcome::Match
                    } else {
                        RowOutcome::NoMatch
                    },
                )
            })
            .collect();
        let identity = folder.column_filter_identity();
        assert!(folder.apply_column_filter_batch(identity, rows));
        assert_eq!(names(&folder), ["..", "inner.txt"]);
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
        reload(&mut folder);
        assert_eq!(folder.current().unwrap().name, "sub");
        folder.expand();
        reload(&mut folder);
        assert_eq!(names(&folder), ["..", "sub", "inner.txt", "a.txt", "b.txt"]);
        assert_eq!(folder.entries()[2].depth, 1);
        assert_eq!(folder.file_count(), 3);

        folder.expand(); // already expanded: step into first child
        assert_eq!(folder.current().unwrap().name, "inner.txt");
        folder.collapse(); // on a child: jump to parent row
        assert_eq!(folder.current().unwrap().name, "sub");
        folder.collapse();
        reload(&mut folder);
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]);

        // Expansion survives toggling tree mode off and on.
        folder.toggle_expanded(&Location::local(dir.path().join("sub")).unwrap());
        folder.set_tree(false);
        reload(&mut folder);
        assert_eq!(folder.entries().len(), 4);
        folder.set_tree(true);
        reload(&mut folder);
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

    #[test]
    fn hidden_filter_hides_and_restores_cached_tree_subtrees() {
        let dir = fixture();
        let hidden = dir.path().join(".secret");
        fs::create_dir(&hidden).unwrap();
        fs::write(hidden.join("inside.txt"), "").unwrap();
        let mut folder = loaded(&dir);
        folder.set_tree(true);
        folder.set_show_hidden(true);
        reload(&mut folder);
        folder.toggle_expanded(&Location::local(hidden).unwrap());
        reload(&mut folder);
        assert!(names(&folder).contains(&"inside.txt"));

        folder.set_show_hidden(false);
        assert!(!names(&folder).contains(&".secret"));
        assert!(!names(&folder).contains(&"inside.txt"));
        folder.set_show_hidden(true);
        assert!(names(&folder).contains(&"inside.txt"));
    }
}

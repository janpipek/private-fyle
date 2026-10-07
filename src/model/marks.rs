//! Marking rows, and the targets operations act on.

use std::path::PathBuf;

use super::Folder;

impl Folder {
    /// Marked entries, or the entry under the cursor when nothing is marked.
    pub fn targets(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            return self.marked.iter().cloned().collect();
        }
        self.current()
            .filter(|e| !e.is_parent)
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default()
    }

    /// Target a context-menu row, preserving a marked selection only when
    /// the clicked entry belongs to it. Invalid rows leave selection alone.
    pub fn select_context_row(&mut self, ix: usize) {
        let Some(row) = self.entries.get(ix) else {
            return;
        };
        if !self.marked.contains(&row.path) {
            self.marked.clear();
        }
        self.set_cursor(ix);
    }

    /// Toggles the mark on the cursor entry and advances the cursor.
    pub fn toggle_mark(&mut self) {
        let Some(entry) = self.current().filter(|e| !e.is_parent) else {
            return;
        };
        let path = entry.path.clone();
        if !self.marked.remove(&path) {
            self.marked.insert(path);
        }
        self.set_cursor(self.cursor + 1);
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Paths of the shown entries that can be marked (not "..").
    pub fn markable(&self) -> impl Iterator<Item = &PathBuf> {
        self.entries
            .iter()
            .filter(|e| !e.is_parent)
            .map(|e| &e.path)
    }

    /// Marks every shown entry (the filter limits what is shown).
    pub fn mark_all(&mut self) {
        let paths: Vec<_> = self.markable().cloned().collect();
        self.marked.extend(paths);
    }

    /// Marks the unmarked shown entries and unmarks the marked ones.
    pub fn invert_marks(&mut self) {
        let paths: Vec<_> = self.markable().cloned().collect();
        for path in paths {
            if !self.marked.remove(&path) {
                self.marked.insert(path);
            }
        }
    }

    /// Marks the shown entries whose name matches `pattern` (a glob if it
    /// has `*`, `?` or `[`, else a substring; case-insensitive). Returns
    /// how many matched.
    pub fn mark_matching(&mut self, pattern: &str) -> Result<usize, String> {
        let matcher = crate::search::NameMatcher::new(pattern)?;
        let paths: Vec<_> = self
            .entries
            .iter()
            .filter(|e| !e.is_parent)
            // Search results show relative paths; match the file name.
            .filter(|e| matcher.matches(e.name.rsplit('/').next().unwrap_or(&e.name)))
            .map(|e| e.path.clone())
            .collect();
        let count = paths.len();
        self.marked.extend(paths);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {

    use crate::model::Folder;
    use crate::model::test_support::{fixture, loaded};
    use std::fs;
    use std::path::PathBuf;
    #[test]
    fn marks_and_targets() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.toggle_mark(); // ".." cannot be marked
        assert_eq!(folder.marked_count(), 0);
        assert_eq!(folder.targets(), Vec::<PathBuf>::new());

        folder.set_cursor(2);
        folder.toggle_mark();
        folder.toggle_mark();
        assert_eq!(folder.marked_count(), 2);
        assert_eq!(folder.current().unwrap().name, "b.txt"); // stays on last row
        assert_eq!(
            folder.targets(),
            [dir.path().join("a.txt"), dir.path().join("b.txt")]
        );

        // Reload keeps marks that still exist.
        fs::remove_file(dir.path().join("a.txt")).unwrap();
        folder.load().unwrap();
        assert_eq!(folder.targets(), [dir.path().join("b.txt")]);

        folder.clear_marks();
        assert_eq!(folder.targets(), [dir.path().join("b.txt")]); // cursor entry
    }

    #[test]
    fn context_click_targets_clicked_entry_or_its_marked_group() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.set_cursor(2);
        folder.toggle_mark();
        folder.toggle_mark();
        let marked = vec![dir.path().join("a.txt"), dir.path().join("b.txt")];

        folder.select_context_row(2);
        assert_eq!(folder.current().unwrap().name, "a.txt");
        assert_eq!(folder.targets(), marked);
        folder.select_context_row(usize::MAX);
        assert_eq!(folder.current().unwrap().name, "a.txt");
        assert_eq!(folder.targets(), marked);

        folder.select_context_row(1);
        assert_eq!(folder.marked_count(), 0);
        assert_eq!(folder.targets(), [dir.path().join("sub")]);
        folder.mark_all();
        folder.select_context_row(0); // Parent is never a file-operation target.
        assert_eq!(folder.marked_count(), 0);
        assert!(folder.targets().is_empty());
        folder.select_context_row(3);
        assert_eq!(folder.targets(), [dir.path().join("b.txt")]);
    }

    #[test]
    fn mark_all_invert_and_by_pattern() {
        let dir = fixture();
        let mut folder = loaded(&dir); // .., sub, a.txt, b.txt
        let marked = |f: &Folder| {
            let mut names: Vec<_> = f
                .entries()
                .iter()
                .filter(|e| f.is_marked(&e.path))
                .map(|e| e.name.clone())
                .collect();
            names.sort();
            names
        };
        folder.mark_all();
        assert_eq!(marked(&folder), ["a.txt", "b.txt", "sub"]);
        folder.set_cursor(2);
        folder.toggle_mark(); // unmark a.txt
        folder.invert_marks();
        assert_eq!(marked(&folder), ["a.txt"]);

        folder.clear_marks();
        assert_eq!(folder.mark_matching("*.TXT"), Ok(2));
        assert_eq!(marked(&folder), ["a.txt", "b.txt"]);
        folder.clear_marks();
        assert_eq!(folder.mark_matching("su"), Ok(1)); // substring
        assert_eq!(marked(&folder), ["sub"]);
        assert!(folder.mark_matching("[").is_err());

        // Only what the filter shows.
        folder.clear_marks();
        folder.set_filter("a");
        folder.mark_all();
        assert_eq!(marked(&folder), ["a.txt"]);
    }
}

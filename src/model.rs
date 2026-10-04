//! UI-independent state of a folder panel. Views render a [`Folder`] and
//! translate input into calls on it; all navigation, cursor, marking,
//! sorting and tree expansion rules live here.

use std::collections::BTreeSet;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use crate::fs_ops::{self, Entry, SortKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            descending: false,
        }
    }
}

/// Result of activating (Enter / double-click) the entry under the cursor.
#[derive(Debug, PartialEq, Eq)]
pub enum Activation {
    Entered,
    OpenFile(PathBuf),
    Nothing,
}

/// A listed entry and its nesting depth (always 0 outside tree mode).
#[derive(Clone, Debug)]
pub struct Row {
    pub entry: Entry,
    pub depth: usize,
}

impl Deref for Row {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        &self.entry
    }
}

pub struct Folder {
    path: PathBuf,
    /// Rows shown: `all` narrowed by `filter`.
    entries: Vec<Row>,
    /// Everything listed, before filtering.
    all: Vec<Row>,
    /// Case-insensitive substring the shown names must contain.
    filter: String,
    /// Set while showing search results instead of a directory listing;
    /// `path` is then the search root.
    results: Option<String>,
    cursor: usize,
    marked: BTreeSet<PathBuf>,
    sort: Sort,
    show_hidden: bool,
    tree: bool,
    expanded: BTreeSet<PathBuf>,
}

impl Folder {
    /// Creates an unloaded folder; call [`Folder::load_or_ancestor`] next.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            all: Vec::new(),
            filter: String::new(),
            results: None,
            cursor: 0,
            marked: BTreeSet::new(),
            sort: Sort::default(),
            show_hidden: false,
            tree: false,
            expanded: BTreeSet::new(),
        }
    }

    /// Initial options, applied by the first load.
    pub fn with_options(mut self, show_hidden: bool, tree: bool, sort: Sort) -> Self {
        self.show_hidden = show_hidden;
        self.tree = tree;
        self.sort = sort;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn entries(&self) -> &[Row] {
        &self.entries
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn current(&self) -> Option<&Row> {
        self.entries.get(self.cursor)
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    pub fn is_tree(&self) -> bool {
        self.tree
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.tree && self.expanded.contains(path)
    }

    pub fn is_marked(&self, path: &Path) -> bool {
        self.marked.contains(path)
    }

    pub fn marked_count(&self) -> usize {
        self.marked.len()
    }

    pub fn dir_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.is_dir && !e.is_parent)
            .count()
    }

    pub fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_dir).count()
    }

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

    /// Returns whether the cursor moved.
    pub fn set_cursor(&mut self, ix: usize) -> bool {
        let ix = ix.min(self.entries.len().saturating_sub(1));
        let changed = ix != self.cursor;
        self.cursor = ix;
        changed
    }

    /// Title of the search whose results are shown, if any.
    pub fn results(&self) -> Option<&str> {
        self.results.as_deref()
    }

    /// Switches to showing search results under `root` (initially none;
    /// see [`Folder::add_results`]). ".." leads back to `root`.
    pub fn show_results(&mut self, root: PathBuf, title: String) {
        self.all = vec![Row {
            entry: Entry {
                name: "..".into(),
                path: root.clone(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
            },
            depth: 0,
        }];
        self.path = root;
        self.results = Some(title);
        self.marked.clear();
        self.filter.clear();
        self.cursor = 0;
        self.apply_filter();
    }

    /// Appends search hits, keeping the cursor on its entry.
    pub fn add_results(&mut self, entries: Vec<Entry>) {
        let selected = self.current().map(|e| e.path.clone());
        self.all
            .extend(entries.into_iter().map(|entry| Row { entry, depth: 0 }));
        self.apply_filter();
        self.select_path(selected.as_deref());
    }

    /// Re-reads the current directory, keeping cursor entry and marks.
    /// Search results are not re-run; entries that disappeared are dropped.
    pub fn load(&mut self) -> Result<(), String> {
        if self.results.is_some() {
            let selected = self.current().map(|e| e.path.clone());
            self.all.retain(|r| r.is_parent || r.path.exists());
            self.marked.retain(|p| p.exists());
            self.apply_filter();
            if !self.select_path(selected.as_deref()) {
                self.set_cursor(self.cursor);
            }
            return Ok(());
        }
        let selected = self.current().map(|e| e.path.clone());
        self.replace(self.path.clone(), selected)
    }

    /// Loads the current directory, or else its nearest readable ancestor.
    /// Returns the first error, if any.
    pub fn load_or_ancestor(&mut self) -> Option<String> {
        let mut first_error = None;
        while let Err(e) = self.load() {
            first_error.get_or_insert(e);
            match fs_ops::parent_of(&self.path) {
                Some(parent) => self.path = parent,
                None => break,
            }
        }
        first_error
    }

    /// Changes directory, putting the cursor on the directory we came from.
    pub fn navigate(&mut self, path: PathBuf) -> Result<(), String> {
        let came_from = self.path.clone();
        self.replace(path, Some(came_from))
    }

    pub fn go_up(&mut self) -> Result<(), String> {
        if self.results.is_some() {
            return self.replace(self.path.clone(), None);
        }
        match fs_ops::parent_of(&self.path) {
            Some(parent) => self.navigate(parent),
            None => Ok(()),
        }
    }

    pub fn activate(&mut self) -> Result<Activation, String> {
        let Some(entry) = self.current().map(|r| r.entry.clone()) else {
            return Ok(Activation::Nothing);
        };
        if entry.is_dir {
            if self.results.is_some() && entry.is_parent {
                self.replace(entry.path, None)?;
            } else {
                self.navigate(entry.path)?;
            }
            Ok(Activation::Entered)
        } else if self.results.is_some() {
            // A search hit: show it in its directory.
            let parent = entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            self.replace(parent, Some(entry.path))?;
            Ok(Activation::Entered)
        } else {
            Ok(Activation::OpenFile(entry.path))
        }
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

    /// Re-sorts, keeping the cursor on the same entry.
    pub fn set_sort(&mut self, sort: Sort) {
        self.sort = sort;
        if self.results.is_some() {
            let selected = self.current().map(|e| e.path.clone());
            let mut entries: Vec<Entry> = self.all.drain(..).map(|r| r.entry).collect();
            fs_ops::sort_entries(&mut entries, sort.key, sort.descending);
            self.all = entries
                .into_iter()
                .map(|entry| Row { entry, depth: 0 })
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
        let Some(row) = self.current().filter(|r| r.is_dir && !r.is_parent) else {
            return None;
        };
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
        let Some(row) = self.current().filter(|_| self.tree) else {
            return None;
        };
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

    fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.entries = self
            .all
            .iter()
            .filter(|r| r.is_parent || needle.is_empty() || r.name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
    }

    pub fn set_show_hidden(&mut self, show_hidden: bool) -> Option<String> {
        self.show_hidden = show_hidden;
        self.load_or_ancestor()
    }

    fn replace(&mut self, path: PathBuf, select: Option<PathBuf>) -> Result<(), String> {
        let mut entries = Vec::new();
        self.list(&path, 0, &mut entries)
            .map_err(|e| format!("{}: {e}", fs_ops::display_path(&path)))?;
        if path != self.path || self.results.is_some() {
            self.marked.clear();
            self.filter.clear();
        }
        self.results = None;
        self.path = path;
        self.all = entries;
        self.marked
            .retain(|p| self.all.iter().any(|e| &e.path == p));
        self.apply_filter();
        if !self.select_path(select.as_deref()) {
            self.set_cursor(self.cursor);
        }
        Ok(())
    }

    /// Appends the sorted listing of `dir`, recursing into expanded
    /// directories in tree mode. Unreadable subdirectories list as empty.
    fn list(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) -> io::Result<()> {
        let mut entries = fs_ops::read_dir(dir, self.show_hidden)?;
        if depth > 0 {
            entries.retain(|e| !e.is_parent);
        }
        fs_ops::sort_entries(&mut entries, self.sort.key, self.sort.descending);
        for entry in entries {
            let descend = self.tree && !entry.is_parent && self.expanded.contains(&entry.path);
            let path = entry.path.clone();
            out.push(Row { entry, depth });
            if descend {
                let _ = self.list(&path, depth + 1, out);
            }
        }
        Ok(())
    }

    /// Moves the cursor to `path`; returns false if it is not listed.
    pub fn select_path(&mut self, path: Option<&Path>) -> bool {
        let Some(ix) = path.and_then(|p| self.entries.iter().position(|e| e.path == p)) else {
            return false;
        };
        self.cursor = ix;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("fyle-model-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("sub")).unwrap();
            fs::write(dir.join("b.txt"), "12345").unwrap();
            fs::write(dir.join("a.txt"), "1").unwrap();
            fs::write(dir.join(".hidden"), "").unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(folder: &Folder) -> Vec<&str> {
        folder.entries().iter().map(|e| e.name.as_str()).collect()
    }

    fn loaded(dir: &TempDir) -> Folder {
        let mut folder = Folder::new(dir.0.clone());
        assert_eq!(folder.load_or_ancestor(), None);
        folder
    }

    #[test]
    fn lists_sorted_without_hidden() {
        let dir = TempDir::new("list");
        let mut folder = loaded(&dir);
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]);
        assert_eq!((folder.dir_count(), folder.file_count()), (1, 2));

        folder.set_show_hidden(true);
        assert_eq!(names(&folder), ["..", "sub", ".hidden", "a.txt", "b.txt"]);
    }

    #[test]
    fn sort_keeps_cursor_on_entry() {
        let dir = TempDir::new("sort");
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
    fn enter_and_go_up_restore_cursor() {
        let dir = TempDir::new("nav");
        let mut folder = loaded(&dir);
        folder.set_cursor(1);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.path(), dir.0.join("sub"));
        assert_eq!(names(&folder), [".."]);

        folder.go_up().unwrap();
        assert_eq!(folder.path(), dir.0);
        assert_eq!(folder.current().unwrap().name, "sub");

        folder.set_cursor(2);
        assert_eq!(
            folder.activate(),
            Ok(Activation::OpenFile(dir.0.join("a.txt")))
        );
    }

    #[test]
    fn marks_and_targets() {
        let dir = TempDir::new("marks");
        let mut folder = loaded(&dir);
        folder.toggle_mark(); // ".." cannot be marked
        assert_eq!(folder.marked_count(), 0);
        assert_eq!(folder.targets(), Vec::<PathBuf>::new());

        folder.set_cursor(2);
        folder.toggle_mark();
        folder.toggle_mark();
        assert_eq!(folder.marked_count(), 2);
        assert_eq!(folder.current().unwrap().name, "b.txt"); // stays on last row
        assert_eq!(folder.targets(), [dir.0.join("a.txt"), dir.0.join("b.txt")]);

        // Reload keeps marks that still exist.
        fs::remove_file(dir.0.join("a.txt")).unwrap();
        folder.load().unwrap();
        assert_eq!(folder.targets(), [dir.0.join("b.txt")]);

        folder.clear_marks();
        assert_eq!(folder.targets(), [dir.0.join("b.txt")]); // cursor entry
    }

    #[test]
    fn tree_expand_collapse() {
        let dir = TempDir::new("tree");
        fs::write(dir.0.join("sub").join("inner.txt"), "").unwrap();
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
        folder.toggle_expanded(&dir.0.join("sub"));
        folder.set_tree(false);
        assert_eq!(folder.entries().len(), 4);
        folder.set_tree(true);
        assert_eq!(folder.entries().len(), 5);
    }

    #[test]
    fn filter_narrows_rows_until_directory_changes() {
        let dir = TempDir::new("filter");
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
    fn search_results_mode() {
        let dir = TempDir::new("results");
        fs::write(dir.0.join("sub").join("deep.txt"), "").unwrap();
        let mut folder = loaded(&dir);
        let entry = |rel: &str| {
            let path = dir.0.join(rel);
            Entry {
                name: rel.into(),
                is_dir: path.is_dir(),
                path,
                is_parent: false,
                size: 0,
                modified: None,
            }
        };

        folder.show_results(dir.0.clone(), "search".into());
        assert_eq!(folder.results(), Some("search"));
        assert_eq!(names(&folder), [".."]);
        folder.add_results(vec![entry("sub/deep.txt"), entry("b.txt")]);
        folder.add_results(vec![entry("a.txt")]);
        assert_eq!(names(&folder), ["..", "sub/deep.txt", "b.txt", "a.txt"]);

        folder.set_sort(Sort::default()); // sorts in place, no re-listing
        assert_eq!(names(&folder), ["..", "a.txt", "b.txt", "sub/deep.txt"]);

        fs::remove_file(dir.0.join("b.txt")).unwrap();
        folder.load().unwrap(); // drops vanished hits
        assert_eq!(names(&folder), ["..", "a.txt", "sub/deep.txt"]);

        // Enter on a hit shows it in its directory.
        folder.set_cursor(2);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.results(), None);
        assert_eq!(folder.path(), dir.0.join("sub"));
        assert_eq!(folder.current().unwrap().name, "deep.txt");

        // ".." / go up from results returns to the search root.
        folder.show_results(dir.0.clone(), "again".into());
        folder.go_up().unwrap();
        assert_eq!((folder.results(), folder.path()), (None, dir.0.as_path()));
        assert_eq!(names(&folder), ["..", "sub", "a.txt"]);
    }

    #[test]
    fn missing_directory_falls_back_to_ancestor() {
        let dir = TempDir::new("missing");
        let mut folder = Folder::new(dir.0.join("gone").join("deeper"));
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), dir.0);
    }
}

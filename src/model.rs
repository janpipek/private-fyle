//! UI-independent state of a folder panel. Views render a [`Folder`] and
//! translate input into calls on it; all navigation, cursor, marking,
//! sorting and tree expansion rules live here.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

mod archives;
mod history;
mod marks;
mod remote;
mod results;
mod sizes;
#[cfg(test)]
mod test_support;
mod view;

use archives::Mount;
use history::Visit;

use crate::archive;
use crate::dirsize::DirSize;
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
    /// For a content search hit: the first matching line (1-based) and its text.
    pub hit_line: Option<(u64, String)>,
}

impl Deref for Row {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        &self.entry
    }
}

#[derive(Clone)]
pub struct Folder {
    path: PathBuf,
    /// Remote listings loaded in the background; remote rows are built from
    /// them, never from I/O.
    remote_listings: remote::ListingCache,
    /// Local folder the panel came from, where ".." at a remote root returns.
    remote_origin: Option<PathBuf>,
    /// Rows shown: `all` narrowed by `filter`.
    entries: Vec<Row>,
    /// Everything listed, before filtering.
    all: Vec<Row>,
    /// Case-insensitive substring the shown names must contain.
    filter: String,
    /// Set while showing search results instead of a directory listing;
    /// `path` is then the search root.
    results: Option<String>,
    /// Set while browsing inside an archive; `path` is then the archive
    /// file joined with the inner path.
    mount: Option<Mount>,
    cursor: usize,
    marked: BTreeSet<PathBuf>,
    sort: Sort,
    show_hidden: bool,
    tree: bool,
    expanded: BTreeSet<PathBuf>,
    /// Directory sizes computed on demand, by path. Kept across re-sorts
    /// and filters; dropped on navigation and [`Folder::refresh`].
    sizes: HashMap<PathBuf, DirSize>,
    /// Bumped whenever `sizes` is cleared, so a scan started earlier can
    /// tell its results are stale.
    size_epoch: u64,
    /// Bumped whenever the listing is (re)read, so views can drop caches.
    generation: u64,
    /// Directories visited before / after (Back, Forward), with the entry
    /// the cursor was on, most recent last.
    back: Vec<Visit>,
    forward: Vec<Visit>,
    /// Set while an archive is being read in the background: the folder
    /// as it was before, to return to when the read ends.
    loading: Option<Box<Folder>>,
}

impl Folder {
    /// Creates an unloaded folder; call [`Folder::load_or_ancestor`] next.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            remote_listings: Default::default(),
            remote_origin: None,
            entries: Vec::new(),
            all: Vec::new(),
            filter: String::new(),
            results: None,
            mount: None,
            cursor: 0,
            marked: BTreeSet::new(),
            sort: Sort::default(),
            show_hidden: false,
            tree: false,
            expanded: BTreeSet::new(),
            sizes: HashMap::new(),
            size_epoch: 0,
            generation: 0,
            back: Vec::new(),
            forward: Vec::new(),
            loading: None,
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

    /// Changes whenever the listing is read again (see [`Folder::load`]).
    pub fn generation(&self) -> u64 {
        self.generation
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
        self.tree && self.mount.is_none() && self.expanded.contains(path)
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

    /// Returns whether the cursor moved.
    pub fn set_cursor(&mut self, ix: usize) -> bool {
        let ix = ix.min(self.entries.len().saturating_sub(1));
        let changed = ix != self.cursor;
        self.cursor = ix;
        changed
    }

    /// Re-reads the current directory, keeping cursor entry and marks.
    /// Search results are not re-run; entries that disappeared are dropped.
    /// While an archive is being read it does nothing (the placeholder stays).
    pub fn load(&mut self) -> Result<(), String> {
        if self.is_loading() {
            return Ok(());
        }
        if self.results.is_some() {
            self.generation += 1;
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

    /// Like [`Folder::load`], but also forgets computed directory sizes,
    /// which may be stale.
    pub fn refresh(&mut self) -> Option<String> {
        self.clear_sizes();
        self.load_or_ancestor()
    }

    /// Loads the current directory, or else its nearest readable ancestor.
    /// Returns the first error, if any.
    pub fn load_or_ancestor(&mut self) -> Option<String> {
        let mut first_error = None;
        while let Err(e) = self.load() {
            first_error.get_or_insert(e);
            if self.is_remote() {
                break;
            }
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

    /// While an archive is being read, going up cancels the read instead.
    pub fn go_up(&mut self) -> Result<(), String> {
        if self.is_loading() {
            self.end_archive_load(true);
            return Ok(());
        }
        if self.results.is_some() {
            return self.replace(self.path.clone(), None);
        }
        match self.parent_location() {
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
        } else if !self.is_remote()
            && self.mount.is_none()
            && archive::format_of(&entry.path).is_some()
        {
            // An archive opens like a directory.
            self.navigate(entry.path)?;
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

    fn replace_untracked(&mut self, path: PathBuf, select: Option<PathBuf>) -> Result<(), String> {
        let mut entries = Vec::new();
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", fs_ops::display_path(&path));
        let mount = if crate::remote::is_remote(&path) {
            None
        } else {
            self.mount_for(&path).map_err(|e| error(&e))?
        };
        if crate::remote::is_remote(&path) {
            self.list_remote(&path, &mut entries)
                .map_err(|e| error(&e))?;
        } else {
            match &mount {
                Some((mount, inner)) => self
                    .list_archive(mount, inner, &mut entries)
                    .map_err(|e| error(&e))?,
                None => self.list(&path, 0, &mut entries).map_err(|e| error(&e))?,
            }
        }
        if path != self.path || self.results.is_some() {
            self.marked.clear();
            self.filter.clear();
            self.clear_sizes();
        }
        self.results = None;
        self.mount = mount.map(|(mount, _)| mount);
        self.path = path;
        self.all = entries;
        self.generation += 1;
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
        self.apply_sizes(&mut entries);
        fs_ops::sort_entries(&mut entries, self.sort.key, self.sort.descending);
        for entry in entries {
            let descend = self.tree && !entry.is_parent && self.expanded.contains(&entry.path);
            let path = entry.path.clone();
            out.push(Row {
                entry,
                depth,
                hit_line: None,
            });
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
    use crate::model::test_support::{fixture, loaded, names};

    #[test]
    fn lists_sorted_without_hidden() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]);
        assert_eq!((folder.dir_count(), folder.file_count()), (1, 2));

        folder.set_show_hidden(true);
        assert_eq!(names(&folder), ["..", "sub", ".hidden", "a.txt", "b.txt"]);
    }

    #[test]
    fn enter_and_go_up_restore_cursor() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.set_cursor(1);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.path(), dir.path().join("sub"));
        assert_eq!(names(&folder), [".."]);

        folder.go_up().unwrap();
        assert_eq!(folder.path(), dir.path());
        assert_eq!(folder.current().unwrap().name, "sub");

        folder.set_cursor(2);
        assert_eq!(
            folder.activate(),
            Ok(Activation::OpenFile(dir.path().join("a.txt")))
        );
    }

    #[test]
    fn missing_directory_falls_back_to_ancestor() {
        let dir = fixture();
        let mut folder = Folder::new(dir.path().join("gone").join("deeper"));
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), dir.path());
    }
}

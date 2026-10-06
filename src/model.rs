//! UI-independent state of a folder panel. Views render a [`Folder`] and
//! translate input into calls on it; all navigation, cursor, marking,
//! sorting and tree expansion rules live here.

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[allow(dead_code)] // not wired into the UI yet; remove once it is
mod remote;

use crate::archive;
use crate::dirsize::DirSize;
use crate::fs_ops::{self, Entry, SortKey};
use crate::links::LinkState;
use crate::search::Hit;

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
    /// Last remote listing loaded in the background, with the panel path it
    /// belongs to; remote rows are built from it, never from I/O.
    remote_listing: Option<(PathBuf, Vec<Entry>)>,
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

/// What [`Folder::marked_total`] knows about the marked entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkedTotal {
    pub bytes: u64,
    /// Marked directories without a computed size (not counted in `bytes`).
    pub unknown_dirs: usize,
    /// Some computed size is a lower bound (running, cancelled or errors).
    pub inexact: bool,
}

impl MarkedTotal {
    /// For the footer, e.g. "1.2 G" or "≥ 1.2 G (2 folders not measured)".
    pub fn label(&self) -> String {
        let size = fs_ops::format_size(self.bytes);
        let bound = if self.inexact || self.unknown_dirs > 0 {
            "≥ "
        } else {
            ""
        };
        match self.unknown_dirs {
            0 => format!("{bound}{size}"),
            1 => format!("{bound}{size} (1 folder not measured)"),
            n => format!("{bound}{size} ({n} folders not measured)"),
        }
    }
}

/// An archive shown as a directory tree.
#[derive(Clone)]
struct Mount {
    file: PathBuf,
    index: Arc<archive::Index>,
}

/// A directory in the Back/Forward history and its cursor entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visit {
    path: PathBuf,
    selected: Option<PathBuf>,
}

/// History entries kept per direction.
const HISTORY_LEN: usize = 100;

fn push_capped(list: &mut Vec<Visit>, visit: Visit) {
    if list.last() != Some(&visit) {
        list.push(visit);
    }
    if list.len() > HISTORY_LEN {
        list.remove(0);
    }
}

impl Folder {
    /// Creates an unloaded folder; call [`Folder::load_or_ancestor`] next.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            remote_listing: None,
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

    /// Directories "Calculate Sizes" measures: the marked ones; else the
    /// one under the cursor; else (cursor on ".." or a file) all listed
    /// directories.
    pub fn size_targets(&self) -> Vec<PathBuf> {
        let dirs = |rows: &mut dyn Iterator<Item = &Row>| {
            rows.filter(|r| r.is_dir && !r.is_parent)
                .map(|r| r.path.clone())
                .collect::<Vec<_>>()
        };
        if !self.marked.is_empty() {
            return dirs(&mut self.all.iter().filter(|r| self.marked.contains(&r.path)));
        }
        match self.current() {
            Some(row) if row.is_dir && !row.is_parent => vec![row.path.clone()],
            _ => dirs(&mut self.entries.iter().filter(|r| r.depth == 0)),
        }
    }

    fn clear_sizes(&mut self) {
        self.sizes.clear();
        self.size_epoch += 1;
    }

    /// Changes whenever computed sizes are forgotten (refresh, navigation).
    pub fn size_epoch(&self) -> u64 {
        self.size_epoch
    }

    /// Like [`Folder::set_dir_size`], but ignored when sizes were forgotten
    /// since `epoch` was read from [`Folder::size_epoch`].
    pub fn set_dir_size_at(&mut self, epoch: u64, path: &Path, size: DirSize) {
        if epoch == self.size_epoch {
            self.set_dir_size(path, size);
        }
    }

    /// The computed size of the directory `path`, if it was measured.
    pub fn dir_size(&self, path: &Path) -> Option<&DirSize> {
        self.sizes.get(path)
    }

    /// Records a (possibly partial) size for a listed directory; ignored
    /// when `path` is not listed any more. Sort order is not changed until
    /// [`Folder::resort_by_size`].
    pub fn set_dir_size(&mut self, path: &Path, size: DirSize) {
        if !self.all.iter().any(|r| r.is_dir && r.path == path) {
            return;
        }
        self.sizes.insert(path.to_path_buf(), size);
        for row in self.all.iter_mut().chain(self.entries.iter_mut()) {
            if row.path == path {
                row.entry.size = size.bytes;
            }
        }
    }

    /// Marks directories whose scan is still running as cancelled.
    pub fn cancel_running_sizes(&mut self) {
        for size in self.sizes.values_mut().filter(|s| s.is_running()) {
            size.state = crate::dirsize::ScanState::Cancelled;
        }
    }

    /// Re-sorts when sorting by size, so computed sizes take effect.
    pub fn resort_by_size(&mut self) {
        if self.sort.key == SortKey::Size {
            self.set_sort(self.sort);
        }
    }

    /// Replaces the size of listed directories by their computed size, so
    /// that sorting by size uses it.
    fn apply_sizes(&self, entries: &mut [Entry]) {
        if self.sizes.is_empty() {
            return;
        }
        for entry in entries.iter_mut().filter(|e| e.is_dir && !e.is_parent) {
            if let Some(size) = self.sizes.get(&entry.path) {
                entry.size = size.bytes;
            }
        }
    }

    /// Total size of the marked entries: files plus measured directories.
    pub fn marked_total(&self) -> MarkedTotal {
        let mut total = MarkedTotal::default();
        // In tree mode a marked row may lie inside a marked, measured
        // directory whose size already includes it.
        let covered = |path: &Path| {
            path.ancestors()
                .skip(1)
                .any(|a| self.marked.contains(a) && self.sizes.contains_key(a))
        };
        for row in self.all.iter().filter(|r| self.marked.contains(&r.path)) {
            if covered(&row.path) {
                continue;
            }
            if !row.is_dir {
                total.bytes += row.size;
            } else if let Some(size) = self.sizes.get(&row.path) {
                total.bytes += size.bytes;
                total.inexact |= !size.is_exact();
            } else {
                total.unknown_dirs += 1;
            }
        }
        total
    }

    /// Returns whether the cursor moved.
    pub fn set_cursor(&mut self, ix: usize) -> bool {
        let ix = ix.min(self.entries.len().saturating_sub(1));
        let changed = ix != self.cursor;
        self.cursor = ix;
        changed
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

    /// The archive file being browsed, if any.
    pub fn archive(&self) -> Option<&Path> {
        self.mount.as_ref().map(|m| m.file.as_path())
    }

    /// The location inside the archive being browsed (empty at its root).
    pub fn inner_path(&self) -> Option<String> {
        let mount = self.mount.as_ref()?;
        archive::inner_of(&mount.file, &self.path)
    }

    /// The archive and the inner paths of [`Folder::targets`], for
    /// extracting them.
    pub fn archive_targets(&self) -> Option<(PathBuf, Vec<String>)> {
        let file = self.archive()?.to_path_buf();
        let inner = self
            .targets()
            .iter()
            .filter_map(|p| archive::inner_of(&file, p))
            .collect();
        Some((file, inner))
    }

    /// The archive file that must be read before `path` can be shown, if
    /// `path` is inside one that is not cached yet (see
    /// [`archive::Index::cached`]). Showing it would block on that read.
    pub fn archive_needed(&self, path: &Path) -> Option<PathBuf> {
        if crate::remote::is_remote(path) || path.as_os_str().is_empty() || path.is_dir() {
            return None;
        }
        let (file, _) = archive::locate(path)?;
        (!archive::Index::is_cached(&file)).then_some(file)
    }

    /// Where [`Folder::activate`] would go: the cursor entry if it is a
    /// folder or an archive.
    pub fn activation_target(&self) -> Option<PathBuf> {
        let entry = self.current()?;
        (entry.is_dir || (self.mount.is_none() && archive::format_of(&entry.path).is_some()))
            .then(|| entry.path.clone())
    }

    /// Where Back (or Forward) would go.
    pub fn history_target(&self, back: bool) -> Option<PathBuf> {
        let list = if back { &self.back } else { &self.forward };
        list.last().map(|v| v.path.clone())
    }

    /// Shows an empty listing of the archive location `target` while its
    /// archive is read elsewhere (see [`Folder::archive_needed`]). The
    /// previous state comes back with [`Folder::end_archive_load`].
    pub fn begin_archive_load(&mut self, target: PathBuf) {
        self.end_archive_load(false);
        let Some(file) = self.archive_needed(&target) else {
            return;
        };
        let saved = Box::new(self.clone());
        self.entries.clear();
        self.all.clear();
        self.marked.clear();
        self.filter.clear();
        self.results = None;
        self.cursor = 0;
        self.mount = Some(Mount {
            file,
            index: Arc::default(),
        });
        self.path = target;
        self.generation += 1;
        self.loading = Some(saved);
    }

    /// Whether an archive is being read for this folder.
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Ends [`Folder::begin_archive_load`], restoring the previous state.
    /// With `leave`, the cursor goes to the archive in its containing
    /// directory (used when the read is cancelled).
    pub fn end_archive_load(&mut self, leave: bool) {
        let Some(saved) = self.loading.take() else {
            return;
        };
        let file = self.archive().map(Path::to_path_buf);
        *self = *saved;
        if let (true, Some(file)) = (leave, file) {
            let parent = fs_ops::parent_of(&file).unwrap_or_default();
            if self.path != parent || self.mount.is_some() {
                // Best effort: the previous state stays if the directory is gone.
                let _ = self.replace(parent, Some(file));
            } else {
                self.select_path(Some(&file));
            }
        }
    }

    /// A compact text form of the panel location, separating an archive file
    /// from its internal path.
    pub fn header(&self) -> String {
        match (self.archive(), self.inner_path()) {
            (Some(file), Some(inner)) => archive::location(file, &inner),
            _ => fs_ops::display_path(&self.path),
        }
    }

    /// Title of the search whose results are shown, if any.
    pub fn results(&self) -> Option<&str> {
        self.results.as_deref()
    }

    /// Switches to showing search results under `root` (initially none;
    /// see [`Folder::add_results`]). ".." leads back to `root`.
    pub fn show_results(&mut self, root: PathBuf, title: String) {
        self.clear_sizes();
        self.all = vec![Row {
            entry: Entry {
                name: "..".into(),
                path: root.clone(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            },
            depth: 0,
            hit_line: None,
        }];
        self.path = root;
        self.mount = None;
        self.results = Some(title);
        self.generation += 1;
        self.marked.clear();
        self.filter.clear();
        self.cursor = 0;
        self.apply_filter();
    }

    /// Appends search hits, keeping the cursor on its entry.
    pub fn add_results(&mut self, hits: Vec<Hit>) {
        let selected = self.current().map(|e| e.path.clone());
        self.all.extend(hits.into_iter().map(|hit| Row {
            entry: hit.entry,
            depth: 0,
            hit_line: hit.line,
        }));
        self.apply_filter();
        self.select_path(selected.as_deref());
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
    fn markable(&self) -> impl Iterator<Item = &PathBuf> {
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
        self.end_archive_load(false);
        self.show_hidden = show_hidden;
        self.load_or_ancestor()
    }

    /// Shows `path`, recording the directory left in the Back history.
    fn replace(&mut self, path: PathBuf, select: Option<PathBuf>) -> Result<(), String> {
        let left = self.visit();
        self.replace_untracked(path, select)?;
        if let Some(visit) = left.filter(|v| v.path != self.path) {
            push_capped(&mut self.back, visit);
            self.forward.clear();
        }
        Ok(())
    }

    /// The directory shown now as a history entry (none for search results).
    fn visit(&self) -> Option<Visit> {
        (self.results.is_none()).then(|| Visit {
            path: self.path.clone(),
            selected: self.current().map(|e| e.path.clone()),
        })
    }

    /// Back: the previous directory, cursor where it was. Returns whether
    /// there was one.
    pub fn go_back(&mut self) -> Result<bool, String> {
        self.travel(true)
    }

    pub fn go_forward(&mut self) -> Result<bool, String> {
        self.travel(false)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// One step back (or forward): the current directory goes onto the
    /// other list. A directory that is gone is dropped from the history.
    fn travel(&mut self, back: bool) -> Result<bool, String> {
        let from = if back {
            &mut self.back
        } else {
            &mut self.forward
        };
        let Some(target) = from.pop() else {
            return Ok(false);
        };
        let here = self.visit();
        self.replace_untracked(target.path, target.selected)?;
        if let Some(here) = here {
            push_capped(
                if back {
                    &mut self.forward
                } else {
                    &mut self.back
                },
                here,
            );
        }
        Ok(true)
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

    /// The archive (and the inner path in it) that `path` is in, if it is
    /// not an ordinary directory. The archive is read again only when it
    /// changed on disk ([`archive::Index::cached`]).
    fn mount_for(&self, path: &Path) -> io::Result<Option<(Mount, String)>> {
        if crate::remote::is_remote(path) || path.as_os_str().is_empty() || path.is_dir() {
            return Ok(None);
        }
        let Some((file, inner)) = archive::locate(path) else {
            return Ok(None);
        };
        let index = archive::Index::cached(&file)?;
        let mount = Mount { file, index };
        Ok(Some((mount, inner)))
    }

    /// Appends the sorted listing of the archive directory `inner`, with
    /// ".." leading to the parent (the containing directory at the root).
    fn list_archive(&self, mount: &Mount, inner: &str, out: &mut Vec<Row>) -> io::Result<()> {
        if !mount.index.is_dir(inner) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "not a folder in the archive",
            ));
        }
        let parent = match inner.rsplit_once('/') {
            Some((parent, _)) => Some(archive::virtual_path(&mount.file, parent)),
            None if inner.is_empty() => fs_ops::parent_of(&mount.file),
            None => Some(mount.file.clone()),
        };
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
            mount
                .index
                .list(inner)
                .filter(|node| self.show_hidden || !node.name.starts_with('.'))
                .map(|node| Entry {
                    name: node.name.clone(),
                    path: archive::virtual_path(&mount.file, &node.path),
                    is_dir: node.is_dir(),
                    is_parent: false,
                    size: node.size,
                    modified: node.modified,
                    attrs: fs_ops::Attrs {
                        mode: node.mode,
                        ..Default::default()
                    },
                    link: match &node.kind {
                        archive::Kind::Symlink(target) => LinkState::Link {
                            target: target.into(),
                        },
                        _ => LinkState::NotLink,
                    },
                }),
        );
        fs_ops::sort_entries(&mut entries, self.sort.key, self.sort.descending);
        out.extend(entries.into_iter().map(|entry| Row {
            entry,
            depth: 0,
            hit_line: None,
        }));
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
    use std::fs;
    use tempfile::TempDir;

    /// A scratch folder with `sub/`, `a.txt`, `b.txt` and `.hidden`.
    fn fixture() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("b.txt"), "12345").unwrap();
        fs::write(dir.path().join("a.txt"), "1").unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        dir
    }

    fn names(folder: &Folder) -> Vec<&str> {
        folder.entries().iter().map(|e| e.name.as_str()).collect()
    }

    fn loaded(dir: &TempDir) -> Folder {
        let mut folder = Folder::new(dir.path().to_path_buf());
        assert_eq!(folder.load_or_ancestor(), None);
        folder
    }

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
    fn back_and_forward() {
        let dir = fixture();
        fs::create_dir(dir.path().join("sub").join("deep")).unwrap();
        let mut folder = loaded(&dir);
        let sub = dir.path().join("sub");
        assert!(!folder.can_go_back());
        assert_eq!(folder.go_back(), Ok(false));

        folder.set_cursor(2); // a.txt
        folder.navigate(sub.clone()).unwrap();
        folder.navigate(sub.join("deep")).unwrap();
        folder.load().unwrap(); // a reload is no visit
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.path(), sub);
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.path(), dir.path());
        assert_eq!(folder.current().unwrap().name, "a.txt"); // cursor restored
        assert!(!folder.can_go_back());

        assert_eq!(folder.go_forward(), Ok(true));
        assert_eq!(folder.path(), sub);
        // A new visit clears Forward.
        folder.go_up().unwrap();
        assert!(!folder.can_go_forward());
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.path(), sub);

        // A vanished directory is dropped with an error.
        folder.navigate(sub.join("deep")).unwrap();
        folder.go_back().unwrap();
        fs::remove_dir(sub.join("deep")).unwrap();
        assert!(folder.go_forward().is_err());
        assert!(!folder.can_go_forward());
        assert_eq!(folder.path(), sub);
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

    #[test]
    fn search_results_mode() {
        let dir = fixture();
        fs::write(dir.path().join("sub").join("deep.txt"), "").unwrap();
        let mut folder = loaded(&dir);
        let entry = |rel: &str| {
            let path = dir.path().join(rel);
            Hit {
                entry: Entry {
                    name: rel.into(),
                    is_dir: path.is_dir(),
                    path,
                    is_parent: false,
                    size: 0,
                    modified: None,
                    attrs: Default::default(),
                    link: Default::default(),
                },
                line: (rel == "a.txt").then(|| (3, "found".to_string())),
            }
        };

        folder.show_results(dir.path().to_path_buf(), "search".into());
        assert_eq!(folder.results(), Some("search"));
        assert_eq!(names(&folder), [".."]);
        folder.add_results(vec![entry("sub/deep.txt"), entry("b.txt")]);
        folder.add_results(vec![entry("a.txt")]);
        assert_eq!(names(&folder), ["..", "sub/deep.txt", "b.txt", "a.txt"]);
        assert_eq!(folder.entries()[3].hit_line, Some((3, "found".into())));

        folder.set_sort(Sort::default()); // sorts in place, no re-listing
        assert_eq!(names(&folder), ["..", "a.txt", "b.txt", "sub/deep.txt"]);
        assert_eq!(folder.entries()[1].hit_line, Some((3, "found".into())));

        fs::remove_file(dir.path().join("b.txt")).unwrap();
        folder.load().unwrap(); // drops vanished hits
        assert_eq!(names(&folder), ["..", "a.txt", "sub/deep.txt"]);

        // Enter on a hit shows it in its directory.
        folder.set_cursor(2);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.results(), None);
        assert_eq!(folder.path(), dir.path().join("sub"));
        assert_eq!(folder.current().unwrap().name, "deep.txt");

        // ".." / go up from results returns to the search root.
        folder.show_results(dir.path().to_path_buf(), "again".into());
        folder.go_up().unwrap();
        assert_eq!((folder.results(), folder.path()), (None, dir.path()));
        assert_eq!(names(&folder), ["..", "sub", "a.txt"]);
    }

    #[test]
    fn missing_directory_falls_back_to_ancestor() {
        let dir = fixture();
        let mut folder = Folder::new(dir.path().join("gone").join("deeper"));
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), dir.path());
    }

    fn zip_file(path: &Path) {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, text) in [
            ("top.txt", "t"),
            ("docs/a.md", "aa"),
            ("docs/deep/b.md", "b"),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn browses_an_archive_like_a_directory() {
        let dir = fixture();
        let zip = dir.path().join("pack.zip");
        zip_file(&zip);
        let mut folder = loaded(&dir);
        assert_eq!(folder.archive(), None);
        folder.select_path(Some(&zip));
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.archive(), Some(zip.as_path()));
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        assert_eq!(names(&folder), ["..", "docs", "top.txt"]);
        assert_eq!(folder.header(), format!("{} ›", zip.display()));

        folder.set_cursor(1);
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(folder.inner_path().as_deref(), Some("docs"));
        assert_eq!(names(&folder), ["..", "deep", "a.md"]);
        assert_eq!(folder.header(), format!("{} › docs", zip.display()));
        assert_eq!(folder.entries()[1].path, zip.join("docs/deep"));

        // Files are not opened; marks and targets work.
        folder.set_cursor(2);
        assert_eq!(
            folder.archive_targets(),
            Some((zip.clone(), vec!["docs/a.md".to_string()]))
        );
        folder.toggle_mark();
        assert_eq!(folder.marked_count(), 1);

        // Up to the archive root, cursor on the folder we came from.
        folder.go_up().unwrap();
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        assert_eq!(folder.current().unwrap().name, "docs");
        // Reloading an unchanged archive does not read it again.
        let before = folder.mount.as_ref().unwrap().index.clone();
        folder.load().unwrap();
        assert!(Arc::ptr_eq(&before, &folder.mount.as_ref().unwrap().index));
        // Up again leaves the archive, cursor on it.
        folder.go_up().unwrap();
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.path(), dir.path());
        assert_eq!(folder.current().unwrap().path, zip);
    }

    #[test]
    fn uncached_archive_loads_in_two_steps() {
        let dir = fixture();
        let zip = dir.path().join("lazy.zip");
        zip_file(&zip);
        let mut folder = loaded(&dir);
        let before: Vec<String> = names(&folder).into_iter().map(String::from).collect();
        folder.select_path(Some(&zip));
        assert_eq!(folder.activation_target(), Some(zip.clone()));
        assert_eq!(folder.archive_needed(dir.path()), None);
        assert_eq!(folder.archive_needed(&zip), Some(zip.clone()));

        // Loading shows the archive location with nothing in it.
        folder.begin_archive_load(zip.clone());
        assert!(folder.is_loading());
        assert_eq!(folder.header(), format!("{} ›", zip.display()));
        assert!(folder.entries().is_empty());

        // Cancelling returns to the directory, cursor on the archive.
        folder.end_archive_load(true);
        assert!(!folder.is_loading());
        assert_eq!(
            names(&folder),
            before.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(folder.current().unwrap().path, zip);

        // Reloading does nothing while loading; going up cancels the read.
        folder.begin_archive_load(zip.clone());
        folder.load().unwrap();
        assert!(folder.is_loading());
        folder.go_up().unwrap();
        assert!(!folder.is_loading());
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.current().unwrap().path, zip);

        // Toggling hidden files drops the loading state too.
        folder.begin_archive_load(zip.clone());
        assert_eq!(folder.set_show_hidden(true), None);
        assert!(!folder.is_loading());
        assert_eq!(folder.archive(), None);

        // Once read (elsewhere), entering is instant and needs no loading.
        archive::Index::cached(&zip).unwrap();
        assert_eq!(folder.archive_needed(&zip), None);
        folder.begin_archive_load(zip.clone());
        assert!(!folder.is_loading());
        assert_eq!(folder.activate(), Ok(Activation::Entered));
        assert_eq!(names(&folder), ["..", "docs", "top.txt"]);
    }

    #[test]
    fn history_target_names_the_next_directory() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        assert_eq!(folder.history_target(true), None);
        let sub = folder.current().unwrap().path.clone();
        assert_eq!(folder.activation_target(), Some(sub.clone()));
        folder.activate().unwrap();
        assert_eq!(folder.history_target(true).as_deref(), Some(dir.path()));
        folder.go_back().unwrap();
        assert_eq!(folder.history_target(false), Some(sub));
    }

    #[test]
    fn parent_row_leaves_the_archive_and_history_returns() {
        let dir = fixture();
        let zip = dir.path().join("pack.zip");
        zip_file(&zip);
        let mut folder = loaded(&dir);
        folder.select_path(Some(&zip));
        folder.activate().unwrap();
        folder.set_cursor(1);
        folder.activate().unwrap(); // docs
        folder.set_cursor(0);
        folder.activate().unwrap(); // ".." to the archive root
        assert_eq!(folder.inner_path().as_deref(), Some(""));
        folder.set_cursor(0);
        folder.activate().unwrap(); // ".." out of the archive
        assert_eq!(folder.archive(), None);

        // Back re-enters the archive.
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.archive(), Some(zip.as_path()));
        assert_eq!(folder.go_back(), Ok(true));
        assert_eq!(folder.inner_path().as_deref(), Some("docs"));
    }

    #[test]
    fn archive_listing_honours_hidden_and_sort() {
        let dir = fixture();
        let zip = dir.path().join("pack.zip");
        {
            use std::io::Write;
            let mut z = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for (name, text) in [("a.txt", "1"), ("b.txt", "1234"), (".hid", "")] {
                z.start_file(name, opts).unwrap();
                z.write_all(text.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let mut folder = Folder::new(zip.clone());
        assert_eq!(folder.load_or_ancestor(), None);
        assert_eq!(folder.archive(), Some(zip.as_path()));
        assert_eq!(names(&folder), ["..", "a.txt", "b.txt"]);
        folder.set_show_hidden(true);
        assert_eq!(names(&folder), ["..", ".hid", "a.txt", "b.txt"]);
        folder.set_sort(Sort {
            key: SortKey::Size,
            descending: true,
        });
        assert_eq!(names(&folder), ["..", "b.txt", "a.txt", ".hid"]);
        assert_eq!(folder.current().unwrap().name, "..");
    }

    #[test]
    fn broken_or_missing_archives_report_errors() {
        let dir = fixture();
        let bad = dir.path().join("bad.zip");
        fs::write(&bad, "not a zip").unwrap();
        let mut folder = loaded(&dir);
        folder.select_path(Some(&bad));
        assert!(folder.activate().is_err());
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.path(), dir.path());

        // An archive that disappears falls back to its directory.
        let zip = dir.path().join("pack.zip");
        zip_file(&zip);
        folder.load().unwrap();
        folder.select_path(Some(&zip));
        folder.activate().unwrap();
        fs::remove_file(&zip).unwrap();
        folder.load_or_ancestor();
        assert_eq!(folder.archive(), None);
        assert_eq!(folder.path(), dir.path());
    }

    fn done(bytes: u64) -> DirSize {
        DirSize {
            bytes,
            state: crate::dirsize::ScanState::Done,
            ..Default::default()
        }
    }

    #[test]
    fn size_targets_follow_marks_cursor_and_parent() {
        let dir = fixture();
        fs::create_dir(dir.path().join("zdir")).unwrap();
        let mut folder = loaded(&dir);
        let all = vec![dir.path().join("sub"), dir.path().join("zdir")];
        // Cursor on "..": every listed directory.
        folder.set_cursor(0);
        assert_eq!(folder.size_targets(), all);
        // Cursor on a file: also every directory.
        folder.select_path(Some(&dir.path().join("a.txt")));
        assert_eq!(folder.size_targets(), all);
        // Cursor on a directory.
        folder.select_path(Some(&dir.path().join("zdir")));
        assert_eq!(folder.size_targets(), vec![dir.path().join("zdir")]);
        // Marks win; marked files are ignored.
        folder.select_path(Some(&dir.path().join("sub")));
        folder.toggle_mark();
        folder.select_path(Some(&dir.path().join("a.txt")));
        folder.toggle_mark();
        folder.select_path(Some(&dir.path().join("zdir")));
        assert_eq!(folder.size_targets(), vec![dir.path().join("sub")]);
    }

    #[test]
    fn dir_sizes_are_cached_sorted_and_cleared() {
        let dir = fixture();
        fs::create_dir(dir.path().join("big")).unwrap();
        let (sub, big) = (dir.path().join("sub"), dir.path().join("big"));
        let mut folder = loaded(&dir);
        folder.set_sort(Sort {
            key: SortKey::Size,
            descending: false,
        });
        assert_eq!(names(&folder), ["..", "big", "sub", "a.txt", "b.txt"]);

        folder.set_dir_size(&sub, done(9000));
        folder.set_dir_size(&big, done(10));
        folder.set_dir_size(&dir.path().join("gone"), done(1)); // not listed: ignored
        assert_eq!(folder.dir_size(&sub), Some(&done(9000)));
        assert_eq!(folder.dir_size(&dir.path().join("gone")), None);
        folder.resort_by_size();
        assert_eq!(names(&folder), ["..", "big", "sub", "a.txt", "b.txt"]);
        folder.set_dir_size(&big, done(99999));
        folder.resort_by_size();
        assert_eq!(names(&folder), ["..", "sub", "big", "a.txt", "b.txt"]);

        // Sizes survive sorting, filtering and reloading rows...
        folder.set_sort(Sort::default());
        folder.set_filter("su");
        folder.set_filter("");
        assert_eq!(folder.dir_size(&sub), Some(&done(9000)));
        // ...and are dropped on refresh and navigation.
        let epoch = folder.size_epoch();
        folder.refresh();
        assert_eq!(folder.dir_size(&sub), None);
        // A scan started before the refresh cannot bring sizes back.
        folder.set_dir_size_at(epoch, &sub, done(5));
        assert_eq!(folder.dir_size(&sub), None);
        folder.set_dir_size_at(folder.size_epoch(), &sub, done(5));
        assert_eq!(folder.dir_size(&sub), Some(&done(5)));
        folder.set_dir_size(&sub, done(1));
        folder.navigate(sub.clone()).unwrap();
        folder.go_up().unwrap();
        assert_eq!(folder.dir_size(&sub), None);
    }

    #[test]
    fn marked_total_skips_rows_inside_measured_marked_dirs() {
        let dir = fixture();
        let sub = dir.path().join("sub");
        fs::write(sub.join("inner.txt"), "12345").unwrap();
        let mut folder = loaded(&dir);
        folder.set_tree(true);
        folder.toggle_expanded(&sub);
        assert!(names(&folder).contains(&"inner.txt"));
        folder.mark_all();
        let total = folder.marked_total();
        assert_eq!((total.bytes, total.unknown_dirs), (11, 1));
        folder.set_dir_size(&sub, done(1000));
        let total = folder.marked_total();
        assert_eq!((total.bytes, total.unknown_dirs), (1006, 0));
    }

    #[test]
    fn marked_total_adds_files_and_known_dirs() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.mark_all();
        let total = folder.marked_total();
        assert_eq!((total.bytes, total.unknown_dirs), (6, 1));
        assert_eq!(total.label(), "≥ 6 B (1 folder not measured)");

        folder.set_dir_size(&dir.path().join("sub"), done(2048));
        let total = folder.marked_total();
        assert_eq!(
            (total.bytes, total.unknown_dirs, total.inexact),
            (2054, 0, false)
        );
        assert_eq!(total.label(), "2.0 K");

        folder.set_dir_size(
            &dir.path().join("sub"),
            DirSize {
                errors: 1,
                ..done(2048)
            },
        );
        assert_eq!(folder.marked_total().label(), "≥ 2.0 K");
        folder.cancel_running_sizes();
        folder.set_dir_size(
            &dir.path().join("sub"),
            DirSize {
                state: crate::dirsize::ScanState::Scanning,
                ..done(5)
            },
        );
        folder.cancel_running_sizes();
        assert_eq!(
            folder.dir_size(&dir.path().join("sub")).map(|s| s.state),
            Some(crate::dirsize::ScanState::Cancelled)
        );
    }
}

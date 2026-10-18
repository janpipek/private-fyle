//! UI-independent state of a folder panel. Views render a [`Folder`] and
//! translate input into calls on it; all navigation, cursor, marking,
//! sorting and tree expansion rules live here.

use std::collections::{HashMap, HashSet};
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

mod archives;
mod column_filter;
mod history;
mod marks;
pub use marks::{DragSnapshot, PointerSelection};
mod remote;
mod results;
mod sizes;
mod swap;
#[cfg(test)]
pub mod test_support;
mod view;

use archives::Mount;
use history::Visit;

use crate::column_filter::ColumnFilterState;
use crate::dirsize::DirSize;
use crate::entry::{Entry, Sort, compare_entries};
use crate::fs_ops;
use crate::listing::contracts::{ListingRequest, ListingRow, ListingSnapshot};
use crate::location::{Location, PanelTarget};

/// A listed entry and its nesting depth (always 0 outside tree mode).
#[derive(Clone, Debug)]
pub struct Row {
    pub entry: Entry,
    /// Typed identity of this row; `entry.path` is a UI compatibility spelling.
    pub location: Location,
    pub depth: usize,
    /// For a content search hit: the first matching line (1-based) and its text.
    pub hit_line: Option<(u64, String)>,
}

/// The kind of content currently shown by a panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelContent {
    /// A local, remote, or archive directory listing.
    Directory(Location),
    /// Search results rooted at a typed directory location.
    Search { root: Location, title: String },
    /// The platform's top-level places (drive) overview.
    Places,
}

impl Deref for Row {
    type Target = Entry;

    fn deref(&self) -> &Entry {
        &self.entry
    }
}

#[derive(Clone)]
pub struct Folder {
    /// Authoritative panel content and address.
    content: PanelContent,
    /// Address the panel came from, where ".." at a remote root returns.
    remote_origin: Option<Location>,
    /// Rows shown: `all` narrowed by `filter`.
    entries: Vec<Row>,
    /// Everything listed, before filtering.
    all: Vec<Row>,
    /// Rules and compact row decisions for column-value filters.
    column_filter: ColumnFilterState,
    column_filter_cursor: Option<Location>,
    /// Case-insensitive substring the shown names must contain.
    filter: String,
    /// Set while browsing inside an archive; `path` is then the archive
    /// file joined with the inner path.
    mount: Option<Mount>,
    cursor: usize,
    marked: HashSet<Location>,
    /// Stable start of a Shift-click range, across later cursor moves.
    pointer_anchor: Option<Location>,
    sort: Sort,
    show_hidden: bool,
    tree: bool,
    expanded: HashSet<Location>,
    /// Directory sizes computed on demand, by path. Kept across re-sorts
    /// and filters; dropped on navigation and explicit refresh.
    sizes: HashMap<PathBuf, DirSize>,
    /// Bumped whenever `sizes` is cleared, so a scan started earlier can
    /// tell its results are stale.
    size_epoch: u64,
    /// Bumped whenever the listing is (re)read, so views can drop caches.
    generation: u64,
    /// Changes when the listing or a listing-backed value changes.
    listing_revision: u64,
    /// Column-scan revision excludes streamed search appends, which are handled as a suffix.
    column_scan_revision: u64,
    /// Directories visited before / after (Back, Forward), with the entry
    /// the cursor was on, most recent last.
    back: Vec<Visit>,
    forward: Vec<Visit>,
}

impl Folder {
    /// Creates an unloaded folder; apply a snapshot from the listing service next.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let content = if path.as_os_str().is_empty() {
            PanelContent::Places
        } else {
            PanelContent::Directory(Location::from_panel_path(&path, None)?)
        };
        Ok(Self {
            content,
            remote_origin: None,
            entries: Vec::new(),
            all: Vec::new(),
            column_filter: ColumnFilterState::default(),
            column_filter_cursor: None,
            filter: String::new(),
            mount: None,
            cursor: 0,
            marked: HashSet::new(),
            pointer_anchor: None,
            sort: Sort::default(),
            show_hidden: false,
            tree: false,
            expanded: HashSet::new(),
            sizes: HashMap::new(),
            size_epoch: 0,
            generation: 0,
            listing_revision: 0,
            column_scan_revision: 0,
            back: Vec::new(),
            forward: Vec::new(),
        })
    }

    /// Initial options, applied by the first load.
    pub fn with_options(mut self, show_hidden: bool, tree: bool, sort: Sort) -> Self {
        self.show_hidden = show_hidden;
        self.tree = tree;
        self.sort = sort;
        self
    }

    /// The existing panel spelling for UI callers. Virtual paths must not
    /// be passed to native filesystem operations.
    pub fn path(&self) -> PathBuf {
        match &self.content {
            PanelContent::Directory(location) => location.panel_path(),
            PanelContent::Search { root, .. } => root.panel_path(),
            PanelContent::Places => PathBuf::new(),
        }
    }

    /// The typed identity of the folder currently shown.
    pub fn location(&self) -> Option<&Location> {
        match &self.content {
            PanelContent::Directory(location) => Some(location),
            PanelContent::Search { root, .. } => Some(root),
            PanelContent::Places => None,
        }
    }

    /// Returns a native directory path only when this panel shows a local
    /// filesystem directory or local search root.
    pub fn local_directory(&self) -> Option<&Path> {
        match &self.content {
            PanelContent::Directory(Location::Local(local)) => Some(local.as_path()),
            PanelContent::Search {
                root: Location::Local(local),
                ..
            } => Some(local.as_path()),
            _ => None,
        }
    }

    /// The local or remote folder this panel shows if it can be bookmarked:
    /// not search results, an archive or the drive list.
    pub fn bookmark_target(&self) -> Option<&Location> {
        match &self.content {
            PanelContent::Directory(location @ Location::Local(local))
                if self.archive().is_none()
                    && self.results().is_none()
                    && !local.as_path().as_os_str().is_empty() =>
            {
                Some(location)
            }
            PanelContent::Directory(location @ Location::Remote(_))
                if self.archive().is_none() && self.results().is_none() =>
            {
                Some(location)
            }
            _ => None,
        }
    }

    /// Returns the current panel content mode.
    pub fn content(&self) -> &PanelContent {
        &self.content
    }

    /// Returns this panel's typed navigation target.
    pub fn target(&self) -> PanelTarget {
        match &self.content {
            PanelContent::Directory(location) | PanelContent::Search { root: location, .. } => {
                PanelTarget::Location(location.clone())
            }
            PanelContent::Places => PanelTarget::Places,
        }
    }

    /// Selects the row that should remain highlighted after navigating away.
    pub fn selection_for_navigation(&self, target: &PanelTarget) -> Option<Location> {
        if let PanelTarget::Location(Location::Archive { source, member }) = target
            && member.as_str().is_empty()
            && self
                .current()
                .is_some_and(|row| row.location.as_local_path() == source.as_local_path())
        {
            return self.current().map(|row| row.location.clone());
        }
        if let (Some(Location::Archive { source, member }), Some(Location::Local(destination))) =
            (self.location(), target.location())
            && member.as_str().is_empty()
            && source
                .as_local_path()
                .and_then(fs_ops::parent_of)
                .is_some_and(|parent| parent == destination.as_path())
        {
            return Some((**source).clone());
        }
        if matches!(target, PanelTarget::Location(Location::Remote(remote)) if remote.parent().is_none())
        {
            return self.location().cloned();
        }
        // Going to the parent (Go Up or the `..` row): the cursor lands on the
        // folder just left, as for local folders.
        if self.parent_target().as_ref() == Some(target) {
            return self.location().cloned();
        }
        self.current().map(|row| row.location.clone())
    }

    /// The typed destination represented by the current parent row.
    pub fn parent_target(&self) -> Option<PanelTarget> {
        self.entries
            .iter()
            .find(|row| row.is_parent)
            .map(|row| Self::parent_row_target(&row.path, &row.location))
    }

    /// Maps a parent row to a typed panel destination, including Windows'
    /// drive overview, which has no native path.
    pub fn parent_row_target(path: &Path, location: &Location) -> PanelTarget {
        if path.as_os_str().is_empty() {
            PanelTarget::Places
        } else {
            PanelTarget::Location(location.clone())
        }
    }

    /// Captures the options needed to read a target away from the UI thread.
    pub fn listing_request(
        &self,
        target: PanelTarget,
        allow_ancestor_fallback: bool,
        preserve_search: bool,
    ) -> ListingRequest {
        let remote_origin = if matches!(target.location(), Some(Location::Remote(_))) {
            if self.is_remote() {
                self.remote_origin.clone()
            } else {
                self.location().cloned()
            }
        } else {
            self.remote_origin.clone()
        };
        let search_hits = (preserve_search
            && target == self.target()
            && matches!(self.content(), PanelContent::Search { .. }))
        .then(|| {
            self.all
                .iter()
                .map(|row| ListingRow {
                    entry: row.entry.clone(),
                    location: row.location.clone(),
                    depth: row.depth,
                    hit_line: row.hit_line.clone(),
                })
                .collect()
        });
        ListingRequest {
            target,
            tree: self.tree,
            expanded: self.expanded.clone(),
            sort: self.sort,
            remote_origin,
            allow_ancestor_fallback,
            search_hits,
            force_remote_refresh: false,
        }
    }

    /// Applies a completed listing while preserving the current filter,
    /// selection, marks, and size cache when the typed target is unchanged.
    pub fn apply_listing(&mut self, snapshot: ListingSnapshot, selected: Option<Location>) {
        let old_target = self.target();
        let same_target = old_target == snapshot.target;
        if !same_target {
            self.marked.clear();
            self.filter.clear();
            self.column_filter.clear();
            self.column_filter_cursor = None;
            self.clear_sizes();
        }
        if !snapshot.search_results {
            self.content = match snapshot.target.clone() {
                PanelTarget::Location(location) => PanelContent::Directory(location),
                PanelTarget::Places => PanelContent::Places,
            };
        }
        self.remote_origin = snapshot.remote_origin.clone();
        self.mount = match self.location() {
            Some(Location::Archive { source, .. }) => {
                let file = source.as_local_path().expect("archive source is local");
                Some(Mount::loaded(file.to_path_buf()))
            }
            _ => None,
        };
        self.all = snapshot
            .rows
            .into_iter()
            .map(|row| Row {
                entry: row.entry,
                location: row.location,
                depth: row.depth,
                hit_line: row.hit_line,
            })
            .collect();
        self.apply_sizes_to_rows();
        sort_row_tree(&mut self.all, 0, self.sort);
        self.generation = self.generation.wrapping_add(1);
        self.listing_revision = self.listing_revision.wrapping_add(1);
        self.column_scan_revision = self.column_scan_revision.wrapping_add(1);
        self.column_filter.begin(self.all.len());
        self.marked
            .retain(|location| self.all.iter().any(|row| &row.location == location));
        self.apply_filter();
        if !self.select_location(selected.as_ref()) {
            self.set_cursor(self.cursor);
        }
    }

    /// Commits a successful navigation and records the directory left behind.
    pub fn commit_navigation_listing(
        &mut self,
        snapshot: ListingSnapshot,
        selected: Option<Location>,
    ) {
        let left = self.visit();
        self.apply_listing(snapshot, selected);
        if let Some(visit) = left
            .clone()
            .filter(|visit| Some(&visit.path) != self.location())
        {
            history::push_capped(&mut self.back, visit);
            self.forward.clear();
        }
    }

    /// Returns the next typed history location and saved cursor identity.
    pub fn history_visit(
        &self,
        back: bool,
    ) -> Option<(Location, Option<Location>, Option<Location>)> {
        let list = if back { &self.back } else { &self.forward };
        list.last().map(|visit| {
            (
                visit.path.clone(),
                visit.selected.clone(),
                visit.remote_origin.clone(),
            )
        })
    }

    /// Applies a successful Back or Forward listing and updates both stacks.
    pub fn commit_history_listing(
        &mut self,
        snapshot: ListingSnapshot,
        back: bool,
        selected: Option<Location>,
    ) {
        let here = self.visit();
        let next_target = snapshot.target.clone();
        let from = if back {
            &mut self.back
        } else {
            &mut self.forward
        };
        if from
            .last()
            .is_some_and(|visit| Some(&visit.path) == next_target.location())
        {
            from.pop();
        }
        self.apply_listing(snapshot, selected);
        if let Some(here) = here {
            history::push_capped(
                if back {
                    &mut self.forward
                } else {
                    &mut self.back
                },
                here,
            );
        }
    }

    /// Re-sorts cached rows by sibling group without reading the filesystem.
    pub fn sort_cached(&mut self) {
        sort_row_tree(&mut self.all, 0, self.sort);
        self.apply_filter();
    }

    /// Refreshes row directory-size values from the model's current cache.
    fn apply_sizes_to_rows(&mut self) {
        for row in &mut self.all {
            if let Some(size) = self.sizes.get(&row.path) {
                row.entry.size = Some(size.bytes);
            }
        }
    }

    pub fn entries(&self) -> &[Row] {
        &self.entries
    }

    /// Changes whenever the listing is read again by the listing service.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn current(&self) -> Option<&Row> {
        self.entries.get(self.cursor)
    }

    /// Name of the entry under the cursor, excluding the parent navigation row.
    pub fn current_copy_name(&self) -> Option<&str> {
        self.current()
            .filter(|entry| !entry.is_parent)
            .map(|entry| entry.name.as_str())
    }

    pub fn sort(&self) -> Sort {
        self.sort
    }

    pub fn is_tree(&self) -> bool {
        self.tree
    }

    pub fn is_expanded(&self, location: &Location) -> bool {
        self.tree && self.mount.is_none() && self.expanded.contains(location)
    }

    /// Returns the current typed expansion set for service listing requests.
    pub fn expanded_locations(&self) -> HashSet<Location> {
        self.expanded.clone()
    }

    pub fn is_marked(&self, location: &Location) -> bool {
        self.marked.contains(location)
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
        self.column_filter_cursor = None;
        let ix = ix.min(self.entries.len().saturating_sub(1));
        let changed = ix != self.cursor;
        self.cursor = ix;
        changed
    }

    /// Moves the cursor to `path`; returns false if it is not listed.
    pub fn select_path(&mut self, path: Option<&Path>) -> bool {
        let Some(ix) = path.and_then(|p| self.entries.iter().position(|e| e.path == p)) else {
            return false;
        };
        self.cursor = ix;
        true
    }

    /// Moves the cursor to a typed row identity; returns false when absent.
    pub fn select_location(&mut self, location: Option<&Location>) -> bool {
        let Some(ix) =
            location.and_then(|target| self.entries.iter().position(|row| &row.location == target))
        else {
            return false;
        };
        self.cursor = ix;
        true
    }
}

fn sort_row_tree(rows: &mut Vec<Row>, depth: usize, sort: Sort) {
    let mut groups: Vec<Vec<Row>> = Vec::new();
    for row in rows.drain(..) {
        if row.depth == depth {
            groups.push(vec![row]);
        } else if let Some(group) = groups.last_mut() {
            group.push(row);
        }
    }
    groups.sort_by(|a, b| compare_entries(&a[0].entry, &b[0].entry, sort.key, sort.descending));
    for mut group in groups {
        if group.len() > 1 {
            let mut children = group.split_off(1);
            sort_row_tree(&mut children, depth + 1, sort);
            group.extend(children);
        }
        rows.extend(group);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::ListingService;
    use crate::model::test_support::{Activation, TestFolder};
    use crate::model::test_support::{fixture, loaded, names};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn copy_name_uses_cursor_and_skips_parent() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        assert_eq!(folder.current_copy_name(), None);
        folder.set_cursor(1);
        assert_eq!(folder.current_copy_name(), Some("sub"));
        folder.set_cursor(2);
        assert_eq!(folder.current_copy_name(), Some("a.txt"));
    }

    #[test]
    fn service_refresh_preserves_search_but_navigation_exits_it() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.show_results(dir.path().to_path_buf(), "Search docs".into());
        let vanished = dir.path().join("b.txt");
        folder.add_results(vec![crate::search::Hit {
            entry: Entry {
                name: "b.txt".into(),
                path: vanished.clone(),
                is_dir: false,
                is_parent: false,
                size: Some(1),
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            },
            line: None,
        }]);
        std::fs::remove_file(vanished).unwrap();
        let refresh = folder.listing_request(folder.target(), false, true);
        let snapshot = ListingService::default()
            .list(&refresh, &AtomicBool::new(false))
            .unwrap();
        folder.apply_listing(snapshot, None);
        assert_eq!(folder.results(), Some("Search docs"));
        assert!(!folder.entries().iter().any(|row| row.name == "b.txt"));

        let navigation = folder.listing_request(folder.target(), false, false);
        let snapshot = ListingService::default()
            .list(&navigation, &AtomicBool::new(false))
            .unwrap();
        folder.apply_listing(snapshot, None);
        assert_eq!(folder.results(), None);
        assert!(folder.entries().iter().any(|row| row.name == "a.txt"));
    }

    #[test]
    fn typed_places_navigation_uses_the_panel_target_without_a_native_path() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.navigate_target(PanelTarget::Places).unwrap();
        assert_eq!(folder.content(), &PanelContent::Places);
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
            Ok(Activation::OpenFile(
                Location::local(dir.path().join("a.txt")).unwrap(),
            ))
        );
    }

    #[test]
    fn missing_directory_falls_back_to_ancestor() {
        let dir = fixture();
        let mut folder = TestFolder::new(dir.path().join("gone").join("deeper")).unwrap();
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), dir.path());
    }

    #[test]
    fn rejects_invalid_start_addresses_without_a_panic() {
        assert!(TestFolder::new("relative".into()).is_err());
        assert!(TestFolder::new("nas:relative".into()).is_err());
        assert!(TestFolder::new(PathBuf::new()).is_ok());
    }

    #[test]
    fn empty_parent_row_targets_places_without_a_fake_local_path() {
        let root = Location::local(std::env::temp_dir()).unwrap();
        assert_eq!(
            Folder::parent_row_target(Path::new(""), &root),
            PanelTarget::Places
        );
    }

    #[test]
    fn leaving_places_for_a_directory_uses_no_fake_parent_location() {
        let dir = fixture();
        let mut folder = TestFolder::new(PathBuf::new()).unwrap();
        assert!(matches!(folder.content(), PanelContent::Places));
        assert!(folder.location().is_none());

        folder
            .navigate_location(Location::local(dir.path().to_path_buf()).unwrap())
            .unwrap();
        assert!(matches!(folder.content(), PanelContent::Directory(_)));
        assert_eq!(folder.path(), dir.path());
        assert_eq!(names(&folder), ["..", "sub", "a.txt", "b.txt"]);
    }

    #[test]
    fn bookmark_target_covers_local_and_remote_folders_but_not_places() {
        let dir = fixture();
        let local = TestFolder::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            local.bookmark_target(),
            Some(&Location::local(dir.path().to_path_buf()).unwrap())
        );
        let remote = TestFolder::new("nas:/docs/2026".into()).unwrap();
        assert_eq!(
            remote.bookmark_target().map(Location::panel_path),
            Some(PathBuf::from("nas:/docs/2026"))
        );
        assert!(
            TestFolder::new(PathBuf::new())
                .unwrap()
                .bookmark_target()
                .is_none()
        );
    }
}

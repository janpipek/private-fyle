//! UI-independent state of a folder panel. Views render a [`Folder`] and
//! translate input into calls on it; all navigation, cursor, marking,
//! sorting and tree expansion rules live here.

use std::collections::{HashMap, HashSet};
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

mod archives;
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

#[cfg(test)]
use crate::archive;
use crate::dirsize::DirSize;
#[cfg(test)]
use crate::entry::sort_entries;
use crate::entry::{Entry, Sort, compare_entries};
use crate::fs_ops;
use crate::listing::contracts::{ListingRequest, ListingRow, ListingSnapshot};
use crate::location::{Location, PanelTarget};

/// Result of activating (Enter / double-click) the entry under the cursor.
#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
pub enum Activation {
    Entered,
    OpenFile(Location),
    Nothing,
}

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
    /// Remote listings loaded in the background; remote rows are built from
    /// them, never from I/O.
    #[cfg(test)]
    remote_listings: remote::ListingCache,
    /// Address the panel came from, where ".." at a remote root returns.
    remote_origin: Option<Location>,
    /// Rows shown: `all` narrowed by `filter`.
    entries: Vec<Row>,
    /// Everything listed, before filtering.
    all: Vec<Row>,
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
    #[cfg(test)]
    loading: Option<Box<Folder>>,
}

impl Folder {
    /// Creates an unloaded folder; call [`Folder::load_or_ancestor`] next.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let content = if path.as_os_str().is_empty() {
            PanelContent::Places
        } else {
            PanelContent::Directory(Location::from_panel_path(&path, None)?)
        };
        Ok(Self {
            content,
            #[cfg(test)]
            remote_listings: Default::default(),
            remote_origin: None,
            entries: Vec::new(),
            all: Vec::new(),
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
            back: Vec::new(),
            forward: Vec::new(),
            #[cfg(test)]
            loading: None,
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
        let ix = ix.min(self.entries.len().saturating_sub(1));
        let changed = ix != self.cursor;
        self.cursor = ix;
        changed
    }

    /// Re-reads the current directory, keeping cursor entry and marks.
    /// Search results are not re-run; entries that disappeared are dropped.
    /// While an archive is being read it does nothing (the placeholder stays).
    #[cfg(test)]
    pub fn load(&mut self) -> Result<(), String> {
        if self.is_loading() {
            return Ok(());
        }
        if matches!(&self.content, PanelContent::Places) {
            let mut entries = Vec::new();
            self.list(&self.path(), 0, &mut entries)
                .map_err(|error| format!("Drives: {error}"))?;
            self.all = entries;
            self.apply_filter();
            self.generation += 1;
            return Ok(());
        }
        if self.results().is_some() {
            self.generation += 1;
            let selected = self.current().map(|row| row.location.clone());
            self.all.retain(|r| r.is_parent || r.path.exists());
            self.marked
                .retain(|location| location.as_local_path().is_some_and(Path::exists));
            self.apply_filter();
            if !self.select_location(selected.as_ref()) {
                self.set_cursor(self.cursor);
            }
            return Ok(());
        }
        let Some(location) = self.location().cloned() else {
            return Ok(());
        };
        let selected = self.current().map(|row| row.location.clone());
        self.replace_untracked_location(location, selected)
    }

    /// Like [`Folder::load`], but also forgets computed directory sizes,
    /// which may be stale.
    #[cfg(test)]
    pub fn refresh(&mut self) -> Option<String> {
        self.clear_sizes();
        self.load_or_ancestor()
    }

    /// Loads the current directory, or else its nearest readable ancestor.
    /// Returns the first error, if any.
    #[cfg(test)]
    pub fn load_or_ancestor(&mut self) -> Option<String> {
        let mut first_error = None;
        while let Err(e) = self.load() {
            first_error.get_or_insert(e);
            if self.is_remote() {
                break;
            }
            let parent = match self.location() {
                Some(Location::Local(local)) => fs_ops::parent_of(local.as_path()),
                Some(Location::Archive { source, .. }) => {
                    source.as_local_path().and_then(fs_ops::parent_of)
                }
                Some(Location::Remote(_)) | None => None,
            };
            match parent {
                Some(parent) => {
                    self.content = PanelContent::Directory(
                        Location::local(parent.clone())
                            .expect("parent of an absolute path remains absolute"),
                    );
                }
                None => break,
            }
        }
        first_error
    }

    /// Changes directory, putting the cursor on the directory we came from.
    #[cfg(test)]
    pub fn navigate(&mut self, path: PathBuf) -> Result<(), String> {
        let location = Location::from_panel_path(&path, self.archive())
            .map_err(|e| format!("{}: {e}", fs_ops::display_path(&path)))?;
        self.navigate_location(location)
    }

    /// Changes to a typed address, putting the cursor on the directory we came from.
    #[cfg(test)]
    pub fn navigate_location(&mut self, location: Location) -> Result<(), String> {
        let selected = match (self.location(), &location) {
            (Some(Location::Archive { source, member }), Location::Local(destination))
                if member.as_str().is_empty()
                    && source
                        .as_local_path()
                        .and_then(fs_ops::parent_of)
                        .is_some_and(|parent| parent == destination.as_path()) =>
            {
                Some((**source).clone())
            }
            (current, _) => current.cloned(),
        };
        self.replace_location(location, selected)
    }

    /// Changes to a typed panel destination, including the places overview.
    #[cfg(test)]
    pub fn navigate_target(&mut self, target: PanelTarget) -> Result<(), String> {
        match target.clone().into_location() {
            Some(location) => self.navigate_location(location),
            None => self.replace_places(),
        }
    }

    /// While an archive is being read, going up cancels the read instead.
    #[cfg(test)]
    pub fn go_up(&mut self) -> Result<(), String> {
        if self.is_loading() {
            self.end_archive_load(true);
            return Ok(());
        }
        if self.results().is_some() {
            return self.replace(self.path(), None);
        }
        match self.parent_location() {
            Some(parent) => self.navigate(parent),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub fn activate(&mut self) -> Result<Activation, String> {
        let Some(entry) = self.current().cloned() else {
            return Ok(Activation::Nothing);
        };
        if entry.is_dir {
            if self.results().is_some() && entry.is_parent {
                let root = self
                    .location()
                    .cloned()
                    .ok_or_else(|| "search root has no location".to_string())?;
                self.replace_location(root, None)?;
            } else {
                self.navigate_location(entry.location)?;
            }
            Ok(Activation::Entered)
        } else if !self.is_remote()
            && self.mount.is_none()
            && entry
                .location
                .as_local_path()
                .is_some_and(|path| archive::format_of(path).is_some())
        {
            // An archive opens like a directory.
            self.navigate_location(entry.location)?;
            Ok(Activation::Entered)
        } else if self.results().is_some() {
            // A search hit: show it in its directory.
            let parent = entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let parent = Location::local(parent).map_err(|error| error.to_string())?;
            self.replace_location(parent, Some(entry.location))?;
            Ok(Activation::Entered)
        } else {
            Ok(Activation::OpenFile(entry.location))
        }
    }

    /// Loads a typed address, preserving its kind during history travel.
    #[cfg(test)]
    fn replace_untracked_location(
        &mut self,
        requested: Location,
        selected: Option<Location>,
    ) -> Result<(), String> {
        let path = requested.panel_path();
        let mut entries = Vec::new();
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", fs_ops::display_path(&path));
        let mount = if matches!(requested, Location::Remote(_)) {
            None
        } else {
            self.mount_for(&path).map_err(|e| error(&e))?
        };
        let location = match (&requested, &mount) {
            (Location::Archive { .. }, None) => {
                return Err(error(&"archive is no longer available"));
            }
            (Location::Local(_), Some((mount, inner))) => Location::archive(
                Location::local(mount.file.clone()).map_err(|e| error(&e))?,
                inner,
            )
            .map_err(|e| error(&e))?,
            _ => requested,
        };
        match &location {
            Location::Remote(remote) => self
                .list_remote(remote, &mut entries)
                .map_err(|e| error(&e))?,
            Location::Archive { .. } => {
                let (mount, inner) = mount.as_ref().expect("archive location has a mount");
                self.list_archive(mount, inner, &mut entries)
                    .map_err(|e| error(&e))?;
            }
            Location::Local(local) => self
                .list(local.as_path(), 0, &mut entries)
                .map_err(|e| error(&e))?,
        }
        if self.location() != Some(&location) || self.results().is_some() {
            self.marked.clear();
            self.filter.clear();
            self.clear_sizes();
        }
        self.content = PanelContent::Directory(location.clone());
        self.mount = mount.map(|(mount, _)| mount);
        self.all = entries;
        self.generation += 1;
        self.marked
            .retain(|location| self.all.iter().any(|row| &row.location == location));
        self.apply_filter();
        if !self.select_location(selected.as_ref()) {
            self.set_cursor(self.cursor);
        }
        Ok(())
    }

    /// Appends the sorted listing of `dir`, recursing into expanded
    /// directories in tree mode. Unreadable subdirectories list as empty.
    #[cfg(test)]
    fn list(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) -> io::Result<()> {
        let mut entries = fs_ops::read_dir(dir, true)?;
        if depth > 0 {
            entries.retain(|e| !e.is_parent);
        }
        self.apply_sizes(&mut entries);
        sort_entries(&mut entries, self.sort.key, self.sort.descending);
        for entry in entries {
            let location = Location::from_panel_path(&entry.path, None)
                .expect("native listing returns absolute paths");
            let descend = self.tree && !entry.is_parent && self.expanded.contains(&location);
            let path = entry.path.clone();
            out.push(Row {
                location,
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
        let mut folder = Folder::new(dir.path().join("gone").join("deeper")).unwrap();
        assert!(folder.load_or_ancestor().is_some());
        assert_eq!(folder.path(), dir.path());
    }

    #[test]
    fn rejects_invalid_start_addresses_without_a_panic() {
        assert!(Folder::new("relative".into()).is_err());
        assert!(Folder::new("nas:relative".into()).is_err());
        assert!(Folder::new(PathBuf::new()).is_ok());
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
        let mut folder = Folder::new(PathBuf::new()).unwrap();
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
        let local = Folder::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            local.bookmark_target(),
            Some(&Location::local(dir.path().to_path_buf()).unwrap())
        );
        let remote = Folder::new("nas:/docs/2026".into()).unwrap();
        assert_eq!(
            remote.bookmark_target().map(Location::panel_path),
            Some(PathBuf::from("nas:/docs/2026"))
        );
        assert!(
            Folder::new(PathBuf::new())
                .unwrap()
                .bookmark_target()
                .is_none()
        );
    }
}

//! Marking rows, and the targets operations act on.

use std::path::PathBuf;

use super::Folder;
use crate::location::Location;
use crate::location::PanelTarget;

/// Typed, immutable drag source captured before focus or listing changes.
#[derive(Clone, Debug)]
pub struct DragSnapshot {
    /// Identity of the source panel listing.
    pub target: PanelTarget,
    /// Listing generation used to reject stale drops.
    pub generation: u64,
    /// Selected entries captured at drag start.
    pub items: Vec<DragItem>,
}

/// A selected entry and the directory bit from its listing, without disk I/O.
#[derive(Clone, Debug)]
pub struct DragItem {
    /// Typed file address, independent of the active panel.
    pub location: Location,
    /// Directory flag captured from the visible listing.
    pub is_dir: bool,
}

impl DragSnapshot {
    /// Whether the source panel still shows the listing the drag began in.
    pub fn is_current(&self, folder: &Folder) -> bool {
        self.target == folder.target() && self.generation == folder.generation()
    }

    /// Native paths for an outbound desktop drag; remote and archive entries
    /// need materialization and have no native payload yet.
    pub fn native_paths(&self) -> Option<Vec<(PathBuf, bool)>> {
        self.items
            .iter()
            .map(|item| Some((item.location.as_local_path()?.to_path_buf(), item.is_dir)))
            .collect()
    }

    /// Typed source identities, independent of which panel has focus now.
    pub fn locations(&self) -> Vec<Location> {
        self.items
            .iter()
            .map(|item| item.location.clone())
            .collect()
    }
}

/// How a pointer press changes the visible selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerSelection {
    Single,
    Toggle,
    Range,
    AddRange,
}

impl Folder {
    /// Applies a list or icon click using the same typed row identities as
    /// keyboard marks. A plain press on a marked row keeps the group so it can
    /// be dragged together; clicking an unmarked row selects it alone.
    pub fn pointer_select(&mut self, ix: usize, mode: PointerSelection) {
        let Some(row) = self.entries.get(ix) else {
            return;
        };
        let location = row.location.clone();
        let parent = row.is_parent;
        let previous = self.current().map(|row| row.location.clone());
        if parent {
            self.marked.clear();
            self.pointer_anchor = None;
            self.set_cursor(ix);
            return;
        }
        match mode {
            PointerSelection::Single => {
                if !self.marked.contains(&location) {
                    self.marked.clear();
                }
                self.pointer_anchor = Some(location);
            }
            PointerSelection::Toggle => {
                if !self.marked.remove(&location) {
                    self.marked.insert(location.clone());
                }
                self.pointer_anchor = Some(location);
            }
            PointerSelection::Range | PointerSelection::AddRange => {
                let anchor = self.pointer_anchor.as_ref().or(previous.as_ref());
                let start = anchor
                    .and_then(|anchor| self.entries.iter().position(|row| &row.location == anchor))
                    .unwrap_or(ix);
                if mode == PointerSelection::Range {
                    self.marked.clear();
                }
                for row in &self.entries[start.min(ix)..=start.max(ix)] {
                    if !row.is_parent {
                        self.marked.insert(row.location.clone());
                    }
                }
                if self.pointer_anchor.is_none() {
                    self.pointer_anchor = anchor.cloned();
                }
            }
        }
        self.set_cursor(ix);
    }

    /// The row or its marked group captured at the start of a drag.
    pub fn drag_locations(&self, ix: usize) -> Vec<Location> {
        let Some(row) = self.entries.get(ix).filter(|row| !row.is_parent) else {
            return Vec::new();
        };
        if self.marked.contains(&row.location) {
            let mut targets: Vec<_> = self.marked.iter().cloned().collect();
            targets.sort_by_key(Location::panel_path);
            targets
        } else {
            vec![row.location.clone()]
        }
    }

    /// Captures the selected item or marked group for drag and drop.
    pub fn drag_snapshot(&self, ix: usize) -> Option<DragSnapshot> {
        let items: Vec<_> = self
            .drag_locations(ix)
            .into_iter()
            .filter_map(|location| {
                self.entries
                    .iter()
                    .find(|row| row.location == location)
                    .map(|row| DragItem {
                        location,
                        is_dir: row.is_dir,
                    })
            })
            .collect();
        (!items.is_empty()).then(|| DragSnapshot {
            target: self.target(),
            generation: self.generation(),
            items,
        })
    }

    /// Marked row identities, or the row under the cursor when none are marked.
    pub fn target_locations(&self) -> Vec<Location> {
        if !self.marked.is_empty() {
            let mut targets: Vec<_> = self.marked.iter().cloned().collect();
            targets.sort_by_key(Location::panel_path);
            return targets;
        }
        self.current()
            .filter(|row| !row.is_parent)
            .map(|row| vec![row.location.clone()])
            .unwrap_or_default()
    }

    /// Converts selected identities to native paths for filesystem jobs.
    pub fn local_targets(&self) -> std::io::Result<Vec<PathBuf>> {
        self.target_locations()
            .into_iter()
            .map(|location| {
                location.as_local_path().map(PathBuf::from).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("{}: not a local path", location.panel_path().display()),
                    )
                })
            })
            .collect()
    }

    /// Target a context-menu row, preserving a marked selection only when
    /// the clicked entry belongs to it. Invalid rows leave selection alone.
    pub fn select_context_row(&mut self, ix: usize) {
        let Some(row) = self.entries.get(ix) else {
            return;
        };
        if !self.marked.contains(&row.location) {
            self.marked.clear();
        }
        self.set_cursor(ix);
    }

    /// Toggles the mark on the cursor entry and advances the cursor.
    pub fn toggle_mark(&mut self) {
        let Some(entry) = self.current().filter(|e| !e.is_parent) else {
            return;
        };
        let location = entry.location.clone();
        if !self.marked.remove(&location) {
            self.marked.insert(location);
        }
        self.set_cursor(self.cursor + 1);
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Identities of the shown entries that can be marked (not "..").
    pub fn markable(&self) -> impl Iterator<Item = &Location> {
        self.entries
            .iter()
            .filter(|e| !e.is_parent)
            .map(|e| &e.location)
    }

    /// Marks every shown entry (the filter limits what is shown).
    pub fn mark_all(&mut self) {
        let locations: Vec<_> = self.markable().cloned().collect();
        self.marked.extend(locations);
    }

    /// Marks the unmarked shown entries and unmarks the marked ones.
    pub fn invert_marks(&mut self) {
        let locations: Vec<_> = self.markable().cloned().collect();
        for location in locations {
            if !self.marked.remove(&location) {
                self.marked.insert(location);
            }
        }
    }

    /// Marks the shown entries whose name matches `pattern` (a glob if it
    /// has `*`, `?` or `[`, else a substring; case-insensitive). Returns
    /// how many matched.
    pub fn mark_matching(&mut self, pattern: &str) -> Result<usize, String> {
        let matcher = crate::search::NameMatcher::new(pattern)?;
        let locations: Vec<_> = self
            .entries
            .iter()
            .filter(|e| !e.is_parent)
            // Search results show relative paths; match the file name.
            .filter(|e| matcher.matches(e.name.rsplit('/').next().unwrap_or(&e.name)))
            .map(|e| e.location.clone())
            .collect();
        let count = locations.len();
        self.marked.extend(locations);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {

    use crate::location::Location;
    use crate::model::Folder;
    use crate::model::marks::PointerSelection;
    use crate::model::test_support::{fixture, loaded};
    use std::fs;
    use std::path::PathBuf;

    fn target_paths(folder: &Folder) -> Vec<PathBuf> {
        folder
            .target_locations()
            .iter()
            .map(Location::panel_path)
            .collect()
    }

    #[test]
    fn marks_and_targets() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.toggle_mark(); // ".." cannot be marked
        assert_eq!(folder.marked_count(), 0);
        assert_eq!(target_paths(&folder), Vec::<PathBuf>::new());

        folder.set_cursor(2);
        folder.toggle_mark();
        folder.toggle_mark();
        assert_eq!(folder.marked_count(), 2);
        assert_eq!(folder.current().unwrap().name, "b.txt"); // stays on last row
        assert_eq!(
            target_paths(&folder),
            [dir.path().join("a.txt"), dir.path().join("b.txt")]
        );

        // Reload keeps marks that still exist.
        fs::remove_file(dir.path().join("a.txt")).unwrap();
        folder.load().unwrap();
        assert_eq!(target_paths(&folder), [dir.path().join("b.txt")]);

        folder.clear_marks();
        assert_eq!(target_paths(&folder), [dir.path().join("b.txt")]); // cursor entry
    }

    #[test]
    fn pointer_click_toggle_range_and_drag_snapshot() {
        let dir = fixture();
        let mut folder = loaded(&dir); // .., sub, a.txt, b.txt
        folder.pointer_select(2, PointerSelection::Single);
        assert_eq!(target_paths(&folder), [dir.path().join("a.txt")]);
        folder.pointer_select(3, PointerSelection::Toggle);
        assert_eq!(folder.marked_count(), 1);
        folder.pointer_select(1, PointerSelection::Range);
        assert_eq!(folder.marked_count(), 3);
        assert_eq!(folder.drag_locations(2).len(), 3);
        folder.pointer_select(0, PointerSelection::AddRange);
        assert_eq!(folder.marked_count(), 0);
        assert!(folder.drag_locations(0).is_empty());
    }

    #[test]
    fn drag_snapshot_keeps_source_and_native_paths() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.pointer_select(2, PointerSelection::Toggle);
        folder.pointer_select(3, PointerSelection::Toggle);
        let drag = folder.drag_snapshot(2).unwrap();
        assert_eq!(drag.items.len(), 2);
        assert!(drag.is_current(&folder));
        assert_eq!(drag.native_paths().unwrap().len(), 2);
        folder.load().unwrap();
        assert!(!drag.is_current(&folder));
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
        assert_eq!(target_paths(&folder), marked);
        folder.select_context_row(usize::MAX);
        assert_eq!(folder.current().unwrap().name, "a.txt");
        assert_eq!(target_paths(&folder), marked);

        folder.select_context_row(1);
        assert_eq!(folder.marked_count(), 0);
        assert_eq!(target_paths(&folder), [dir.path().join("sub")]);
        folder.mark_all();
        folder.select_context_row(0); // Parent is never a file-operation target.
        assert_eq!(folder.marked_count(), 0);
        assert!(folder.target_locations().is_empty());
        folder.select_context_row(3);
        assert_eq!(target_paths(&folder), [dir.path().join("b.txt")]);
    }

    #[test]
    fn mark_all_invert_and_by_pattern() {
        let dir = fixture();
        let mut folder = loaded(&dir); // .., sub, a.txt, b.txt
        let marked = |f: &Folder| {
            let mut names: Vec<_> = f
                .entries()
                .iter()
                .filter(|e| f.is_marked(&e.location))
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

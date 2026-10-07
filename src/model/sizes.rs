//! Directory sizes computed on demand and the total of the marked entries.

use std::path::{Path, PathBuf};

#[cfg(test)]
use super::Entry;
use super::{Folder, Row};
use crate::dirsize::DirSize;
use crate::fs_ops::{self, SortKey};
use crate::location::Location;

/// What [`Folder::marked_total`] knows about the marked entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkedTotal {
    pub bytes: u64,
    /// Marked directories without a computed size (not counted in `bytes`).
    pub unknown_dirs: usize,
    /// Marked files whose provider did not report a size.
    pub unknown_files: usize,
    /// Some computed size is a lower bound (running, cancelled or errors).
    pub inexact: bool,
}

impl MarkedTotal {
    /// For the footer, marking partial totals when any size is unknown.
    pub fn label(&self) -> String {
        let size = fs_ops::format_size(self.bytes);
        let bound = if self.inexact || self.unknown_dirs > 0 || self.unknown_files > 0 {
            "≥ "
        } else {
            ""
        };
        let folder_note = match self.unknown_dirs {
            0 => String::new(),
            1 => "1 folder not measured".to_string(),
            n => format!("{n} folders not measured"),
        };
        let file_note = match self.unknown_files {
            0 => String::new(),
            1 => "1 file size unknown".to_string(),
            n => format!("{n} file sizes unknown"),
        };
        let note = [folder_note, file_note]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if note.is_empty() {
            format!("{bound}{size}")
        } else {
            format!("{bound}{size} ({note})")
        }
    }

    fn include_file(&mut self, size: Option<u64>) {
        match size {
            Some(bytes) => self.bytes += bytes,
            None => self.unknown_files += 1,
        }
    }
}

impl Folder {
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
            return dirs(
                &mut self
                    .all
                    .iter()
                    .filter(|r| self.marked.contains(&r.location)),
            );
        }
        match self.current() {
            Some(row) if row.is_dir && !row.is_parent => vec![row.path.clone()],
            _ => dirs(&mut self.entries.iter().filter(|r| r.depth == 0)),
        }
    }

    pub fn clear_sizes(&mut self) {
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
                row.entry.size = Some(size.bytes);
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
    #[cfg(test)]
    pub fn apply_sizes(&self, entries: &mut [Entry]) {
        if self.sizes.is_empty() {
            return;
        }
        for entry in entries.iter_mut().filter(|e| e.is_dir && !e.is_parent) {
            if let Some(size) = self.sizes.get(&entry.path) {
                entry.size = Some(size.bytes);
            }
        }
    }

    /// Total size of the marked entries: files plus measured directories.
    pub fn marked_total(&self) -> MarkedTotal {
        let mut total = MarkedTotal::default();
        // In tree mode a marked row may lie inside a marked, measured
        // directory whose size already includes it.
        let covered = |path: &Path| {
            path.ancestors().skip(1).any(|a| {
                Location::local(a.to_path_buf())
                    .is_ok_and(|location| self.marked.contains(&location))
                    && self.sizes.contains_key(a)
            })
        };
        for row in self
            .all
            .iter()
            .filter(|r| self.marked.contains(&r.location))
        {
            if covered(&row.path) {
                continue;
            }
            if !row.is_dir {
                total.include_file(row.size);
            } else if let Some(size) = self.sizes.get(&row.path) {
                total.bytes += size.bytes;
                total.inexact |= !size.is_exact();
            } else {
                total.unknown_dirs += 1;
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_ops::SortKey;
    use crate::model::Sort;
    use crate::model::test_support::{fixture, loaded, names, reload};

    use std::fs;

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
    fn unknown_marked_files_make_the_total_a_lower_bound() {
        let mut total = MarkedTotal::default();
        total.include_file(Some(0));
        total.include_file(Some(12));
        total.include_file(None);
        assert_eq!(total.bytes, 12);
        assert_eq!(total.unknown_files, 1);
        assert_eq!(total.label(), "≥ 12 B (1 file size unknown)");
    }

    #[test]
    fn marked_total_skips_rows_inside_measured_marked_dirs() {
        let dir = fixture();
        let sub = dir.path().join("sub");
        fs::write(sub.join("inner.txt"), "12345").unwrap();
        let mut folder = loaded(&dir);
        folder.set_tree(true);
        reload(&mut folder);
        folder.toggle_expanded(&Location::local(sub.clone()).unwrap());
        reload(&mut folder);
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

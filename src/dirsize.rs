//! Recursive directory sizes, computed on demand. UI-independent: the
//! scanner walks directories and reports [`DirSize`] values; the folder
//! model caches them and the views show them.
//!
//! Rules: symbolic links are never followed (a link counts as its own
//! length); on Unix a file with several hard links counts once per root
//! (by device and inode), so each result stands alone; a scan stays on
//! the filesystem of its root (like `du -x`) and skips mount points, which
//! does not make a result incomplete. Sizes are apparent lengths, as in the
//! Size column, not disk usage.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::fs_ops::format_size;

/// How far a scan of one directory got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanState {
    /// Still running: the size is a lower bound.
    #[default]
    Scanning,
    Done,
    /// Stopped by the user or by leaving the folder: a lower bound.
    Cancelled,
}

/// The recursive size of one directory, possibly partial.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirSize {
    pub bytes: u64,
    /// Files (and links) counted.
    pub files: u64,
    /// Entries and directories that could not be read and are missing.
    pub errors: u64,
    pub state: ScanState,
}

impl DirSize {
    /// Finished without read errors: `bytes` is the true size.
    pub fn is_exact(&self) -> bool {
        self.state == ScanState::Done && self.errors == 0
    }

    /// Still being measured.
    pub fn is_running(&self) -> bool {
        self.state == ScanState::Scanning
    }

    /// Text for the Size column: "1.2 G", "≥ 1.2 G" when incomplete
    /// (read errors or cancelled), "… 1.2 G" while scanning.
    pub fn label(&self) -> String {
        let size = format_size(self.bytes);
        match self.state {
            ScanState::Scanning => format!("… {size}"),
            _ if self.is_exact() => size,
            _ => format!("≥ {size}"),
        }
    }
}

/// Entries between progress reports.
const REPORT_EVERY: u64 = 512;

#[cfg(unix)]
fn identity(meta: &fs::Metadata) -> (u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino(), meta.nlink())
}

#[cfg(not(unix))]
fn identity(_: &fs::Metadata) -> (u64, u64, u64) {
    (0, 0, 1)
}

/// Measures each of `roots` in turn. `update` gets every root's running
/// total now and then, and its final value (`Done`, or `Cancelled` for the
/// root that was running when `cancel` was set; later roots are not
/// reported). Blocking: run it in the background.
pub fn scan_dirs(roots: &[PathBuf], cancel: &AtomicBool, mut update: impl FnMut(&Path, &DirSize)) {
    for root in roots {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut seen = HashSet::new();
        let size = scan_one(root, cancel, &mut seen, &mut |s| update(root, s));
        update(root, &size);
    }
}

/// One directory, without progress reports.
#[cfg(test)]
pub fn scan_dir(root: &Path, cancel: &AtomicBool) -> DirSize {
    let mut seen = HashSet::new();
    scan_one(root, cancel, &mut seen, &mut |_| {})
}

fn scan_one(
    root: &Path,
    cancel: &AtomicBool,
    seen: &mut HashSet<(u64, u64)>,
    progress: &mut dyn FnMut(&DirSize),
) -> DirSize {
    let mut total = DirSize::default();
    let root_dev = match fs::metadata(root) {
        Ok(meta) => identity(&meta).0,
        Err(_) => {
            total.errors = 1;
            total.state = ScanState::Done;
            return total;
        }
    };
    let mut pending = vec![root.to_path_buf()];
    let mut counted = 0u64;
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                total.errors += 1;
                continue;
            }
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                total.state = ScanState::Cancelled;
                return total;
            }
            let Ok(entry) = entry else {
                total.errors += 1;
                continue;
            };
            // `DirEntry::metadata` does not follow symbolic links.
            let Ok(meta) = entry.metadata() else {
                total.errors += 1;
                continue;
            };
            let (dev, ino, nlinks) = identity(&meta);
            if meta.is_dir() {
                if dev == root_dev {
                    pending.push(entry.path());
                }
                continue;
            }
            if nlinks > 1 && !seen.insert((dev, ino)) {
                continue;
            }
            total.bytes += meta.len();
            total.files += 1;
            counted += 1;
            if counted.is_multiple_of(REPORT_EVERY) {
                progress(&total);
            }
        }
    }
    total.state = ScanState::Done;
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory removed on drop (the crate has no `tempfile`).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            use std::sync::atomic::AtomicUsize;
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "fyle-dirsize-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, len: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, vec![b'x'; len]).unwrap();
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn sums_nested_files() {
        let tmp = TempDir::new();
        write(&tmp.path().join("a"), 10);
        write(&tmp.path().join("sub/b"), 20);
        write(&tmp.path().join("sub/deep/c"), 30);
        fs::create_dir(tmp.path().join("empty")).unwrap();
        let size = scan_dir(tmp.path(), &no_cancel());
        assert_eq!((size.bytes, size.files, size.errors), (60, 3, 0));
        assert!(size.is_exact());
        assert_eq!(scan_dir(&tmp.path().join("empty"), &no_cancel()).bytes, 0);
    }

    #[test]
    fn missing_root_is_an_error() {
        let tmp = TempDir::new();
        let size = scan_dir(&tmp.path().join("nope"), &no_cancel());
        assert_eq!(size.errors, 1);
        assert!(!size.is_exact());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let tmp = TempDir::new();
        let outside = TempDir::new();
        write(&outside.path().join("big"), 1000);
        write(&tmp.path().join("file"), 5);
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("dirlink")).unwrap();
        std::os::unix::fs::symlink(tmp.path(), tmp.path().join("loop")).unwrap();
        let size = scan_dir(tmp.path(), &no_cancel());
        assert_eq!(size.files, 3);
        assert!(size.bytes >= 5 && size.bytes < 1000, "{}", size.bytes);
        assert!(size.is_exact());
    }

    #[cfg(unix)]
    #[test]
    fn counts_hard_links_once() {
        let tmp = TempDir::new();
        write(&tmp.path().join("a/orig"), 100);
        fs::create_dir(tmp.path().join("b")).unwrap();
        fs::hard_link(tmp.path().join("a/orig"), tmp.path().join("b/link")).unwrap();
        fs::hard_link(tmp.path().join("a/orig"), tmp.path().join("link2")).unwrap();
        let size = scan_dir(tmp.path(), &no_cancel());
        assert_eq!((size.bytes, size.files), (100, 1));

        // Each root of one scan stands alone: the inode counts in both.
        let mut got = std::collections::HashMap::new();
        let roots = [tmp.path().join("a"), tmp.path().join("b")];
        scan_dirs(&roots, &no_cancel(), |p, s| {
            got.insert(p.to_path_buf(), *s);
        });
        assert_eq!(got[&roots[0]].bytes, 100);
        assert_eq!(got[&roots[1]].bytes, 100);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_makes_result_incomplete() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        write(&tmp.path().join("ok"), 7);
        write(&tmp.path().join("locked/hidden"), 50);
        let locked = tmp.path().join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable = fs::read_dir(&locked).is_ok(); // running as root
        let size = scan_dir(tmp.path(), &no_cancel());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }
        assert_eq!((size.bytes, size.errors), (7, 1));
        assert!(!size.is_exact());
        assert!(size.label().starts_with("≥ "));
    }

    #[test]
    fn cancelled_scan_is_a_lower_bound() {
        let tmp = TempDir::new();
        write(&tmp.path().join("a"), 10);
        let cancel = AtomicBool::new(true);
        let size = scan_dir(tmp.path(), &cancel);
        assert_eq!(size.state, ScanState::Cancelled);
        assert!(!size.is_exact());

        let mut calls = 0;
        scan_dirs(&[tmp.path().to_path_buf()], &cancel, |_, _| calls += 1);
        assert_eq!(calls, 0, "nothing is reported once cancelled");
    }

    #[test]
    fn reports_progress_and_final_value() {
        let tmp = TempDir::new();
        for i in 0..(REPORT_EVERY as usize + 3) {
            write(&tmp.path().join(format!("f{i}")), 1);
        }
        let mut states = Vec::new();
        scan_dirs(&[tmp.path().to_path_buf()], &no_cancel(), |_, s| {
            states.push(*s)
        });
        assert!(states.len() >= 2);
        assert!(states[0].is_running());
        let last = states.last().unwrap();
        assert!(last.is_exact());
        assert_eq!(last.bytes, REPORT_EVERY + 3);
    }

    #[test]
    fn labels() {
        let done = DirSize {
            bytes: 1536,
            state: ScanState::Done,
            ..Default::default()
        };
        assert_eq!(done.label(), "1.5 K");
        let partial = DirSize { errors: 2, ..done };
        assert_eq!(partial.label(), "≥ 1.5 K");
        let cancelled = DirSize {
            state: ScanState::Cancelled,
            ..done
        };
        assert_eq!(cancelled.label(), "≥ 1.5 K");
        let running = DirSize {
            state: ScanState::Scanning,
            ..done
        };
        assert_eq!(running.label(), "… 1.5 K");
    }
}

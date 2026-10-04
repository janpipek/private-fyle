//! Long-running file operations (copy, move, trash, delete) with progress
//! and cancellation, UI-independent. The UI runs [`run`] in the background
//! and polls [`Progress`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::fs_ops;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Trash,
    Delete,
}

impl JobKind {
    pub fn verb(self) -> (&'static str, &'static str) {
        match self {
            Self::Copy => ("Copying", "Copied"),
            Self::Move => ("Moving", "Moved"),
            Self::Trash => ("Moving to trash", "Moved to trash"),
            Self::Delete => ("Deleting", "Deleted"),
        }
    }
}

/// Shared between the worker and the UI.
#[derive(Default)]
pub struct Progress {
    /// Units of work: files for copy/move, items for trash/delete.
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub cancel: AtomicBool,
    /// What is being processed now.
    pub current: Mutex<String>,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.;
        }
        (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.)
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn step(&self, path: &Path) -> io::Result<()> {
        self.done.fetch_add(1, Ordering::Relaxed);
        *self.current.lock().unwrap() = path.display().to_string();
        if self.cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Top-level items fully processed.
    pub done: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

/// Files (and symlinks) below `path`, counting `path` itself if a file.
pub fn count_files(path: &Path) -> usize {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::read_dir(path)
            .map(|items| items.flatten().map(|i| count_files(&i.path())).sum())
            .unwrap_or(0),
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Runs the operation on each source. `dest` is the target directory for
/// copy and move. Blocking; checks `progress.cancel` between files.
pub fn run(
    kind: JobKind,
    sources: &[PathBuf],
    dest: Option<&Path>,
    progress: &Progress,
) -> Outcome {
    let counts: Vec<usize> = match kind {
        JobKind::Copy | JobKind::Move => sources.iter().map(|s| count_files(s)).collect(),
        JobKind::Trash | JobKind::Delete => vec![1; sources.len()],
    };
    progress.total.store(counts.iter().sum(), Ordering::Relaxed);

    let mut outcome = Outcome::default();
    for (src, count) in sources.iter().zip(counts) {
        if progress.cancelled() {
            outcome.cancelled = true;
            break;
        }
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned());
        let name = name.unwrap_or_else(|| src.display().to_string());
        let before = progress.done.load(Ordering::Relaxed);
        let result = match kind {
            JobKind::Copy | JobKind::Move => {
                let target = dest.unwrap_or(Path::new(".")).join(&name);
                let mut on_file = |p: &Path| progress.step(p);
                let result = if kind == JobKind::Copy {
                    fs_ops::copy_with(src, &target, &mut on_file)
                } else {
                    fs_ops::move_with(src, &target, &mut on_file)
                };
                // A plain rename moves everything at once.
                if result.is_ok() {
                    progress.done.store(before + count, Ordering::Relaxed);
                }
                result.map_err(|e| e.to_string())
            }
            JobKind::Trash => {
                fs_ops::trash(src).and_then(|()| progress.step(src).map_err(|e| e.to_string()))
            }
            JobKind::Delete => fs_ops::delete(src)
                .and_then(|()| progress.step(src))
                .map_err(|e| e.to_string()),
        };
        match result {
            Ok(()) => outcome.done += 1,
            Err(_) if progress.cancelled() => {
                outcome.cancelled = true;
                break;
            }
            Err(e) => outcome.errors.push(format!("{name}: {e}")),
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fyle-jobs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for file in ["src/a/1.txt", "src/a/2.txt", "src/b.txt", "single.txt"] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file).unwrap();
        }
        fs::create_dir_all(root.join("dest")).unwrap();
        root
    }

    #[test]
    fn copies_with_file_progress() {
        let root = tree("copy");
        let progress = Progress::default();
        let sources = [root.join("src"), root.join("single.txt")];
        let outcome = run(JobKind::Copy, &sources, Some(&root.join("dest")), &progress);
        assert_eq!(
            outcome,
            Outcome {
                done: 2,
                ..Outcome::default()
            }
        );
        assert_eq!(progress.total.load(Ordering::Relaxed), 4);
        assert_eq!(progress.done.load(Ordering::Relaxed), 4);
        assert_eq!(progress.fraction(), 1.);
        assert!(root.join("dest/src/a/2.txt").is_file());

        // Copying again fails per item, without stopping the others.
        let outcome = run(
            JobKind::Copy,
            &sources,
            Some(&root.join("dest")),
            &Progress::default(),
        );
        assert_eq!(outcome.done, 0);
        assert_eq!(outcome.errors.len(), 2);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn moves_and_deletes() {
        let root = tree("move");
        let progress = Progress::default();
        let outcome = run(
            JobKind::Move,
            &[root.join("src")],
            Some(&root.join("dest")),
            &progress,
        );
        assert_eq!(outcome.done, 1);
        assert_eq!(progress.done.load(Ordering::Relaxed), 3);
        assert!(!root.join("src").exists() && root.join("dest/src/b.txt").exists());

        let outcome = run(
            JobKind::Delete,
            &[root.join("dest/src")],
            None,
            &Progress::default(),
        );
        assert_eq!(outcome.done, 1);
        assert!(!root.join("dest/src").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancel_stops_between_files() {
        let root = tree("cancel");
        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let outcome = run(
            JobKind::Copy,
            &[root.join("src")],
            Some(&root.join("dest")),
            &progress,
        );
        assert!(outcome.cancelled);
        assert_eq!(outcome.done, 0);
        assert!(outcome.errors.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}

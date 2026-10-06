//! Long-running file operations (copy, move, trash, delete) with progress
//! and cancellation, UI-independent. The UI runs [`run`] in the background
//! and polls [`Progress`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::archive;
use crate::fs_ops;
use crate::transfer::{Conflict, Decision, TransferKind, TransferReport, TransferSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Trash,
    Delete,
    /// Extracting archive entries; see [`run_extract`].
    Extract,
    /// Uploading to, downloading from or removing on a remote; these run
    /// through [`crate::remote::jobs`].
    Upload,
    Download,
    Remove,
}

impl JobKind {
    pub fn verb(self) -> (&'static str, &'static str) {
        match self {
            Self::Copy => ("Copying", "Copied"),
            Self::Move => ("Moving", "Moved"),
            Self::Trash => ("Moving to trash", "Moved to trash"),
            Self::Delete => ("Deleting", "Deleted"),
            Self::Extract => ("Extracting", "Extracted"),
            Self::Upload => ("Uploading", "Uploaded"),
            Self::Download => ("Downloading", "Downloaded"),
            Self::Remove => ("Removing", "Removed"),
        }
    }
}

impl JobKind {
    /// What [`Outcome::done`] counts, for the status line.
    pub fn unit(self) -> &'static str {
        match self {
            Self::Extract => "file(s)",
            _ => "item(s)",
        }
    }
}

/// Shared between the worker and the UI.
pub struct Progress {
    /// Units of work: files for copy/move, items for trash/delete.
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub cancel: AtomicBool,
    /// What is being processed now.
    pub current: Mutex<String>,
    conflict: Mutex<ConflictState>,
    conflict_ready: Condvar,
}

#[derive(Clone, Debug)]
pub struct ConflictRequest {
    pub id: u64,
    pub conflict: Conflict,
}

#[derive(Default)]
struct ConflictState {
    next_id: u64,
    pending: Option<ConflictRequest>,
    response: Option<(u64, Decision)>,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
            current: Mutex::new(String::new()),
            conflict: Mutex::new(ConflictState::default()),
            conflict_ready: Condvar::new(),
        }
    }
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

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.conflict_ready.notify_all();
    }

    pub fn pending_conflict(&self) -> Option<ConflictRequest> {
        self.conflict
            .lock()
            .expect("conflict lock is never poisoned")
            .pending
            .clone()
    }

    pub fn respond(&self, id: u64, decision: Decision) -> Result<(), &'static str> {
        let mut state = self
            .conflict
            .lock()
            .expect("conflict lock is never poisoned");
        let Some(request) = &state.pending else {
            return Err("conflict is no longer pending");
        };
        if request.id != id {
            return Err("conflict response is stale");
        }
        if !request.conflict.allows(decision.choice) {
            return Err("choice is not valid for this conflict");
        }
        if state.response.is_some() {
            return Err("conflict was already answered");
        }
        state.response = Some((id, decision));
        self.conflict_ready.notify_all();
        Ok(())
    }

    pub fn resolve(&self, conflict: &Conflict) -> io::Result<Decision> {
        let mut state = self
            .conflict
            .lock()
            .expect("conflict lock is never poisoned");
        if self.cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1);
        state.pending = Some(ConflictRequest {
            id,
            conflict: conflict.clone(),
        });
        state.response = None;
        loop {
            if self.cancelled() {
                state.pending = None;
                state.response = None;
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            if let Some((response_id, decision)) = state.response.take()
                && response_id == id
            {
                state.pending = None;
                return Ok(decision);
            }
            state = self
                .conflict_ready
                .wait(state)
                .expect("conflict lock is never poisoned");
        }
    }

    fn step(&self, path: &Path) -> io::Result<()> {
        self.done.fetch_add(1, Ordering::Relaxed);
        *self
            .current
            .lock()
            .expect("progress lock is never poisoned") = path.display().to_string();
        if self.cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        Ok(())
    }
}

/// Entries yanked (Ctrl-C) or cut (Ctrl-X), waiting to be pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Yank {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

impl Yank {
    /// The job pasting into `dest`, or why not. Pasting into the directory
    /// the entries come from would only produce "already exists" errors.
    pub fn paste_into(&self, dest: &Path) -> Result<JobKind, String> {
        if self.cut && self.paths.iter().all(|p| p.parent() == Some(dest)) {
            return Err("Already in this directory".into());
        }
        if let Some(inside) = self.paths.iter().find(|p| dest.starts_with(p)) {
            return Err(format!("Cannot paste {} into itself", inside.display()));
        }
        Ok(if self.cut {
            JobKind::Move
        } else {
            JobKind::Copy
        })
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Top-level items fully processed.
    pub done: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
    /// Conflicts explicitly skipped, including skips inside directories.
    pub skipped: usize,
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
#[cfg(test)]
pub fn run(
    kind: JobKind,
    sources: &[PathBuf],
    dest: Option<&Path>,
    progress: &Progress,
) -> Outcome {
    run_with_resolver(kind, sources, dest, progress, &mut |_| {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists; conflict resolution required",
        ))
    })
}

/// Runs with an explicit conflict resolver. The UI uses [`run_interactive`];
/// tests and non-interactive callers can choose deterministic behavior.
pub fn run_with_resolver(
    kind: JobKind,
    sources: &[PathBuf],
    dest: Option<&Path>,
    progress: &Progress,
    resolve: &mut dyn FnMut(&Conflict) -> io::Result<Decision>,
) -> Outcome {
    let counts: Vec<usize> = match kind {
        JobKind::Copy | JobKind::Move => sources.iter().map(|s| count_files(s)).collect(),
        JobKind::Trash | JobKind::Delete => vec![1; sources.len()],
        JobKind::Extract | JobKind::Upload | JobKind::Download | JobKind::Remove => {
            unreachable!("this kind does not run through run_with_resolver")
        }
    };
    progress.total.store(counts.iter().sum(), Ordering::Relaxed);

    let mut outcome = Outcome::default();
    let mut session = TransferSession::default();
    for (src, count) in sources.iter().zip(counts) {
        if progress.cancelled() {
            outcome.cancelled = true;
            break;
        }
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned());
        let name = name.unwrap_or_else(|| src.display().to_string());
        let before = progress.done.load(Ordering::Relaxed);
        let result = match kind {
            JobKind::Extract | JobKind::Upload | JobKind::Download | JobKind::Remove => {
                unreachable!("this kind does not run through run_with_resolver")
            }
            JobKind::Copy | JobKind::Move => {
                let Some(file_name) = src.file_name() else {
                    outcome
                        .errors
                        .push(format!("{}: source has no file name", src.display()));
                    continue;
                };
                let target = dest.unwrap_or(Path::new(".")).join(file_name);
                let mut on_file = |p: &Path| progress.step(p);
                let transfer_kind = if kind == JobKind::Copy {
                    TransferKind::Copy
                } else {
                    TransferKind::Move
                };
                let result = session.transfer(
                    transfer_kind,
                    src,
                    &target,
                    &mut on_file,
                    &|| progress.cancelled(),
                    resolve,
                );
                // A plain rename moves everything at once.
                if result.is_ok() {
                    progress.done.store(before + count, Ordering::Relaxed);
                }
                result.map_err(|e| e.to_string())
            }
            JobKind::Trash => fs_ops::trash(src)
                .and_then(|()| progress.step(src).map_err(|e| e.to_string()))
                .map(|()| TransferReport {
                    complete: true,
                    skipped: 0,
                }),
            JobKind::Delete => fs_ops::delete(src)
                .and_then(|()| progress.step(src))
                .map(|()| TransferReport {
                    complete: true,
                    skipped: 0,
                })
                .map_err(|e| e.to_string()),
        };
        match result {
            Ok(report) => {
                outcome.skipped += report.skipped;
                if report.complete {
                    outcome.done += 1;
                }
            }
            Err(_) if progress.cancelled() => {
                outcome.cancelled = true;
                break;
            }
            Err(e) => outcome.errors.push(format!("{name}: {e}")),
        }
    }
    outcome
}

/// Interactive copy/move entry point. Conflict requests are published through
/// `progress`; cancellation wakes a worker waiting for the UI.
pub fn run_interactive(
    kind: JobKind,
    sources: &[PathBuf],
    dest: Option<&Path>,
    progress: &Progress,
) -> Outcome {
    run_with_resolver(kind, sources, dest, progress, &mut |conflict| {
        progress.resolve(conflict)
    })
}

/// What to extract: `wanted` are inner paths in `file` (`""` for all of
/// it); with `into_folder` everything lands in a folder named after the
/// archive.
#[derive(Debug, Clone)]
pub struct Extraction {
    pub file: PathBuf,
    pub wanted: Vec<String>,
    pub into_folder: bool,
}

/// Extracts into a staging directory inside `dest` (same file system, so
/// the final step is a rename), then moves the results to `dest` with the
/// usual conflict resolution: nothing existing is overwritten unasked.
pub fn run_extract(
    job: &Extraction,
    dest: &Path,
    progress: &Progress,
    resolve: &mut dyn FnMut(&Conflict) -> io::Result<Decision>,
) -> Outcome {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let mut outcome = Outcome::default();
    let name = job.file.display().to_string();
    let index = match archive::Index::cached(&job.file) {
        Ok(index) => index,
        Err(e) => {
            outcome.errors.push(format!("{name}: {e}"));
            return outcome;
        }
    };
    let total = job.wanted.iter().map(|w| index.count_files(w)).sum();
    progress.total.store(total, Ordering::Relaxed);

    let staging = dest.join(format!(
        ".fyle-extract-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(e) = std::fs::create_dir(&staging) {
        outcome
            .errors
            .push(format!("{}: {e}", fs_ops::display_path(dest)));
        return outcome;
    }
    let prefix = job.into_folder.then(|| archive::stem(&job.file));
    let result = archive::extract(
        &job.file,
        &job.wanted,
        prefix.as_deref(),
        &staging,
        &mut |p| progress.step(Path::new(p)),
    );
    match result {
        Err(_) if progress.cancelled() => outcome.cancelled = true,
        Err(e) => outcome.errors.push(format!("{name}: {e}")),
        Ok(report) => {
            outcome.errors.extend(
                report
                    .skipped
                    .into_iter()
                    .map(|why| format!("{name}: skipped {why}")),
            );
            let mut staged: Vec<PathBuf> = std::fs::read_dir(&staging)
                .map(|items| items.flatten().map(|i| i.path()).collect())
                .unwrap_or_default();
            staged.sort();
            // The move phase counts its own files, from zero.
            progress.done.store(0, Ordering::Relaxed);
            // Conflicts name the archive entry, not the staged file.
            let mut resolve_shown = |conflict: &Conflict| {
                let mut shown = conflict.clone();
                if let Some(entry) = staged_entry(job, &staging, &shown.source.path) {
                    shown.source.path = PathBuf::from(archive::location(&job.file, &entry));
                }
                resolve(&shown)
            };
            let moved = run_with_resolver(
                JobKind::Move,
                &staged,
                Some(dest),
                progress,
                &mut resolve_shown,
            );
            // Files written: not top-level items, and not the skipped ones.
            outcome.skipped = moved.skipped;
            outcome.done = progress
                .done
                .load(Ordering::Relaxed)
                .saturating_sub(moved.skipped);
            outcome.cancelled = moved.cancelled;
            outcome.errors.extend(moved.errors);
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    outcome
}

/// The archive entry (inner path) that was extracted to `staged` inside
/// `staging`, for showing it instead of the staging path.
fn staged_entry(job: &Extraction, staging: &Path, staged: &Path) -> Option<String> {
    let rel = staged.strip_prefix(staging).ok()?.to_str()?;
    let prefix = job.into_folder.then(|| archive::stem(&job.file));
    archive::entry_of(rel, &job.wanted, prefix.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for file in ["src/a/1.txt", "src/a/2.txt", "src/b.txt", "single.txt"] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file).unwrap();
        }
        fs::create_dir_all(root.join("dest")).unwrap();
        dir
    }

    #[test]
    fn copies_with_file_progress() {
        let tmp = tree();
        let root = tmp.path();
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
    }

    #[test]
    fn moves_and_deletes() {
        let tmp = tree();
        let root = tmp.path();
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
    }

    #[test]
    fn paste_rules() {
        let yank = Yank {
            paths: vec![PathBuf::from("/a/x"), PathBuf::from("/a/y")],
            cut: true,
        };
        assert_eq!(yank.paste_into(Path::new("/b")), Ok(JobKind::Move));
        assert!(yank.paste_into(Path::new("/a")).is_err());
        assert!(yank.paste_into(Path::new("/a/x/sub")).is_err());
        let copy = Yank { cut: false, ..yank };
        assert_eq!(copy.paste_into(Path::new("/b")), Ok(JobKind::Copy));
    }

    fn zip_with(path: &Path, files: &[(&str, &str)]) {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, text) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extracts_into_a_folder_and_cleans_up() {
        let tmp = tree();
        let root = tmp.path();
        let zip = root.join("pack.zip");
        zip_with(&zip, &[("a/1.txt", "one"), ("b.txt", "two")]);
        let job = Extraction {
            file: zip,
            wanted: vec![String::new()],
            into_folder: true,
        };
        let dest = root.join("dest");
        let progress = Progress::default();
        let outcome = run_extract(&job, &dest, &progress, &mut |_| unreachable!());
        assert_eq!(outcome.errors, Vec::<String>::new());
        assert_eq!(outcome.done, 2);
        assert_eq!(progress.total.load(Ordering::Relaxed), 2);
        assert_eq!(
            fs::read_to_string(dest.join("pack/a/1.txt")).unwrap(),
            "one"
        );
        // No staging directory is left behind.
        let left: Vec<_> = fs::read_dir(&dest).unwrap().flatten().collect();
        assert_eq!(left.len(), 1);
    }

    #[test]
    fn extraction_asks_about_existing_files() {
        let tmp = tree();
        let root = tmp.path();
        let zip = root.join("pack.zip");
        zip_with(&zip, &[("keep.txt", "new"), ("fresh.txt", "fresh")]);
        let dest = root.join("dest");
        fs::write(dest.join("keep.txt"), "old").unwrap();
        let job = Extraction {
            file: zip,
            wanted: vec!["keep.txt".into(), "fresh.txt".into()],
            into_folder: false,
        };
        let mut asked = 0;
        let outcome = run_extract(&job, &dest, &Progress::default(), &mut |_| {
            asked += 1;
            Ok(Decision {
                choice: crate::transfer::ConflictChoice::Skip,
                apply_to_all: false,
            })
        });
        assert_eq!(asked, 1);
        assert_eq!(outcome.skipped, 1);
        assert_eq!(fs::read_to_string(dest.join("keep.txt")).unwrap(), "old");
        assert_eq!(fs::read_to_string(dest.join("fresh.txt")).unwrap(), "fresh");

        // Without an answer (no resolver), the file is kept and an error reported.
        let outcome = run_extract(&job, &dest, &Progress::default(), &mut |_| {
            Err(io::Error::other("no"))
        });
        assert_eq!(fs::read_to_string(dest.join("keep.txt")).unwrap(), "old");
        assert!(!outcome.errors.is_empty());
    }

    #[test]
    fn cancelled_extraction_leaves_nothing() {
        let tmp = tree();
        let root = tmp.path();
        let zip = root.join("pack.zip");
        zip_with(&zip, &[("a.txt", "a")]);
        let job = Extraction {
            file: zip,
            wanted: vec![String::new()],
            into_folder: false,
        };
        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let dest = root.join("dest");
        let outcome = run_extract(&job, &dest, &progress, &mut |_| unreachable!());
        assert!(outcome.cancelled);
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    }

    #[test]
    fn cancel_stops_between_files() {
        let tmp = tree();
        let root = tmp.path();
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
    }

    #[test]
    fn extraction_conflict_names_the_archive_entry() {
        let tmp = tree();
        let root = tmp.path();
        let zip = root.join("pack.zip");
        zip_with(&zip, &[("docs/keep.txt", "new"), ("top.txt", "t")]);
        let dest = root.join("dest");
        fs::write(dest.join("keep.txt"), "old").unwrap();
        let job = Extraction {
            file: zip.clone(),
            wanted: vec!["docs/keep.txt".into()],
            into_folder: false,
        };
        let mut shown = Vec::new();
        let outcome = run_extract(&job, &dest, &Progress::default(), &mut |c| {
            shown.push((c.source.path.clone(), c.source.size));
            Ok(Decision {
                choice: crate::transfer::ConflictChoice::Skip,
                apply_to_all: false,
            })
        });
        let expected = PathBuf::from(format!("{} › docs/keep.txt", zip.display()));
        assert_eq!(shown, [(expected, Some(3))]);
        assert_eq!((outcome.done, outcome.skipped), (0, 1));
    }

    #[test]
    fn staged_entries_map_back_to_archive_paths() {
        let job = |wanted: &[&str], into_folder| Extraction {
            file: PathBuf::from("/a/pack.zip"),
            wanted: wanted.iter().map(|w| w.to_string()).collect(),
            into_folder,
        };
        let staging = Path::new("/d/.fyle-extract-1-0");
        let entry = |job: &Extraction, rel: &str| staged_entry(job, staging, &staging.join(rel));
        let whole = job(&[""], true);
        assert_eq!(entry(&whole, "pack/a/1.txt").as_deref(), Some("a/1.txt"));
        assert_eq!(entry(&whole, "other"), None);
        let some = job(&["docs/x", "top.txt"], false);
        assert_eq!(entry(&some, "x/deep/f").as_deref(), Some("docs/x/deep/f"));
        assert_eq!(entry(&some, "top.txt").as_deref(), Some("top.txt"));
        assert_eq!(entry(&some, "nope"), None);
    }
}

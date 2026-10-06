//! File operations on remotes, independent of the UI: which transfers are
//! possible, conflicts with existing names, running uploads, downloads,
//! moves and removals as jobs, and the local cache that opened remote files
//! are downloaded into.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{Capabilities, Control, Location, Provider, Removal, is_remote};
use crate::jobs::{JobKind, Outcome, Progress};

/// Where a panel's folder lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Local,
    /// On the remote with this name (empty when the path is malformed).
    Remote(String),
}

impl Side {
    /// The side a panel path is on; reads no disk.
    pub fn of(path: &Path) -> Self {
        if !is_remote(path) {
            return Self::Local;
        }
        match Location::parse(path) {
            Ok(Some(location)) => Self::Remote(location.remote().to_owned()),
            _ => Self::Remote(String::new()),
        }
    }
}

/// What the UI needs to know about a configured remote.
#[derive(Clone, Debug)]
pub struct RemoteInfo {
    pub capabilities: Capabilities,
}

/// How a copy or move between two panels runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferPlan {
    /// Local folder to remote folder.
    Upload,
    /// Remote folder to local folder.
    Download,
    /// Into another folder of the same remote.
    MoveWithin,
}

/// Decides how copying (or moving) from `source` to `dest` runs. `Ok(None)`
/// means both are local, which the ordinary file jobs handle. `info` looks
/// a remote up by name. The error says why it is not possible.
pub fn plan_transfer(
    copy: bool,
    source: &Side,
    dest: &Side,
    info: impl Fn(&str) -> Option<RemoteInfo>,
) -> Result<Option<TransferPlan>, &'static str> {
    const UNKNOWN: &str = "This remote is not configured";
    match (source, dest) {
        (Side::Local, Side::Local) => Ok(None),
        (Side::Local, Side::Remote(_)) | (Side::Remote(_), Side::Local) if !copy => {
            Err("Moving between local and remote folders is not supported yet; copy instead")
        }
        (Side::Local, Side::Remote(name)) => {
            let remote = info(name).ok_or(UNKNOWN)?;
            if remote.capabilities.upload {
                Ok(Some(TransferPlan::Upload))
            } else {
                Err("This remote does not support uploading")
            }
        }
        (Side::Remote(name), Side::Local) => {
            info(name).ok_or(UNKNOWN)?;
            Ok(Some(TransferPlan::Download))
        }
        (Side::Remote(_), Side::Remote(_)) if copy => {
            Err("Copying between remote folders is not supported yet")
        }
        (Side::Remote(from), Side::Remote(to)) if from != to => {
            Err("Moving between different remotes is not supported yet")
        }
        (Side::Remote(name), Side::Remote(_)) => {
            let remote = info(name).ok_or(UNKNOWN)?;
            if remote.capabilities.move_within {
                Ok(Some(TransferPlan::MoveWithin))
            } else {
                Err("This remote does not support moving items")
            }
        }
    }
}

/// Dialog texts for removing `what` (e.g. "3 items") from the remote
/// `label`: title, question and the confirm button.
pub fn removal_prompt(
    removal: Removal,
    what: &str,
    label: &str,
) -> (&'static str, String, &'static str) {
    match removal {
        Removal::Trash => (
            "Move to trash",
            format!("Move {what} to {label} trash?"),
            "Move to trash",
        ),
        Removal::Permanent => (
            "Delete permanently",
            format!("Permanently delete {what} from {label}? This cannot be undone."),
            "Delete",
        ),
    }
}

/// A remote file or folder to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteItem {
    pub location: Location,
    pub is_dir: bool,
}

/// Which of `names` exist in the local folder `dir`. Reads the disk.
pub fn existing_local(dir: &Path, names: &[String]) -> HashSet<String> {
    names
        .iter()
        .filter(|name| std::fs::symlink_metadata(dir.join(name)).is_ok())
        .cloned()
        .collect()
}

/// The names in the remote folder `dir`. Blocking.
pub fn existing_remote(
    provider: &dyn Provider,
    dir: &Location,
    control: &Control,
) -> io::Result<HashSet<String>> {
    Ok(provider
        .list(dir, control)?
        .into_iter()
        .map(|entry| entry.name)
        .collect())
}

/// Runs `op` on each item as one job: counts items in `progress`, shows the
/// current name, collects errors and stops when cancelled.
pub fn run_items<T>(
    items: &[T],
    name: impl Fn(&T) -> String,
    progress: &Progress,
    op: impl Fn(&T, &Control) -> io::Result<()>,
) -> Outcome {
    progress.total.store(items.len(), Ordering::Relaxed);
    let set_current = |text: &str| {
        *progress
            .current
            .lock()
            .expect("progress lock is never poisoned") = text.to_owned();
    };
    let report = |current: &str, _bytes: u64| set_current(current);
    let mut outcome = Outcome::default();
    for item in items {
        if progress.cancelled() {
            outcome.cancelled = true;
            break;
        }
        let name = name(item);
        set_current(&name);
        let control = Control {
            cancel: &progress.cancel,
            progress: &report,
        };
        match op(item, &control) {
            Ok(()) => outcome.done += 1,
            Err(_) if progress.cancelled() => {
                outcome.cancelled = true;
                break;
            }
            Err(e) => outcome.errors.push(format!("{name}: {e}")),
        }
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    outcome
}

/// A copy or move between a local and a remote folder (or within one
/// remote), prepared on the UI side and run as one job.
pub enum Transfer {
    Upload {
        provider: Arc<dyn Provider>,
        sources: Vec<PathBuf>,
        dest: Location,
    },
    Download {
        provider: Arc<dyn Provider>,
        items: Vec<RemoteItem>,
        dest: PathBuf,
    },
    Move {
        provider: Arc<dyn Provider>,
        items: Vec<RemoteItem>,
        dest: Location,
    },
}

impl Transfer {
    /// The names the items get at the destination.
    fn names(&self) -> Vec<String> {
        let file_name = |path: &PathBuf| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let remote_name = |item: &RemoteItem| item.location.name().unwrap_or_default().to_owned();
        match self {
            Self::Upload { sources, .. } => sources.iter().map(file_name).collect(),
            Self::Download { items, .. } | Self::Move { items, .. } => {
                items.iter().map(remote_name).collect()
            }
        }
    }

    /// Which names already exist at the destination (a fresh listing for a
    /// remote one). Blocking.
    pub fn existing(&self, control: &Control) -> io::Result<HashSet<String>> {
        match self {
            Self::Upload { provider, dest, .. } | Self::Move { provider, dest, .. } => {
                existing_remote(provider.as_ref(), dest, control)
            }
            Self::Download { dest, .. } => Ok(existing_local(dest, &self.names())),
        }
    }

    /// The transfer without the items whose names are in `existing`, and
    /// the names that were dropped.
    pub fn skip_existing(self, existing: &HashSet<String>) -> (Self, Vec<String>) {
        fn split<T>(
            items: Vec<T>,
            names: Vec<String>,
            existing: &HashSet<String>,
        ) -> (Vec<T>, Vec<String>) {
            let mut names = names.into_iter();
            let mut free = Vec::new();
            let mut taken = Vec::new();
            for item in items {
                let name = names.next().unwrap_or_default();
                if existing.contains(&name) {
                    taken.push(name);
                } else {
                    free.push(item);
                }
            }
            (free, taken)
        }
        let names = self.names();
        match self {
            Self::Upload {
                provider,
                sources,
                dest,
            } => {
                let (sources, taken) = split(sources, names, existing);
                (
                    Self::Upload {
                        provider,
                        sources,
                        dest,
                    },
                    taken,
                )
            }
            Self::Download {
                provider,
                items,
                dest,
            } => {
                let (items, taken) = split(items, names, existing);
                (
                    Self::Download {
                        provider,
                        items,
                        dest,
                    },
                    taken,
                )
            }
            Self::Move {
                provider,
                items,
                dest,
            } => {
                let (items, taken) = split(items, names, existing);
                (
                    Self::Move {
                        provider,
                        items,
                        dest,
                    },
                    taken,
                )
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Upload { sources, .. } => sources.is_empty(),
            Self::Download { items, .. } | Self::Move { items, .. } => items.is_empty(),
        }
    }

    /// How the job shows in the jobs bar.
    pub fn job_kind(&self) -> JobKind {
        match self {
            Self::Upload { .. } => JobKind::Upload,
            Self::Download { .. } => JobKind::Download,
            Self::Move { .. } => JobKind::Move,
        }
    }

    /// Runs every item. Blocking; for a background worker.
    pub fn run(&self, progress: &Progress) -> Outcome {
        match self {
            Self::Upload {
                provider,
                sources,
                dest,
            } => upload(provider.as_ref(), sources, dest, progress),
            Self::Download {
                provider,
                items,
                dest,
            } => download(provider.as_ref(), items, dest, progress),
            Self::Move {
                provider,
                items,
                dest,
            } => move_within(provider.as_ref(), items, dest, progress),
        }
    }
}

/// Uploads local `sources` into the remote folder `dest`.
pub fn upload(
    provider: &dyn Provider,
    sources: &[PathBuf],
    dest: &Location,
    progress: &Progress,
) -> Outcome {
    run_items(
        sources,
        |path| path.display().to_string(),
        progress,
        |path, control| provider.upload(path, dest, control),
    )
}

/// Downloads `sources` into the local folder `dest`.
pub fn download(
    provider: &dyn Provider,
    sources: &[RemoteItem],
    dest: &Path,
    progress: &Progress,
) -> Outcome {
    run_items(
        sources,
        |item| item.location.panel_path().display().to_string(),
        progress,
        |item, control| provider.download(&item.location, item.is_dir, dest, control),
    )
}

/// Removes `targets` the way the remote does (see [`Capabilities::removal`]).
pub fn remove(provider: &dyn Provider, targets: &[RemoteItem], progress: &Progress) -> Outcome {
    run_items(
        targets,
        |item| item.location.panel_path().display().to_string(),
        progress,
        |item, control| provider.remove(&item.location, item.is_dir, control),
    )
}

/// Moves `targets` into the folder `dest` of the same remote.
pub fn move_within(
    provider: &dyn Provider,
    targets: &[RemoteItem],
    dest: &Location,
    progress: &Progress,
) -> Outcome {
    run_items(
        targets,
        |item| item.location.panel_path().display().to_string(),
        progress,
        |item, control| provider.move_to(&item.location, dest, control),
    )
}

/// `<cache dir>/fyle/remote`, where opened remote files are downloaded.
pub fn cache_root() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("fyle").join("remote"))
}

/// The local copy of `location` below `root`: `<root>/<remote>/<path>`.
/// Locations hold only validated names, so it cannot leave `root`.
pub fn cached_file(root: &Path, location: &Location) -> PathBuf {
    let mut path = root.join(location.remote());
    path.extend(location.parts());
    path
}

/// Downloads the file `location` into the cache below `root`, replacing an
/// older copy, and returns the local path. Blocking.
pub fn fetch(
    provider: &dyn Provider,
    location: &Location,
    root: &Path,
    control: &Control,
) -> io::Result<PathBuf> {
    let file = cached_file(root, location);
    let error = |e: io::Error| io::Error::new(e.kind(), format!("{}: {e}", file.display()));
    let dir = file
        .parent()
        .ok_or_else(|| io::Error::other("remote root is not a file"))?;
    std::fs::create_dir_all(dir).map_err(error)?;
    match std::fs::remove_file(&file) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(e)),
    }
    provider.download(location, false, dir, control)?;
    if std::fs::metadata(&file).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the download did not produce the file",
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::fs_ops::Entry;

    /// Records calls; downloads write a small file; `fail` names make
    /// the operation fail.
    #[derive(Default)]
    struct Fake {
        log: Mutex<Vec<String>>,
        fail: Option<&'static str>,
        names: Vec<&'static str>,
    }

    impl Fake {
        fn check(&self, what: &str, name: &str) -> io::Result<()> {
            self.log.lock().unwrap().push(format!("{what} {name}"));
            if self.fail == Some(name) {
                return Err(io::Error::other("boom"));
            }
            Ok(())
        }
    }

    impl Provider for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        fn label(&self) -> &str {
            "Fake"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        fn list(&self, dir: &Location, _: &Control) -> io::Result<Vec<Entry>> {
            let items = self
                .names
                .iter()
                .map(|name| crate::remote::Item {
                    name: (*name).into(),
                    is_dir: false,
                    size: 0,
                    modified: None,
                    created: None,
                })
                .collect();
            crate::remote::entries(dir, "Fake", items)
        }
        fn download(
            &self,
            source: &Location,
            _: bool,
            destination: &Path,
            _: &Control,
        ) -> io::Result<()> {
            let name = source.name().unwrap();
            self.check("download", name)?;
            std::fs::write(destination.join(name), b"data")
        }
        fn upload(&self, source: &Path, destination: &Location, _: &Control) -> io::Result<()> {
            let name = source.file_name().unwrap().to_string_lossy();
            self.check(
                "upload",
                &format!("{name}->{}", destination.panel_path().display()),
            )
        }
        fn remove(&self, target: &Location, _: bool, _: &Control) -> io::Result<()> {
            self.check("remove", target.name().unwrap())
        }
        fn move_to(&self, target: &Location, to: &Location, _: &Control) -> io::Result<()> {
            self.check(
                "move",
                &format!("{}->{}", target.name().unwrap(), to.panel_path().display()),
            )
        }
    }

    fn root() -> Location {
        Location::root("fake").unwrap()
    }

    fn item(name: &str) -> RemoteItem {
        RemoteItem {
            location: root().child(name).unwrap(),
            is_dir: false,
        }
    }

    fn info(upload: bool, move_within: bool) -> impl Fn(&str) -> Option<RemoteInfo> {
        move |name| {
            (name != "gone").then(|| RemoteInfo {
                capabilities: Capabilities {
                    upload,
                    move_within,
                    ..Default::default()
                },
            })
        }
    }

    #[test]
    fn transfers_between_panels_are_planned_or_explained() {
        let remote = |n: &str| Side::Remote(n.into());
        let plan = |copy, from: &Side, to: &Side| plan_transfer(copy, from, to, info(true, true));
        assert_eq!(plan(true, &Side::Local, &Side::Local), Ok(None));
        assert_eq!(
            plan(true, &Side::Local, &remote("a")),
            Ok(Some(TransferPlan::Upload))
        );
        assert_eq!(
            plan(true, &remote("a"), &Side::Local),
            Ok(Some(TransferPlan::Download))
        );
        assert_eq!(
            plan(false, &remote("a"), &remote("a")),
            Ok(Some(TransferPlan::MoveWithin))
        );
        assert!(
            plan(true, &remote("a"), &remote("a"))
                .unwrap_err()
                .contains("Copying")
        );
        assert!(
            plan(false, &remote("a"), &remote("b"))
                .unwrap_err()
                .contains("different remotes")
        );
        for (from, to) in [(Side::Local, remote("a")), (remote("a"), Side::Local)] {
            assert!(
                plan(false, &from, &to)
                    .unwrap_err()
                    .contains("copy instead")
            );
        }
        assert!(plan(true, &Side::Local, &remote("gone")).is_err());
        let no_upload = plan_transfer(true, &Side::Local, &remote("a"), info(false, false));
        assert_eq!(no_upload, Err("This remote does not support uploading"));
        let no_move = plan_transfer(false, &remote("a"), &remote("a"), info(true, false));
        assert!(no_move.is_err());
        assert_eq!(Side::of(Path::new("/tmp/x")), Side::Local);
        assert_eq!(Side::of(Path::new("gdrive:/x")), remote("gdrive"));
    }

    #[test]
    fn removal_text_names_the_remote_and_what_happens() {
        let (_, text, _) = removal_prompt(Removal::Trash, "3 items", "Proton Drive");
        assert_eq!(text, "Move 3 items to Proton Drive trash?");
        let (title, text, ok) = removal_prompt(Removal::Permanent, "\"a\"", "NAS");
        assert_eq!((title, ok), ("Delete permanently", "Delete"));
        assert!(text.starts_with("Permanently delete \"a\" from NAS?"));
    }

    #[test]
    fn existing_names_are_split_off() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("here"), "").unwrap();
        let names = ["here".to_string(), "absent".to_string()];
        assert_eq!(
            existing_local(dir.path(), &names),
            ["here".to_string()].into()
        );
        let cancel = AtomicBool::new(false);
        let fake = Fake {
            names: vec!["x", "y"],
            ..Default::default()
        };
        let listed = existing_remote(&fake, &root(), &Control::new(&cancel)).unwrap();
        assert_eq!(listed, ["x".to_string(), "y".to_string()].into());
    }

    #[test]
    fn transfers_drop_items_that_already_exist() {
        let provider: Arc<dyn Provider> = Arc::new(Fake {
            names: vec!["b.txt"],
            ..Default::default()
        });
        let dir = tempfile::TempDir::new().unwrap();
        let upload = Transfer::Upload {
            provider: provider.clone(),
            sources: vec![dir.path().join("a.txt"), dir.path().join("b.txt")],
            dest: root(),
        };
        let cancel = AtomicBool::new(false);
        let existing = upload.existing(&Control::new(&cancel)).unwrap();
        let (rest, taken) = upload.skip_existing(&existing);
        assert_eq!(taken, ["b.txt"]);
        assert_eq!(rest.names(), ["a.txt"]);
        assert_eq!(rest.job_kind(), JobKind::Upload);

        // Downloads compare with the local destination folder.
        std::fs::write(dir.path().join("f"), "").unwrap();
        let download = Transfer::Download {
            provider,
            items: vec![item("f"), item("g")],
            dest: dir.path().to_owned(),
        };
        let existing = download.existing(&Control::new(&cancel)).unwrap();
        let (rest, taken) = download.skip_existing(&existing);
        assert_eq!(taken, ["f"]);
        assert!(!rest.is_empty());
        let outcome = rest.run(&Progress::default());
        assert_eq!(outcome.done, 1);
        assert!(dir.path().join("g").exists());
    }

    #[test]
    fn jobs_count_items_and_collect_errors() {
        let fake = Fake {
            fail: Some("b"),
            ..Default::default()
        };
        let progress = Progress::default();
        let outcome = remove(&fake, &[item("a"), item("b"), item("c")], &progress);
        assert_eq!(outcome.done, 2);
        assert_eq!(outcome.errors, ["fake:/b: boom"]);
        assert_eq!(progress.total.load(Ordering::Relaxed), 3);
        assert_eq!(progress.done.load(Ordering::Relaxed), 3);
        assert_eq!(*progress.current.lock().unwrap(), "fake:/c");
        assert_eq!(
            *fake.log.lock().unwrap(),
            ["remove a", "remove b", "remove c"]
        );

        let moved = move_within(
            &fake,
            &[item("a")],
            &root().child("dir").unwrap(),
            &progress,
        );
        assert_eq!(moved.done, 1);
        assert_eq!(
            fake.log.lock().unwrap().last().unwrap(),
            "move a->fake:/dir"
        );
    }

    #[test]
    fn cancelling_stops_before_the_next_item() {
        let fake = Fake::default();
        let progress = Progress::default();
        progress.cancel();
        let outcome = remove(&fake, &[item("a")], &progress);
        assert!(outcome.cancelled);
        assert_eq!(outcome.done, 0);
        assert!(fake.log.lock().unwrap().is_empty());
    }

    #[test]
    fn upload_and_download_pass_each_item_to_the_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let fake = Fake::default();
        let progress = Progress::default();
        let sources = [dir.path().join("one.txt"), dir.path().join("two.txt")];
        let outcome = upload(&fake, &sources, &root().child("up").unwrap(), &progress);
        assert_eq!(outcome.done, 2);
        assert_eq!(fake.log.lock().unwrap()[0], "upload one.txt->fake:/up");

        let outcome = download(&fake, &[item("f")], dir.path(), &Progress::default());
        assert_eq!(outcome.done, 1);
        assert_eq!(std::fs::read(dir.path().join("f")).unwrap(), b"data");
    }

    #[test]
    fn opened_files_are_cached_below_the_cache_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let fake = Fake::default();
        let location = root().child("docs").unwrap().child("a b.txt").unwrap();
        assert_eq!(
            cached_file(dir.path(), &location),
            dir.path().join("fake/docs/a b.txt")
        );
        let cancel = AtomicBool::new(false);
        let control = Control::new(&cancel);
        let path = fetch(&fake, &location, dir.path(), &control).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
        // A second fetch replaces the old copy instead of skipping.
        std::fs::write(&path, "stale").unwrap();
        fetch(&fake, &location, dir.path(), &control).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
        let failing = Fake {
            fail: Some("a b.txt"),
            ..Default::default()
        };
        assert!(fetch(&failing, &location, dir.path(), &control).is_err());
        assert!(!path.exists());
    }
}

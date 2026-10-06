//! Conflict-aware copy and move operations.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferKind {
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryInfo {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub source: EntryInfo,
    pub destination: EntryInfo,
    aliases: bool,
}

impl Conflict {
    #[cfg(test)]
    pub fn from_paths(source: &Path, destination: &Path) -> io::Result<Self> {
        let source = EntryInfo::from_path(source)?;
        let destination = EntryInfo::from_path(destination)?;
        let aliases = destructive_overlap(&source.path, &destination.path)?;
        Ok(Self {
            source,
            destination,
            aliases,
        })
    }

    pub fn allows(&self, choice: ConflictChoice) -> bool {
        match choice {
            ConflictChoice::Skip | ConflictChoice::KeepBoth | ConflictChoice::Cancel => true,
            ConflictChoice::Replace => !self.aliases,
            ConflictChoice::Merge => {
                !self.aliases
                    && self.source.kind == EntryKind::Directory
                    && self.destination.kind == EntryKind::Directory
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConflictChoice {
    Skip,
    Replace,
    KeepBoth,
    Merge,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub choice: ConflictChoice,
    pub apply_to_all: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferReport {
    pub complete: bool,
    pub skipped: usize,
}

#[derive(Default)]
pub struct TransferSession {
    remembered: HashMap<(EntryKind, EntryKind), ConflictChoice>,
}

impl TransferSession {
    pub fn transfer(
        &mut self,
        kind: TransferKind,
        src: &Path,
        dst: &Path,
        on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
        cancelled: &dyn Fn() -> bool,
        resolve: &mut dyn FnMut(&Conflict) -> io::Result<Decision>,
    ) -> io::Result<TransferReport> {
        self.transfer_inner(kind, src, dst, on_file, cancelled, resolve)
    }

    fn transfer_inner(
        &mut self,
        kind: TransferKind,
        src: &Path,
        dst: &Path,
        on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
        cancelled: &dyn Fn() -> bool,
        resolve: &mut dyn FnMut(&Conflict) -> io::Result<Decision>,
    ) -> io::Result<TransferReport> {
        check_cancelled(cancelled)?;
        let source = EntryInfo::from_path(src)?;
        match EntryInfo::from_path(dst) {
            Ok(destination) => {
                let conflict = Conflict {
                    aliases: destructive_overlap(src, dst)?,
                    source,
                    destination,
                };
                let key = (conflict.source.kind, conflict.destination.kind);
                let decision = match self.remembered.get(&key).copied() {
                    Some(choice) if conflict.allows(choice) => Decision {
                        choice,
                        apply_to_all: true,
                    },
                    _ => resolve(&conflict)?,
                };
                if !conflict.allows(decision.choice) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "that conflict choice is not valid for these entries",
                    ));
                }
                if decision.apply_to_all && decision.choice != ConflictChoice::Cancel {
                    self.remembered.insert(key, decision.choice);
                }
                match decision.choice {
                    ConflictChoice::Skip => Ok(TransferReport {
                        complete: false,
                        skipped: 1,
                    }),
                    ConflictChoice::Cancel => Err(cancelled_error()),
                    ConflictChoice::KeepBoth => self.keep_both(kind, src, dst, on_file, cancelled),
                    ConflictChoice::Replace => {
                        validate_target(src, dst)?;
                        replace(kind, src, dst, on_file, cancelled)
                    }
                    ConflictChoice::Merge => {
                        self.merge(kind, src, dst, on_file, cancelled, resolve)
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                validate_target(src, dst)?;
                transfer_new(kind, src, dst, on_file, cancelled)
            }
            Err(e) => Err(e),
        }
    }

    fn keep_both(
        &mut self,
        kind: TransferKind,
        src: &Path,
        dst: &Path,
        on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<TransferReport> {
        for number in 2.. {
            check_cancelled(cancelled)?;
            let candidate = numbered_path(dst, number);
            validate_target(src, &candidate)?;
            match transfer_new(kind, src, &candidate, on_file, cancelled) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                result => return result,
            }
        }
        unreachable!()
    }

    fn merge(
        &mut self,
        kind: TransferKind,
        src: &Path,
        dst: &Path,
        on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
        cancelled: &dyn Fn() -> bool,
        resolve: &mut dyn FnMut(&Conflict) -> io::Result<Decision>,
    ) -> io::Result<TransferReport> {
        validate_target(src, dst)?;
        let mut report = TransferReport {
            complete: true,
            skipped: 0,
        };
        for item in fs::read_dir(src)? {
            check_cancelled(cancelled)?;
            let item = item?;
            let child = self.transfer_inner(
                kind,
                &item.path(),
                &dst.join(item.file_name()),
                on_file,
                cancelled,
                resolve,
            )?;
            report.complete &= child.complete;
            report.skipped += child.skipped;
        }
        if kind == TransferKind::Move && report.complete {
            fs::remove_dir(src)?;
        }
        Ok(report)
    }
}

impl EntryInfo {
    pub fn from_path(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        Ok(Self {
            path: path.to_path_buf(),
            kind,
            size: (kind == EntryKind::File).then_some(metadata.len()),
            modified: metadata.modified().ok(),
        })
    }
}

fn check_cancelled(cancelled: &dyn Fn() -> bool) -> io::Result<()> {
    if cancelled() {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "transfer cancelled")
}

fn validate_target(src: &Path, dst: &Path) -> io::Result<()> {
    let source = fs::canonicalize(src)?;
    let destination = canonical_candidate(dst)?;
    let source_is_dir = fs::symlink_metadata(src)?.is_dir();
    if source == destination || (source_is_dir && destination.starts_with(&source)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot transfer an entry into itself",
        ));
    }
    Ok(())
}

fn destructive_overlap(src: &Path, dst: &Path) -> io::Result<bool> {
    if same_entry(src, dst)? {
        return Ok(true);
    }
    let source = fs::canonicalize(src)?;
    let destination = canonical_candidate(dst)?;
    Ok(source.starts_with(&destination) || destination.starts_with(&source))
}

fn canonical_candidate(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(original) => {
            let Some(parent) = path.parent() else {
                return Err(original);
            };
            let Some(name) = path.file_name() else {
                return Err(original);
            };
            Ok(fs::canonicalize(parent)?.join(name))
        }
    }
}

fn same_entry(a: &Path, b: &Path) -> io::Result<bool> {
    let a_metadata = fs::symlink_metadata(a)?;
    let b_metadata = fs::symlink_metadata(b)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(a_metadata.dev() == b_metadata.dev() && a_metadata.ino() == b_metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = (a_metadata, b_metadata);
        Ok(fs::canonicalize(a)? == fs::canonicalize(b)?)
    }
}

fn numbered_path(path: &Path, number: usize) -> PathBuf {
    let name = path.file_name().unwrap_or_else(|| OsStr::new("item"));
    let as_path = Path::new(name);
    let mut numbered = OsString::from(as_path.file_stem().unwrap_or(name));
    numbered.push(format!(" ({number})"));
    if let Some(extension) = as_path.extension() {
        numbered.push(".");
        numbered.push(extension);
    }
    path.with_file_name(numbered)
}

fn transfer_new(
    kind: TransferKind,
    src: &Path,
    dst: &Path,
    on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<TransferReport> {
    check_cancelled(cancelled)?;
    match kind {
        TransferKind::Copy => copy_new(src, dst, on_file, cancelled)?,
        TransferKind::Move => move_new(src, dst, on_file, cancelled)?,
    }
    Ok(TransferReport {
        complete: true,
        skipped: 0,
    })
}

fn copy_new(
    src: &Path,
    dst: &Path,
    on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<()> {
    check_cancelled(cancelled)?;
    let metadata = fs::symlink_metadata(src)?;
    if metadata.file_type().is_symlink() {
        copy_symlink(src, dst)?;
        if let Err(error) = on_file(src) {
            let _ = remove_entry(dst);
            return Err(error);
        }
        Ok(())
    } else if metadata.is_dir() {
        fs::create_dir(dst)?;
        let result = (|| {
            for item in fs::read_dir(src)? {
                check_cancelled(cancelled)?;
                let item = item?;
                copy_new(
                    &item.path(),
                    &dst.join(item.file_name()),
                    on_file,
                    cancelled,
                )?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = remove_entry(dst);
        }
        result
    } else if metadata.is_file() {
        copy_file_new(src, dst)?;
        if let Err(error) = on_file(src) {
            let _ = fs::remove_file(dst);
            return Err(error);
        }
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "copying this entry type is not supported",
        ))
    }
}

fn copy_file_new(src: &Path, dst: &Path) -> io::Result<()> {
    let mut input = fs::File::open(src)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)?;
    let result = io::copy(&mut input, &mut output)
        .and_then(|_| output.flush())
        .and_then(|_| fs::set_permissions(dst, input.metadata()?.permissions()));
    if result.is_err() {
        drop(output);
        let _ = fs::remove_file(dst);
    }
    result
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(src)?, dst)
}

#[cfg(not(unix))]
fn copy_symlink(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "copying symbolic links is not supported on this platform",
    ))
}

fn move_new(
    src: &Path,
    dst: &Path,
    on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<()> {
    match rename_no_replace(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_new(src, dst, on_file, cancelled)?;
            remove_entry(src)
        }
        Err(e) => Err(e),
    }
}

#[cfg(target_os = "linux")]
fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let src = std::ffi::CString::new(src.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in source path"))?;
    let dst = std::ffi::CString::new(dst.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in destination path"))?;
    // SAFETY: `src` and `dst` are NUL-terminated CStrings that outlive the call,
    // so both pointers are valid; AT_FDCWD makes them ordinary paths and
    // RENAME_NOREPLACE is a valid flag. The call only reads the strings.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            src.as_ptr(),
            libc::AT_FDCWD,
            dst.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_no_replace(src: &Path, dst: &Path) -> io::Result<()> {
    if fs::symlink_metadata(dst).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    fs::rename(src, dst)
}

fn replace(
    kind: TransferKind,
    src: &Path,
    dst: &Path,
    on_file: &mut dyn FnMut(&Path) -> io::Result<()>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<TransferReport> {
    check_cancelled(cancelled)?;
    if kind == TransferKind::Copy {
        let staged = temporary_sibling(dst, "new")?;
        if let Err(error) = copy_new(src, &staged, on_file, cancelled) {
            let _ = remove_entry(&staged);
            return Err(error);
        }
        check_cancelled(cancelled).inspect_err(|_| {
            let _ = remove_entry(&staged);
        })?;
        install_replacement(&staged, dst)?;
    } else {
        let backup = temporary_sibling(dst, "old")?;
        rename_no_replace(dst, &backup)?;
        match move_new(src, dst, on_file, cancelled) {
            Ok(()) => remove_entry(&backup)?,
            Err(error) => {
                let _ = remove_entry(dst);
                let _ = rename_no_replace(&backup, dst);
                return Err(error);
            }
        }
    }
    Ok(TransferReport {
        complete: true,
        skipped: 0,
    })
}

fn install_replacement(staged: &Path, dst: &Path) -> io::Result<()> {
    let backup = temporary_sibling(dst, "old")?;
    rename_no_replace(dst, &backup)?;
    match rename_no_replace(staged, dst) {
        Ok(()) => remove_entry(&backup),
        Err(error) => {
            let _ = rename_no_replace(&backup, dst);
            let _ = remove_entry(staged);
            Err(error)
        }
    }
}

fn temporary_sibling(path: &Path, label: &str) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..1000 {
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!(".fyle-transfer-{label}-{}-{number}", std::process::id());
        let candidate = parent.join(name);
        match fs::symlink_metadata(&candidate) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a temporary transfer name",
    ))
}

fn remove_entry(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path).or_else(|first| fs::remove_dir(path).map_err(|_| first))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn decision(choice: ConflictChoice) -> impl FnMut(&Conflict) -> io::Result<Decision> {
        move |_| {
            Ok(Decision {
                choice,
                apply_to_all: false,
            })
        }
    }

    fn transfer(
        session: &mut TransferSession,
        kind: TransferKind,
        src: &Path,
        dst: &Path,
        choice: ConflictChoice,
    ) -> io::Result<TransferReport> {
        session.transfer(
            kind,
            src,
            dst,
            &mut |_| Ok(()),
            &|| false,
            &mut decision(choice),
        )
    }

    #[test]
    fn nonconflicting_copy_and_move_preserve_tree_contents() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("source/a.txt"), "a");
        write(&dir.path().join("source/sub/b.txt"), "b");
        let mut session = TransferSession::default();
        let copied = transfer(
            &mut session,
            TransferKind::Copy,
            &dir.path().join("source"),
            &dir.path().join("copy"),
            ConflictChoice::Cancel,
        )
        .unwrap();
        assert!(copied.complete);
        assert_eq!(
            fs::read_to_string(dir.path().join("copy/sub/b.txt")).unwrap(),
            "b"
        );
        assert!(dir.path().join("source/a.txt").is_file());

        let moved = transfer(
            &mut session,
            TransferKind::Move,
            &dir.path().join("copy"),
            &dir.path().join("moved"),
            ConflictChoice::Cancel,
        )
        .unwrap();
        assert!(moved.complete);
        assert!(!dir.path().join("copy").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("moved/a.txt")).unwrap(),
            "a"
        );
    }

    #[test]
    fn skip_preserves_both_entries_for_copy_and_move() {
        for kind in [TransferKind::Copy, TransferKind::Move] {
            let dir = TempDir::new().unwrap();
            write(&dir.path().join("source"), "new");
            write(&dir.path().join("destination"), "old");
            let report = transfer(
                &mut TransferSession::default(),
                kind,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                ConflictChoice::Skip,
            )
            .unwrap();
            assert_eq!(
                report,
                TransferReport {
                    complete: false,
                    skipped: 1
                }
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("source")).unwrap(),
                "new"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("destination")).unwrap(),
                "old"
            );
        }
    }

    #[test]
    fn replace_works_for_copy_and_move() {
        for kind in [TransferKind::Copy, TransferKind::Move] {
            let dir = TempDir::new().unwrap();
            write(&dir.path().join("source"), "new");
            write(&dir.path().join("destination"), "old");
            let report = transfer(
                &mut TransferSession::default(),
                kind,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                ConflictChoice::Replace,
            )
            .unwrap();
            assert!(report.complete);
            assert_eq!(
                fs::read_to_string(dir.path().join("destination")).unwrap(),
                "new"
            );
            assert_eq!(
                dir.path().join("source").exists(),
                kind == TransferKind::Copy
            );
        }
    }

    #[test]
    fn keep_both_retains_extension_and_avoids_existing_names() {
        for kind in [TransferKind::Copy, TransferKind::Move] {
            let dir = TempDir::new().unwrap();
            write(&dir.path().join("report.txt"), "source");
            write(&dir.path().join("out/report.txt"), "first");
            write(&dir.path().join("out/report (2).txt"), "second");
            transfer(
                &mut TransferSession::default(),
                kind,
                &dir.path().join("report.txt"),
                &dir.path().join("out/report.txt"),
                ConflictChoice::KeepBoth,
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(dir.path().join("out/report.txt")).unwrap(),
                "first"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("out/report (2).txt")).unwrap(),
                "second"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("out/report (3).txt")).unwrap(),
                "source"
            );
            assert_eq!(
                dir.path().join("report.txt").exists(),
                kind == TransferKind::Copy
            );
        }
    }

    #[test]
    fn directory_merge_copies_new_entries_and_resolves_nested_conflicts() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("source/new.txt"), "new");
        write(&dir.path().join("source/sub/shared.txt"), "source");
        write(
            &dir.path().join("destination/sub/shared.txt"),
            "destination",
        );
        let mut choices = [
            ConflictChoice::Merge,
            ConflictChoice::Merge,
            ConflictChoice::Replace,
        ]
        .into_iter();
        let report = TransferSession::default()
            .transfer(
                TransferKind::Copy,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                &mut |_| Ok(()),
                &|| false,
                &mut |_| {
                    Ok(Decision {
                        choice: choices.next().unwrap(),
                        apply_to_all: false,
                    })
                },
            )
            .unwrap();
        assert!(report.complete);
        assert_eq!(
            fs::read_to_string(dir.path().join("destination/new.txt")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("destination/sub/shared.txt")).unwrap(),
            "source"
        );
        assert!(dir.path().join("source/sub/shared.txt").is_file());
    }

    #[test]
    fn move_merge_with_skip_leaves_only_skipped_source_entries() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("source/moved.txt"), "moved");
        write(&dir.path().join("source/sub/skipped.txt"), "source");
        write(
            &dir.path().join("destination/sub/skipped.txt"),
            "destination",
        );
        let mut choices = [
            ConflictChoice::Merge,
            ConflictChoice::Merge,
            ConflictChoice::Skip,
        ]
        .into_iter();
        let report = TransferSession::default()
            .transfer(
                TransferKind::Move,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                &mut |_| Ok(()),
                &|| false,
                &mut |_| {
                    Ok(Decision {
                        choice: choices.next().unwrap(),
                        apply_to_all: false,
                    })
                },
            )
            .unwrap();
        assert_eq!(
            report,
            TransferReport {
                complete: false,
                skipped: 1
            }
        );
        assert!(!dir.path().join("source/moved.txt").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("source/sub/skipped.txt")).unwrap(),
            "source"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("destination/sub/skipped.txt")).unwrap(),
            "destination"
        );
    }

    #[test]
    fn cancel_choice_and_mid_copy_return_interrupted() {
        let dir = TempDir::new().unwrap();
        for kind in [TransferKind::Copy, TransferKind::Move] {
            write(&dir.path().join("source"), "source");
            write(&dir.path().join("destination"), "destination");
            let error = transfer(
                &mut TransferSession::default(),
                kind,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                ConflictChoice::Cancel,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert_eq!(
                fs::read_to_string(dir.path().join("source")).unwrap(),
                "source"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("destination")).unwrap(),
                "destination"
            );
        }

        write(&dir.path().join("tree/a"), "a");
        write(&dir.path().join("tree/b"), "b");
        let files = Cell::new(0);
        let error = TransferSession::default()
            .transfer(
                TransferKind::Copy,
                &dir.path().join("tree"),
                &dir.path().join("tree-copy"),
                &mut |_| {
                    files.set(files.get() + 1);
                    Ok(())
                },
                &|| files.get() == 1,
                &mut decision(ConflictChoice::Cancel),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(dir.path().join("tree/a").exists() && dir.path().join("tree/b").exists());
        assert!(!dir.path().join("tree-copy").exists());
    }

    #[test]
    fn apply_all_is_reused_only_for_the_same_kind_pair() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("s1"), "s1");
        write(&dir.path().join("d1"), "d1");
        write(&dir.path().join("s2"), "s2");
        write(&dir.path().join("d2"), "d2");
        fs::create_dir(dir.path().join("folder")).unwrap();
        write(&dir.path().join("file-at-folder-target"), "file");
        let prompts = Cell::new(0);
        let mut resolve = |_: &Conflict| {
            prompts.set(prompts.get() + 1);
            Ok(Decision {
                choice: ConflictChoice::Skip,
                apply_to_all: true,
            })
        };
        let mut session = TransferSession::default();
        for (src, dst) in [
            ("s1", "d1"),
            ("s2", "d2"),
            ("file-at-folder-target", "folder"),
        ] {
            session
                .transfer(
                    TransferKind::Copy,
                    &dir.path().join(src),
                    &dir.path().join(dst),
                    &mut |_| Ok(()),
                    &|| false,
                    &mut resolve,
                )
                .unwrap();
        }
        assert_eq!(prompts.get(), 2);
    }

    #[test]
    fn same_path_keep_both_is_safe_but_destructive_choices_are_rejected() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("file.txt"), "contents");
        transfer(
            &mut TransferSession::default(),
            TransferKind::Copy,
            &dir.path().join("file.txt"),
            &dir.path().join("file.txt"),
            ConflictChoice::KeepBoth,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("file (2).txt")).unwrap(),
            "contents"
        );

        for choice in [ConflictChoice::Replace, ConflictChoice::Merge] {
            let error = transfer(
                &mut TransferSession::default(),
                TransferKind::Copy,
                &dir.path().join("file.txt"),
                &dir.path().join("file.txt"),
                choice,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt")).unwrap(),
            "contents"
        );
    }

    #[test]
    fn rejects_descendant_targets_and_replacing_an_ancestor() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("source/child/file"), "data");
        let error = transfer(
            &mut TransferSession::default(),
            TransferKind::Copy,
            &dir.path().join("source"),
            &dir.path().join("source/child/new"),
            ConflictChoice::Cancel,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        let error = transfer(
            &mut TransferSession::default(),
            TransferKind::Move,
            &dir.path().join("source/child"),
            &dir.path().join("source"),
            ConflictChoice::Replace,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read_to_string(dir.path().join("source/child/file")).unwrap(),
            "data"
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonical_aliases_and_hard_links_disallow_replacement() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("tree/file"), "data");
        std::os::unix::fs::symlink(dir.path().join("tree"), dir.path().join("alias")).unwrap();
        let conflict =
            Conflict::from_paths(&dir.path().join("tree"), &dir.path().join("alias")).unwrap();
        assert!(!conflict.allows(ConflictChoice::Replace));
        assert!(!conflict.allows(ConflictChoice::Merge));

        fs::hard_link(dir.path().join("tree/file"), dir.path().join("hard-link")).unwrap();
        let conflict =
            Conflict::from_paths(&dir.path().join("tree/file"), &dir.path().join("hard-link"))
                .unwrap();
        assert!(!conflict.allows(ConflictChoice::Replace));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_destination_symlink_is_a_conflict_and_target_is_untouched() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("source"), "new");
        std::os::unix::fs::symlink("missing-target", dir.path().join("destination")).unwrap();
        let prompts = Cell::new(0);
        TransferSession::default()
            .transfer(
                TransferKind::Copy,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                &mut |_| Ok(()),
                &|| false,
                &mut |_| {
                    prompts.set(prompts.get() + 1);
                    Ok(Decision {
                        choice: ConflictChoice::Replace,
                        apply_to_all: false,
                    })
                },
            )
            .unwrap();
        assert_eq!(prompts.get(), 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("destination")).unwrap(),
            "new"
        );
        assert!(!dir.path().join("missing-target").exists());
    }

    #[test]
    fn failed_replace_copy_preserves_source_and_destination() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("source"), "new");
        write(&dir.path().join("destination"), "old");
        let error = TransferSession::default()
            .transfer(
                TransferKind::Copy,
                &dir.path().join("source"),
                &dir.path().join("destination"),
                &mut |_| Err(io::Error::other("progress failed")),
                &|| false,
                &mut decision(ConflictChoice::Replace),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            fs::read_to_string(dir.path().join("source")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("destination")).unwrap(),
            "old"
        );
    }
}

//! Filesystem operations and adapters for native metadata and account names.

use std::fs;
use std::io;
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, SystemTime};

use crate::entry::{Attrs, Entry};
use crate::links::LinkState;

/// Converts native metadata to display attributes, preserving platform behavior.
pub fn attrs_from_metadata(meta: &fs::Metadata) -> Attrs {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Attrs {
            mode: Some(meta.mode()),
            uid: Some(meta.uid()),
            readonly: meta.permissions().readonly(),
            created: meta.created().ok(),
        }
    }
    #[cfg(not(unix))]
    {
        Attrs {
            mode: None,
            uid: None,
            readonly: meta.permissions().readonly(),
            created: meta.created().ok(),
        }
    }
}

/// Resolves a user id from `/etc/passwd`, falling back to its number or empty.
pub fn owner_name(uid: Option<u32>) -> String {
    match uid {
        Some(uid) => account_name(uid, AccountKind::User).unwrap_or_else(|| uid.to_string()),
        None => String::new(),
    }
}

/// Resolves Unix account names for a user and group ID on a background worker.
pub fn account_names(uid: u32, gid: u32) -> (Option<String>, Option<String>) {
    (
        account_name(uid, AccountKind::User),
        account_name(gid, AccountKind::Group),
    )
}

#[derive(Clone, Copy)]
enum AccountKind {
    User,
    Group,
}

fn account_name(id: u32, kind: AccountKind) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static USERS: OnceLock<HashMap<u32, String>> = OnceLock::new();
    static GROUPS: OnceLock<HashMap<u32, String>> = OnceLock::new();
    let names = match kind {
        AccountKind::User => USERS.get_or_init(|| read_accounts("/etc/passwd")),
        AccountKind::Group => GROUPS.get_or_init(|| read_accounts("/etc/group")),
    };
    names.get(&id).cloned()
}

fn read_accounts(path: &str) -> std::collections::HashMap<u32, String> {
    #[cfg(unix)]
    {
        const MAX_ACCOUNT_FILE_BYTES: u64 = 1024 * 1024;
        let mut data = String::new();
        let Ok(file) = fs::File::open(path) else {
            return Default::default();
        };
        if file
            .take(MAX_ACCOUNT_FILE_BYTES + 1)
            .read_to_string(&mut data)
            .is_err()
            || data.len() as u64 > MAX_ACCOUNT_FILE_BYTES
        {
            return Default::default();
        }
        parse_accounts(&data)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Default::default()
    }
}

fn parse_accounts(data: &str) -> std::collections::HashMap<u32, String> {
    data.lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            if name.is_empty() {
                return None;
            }
            let id = fields.nth(1)?;
            Some((id.parse::<u32>().ok()?, name.to_string()))
        })
        .collect()
}
/// The directory ".." leads to. On Windows a drive root leads to the drive
/// list, represented by an empty path.
pub fn parent_of(dir: &Path) -> Option<PathBuf> {
    match dir.parent() {
        Some(parent) => Some(parent.to_path_buf()),
        None if cfg!(windows) && !dir.as_os_str().is_empty() => Some(PathBuf::new()),
        None => None,
    }
}

/// Human-readable location for the panel header.
pub fn display_path(dir: &Path) -> String {
    if dir.as_os_str().is_empty() {
        "Drives".into()
    } else {
        dir.display().to_string()
    }
}

/// Lists `dir` (unsorted), with a ".." entry when there is a parent.
#[cfg(test)]
pub fn read_dir(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    read_dir_cancellable(dir, show_hidden, None)
}

/// Lists a directory while checking cancellation between entries and before
/// metadata probes. Returns `Interrupted` when the worker has been cancelled.
pub fn read_dir_cancellable(
    dir: &Path,
    show_hidden: bool,
    cancel: Option<&AtomicBool>,
) -> io::Result<Vec<Entry>> {
    let mut check_cancel = || {
        if cancel.is_some_and(|flag| flag.load(AtomicOrdering::Relaxed)) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "listing cancelled",
            ))
        } else {
            Ok(())
        }
    };
    read_dir_with_check(dir, show_hidden, &mut check_cancel)
}

fn read_dir_with_check(
    dir: &Path,
    show_hidden: bool,
    check_cancel: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<Vec<Entry>> {
    check_cancel()?;
    if dir.as_os_str().is_empty() {
        return Ok(drives());
    }
    let mut entries = Vec::new();
    if let Some(parent) = parent_of(dir) {
        entries.push(Entry {
            name: "..".into(),
            path: parent,
            is_dir: true,
            is_parent: true,
            size: Some(0),
            modified: None,
            attrs: Attrs::default(),
            link: Default::default(),
        });
    }
    for item in fs::read_dir(dir)? {
        check_cancel()?;
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        let path = item.path();
        check_cancel()?;
        // Follow symlinks so linked directories can be entered.
        let followed = fs::metadata(&path);
        let meta = followed
            .as_ref()
            .ok()
            .cloned()
            .or_else(|| item.metadata().ok());
        if !show_hidden && is_hidden(&name, meta.as_ref()) {
            continue;
        }
        // Only shown links are probed.
        let link = if item.file_type().is_ok_and(|kind| kind.is_symlink()) {
            LinkState::probe(&path, &followed)
        } else {
            LinkState::NotLink
        };
        entries.push(Entry {
            name,
            path,
            is_dir: meta.as_ref().is_some_and(|m| m.is_dir()),
            is_parent: false,
            size: meta.as_ref().map(|m| m.len()),
            attrs: meta.as_ref().map(attrs_from_metadata).unwrap_or_default(),
            link,
            modified: meta.and_then(|m| m.modified().ok()),
        });
    }
    Ok(entries)
}

#[allow(unused_variables)]
fn is_hidden(name: &str, meta: Option<&fs::Metadata>) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if meta.is_some_and(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0) {
            return true;
        }
    }
    name.starts_with('.')
}

fn drives() -> Vec<Entry> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\", letter as char)))
            .filter(|root| root.exists())
            .map(|root| Entry {
                name: root.display().to_string(),
                path: root,
                is_dir: true,
                is_parent: false,
                size: Some(0),
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            })
            .collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

#[cfg(test)]
/// Called after each file (or symlink) is copied; an error aborts.
pub type OnFile<'a> = &'a mut dyn FnMut(&Path) -> io::Result<()>;

/// Copies a file or directory tree to `dst`. Refuses to overwrite.
#[cfg(test)]
pub fn copy(src: &Path, dst: &Path) -> io::Result<()> {
    copy_with(src, dst, &mut |_| Ok(()))
}

#[cfg(test)]
/// [`copy`] reporting each copied file to `on_file`.
pub fn copy_with(src: &Path, dst: &Path, on_file: OnFile) -> io::Result<()> {
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", dst.display()),
        ));
    }
    if dst.starts_with(src) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy a directory into itself",
        ));
    }
    copy_inner(src, dst, on_file)
}

#[cfg(test)]
fn copy_inner(src: &Path, dst: &Path, on_file: OnFile) -> io::Result<()> {
    // Unix recreates symlinks; elsewhere (creating them needs privileges on
    // Windows) the link target's content is copied.
    #[cfg(unix)]
    if fs::symlink_metadata(src)?.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)?;
        return on_file(src);
    }
    if fs::metadata(src)?.is_dir() {
        fs::create_dir(dst)?;
        for item in fs::read_dir(src)? {
            let item = item?;
            copy_inner(&item.path(), &dst.join(item.file_name()), on_file)?;
        }
        Ok(())
    } else {
        fs::copy(src, dst)?;
        on_file(src)
    }
}

/// Moves a file or directory; falls back to copy + delete across filesystems.
#[cfg(test)]
pub fn move_to(src: &Path, dst: &Path) -> io::Result<()> {
    move_with(src, dst, &mut |_| Ok(()))
}

#[cfg(test)]
/// [`move_to`]; `on_file` sees each file copied when the move has to copy.
pub fn move_with(src: &Path, dst: &Path, on_file: OnFile) -> io::Result<()> {
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", dst.display()),
        ));
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_with(src, dst, on_file)?;
            delete(src)
        }
        Err(e) => Err(e),
    }
}

/// Moves a file or directory to the platform's trash / recycle bin.
pub fn trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Deletes a file or directory tree. A symlink is removed, not its target.
pub fn delete(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        // Windows directory symlinks and junctions need remove_dir.
        return fs::remove_file(path).or_else(|_| fs::remove_dir(path));
    }
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn format_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Formats as "YYYY-MM-DD HH:MM" in local time, or an em dash if out of range.
pub fn format_time(time: SystemTime) -> String {
    format_time_with(time, "%Y-%m-%d %H:%M")
}

/// Formats a timestamp with seconds in local time, or an em dash if invalid.
pub fn format_time_with_seconds(time: SystemTime) -> String {
    format_time_with(time, "%Y-%m-%d %H:%M:%S")
}

fn format_time_with(time: SystemTime, format: &str) -> String {
    let Ok(timestamp) = jiff::Timestamp::try_from(time) else {
        return "—".into();
    };
    timestamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime(format)
        .to_string()
}

/// Converts a Jiff timestamp to the standard library representation when possible.
pub fn system_time_from_timestamp(timestamp: jiff::Timestamp) -> Option<SystemTime> {
    const NANOS_PER_SECOND: i128 = 1_000_000_000;
    let nanos = timestamp.as_nanosecond();
    if nanos >= 0 {
        let seconds = u64::try_from(nanos / NANOS_PER_SECOND).ok()?;
        let subsec_nanos = u32::try_from(nanos % NANOS_PER_SECOND).ok()?;
        SystemTime::UNIX_EPOCH.checked_add(Duration::new(seconds, subsec_nanos))
    } else {
        let nanos = nanos.checked_neg()?;
        let seconds = u64::try_from(nanos / NANOS_PER_SECOND).ok()?;
        let subsec_nanos = u32::try_from(nanos % NANOS_PER_SECOND).ok()?;
        SystemTime::UNIX_EPOCH.checked_sub(Duration::new(seconds, subsec_nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::sort_entries;
    use std::cell::Cell;
    use tempfile::TempDir;

    #[test]
    fn missing_owner_has_empty_fallback() {
        assert_eq!(owner_name(None), "");
    }

    #[test]
    fn account_parser_uses_correct_id_field_and_ignores_invalid_rows() {
        let users = parse_accounts(
            "alice:x:1001:2001:Alice:/home/alice:/bin/sh\ninvalid:x:oops:2002::/:/bin/sh\n:x:1003:2003::/:/bin/sh\n",
        );
        assert_eq!(users.get(&1001).map(String::as_str), Some("alice"));
        assert!(!users.contains_key(&2001));
        assert!(!users.contains_key(&1003));

        let groups = parse_accounts("staff:x:2001:alice\ninvalid:x:nope:\n");
        assert_eq!(groups.get(&2001).map(String::as_str), Some("staff"));
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn cancellation_is_checked_between_entries_and_metadata() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("entry"), "data").unwrap();
        let checks = Cell::new(0);
        let mut check = || {
            let count = checks.get() + 1;
            checks.set(count);
            if count == 3 {
                Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
            } else {
                Ok(())
            }
        };
        let error = read_dir_with_check(dir.path(), false, &mut check).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(checks.get(), 3);
    }

    #[test]
    fn formats_size() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(2048), "2.0 K");
    }

    #[test]
    fn copy_move_delete_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        fs::create_dir_all(base.join("a").join("sub")).unwrap();
        fs::write(base.join("a").join("sub").join("f.txt"), "hi").unwrap();

        copy(&base.join("a"), &base.join("b")).unwrap();
        let copied = base.join("b").join("sub").join("f.txt");
        assert_eq!(fs::read_to_string(copied).unwrap(), "hi");
        assert!(copy(&base.join("a"), &base.join("a").join("sub").join("x")).is_err());
        assert!(copy(&base.join("a"), &base.join("b")).is_err());

        move_to(&base.join("b"), &base.join("c")).unwrap();
        assert!(!base.join("b").exists());
        assert!(base.join("c").join("sub").join("f.txt").exists());

        let mut listing = read_dir(&base, false).unwrap();
        sort_entries(&mut listing, crate::entry::SortKey::Name, false);
        let names: Vec<_> = listing.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "a", "c"]);

        delete(&base.join("c")).unwrap();
        assert!(!base.join("c").exists());
    }
}

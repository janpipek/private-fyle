use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Lists `dir`: a ".." entry (unless at root), then directories, then files,
/// each group sorted case-insensitively.
pub fn read_dir(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = item.path();
        // Follow symlinks so linked directories can be entered.
        let meta = fs::metadata(&path).or_else(|_| item.metadata()).ok();
        entries.push(Entry {
            name,
            path,
            is_dir: meta.as_ref().is_some_and(|m| m.is_dir()),
            is_parent: false,
            size: meta.as_ref().map_or(0, |m| m.len()),
            modified: meta.and_then(|m| m.modified().ok()),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if let Some(parent) = dir.parent() {
        entries.insert(
            0,
            Entry {
                name: "..".into(),
                path: parent.to_path_buf(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
            },
        );
    }
    Ok(entries)
}

/// Copies a file or directory tree to `dst`. Refuses to overwrite.
pub fn copy(src: &Path, dst: &Path) -> io::Result<()> {
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
    copy_inner(src, dst)
}

fn copy_inner(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(fs::read_link(src)?, dst);
    }
    if meta.is_dir() {
        fs::create_dir(dst)?;
        for item in fs::read_dir(src)? {
            let item = item?;
            copy_inner(&item.path(), &dst.join(item.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

/// Moves a file or directory; falls back to copy + delete across filesystems.
pub fn move_to(src: &Path, dst: &Path) -> io::Result<()> {
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", dst.display()),
        ));
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy(src, dst)?;
            delete(src)
        }
        Err(e) => Err(e),
    }
}

pub fn delete(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
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

/// Formats as "YYYY-MM-DD HH:MM" (UTC).
pub fn format_time(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_time() {
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(format_time(t), "2023-11-14 22:13");
    }

    #[test]
    fn formats_size() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(2048), "2.0 K");
    }

    #[test]
    fn copy_move_delete_tree() {
        let base = std::env::temp_dir().join(format!("fyle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("a/sub")).unwrap();
        fs::write(base.join("a/sub/f.txt"), "hi").unwrap();

        copy(&base.join("a"), &base.join("b")).unwrap();
        assert_eq!(fs::read_to_string(base.join("b/sub/f.txt")).unwrap(), "hi");
        assert!(copy(&base.join("a"), &base.join("a/sub/x")).is_err());
        assert!(copy(&base.join("a"), &base.join("b")).is_err());

        move_to(&base.join("b"), &base.join("c")).unwrap();
        assert!(!base.join("b").exists() && base.join("c/sub/f.txt").exists());

        let listing = read_dir(&base, false).unwrap();
        let names: Vec<_> = listing.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "a", "c"]);

        delete(&base.join("c")).unwrap();
        assert!(!base.join("c").exists());
        fs::remove_dir_all(&base).unwrap();
    }
}

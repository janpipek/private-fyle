use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub attrs: Attrs,
}

/// Platform attributes shown in optional columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    /// Unix mode bits (type and permissions).
    pub mode: Option<u32>,
    /// Unix owner id.
    pub uid: Option<u32>,
    pub readonly: bool,
    /// Creation time, if the file system records it.
    pub created: Option<SystemTime>,
}

impl Attrs {
    pub fn from_meta(meta: &fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                mode: Some(meta.mode()),
                uid: Some(meta.uid()),
                readonly: meta.permissions().readonly(),
                created: meta.created().ok(),
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                mode: None,
                uid: None,
                readonly: meta.permissions().readonly(),
                created: meta.created().ok(),
            }
        }
    }

    /// `drwxr-xr-x` on Unix; `r-` / `rw` elsewhere.
    pub fn permissions(&self, is_dir: bool) -> String {
        let Some(mode) = self.mode else {
            return if self.readonly {
                "r-".into()
            } else {
                "rw".into()
            };
        };
        let kind = if is_dir { 'd' } else { '-' };
        let bits = ['r', 'w', 'x'];
        std::iter::once(kind)
            .chain((0..9).map(|i| {
                let bit = 1 << (8 - i);
                if mode & bit != 0 { bits[i % 3] } else { '-' }
            }))
            .collect()
    }

    /// Owner's user name (from /etc/passwd), else the numeric id.
    pub fn owner(&self) -> String {
        match self.uid {
            Some(uid) => user_name(uid).unwrap_or_else(|| uid.to_string()),
            None => String::new(),
        }
    }
}

/// User names by uid, read once from /etc/passwd.
fn user_name(uid: u32) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static USERS: OnceLock<HashMap<u32, String>> = OnceLock::new();
    USERS
        .get_or_init(|| {
            fs::read_to_string("/etc/passwd")
                .unwrap_or_default()
                .lines()
                .filter_map(|line| {
                    let mut fields = line.split(':');
                    let name = fields.next()?.to_string();
                    let uid = fields.nth(1)?.parse().ok()?;
                    Some((uid, name))
                })
                .collect()
        })
        .get(&uid)
        .cloned()
}

impl Entry {
    /// The extension shown in its own column; empty for directories.
    pub fn ext(&self) -> &str {
        if self.is_dir {
            return "";
        }
        match self.name.rfind('.') {
            Some(0) | None => "",
            Some(ix) => &self.name[ix + 1..],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Ext,
    Size,
    Modified,
}

/// Sorts with ".." first and directories before files, regardless of order.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, descending: bool) {
    entries.sort_by(|a, b| {
        b.is_parent
            .cmp(&a.is_parent)
            .then_with(|| b.is_dir.cmp(&a.is_dir))
            .then_with(|| {
                let by_name = || natural_cmp(&a.name, &b.name);
                let ord = match key {
                    SortKey::Name => by_name(),
                    SortKey::Ext => a
                        .ext()
                        .to_lowercase()
                        .cmp(&b.ext().to_lowercase())
                        .then_with(by_name),
                    SortKey::Size => a.size.cmp(&b.size).then_with(by_name),
                    SortKey::Modified => a.modified.cmp(&b.modified).then_with(by_name),
                };
                if descending { ord.reverse() } else { ord }
            })
    });
}

/// Case-insensitive "natural" order: runs of digits compare by numeric
/// value, so `file2` < `file10`. Ties fall back to plain comparison so the
/// order is total (`a01` vs `a1`, `A` vs `a`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        digits.push(c);
                    }
                    digits
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let ord = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(c), Some(d)) => {
                let ord = c.to_lowercase().cmp(d.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                x.next();
                y.next();
            }
        }
    }
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
pub fn read_dir(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
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
            size: 0,
            modified: None,
            attrs: Attrs::default(),
        });
    }
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        let path = item.path();
        // Follow symlinks so linked directories can be entered.
        let meta = fs::metadata(&path).or_else(|_| item.metadata()).ok();
        if !show_hidden && is_hidden(&name, meta.as_ref()) {
            continue;
        }
        entries.push(Entry {
            name,
            path,
            is_dir: meta.as_ref().is_some_and(|m| m.is_dir()),
            is_parent: false,
            size: meta.as_ref().map_or(0, |m| m.len()),
            attrs: meta.as_ref().map(Attrs::from_meta).unwrap_or_default(),
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
                size: 0,
                modified: None,
                attrs: Default::default(),
            })
            .collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Called after each file (or symlink) is copied; an error aborts.
pub type OnFile<'a> = &'a mut dyn FnMut(&Path) -> io::Result<()>;

/// Copies a file or directory tree to `dst`. Refuses to overwrite.
#[cfg(test)]
pub fn copy(src: &Path, dst: &Path) -> io::Result<()> {
    copy_with(src, dst, &mut |_| Ok(()))
}

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

/// Formats as "YYYY-MM-DD HH:MM" in local time.
pub fn format_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_parent: name == "..",
            size,
            modified: None,
            attrs: Default::default(),
        }
    }

    #[test]
    fn natural_order() {
        let mut names = vec![
            "file10", "File2", "file1", "a01", "a1", "b", "A", "x9y", "x10y",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "A", "a01", "a1", "b", "file1", "File2", "file10", "x9y", "x10y"
            ]
        );
        assert_eq!(
            natural_cmp("99999999999999999999", "100000000000000000000"),
            Ordering::Less
        );
    }

    #[test]
    fn formats_permissions() {
        let attrs = |mode| Attrs {
            mode: Some(mode),
            uid: None,
            readonly: false,
            created: None,
        };
        assert_eq!(attrs(0o100644).permissions(false), "-rw-r--r--");
        assert_eq!(attrs(0o40755).permissions(true), "drwxr-xr-x");
        let windows = Attrs {
            mode: None,
            uid: None,
            readonly: true,
            created: None,
        };
        assert_eq!(windows.permissions(false), "r-");
        assert_eq!(windows.owner(), "");
    }

    #[test]
    fn formats_size() {
        assert_eq!(format_size(12), "12 B");
        assert_eq!(format_size(2048), "2.0 K");
    }

    #[test]
    fn extension() {
        assert_eq!(entry("a.tar.gz", false, 0).ext(), "gz");
        assert_eq!(entry(".bashrc", false, 0).ext(), "");
        assert_eq!(entry("dir.d", true, 0).ext(), "");
    }

    #[test]
    fn sorting_keeps_parent_and_dirs_first() {
        let mut entries = vec![
            entry("b.txt", false, 5),
            entry("A.rs", false, 10),
            entry("zdir", true, 0),
            entry("..", true, 0),
            entry("adir", true, 0),
        ];
        let names = |e: &[Entry]| e.iter().map(|e| e.name.clone()).collect::<Vec<_>>();

        sort_entries(&mut entries, SortKey::Name, false);
        assert_eq!(names(&entries), ["..", "adir", "zdir", "A.rs", "b.txt"]);

        sort_entries(&mut entries, SortKey::Name, true);
        assert_eq!(names(&entries), ["..", "zdir", "adir", "b.txt", "A.rs"]);

        sort_entries(&mut entries, SortKey::Size, false);
        assert_eq!(names(&entries), ["..", "adir", "zdir", "b.txt", "A.rs"]);
    }

    #[test]
    fn copy_move_delete_tree() {
        let base = std::env::temp_dir().join(format!("fyle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
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
        sort_entries(&mut listing, SortKey::Name, false);
        let names: Vec<_> = listing.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "a", "c"]);

        delete(&base.join("c")).unwrap();
        assert!(!base.join("c").exists());
        fs::remove_dir_all(&base).unwrap();
    }
}

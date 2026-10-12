//! Shared file entries, attributes and pure sorting rules.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::links::LinkState;

/// A file or directory row shared by local, remote and archive listings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Display name, or a relative path for search hits.
    pub name: String,
    /// Compatibility path spelling used by native operations and the UI.
    pub path: PathBuf,
    /// Whether the entry represents a directory.
    pub is_dir: bool,
    /// Whether the row is the parent-directory entry.
    pub is_parent: bool,
    /// File size when the source provides it.
    pub size: Option<u64>,
    /// Last-modified time when available.
    pub modified: Option<SystemTime>,
    /// Native attributes displayed in optional columns.
    pub attrs: Attrs,
    /// Symlink state; the other fields describe the link's target.
    pub link: LinkState,
}

/// Platform attributes shown in optional columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    /// Unix mode bits (type and permissions).
    pub mode: Option<u32>,
    /// Unix owner id.
    pub uid: Option<u32>,
    /// Whether the platform reports the entry as read-only.
    pub readonly: bool,
    /// Creation time, if the file system records it.
    pub created: Option<SystemTime>,
}

impl Attrs {
    /// Formats permissions for display using Unix mode bits when available.
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
}

impl Entry {
    /// The extension shown in its own column; empty for directories.
    /// The extension, without the dot; empty for directories and names
    /// like `.gitignore`. Search results name a relative path, so only its
    /// last component counts.
    pub fn ext(&self) -> &str {
        if self.is_dir {
            return "";
        }
        let file = self.name.rfind('/').map_or(0, |ix| ix + 1);
        match self.name[file..].rfind('.') {
            Some(0) | None => "",
            Some(ix) => &self.name[file + ix + 1..],
        }
    }

    /// The name without `.ext` (see [`Entry::ext`]), for when the extension
    /// is shown in its own column: `main.rs` → `main`, `.gitignore` stays.
    pub fn stem(&self) -> &str {
        match self.ext() {
            "" => &self.name,
            ext => &self.name[..self.name.len() - ext.len() - 1],
        }
    }
}

/// Sort field used by listings and the panel model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    /// Sort by natural name order.
    Name,
    /// Sort by extension, then natural name order.
    Ext,
    /// Sort by size, then natural name order.
    Size,
    /// Sort by modification time, then natural name order.
    Modified,
}

/// Panel sorting choices, including their direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    /// Field used to order entries.
    pub key: SortKey,
    /// Whether entries within each directory/parent group are descending.
    pub descending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self {
            key: SortKey::Name,
            descending: false,
        }
    }
}

/// Sorts with `..` first and directories before files, regardless of order.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, descending: bool) {
    entries.sort_by(|a, b| compare_entries(a, b, key, descending));
}

/// Compares two rows using the same ordering as [`sort_entries`].
pub fn compare_entries(a: &Entry, b: &Entry, key: SortKey, descending: bool) -> Ordering {
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
                SortKey::Size => size_order(a.size, b.size).then_with(by_name),
                SortKey::Modified => a.modified.cmp(&b.modified).then_with(by_name),
            };
            if descending { ord.reverse() } else { ord }
        })
}

/// Orders known sizes numerically, with unknown sizes last in ascending order.
pub fn size_order(a: Option<u64>, b: Option<u64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Case-insensitive natural order; digit runs compare numerically.
/// Ties fall back to plain comparison so the order is total (`a01` vs `a1`,
/// `A` vs `a`).
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

#[cfg(test)]
mod tests {
    use super::{Attrs, Entry, Sort, SortKey, natural_cmp, size_order, sort_entries};
    use std::cmp::Ordering;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_parent: name == "..",
            size: Some(size),
            modified: None,
            attrs: Attrs::default(),
            link: Default::default(),
        }
    }

    #[test]
    fn sort_defaults_to_ascending_name() {
        assert_eq!(
            Sort::default(),
            Sort {
                key: SortKey::Name,
                descending: false
            }
        );
    }

    #[test]
    fn size_order_keeps_unknown_sizes_distinct_and_deterministic() {
        let mut rows = vec![
            entry("unknown", false, 0),
            entry("zero", false, 0),
            entry("large", false, 9),
        ];
        rows[0].size = None;
        sort_entries(&mut rows, SortKey::Size, false);
        assert_eq!(
            rows.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["zero", "large", "unknown"]
        );
        sort_entries(&mut rows, SortKey::Size, true);
        assert_eq!(
            rows.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["unknown", "large", "zero"]
        );
        assert_eq!(rows[2].size, Some(0));
        assert_eq!(size_order(Some(1), None), Ordering::Less);
        assert_eq!(size_order(None, Some(1)), Ordering::Greater);
        assert_eq!(size_order(None, None), Ordering::Equal);
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
    fn splits_extension_off_the_last_component() {
        let split = |name: &str, is_dir| {
            let e = entry(name, is_dir, 0);
            (e.stem().to_string(), e.ext().to_string())
        };
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(split("main.rs", false), pair("main", "rs"));
        assert_eq!(split("archive.tar.gz", false), pair("archive.tar", "gz"));
        assert_eq!(split(".gitignore", false), pair(".gitignore", ""));
        assert_eq!(split(".config.toml", false), pair(".config", "toml"));
        assert_eq!(split("Makefile", false), pair("Makefile", ""));
        assert_eq!(split("trailing.", false), pair("trailing.", ""));
        assert_eq!(split("photos.2024", true), pair("photos.2024", ""));
        assert_eq!(split("src/v1.2/notes", false), pair("src/v1.2/notes", ""));
        assert_eq!(split("docs/.env", false), pair("docs/.env", ""));
        assert_eq!(split("a.b/c.txt", false), pair("a.b/c", "txt"));
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
        let read_only = Attrs {
            mode: None,
            uid: None,
            readonly: true,
            created: None,
        };
        assert_eq!(read_only.permissions(false), "r-");
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
    fn extension() {
        assert_eq!(entry("a.tar.gz", false, 0).ext(), "gz");
        assert_eq!(entry(".bashrc", false, 0).ext(), "");
        assert_eq!(entry("dir.d", true, 0).ext(), "");
    }
}

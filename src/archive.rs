//! Archives (zip, tar, tar.gz) as virtual directories, UI-independent.
//!
//! An [`Index`] lists an archive's entries as a tree of [`Node`]s; the
//! folder model shows it like a directory. [`crate::location::Location::Archive`]
//! is the typed identity for an archive member; [`locate`] remains a compatibility
//! helper for native path spellings used at input boundaries. Entries are
//! read-only: the only way out is [`extract`], which never writes outside its
//! destination.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    Tar,
    TarGz,
}

/// The archive format of a file name, if fyle can browse it. Only the
/// name counts (not .docx or .epub, which are zip files but are opened
/// as documents).
pub fn format_of(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let ends = |exts: &[&str]| exts.iter().any(|e| name.ends_with(e));
    if ends(&[".tar.gz", ".tgz", ".crate"]) {
        Some(Format::TarGz)
    } else if ends(&[".tar"]) {
        Some(Format::Tar)
    } else if ends(&[".zip", ".jar", ".war", ".whl"]) {
        Some(Format::Zip)
    } else {
        None
    }
}

/// The archive's file name without its format suffix (`a.tar.gz` → `a`).
pub fn stem(file: &Path) -> String {
    let name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let lower = name.to_lowercase();
    for suffix in [
        ".tar.gz", ".tgz", ".crate", ".tar", ".zip", ".jar", ".war", ".whl",
    ] {
        if lower.ends_with(suffix) && name.len() > suffix.len() {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    name
}

/// The path of `inner` inside `file`.
#[cfg(test)]
pub fn virtual_path(file: &Path, inner: &str) -> PathBuf {
    if inner.is_empty() {
        file.to_path_buf()
    } else {
        file.join(inner)
    }
}

/// The panel header form of an archive location: `file.zip › inner/dir`
/// (just the file for the archive root).
pub fn location(file: &Path, inner: &str) -> String {
    format!("{} › {inner}", file.display())
        .trim_end()
        .to_string()
}

/// Splits a path inside an archive into the archive file and the inner
/// path (empty for the archive itself): the nearest ancestor that is a
/// regular archive file. None for paths that are not inside one.
pub fn locate(path: &Path) -> Option<(PathBuf, String)> {
    for ancestor in path.ancestors() {
        match fs::metadata(ancestor) {
            Ok(meta) if meta.is_dir() => return None,
            Ok(meta) if meta.is_file() => {
                format_of(ancestor)?;
                let inner = inner_of(ancestor, path)?;
                return Some((ancestor.to_path_buf(), inner));
            }
            _ => {}
        }
    }
    None
}

/// `path` relative to `file` as an inner path (`a/b`); None if it is not
/// below it or has odd components.
pub fn inner_of(file: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(file).ok()?;
    let mut parts = Vec::new();
    for component in rest.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Result of cleaning an entry name.
#[derive(Debug, PartialEq, Eq)]
pub enum Cleaned {
    /// The archive root itself (`./`, `/` is *not* this: see `Unsafe`).
    Root,
    /// A relative path of normal components joined by `/`.
    Path(String),
    /// Absolute, or containing `..`, a drive prefix or NUL: never used.
    Unsafe,
}

/// Cleans an entry name: drops `.` and empty components and treats `\` as
/// a separator; rejects absolute names and `..`.
pub fn clean(raw: &str) -> Cleaned {
    if raw.contains('\0') || raw.starts_with('/') || raw.starts_with('\\') {
        return Cleaned::Unsafe;
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Cleaned::Unsafe,
            p if p.len() >= 2
                && p.as_bytes()[1] == b':'
                && p.as_bytes()[0].is_ascii_alphabetic() =>
            {
                return Cleaned::Unsafe; // C:
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        Cleaned::Root
    } else {
        Cleaned::Path(parts.join("/"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// A symbolic link and its target (empty when only listing a zip).
    Symlink(String),
    /// Hard links, devices, fifos: listed but never extracted.
    Other,
}

/// One archive entry as read from the archive, name not yet cleaned.
struct Raw {
    name: String,
    kind: Kind,
    size: u64,
    modified: Option<SystemTime>,
    mode: Option<u32>,
    /// Why the data cannot be read (encrypted, unsupported compression).
    problem: Option<String>,
}

/// An entry in the [`Index`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    /// Cleaned path inside the archive.
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub mode: Option<u32>,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
}

/// Archives kept by [`Index::cached`]; the least recently used goes first.
const CACHE_KEPT: usize = 4;

type Cache = Vec<(PathBuf, u64, Option<SystemTime>, Arc<Index>)>;

fn cache() -> std::sync::MutexGuard<'static, Cache> {
    static CACHE: Mutex<Cache> = Mutex::new(Vec::new());
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The tree of an archive's entries. Directories that only appear in the
/// path of other entries exist implicitly.
#[derive(Debug, Default)]
pub struct Index {
    nodes: BTreeMap<String, Node>,
    children: HashMap<String, Vec<String>>,
    /// Entries left out because their names were unsafe.
    pub rejected: usize,
}

impl Index {
    pub fn read(file: &Path) -> io::Result<Self> {
        Self::read_inner(file, None)
    }

    fn read_inner(file: &Path, cancel: Option<&AtomicBool>) -> io::Result<Self> {
        let format = format_of(file).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "not a supported archive")
        })?;
        let mut index = Self::default();
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(cancelled());
        }
        walk(file, format, cancel, &|_| false, &mut |raw, _| {
            index.add(raw);
            Ok(true)
        })?;
        index.finish();
        Ok(index)
    }

    /// The index of `file`, shared: re-read only when the file's size or
    /// modification time changed since it was last read. Reading a large
    /// tar.gz means decompressing all of it, so listings, reloads and
    /// previews should go through here.
    pub fn cached(file: &Path) -> io::Result<Arc<Self>> {
        Self::cached_inner(file, None)
    }

    /// Returns a cached archive index or reads one with cancellation checks.
    /// A partial index is never inserted into the shared cache.
    pub fn cached_cancellable(file: &Path, cancel: &AtomicBool) -> io::Result<Arc<Self>> {
        Self::cached_inner(file, Some(cancel))
    }

    fn cached_inner(file: &Path, cancel: Option<&AtomicBool>) -> io::Result<Arc<Self>> {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(cancelled());
        }
        let meta = fs::metadata(file)?;
        let (len, modified) = (meta.len(), meta.modified().ok());
        let mut cache = cache();
        if let Some(i) = cache
            .iter()
            .position(|(p, l, m, _)| p == file && *l == len && *m == modified)
        {
            let hit = cache.remove(i);
            let index = hit.3.clone();
            cache.push(hit);
            return Ok(index);
        }
        cache.retain(|(p, ..)| p != file);
        drop(cache);
        let index = Arc::new(match cancel {
            Some(cancel) => Self::read_inner(file, Some(cancel))?,
            None => Self::read(file)?,
        });
        let mut cache = self::cache();
        cache.push((file.to_path_buf(), len, modified, index.clone()));
        if cache.len() > CACHE_KEPT {
            cache.remove(0);
        }
        Ok(index)
    }

    /// Whether [`Index::cached`] would return `file` without reading it.
    #[cfg(test)]
    pub fn is_cached(file: &Path) -> bool {
        let Ok(meta) = fs::metadata(file) else {
            return false;
        };
        let (len, modified) = (meta.len(), meta.modified().ok());
        cache()
            .iter()
            .any(|(p, l, m, _)| p == file && *l == len && *m == modified)
    }

    fn add(&mut self, raw: Raw) {
        let path = match clean(&raw.name) {
            Cleaned::Root => return,
            Cleaned::Unsafe => {
                self.rejected += 1;
                return;
            }
            Cleaned::Path(path) => path,
        };
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        // A later entry replaces an earlier one, except that a file never
        // replaces a directory (the directory's contents would be lost).
        if self.nodes.get(&path).is_some_and(|n| n.is_dir()) && raw.kind != Kind::Dir {
            return;
        }
        self.nodes.insert(
            path.clone(),
            Node {
                name,
                path,
                kind: raw.kind,
                size: raw.size,
                modified: raw.modified,
                mode: raw.mode,
            },
        );
    }

    /// Adds the implicit directories and the child lists.
    fn finish(&mut self) {
        let paths: Vec<String> = self.nodes.keys().cloned().collect();
        for path in paths {
            let mut current = path.as_str();
            while let Some((parent, _)) = current.rsplit_once('/') {
                if !self.nodes.contains_key(parent) {
                    self.nodes.insert(
                        parent.to_string(),
                        Node {
                            name: parent.rsplit('/').next().unwrap_or(parent).to_string(),
                            path: parent.to_string(),
                            kind: Kind::Dir,
                            size: 0,
                            modified: None,
                            mode: None,
                        },
                    );
                }
                current = parent;
            }
        }
        // A file where a directory is needed: the files below it win.
        let dirs: Vec<String> = self
            .nodes
            .keys()
            .filter_map(|p| p.rsplit_once('/').map(|(parent, _)| parent.to_string()))
            .collect();
        for dir in dirs {
            if let Some(node) = self.nodes.get_mut(&dir) {
                node.kind = Kind::Dir;
            }
        }
        for path in self.nodes.keys() {
            let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
            self.children
                .entry(parent.to_string())
                .or_default()
                .push(path.clone());
        }
    }

    /// The entry at `path` (cleaned, `""` is the root directory).
    pub fn get(&self, path: &str) -> Option<&Node> {
        self.nodes.get(path)
    }

    pub fn is_dir(&self, path: &str) -> bool {
        path.is_empty() || self.get(path).is_some_and(Node::is_dir)
    }

    /// The direct children of the directory `dir`, in path order.
    pub fn list(&self, dir: &str) -> impl Iterator<Item = &Node> {
        self.children
            .get(dir)
            .into_iter()
            .flatten()
            .filter_map(|path| self.nodes.get(path))
    }

    /// Files (and links) at or below `path`, for progress totals.
    pub fn count_files(&self, path: &str) -> usize {
        if path.is_empty() {
            return self.nodes.values().filter(|n| !n.is_dir()).count();
        }
        let below = format!("{path}/");
        self.nodes
            .iter()
            .filter(|(p, n)| !n.is_dir() && (p.as_str() == path || p.starts_with(&below)))
            .count()
    }

    /// Direct children of a directory: (dirs, files, size of files).
    pub fn summary(&self, dir: &str) -> (usize, usize, u64) {
        let (mut dirs, mut files, mut size) = (0, 0, 0);
        for node in self.list(dir) {
            if node.is_dir() {
                dirs += 1;
            } else {
                files += 1;
                size += node.size;
            }
        }
        (dirs, files, size)
    }
}

fn from_unix(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn zip_time(time: zip::DateTime) -> Option<SystemTime> {
    let days = days_from_civil(time.year().into(), time.month().into(), time.day().into());
    let secs = days * 86400
        + i64::from(time.hour()) * 3600
        + i64::from(time.minute()) * 60
        + i64::from(time.second());
    u64::try_from(secs).ok().map(from_unix)
}

/// Calls `f` on every entry, with a reader for its data (empty unless
/// `with_data`) until `f` returns false.
/// Calls `f` for each entry. Entry data is only readable (zip) for entries
/// whose raw name `wants` accepts; the others get an empty reader.
fn walk(
    file: &Path,
    format: Format,
    cancel: Option<&AtomicBool>,
    wants: &dyn Fn(&str) -> bool,
    f: &mut dyn FnMut(Raw, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let reader = BufReader::new(File::open(file)?);
    match cancel {
        Some(cancel) => walk_reader(
            CancellableReader {
                inner: reader,
                cancel,
            },
            format,
            wants,
            f,
        ),
        None => walk_reader(reader, format, wants, f),
    }
}

fn walk_reader<R: Read + Seek>(
    reader: R,
    format: Format,
    wants: &dyn Fn(&str) -> bool,
    f: &mut dyn FnMut(Raw, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    match format {
        Format::Zip => walk_zip(reader, wants, f),
        Format::Tar => walk_tar(reader, f),
        Format::TarGz => walk_tar(flate2::read::GzDecoder::new(reader), f),
    }
}

fn walk_zip<R: Read + Seek>(
    reader: R,
    wants: &dyn Fn(&str) -> bool,
    f: &mut dyn FnMut(Raw, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    const S_IFMT: u32 = 0o170000;
    const S_IFLNK: u32 = 0o120000;
    let mut archive = zip::ZipArchive::new(reader).map_err(io::Error::other)?;
    for i in 0..archive.len() {
        // Raw access lists even entries that cannot be decompressed.
        let (name, mut raw, is_link) = {
            let entry = archive.by_index_raw(i).map_err(io::Error::other)?;
            let mode = entry.unix_mode();
            let is_link = !entry.is_dir() && mode.is_some_and(|m| m & S_IFMT == S_IFLNK);
            let raw = Raw {
                name: entry.name().to_string(),
                kind: if entry.is_dir() {
                    Kind::Dir
                } else if is_link {
                    Kind::Symlink(String::new())
                } else {
                    Kind::File
                },
                size: entry.size(),
                modified: entry.last_modified().and_then(zip_time),
                mode: mode.map(|m| m & 0o777),
                problem: None,
            };
            (entry.name().to_string(), raw, is_link)
        };
        let proceed = if raw.kind == Kind::Dir || !wants(&name) {
            f(raw, &mut io::empty())?
        } else {
            match archive.by_index(i) {
                Ok(mut entry) => {
                    if is_link {
                        let mut target = String::new();
                        (&mut entry).take(4096).read_to_string(&mut target)?;
                        raw.kind = Kind::Symlink(target);
                    }
                    f(raw, &mut entry)?
                }
                Err(e) => {
                    raw.problem = Some(format!("cannot be read ({e})"));
                    f(raw, &mut io::empty())?
                }
            }
        };
        if !proceed {
            break;
        }
    }
    Ok(())
}

struct CancellableReader<'a> {
    inner: BufReader<File>,
    cancel: &'a AtomicBool,
}

impl Read for CancellableReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        self.inner.read(buffer)
    }
}

impl Seek for CancellableReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        self.inner.seek(position)
    }
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "archive listing cancelled")
}

fn walk_tar(
    reader: impl Read,
    f: &mut dyn FnMut(Raw, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_string_lossy().into_owned();
        let header = entry.header();
        let kind = match header.entry_type() {
            t if t.is_dir() => Kind::Dir,
            t if t.is_file() => Kind::File,
            t if t.is_symlink() => Kind::Symlink(
                entry
                    .link_name()?
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            _ => Kind::Other,
        };
        let header = entry.header();
        let raw = Raw {
            name,
            kind,
            size: header.size().unwrap_or(0),
            modified: header.mtime().ok().map(from_unix),
            mode: header.mode().ok().map(|m| m & 0o777),
            problem: None,
        };
        if !f(raw, &mut entry)? {
            break;
        }
    }
    Ok(())
}

/// Copies the file at `inner` to `out`, at most `limit` bytes. Errors if
/// it is not a file or is larger than `limit`.
pub fn read_file(file: &Path, inner: &str, limit: u64, out: &mut dyn Write) -> io::Result<()> {
    let format = format_of(file)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a supported archive"))?;
    let mut found = false;
    let is_wanted = |name: &str| clean(name) == Cleaned::Path(inner.to_string());
    walk(file, format, None, &is_wanted, &mut |raw, data| {
        if !is_wanted(&raw.name) || raw.kind != Kind::File {
            return Ok(true);
        }
        if let Some(problem) = &raw.problem {
            return Err(io::Error::other(format!("{inner} {problem}")));
        }
        if raw.size > limit {
            return Err(io::Error::other(format!(
                "{} is too large to preview from an archive ({} > {} bytes); extract it first",
                inner, raw.size, limit
            )));
        }
        io::copy(&mut data.take(limit), out)?;
        found = true;
        Ok(false)
    })?;
    if found {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{inner} is not a file in the archive"),
        ))
    }
}

mod extraction;
pub use extraction::{ExtractReport, entry_of};

/// Extracts selected archive entries using the safe extraction implementation.
pub fn extract(
    file: &Path,
    wanted: &[String],
    prefix: Option<&str>,
    dest: &Path,
    step: &mut dyn FnMut(&str) -> io::Result<()>,
) -> io::Result<ExtractReport> {
    extraction::extract(file, wanted, prefix, dest, step)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn cancellable_archive_reader_stops_between_reads() {
        use super::CancellableReader;
        use std::fs::File;
        use std::io::{BufReader, Read};
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = TempDir::new().unwrap();
        let file = dir.path().join("reader.bin");
        fs::write(&file, b"archive data").unwrap();
        let cancel = AtomicBool::new(false);
        let mut reader = CancellableReader {
            inner: BufReader::new(File::open(file).unwrap()),
            cancel: &cancel,
        };
        let mut byte = [0; 1];
        assert_eq!(reader.read(&mut byte).unwrap(), 1);
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            reader.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }

    /// An entry to put in a test archive.
    pub(super) enum Item<'a> {
        File(&'a str, &'a str),
        Dir(&'a str),
        #[cfg(unix)]
        Link(&'a str, &'a str),
    }

    pub(super) fn make_zip(path: &Path, items: &[Item]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for item in items {
            match item {
                Item::File(name, text) => {
                    zip.start_file(*name, opts).unwrap();
                    zip.write_all(text.as_bytes()).unwrap();
                }
                Item::Dir(name) => zip.add_directory(*name, opts).unwrap(),
                #[cfg(unix)]
                Item::Link(name, target) => zip.add_symlink(*name, *target, opts).unwrap(),
            }
        }
        zip.finish().unwrap();
    }

    /// Writes a tar stream; names are set in the raw header so that unsafe
    /// ones (`..`, absolute) can be built.
    pub(super) fn tar_bytes(items: &[Item]) -> Vec<u8> {
        let mut out = Vec::new();
        for item in items {
            let mut header = tar::Header::new_gnu();
            let (name, kind, link, body): (&str, tar::EntryType, Option<&str>, &[u8]) = match item {
                Item::File(name, text) => (*name, tar::EntryType::Regular, None, text.as_bytes()),
                Item::Dir(name) => (*name, tar::EntryType::Directory, None, &b""[..]),
                #[cfg(unix)]
                Item::Link(name, target) => {
                    (*name, tar::EntryType::Symlink, Some(*target), &b""[..])
                }
            };
            header.set_entry_type(kind);
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(1_700_000_000);
            {
                let raw = header.as_old_mut();
                raw.name[..name.len()].copy_from_slice(name.as_bytes());
                if let Some(link) = link {
                    raw.linkname[..link.len()].copy_from_slice(link.as_bytes());
                }
            }
            header.set_cksum();
            out.extend_from_slice(header.as_bytes());
            out.extend_from_slice(body);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0; 1024]);
        out
    }

    pub(super) fn make_tar(path: &Path, items: &[Item], gz: bool) {
        let bytes = tar_bytes(items);
        if gz {
            let mut enc = flate2::write::GzEncoder::new(
                File::create(path).unwrap(),
                flate2::Compression::default(),
            );
            enc.write_all(&bytes).unwrap();
            enc.finish().unwrap();
        } else {
            fs::write(path, bytes).unwrap();
        }
    }

    pub(super) const SAMPLE: &[Item] = &[
        Item::File("top.txt", "top"),
        Item::File("docs/readme.md", "# hi"),
        Item::File("docs/img/a.png", "png"),
        Item::Dir("empty/"),
    ];

    pub(super) fn all_formats(dir: &TempDir, items: &[Item]) -> Vec<PathBuf> {
        let zip = dir.path().join("t.zip");
        let tar = dir.path().join("t.tar");
        let tgz = dir.path().join("t.tar.gz");
        make_zip(&zip, items);
        make_tar(&tar, items, false);
        make_tar(&tgz, items, true);
        vec![zip, tar, tgz]
    }

    #[test]
    fn detects_formats_by_name() {
        assert_eq!(format_of(Path::new("/a/B.ZIP")), Some(Format::Zip));
        assert_eq!(format_of(Path::new("x.tar")), Some(Format::Tar));
        assert_eq!(format_of(Path::new("x.tar.gz")), Some(Format::TarGz));
        assert_eq!(format_of(Path::new("x.tgz")), Some(Format::TarGz));
        assert_eq!(format_of(Path::new("x.docx")), None);
        assert_eq!(format_of(Path::new("x.gz")), None);
    }

    #[test]
    fn cleans_entry_names() {
        assert_eq!(clean("a/b"), Cleaned::Path("a/b".into()));
        assert_eq!(clean("./a//b/"), Cleaned::Path("a/b".into()));
        assert_eq!(clean("a\\b"), Cleaned::Path("a/b".into()));
        assert_eq!(clean("./"), Cleaned::Root);
        for bad in [
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a/..",
            "\\x",
            "C:/x",
            "a\0b",
        ] {
            assert_eq!(clean(bad), Cleaned::Unsafe, "{bad}");
        }
    }

    #[test]
    fn lists_a_tree_in_every_format() {
        let dir = TempDir::new().unwrap();
        for file in all_formats(&dir, SAMPLE) {
            let index = Index::read(&file).unwrap();
            let names =
                |d: &str| -> Vec<String> { index.list(d).map(|n| n.name.clone()).collect() };
            assert_eq!(names(""), ["docs", "empty", "top.txt"], "{file:?}");
            assert_eq!(names("docs"), ["img", "readme.md"], "{file:?}");
            assert_eq!(names("docs/img"), ["a.png"]);
            assert!(index.is_dir("docs") && index.is_dir("empty") && index.is_dir(""));
            assert!(!index.is_dir("top.txt") && !index.is_dir("nope"));
            assert_eq!(index.get("docs/readme.md").unwrap().size, 4);
            assert_eq!(index.count_files(""), 3);
            assert_eq!(index.count_files("docs"), 2);
            assert_eq!(index.count_files("top.txt"), 1);
            assert_eq!(index.summary(""), (2, 1, 3));
            assert_eq!(index.rejected, 0);
        }
    }

    #[test]
    fn unsafe_names_are_not_listed() {
        let dir = TempDir::new().unwrap();
        let tar = dir.path().join("evil.tar");
        make_tar(
            &tar,
            &[
                Item::File("../evil.txt", "x"),
                Item::File("/abs.txt", "x"),
                Item::File("ok.txt", "x"),
            ],
            false,
        );
        let index = Index::read(&tar).unwrap();
        let names: Vec<_> = index.list("").map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["ok.txt"]);
        assert_eq!(index.rejected, 2);
    }

    #[test]
    fn locates_paths_inside_archives() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("a.zip");
        make_zip(&zip, SAMPLE);
        assert_eq!(locate(&zip), Some((zip.clone(), String::new())));
        assert_eq!(
            locate(&zip.join("docs/img")),
            Some((zip.clone(), "docs/img".into()))
        );
        assert_eq!(locate(dir.path()), None);
        assert_eq!(locate(&dir.path().join("missing/x")), None);
        assert_eq!(virtual_path(&zip, ""), zip);
        assert_eq!(virtual_path(&zip, "a/b"), zip.join("a/b"));
        assert_eq!(inner_of(&zip, &zip.join("a/b")), Some("a/b".into()));
        assert_eq!(inner_of(&zip, &dir.path().join("other")), None);
    }

    #[test]
    fn reads_one_file_with_a_limit() {
        let dir = TempDir::new().unwrap();
        for file in all_formats(&dir, SAMPLE) {
            let mut out = Vec::new();
            read_file(&file, "docs/readme.md", 100, &mut out).unwrap();
            assert_eq!(out, b"# hi");
            let err = read_file(&file, "docs/readme.md", 2, &mut Vec::new()).unwrap_err();
            assert!(err.to_string().contains("too large"), "{err}");
            assert!(read_file(&file, "docs", 100, &mut Vec::new()).is_err());
            assert!(read_file(&file, "nope", 100, &mut Vec::new()).is_err());
        }
    }

    #[test]
    fn cached_index_is_reused_until_the_file_changes() {
        let dir = TempDir::new().unwrap();
        let zip = dir.path().join("c.zip");
        make_zip(&zip, &[Item::File("a.txt", "a")]);
        let first = Index::cached(&zip).unwrap();
        assert!(Arc::ptr_eq(&first, &Index::cached(&zip).unwrap()));
        make_zip(&zip, &[Item::File("a.txt", "a"), Item::File("b.txt", "bb")]);
        let second = Index::cached(&zip).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(second.get("b.txt").is_some());
    }

    #[test]
    fn stems() {
        assert_eq!(stem(Path::new("/x/a.tar.gz")), "a");
        assert_eq!(stem(Path::new("b.ZIP")), "b");
        assert_eq!(stem(Path::new("c.v1.tgz")), "c.v1");
        assert_eq!(stem(Path::new(".zip")), ".zip");
    }
}

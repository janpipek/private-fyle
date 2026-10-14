//! File previews, UI-independent.
//!
//! A [`Registry`] holds [`Previewer`]s in priority order; the first one whose
//! [`Previewer::matches`] accepts a file's [`Probe`] produces its [`Content`].
//! `Content` is plain data; the view decides how to draw each variant.
//!
//! To support a new file type:
//! 1. implement [`Previewer`] in a new module (match on extension, name or
//!    magic bytes in [`Probe::head`]),
//! 2. register it in [`Registry::with_defaults`] before the generic
//!    fallbacks (text, binary),
//! 3. only if it needs a new kind of display, add a [`Content`] variant and a
//!    render arm in `preview_view.rs`.

mod archive;
mod binary;
mod directory;
mod document;
mod image;
mod markdown;
mod media;
mod notebook;
mod parquet;
mod pdf;
mod pptx;
mod raw;
/// Preview target lifecycle and page/sheet navigation decisions.
pub mod session;
mod spreadsheet;
mod table;
pub mod text;

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub use archive::ArchivePreviewer;
pub use binary::BinaryPreviewer;
pub use directory::DirectoryPreviewer;
pub use document::DocumentPreviewer;
pub use image::ImagePreviewer;
pub use markdown::MarkdownPreviewer;
pub use media::MediaPreviewer;
pub use notebook::{Cell as NotebookCell, Notebook, NotebookPreviewer, Output as NotebookOutput};
pub use parquet::ParquetPreviewer;
pub use pdf::{PdfPreviewer, render_page};
pub use pptx::{Presentation, PresentationPreviewer, Slide};
pub use raw::RawPreviewer;
pub use spreadsheet::{Sheet, SpreadsheetPreviewer, read_sheet};
pub use table::TablePreviewer;
pub use text::TextPreviewer;

/// Bytes read up front for type detection.
const HEAD_LEN: usize = 8 * 1024;

/// Where to keep a file generated from `source` (a rendered page, a video
/// frame): in the temp directory, named after the source path and its
/// modification time, so it is reused until the source changes.
pub(crate) fn cache_file(source: &Path, ext: &str) -> io::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let modified = fs::metadata(source)?.modified()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (source, modified).hash(&mut hasher);
    let dir = std::env::temp_dir().join("fyle-preview-cache");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{:016x}.{ext}", hasher.finish())))
}

/// What a previewer can cheaply know about a file before loading it.
#[derive(Debug)]
pub struct Probe {
    pub path: PathBuf,
    /// File name, as shown.
    pub name: String,
    /// Lowercase extension without the dot; empty if none.
    pub ext: String,
    pub is_dir: bool,
    pub size: u64,
    /// The first bytes of a regular file (empty for directories).
    pub head: Vec<u8>,
}

impl Probe {
    pub fn read(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        let mut head = Vec::new();
        if meta.is_file() {
            File::open(path)?
                .take(HEAD_LEN as u64)
                .read_to_end(&mut head)?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            ext: path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            is_dir: meta.is_dir(),
            size: meta.len(),
            head,
        })
    }

    /// Heuristic: no NUL bytes and valid UTF-8 (ignoring a sequence cut off
    /// at the end of the head).
    pub fn looks_like_text(&self) -> bool {
        if self.head.contains(&0) {
            return false;
        }
        match std::str::from_utf8(&self.head) {
            Ok(_) => true,
            Err(e) => e.error_len().is_none(),
        }
    }
}

/// Preview data, independent of how it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Text, with a syntax-highlighting language name if known.
    Text {
        text: String,
        language: Option<&'static str>,
        truncated: bool,
    },
    /// An image the view loads from `path` (a PDF's rendered first page).
    Image { path: PathBuf },
    /// PDF pages rendered one at a time by the preview view. Metadata stays
    /// in the model for later use by file properties.
    Pdf {
        path: PathBuf,
        pages: usize,
        metadata: Vec<(String, String)>,
    },
    /// A workbook: sheet names, with the first sheet's sample. The view loads
    /// other sheets on demand with [`read_sheet`].
    Spreadsheet {
        path: PathBuf,
        sheets: Vec<String>,
        first: Box<Sheet>,
    },
    /// Summary of a directory's direct children.
    Directory {
        dirs: usize,
        files: usize,
        files_size: u64,
    },
    /// Hex dump of the start of a binary file.
    Binary { dump: String, truncated: bool },
    /// Markdown source, shown rendered.
    Markdown { text: String },
    /// Read-only Jupyter notebook cells and saved outputs.
    Notebook(std::sync::Arc<Notebook>),
    /// Read-only slide text from a PowerPoint presentation.
    Presentation(std::sync::Arc<Presentation>),
    /// Tabular data (CSV, TSV or Parquet): a header row and records.
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        truncated: bool,
    },
    /// Labelled details only (e.g. an audio file's tags and format).
    Info { info: Vec<(String, String)> },
    /// Contents of an archive.
    Archive {
        items: Vec<ArchiveItem>,
        /// Items in the archive (more than listed when truncated).
        total: usize,
        truncated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveItem {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    /// Name of the previewer that produced it, e.g. "Text".
    pub kind: &'static str,
    pub content: Content,
    /// Set when `path` is a symlink; the other fields describe its target.
    pub link: crate::links::LinkState,
    /// Canonical symlink target, computed while the preview loads in background.
    pub resolved_link: Option<PathBuf>,
}

pub trait Previewer: Send + Sync {
    /// Short, user-visible kind, e.g. "Image".
    fn name(&self) -> &'static str;
    fn matches(&self, probe: &Probe) -> bool;
    fn load(&self, probe: &Probe) -> io::Result<Content>;
}

#[derive(Default)]
pub struct Registry {
    previewers: Vec<Box<dyn Previewer>>,
}

impl Registry {
    /// Built-in previewers, most specific first.
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(DirectoryPreviewer);
        registry.register(RawPreviewer);
        registry.register(ImagePreviewer);
        registry.register(PdfPreviewer);
        registry.register(MediaPreviewer);
        // Before the archive previewer: XLSX, ODS, DOCX and ODT are zip files.
        registry.register(SpreadsheetPreviewer);
        registry.register(DocumentPreviewer);
        registry.register(PresentationPreviewer);
        registry.register(ArchivePreviewer);
        registry.register(MarkdownPreviewer);
        registry.register(NotebookPreviewer);
        registry.register(ParquetPreviewer);
        registry.register(TablePreviewer);
        registry.register(TextPreviewer);
        registry.register(BinaryPreviewer);
        registry
    }

    /// Adds a previewer after the existing ones.
    pub fn register(&mut self, previewer: impl Previewer + 'static) {
        self.previewers.push(Box::new(previewer));
    }

    /// Probes and loads `path` with the first matching previewer. Blocking.
    pub fn preview(&self, path: &Path) -> Result<Preview, String> {
        // Remote files are previewed from their downloaded copy only.
        if crate::location::is_remote(path) {
            return Err(format!("{}: not downloaded", path.display()));
        }
        if !path.exists()
            && let Some((file, inner)) = crate::archive::locate(path)
            && !inner.is_empty()
        {
            return self.preview_in_archive(path, &file, &inner);
        }
        let error = |e: io::Error| format!("{}: {e}", path.display());
        // A broken link cannot be read; say where it points instead.
        let probe = Probe::read(path).map_err(|e| {
            let link = crate::links::LinkState::of(path);
            match link.is_broken().then(|| link.tooltip(path)).flatten() {
                Some(tip) => format!("{}: {tip}", path.display()),
                None => error(e),
            }
        })?;
        let previewer = self
            .previewers
            .iter()
            .find(|p| p.matches(&probe))
            .ok_or_else(|| format!("No preview for {}", probe.name))?;
        let content = previewer.load(&probe).map_err(error)?;
        let link = crate::links::LinkState::of(path);
        let resolved_link = link.resolved(path);
        Ok(Preview {
            path: probe.path,
            name: probe.name,
            size: probe.size,
            kind: previewer.name(),
            content,
            link,
            resolved_link,
        })
    }

    /// Previews an entry of an archive: a folder is summarised from the
    /// listing, a file is extracted to the preview cache first (up to
    /// [`ARCHIVE_LIMIT`] bytes) and previewed from there.
    fn preview_in_archive(&self, path: &Path, file: &Path, inner: &str) -> Result<Preview, String> {
        let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        let index = crate::archive::Index::cached(file).map_err(|e| error(&e))?;
        let node = index
            .get(inner)
            .ok_or_else(|| error(&"not in the archive"))?;
        let (name, size) = (node.name.clone(), node.size);
        if node.is_dir() {
            let (dirs, files, files_size) = index.summary(inner);
            return Ok(Preview {
                path: path.to_path_buf(),
                name,
                size,
                kind: "Directory",
                content: Content::Directory {
                    dirs,
                    files,
                    files_size,
                },
                link: Default::default(),
                resolved_link: None,
            });
        }
        let modified = fs::metadata(file).and_then(|m| m.modified()).ok();
        let cached = cache_dir(file, modified, inner).map_err(|e| error(&e))?;
        let temp = cached.join(&name);
        if !fs::symlink_metadata(&temp).is_ok_and(|m| m.is_file()) {
            prune_cache(&cached);
            fs::create_dir_all(&cached).map_err(|e| error(&e))?;
            static PARTS: AtomicUsize = AtomicUsize::new(0);
            let partial = cached.join(format!(
                ".{}-{}.part",
                std::process::id(),
                PARTS.fetch_add(1, Ordering::Relaxed)
            ));
            let result = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)
                .and_then(|mut out| {
                    crate::archive::read_file(file, inner, ARCHIVE_LIMIT, &mut out)
                });
            match result {
                Ok(()) => fs::rename(&partial, &temp).map_err(|e| error(&e))?,
                Err(e) => {
                    let _ = fs::remove_file(&partial);
                    return Err(error(&e));
                }
            }
        }
        let mut preview = self.preview(&temp)?;
        preview.path = path.to_path_buf();
        preview.name = name;
        preview.resolved_link = None;
        Ok(preview)
    }
}

/// Largest archive entry previewed (it is extracted to disk first).
const ARCHIVE_LIMIT: u64 = 32 * 1024 * 1024;

/// Most the archive preview cache keeps on disk.
const CACHE_LIMIT: u64 = 256 * 1024 * 1024;

/// The per-user directory holding extracted archive entries.
fn cache_root() -> io::Result<PathBuf> {
    dirs::cache_dir()
        .map(|dir| dir.join("fyle").join("archive-preview"))
        .ok_or_else(|| io::Error::other("no cache directory"))
}

/// Where an archive entry is extracted for previewing: named after the
/// archive, its modification time and the entry, so it is reused.
fn cache_dir(
    file: &Path,
    modified: Option<std::time::SystemTime>,
    inner: &str,
) -> io::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (file, modified, inner).hash(&mut hasher);
    Ok(cache_root()?.join(format!("{:016x}", hasher.finish())))
}

/// Deletes the least recently written cache folders until the cache is
/// within [`CACHE_LIMIT`], keeping `keep`.
fn prune_cache(keep: &Path) {
    let Some(root) = keep.parent() else { return };
    let Ok(items) = fs::read_dir(root) else {
        return;
    };
    let mut folders: Vec<(std::time::SystemTime, u64, PathBuf)> = items
        .flatten()
        .filter(|item| item.path() != keep)
        .map(|item| {
            let size = fs::read_dir(item.path())
                .map(|files| {
                    files
                        .flatten()
                        .filter_map(|f| f.metadata().ok())
                        .map(|m| m.len())
                        .sum()
                })
                .unwrap_or(0);
            let modified = item
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (modified, size, item.path())
        })
        .collect();
    let mut total: u64 = folders.iter().map(|f| f.1).sum();
    folders.sort();
    for (_, size, path) in folders {
        if total <= CACHE_LIMIT {
            break;
        }
        if fs::remove_dir_all(&path).is_ok() {
            total -= size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A scratch folder with an empty `sub/`.
    fn fixture() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        dir
    }

    fn file(dir: &TempDir, name: &str, data: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, data).unwrap();
        path
    }

    #[test]
    fn picks_previewer_by_type() {
        let dir = fixture();
        let registry = Registry::with_defaults();

        let rs = file(&dir, "main.rs", b"fn main() {}\n");
        let preview = registry.preview(&rs).unwrap();
        assert_eq!(preview.kind, "Text");
        assert_eq!(
            preview.content,
            Content::Text {
                text: "fn main() {}\n".into(),
                language: Some("rust"),
                truncated: false
            }
        );

        let png = file(&dir, "pic.PNG", b"\x89PNG\r\n\x1a\n");
        assert_eq!(registry.preview(&png).unwrap().kind, "Image");

        let bin = file(&dir, "blob", &[0, 1, 2, b'A']);
        let preview = registry.preview(&bin).unwrap();
        assert_eq!(preview.kind, "Binary");
        let Content::Binary { dump, truncated } = preview.content else {
            panic!("expected binary");
        };
        assert!(!truncated);
        assert!(dump.starts_with("00000000  00 01 02 41"), "{dump}");
        assert!(dump.ends_with("|...A|"), "{dump}");

        let preview = registry.preview(dir.path()).unwrap();
        assert_eq!(
            preview.content,
            Content::Directory {
                dirs: 1,
                files: 3,
                files_size: 13 + 8 + 4
            }
        );

        assert!(registry.preview(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn custom_previewer_takes_precedence_when_registered_first() {
        struct Csv;
        impl Previewer for Csv {
            fn name(&self) -> &'static str {
                "CSV"
            }
            fn matches(&self, probe: &Probe) -> bool {
                probe.ext == "csv"
            }
            fn load(&self, _: &Probe) -> io::Result<Content> {
                Ok(Content::Text {
                    text: "table".into(),
                    language: None,
                    truncated: false,
                })
            }
        }

        let dir = fixture();
        let csv = file(&dir, "data.csv", b"a,b\n");
        let mut registry = Registry::default();
        registry.register(Csv);
        registry.register(TextPreviewer);
        assert_eq!(registry.preview(&csv).unwrap().kind, "CSV");
    }

    #[test]
    fn markdown_tables_and_archives() {
        let dir = fixture();
        let registry = Registry::with_defaults();

        let md = file(&dir, "README.md", b"# Title\n");
        assert_eq!(
            registry.preview(&md).unwrap().content,
            Content::Markdown {
                text: "# Title\n".into()
            }
        );

        let csv = file(&dir, "data.csv", b"name,qty\n\"Bolt, M3\",10\nNut,5\n");
        let Content::Table {
            header,
            rows,
            truncated,
        } = registry.preview(&csv).unwrap().content
        else {
            panic!("expected a table");
        };
        assert_eq!(header, ["name", "qty"]);
        assert_eq!(rows, [vec!["Bolt, M3", "10"], vec!["Nut", "5"]]);
        assert!(!truncated);

        // A tar.gz with one file, built with the same crates.
        let path = dir.path().join("a.tar.gz");
        {
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&path).unwrap(),
                flate2::Compression::fast(),
            );
            let mut builder = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(5);
            header.set_cksum();
            builder
                .append_data(&mut header, "dir/hello.txt", &b"hello"[..])
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }
        let preview = registry.preview(&path).unwrap();
        assert_eq!(preview.kind, "Archive");
        assert_eq!(
            preview.content,
            Content::Archive {
                items: vec![ArchiveItem {
                    name: "dir/hello.txt".into(),
                    size: 5,
                    is_dir: false
                }],
                total: 1,
                truncated: false
            }
        );
    }

    #[test]
    fn text_detection() {
        let probe = |head: &[u8]| Probe {
            path: PathBuf::new(),
            name: String::new(),
            ext: String::new(),
            is_dir: false,
            size: 0,
            head: head.to_vec(),
        };
        assert!(probe(b"hello").looks_like_text());
        assert!(probe("žluťoučký".as_bytes()).looks_like_text());
        assert!(probe(&"ž".as_bytes()[..1]).looks_like_text()); // cut-off sequence
        assert!(!probe(b"a\0b").looks_like_text());
        assert!(!probe(&[0xff, 0xfe, 0x41]).looks_like_text());
    }

    #[test]
    fn previews_entries_inside_an_archive() {
        use std::io::Write;
        let dir = fixture();
        let zip_path = dir.path().join("pack.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("src/main.rs", opts).unwrap();
        zip.write_all(b"fn main() {}\n").unwrap();
        zip.start_file("src/big.bin", opts).unwrap();
        zip.write_all(&[0u8; 64]).unwrap();
        zip.finish().unwrap();

        let registry = Registry::with_defaults();
        let preview = registry.preview(&zip_path.join("src/main.rs")).unwrap();
        assert_eq!(preview.kind, "Text");
        assert_eq!(preview.name, "main.rs");
        assert_eq!(preview.path, zip_path.join("src/main.rs"));
        assert!(
            matches!(preview.content, Content::Text { ref text, .. } if text == "fn main() {}\n")
        );

        let preview = registry.preview(&zip_path.join("src")).unwrap();
        assert_eq!(
            preview.content,
            Content::Directory {
                dirs: 0,
                files: 2,
                files_size: 13 + 64
            }
        );
        assert!(registry.preview(&zip_path.join("src/nope")).is_err());
        // The archive itself still lists its contents.
        assert_eq!(registry.preview(&zip_path).unwrap().kind, "Archive");
    }
}

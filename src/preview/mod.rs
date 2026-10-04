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

mod binary;
mod directory;
mod image;
mod text;

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub use binary::BinaryPreviewer;
pub use directory::DirectoryPreviewer;
pub use image::ImagePreviewer;
pub use text::TextPreviewer;

/// Bytes read up front for type detection.
const HEAD_LEN: usize = 8 * 1024;

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
    /// An image the view loads from `path`.
    Image { path: PathBuf },
    /// Summary of a directory's direct children.
    Directory {
        dirs: usize,
        files: usize,
        files_size: u64,
    },
    /// Hex dump of the start of a binary file.
    Binary { dump: String, truncated: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    /// Name of the previewer that produced it, e.g. "Text".
    pub kind: &'static str,
    pub content: Content,
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
        registry.register(ImagePreviewer);
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
        let error = |e: io::Error| format!("{}: {e}", path.display());
        let probe = Probe::read(path).map_err(error)?;
        let previewer = self
            .previewers
            .iter()
            .find(|p| p.matches(&probe))
            .ok_or_else(|| format!("No preview for {}", probe.name))?;
        let content = previewer.load(&probe).map_err(error)?;
        Ok(Preview {
            path: probe.path,
            name: probe.name,
            size: probe.size,
            kind: previewer.name(),
            content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("fyle-preview-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("sub")).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str, data: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, data).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn picks_previewer_by_type() {
        let dir = TempDir::new("types");
        let registry = Registry::with_defaults();

        let rs = dir.file("main.rs", b"fn main() {}\n");
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

        let png = dir.file("pic.PNG", b"\x89PNG\r\n\x1a\n");
        assert_eq!(registry.preview(&png).unwrap().kind, "Image");

        let bin = dir.file("blob", &[0, 1, 2, b'A']);
        let preview = registry.preview(&bin).unwrap();
        assert_eq!(preview.kind, "Binary");
        let Content::Binary { dump, truncated } = preview.content else {
            panic!("expected binary");
        };
        assert!(!truncated);
        assert!(dump.starts_with("00000000  00 01 02 41"), "{dump}");
        assert!(dump.ends_with("|...A|"), "{dump}");

        let preview = registry.preview(&dir.0).unwrap();
        assert_eq!(
            preview.content,
            Content::Directory {
                dirs: 1,
                files: 3,
                files_size: 13 + 8 + 4
            }
        );

        assert!(registry.preview(&dir.0.join("missing")).is_err());
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

        let dir = TempDir::new("custom");
        let csv = dir.file("data.csv", b"a,b\n");
        let mut registry = Registry::default();
        registry.register(Csv);
        registry.register(TextPreviewer);
        assert_eq!(registry.preview(&csv).unwrap().kind, "CSV");
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
}

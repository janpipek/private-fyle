//! PDF metadata and on-demand page rendering through Poppler.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Content, Previewer, Probe};

/// Loads PDF metadata and supports on-demand page rendering through Poppler.
pub struct PdfPreviewer;

impl Previewer for PdfPreviewer {
    fn name(&self) -> &'static str {
        "PDF"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && (probe.ext == "pdf" || probe.head.starts_with(b"%PDF-"))
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        // Check the renderer here so systems without Poppler keep the useful
        // installation hint instead of showing a page-load error later.
        match Command::new("pdftoppm").arg("-v").output() {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Content::Text {
                    text: "PDF preview needs pdftoppm from poppler (e.g. dnf install \
                           poppler-utils, apt install poppler-utils)."
                        .into(),
                    language: None,
                    truncated: false,
                });
            }
            Err(e) => return Err(e),
        }
        let metadata = Command::new("pdfinfo")
            .arg(&probe.path)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| parse_info(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or_default();
        let pages = page_count(&metadata).unwrap_or(1);
        Ok(Content::Pdf {
            path: probe.path.clone(),
            pages,
            metadata,
        })
    }
}

/// Number of pages reported by `pdfinfo`, when present and valid.
fn page_count(metadata: &[(String, String)]) -> Option<usize> {
    metadata
        .iter()
        .find(|(key, _)| key == "Pages")?
        .1
        .parse()
        .ok()
        .filter(|pages| *pages > 0)
}

/// Renders one PDF page into the shared preview cache.
pub fn render_page(pdf: &Path, page: usize) -> io::Result<PathBuf> {
    static TEMP_ID: AtomicUsize = AtomicUsize::new(0);
    if page == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PDF page numbers start at 1",
        ));
    }
    let cache_key = super::cache_file(pdf, "pdf")?;
    let png = cache_key.with_extension(format!("p{page}.png"));
    if png.is_file() && fs::metadata(&png)?.len() > 0 {
        return Ok(png);
    }
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let prefix = png.with_file_name(format!(".fyle-pdf-{}-{page}-{id}", std::process::id()));
    let generated = prefix.with_extension("png");
    let output = Command::new("pdftoppm")
        .args([
            "-png",
            "-singlefile",
            "-f",
            &page.to_string(),
            "-l",
            &page.to_string(),
            "-scale-to",
            "1200",
        ])
        .arg(pdf)
        .arg(&prefix)
        .output()?;
    if !output.status.success() || !generated.is_file() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(io::Error::other(if message.is_empty() {
            "pdftoppm failed".to_string()
        } else {
            message
        }));
    }
    match fs::rename(&generated, &png) {
        Ok(()) => Ok(png),
        Err(_e) if png.is_file() => {
            let _ = fs::remove_file(&generated);
            Ok(png)
        }
        Err(e) => Err(e),
    }
}

/// The interesting lines of `pdfinfo` output.
fn parse_info(text: &str) -> Vec<(String, String)> {
    const KEYS: &[&str] = &[
        "Title",
        "Author",
        "Creator",
        "Producer",
        "CreationDate",
        "Pages",
        "Page size",
    ];
    let mut info: Vec<(String, String)> = text
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let value = value.trim();
            (KEYS.contains(&key) && !value.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect();
    info.sort_by_key(|(key, _)| KEYS.iter().position(|k| k == key));
    info
}

#[cfg(test)]
mod tests {
    use super::{PdfPreviewer, page_count, parse_info};
    use crate::preview::{Previewer, Probe};
    use std::path::PathBuf;

    #[test]
    fn parses_pdfinfo_and_page_count() {
        let text = "Producer:        LibreOffice 7.6\nTagged:          no\n\
                    Pages:           3\nTitle:           Report\nAuthor:          \n\
                    Page size:       595 x 842 pts (A4)\n";
        let info = parse_info(text);
        assert_eq!(page_count(&info), Some(3));
        assert_eq!(
            info,
            [
                ("Title".to_string(), "Report".to_string()),
                ("Producer".to_string(), "LibreOffice 7.6".to_string()),
                ("Pages".to_string(), "3".to_string()),
                ("Page size".to_string(), "595 x 842 pts (A4)".to_string()),
            ]
        );
    }

    #[test]
    fn invalid_page_counts_are_ignored() {
        for value in ["0", "many", ""] {
            assert_eq!(page_count(&[("Pages".into(), value.into())]), None);
        }
        assert_eq!(page_count(&[]), None);
    }

    #[test]
    fn matches_by_extension_or_magic() {
        let probe = |ext: &str, head: &[u8]| Probe {
            path: PathBuf::new(),
            name: String::new(),
            ext: ext.into(),
            is_dir: false,
            size: 0,
            head: head.to_vec(),
        };
        assert!(PdfPreviewer.matches(&probe("pdf", b"")));
        assert!(PdfPreviewer.matches(&probe("", b"%PDF-1.7\n")));
        assert!(!PdfPreviewer.matches(&probe("txt", b"hello")));
    }
}

//! PDF: the first page rendered by poppler's `pdftoppm`, with `pdfinfo`'s
//! summary. Without poppler, a note saying what to install.

use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Content, Previewer, Probe};

pub struct PdfPreviewer;

impl Previewer for PdfPreviewer {
    fn name(&self) -> &'static str {
        "PDF"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && (probe.ext == "pdf" || probe.head.starts_with(b"%PDF-"))
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let info = Command::new("pdfinfo")
            .arg(&probe.path)
            .output()
            .ok()
            .map(|o| parse_info(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default();
        match render_first_page(&probe.path) {
            Ok(png) => Ok(Content::Image { path: png, info }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Content::Text {
                text: "PDF preview needs pdftoppm from poppler (e.g. dnf install \
                       poppler-utils, apt install poppler-utils)."
                    .into(),
                language: None,
                truncated: false,
            }),
            Err(e) => Err(e),
        }
    }
}

/// Renders page 1 into a PNG in the temp directory, reused while the PDF
/// is unchanged.
fn render_first_page(pdf: &Path) -> io::Result<PathBuf> {
    let modified = std::fs::metadata(pdf)?.modified()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (pdf, modified).hash(&mut hasher);
    let dir = std::env::temp_dir().join("fyle-pdf-preview");
    std::fs::create_dir_all(&dir)?;
    let prefix = dir.join(format!("{:016x}", hasher.finish()));
    let png = prefix.with_extension("png");
    if png.exists() {
        return Ok(png);
    }
    let output = Command::new("pdftoppm")
        .args([
            "-png",
            "-singlefile",
            "-f",
            "1",
            "-l",
            "1",
            "-scale-to",
            "1200",
        ])
        .arg(pdf)
        .arg(&prefix)
        .output()?;
    if !output.status.success() || !png.exists() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(io::Error::other(if message.is_empty() {
            "pdftoppm failed".to_string()
        } else {
            message
        }));
    }
    Ok(png)
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
    use super::*;

    #[test]
    fn parses_pdfinfo() {
        let text = "Producer:        LibreOffice 7.6\nTagged:          no\n\
                    Pages:           3\nTitle:           Report\nAuthor:          \n\
                    Page size:       595 x 842 pts (A4)\n";
        assert_eq!(
            parse_info(text),
            [
                ("Title".to_string(), "Report".to_string()),
                ("Producer".to_string(), "LibreOffice 7.6".to_string()),
                ("Pages".to_string(), "3".to_string()),
                ("Page size".to_string(), "595 x 842 pts (A4)".to_string()),
            ]
        );
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

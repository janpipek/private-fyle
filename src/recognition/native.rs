//! Filename hints, binary signatures and conservative bounded text heuristics.

use super::{Candidate, Evidence};

pub(super) fn by_name(name: &str) -> Option<Candidate> {
    let lower = name.to_lowercase();
    let compound = if lower.ends_with(".tar.xz") {
        Some(("application/x-xz-compressed-tar", "xz"))
    } else if lower.ends_with(".tar.bz2") {
        Some(("application/x-bzip2-compressed-tar", "bz2"))
    } else if lower.ends_with(".tar.zst") || lower.ends_with(".tar.zstd") {
        Some(("application/x-zstd-compressed-tar", "zst"))
    } else {
        None
    };
    if let Some((mime, extension)) = compound {
        return Some(Candidate::new(mime, Evidence::Name, Some(extension)));
    }
    let ext = match name {
        "Justfile" | "justfile" => "just",
        "CMakeLists.txt" => "cmake",
        "Makefile" | "makefile" | "GNUmakefile" => "mk",
        _ => lower.rsplit_once('.').map_or("", |(_, ext)| ext),
    };
    let (mime, ext) = match ext {
        "rs" => ("text/x-rust", "rs"),
        "py" | "pyi" => ("text/x-python", "py"),
        "js" | "mjs" | "cjs" => ("application/javascript", "js"),
        "ts" | "mts" | "cts" => ("text/typescript", "ts"),
        "c" | "h" => ("text/x-csrc", "c"),
        "cpp" | "cc" | "cxx" | "hpp" => ("text/x-c++src", "cpp"),
        "sh" | "bash" | "zsh" => ("application/x-shellscript", "sh"),
        "toml" => ("application/toml", "toml"),
        "csv" => ("text/csv", "csv"),
        "tsv" => ("text/tab-separated-values", "tsv"),
        "txt" | "log" => ("text/plain", "txt"),
        "md" | "markdown" => ("text/markdown", "md"),
        "yaml" | "yml" => ("application/yaml", "yaml"),
        "json" => ("application/json", "json"),
        "xml" => ("application/xml", "xml"),
        "xmp" => ("application/rdf+xml", "xmp"),
        "rpm" => ("application/x-rpm", "rpm"),
        "deb" | "udeb" => ("application/vnd.debian.binary-package", "deb"),
        "sqlite" | "sqlite3" | "db" => ("application/vnd.sqlite3", "sqlite"),
        "glsl" | "vert" | "frag" | "geom" | "tesc" | "tese" | "comp" => ("text/x-glsl", "glsl"),
        "wgsl" => ("text/wgsl", "wgsl"),
        "hlsl" | "fx" => ("text/x-hlsl", "hlsl"),
        "just" => ("text/x-just", "just"),
        "cmake" => ("text/x-cmake", "cmake"),
        "mk" => ("text/x-makefile", "mk"),
        "png" => ("image/png", "png"),
        "jpg" | "jpeg" => ("image/jpeg", "jpg"),
        "gif" => ("image/gif", "gif"),
        "webp" => ("image/webp", "webp"),
        "bmp" => ("image/bmp", "bmp"),
        "tif" | "tiff" => ("image/tiff", "tiff"),
        "cr2" => ("image/x-canon-cr2", "cr2"),
        "dng" => ("image/x-adobe-dng", "dng"),
        "nef" => ("image/x-nikon-nef", "nef"),
        "nrw" => ("image/x-nikon-nrw", "nrw"),
        "arw" => ("image/x-sony-arw", "arw"),
        "sr2" => ("image/x-sony-sr2", "sr2"),
        "srf" => ("image/x-sony-srf", "srf"),
        "pef" => ("image/x-pentax-pef", "pef"),
        "svg" => ("image/svg+xml", "svg"),
        "pdf" => ("application/pdf", "pdf"),
        "mp3" => ("audio/mpeg", "mp3"),
        "flac" => ("audio/flac", "flac"),
        "wav" => ("audio/x-wav", "wav"),
        "zip" => ("application/zip", "zip"),
        "docx" => (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        ),
        "xlsx" => (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        ),
        "pptx" => (
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        ),
        "odt" => ("application/vnd.oasis.opendocument.text", "odt"),
        "ods" => ("application/vnd.oasis.opendocument.spreadsheet", "ods"),
        "parquet" => ("application/vnd.apache.parquet", "parquet"),
        "7z" => ("application/x-7z-compressed", "7z"),
        "rar" => ("application/vnd.rar", "rar"),
        "xz" => ("application/x-xz", "xz"),
        "bz2" => ("application/x-bzip2", "bz2"),
        "zst" | "zstd" => ("application/zstd", "zst"),
        "avi" => ("video/x-msvideo", "avi"),
        _ => return None,
    };
    Some(Candidate::new(mime, Evidence::Name, Some(ext)))
}

pub(super) fn zip_subtype(mime: &str) -> bool {
    mime.starts_with("application/vnd.openxmlformats-officedocument.")
        || mime.starts_with("application/vnd.oasis.opendocument.")
        || mime.contains("macroEnabled")
}

pub(super) fn compression_subtype(mime: &str) -> bool {
    mime == "application/x-xz-compressed-tar"
        || mime == "application/x-bzip2-compressed-tar"
        || mime == "application/x-bzip-compressed-tar"
        || mime == "application/x-zstd-compressed-tar"
}

pub(super) fn raw_tiff_subtype(mime: &str) -> bool {
    matches!(
        mime,
        "image/x-canon-cr2"
            | "image/x-adobe-dng"
            | "image/x-nikon-nef"
            | "image/x-nikon-nrw"
            | "image/x-sony-arw"
            | "image/x-sony-sr2"
            | "image/x-sony-srf"
            | "image/x-pentax-pef"
    )
}

pub(super) fn detect(head: &[u8]) -> Vec<Candidate> {
    let signature = if head.starts_with(b"\xed\xab\xee\xdb") {
        Some(("application/x-rpm", "rpm"))
    } else if head.starts_with(b"!<arch>\n")
        && head.get(8..24).is_some_and(|name| {
            std::str::from_utf8(name)
                .is_ok_and(|name| name.trim_end().trim_end_matches('/') == "debian-binary")
        })
    {
        Some(("application/vnd.debian.binary-package", "deb"))
    } else if head.starts_with(b"SQLite format 3\0") {
        Some(("application/vnd.sqlite3", "sqlite"))
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some(("image/jpeg", "jpg"))
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP") {
        Some(("image/webp", "webp"))
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
        Some(("audio/x-wav", "wav"))
    } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"AVI ") {
        Some(("video/x-msvideo", "avi"))
    } else if head.starts_with(b"BM") && head.len() >= 14 && head.get(6..10) == Some(&[0; 4]) {
        Some(("image/bmp", "bmp"))
    } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        Some(("image/tiff", "tiff"))
    } else if head.starts_with(b"7z\xbc\xaf'\x1c") {
        Some(("application/x-7z-compressed", "7z"))
    } else if head.starts_with(b"Rar!\x1a\x07\0") || head.starts_with(b"Rar!\x1a\x07\x01\0") {
        Some(("application/vnd.rar", "rar"))
    } else if head.starts_with(b"\xfd7zXZ\0") {
        Some(("application/x-xz", "xz"))
    } else if head.starts_with(b"BZh")
        && head.get(3).is_some_and(|byte| (b'1'..=b'9').contains(byte))
    {
        Some(("application/x-bzip2", "bz2"))
    } else if head.starts_with(b"\x28\xb5\x2f\xfd") {
        Some(("application/zstd", "zst"))
    } else if head.starts_with(b"%PDF-") {
        Some(("application/pdf", "pdf"))
    } else if head.starts_with(b"fLaC") {
        Some(("audio/flac", "flac"))
    } else if head.starts_with(b"ID3") {
        Some(("audio/mpeg", "mp3"))
    } else if head.starts_with(b"PAR1") {
        Some(("application/vnd.apache.parquet", "parquet"))
    } else if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        Some(("application/zip", "zip"))
    } else {
        None
    };
    if let Some((mime, ext)) = signature {
        let ext = if matches!(
            mime,
            "application/x-7z-compressed"
                | "application/vnd.rar"
                | "application/x-xz"
                | "application/x-bzip2"
                | "application/zstd"
                | "video/x-msvideo"
        ) {
            None
        } else {
            Some(ext)
        };
        return vec![Candidate::new(mime, Evidence::Signature, ext)];
    }
    if head.contains(&0) {
        return Vec::new();
    }
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(error) if error.error_len().is_none() => {
            std::str::from_utf8(&head[..error.valid_up_to()]).expect("valid UTF-8 prefix")
        }
        Err(_) => return Vec::new(),
    };
    let text = text.trim_start_matches('\u{feff}').trim();
    let mut result = Vec::new();
    // Parse only the bounded prefix. Incomplete JSON falls back to name/text evidence.
    if (text.starts_with('{') || text.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(text).is_ok()
    {
        result.push(Candidate::new(
            "application/json",
            Evidence::Heuristic,
            Some("json"),
        ));
    }
    if text.starts_with('<') {
        // Structural parsing avoids matching XML-looking prose and comments.
        if let Ok(document) = roxmltree::Document::parse(text) {
            let root = document.root_element();
            let (mime, ext) = match (root.tag_name().name(), root.tag_name().namespace()) {
                ("svg", Some("http://www.w3.org/2000/svg")) => ("image/svg+xml", "svg"),
                ("xmpmeta", Some("adobe:ns:meta/"))
                | ("RDF", Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")) => {
                    ("application/rdf+xml", "xmp")
                }
                _ => ("application/xml", "xml"),
            };
            let evidence = if mime == "application/xml" {
                Evidence::Heuristic
            } else {
                Evidence::Signature
            };
            result.push(Candidate::new(mime, evidence, Some(ext)));
        }
    }
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(128)
        .collect();
    // Several simple mapping entries are stronger than a single colon in prose.
    let mappings = lines
        .iter()
        .filter(|line| {
            let Some((key, value)) = line.split_once(':') else {
                return false;
            };
            !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                && (value.is_empty() || value.starts_with(char::is_whitespace))
        })
        .count();
    if mappings >= 2
        && mappings
            == lines
                .iter()
                .filter(|line| **line != "---" && **line != "...")
                .count()
    {
        result.push(Candidate::new(
            "application/yaml",
            Evidence::Heuristic,
            Some("yaml"),
        ));
    }
    if text
        .lines()
        .any(|line| line.trim_start().starts_with("#version "))
        && text.contains("void main(")
        && text.contains('{')
    {
        result.push(Candidate::new(
            "text/x-glsl",
            Evidence::Heuristic,
            Some("glsl"),
        ));
    }
    if (text.contains("@vertex") || text.contains("@fragment") || text.contains("@compute"))
        && text.contains("fn ")
        && text.contains('{')
    {
        result.push(Candidate::new(
            "text/wgsl",
            Evidence::Heuristic,
            Some("wgsl"),
        ));
    }
    result.push(Candidate::new(
        "text/plain",
        Evidence::Fallback,
        Some("txt"),
    ));
    result
}

#[cfg(test)]
mod tests {
    use crate::recognition::{NativeDetector, detect};

    #[test]
    fn text_heuristics_recognize_structures_and_leave_prose_generic() {
        for (bytes, mime) in [
            ("name: demo\nenabled: true\n", "application/yaml"),
            ("#version 450\nvoid main() {}", "text/x-glsl"),
            ("@compute @workgroup_size(1) fn main() {}", "text/wgsl"),
            (
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"/>"#,
                "application/rdf+xml",
            ),
            ("{\"ok\":true}", "application/json"),
            (
                "Note: this is ordinary prose.\nAnother sentence.",
                "text/plain",
            ),
            ("http://example.org\nhttps://example.org", "text/plain"),
            ("<!-- <svg/> -->hello", "text/plain"),
        ] {
            assert_eq!(
                detect(
                    "unknown",
                    false,
                    Some(bytes.as_bytes()),
                    None,
                    &NativeDetector
                )[0]
                .mime,
                mime,
                "{bytes}"
            );
        }
    }

    #[test]
    fn recognizes_bounded_container_and_image_prefixes_without_preview_extensions() {
        for (bytes, mime) in [
            (&b"BM\0\0\0\0\0\0\0\0\0\0\0\0"[..], "image/bmp"),
            (b"II*\0", "image/tiff"),
            (b"MM\0*", "image/tiff"),
            (b"7z\xbc\xaf'\x1c", "application/x-7z-compressed"),
            (b"Rar!\x1a\x07\0", "application/vnd.rar"),
            (b"Rar!\x1a\x07\x01\0", "application/vnd.rar"),
            (b"\xfd7zXZ\0", "application/x-xz"),
            (b"BZh9", "application/x-bzip2"),
            (b"\x28\xb5\x2f\xfd", "application/zstd"),
            (b"RIFF\0\0\0\0AVI ", "video/x-msvideo"),
        ] {
            let candidate = super::detect(bytes)
                .into_iter()
                .next()
                .expect("signature candidate");
            assert_eq!(candidate.mime, mime, "{bytes:?}");
            if mime.starts_with("application/x-")
                || mime == "application/vnd.rar"
                || mime == "video/x-msvideo"
            {
                assert_eq!(candidate.extension, None);
            }
        }
    }

    #[test]
    fn rejects_truncated_magics_and_false_bmp_bzip_riff_subtypes() {
        for bytes in [
            b"BM".as_slice(),
            b"II*",
            b"7z\xbc\xaf'",
            b"Rar!\x1a\x07",
            b"\xfd7zXZ",
            b"BZh0",
            b"BZhX",
            b"RIFF\0\0\0\0NOPE",
        ] {
            assert!(
                super::detect(bytes)
                    .iter()
                    .all(|candidate| candidate.evidence != crate::recognition::Evidence::Signature),
                "{bytes:?}"
            );
        }
        let mut bmp = b"BM\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        bmp[6] = 1;
        assert!(
            super::detect(&bmp)
                .iter()
                .all(|candidate| candidate.evidence != crate::recognition::Evidence::Signature)
        );
    }
}

//! Bounded file recognition shared by desktop associations, previews and panel icons.

mod native;

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::mime::MimeDb;

/// Maximum prefix inspected by any detector.
pub const HEAD_LIMIT: usize = 8192;

/// Evidence ordering, strongest first when candidates are presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
    /// Generic UTF-8 text or unknown binary fallback.
    Fallback,
    /// Bounded content clues, without a complete format parse.
    Heuristic,
    /// Recognized filename or extension.
    Name,
    /// Distinctive binary signature or structured text root.
    Signature,
}

/// One plausible type; alternatives remain available for future viewer selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Canonical MIME vocabulary used for desktop associations.
    pub mime: String,
    /// Why this candidate was selected.
    pub evidence: Evidence,
    /// Extension understood by existing preview adapters, where applicable.
    pub extension: Option<&'static str>,
}

impl Candidate {
    fn new(mime: &str, evidence: Evidence, extension: Option<&'static str>) -> Self {
        Self {
            mime: mime.into(),
            evidence,
            extension,
        }
    }
}

/// Pluggable content-only detector. The caller grants a bounded byte prefix, never a path.
pub trait Detector {
    /// Returns candidates in deterministic preference order for equally strong evidence.
    fn detect(&self, head: &[u8]) -> Vec<Candidate>;
}

/// Native, dependency-free signature and text-heuristic implementation.
pub struct NativeDetector;

impl Detector for NativeDetector {
    fn detect(&self, head: &[u8]) -> Vec<Candidate> {
        native::detect(&head[..head.len().min(HEAD_LIMIT)])
    }
}

/// Recognizes a name immediately and optionally refines it from already authorized content.
/// Directories never use content. A generic ZIP signature preserves named ZIP subtypes.
pub fn detect(
    name: &str,
    is_dir: bool,
    head: Option<&[u8]>,
    db: Option<&MimeDb>,
    detector: &dyn Detector,
) -> Vec<Candidate> {
    if is_dir {
        return vec![Candidate::new(crate::mime::DIRECTORY, Evidence::Name, None)];
    }
    let mut candidates = Vec::new();
    let mut named = native::by_name(name);
    if let Some(db) = db {
        if let Some(candidate) = &mut named {
            candidate.mime = db.canonical(&candidate.mime).to_string();
        }
        let mime = db.detect(name, false);
        if mime != crate::mime::UNKNOWN {
            let extension = named
                .as_ref()
                .filter(|c| c.mime == mime)
                .and_then(|c| c.extension);
            candidates.push(Candidate::new(&mime, Evidence::Name, extension));
        }
    }
    candidates.extend(named);
    if let Some(head) = head {
        let mut content = detector.detect(&head[..head.len().min(HEAD_LIMIT)]);
        if let Some(db) = db {
            for candidate in &mut content {
                candidate.mime = db.canonical(&candidate.mime).to_string();
            }
        }
        if content.iter().any(|c| c.mime == "application/zip")
            && candidates.iter().any(|c| native::zip_subtype(&c.mime))
        {
            content.retain(|c| c.mime != "application/zip");
        }
        if let Some(subtype) = candidates
            .iter()
            .find(|c| native::compression_subtype(&c.mime))
        {
            let matching_container = match subtype.mime.as_str() {
                "application/x-xz-compressed-tar" => "application/x-xz",
                "application/x-bzip-compressed-tar" | "application/x-bzip2-compressed-tar" => {
                    "application/x-bzip2"
                }
                "application/x-zstd-compressed-tar" => "application/zstd",
                _ => "",
            };
            content.retain(|candidate| candidate.mime != matching_container);
        }
        if candidates
            .iter()
            .any(|candidate| native::raw_tiff_subtype(&candidate.mime))
        {
            content.retain(|candidate| candidate.mime != "image/tiff");
        }
        candidates.extend(content);
    }
    candidates.push(Candidate::new(
        crate::mime::UNKNOWN,
        Evidence::Fallback,
        None,
    ));
    candidates.sort_by_key(|c| std::cmp::Reverse(c.evidence));
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.mime.clone()));
    candidates
}

/// Reads only a regular file's bounded prefix; callers must run this off the UI thread.
pub fn read_head(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(target_os = "linux")]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        File::options()
            .custom_flags(libc::O_NONBLOCK)
            .read(true)
            .open(path)?
    };
    #[cfg(not(target_os = "linux"))]
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let mut head = Vec::new();
    file.take(HEAD_LIMIT as u64).read_to_end(&mut head)?;
    Ok(head)
}

#[cfg(test)]
mod tests {
    use super::{Candidate, Detector, Evidence, HEAD_LIMIT, NativeDetector, detect};

    #[test]
    fn signatures_override_misleading_names_and_keep_alternatives() {
        let candidates = detect(
            "photo.txt",
            false,
            Some(b"\x89PNG\r\n\x1a\n"),
            None,
            &NativeDetector,
        );
        assert_eq!(candidates[0].mime, "image/png");
        assert_eq!(candidates[0].evidence, Evidence::Signature);
        assert!(candidates.iter().any(|c| c.mime == "text/plain"));
        assert_eq!(
            detect(
                "report.docx",
                false,
                Some(b"PK\x03\x04"),
                None,
                &NativeDetector
            )[0]
            .extension,
            Some("docx")
        );
    }

    #[test]
    fn injectable_detector_receives_bounded_content_and_never_directories() {
        struct Fake;
        impl Detector for Fake {
            fn detect(&self, head: &[u8]) -> Vec<Candidate> {
                assert_eq!(head.len(), HEAD_LIMIT);
                vec![Candidate::new("example/custom", Evidence::Signature, None)]
            }
        }
        let input = vec![0; HEAD_LIMIT * 2];
        assert_eq!(
            detect("unknown", false, Some(&input), None, &Fake)[0].mime,
            "example/custom"
        );
        assert_eq!(
            detect("folder", true, Some(b"short"), None, &Fake)[0].mime,
            "inode/directory"
        );
    }

    #[test]
    fn file_reads_are_bounded_and_errors_do_not_invent_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("large");
        std::fs::write(&path, vec![b'x'; HEAD_LIMIT * 2]).unwrap();
        assert_eq!(super::read_head(&path).unwrap().len(), HEAD_LIMIT);
        assert!(super::read_head(dir.path()).is_err());
        assert!(super::read_head(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn content_aliases_canonicalize_before_deduplication_and_zip_suppression() {
        struct AliasDetector;
        impl Detector for AliasDetector {
            fn detect(&self, _head: &[u8]) -> Vec<Candidate> {
                vec![Candidate::new(
                    "application/x-alias",
                    Evidence::Signature,
                    None,
                )]
            }
        }
        let mut db = crate::mime::MimeDb::default();
        db.add_aliases("application/x-alias image/png\n");
        let candidates = detect(
            "photo.png",
            false,
            Some(b"anything"),
            Some(&db),
            &AliasDetector,
        );
        assert_eq!(
            candidates
                .iter()
                .filter(|candidate| candidate.mime == "image/png")
                .count(),
            1
        );
        assert_eq!(candidates[0].evidence, Evidence::Signature);
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.mime == "application/x-alias")
        );
        let zip_candidates = detect(
            "report.docx",
            false,
            Some(b"PK\x03\x04"),
            None,
            &NativeDetector,
        );
        assert!(
            !zip_candidates
                .iter()
                .any(|candidate| candidate.mime == "application/zip")
        );
        assert!(zip_candidates.iter().any(|candidate| candidate.mime
            == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"));
    }

    #[test]
    fn matching_compressed_tar_name_keeps_specificity_but_mismatch_keeps_signature() {
        let matching = detect(
            "backup.tar.xz",
            false,
            Some(b"\xfd7zXZ\0"),
            None,
            &NativeDetector,
        );
        assert_eq!(matching[0].mime, "application/x-xz-compressed-tar");
        assert!(
            !matching
                .iter()
                .any(|candidate| candidate.mime == "application/x-xz")
        );
        let mismatched = detect(
            "backup.tar.xz",
            false,
            Some(b"\x28\xb5\x2f\xfd"),
            None,
            &NativeDetector,
        );
        assert_eq!(mismatched[0].mime, "application/zstd");
        assert!(
            mismatched
                .iter()
                .any(|candidate| candidate.mime == "application/x-xz-compressed-tar")
        );
        let mut db = crate::mime::MimeDb::default();
        db.add_aliases("application/x-bzip-compressed-tar application/x-bzip2-compressed-tar\n");
        let bz2 = detect(
            "backup.tar.bz2",
            false,
            Some(b"BZh9"),
            Some(&db),
            &NativeDetector,
        );
        assert_eq!(bz2[0].mime, "application/x-bzip2-compressed-tar");
        assert!(
            !bz2.iter()
                .any(|candidate| candidate.mime == "application/x-bzip2")
        );
    }

    #[test]
    fn tiff_prefix_preserves_only_known_raw_filename_subtypes() {
        let raw = detect("photo.nef", false, Some(b"II*\0"), None, &NativeDetector);
        assert_eq!(raw[0].mime, "image/x-nikon-nef");
        assert!(!raw.iter().any(|candidate| candidate.mime == "image/tiff"));
        let db = crate::mime::MimeDb::default();
        let raw = detect(
            "photo.nef",
            false,
            Some(b"II*\0"),
            Some(&db),
            &NativeDetector,
        );
        assert_eq!(raw[0].mime, "image/x-nikon-nef");
        assert!(!raw.iter().any(|candidate| candidate.mime == "image/tiff"));
        let tiff = detect("photo.txt", false, Some(b"II*\0"), None, &NativeDetector);
        assert_eq!(tiff[0].mime, "image/tiff");
    }
}

//! Bounded extraction of embedded JPEG previews from common camera RAW files.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// RAW extensions for which embedded JPEG previews are attempted.
pub const EXTENSIONS: &[&str] = &[
    "raw", "cr2", "cr3", "nef", "nrw", "arw", "sr2", "srf", "raf", "rw2", "orf", "pef", "dng",
];

/// Maximum prefix inspected in a camera file.
pub const SCAN_LIMIT: u64 = 64 * 1024 * 1024;
const MAX_CANDIDATES: usize = 32;
const MAX_ATTEMPTS: usize = 32;
const MAX_JPEG_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PIXELS: u64 = MAX_DECODE_BYTES / 4;
const MAX_SIDE: u32 = 20_000;

/// Whether a path has an extension handled by the embedded-preview reader.
pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

/// Extracts and caches the largest decodable embedded JPEG preview.
///
/// The source is only scanned through [`SCAN_LIMIT`], candidates are bounded,
/// and dimensions are checked before decode. The cache key includes the source
/// path, byte size and nanosecond modification time; writes are atomic.
pub fn extract(path: &Path) -> io::Result<PathBuf> {
    extract_cached(path, &std::env::temp_dir().join("fyle-raw-preview-cache"))
}

/// Extracts a preview using `cache_dir`, useful to keep callers' cache state isolated.
pub fn extract_cached(path: &Path, cache_dir: &Path) -> io::Result<PathBuf> {
    let metadata = fs::metadata(path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    (path, metadata.len(), metadata.modified()?).hash(&mut hasher);
    create_cache_dir(cache_dir)?;
    let cached = cache_dir.join(format!("{:016x}.jpg", hasher.finish()));
    if fs::symlink_metadata(&cached).is_ok_and(|entry| entry.is_file()) {
        return Ok(cached);
    }

    let source = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len().min(1024 * 1024) as usize);
    source.take(SCAN_LIMIT).read_to_end(&mut bytes)?;
    let current = fs::metadata(path)?;
    if current.len() != metadata.len() || current.modified()? != metadata.modified()? {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "source changed during preview extraction",
        ));
    }
    let candidates = candidates(&bytes);
    let mut candidates = candidates;
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.pixels));
    let mut selected = None;
    for candidate in candidates {
        let data = &bytes[candidate.range.clone()];
        let mut reader = image::ImageReader::new(std::io::Cursor::new(data))
            .with_guessed_format()
            .map_err(io::Error::other)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(MAX_DECODE_BYTES);
        reader.limits(limits);
        if reader.decode().is_ok() {
            selected = Some(data);
            break;
        }
    }
    let jpeg = selected.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "no decodable embedded JPEG preview",
        )
    })?;
    write_atomic(&cached, jpeg)?;
    Ok(cached)
}

#[derive(Debug)]
struct Candidate {
    range: std::ops::Range<usize>,
    pixels: u64,
}

fn candidates(bytes: &[u8]) -> Vec<Candidate> {
    let mut found = Vec::new();
    let mut cursor = 0;
    let mut attempts = 0;
    while found.len() < MAX_CANDIDATES && attempts < MAX_ATTEMPTS {
        let Some(relative) = bytes[cursor..]
            .windows(3)
            .position(|window| window[0..2] == [0xff, 0xd8] && window[2] == 0xff)
        else {
            break;
        };
        let start = cursor + relative;
        attempts += 1;
        let bounded_end = start.saturating_add(MAX_JPEG_BYTES).min(bytes.len());
        match jpeg_end(&bytes[..bounded_end], start) {
            Some((end, width, height)) if width <= MAX_SIDE && height <= MAX_SIDE => {
                let pixels = u64::from(width) * u64::from(height);
                if pixels > 0 && pixels <= MAX_PIXELS {
                    found.push(Candidate {
                        range: start..end,
                        pixels,
                    });
                    cursor = end;
                } else {
                    cursor = start + 2;
                }
            }
            _ => cursor = start + 2,
        }
    }
    found
}

/// Returns the exclusive EOI offset and frame dimensions for a JPEG.
fn jpeg_end(bytes: &[u8], start: usize) -> Option<(usize, u32, u32)> {
    if bytes.get(start..start + 2)? != [0xff, 0xd8] {
        return None;
    }
    let mut cursor = start + 2;
    let mut in_scan = false;
    let mut dimensions = None;
    loop {
        let marker = if in_scan {
            in_scan = false;
            loop {
                while bytes.get(cursor) != Some(&0xff) {
                    cursor += 1;
                    bytes.get(cursor)?;
                }
                while bytes.get(cursor) == Some(&0xff) {
                    cursor += 1;
                }
                let code = *bytes.get(cursor)?;
                cursor += 1;
                match code {
                    0x00 | 0xd0..=0xd7 => continue,
                    _ => break code,
                }
            }
        } else {
            while bytes.get(cursor) == Some(&0xff) {
                cursor += 1;
            }
            if bytes.get(cursor - 1) != Some(&0xff) {
                return None;
            }
            let marker = *bytes.get(cursor)?;
            cursor += 1;
            marker
        };
        if marker == 0xd9 {
            return dimensions.map(|(width, height)| (cursor, width, height));
        }
        if matches!(marker, 0xd8 | 0x01 | 0xd0..=0xd7) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(cursor)?,
            *bytes.get(cursor + 1)?,
        ]));
        if length < 2 {
            return None;
        }
        let segment_start = cursor + 2;
        let segment_end = cursor.checked_add(length)?;
        let payload = bytes.get(segment_start..segment_end)?;
        if matches!(marker, 0xc0..=0xc2) && payload.len() >= 5 && payload[0] == 8 {
            dimensions = Some((
                u32::from(u16::from_be_bytes([payload[3], payload[4]])),
                u32::from(u16::from_be_bytes([payload[1], payload[2]])),
            ));
        }
        cursor = segment_end;
        if marker == 0xda {
            in_scan = true;
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let temporary = parent.join(format!(
        ".raw-{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn create_cache_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Seek, SeekFrom, Write};
    use std::time::{Duration, SystemTime};

    use image::ImageEncoder;

    use super::{EXTENSIONS, SCAN_LIMIT, candidates, extract_cached, supported};

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let pixels = vec![180u8; (width * height * 3) as usize];
        let mut encoded = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut encoded)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgb8)
            .expect("encode fixture JPEG");
        encoded.into_inner()
    }

    fn raw(path: &std::path::Path, jpegs: &[Vec<u8>]) {
        let mut bytes = b"RAW-like TIFF wrapper".to_vec();
        for jpeg in jpegs {
            bytes.extend(jpeg);
            bytes.extend(b"padding");
        }
        std::fs::write(path, bytes).expect("write fixture");
    }

    fn set_mtime(path: &std::path::Path, seconds: u64) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open fixture")
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .expect("set fixture mtime");
    }

    #[test]
    fn discovers_supported_extensions_case_insensitively() {
        for extension in EXTENSIONS {
            assert!(supported(std::path::Path::new(&format!(
                "photo.{extension}"
            ))));
            assert!(supported(std::path::Path::new(&format!(
                "photo.{}",
                extension.to_uppercase()
            ))));
        }
        assert!(!supported(std::path::Path::new("photo.jpeg")));
    }

    #[test]
    fn parses_and_selects_largest_valid_preview() {
        let small = jpeg(12, 8);
        let large = jpeg(64, 32);
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("capture.NEF");
        raw(&path, &[small, large.clone()]);
        let result = extract_cached(&path, &dir.path().join("cache")).expect("embedded preview");
        assert_eq!(std::fs::read(result).expect("cached JPEG"), large);
    }

    #[test]
    fn cache_reuses_source_identity_and_invalidates_on_source_state_change() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let cache = dir.path().join("cache");
        let source = dir.path().join("one.nef");
        let header = b"RAW-like wrapper";
        let bytes = [header.as_slice(), &jpeg(32, 24)].concat();
        std::fs::write(&source, &bytes).expect("write source");
        set_mtime(&source, 1_600_000_000);

        let first = extract_cached(&source, &cache).expect("first extraction");
        let reused = extract_cached(&source, &cache).expect("cached extraction");
        assert_eq!(first, reused);
        assert_eq!(
            std::fs::read(&first).expect("preview"),
            bytes[header.len()..]
        );

        let same_bytes_elsewhere = dir.path().join("two.nef");
        std::fs::write(&same_bytes_elsewhere, &bytes).expect("write second source");
        set_mtime(&same_bytes_elsewhere, 1_600_000_000);
        let second_source = extract_cached(&same_bytes_elsewhere, &cache).expect("second source");
        assert_ne!(first, second_source);

        let mut changed = bytes.clone();
        changed[0] ^= 1;
        std::fs::write(&source, changed).expect("change source without changing size");
        set_mtime(&source, 1_600_000_001);
        let invalidated = extract_cached(&source, &cache).expect("invalidated cache");
        assert_ne!(first, invalidated);
    }

    #[test]
    fn concurrent_extractions_publish_complete_jpegs_without_part_files() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let cache = dir.path().join("cache");
        let source = dir.path().join("camera.arw");
        let preview = jpeg(80, 48);
        raw(&source, std::slice::from_ref(&preview));
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| extract_cached(&source, &cache)))
                .collect();
            for worker in workers {
                let cached = worker.join().expect("worker").expect("extraction");
                assert_eq!(
                    std::fs::read(cached).expect("complete cached JPEG"),
                    preview
                );
            }
        });
        assert_eq!(
            std::fs::read_dir(&cache)
                .expect("cache directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
                .count(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn cache_directory_and_extracted_jpeg_have_private_modes() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::TempDir::new().expect("temp dir");
        let cache = dir.path().join("cache");
        let source = dir.path().join("sample.dng");
        raw(&source, &[jpeg(16, 12)]);
        let extracted = extract_cached(&source, &cache).expect("extraction");
        assert_eq!(
            std::fs::metadata(cache)
                .expect("cache metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(extracted)
                .expect("JPEG metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn false_soi_pairs_do_not_hide_a_later_preview() {
        let valid = jpeg(24, 16);
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("capture.nef");
        let mut bytes = b"RAW data".to_vec();
        for _ in 0..40 {
            bytes.extend([0xff, 0xd8, 0x00]);
        }
        bytes.extend(&valid);
        std::fs::write(&path, bytes).expect("write fixture");
        let result = extract_cached(&path, &dir.path().join("cache")).expect("preview");
        assert_eq!(std::fs::read(result).expect("cached JPEG"), valid);
    }

    #[test]
    fn skips_fake_soi_markers_inside_app_payload_and_rejects_truncation() {
        let nested = jpeg(40, 30);
        let mut outer = jpeg(20, 16);
        let payload = [b"EXIF".as_slice(), &nested].concat();
        let mut app = vec![0xff, 0xe1];
        app.extend(((payload.len() + 2) as u16).to_be_bytes());
        app.extend(payload);
        outer.splice(2..2, app);
        assert_eq!(candidates(&outer).len(), 1);
        assert!(candidates(&nested[..nested.len() - 2]).is_empty());
    }

    #[test]
    fn ignores_lossless_sensor_jpeg_frames() {
        let mut sensor = vec![0xff, 0xd8, 0xff, 0xc3, 0x00, 0x0b];
        sensor.extend([8, 0, 16, 0, 16, 3, 1, 0x11, 0]);
        sensor.extend([0xff, 0xda, 0x00, 0x08, 1, 1, 0, 0, 0x3f, 0, 0, 0xff, 0xd9]);
        assert!(candidates(&sensor).is_empty());
    }

    #[test]
    fn falls_back_from_corrupt_large_candidate_and_dispatches_f3() {
        let mut corrupt = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08];
        corrupt.extend(4_000u16.to_be_bytes());
        corrupt.extend(4_000u16.to_be_bytes());
        corrupt.extend([3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        corrupt.extend([0xff, 0xda, 0x00, 0x08, 1, 1, 0, 0, 0x3f, 0, 0xff, 0xd9]);
        let valid = jpeg(32, 24);
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("capture.dng");
        raw(&path, &[corrupt, valid.clone()]);
        let extracted = extract_cached(&path, &dir.path().join("cache")).expect("fallback preview");
        assert_eq!(std::fs::read(extracted).expect("cached JPEG"), valid);

        let preview = crate::preview::Registry::with_defaults()
            .preview(&path)
            .expect("F3 preview");
        assert_eq!(preview.kind, "RAW image");
        assert!(matches!(
            preview.content,
            crate::preview::Content::Image { .. }
        ));
    }

    #[test]
    fn reports_missing_preview_and_bounds_source_scan() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("empty.dng");
        std::fs::write(&path, b"no embedded JPEG").expect("fixture");
        assert!(extract_cached(&path, &dir.path().join("cache")).is_err());
    }

    #[test]
    fn does_not_read_a_preview_after_the_scan_limit() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("late-preview.dng");
        let preview = jpeg(16, 12);
        let file = std::fs::File::create(&path).expect("create sparse fixture");
        file.set_len(SCAN_LIMIT + preview.len() as u64)
            .expect("size fixture");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open fixture");
        file.seek(SeekFrom::Start(SCAN_LIMIT))
            .expect("seek after scan limit");
        file.write_all(&preview).expect("write late preview");
        assert!(extract_cached(&path, &dir.path().join("cache")).is_err());
    }

    #[test]
    fn rejects_jpegs_over_dimension_or_decoded_memory_limits() {
        let mut too_wide = jpeg(16, 12);
        let sof = too_wide
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .expect("baseline SOF");
        too_wide[sof + 5..sof + 7].copy_from_slice(&20_001u16.to_be_bytes());
        assert!(candidates(&too_wide).is_empty());

        let mut too_large = jpeg(16, 12);
        let sof = too_large
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .expect("baseline SOF");
        too_large[sof + 5..sof + 7].copy_from_slice(&4_100u16.to_be_bytes());
        too_large[sof + 7..sof + 9].copy_from_slice(&4_100u16.to_be_bytes());
        assert!(candidates(&too_large).is_empty());
    }
}

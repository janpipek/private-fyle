//! Bounded parsing and selection of embedded JPEG previews in camera RAW data.

use std::io;
use std::path::Path;

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

/// Selects the largest decodable embedded JPEG in a bounded byte prefix.
///
/// Candidate count, encoded size, dimensions and decoded memory are capped.
pub fn select_jpeg(bytes: &[u8]) -> io::Result<&[u8]> {
    let mut candidates = candidates(bytes);
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.pixels));
    for candidate in candidates {
        let data = &bytes[candidate.range];
        let mut reader = image::ImageReader::new(std::io::Cursor::new(data))
            .with_guessed_format()
            .map_err(io::Error::other)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(MAX_DECODE_BYTES);
        reader.limits(limits);
        if reader.decode().is_ok() {
            return Ok(data);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "no decodable embedded JPEG preview",
    ))
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::ImageEncoder;

    use super::{EXTENSIONS, MAX_SIDE, SCAN_LIMIT, select_jpeg, supported};

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let pixels = vec![180u8; (width * height * 3) as usize];
        let mut encoded = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new(&mut encoded)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgb8)
            .expect("encode fixture JPEG");
        encoded.into_inner()
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
        assert_eq!(SCAN_LIMIT, 64 * 1024 * 1024);
    }

    #[test]
    fn selects_largest_decodable_candidate_and_falls_back_from_corrupt_larger() {
        let small = jpeg(12, 8);
        let large = jpeg(64, 32);
        let mut corrupt = vec![0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08];
        corrupt.extend(4_000u16.to_be_bytes());
        corrupt.extend(4_000u16.to_be_bytes());
        corrupt.extend([3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        corrupt.extend([0xff, 0xda, 0x00, 0x08, 1, 1, 0, 0, 0x3f, 0, 0xff, 0xd9]);
        let bytes = [corrupt, small, large.clone()].concat();
        assert_eq!(select_jpeg(&bytes).unwrap(), large);
    }

    #[test]
    fn bounded_parser_skips_payload_false_starts_and_rejects_truncation_and_large_frames() {
        let nested = jpeg(40, 30);
        let mut outer = jpeg(20, 16);
        let payload = [b"EXIF".as_slice(), &nested].concat();
        let mut app = vec![0xff, 0xe1];
        app.extend(((payload.len() + 2) as u16).to_be_bytes());
        app.extend(payload);
        outer.splice(2..2, app);
        assert_eq!(select_jpeg(&outer).unwrap(), outer);
        assert!(select_jpeg(&nested[..nested.len() - 2]).is_err());

        let mut too_wide = jpeg(16, 12);
        let sof = too_wide
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .unwrap();
        too_wide[sof + 5..sof + 7].copy_from_slice(&((MAX_SIDE + 1) as u16).to_be_bytes());
        assert!(select_jpeg(&too_wide).is_err());

        let mut too_many_pixels = jpeg(16, 12);
        let sof = too_many_pixels
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .unwrap();
        too_many_pixels[sof + 5..sof + 7].copy_from_slice(&4_100u16.to_be_bytes());
        too_many_pixels[sof + 7..sof + 9].copy_from_slice(&4_100u16.to_be_bytes());
        assert!(select_jpeg(&too_many_pixels).is_err());
    }

    #[test]
    fn ignores_lossless_sensor_jpeg_frames() {
        let sensor = [
            0xff, 0xd8, 0xff, 0xc3, 0x00, 0x0b, 8, 0, 16, 0, 16, 3, 1, 0x11, 0, 0xff, 0xda, 0x00,
            0x08, 1, 1, 0, 0, 0x3f, 0, 0, 0xff, 0xd9,
        ];
        assert!(select_jpeg(&sensor).is_err());
    }
}

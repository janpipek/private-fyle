//! Bounded, metadata-only EXIF reading for Properties.

use std::fmt::{self, Write as _};
use std::fs::File;
#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const IMAGE_LIMIT: u64 = 8 * 1024 * 1024;
const RAW_LIMIT: u64 = crate::formats::raw::SCAN_LIMIT;
const MAX_RAW_EXIF_CANDIDATES: usize = 16;
const MAX_RAW_TIFF_BYTES: usize = 1024 * 1024;
const MAX_FIELD_CHARS: usize = 4096;

/// A stable semantic EXIF field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExifField {
    /// Stable field identity, suitable for localization by a view.
    pub id: ExifFieldId,
    /// Reader-formatted value.
    pub value: String,
}

/// EXIF identities supported by the Properties reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExifFieldId {
    /// Camera manufacturer.
    Make,
    /// Camera model.
    Model,
    /// Lens model.
    LensModel,
    /// Original capture date and time.
    DateTimeOriginal,
    /// Exposure time.
    ExposureTime,
    /// Aperture number.
    FNumber,
    /// Photographic sensitivity.
    PhotographicSensitivity,
    /// Focal length.
    FocalLength,
    /// Pixel width.
    PixelXDimension,
    /// Pixel height.
    PixelYDimension,
    /// GPS latitude.
    GPSLatitude,
    /// GPS longitude.
    GPSLongitude,
    /// Metadata software.
    Software,
}

impl ExifFieldId {
    /// Stable machine identity independent of Rust's debug formatting.
    pub fn id(self) -> &'static str {
        match self {
            Self::Make => "make",
            Self::Model => "model",
            Self::LensModel => "lens_model",
            Self::DateTimeOriginal => "date_time_original",
            Self::ExposureTime => "exposure_time",
            Self::FNumber => "f_number",
            Self::PhotographicSensitivity => "photographic_sensitivity",
            Self::FocalLength => "focal_length",
            Self::PixelXDimension => "pixel_x_dimension",
            Self::PixelYDimension => "pixel_y_dimension",
            Self::GPSLatitude => "gps_latitude",
            Self::GPSLongitude => "gps_longitude",
            Self::Software => "software",
        }
    }
}

#[derive(Debug)]
pub(super) struct ReadResult {
    pub fields: Vec<ExifField>,
    pub limit: u64,
    pub truncated: bool,
}

/// Reads only EXIF containers; no image decoder or embedded JPEG decoder runs.
pub(super) fn read(path: &Path, cancel: &AtomicBool) -> io::Result<ReadResult> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let raw = crate::formats::raw::supported(path);
    let limit = if raw { RAW_LIMIT } else { IMAGE_LIMIT };
    let file = open_regular_file(path)?;
    let capped = Capped {
        inner: file,
        read: 0,
        limit,
        cancel,
        hit_limit: false,
    };
    let (fields, truncated) = if raw {
        let mut bytes = Vec::new();
        let mut reader = BufReader::new(capped);
        reader.read_to_end(&mut bytes)?;
        let hit_limit = reader.into_inner().hit_limit;
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let (fields, value_truncated) = parse_raw_or_container(bytes, cancel)?;
        (fields, hit_limit || value_truncated)
    } else {
        let mut reader = BufReader::new(capped);
        let (fields, value_truncated) = parse_container(&mut reader)?;
        (fields, reader.into_inner().hit_limit || value_truncated)
    };
    Ok(ReadResult {
        fields,
        limit,
        truncated,
    })
}

#[cfg(target_os = "linux")]
fn open_regular_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "EXIF source is not a regular file",
        ));
    }
    Ok(file)
}

#[cfg(not(target_os = "linux"))]
fn open_regular_file(path: &Path) -> io::Result<File> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "EXIF source is not a regular file",
        ));
    }
    Ok(file)
}

fn parse_raw_or_container(
    bytes: Vec<u8>,
    cancel: &AtomicBool,
) -> io::Result<(Vec<ExifField>, bool)> {
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return collect(exif::Reader::new().read_raw(bytes));
    }
    let fields =
        collect(exif::Reader::new().read_from_container(&mut Cursor::new(bytes.as_slice())))?;
    if !fields.0.is_empty() {
        return Ok(fields);
    }
    // A RAW file may store EXIF in an embedded JPEG APP1 segment. Parse the
    // TIFF payload directly; this deliberately avoids image decoding.
    let mut cursor = 0;
    let mut candidates = 0;
    while candidates < MAX_RAW_EXIF_CANDIDATES
        && let Some(offset) = bytes[cursor..]
            .windows(6)
            .position(|window| window == b"Exif\0\0")
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        candidates += 1;
        let start = cursor + offset + 6;
        let end = start.saturating_add(MAX_RAW_TIFF_BYTES).min(bytes.len());
        let Some(tiff) = bytes.get(start..end) else {
            break;
        };
        if (tiff.starts_with(b"II*\0") || tiff.starts_with(b"MM\0*"))
            && let Ok(fields) = collect(exif::Reader::new().read_raw(tiff.to_vec()))
            && !fields.0.is_empty()
        {
            return Ok(fields);
        }
        cursor = start;
        if cursor >= bytes.len() {
            break;
        }
    }
    Ok((Vec::new(), false))
}

fn parse_container<R: io::BufRead + Seek>(reader: &mut R) -> io::Result<(Vec<ExifField>, bool)> {
    collect(exif::Reader::new().read_from_container(reader))
}

fn collect(parsed: Result<exif::Exif, exif::Error>) -> io::Result<(Vec<ExifField>, bool)> {
    let exif = match parsed {
        Ok(exif) => exif,
        Err(exif::Error::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Ok((Vec::new(), false));
        }
        Err(exif::Error::Io(error)) => return Err(error),
        Err(_) => return Ok((Vec::new(), false)),
    };
    const TAGS: &[(exif::Tag, ExifFieldId)] = &[
        (exif::Tag::Make, ExifFieldId::Make),
        (exif::Tag::Model, ExifFieldId::Model),
        (exif::Tag::LensModel, ExifFieldId::LensModel),
        (exif::Tag::DateTimeOriginal, ExifFieldId::DateTimeOriginal),
        (exif::Tag::ExposureTime, ExifFieldId::ExposureTime),
        (exif::Tag::FNumber, ExifFieldId::FNumber),
        (
            exif::Tag::PhotographicSensitivity,
            ExifFieldId::PhotographicSensitivity,
        ),
        (exif::Tag::FocalLength, ExifFieldId::FocalLength),
        (exif::Tag::PixelXDimension, ExifFieldId::PixelXDimension),
        (exif::Tag::PixelYDimension, ExifFieldId::PixelYDimension),
        (exif::Tag::GPSLatitude, ExifFieldId::GPSLatitude),
        (exif::Tag::GPSLongitude, ExifFieldId::GPSLongitude),
        (exif::Tag::Software, ExifFieldId::Software),
    ];
    let mut truncated = false;
    let fields = TAGS
        .iter()
        .filter_map(|&(tag, id)| {
            let field = exif.get_field(tag, exif::In::PRIMARY)?;
            let mut bounded = BoundedString {
                value: String::new(),
                truncated: false,
            };
            let _ = write!(&mut bounded, "{}", field.display_value().with_unit(&exif));
            truncated |= bounded.truncated;
            let value = bounded.value.trim_matches('"').trim().to_owned();
            (!value.is_empty()).then_some(ExifField { id, value })
        })
        .collect();
    Ok((fields, truncated))
}

struct BoundedString {
    value: String,
    truncated: bool,
}

impl fmt::Write for BoundedString {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = MAX_FIELD_CHARS.saturating_sub(self.value.chars().count());
        let end = value
            .char_indices()
            .nth(remaining)
            .map_or(value.len(), |(index, _)| index);
        self.value.push_str(&value[..end]);
        self.truncated |= end < value.len();
        Ok(())
    }
}

struct Capped<'a> {
    inner: File,
    read: u64,
    limit: u64,
    cancel: &'a AtomicBool,
    hit_limit: bool,
}
impl Read for Capped<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let remaining = self.limit.saturating_sub(self.read);
        if remaining == 0 {
            self.hit_limit = !buffer.is_empty()
                && self
                    .inner
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() > self.limit);
            return Ok(0);
        }
        let size = buffer.len().min(remaining as usize);
        let count = self.inner.read(&mut buffer[..size])?;
        self.read += count as u64;
        if self.read == self.limit {
            self.hit_limit = self
                .inner
                .metadata()
                .is_ok_and(|metadata| metadata.len() > self.limit);
        }
        Ok(count)
    }
}
impl Seek for Capped<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

#[cfg(test)]
mod tests {
    use super::{Capped, parse_raw_or_container, read};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn capped_reader_stops_at_limit_and_marks_truncation() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let path = temp.path().join("source.raw");
        std::fs::write(&path, b"12345").expect("write");
        let file = std::fs::File::open(&path).expect("open");
        let cancel = AtomicBool::new(false);
        let mut capped = Capped {
            inner: file,
            read: 0,
            limit: 4,
            cancel: &cancel,
            hit_limit: false,
        };
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut capped, &mut bytes).expect("bounded read");
        assert_eq!(bytes, b"1234");
        assert!(capped.hit_limit);
    }

    #[test]
    fn reads_raw_embedded_exif_without_decoding_jpeg() {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend(1_u16.to_le_bytes());
        tiff.extend(0x010f_u16.to_le_bytes());
        tiff.extend(2_u16.to_le_bytes());
        tiff.extend(5_u32.to_le_bytes());
        tiff.extend(26_u32.to_le_bytes());
        tiff.extend(0_u32.to_le_bytes());
        tiff.extend(b"Acme\0");
        let mut raw = b"raw-prefix".to_vec();
        raw.extend(b"Exif\0\0");
        raw.extend(tiff);

        let fields = parse_raw_or_container(raw, &AtomicBool::new(false))
            .expect("bounded metadata parse")
            .0;
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].id.id(), "make");
        assert_eq!(fields[0].value, "Acme");
    }

    #[test]
    fn reads_exif_from_a_jpeg_app1_container() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let path = temp.path().join("photo.jpg");
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend(1_u16.to_le_bytes());
        tiff.extend(0x010f_u16.to_le_bytes());
        tiff.extend(2_u16.to_le_bytes());
        tiff.extend(5_u32.to_le_bytes());
        tiff.extend(26_u32.to_le_bytes());
        tiff.extend(0_u32.to_le_bytes());
        tiff.extend(b"Acme\0");
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(tiff);
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(((app1.len() + 2) as u16).to_be_bytes());
        jpeg.extend(app1);
        jpeg.extend([0xff, 0xd9]);
        std::fs::write(&path, jpeg).expect("write JPEG fixture");

        let fields = read(&path, &AtomicBool::new(false))
            .expect("read JPEG metadata")
            .fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].id.id(), "make");
        assert_eq!(fields[0].value, "Acme");
    }

    #[test]
    fn cancelled_reader_stops_before_opening_metadata() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let path = temp.path().join("photo.jpg");
        std::fs::write(&path, b"not an image").expect("write");
        let cancel = AtomicBool::new(true);
        assert_eq!(
            read(&path, &cancel).expect_err("cancelled").kind(),
            std::io::ErrorKind::Interrupted
        );
    }
}

//! Shared semantic EXIF parsing and bounded metadata-only Properties reading.

use std::fmt::{self, Write as _};
use std::fs::File;
#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// A supported EXIF tag with one stable presentation-independent identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tag {
    /// Camera manufacturer.
    Make,
    /// Camera model.
    Model,
    /// Lens model name.
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
    /// Image width.
    PixelXDimension,
    /// Image height.
    PixelYDimension,
    /// GPS latitude.
    GPSLatitude,
    /// GPS longitude.
    GPSLongitude,
    /// Software named by the metadata.
    Software,
}

impl Tag {
    /// Stable machine identity shared by Properties and EXIF columns.
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

/// One nonempty EXIF value in stable semantic order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Semantic identity of this parsed value.
    pub tag: Tag,
    /// Library-formatted value with display quotes normalized.
    pub value: String,
}

/// Properties result from its stricter metadata-only reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertiesRead {
    /// Semantic EXIF fields found within the source prefix.
    pub fields: Vec<Field>,
    /// Ordinary-image or RAW prefix limit applied to this read.
    pub limit: u64,
    /// Whether the source or a field value exceeded a reader limit.
    pub truncated: bool,
}

const TAGS: &[(exif::Tag, Tag)] = &[
    (exif::Tag::Make, Tag::Make),
    (exif::Tag::Model, Tag::Model),
    (exif::Tag::LensModel, Tag::LensModel),
    (exif::Tag::DateTimeOriginal, Tag::DateTimeOriginal),
    (exif::Tag::ExposureTime, Tag::ExposureTime),
    (exif::Tag::FNumber, Tag::FNumber),
    (
        exif::Tag::PhotographicSensitivity,
        Tag::PhotographicSensitivity,
    ),
    (exif::Tag::FocalLength, Tag::FocalLength),
    (exif::Tag::PixelXDimension, Tag::PixelXDimension),
    (exif::Tag::PixelYDimension, Tag::PixelYDimension),
    (exif::Tag::GPSLatitude, Tag::GPSLatitude),
    (exif::Tag::GPSLongitude, Tag::GPSLongitude),
    (exif::Tag::Software, Tag::Software),
];

const IMAGE_LIMIT: u64 = 8 * 1024 * 1024;
const RAW_LIMIT: u64 = crate::formats::raw::SCAN_LIMIT;
const MAX_RAW_EXIF_CANDIDATES: usize = 16;
const MAX_RAW_TIFF_BYTES: usize = 1024 * 1024;
const MAX_FIELD_CHARS: usize = 4096;

/// Reads the legacy column metadata with its existing extension gate supplied by the caller.
///
/// Ordinary image reads retain the existing seekable-reader behavior. RAW files
/// read only the common 64 MiB prefix and use the existing bounded JPEG selector.
pub fn read(path: &Path) -> io::Result<Vec<Field>> {
    if crate::formats::raw::supported(path) {
        let mut bytes = Vec::new();
        File::open(path)?.take(RAW_LIMIT).read_to_end(&mut bytes)?;
        let (fields, _) = parse_raw_or_container(&bytes, None, false)?;
        if !fields.is_empty() {
            return Ok(fields);
        }
        let mut bytes = Vec::new();
        File::open(path)?.take(RAW_LIMIT).read_to_end(&mut bytes)?;
        let Ok(jpeg) = crate::formats::raw::select_jpeg(&bytes) else {
            return Ok(Vec::new());
        };
        return Ok(parse_container(Cursor::new(jpeg), None, false)?.0);
    }

    let file = File::open(path)?;
    Ok(parse_container(BufReader::new(file), None, false)?.0)
}

/// Reads bounded EXIF facts for Properties without decoding or materializing images.
pub fn read_properties(path: &Path, cancel: &AtomicBool) -> io::Result<PropertiesRead> {
    read_properties_impl(path, cancel, false)
}

/// Reads bounded EXIF facts and preserves malformed and truncated parse failures.
pub fn read_properties_checked(path: &Path, cancel: &AtomicBool) -> io::Result<PropertiesRead> {
    read_properties_impl(path, cancel, true)
}

fn read_properties_impl(
    path: &Path,
    cancel: &AtomicBool,
    strict: bool,
) -> io::Result<PropertiesRead> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let raw = crate::formats::raw::supported(path);
    let limit = if raw { RAW_LIMIT } else { IMAGE_LIMIT };
    let capped = Capped {
        inner: open_regular_file(path)?,
        read: 0,
        limit,
        cancel,
        hit_limit: false,
    };
    let (fields, truncated) = if raw {
        let mut reader = BufReader::new(capped);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        let hit_limit = reader.into_inner().hit_limit;
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let (fields, value_truncated) = parse_properties_raw(&bytes, cancel, strict)?;
        (fields, hit_limit || value_truncated)
    } else {
        let mut reader = BufReader::new(capped);
        let (fields, value_truncated) =
            parse_container(&mut reader, Some(MAX_FIELD_CHARS), strict)?;
        let hit_limit = reader.into_inner().hit_limit;
        (fields, hit_limit || value_truncated)
    };
    Ok(PropertiesRead {
        fields,
        limit,
        truncated,
    })
}

fn parse_properties_raw(
    bytes: &[u8],
    cancel: &AtomicBool,
    strict: bool,
) -> io::Result<(Vec<Field>, bool)> {
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return parse_raw_tiff(bytes, Some(MAX_FIELD_CHARS), strict);
    }

    let mut candidate_error = None;
    match parse_container(Cursor::new(bytes), Some(MAX_FIELD_CHARS), strict) {
        Ok((fields, truncated)) if !fields.is_empty() => return Ok((fields, truncated)),
        Ok(_) => {}
        Err(error) if strict => candidate_error = Some(error),
        Err(error) => return Err(error),
    }

    // Some RAW files keep EXIF only inside JPEG APP1 segments. Parse the TIFF
    // payload directly, bounded by candidate count and bytes, without image decoding.
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
        if tiff.starts_with(b"II*\0") || tiff.starts_with(b"MM\0*") {
            match parse_raw_tiff(tiff, Some(MAX_FIELD_CHARS), strict) {
                Ok((fields, truncated)) if !fields.is_empty() => {
                    return Ok((fields, truncated));
                }
                Ok(_) => {}
                Err(error) if strict => {
                    candidate_error.get_or_insert(error);
                }
                Err(_) => {}
            };
        }
        cursor = start;
        if cursor >= bytes.len() {
            break;
        }
    }
    if let Some(error) = candidate_error {
        return Err(error);
    }
    Ok((Vec::new(), false))
}

fn parse_raw_or_container(
    bytes: &[u8],
    max_chars: Option<usize>,
    strict: bool,
) -> io::Result<(Vec<Field>, bool)> {
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        parse_raw_tiff(bytes, max_chars, strict)
    } else {
        parse_container(Cursor::new(bytes), max_chars, strict)
    }
}

fn parse_raw_tiff(
    bytes: &[u8],
    max_chars: Option<usize>,
    strict: bool,
) -> io::Result<(Vec<Field>, bool)> {
    collect_result(
        exif::Reader::new().read_raw(bytes.to_vec()),
        max_chars,
        strict,
    )
}

fn parse_container<R: io::BufRead + Seek>(
    mut reader: R,
    max_chars: Option<usize>,
    strict: bool,
) -> io::Result<(Vec<Field>, bool)> {
    collect_result(
        exif::Reader::new().read_from_container(&mut reader),
        max_chars,
        strict,
    )
}

fn collect_result(
    parsed: Result<exif::Exif, exif::Error>,
    max_chars: Option<usize>,
    strict: bool,
) -> io::Result<(Vec<Field>, bool)> {
    match parsed {
        Ok(exif) => Ok(collect(&exif, max_chars)),
        Err(exif::Error::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof && !strict => {
            Ok((Vec::new(), false))
        }
        Err(exif::Error::Io(error)) => Err(error),
        Err(exif::Error::NotFound(_)) => Ok((Vec::new(), false)),
        Err(error) if strict => Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(_) => Ok((Vec::new(), false)),
    }
}

fn collect(exif: &exif::Exif, max_chars: Option<usize>) -> (Vec<Field>, bool) {
    let mut truncated = false;
    let fields = TAGS
        .iter()
        .filter_map(|&(exif_tag, tag)| {
            let field = exif.get_field(exif_tag, exif::In::PRIMARY)?;
            let mut bounded = BoundedString::new(max_chars);
            let _ = write!(&mut bounded, "{}", field.display_value().with_unit(exif));
            truncated |= bounded.truncated;
            let value = bounded.value.trim_matches('"').trim().to_owned();
            (!value.is_empty()).then_some(Field { tag, value })
        })
        .collect();
    (fields, truncated)
}

struct BoundedString {
    value: String,
    max_chars: Option<usize>,
    truncated: bool,
}

impl BoundedString {
    fn new(max_chars: Option<usize>) -> Self {
        Self {
            value: String::new(),
            max_chars,
            truncated: false,
        }
    }
}

impl fmt::Write for BoundedString {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let Some(limit) = self.max_chars else {
            self.value.push_str(value);
            return Ok(());
        };
        let remaining = limit.saturating_sub(self.value.chars().count());
        let end = value
            .char_indices()
            .nth(remaining)
            .map_or(value.len(), |(index, _)| index);
        self.value.push_str(&value[..end]);
        self.truncated |= end < value.len();
        Ok(())
    }
}

fn open_regular_file(path: &Path) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
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
    {
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "EXIF source is not a regular file",
            ));
        }
        Ok(file)
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
    use image::ImageEncoder;

    use super::{Field, Tag, read, read_properties, read_properties_checked};

    fn jpeg_with_exif() -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        let entries: u16 = 2;
        tiff.extend(entries.to_le_bytes());
        let data_start = 8 + 2 + 12 * entries as u32 + 4;
        let (make, model) = (b"Acme\0".as_slice(), b"Snap 1\0".as_slice());
        for (tag, value, offset) in [
            (0x010f_u16, make, data_start),
            (0x0110_u16, model, data_start + make.len() as u32),
        ] {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(2u16.to_le_bytes());
            tiff.extend((value.len() as u32).to_le_bytes());
            tiff.extend(offset.to_le_bytes());
        }
        tiff.extend(0u32.to_le_bytes());
        tiff.extend(make);
        tiff.extend(model);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(tiff);
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(((app1.len() + 2) as u16).to_be_bytes());
        jpeg.extend(app1);
        jpeg.extend([0xff, 0xd9]);
        jpeg
    }

    fn decodable_jpeg_with_exif() -> Vec<u8> {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .write_image(&[128, 128, 128], 1, 1, image::ExtendedColorType::Rgb8)
            .expect("encode JPEG fixture");
        let exif = jpeg_with_exif();
        jpeg.splice(2..2, exif[2..exif.len() - 2].iter().copied());
        jpeg
    }

    #[test]
    fn parses_one_stable_semantic_tag_order_for_the_unbounded_column_reader() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("photo.jpg");
        std::fs::write(&path, jpeg_with_exif()).expect("write JPEG fixture");
        assert_eq!(
            read(&path).expect("read EXIF"),
            [
                Field {
                    tag: Tag::Make,
                    value: "Acme".into()
                },
                Field {
                    tag: Tag::Model,
                    value: "Snap 1".into()
                },
            ]
        );
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").expect("write no-EXIF fixture");
        assert!(read(&path).expect("missing EXIF is empty").is_empty());
    }

    #[test]
    fn reads_direct_tiff_and_embedded_jpeg_with_bounded_properties_reader() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let jpeg = jpeg_with_exif();
        let direct = temp.path().join("direct.dng");
        std::fs::write(&direct, &jpeg[12..jpeg.len() - 2]).expect("write TIFF metadata");
        assert_eq!(
            read_properties(&direct, &Default::default())
                .expect("read direct TIFF")
                .fields[1]
                .value,
            "Snap 1"
        );

        let marker = jpeg
            .windows(6)
            .position(|window| window == b"Exif\0\0")
            .expect("JPEG fixture has an EXIF marker");
        let embedded_tiff = temp.path().join("embedded-tiff.cr3");
        let mut raw = b"RAW prefix".to_vec();
        raw.extend(&jpeg[marker..jpeg.len() - 2]);
        std::fs::write(&embedded_tiff, raw).expect("write embedded TIFF fixture");
        assert_eq!(
            read_properties(&embedded_tiff, &Default::default())
                .expect("read TIFF payload without decoding its container")
                .fields[0]
                .value,
            "Acme"
        );

        let embedded = temp.path().join("embedded.cr3");
        let mut raw = b"RAW wrapper".to_vec();
        raw.extend(decodable_jpeg_with_exif());
        std::fs::write(&embedded, raw).expect("write RAW fixture");
        assert_eq!(
            read_properties(&embedded, &Default::default())
                .expect("read embedded EXIF without image decoding")
                .fields[1]
                .value,
            "Snap 1"
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 3);
    }

    #[test]
    fn bounded_properties_reader_cancels_and_reports_its_input_limit() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("cancel.jpg");
        std::fs::write(&path, jpeg_with_exif()).expect("write JPEG");
        assert_eq!(
            read_properties(&path, &std::sync::atomic::AtomicBool::new(true))
                .expect_err("pre-cancelled read")
                .kind(),
            std::io::ErrorKind::Interrupted
        );

        let limit = 4;
        let file = std::fs::File::open(&path).expect("open fixture");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut capped = super::Capped {
            inner: file,
            read: 0,
            limit,
            cancel: &cancel,
            hit_limit: false,
        };
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut capped, &mut bytes).expect("bounded read");
        assert_eq!(bytes.len(), 4);
        assert!(capped.hit_limit);
    }

    #[test]
    fn checked_properties_reader_preserves_malformed_parse_errors() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("malformed.jpg");
        std::fs::write(&path, b"not an image").expect("write malformed image");

        assert!(
            read_properties(&path, &Default::default())
                .expect("legacy reader keeps treating parse errors as absence")
                .fields
                .is_empty()
        );
        assert_eq!(
            read_properties_checked(&path, &Default::default())
                .expect_err("checked reader preserves parse errors")
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            read_properties_checked(&temp.path().join("missing.jpg"), &Default::default())
                .expect_err("missing files remain distinguishable")
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn file_open_errors_are_reported() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        assert!(read(&temp.path().join("missing.jpg")).is_err());
    }
}

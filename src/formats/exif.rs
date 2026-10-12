//! Semantic EXIF fields for images and camera RAW files.

use std::fs::File;
use std::io::{self, BufReader, Cursor, Read};
use std::path::Path;

/// A semantic EXIF field that consumers can format for their own purpose.
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

/// A nonempty EXIF value with its semantic field identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// The semantic identity of the parsed EXIF value.
    pub tag: Tag,
    /// The library-formatted value with units and display quotes normalized.
    pub value: String,
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

/// Reads available EXIF values in stable semantic order.
///
/// Unreadable files return an I/O error; unsupported, malformed or absent EXIF
/// data returns an empty list. Camera RAW data is inspected only through the
/// shared scan bound, and embedded JPEG fallback is parsed without caching.
pub fn read(path: &Path) -> io::Result<Vec<Field>> {
    if crate::formats::raw::supported(path) {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(crate::formats::raw::SCAN_LIMIT)
            .read_to_end(&mut bytes)?;
        let fields = read_raw_or_container(bytes)?;
        if !fields.is_empty() {
            return Ok(fields);
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(crate::formats::raw::SCAN_LIMIT)
            .read_to_end(&mut bytes)?;
        let Ok(jpeg) = crate::formats::raw::select_jpeg(&bytes) else {
            return Ok(Vec::new());
        };
        return read_container(Cursor::new(jpeg));
    }

    let file = File::open(path)?;
    read_container(BufReader::new(file))
}

fn read_raw_or_container(bytes: Vec<u8>) -> io::Result<Vec<Field>> {
    let parsed = if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        exif::Reader::new().read_raw(bytes)
    } else {
        exif::Reader::new().read_from_container(&mut Cursor::new(bytes))
    };
    collect_result(parsed)
}

fn read_container<R: io::BufRead + io::Seek>(mut reader: R) -> io::Result<Vec<Field>> {
    collect_result(exif::Reader::new().read_from_container(&mut reader))
}

fn collect_result(parsed: Result<exif::Exif, exif::Error>) -> io::Result<Vec<Field>> {
    match parsed {
        Ok(exif) => Ok(collect(&exif)),
        Err(exif::Error::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
            Ok(Vec::new())
        }
        Err(exif::Error::Io(error)) => Err(error),
        Err(_) => Ok(Vec::new()),
    }
}

fn collect(exif: &exif::Exif) -> Vec<Field> {
    TAGS.iter()
        .filter_map(|&(exif_tag, tag)| {
            let field = exif.get_field(exif_tag, exif::In::PRIMARY)?;
            let value = field.display_value().with_unit(exif).to_string();
            let value = value.trim_matches('"').trim().to_string();
            (!value.is_empty()).then_some(Field { tag, value })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use image::ImageEncoder;

    use super::{Field, Tag, read};

    fn jpeg_with_exif() -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        let entries: u16 = 2;
        tiff.extend(entries.to_le_bytes());
        let data_start = 8 + 2 + 12 * entries as u32 + 4;
        let (make, model) = (b"Acme\0".as_slice(), b"Snap 1\0".as_slice());
        let mut entry = |tag: u16, value: &[u8], offset: u32| {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(2u16.to_le_bytes());
            tiff.extend((value.len() as u32).to_le_bytes());
            tiff.extend(offset.to_le_bytes());
        };
        entry(0x010f, make, data_start);
        entry(0x0110, model, data_start + make.len() as u32);
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
    fn reads_ordered_semantic_fields_and_returns_empty_for_missing_exif() {
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
    fn reads_direct_tiff_metadata_and_raw_embedded_jpeg_without_writing_beside_source() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let direct = temp.path().join("photo.dng");
        let jpeg = jpeg_with_exif();
        std::fs::write(&direct, &jpeg[12..jpeg.len() - 2]).expect("write TIFF metadata");
        assert_eq!(read(&direct).expect("read TIFF EXIF")[1].value, "Snap 1");

        let embedded = temp.path().join("embedded.cr3");
        let mut raw = b"RAW wrapper".to_vec();
        raw.extend(decodable_jpeg_with_exif());
        std::fs::write(&embedded, raw).expect("write RAW fixture");
        assert_eq!(
            read(&embedded).expect("read embedded EXIF")[1].value,
            "Snap 1"
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[test]
    fn file_open_errors_are_reported() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        assert!(read(&temp.path().join("missing.jpg")).is_err());
    }
}

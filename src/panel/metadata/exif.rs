//! EXIF tag composition for file metadata columns.

use std::collections::HashMap;
use std::path::Path;

use crate::formats::exif::{Field, Tag};

/// Reads EXIF data only for supported image extensions and maps it to column keys.
pub fn exif_fields(path: &Path) -> HashMap<&'static str, String> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !crate::formats::raw::supported(path)
        && !matches!(
            extension.as_str(),
            "jpg" | "jpeg" | "tif" | "tiff" | "png" | "webp" | "heic" | "heif" | "avif"
        )
    {
        return HashMap::new();
    }
    let fields = crate::formats::exif::read(path).unwrap_or_default();
    project(&fields)
}

fn project(fields: &[Field]) -> HashMap<&'static str, String> {
    let get = |tag| {
        fields
            .iter()
            .find(|field| field.tag == tag)
            .map(|field| field.value.clone())
    };
    let mut result = HashMap::new();
    let mut put = |key, value: Option<String>| {
        if let Some(value) = value {
            result.insert(key, value);
        }
    };
    put(
        "taken",
        get(Tag::DateTimeOriginal)
            .map(|taken: String| taken.get(..16).unwrap_or(&taken).to_string()),
    );
    let make = get(Tag::Make);
    let model = get(Tag::Model);
    put(
        "camera",
        match (make, model) {
            (Some(make), Some(model))
                if !model.to_lowercase().starts_with(&make.to_lowercase()) =>
            {
                Some(format!("{make} {model}"))
            }
            (make, model) => model.or(make),
        },
    );
    put("lens", get(Tag::LensModel));
    put("exposure", get(Tag::ExposureTime));
    put("aperture", get(Tag::FNumber));
    put("iso", get(Tag::PhotographicSensitivity));
    put("focal_length", get(Tag::FocalLength));
    put(
        "dimensions",
        get(Tag::PixelXDimension)
            .zip(get(Tag::PixelYDimension))
            .map(|(width, height)| format!("{width}×{height}")),
    );
    put(
        "gps",
        get(Tag::GPSLatitude)
            .zip(get(Tag::GPSLongitude))
            .map(|(latitude, longitude)| format!("{latitude}, {longitude}")),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::{exif_fields, project};
    use crate::formats::exif::{Field, Tag};

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

    #[test]
    fn projects_semantic_values_to_existing_column_keys_and_formatting() {
        let fields = [
            (Tag::DateTimeOriginal, "2024-05-01 12:30:45"),
            (Tag::Make, "Canon"),
            (Tag::Model, "canon EOS R6"),
            (Tag::LensModel, "RF 50mm"),
            (Tag::ExposureTime, "1/125 sec."),
            (Tag::FNumber, "f/2.8"),
            (Tag::PhotographicSensitivity, "ISO 400"),
            (Tag::FocalLength, "50 mm"),
            (Tag::PixelXDimension, "6000"),
            (Tag::PixelYDimension, "4000"),
            (Tag::GPSLatitude, "51 deg 30' 0\" N"),
            (Tag::GPSLongitude, "0 deg 7' 0\" W"),
        ]
        .map(|(tag, value)| Field {
            tag,
            value: value.into(),
        });
        let projected = project(&fields);
        assert_eq!(
            projected.get("taken").map(String::as_str),
            Some("2024-05-01 12:30")
        );
        assert_eq!(
            projected.get("camera").map(String::as_str),
            Some("canon EOS R6")
        );
        assert_eq!(
            projected.get("dimensions").map(String::as_str),
            Some("6000×4000")
        );
        assert_eq!(
            projected.get("gps").map(String::as_str),
            Some("51 deg 30' 0\" N, 0 deg 7' 0\" W")
        );
        assert_eq!(projected.len(), 9);

        let make_only = project(&[Field {
            tag: Tag::Make,
            value: "Pixel".into(),
        }]);
        assert_eq!(make_only.get("camera").map(String::as_str), Some("Pixel"));
    }

    #[test]
    fn extension_gate_skips_non_image_files() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("photo.txt");
        std::fs::write(&path, b"not an image").expect("write fixture");
        assert!(exif_fields(&path).is_empty());
    }

    #[test]
    fn projects_reader_values_for_jpeg_and_direct_tiff_raw_metadata() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let jpeg = jpeg_with_exif();
        let photo = temp.path().join("photo.jpg");
        std::fs::write(&photo, &jpeg).expect("write JPEG fixture");
        assert_eq!(
            exif_fields(&photo).get("camera").map(String::as_str),
            Some("Acme Snap 1")
        );

        let raw = temp.path().join("photo.dng");
        std::fs::write(&raw, &jpeg[12..jpeg.len() - 2]).expect("write direct TIFF metadata");
        assert_eq!(
            exif_fields(&raw).get("camera").map(String::as_str),
            Some("Acme Snap 1")
        );
    }
}

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;

use super::{Content, Previewer, Probe};

/// Formats GPUI can decode.
const EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "tif", "tiff",
];

pub struct ImagePreviewer;

impl Previewer for ImagePreviewer {
    fn name(&self) -> &'static str {
        "Image"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && EXTENSIONS.contains(&probe.ext.as_str())
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        // Decoding happens in the view, which caches images.
        Ok(Content::Image {
            path: probe.path.clone(),
            info: exif_info(&probe.path),
        })
    }
}

/// EXIF fields worth showing, in this order.
const TAGS: &[(exif::Tag, &str)] = &[
    (exif::Tag::Make, "Make"),
    (exif::Tag::Model, "Camera"),
    (exif::Tag::LensModel, "Lens"),
    (exif::Tag::DateTimeOriginal, "Taken"),
    (exif::Tag::ExposureTime, "Exposure"),
    (exif::Tag::FNumber, "Aperture"),
    (exif::Tag::PhotographicSensitivity, "ISO"),
    (exif::Tag::FocalLength, "Focal length"),
    (exif::Tag::PixelXDimension, "Width"),
    (exif::Tag::PixelYDimension, "Height"),
    (exif::Tag::GPSLatitude, "Latitude"),
    (exif::Tag::GPSLongitude, "Longitude"),
    (exif::Tag::Software, "Software"),
];

/// Label and value of the image's EXIF fields; empty without EXIF data.
pub fn exif_info(path: &Path) -> Vec<(String, String)> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut BufReader::new(file)) else {
        return Vec::new();
    };
    TAGS.iter()
        .filter_map(|&(tag, label)| {
            let field = exif.get_field(tag, exif::In::PRIMARY)?;
            let value = field.display_value().with_unit(&exif).to_string();
            let value = value.trim_matches('"').trim().to_string();
            (!value.is_empty()).then(|| (label.to_string(), value))
        })
        .collect()
}

/// Values for the EXIF columns, by [`crate::columns::ExifField::key`]:
/// `taken`, `camera`, `lens`, `exposure`, `aperture`, `iso`,
/// `focal_length`, `dimensions`, `gps`. Empty for files without EXIF data.
pub fn exif_fields(path: &Path) -> HashMap<&'static str, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // Only formats that carry EXIF; saves opening every file.
    if !matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "tif" | "tiff" | "png" | "webp" | "heic" | "heif" | "avif"
    ) {
        return HashMap::new();
    }
    let info: HashMap<String, String> = exif_info(path).into_iter().collect();
    let get = |label: &str| info.get(label).cloned();
    let mut fields = HashMap::new();
    let mut put = |key, value: Option<String>| {
        if let Some(value) = value {
            fields.insert(key, value);
        }
    };
    // EXIF dates read "2024-05-01 12:30:45"; minutes are enough here.
    put(
        "taken",
        get("Taken").map(|t| t.get(..16).unwrap_or(&t).to_string()),
    );
    // Models usually repeat the make ("Canon EOS R6"); some don't ("Pixel 7").
    put(
        "camera",
        match (get("Make"), get("Camera")) {
            // "Camera" is the Model tag
            (Some(make), Some(model))
                if !model.to_lowercase().starts_with(&make.to_lowercase()) =>
            {
                Some(format!("{make} {model}"))
            }
            (make, model) => model.or(make),
        },
    );
    put("lens", get("Lens"));
    put("exposure", get("Exposure"));
    put("aperture", get("Aperture"));
    put("iso", get("ISO"));
    put("focal_length", get("Focal length"));
    put(
        "dimensions",
        get("Width")
            .zip(get("Height"))
            .map(|(w, h)| format!("{w}×{h}")),
    );
    put(
        "gps",
        get("Latitude")
            .zip(get("Longitude"))
            .map(|(lat, lon)| format!("{lat}, {lon}")),
    );
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal JPEG: SOI, an APP1 EXIF segment (little-endian TIFF with
    /// Make and Model), EOI.
    fn jpeg_with_exif() -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        let entries: u16 = 2;
        tiff.extend(entries.to_le_bytes());
        let data_start = 8 + 2 + 12 * entries as u32 + 4;
        let (make, model) = (b"Acme\0".as_slice(), b"Snap 1\0".as_slice());
        let mut entry = |tag: u16, value: &[u8], offset: u32| {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(2u16.to_le_bytes()); // ASCII
            tiff.extend((value.len() as u32).to_le_bytes());
            tiff.extend(offset.to_le_bytes());
        };
        entry(0x010f, make, data_start);
        entry(0x0110, model, data_start + make.len() as u32);
        tiff.extend(0u32.to_le_bytes()); // no next IFD
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
    fn reads_exif_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("photo.jpg");
        std::fs::write(&path, jpeg_with_exif()).unwrap();
        assert_eq!(
            exif_info(&path),
            [
                ("Make".to_string(), "Acme".to_string()),
                ("Camera".to_string(), "Snap 1".to_string())
            ]
        );
        let fields = exif_fields(&path);
        assert_eq!(
            fields.get("camera").map(String::as_str),
            Some("Acme Snap 1")
        );
        assert_eq!(fields.len(), 1);
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        assert!(exif_info(&path).is_empty());
        assert!(exif_fields(&path).is_empty());
    }
}

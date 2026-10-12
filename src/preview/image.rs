//! Image previews and EXIF values for image and camera RAW files.

use std::io;
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

/// Label and value of the image's EXIF fields; empty without EXIF data.
pub fn exif_info(path: &Path) -> Vec<(String, String)> {
    crate::formats::exif::read(path)
        .unwrap_or_default()
        .into_iter()
        .map(|field| (label(field.tag).to_owned(), field.value))
        .collect()
}

fn label(tag: crate::formats::exif::Tag) -> &'static str {
    use crate::formats::exif::Tag;
    match tag {
        Tag::Make => "Make",
        Tag::Model => "Camera",
        Tag::LensModel => "Lens",
        Tag::DateTimeOriginal => "Taken",
        Tag::ExposureTime => "Exposure",
        Tag::FNumber => "Aperture",
        Tag::PhotographicSensitivity => "ISO",
        Tag::FocalLength => "Focal length",
        Tag::PixelXDimension => "Width",
        Tag::PixelYDimension => "Height",
        Tag::GPSLatitude => "Latitude",
        Tag::GPSLongitude => "Longitude",
        Tag::Software => "Software",
    }
}

#[cfg(test)]
mod tests {
    use super::exif_info;
    use image::ImageEncoder;

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
            .expect("encode JPEG preview");
        let exif = jpeg_with_exif();
        jpeg.splice(2..2, exif[2..exif.len() - 2].iter().copied());
        jpeg
    }

    #[test]
    fn preview_exif_adapter_keeps_existing_labels_and_field_order() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("photo.jpg");
        std::fs::write(&path, jpeg_with_exif()).expect("write JPEG fixture");
        assert_eq!(
            exif_info(&path),
            [
                ("Make".to_string(), "Acme".to_string()),
                ("Camera".to_string(), "Snap 1".to_string()),
            ]
        );
    }

    #[test]
    fn raw_registry_preview_keeps_labeled_embedded_exif() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("photo.CR3");
        let mut raw = b"RAW wrapper".to_vec();
        raw.extend(decodable_jpeg_with_exif());
        std::fs::write(&path, raw).expect("write RAW fixture");

        let preview = crate::preview::Registry::with_defaults()
            .preview(&path)
            .expect("RAW preview");
        let crate::preview::Content::Image { info, .. } = preview.content else {
            panic!("expected image preview");
        };
        assert!(
            info.iter()
                .any(|(label, value)| label == "Camera" && value == "Snap 1")
        );
    }
}

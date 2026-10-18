//! Provider-owned EXIF facts and built-in column projections.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

use crate::data_providers::{
    Applicability, ColumnContribution, ContributionHeader, ContributionId, DemandLimits,
    FieldDefinition, FieldId, FieldKind, PropertiesContribution, ProviderDefinition, ProviderId,
    RegistrationOrigin, Value, validate_definitions,
};
use crate::formats::exif::{self, Field, Tag};

const IMAGE_FORMAT: &str = "exif.image";
const PROPERTY_TAGS: [Tag; 13] = [
    Tag::Make,
    Tag::Model,
    Tag::LensModel,
    Tag::DateTimeOriginal,
    Tag::ExposureTime,
    Tag::FNumber,
    Tag::PhotographicSensitivity,
    Tag::FocalLength,
    Tag::PixelXDimension,
    Tag::PixelYDimension,
    Tag::GPSLatitude,
    Tag::GPSLongitude,
    Tag::Software,
];

const COLUMN_KEYS: [&str; 9] = [
    "taken",
    "camera",
    "lens",
    "exposure",
    "aperture",
    "iso",
    "focal_length",
    "dimensions",
    "gps",
];

/// Native EXIF metadata retained alongside independently validated projections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedExif {
    /// Semantic values from the shared bounded parser.
    pub fields: Vec<Field>,
    /// Byte prefix used by the Properties reader.
    pub limit: u64,
    /// Whether source bytes or one displayed value exceeded its cap.
    pub truncated: bool,
}

/// The one EXIF definition shared by Properties and the existing columns.
pub struct ExifProvider {
    definition: ProviderDefinition,
}

/// Handle for metadata-only EXIF Properties.
#[derive(Clone, Copy)]
pub struct ExifProperties {
    provider: &'static ExifProvider,
}

/// Handle for the nine existing composed EXIF columns.
#[derive(Clone, Copy)]
pub struct ExifColumns {
    provider: &'static ExifProvider,
}

/// Returns the immutable native EXIF definition.
pub fn provider() -> &'static ExifProvider {
    static PROVIDER: OnceLock<ExifProvider> = OnceLock::new();
    PROVIDER.get_or_init(ExifProvider::new)
}

/// Returns the metadata-only Properties handle.
pub fn properties() -> ExifProperties {
    ExifProperties {
        provider: provider(),
    }
}

/// Returns the built-in column projection handle.
pub fn columns() -> ExifColumns {
    ExifColumns {
        provider: provider(),
    }
}

impl ExifProvider {
    fn new() -> Self {
        let semantic_fields = PROPERTY_TAGS
            .into_iter()
            .map(|tag| FieldDefinition {
                id: semantic_id(tag),
                kind: FieldKind::Text,
            })
            .collect::<Vec<_>>();
        let column_fields = COLUMN_KEYS
            .iter()
            .map(|key| FieldDefinition {
                id: column_field_id(key),
                kind: FieldKind::Text,
            })
            .collect::<Vec<_>>();
        let property = PropertiesContribution {
            header: ContributionHeader {
                id: ContributionId::new("builtin.exif.properties")
                    .expect("static Properties ID is valid"),
                label: "EXIF metadata".into(),
                applicability: Applicability::Any,
                demand: DemandLimits {
                    max_bytes: crate::formats::raw::SCAN_LIMIT,
                    max_items: (PROPERTY_TAGS.len() + 16) as u64,
                },
            },
            fields: PROPERTY_TAGS.into_iter().map(semantic_id).collect(),
        };
        let column_applicability = Applicability::Format(IMAGE_FORMAT.into());
        let columns = COLUMN_KEYS
            .iter()
            .map(|key| ColumnContribution {
                header: ContributionHeader {
                    id: ContributionId::new(format!("builtin.exif.{key}-column"))
                        .expect("static column contribution ID is valid"),
                    label: format!("EXIF {key}"),
                    applicability: column_applicability.clone(),
                    // Legacy regular-image reading is seekable and has no fixed input cap;
                    // RAW prefix scanning is bounded by formats::raw::SCAN_LIMIT.
                    demand: DemandLimits {
                        max_bytes: u64::MAX,
                        max_items: PROPERTY_TAGS.len() as u64,
                    },
                },
                column_id: format!("exif.{key}"),
                field: column_field_id(key),
            })
            .collect();
        let definition = ProviderDefinition {
            id: ProviderId::new("builtin.exif").expect("static provider ID is valid"),
            fields: semantic_fields.into_iter().chain(column_fields).collect(),
            properties: vec![property],
            columns,
            viewers: Vec::new(),
        };
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("built-in EXIF contributions are valid");
        Self { definition }
    }

    /// Returns the provider definition for semantic EXIF facts and built-in columns.
    pub fn definition(&self) -> &ProviderDefinition {
        &self.definition
    }

    fn prepare_properties(&self, result: exif::PropertiesRead) -> io::Result<PreparedExif> {
        let contribution = &self.definition.properties[0];
        let mut fields = Vec::new();
        for field in result.fields {
            let id = semantic_id(field.tag);
            if !contribution.fields.contains(&id) {
                continue;
            }
            let definition = self
                .definition
                .fields
                .iter()
                .find(|definition| definition.id == id)
                .expect("EXIF Properties fields have matching definitions");
            definition
                .validate(&Value::Text(field.value.clone()))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            fields.push(field);
        }
        Ok(PreparedExif {
            fields,
            limit: result.limit,
            truncated: result.truncated,
        })
    }

    fn prepare_columns(&self, fields: &[Field]) -> io::Result<HashMap<&'static str, String>> {
        let mut values = HashMap::new();
        for column in &self.definition.columns {
            let Some(key) = static_column_key(&column.column_id) else {
                continue;
            };
            let definition = self
                .definition
                .fields
                .iter()
                .find(|definition| definition.id == column.field)
                .expect("EXIF column field is defined");
            if let Some(value) = column_value(key, fields) {
                definition
                    .validate(&Value::Text(value.clone()))
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                values.insert(key, value);
            }
        }
        Ok(values)
    }
}

impl ExifProperties {
    /// Returns the metadata-only contribution descriptor.
    pub fn descriptor(&self) -> &PropertiesContribution {
        &self.provider.definition.properties[0]
    }

    /// Reads local EXIF with the Properties-specific limits and cancellation.
    pub fn read(&self, path: &Path, cancel: &AtomicBool) -> io::Result<PreparedExif> {
        self.provider
            .prepare_properties(exif::read_properties(path, cancel)?)
    }
}

impl ExifColumns {
    /// Returns the descriptors for the preserved persisted column IDs.
    pub fn descriptors(&self) -> &[ColumnContribution] {
        &self.provider.definition.columns
    }

    /// Preserves the existing regular-image and RAW extension gate.
    pub fn matches_path(&self, path: &Path) -> bool {
        crate::formats::raw::supported(path)
            || path.extension().is_some_and(|extension| {
                matches!(
                    extension.to_string_lossy().to_ascii_lowercase().as_str(),
                    "jpg" | "jpeg" | "tif" | "tiff" | "png" | "webp" | "heic" | "heif" | "avif"
                )
            })
    }

    /// Reads the existing native column source and prepares its composed string columns.
    pub fn read(&self, path: &Path) -> io::Result<HashMap<&'static str, String>> {
        if !self.matches_path(path) {
            return Ok(HashMap::new());
        }
        let fields = exif::read(path)?;
        self.provider.prepare_columns(&fields)
    }
}

fn semantic_id(tag: Tag) -> FieldId {
    FieldId::new(format!("builtin.exif.{}", tag.id())).expect("EXIF tag ID is valid")
}

fn column_field_id(key: &str) -> FieldId {
    FieldId::new(format!("builtin.exif.column.{key}")).expect("EXIF column field ID is valid")
}

fn static_column_key(column_id: &str) -> Option<&'static str> {
    match column_id {
        "exif.taken" => Some("taken"),
        "exif.camera" => Some("camera"),
        "exif.lens" => Some("lens"),
        "exif.exposure" => Some("exposure"),
        "exif.aperture" => Some("aperture"),
        "exif.iso" => Some("iso"),
        "exif.focal_length" => Some("focal_length"),
        "exif.dimensions" => Some("dimensions"),
        "exif.gps" => Some("gps"),
        _ => None,
    }
}

fn value_for_tag(fields: &[Field], tag: Tag) -> Option<String> {
    fields
        .iter()
        .find(|field| field.tag == tag)
        .map(|field| field.value.clone())
}

fn column_value(key: &str, fields: &[Field]) -> Option<String> {
    let get = |tag| value_for_tag(fields, tag);
    match key {
        "taken" => {
            get(Tag::DateTimeOriginal).map(|taken| taken.get(..16).unwrap_or(&taken).to_string())
        }
        "camera" => {
            let make = get(Tag::Make);
            let model = get(Tag::Model);
            match (make, model) {
                (Some(make), Some(model))
                    if !model.to_lowercase().starts_with(&make.to_lowercase()) =>
                {
                    Some(format!("{make} {model}"))
                }
                (make, model) => model.or(make),
            }
        }
        "lens" => get(Tag::LensModel),
        "exposure" => get(Tag::ExposureTime),
        "aperture" => get(Tag::FNumber),
        "iso" => get(Tag::PhotographicSensitivity),
        "focal_length" => get(Tag::FocalLength),
        "dimensions" => get(Tag::PixelXDimension)
            .zip(get(Tag::PixelYDimension))
            .map(|(width, height)| format!("{width}×{height}")),
        "gps" => get(Tag::GPSLatitude)
            .zip(get(Tag::GPSLongitude))
            .map(|(latitude, longitude)| format!("{latitude}, {longitude}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ExifProvider, PROPERTY_TAGS, column_value, columns, properties, provider};
    use crate::data_providers::{RegistrationOrigin, validate_definitions};
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

    fn decodable_jpeg_with_exif() -> Vec<u8> {
        use image::ImageEncoder;

        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .write_image(&[128, 128, 128], 1, 1, image::ExtendedColorType::Rgb8)
            .expect("encode JPEG fixture");
        let exif = jpeg_with_exif();
        jpeg.splice(2..2, exif[2..exif.len() - 2].iter().copied());
        jpeg
    }

    #[test]
    fn one_definition_binds_all_properties_tags_and_legacy_column_ids() {
        let definition = provider().definition();
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("native EXIF definition validates");
        assert_eq!(definition.properties.len(), 1);
        assert_eq!(definition.properties[0].fields.len(), PROPERTY_TAGS.len());
        assert_eq!(definition.columns.len(), 9);
        assert_eq!(
            definition
                .columns
                .iter()
                .map(|column| column.column_id.as_str())
                .collect::<Vec<_>>(),
            [
                "exif.taken",
                "exif.camera",
                "exif.lens",
                "exif.exposure",
                "exif.aperture",
                "exif.iso",
                "exif.focal_length",
                "exif.dimensions",
                "exif.gps"
            ]
        );
        assert_eq!(columns().descriptors(), definition.columns);
    }

    #[test]
    fn composed_column_values_keep_existing_formatting() {
        let fields = [
            (Tag::DateTimeOriginal, "2024-05-01 12:30:45"),
            (Tag::Make, "Canon"),
            (Tag::Model, "EOS R6"),
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
        assert_eq!(
            column_value("taken", &fields).as_deref(),
            Some("2024-05-01 12:30")
        );
        assert_eq!(
            column_value("camera", &fields).as_deref(),
            Some("Canon EOS R6")
        );
        assert_eq!(
            column_value("dimensions", &fields).as_deref(),
            Some("6000×4000")
        );
        assert_eq!(
            column_value("gps", &fields).as_deref(),
            Some("51 deg 30' 0\" N, 0 deg 7' 0\" W")
        );
        assert_eq!(
            column_value(
                "camera",
                &[Field {
                    tag: Tag::Make,
                    value: "Pixel".into()
                }]
            )
            .as_deref(),
            Some("Pixel")
        );
        let local = ExifProvider {
            definition: provider().definition().clone(),
        };
        let projected = local.prepare_columns(&fields).expect("valid text columns");
        assert_eq!(projected.len(), 9);
    }

    #[test]
    fn production_column_handle_keeps_extension_gate_and_tiff_fallback() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let jpeg = jpeg_with_exif();
        let photo = temp.path().join("photo.jpg");
        std::fs::write(&photo, &jpeg).expect("write JPEG fixture");
        let columns = columns();
        let values = columns.read(&photo).expect("read EXIF columns");
        assert_eq!(
            values.get("camera").map(String::as_str),
            Some("Acme Snap 1")
        );

        let text = temp.path().join("photo.txt");
        std::fs::write(&text, &jpeg).expect("write gated fixture");
        assert!(
            columns
                .read(&text)
                .expect("declined file is blank")
                .is_empty()
        );

        let raw = temp.path().join("photo.dng");
        std::fs::write(&raw, &jpeg[12..jpeg.len() - 2]).expect("write direct TIFF metadata");
        assert_eq!(
            columns
                .read(&raw)
                .expect("read direct TIFF")
                .get("camera")
                .map(String::as_str),
            Some("Acme Snap 1")
        );

        let embedded = temp.path().join("embedded.cr3");
        let mut raw = b"RAW wrapper".to_vec();
        raw.extend(decodable_jpeg_with_exif());
        std::fs::write(&embedded, raw).expect("write embedded JPEG RAW fixture");
        assert_eq!(
            columns
                .read(&embedded)
                .expect("read existing RAW JPEG fallback")
                .get("camera")
                .map(String::as_str),
            Some("Acme Snap 1")
        );
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 4);
    }

    #[test]
    fn properties_handle_uses_shared_tags_and_its_eight_mib_limit() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let photo = temp.path().join("photo.jpg");
        std::fs::write(&photo, jpeg_with_exif()).expect("write JPEG fixture");
        let prepared = properties()
            .read(&photo, &Default::default())
            .expect("bounded Properties read");
        assert_eq!(prepared.limit, 8 * 1024 * 1024);
        assert!(!prepared.truncated);
        assert_eq!(prepared.fields.len(), 2);
        assert_eq!(prepared.fields[0].tag, Tag::Make);
        assert_eq!(prepared.fields[1].tag.id(), "model");
    }
}

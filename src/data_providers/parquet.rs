//! Provider-owned bounded Parquet metadata acquisition for Properties and Preview.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

use ::parquet::file::metadata::ParquetMetaData;

use crate::data_providers::{
    Applicability, ContributionHeader, ContributionId, DemandLimits, PropertiesContribution,
    ProviderDefinition, ProviderId, RegistrationOrigin, ViewerContribution, validate_definitions,
};
use crate::formats::parquet;

const FORMAT_ID: &str = "parquet";

/// The Parquet definition and its independent native metadata handles.
pub struct ParquetProvider {
    definition: ProviderDefinition,
}

/// Handle for the bounded footer read used by the Properties mapper.
#[derive(Clone, Copy)]
pub struct ParquetProperties {
    provider: &'static ParquetProvider,
}

/// Handle for the checked footer read before Preview samples row groups.
#[derive(Clone, Copy)]
pub struct ParquetViewer {
    provider: &'static ParquetProvider,
}

/// Returns the one immutable Parquet provider definition.
pub fn provider() -> &'static ParquetProvider {
    static PROVIDER: OnceLock<ParquetProvider> = OnceLock::new();
    PROVIDER.get_or_init(ParquetProvider::new)
}

/// Returns the provider-owned Properties metadata handle.
pub fn properties() -> ParquetProperties {
    ParquetProperties {
        provider: provider(),
    }
}

/// Returns the provider-owned Preview metadata handle.
pub fn viewer() -> ParquetViewer {
    ParquetViewer {
        provider: provider(),
    }
}

impl ParquetProvider {
    fn new() -> Self {
        let applicability = Applicability::Format(FORMAT_ID.into());
        let definition = ProviderDefinition {
            id: ProviderId::new("builtin.parquet").expect("static provider ID is valid"),
            fields: Vec::new(),
            properties: vec![PropertiesContribution {
                header: ContributionHeader {
                    id: ContributionId::new("builtin.parquet.properties")
                        .expect("static Properties ID is valid"),
                    label: "Parquet metadata".into(),
                    applicability: applicability.clone(),
                    demand: properties_demand(),
                },
                fields: Vec::new(),
            }],
            columns: Vec::new(),
            viewers: vec![ViewerContribution {
                header: ContributionHeader {
                    id: ContributionId::new("builtin.parquet.viewer")
                        .expect("static viewer ID is valid"),
                    label: "Parquet rows".into(),
                    applicability,
                    demand: viewer_demand(),
                },
                representation: "parquet.table".into(),
                version: 1,
            }],
        };
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("built-in Parquet contributions are valid");
        Self { definition }
    }

    /// Returns the native metadata definition bound to Properties and Preview.
    pub fn definition(&self) -> &ProviderDefinition {
        &self.definition
    }
}

impl ParquetProperties {
    /// Returns the descriptor declared by this metadata contribution.
    pub fn descriptor(&self) -> &PropertiesContribution {
        &self.provider.definition.properties[0]
    }

    /// Whether an existing local Properties path matches the extension-or-magic policy.
    pub fn matches_path(&self, path: &Path) -> bool {
        if extension_matches_path(&self.descriptor().header.applicability, path) {
            return true;
        }
        let Ok(mut file) = open_regular_file(path) else {
            return false;
        };
        let mut magic = [0; 4];
        file.read_exact(&mut magic).is_ok() && &magic == b"PAR1"
    }

    /// Reads the native footer with the existing cancellation and byte limits.
    pub fn read_metadata(&self, path: &Path, cancel: &AtomicBool) -> io::Result<ParquetMetaData> {
        parquet::check_cancel(Some(cancel))?;
        let mut file = open_regular_file(path)?;
        parquet::read_metadata(&mut file, Some(cancel))
    }
}

impl ParquetViewer {
    /// Returns the descriptor for the row-sample preview representation.
    pub fn descriptor(&self) -> &ViewerContribution {
        &self.provider.definition.viewers[0]
    }

    /// Whether the existing Preview extension-or-captured-magic policy matches.
    pub fn matches_probe(&self, extension: &str, head: &[u8]) -> bool {
        extension_matches(&self.descriptor().header.applicability, extension)
            || head.starts_with(b"PAR1")
    }

    /// Reads the checked native footer before the Preview adapter samples rows.
    pub fn read_metadata(&self, file: &mut File) -> io::Result<ParquetMetaData> {
        parquet::read_metadata(file, None)
    }
}

fn properties_demand() -> DemandLimits {
    DemandLimits {
        max_bytes: parquet::MAX_METADATA_BYTES,
        max_items: 256 + 128,
    }
}

fn viewer_demand() -> DemandLimits {
    DemandLimits {
        max_bytes: parquet::MAX_METADATA_BYTES + 64 * 1024 * 1024,
        max_items: 500 * 40,
    }
}

fn extension_matches_path(applicability: &Applicability, path: &Path) -> bool {
    let is_parquet = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(FORMAT_ID));
    is_parquet && matches!(applicability, Applicability::Format(format) if format == FORMAT_ID)
}

fn extension_matches(applicability: &Applicability, extension: &str) -> bool {
    extension.eq_ignore_ascii_case(FORMAT_ID)
        && matches!(applicability, Applicability::Format(format) if format == FORMAT_ID)
}

fn open_regular_file(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Parquet Properties requires a local regular file",
            ));
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let file = OpenOptions::new().read(true).open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Parquet Properties requires a local regular file",
            ));
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::Path;
    use std::sync::Arc;

    use ::parquet::file::writer::SerializedFileWriter;
    use ::parquet::schema::parser::parse_message_type;
    use tempfile::TempDir;

    use super::{properties, provider, viewer};
    use crate::data_providers::RegistrationOrigin;
    use crate::data_providers::validate_definitions;

    fn empty_file(path: &Path) {
        let schema = Arc::new(
            parse_message_type("message sample { REQUIRED INT32 value; }")
                .expect("valid test schema"),
        );
        SerializedFileWriter::new(
            File::create(path).expect("create fixture"),
            schema,
            Default::default(),
        )
        .expect("create writer")
        .close()
        .expect("close writer");
    }

    fn repeated_file(path: &Path) {
        let schema = Arc::new(
            parse_message_type(
                "message sample { REQUIRED GROUP record { OPTIONAL INT32 value; } REPEATED INT64 values; }",
            )
            .expect("valid repeated test schema"),
        );
        SerializedFileWriter::new(
            File::create(path).expect("create fixture"),
            schema,
            Default::default(),
        )
        .expect("create writer")
        .close()
        .expect("close writer");
    }

    #[test]
    fn definition_binds_distinct_metadata_and_row_sample_demands() {
        let definition = provider().definition();
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("native definition validates");
        let properties = properties();
        let viewer = viewer();
        assert!(definition.fields.is_empty());
        assert_eq!(definition.properties.len(), 1);
        assert_eq!(definition.viewers.len(), 1);
        assert_eq!(
            properties.descriptor().header.applicability,
            viewer.descriptor().header.applicability
        );
        assert_eq!(
            properties.descriptor().header.demand.max_bytes,
            8 * 1024 * 1024
        );
        assert_eq!(properties.descriptor().header.demand.max_items, 384);
        assert_eq!(
            viewer.descriptor().header.demand.max_bytes,
            72 * 1024 * 1024
        );
        assert_eq!(viewer.descriptor().header.demand.max_items, 20_000);
    }

    #[test]
    fn metadata_handles_keep_extension_and_magic_detection_and_decode_native_footer() {
        let temp = TempDir::new().expect("temp dir");
        let by_extension = temp.path().join("data.parquet");
        let by_magic = temp.path().join("data.bin");
        empty_file(&by_extension);
        std::fs::copy(&by_extension, &by_magic).expect("copy parquet fixture");
        assert!(properties().matches_path(&by_extension));
        assert!(properties().matches_path(&by_magic));
        assert!(viewer().matches_probe("parquet", b""));
        assert!(viewer().matches_probe("bin", b"PAR1metadata"));
        assert!(!viewer().matches_probe("txt", b"plain"));
        assert_eq!(
            properties()
                .read_metadata(&by_magic, &Default::default())
                .expect("Properties metadata")
                .file_metadata()
                .num_rows(),
            0
        );
        assert_eq!(
            viewer()
                .read_metadata(&mut File::open(&by_extension).expect("open fixture"))
                .expect("Preview metadata")
                .file_metadata()
                .schema_descr()
                .columns()
                .len(),
            1
        );
    }

    #[test]
    fn properties_schema_remains_available_when_row_sample_declines_repeated_columns() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("nested.parquet");
        repeated_file(&path);

        let properties = crate::properties::Registry::with_defaults()
            .read(&path, &Default::default())
            .expect("Properties maps the shared native footer");
        let parquet_group = properties
            .iter()
            .find(|group| group.id.0 == "parquet")
            .expect("Properties keeps the Parquet group");
        let schema = parquet_group
            .tables
            .iter()
            .find(|table| table.id == "schema")
            .expect("Properties keeps the readable schema table");
        assert_eq!(schema.rows.len(), 2);
        assert_eq!(
            schema.rows[0][0],
            crate::properties::PropertyValue::Text("record.value".into())
        );

        let preview = crate::preview::Registry::with_defaults().preview(&path);
        assert!(preview.is_err(), "row sampling declines repeated columns");
    }
}

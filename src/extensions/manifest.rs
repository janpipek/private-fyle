//! Strict version-1 package manifests and conversion to provider contracts.

use std::io;

use serde::Deserialize;

use crate::data_providers::{
    Applicability, ColumnContribution, ContributionHeader, ContributionId, DemandLimits,
    FieldDefinition, FieldId, FieldKind, PropertiesContribution, ProviderDefinition, ProviderId,
    RegistrationOrigin, Unit, ViewerContribution, validate_definitions,
};

/// Current manifest API major version supported by fyle.
pub const API_VERSION: u32 = 1;
/// Maximum manifest size in bytes.
pub const MANIFEST_BYTES: usize = 256 * 1024;
/// Maximum entry script size in bytes.
pub const SCRIPT_BYTES: usize = 256 * 1024;
/// Maximum providers declared in one package.
pub const MAX_PROVIDERS: usize = 16;
/// Maximum fields declared by one provider.
pub const MAX_FIELDS_PER_PROVIDER: usize = 128;
/// Maximum combined contributions declared by one provider.
pub const MAX_CONTRIBUTIONS_PER_PROVIDER: usize = 64;
/// Maximum combined contributions declared by one package.
pub const MAX_CONTRIBUTIONS_PER_PACKAGE: usize = 256;
/// Maximum byte or item demand a contribution may declare.
pub const MAX_DEMAND_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum item demand a contribution may declare.
pub const MAX_DEMAND_ITEMS: u64 = 100_000;
/// Maximum label length in bytes.
pub const MAX_LABEL_BYTES: usize = 256;
/// Maximum representation identifier length in bytes.
pub const MAX_REPRESENTATION_BYTES: usize = 128;

/// A package directory name, restricted to one path-safe ASCII identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId(String);

impl PackageId {
    /// Validates and constructs one package path component.
    pub fn new(value: impl Into<String>) -> io::Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 256
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
            })
        {
            return Err(invalid("package ID must be one lowercase ASCII identifier"));
        }
        Ok(Self(value))
    }

    /// Returns the validated directory and manifest spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// A fully validated, version-1 provider manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// Stable package identity.
    pub id: PackageId,
    /// Relative script path declared by the manifest.
    pub entry: String,
    /// Provider bundles declared by this package.
    pub providers: Vec<ProviderDefinition>,
}

/// Parses bounded manifest text and validates all declarations before script access.
pub fn parse(id: &PackageId, manifest: &str) -> io::Result<Manifest> {
    if manifest.len() > MANIFEST_BYTES {
        return Err(invalid("manifest exceeds the size limit"));
    }
    let dto: ManifestDto = toml::from_str(manifest)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if dto.api_version != API_VERSION {
        return Err(invalid("unsupported manifest API version"));
    }
    let declared_id = PackageId::new(dto.id)?;
    if &declared_id != id {
        return Err(invalid("manifest package ID does not match its directory"));
    }
    validate_entry(&dto.entry)?;
    if dto.providers.is_empty() || dto.providers.len() > MAX_PROVIDERS {
        return Err(invalid(
            "manifest provider count is outside the supported bounds",
        ));
    }

    let mut providers = Vec::with_capacity(dto.providers.len());
    let mut contribution_count = 0usize;
    for provider in dto.providers {
        let provider_id = ProviderId::new(valid_id(provider.id)?)?;
        let prefix = format!("extension.{}.", id.as_str());
        if !provider_id.as_str().starts_with(&prefix) {
            return Err(invalid("provider ID must belong to its extension package"));
        }
        if provider.fields.len() > MAX_FIELDS_PER_PROVIDER {
            return Err(invalid("provider declares too many fields"));
        }
        let count = provider.properties.len() + provider.columns.len() + provider.viewers.len();
        if count > MAX_CONTRIBUTIONS_PER_PROVIDER {
            return Err(invalid("provider declares too many contributions"));
        }
        contribution_count = contribution_count.saturating_add(count);
        if contribution_count > MAX_CONTRIBUTIONS_PER_PACKAGE {
            return Err(invalid("package declares too many contributions"));
        }
        let fields = provider
            .fields
            .into_iter()
            .map(|field| {
                let id = FieldId::new(valid_id(field.id)?)?;
                let kind = match (field.kind, field.unit) {
                    (FieldType::Text, None) => FieldKind::Text,
                    (FieldType::Text, Some(_)) => {
                        return Err(invalid("text fields cannot declare a unit"));
                    }
                    (FieldType::Quantity, Some(unit)) => FieldKind::Quantity(unit.into()),
                    (FieldType::Quantity, None) => {
                        return Err(invalid("quantity fields require a unit"));
                    }
                };
                Ok(FieldDefinition { id, kind })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let properties = provider
            .properties
            .into_iter()
            .map(|item| {
                Ok(PropertiesContribution {
                    header: header(
                        item.id,
                        item.label,
                        item.max_bytes,
                        item.max_items,
                        item.applicability,
                    )?,
                    fields: item
                        .fields
                        .into_iter()
                        .map(|field| valid_id(field).and_then(FieldId::new))
                        .collect::<io::Result<_>>()?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let columns = provider
            .columns
            .into_iter()
            .map(|item| {
                Ok(ColumnContribution {
                    header: header(
                        item.id,
                        item.label,
                        item.max_bytes,
                        item.max_items,
                        item.applicability,
                    )?,
                    column_id: valid_id(item.column_id)?,
                    field: FieldId::new(valid_id(item.field)?)?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let viewers = provider
            .viewers
            .into_iter()
            .map(|item| {
                validate_label(
                    &item.representation,
                    MAX_REPRESENTATION_BYTES,
                    "representation",
                )?;
                if !item
                    .representation
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                {
                    return Err(invalid(
                        "representation must use ASCII identifier characters",
                    ));
                }
                if item.version != 1 {
                    return Err(invalid("unsupported viewer representation version"));
                }
                Ok(ViewerContribution {
                    header: header(
                        item.id,
                        item.label,
                        item.max_bytes,
                        item.max_items,
                        item.applicability,
                    )?,
                    representation: item.representation,
                    version: item.version,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        providers.push(ProviderDefinition {
            id: provider_id,
            fields,
            properties,
            columns,
            viewers,
        });
    }
    let batch: Vec<_> = providers
        .iter()
        .cloned()
        .map(|provider| (RegistrationOrigin::Extension, provider))
        .collect();
    validate_definitions(&batch)?;
    if providers
        .iter()
        .flat_map(|provider| &provider.columns)
        .any(|column| {
            crate::columns::ColumnId::new(column.column_id.clone())
                .is_ok_and(|id| id.builtin().is_some())
        })
    {
        return Err(invalid(
            "extension column ID collides with a built-in column",
        ));
    }
    Ok(Manifest {
        id: id.clone(),
        entry: dto.entry,
        providers,
    })
}

fn validate_entry(entry: &str) -> io::Result<()> {
    let path = std::path::Path::new(entry);
    if entry.is_empty()
        || entry.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid("entry must be a package-local relative path"));
    }
    Ok(())
}

fn header(
    id: String,
    label: String,
    max_bytes: u64,
    max_items: u64,
    applicability: Option<ApplicabilityDto>,
) -> io::Result<ContributionHeader> {
    validate_label(&label, MAX_LABEL_BYTES, "label")?;
    if max_bytes == 0
        || max_bytes > MAX_DEMAND_BYTES
        || max_items == 0
        || max_items > MAX_DEMAND_ITEMS
    {
        return Err(invalid(
            "contribution demand is outside the supported bounds",
        ));
    }
    Ok(ContributionHeader {
        id: ContributionId::new(valid_id(id)?)?,
        label,
        applicability: applicability.map_or(Ok(Applicability::Any), ApplicabilityDto::convert)?,
        demand: DemandLimits {
            max_bytes,
            max_items,
        },
    })
}

fn validate_label(value: &str, max: usize, name: &str) -> io::Result<()> {
    if value.trim().is_empty() || value.len() > max {
        return Err(invalid_owned(format!("{name} is empty or too long")));
    }
    Ok(())
}

fn valid_id(value: String) -> io::Result<String> {
    if value.len() > 256 {
        return Err(invalid(
            "provider, field, contribution, and column IDs are limited to 256 bytes",
        ));
    }
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDto {
    api_version: u32,
    id: String,
    entry: String,
    providers: Vec<ProviderDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderDto {
    id: String,
    #[serde(default)]
    fields: Vec<FieldDto>,
    #[serde(default)]
    properties: Vec<PropertiesDto>,
    #[serde(default)]
    columns: Vec<ColumnDto>,
    #[serde(default)]
    viewers: Vec<ViewerDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDto {
    id: String,
    kind: FieldType,
    #[serde(default)]
    unit: Option<UnitDto>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FieldType {
    Text,
    Quantity,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum UnitDto {
    Seconds,
    Metres,
}

impl From<UnitDto> for Unit {
    fn from(unit: UnitDto) -> Self {
        match unit {
            UnitDto::Seconds => Self::Seconds,
            UnitDto::Metres => Self::Metres,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertiesDto {
    id: String,
    label: String,
    max_bytes: u64,
    max_items: u64,
    #[serde(default)]
    applicability: Option<ApplicabilityDto>,
    fields: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnDto {
    id: String,
    label: String,
    column_id: String,
    field: String,
    max_bytes: u64,
    max_items: u64,
    #[serde(default)]
    applicability: Option<ApplicabilityDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewerDto {
    id: String,
    label: String,
    representation: String,
    version: u32,
    max_bytes: u64,
    max_items: u64,
    #[serde(default)]
    applicability: Option<ApplicabilityDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicabilityDto {
    kind: String,
    #[serde(default)]
    value: Option<String>,
}

impl ApplicabilityDto {
    fn convert(self) -> io::Result<Applicability> {
        match (self.kind.as_str(), self.value) {
            ("any", None) => Ok(Applicability::Any),
            ("format", Some(value)) if valid_evidence(&value) => Ok(Applicability::Format(value)),
            ("context", Some(value)) if valid_evidence(&value) => Ok(Applicability::Context(value)),
            _ => Err(invalid("invalid applicability declaration")),
        }
    }
}

fn valid_evidence(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_REPRESENTATION_BYTES
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn invalid_owned(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{PackageId, parse};

    const VALID: &str = r#"
api_version = 1
id = "sensor"
entry = "main.lua"

[[providers]]
id = "extension.sensor.reader"

[[providers.fields]]
id = "extension.sensor.reader.distance"
kind = "quantity"
unit = "metres"

[[providers.fields]]
id = "extension.sensor.reader.name"
kind = "text"

[[providers.properties]]
id = "extension.sensor.reader.summary"
label = "Summary"
fields = ["extension.sensor.reader.name"]
max_bytes = 1024
max_items = 10
applicability = { kind = "format", value = "gpx" }

[[providers.columns]]
id = "extension.sensor.reader.distance-cell"
label = "Distance"
column_id = "extension.sensor.reader.distance"
field = "extension.sensor.reader.distance"
max_bytes = 1024
max_items = 10

[[providers.viewers]]
id = "extension.sensor.reader.graph"
label = "Graph"
representation = "sensor.scene"
version = 1
max_bytes = 1024
max_items = 10
applicability = { kind = "context", value = "git" }
"#;

    fn parse_test(manifest: &str) -> std::io::Result<super::Manifest> {
        parse(&PackageId::new("sensor")?, manifest)
    }

    fn count_manifest(
        provider_count: usize,
        fields_per_first: usize,
        viewers_per_provider: usize,
    ) -> String {
        let mut text = String::from("api_version = 1\nid = \"sensor\"\nentry = \"main.lua\"\n");
        for provider in 0..provider_count {
            let owner = format!("extension.sensor.p{provider}");
            text.push_str(&format!("\n[[providers]]\nid = \"{owner}\"\n"));
            if provider == 0 {
                for field in 0..fields_per_first {
                    text.push_str(&format!(
                        "\n[[providers.fields]]\nid = \"{owner}.f{field}\"\nkind = \"text\"\n"
                    ));
                }
            }
            for viewer in 0..viewers_per_provider {
                text.push_str(&format!(
                    "\n[[providers.viewers]]\nid = \"{owner}.v{viewer}\"\nlabel = \"Viewer\"\nrepresentation = \"test.scene\"\nversion = 1\nmax_bytes = 1\nmax_items = 1\n"
                ));
            }
        }
        text
    }

    #[test]
    fn package_id_is_one_path_safe_identifier() {
        for value in [
            "",
            ".",
            "..",
            "sensor/child",
            "sensor\\child",
            "Sensor",
            "sé",
        ] {
            assert!(PackageId::new(value).is_err(), "accepted {value:?}");
        }
        assert_eq!(PackageId::new("sensor_2-x").unwrap().as_str(), "sensor_2-x");
    }

    #[test]
    fn manifest_converts_declarations_to_provider_contracts() {
        let package = parse_test(VALID).unwrap();
        assert_eq!(package.entry, "main.lua");
        assert_eq!(package.providers.len(), 1);
        assert_eq!(package.providers[0].id.as_str(), "extension.sensor.reader");
        assert_eq!(package.providers[0].columns.len(), 1);
        assert_eq!(package.providers[0].properties.len(), 1);
        assert_eq!(package.providers[0].viewers.len(), 1);
        assert_eq!(package.providers[0].fields.len(), 2);
    }

    #[test]
    fn invalid_versions_unknown_keys_ownership_and_references_are_rejected() {
        let bad_manifests = [
            VALID.replace("api_version = 1", "api_version = 2"),
            VALID.replace("id = \"sensor\"", "id = \"other\""),
            VALID.replace(
                "id = \"extension.sensor.reader\"",
                "id = \"extension.other.reader\"",
            ),
            VALID.replace(
                "field = \"extension.sensor.reader.distance\"",
                "field = \"extension.sensor.reader.missing\"",
            ),
            format!("{VALID}\nunknown_key = true\n"),
            VALID.replace("entry = \"main.lua\"", "entry = \"../outside.lua\""),
            VALID.replace("entry = \"main.lua\"", "entry = \"/tmp/outside.lua\""),
        ];
        for manifest in bad_manifests {
            assert!(
                parse_test(&manifest).is_err(),
                "accepted invalid manifest:\n{manifest}"
            );
        }
    }

    #[test]
    fn field_types_demands_and_builtin_column_ids_are_validated() {
        let bad = [
            VALID.replace("unit = \"metres\"\n", ""),
            VALID.replace("kind = \"quantity\"", "kind = \"text\""),
            VALID.replace("max_bytes = 1024", "max_bytes = 16777217"),
            VALID.replace("max_items = 10", "max_items = 0"),
            VALID.replace(
                "column_id = \"extension.sensor.reader.distance\"",
                "column_id = \"size\"",
            ),
        ];
        for manifest in bad {
            assert!(
                parse_test(&manifest).is_err(),
                "accepted invalid declaration:\n{manifest}"
            );
        }
        let text_with_unit = VALID
            .replace("kind = \"quantity\"", "kind = \"text\"")
            .replace("unit = \"metres\"\n", "unit = \"seconds\"\n");
        assert!(parse_test(&text_with_unit).is_err());
    }

    #[test]
    fn manifest_and_source_limits_accept_exact_boundary_and_reject_plus_one() {
        let padding = "#".repeat(super::MANIFEST_BYTES - VALID.len());
        let exact_manifest = format!("{VALID}{padding}");
        assert_eq!(exact_manifest.len(), super::MANIFEST_BYTES);
        assert!(parse_test(&exact_manifest).is_ok());
        assert!(parse_test(&(exact_manifest + "#")).is_err());
    }

    #[test]
    fn declared_ids_accept_256_bytes_and_reject_257() {
        let id_256 = format!("extension.sensor.reader.{}", "x".repeat(232));
        let id_257 = format!("extension.sensor.reader.{}", "x".repeat(233));
        assert_eq!(id_256.len(), 256);
        assert_eq!(id_257.len(), 257);
        let field_id = "extension.sensor.reader.distance";
        let field_spelling = format!("{field_id}\"");
        assert!(
            parse_test(&VALID.replace(&field_spelling, &format!("{id_256}\""))).is_ok(),
            "256-byte field ID rejected"
        );
        assert!(parse_test(&VALID.replace(&field_spelling, &format!("{id_257}\""))).is_err());
    }

    #[test]
    fn declaration_count_limits_accept_the_boundary_and_reject_plus_one() {
        assert!(parse_test(&count_manifest(16, 0, 0)).is_ok());
        assert!(parse_test(&count_manifest(17, 0, 0)).is_err());
        assert!(parse_test(&count_manifest(1, 128, 0)).is_ok());
        assert!(parse_test(&count_manifest(1, 129, 0)).is_err());
        assert!(parse_test(&count_manifest(1, 0, 64)).is_ok());
        assert!(parse_test(&count_manifest(1, 0, 65)).is_err());
        assert!(parse_test(&count_manifest(4, 0, 64)).is_ok());
        assert!(parse_test(&count_manifest(5, 0, 64)).is_err());
    }
}

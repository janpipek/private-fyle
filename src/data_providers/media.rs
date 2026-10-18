//! Shared typed media metadata for the existing Duration, Properties and Preview adapters.

use std::io;
use std::path::Path;
use std::sync::OnceLock;

use crate::data_providers::{
    Applicability, ColumnContribution, Completeness, ContributionHeader, ContributionId,
    DemandLimits, FieldDefinition, FieldId, FieldKind, PropertiesContribution, ProviderDefinition,
    ProviderId, RegistrationOrigin, ResultState, Unit, Value, ViewerContribution,
    validate_definitions,
};
use crate::formats::media::{self, MediaMetadata};
use crate::icons::{self, FileIcon};

const DURATION_FIELD_ID: &str = "builtin.media.duration";

/// Media facts and the validated numeric duration projection from one bounded probe.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedMedia {
    /// Native stream, tag and container facts used by Properties and Preview.
    pub metadata: MediaMetadata,
    /// Independent typed duration value; media detail truncation does not make it partial.
    pub duration: ResultState<crate::data_providers::Quantity>,
}

/// One immutable definition shared by the three media presentation handles.
pub struct MediaProvider {
    definition: ProviderDefinition,
}

/// Handle for the existing built-in Duration column acquisition.
#[derive(Clone, Copy)]
pub struct MediaDurationColumn<'a> {
    provider: &'a MediaProvider,
}

/// Handle for media Properties acquisition.
#[derive(Clone, Copy)]
pub struct MediaProperties<'a> {
    provider: &'a MediaProvider,
}

/// Handle for the media Preview adapter.
#[derive(Clone, Copy)]
pub struct MediaViewer<'a> {
    provider: &'a MediaProvider,
}

/// Returns the one immutable provider definition used by the media adapters.
pub fn provider() -> &'static MediaProvider {
    static PROVIDER: OnceLock<MediaProvider> = OnceLock::new();
    PROVIDER.get_or_init(MediaProvider::new)
}

/// Returns the provider-owned Duration column handle.
pub fn duration_column() -> MediaDurationColumn<'static> {
    MediaDurationColumn {
        provider: provider(),
    }
}

/// Returns the provider-owned Properties handle.
pub fn properties() -> MediaProperties<'static> {
    MediaProperties {
        provider: provider(),
    }
}

/// Returns the provider-owned Preview handle.
pub fn viewer() -> MediaViewer<'static> {
    MediaViewer {
        provider: provider(),
    }
}

impl MediaProvider {
    fn new() -> Self {
        let duration = FieldId::new(DURATION_FIELD_ID).expect("static duration field ID is valid");
        let properties = PropertiesContribution {
            header: ContributionHeader {
                id: ContributionId::new("builtin.media.properties")
                    .expect("static Properties ID is valid"),
                label: "Media details".into(),
                applicability: Applicability::Any,
                demand: demand(),
            },
            fields: vec![duration.clone()],
        };
        let column = ColumnContribution {
            header: ContributionHeader {
                id: ContributionId::new("builtin.media.duration-column")
                    .expect("static Duration column ID is valid"),
                label: "Duration".into(),
                applicability: Applicability::Any,
                demand: demand(),
            },
            column_id: "duration".into(),
            field: duration.clone(),
        };
        let viewer = ViewerContribution {
            header: ContributionHeader {
                id: ContributionId::new("builtin.media.viewer").expect("static viewer ID is valid"),
                label: "Media details".into(),
                applicability: Applicability::Any,
                demand: demand(),
            },
            representation: "media.streams".into(),
            version: 1,
        };
        let definition = ProviderDefinition {
            id: ProviderId::new("builtin.media").expect("static provider ID is valid"),
            fields: vec![FieldDefinition {
                id: duration,
                kind: FieldKind::Quantity(Unit::Seconds),
            }],
            properties: vec![properties],
            columns: vec![column],
            viewers: vec![viewer],
        };
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("built-in media contributions are valid");
        Self { definition }
    }

    /// Returns the complete definition shared by the three adapters.
    pub fn definition(&self) -> &ProviderDefinition {
        &self.definition
    }

    /// Prepares typed provider fields from an already bounded native metadata result.
    pub fn prepare(&self, metadata: MediaMetadata) -> PreparedMedia {
        let duration_field = self
            .definition
            .fields
            .iter()
            .find(|field| field.id.as_str() == DURATION_FIELD_ID)
            .expect("the built-in media duration field is registered");
        self.prepare_for_field(metadata, &duration_field.id)
    }

    fn prepare_for_field(&self, metadata: MediaMetadata, field_id: &FieldId) -> PreparedMedia {
        let duration_field = self
            .definition
            .fields
            .iter()
            .find(|field| &field.id == field_id)
            .expect("media contribution fields are registered");
        let duration = match metadata.duration {
            None => ResultState::Absent,
            Some(seconds) => match Value::quantity(seconds, Unit::Seconds) {
                Ok(value) => match duration_field.validate(&value) {
                    Ok(()) => match value {
                        Value::Quantity(quantity) => ResultState::Ready {
                            value: quantity,
                            completeness: Completeness::Complete,
                        },
                        Value::Text(_) => {
                            ResultState::Failed("media duration projection was not numeric".into())
                        }
                    },
                    Err(error) => ResultState::Failed(error.to_string()),
                },
                Err(error) => ResultState::Failed(error.to_string()),
            },
        };
        PreparedMedia { metadata, duration }
    }

    fn read(
        &self,
        path: &Path,
        field_id: &FieldId,
        cancelled: impl Fn() -> bool,
    ) -> io::Result<PreparedMedia> {
        Ok(self.prepare_for_field(media::probe(path, cancelled)?, field_id))
    }
}

impl MediaDurationColumn<'_> {
    /// Returns the descriptor for the preserved built-in `duration` column.
    pub fn descriptor(&self) -> &ColumnContribution {
        &self.provider.definition.columns[0]
    }

    /// Reads and prepares the duration while retaining the panel queue's blank-on-failure policy.
    pub fn read(&self, path: &Path, cancelled: impl Fn() -> bool) -> io::Result<PreparedMedia> {
        self.provider
            .read(path, &self.descriptor().field, cancelled)
    }
}

impl MediaProperties<'_> {
    /// Returns the descriptor used by the existing media Properties adapter.
    pub fn descriptor(&self) -> &PropertiesContribution {
        &self.provider.definition.properties[0]
    }

    /// Whether the existing filename-based icon catalog recognizes media here.
    pub fn matches_path(&self, path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_media_name)
    }

    /// Reads bounded metadata and prepares the typed duration field.
    pub fn read(&self, path: &Path, cancelled: impl Fn() -> bool) -> io::Result<PreparedMedia> {
        let field_id = self
            .descriptor()
            .fields
            .iter()
            .find(|field| field.as_str() == DURATION_FIELD_ID)
            .expect("media Properties contribution declares the duration field");
        self.provider.read(path, field_id, cancelled)
    }
}

impl MediaViewer<'_> {
    /// Returns the descriptor for the existing media Preview representation.
    pub fn descriptor(&self) -> &ViewerContribution {
        &self.provider.definition.viewers[0]
    }

    /// Whether a captured file name matches the existing media preview policy.
    pub fn matches_name(&self, name: &str) -> bool {
        is_media_name(name)
    }

    /// Reads bounded metadata and prepares the typed duration used for frame seeking.
    pub fn read(&self, path: &Path, cancelled: impl Fn() -> bool) -> io::Result<PreparedMedia> {
        let field = self
            .provider
            .definition
            .fields
            .iter()
            .find(|field| field.id.as_str() == DURATION_FIELD_ID)
            .expect("the media duration field is registered");
        self.provider.read(path, &field.id, cancelled)
    }
}

fn demand() -> DemandLimits {
    DemandLimits {
        // The 1 MiB bound applies to ffprobe output, not bytes read from the source.
        max_bytes: 0,
        max_items: media::STREAM_LIMIT as u64,
    }
}

fn is_media_name(name: &str) -> bool {
    matches!(
        icons::icon_for(name, false, false, false),
        FileIcon::Audio | FileIcon::Video
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DURATION_FIELD_ID, MediaDurationColumn, MediaProperties, MediaProvider, MediaViewer,
        provider,
    };
    use crate::data_providers::{
        Completeness, FieldId, FieldKind, RegistrationOrigin, ResultState, Unit,
        validate_definitions,
    };
    use crate::formats::media::{MediaMetadata, parse};

    #[test]
    fn one_definition_binds_all_three_media_presentations() {
        let provider = provider();
        let definition = provider.definition();
        assert_eq!(definition.id.as_str(), "builtin.media");
        assert_eq!(definition.properties.len(), 1);
        assert_eq!(definition.columns.len(), 1);
        assert_eq!(definition.columns[0].column_id, "duration");
        assert_eq!(definition.viewers.len(), 1);
        assert_eq!(definition.viewers[0].representation, "media.streams");
        assert_eq!(
            MediaDurationColumn { provider }.descriptor().field,
            definition.fields[0].id
        );
        assert_eq!(
            MediaProperties { provider }.descriptor().fields,
            [definition.fields[0].id.clone()]
        );
        assert_eq!(
            MediaViewer { provider }.descriptor().header.demand,
            definition.properties[0].header.demand
        );
    }

    #[test]
    fn reordering_fields_keeps_media_column_and_properties_bindings_stable() {
        let mut definition = provider().definition().clone();
        definition.fields.insert(
            0,
            crate::data_providers::FieldDefinition {
                id: FieldId::new("builtin.media.label").expect("valid test field ID"),
                kind: FieldKind::Text,
            },
        );
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("reordered provider definition remains valid");
        let provider = MediaProvider { definition };
        let column = MediaDurationColumn {
            provider: &provider,
        };
        let properties = MediaProperties {
            provider: &provider,
        };
        assert_eq!(column.descriptor().field.as_str(), DURATION_FIELD_ID);
        assert_eq!(
            properties.descriptor().fields[0].as_str(),
            DURATION_FIELD_ID
        );

        let metadata = parse(br#"{"format":{"duration":"65"}}"#).expect("captured metadata parses");
        let prepared = provider.prepare_for_field(metadata, &column.descriptor().field);
        assert!(matches!(
            prepared.duration,
            ResultState::Ready { value, .. } if value.value() == 65.0
        ));
    }

    #[test]
    fn typed_duration_remains_complete_when_stream_details_are_truncated() {
        let streams = vec![serde_json::json!({"codec_type": "audio"}); 33];
        let captured = serde_json::json!({
            "streams": streams,
            "format": {"duration": "65"}
        });
        let metadata = parse(&serde_json::to_vec(&captured).expect("captured JSON fixture"))
            .expect("captured ffprobe metadata parses");
        assert!(metadata.truncated);
        let prepared = provider().prepare(metadata);
        assert!(matches!(
            prepared.duration,
            ResultState::Ready {
                value,
                completeness: Completeness::Complete
            } if value.value() == 65.0 && value.unit() == Unit::Seconds
        ));
    }

    #[test]
    fn missing_or_invalid_duration_stays_absent_without_losing_native_metadata() {
        let missing = provider().prepare(MediaMetadata::default());
        assert!(matches!(missing.duration, ResultState::Absent));
        let malformed = parse(br#"{"format":{"duration":"NaN"}}"#)
            .expect("malformed optional duration leaves metadata readable");
        let prepared = provider().prepare(malformed);
        assert!(matches!(prepared.duration, ResultState::Absent));
    }

    #[test]
    fn properties_and_preview_keep_the_existing_filename_icon_policy() {
        let properties = super::properties();
        let viewer = super::viewer();
        assert!(properties.matches_path(std::path::Path::new("song.mp3")));
        assert!(viewer.matches_name("movie.mp4"));
        assert!(!properties.matches_path(std::path::Path::new("plain.txt")));
        assert!(!viewer.matches_name("plain.txt"));
    }
}

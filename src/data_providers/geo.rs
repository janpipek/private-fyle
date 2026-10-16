//! Shared GPX/FIT provider adapter for Properties and View.

use std::io;
use std::path::Path;
use std::sync::OnceLock;

use crate::data_providers::{
    Applicability, ColumnContribution, Completeness, ContributionHeader, ContributionId,
    DemandLimits, FieldDefinition, FieldId, FieldKind, PropertiesContribution, Provenance,
    ProviderDefinition, ProviderId, RegistrationOrigin, ResultState, Unit, Value,
    ViewerContribution, validate_definitions,
};
use crate::formats::geo::{self, GeoData, Measurement, MeasurementOrigin};

const DURATION_FIELD_ID: &str = "builtin.geo.duration";
const DISTANCE_FIELD_ID: &str = "builtin.geo.distance";

/// Prepared numeric output for one geo field, retaining how the source supplied it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreparedQuantity {
    /// Finite quantity with an explicit unit.
    pub quantity: crate::data_providers::Quantity,
    /// Whether the parser read this value directly or derived it.
    pub provenance: Provenance,
}

/// One bounded native read and its independently prepared numeric fields.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedGeo {
    /// Native route/activity data retained for existing text and drawing adapters.
    pub data: GeoData,
    /// Duration value, absence, or independent projection failure.
    pub duration: ResultState<PreparedQuantity>,
    /// Distance value, absence, or independent projection failure.
    pub distance: ResultState<PreparedQuantity>,
}

/// The built-in GPX/FIT provider definition and bounded reader.
pub struct GeoProvider {
    definition: ProviderDefinition,
}

/// Handle to this provider's Properties contribution.
#[derive(Clone, Copy)]
pub struct GeoProperties {
    provider: &'static GeoProvider,
}

/// Handle to this provider's View contribution.
#[derive(Clone, Copy)]
pub struct GeoViewer {
    provider: &'static GeoProvider,
}

/// Returns the single immutable provider used by both presentation adapters.
pub fn provider() -> &'static GeoProvider {
    static PROVIDER: OnceLock<GeoProvider> = OnceLock::new();
    PROVIDER.get_or_init(GeoProvider::new)
}

/// Returns the provider-owned Properties contribution handle.
pub fn properties() -> GeoProperties {
    GeoProperties {
        provider: provider(),
    }
}

/// Returns the provider-owned View contribution handle.
pub fn viewer() -> GeoViewer {
    GeoViewer {
        provider: provider(),
    }
}

impl GeoProvider {
    fn new() -> Self {
        let id = ProviderId::new("builtin.geo").expect("static provider ID is valid");
        let duration = FieldId::new(DURATION_FIELD_ID).expect("static duration field ID is valid");
        let distance = FieldId::new(DISTANCE_FIELD_ID).expect("static distance field ID is valid");
        let properties = PropertiesContribution {
            header: ContributionHeader {
                id: ContributionId::new("builtin.geo.properties")
                    .expect("static Properties ID is valid"),
                label: "Route / activity".into(),
                applicability: Applicability::Format("geo.activity".into()),
                demand: demand(),
            },
            fields: vec![duration.clone(), distance.clone()],
        };
        let viewer = ViewerContribution {
            header: ContributionHeader {
                id: ContributionId::new("builtin.geo.viewer").expect("static viewer ID is valid"),
                label: "Route / activity".into(),
                applicability: Applicability::Format("geo.activity".into()),
                demand: demand(),
            },
            representation: "geo.route".into(),
            version: 1,
        };
        let definition = ProviderDefinition {
            id,
            fields: vec![
                FieldDefinition {
                    id: duration,
                    kind: FieldKind::Quantity(Unit::Seconds),
                },
                FieldDefinition {
                    id: distance,
                    kind: FieldKind::Quantity(Unit::Metres),
                },
            ],
            properties: vec![properties],
            columns: Vec::<ColumnContribution>::new(),
            viewers: vec![viewer],
        };
        validate_definitions(&[(RegistrationOrigin::Builtin, definition.clone())])
            .expect("built-in geo contributions are valid");
        Self { definition }
    }

    /// Returns the complete provider definition shared by both adapters.
    pub fn definition(&self) -> &ProviderDefinition {
        &self.definition
    }

    fn read(&self, path: &Path, cancelled: impl Fn() -> bool) -> io::Result<PreparedGeo> {
        let data = geo::read(path, cancelled)?;
        Ok(self.prepare_data(data))
    }

    fn prepare_data(&self, data: GeoData) -> PreparedGeo {
        let duration_field = self
            .definition
            .fields
            .iter()
            .find(|field| field.id.as_str() == DURATION_FIELD_ID)
            .expect("the built-in duration field is registered");
        let distance_field = self
            .definition
            .fields
            .iter()
            .find(|field| field.id.as_str() == DISTANCE_FIELD_ID)
            .expect("the built-in distance field is registered");
        PreparedGeo {
            duration: prepare_measurement(&data, data.duration_seconds, duration_field),
            distance: prepare_measurement(&data, data.distance_meters, distance_field),
            data,
        }
    }
}

impl GeoProperties {
    /// Returns the descriptor declared by the shared provider.
    pub fn descriptor(&self) -> &PropertiesContribution {
        &self.provider.definition.properties[0]
    }

    /// Whether captured extension evidence matches this contribution.
    pub fn matches_extension(&self, extension: &str) -> bool {
        matches_format(&self.descriptor().header.applicability, extension)
    }

    /// Reads bounded geo data and prepares its numeric Properties values.
    pub fn read(&self, path: &Path, cancelled: impl Fn() -> bool) -> io::Result<PreparedGeo> {
        self.provider.read(path, cancelled)
    }
}

impl GeoViewer {
    /// Returns the descriptor declared by the shared provider.
    pub fn descriptor(&self) -> &ViewerContribution {
        &self.provider.definition.viewers[0]
    }

    /// Whether captured extension evidence matches this contribution.
    pub fn matches_extension(&self, extension: &str) -> bool {
        matches_format(&self.descriptor().header.applicability, extension)
    }

    /// Reads bounded geo data and prepares its numeric View values.
    pub fn read(&self, path: &Path, cancelled: impl Fn() -> bool) -> io::Result<PreparedGeo> {
        self.provider.read(path, cancelled)
    }
}

fn demand() -> DemandLimits {
    DemandLimits {
        max_bytes: geo::MAX_BYTES,
        max_items: geo::MAX_POINTS as u64,
    }
}

fn matches_format(applicability: &Applicability, extension: &str) -> bool {
    let Some(format) = format_id(extension) else {
        return false;
    };
    matches!(applicability, Applicability::Format(expected) if expected == format)
}

fn prepare_measurement(
    data: &GeoData,
    measurement: Option<Measurement>,
    field: &FieldDefinition,
) -> ResultState<PreparedQuantity> {
    let Some(measurement) = measurement else {
        return ResultState::Absent;
    };
    let unit = match field.kind {
        FieldKind::Quantity(unit) => unit,
        FieldKind::Text => return ResultState::Failed("geo metric field is not numeric".into()),
    };
    let value = match Value::quantity(measurement.value, unit) {
        Ok(Value::Quantity(quantity)) => quantity,
        Ok(Value::Text(_)) => {
            return ResultState::Failed("geo metric projection was not numeric".into());
        }
        Err(error) => return ResultState::Failed(error.to_string()),
    };
    let completeness = if measurement.origin == MeasurementOrigin::Recorded || !data.truncated {
        Completeness::Complete
    } else {
        Completeness::Truncated("GPS point limit reached".into())
    };
    ResultState::Ready {
        value: PreparedQuantity {
            quantity: value,
            provenance: match measurement.origin {
                MeasurementOrigin::Recorded => Provenance::Recorded,
                MeasurementOrigin::Inferred => Provenance::Inferred,
            },
        },
        completeness,
    }
}

/// Maps a public extension to the stable geo activity evidence ID.
pub fn format_id(extension: &str) -> Option<&'static str> {
    matches!(extension.to_ascii_lowercase().as_str(), "gpx" | "fit").then_some("geo.activity")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{format_id, provider};
    use crate::data_providers::{Completeness, Provenance, ResultState, Unit};
    use crate::formats::geo;
    use crate::formats::geo::{GeoData, Measurement, MeasurementOrigin, Point};
    use tempfile::TempDir;

    #[test]
    fn both_handles_share_one_definition_and_apply_to_gpx_and_fit() {
        let properties = super::properties();
        let viewer = super::viewer();
        assert!(std::ptr::eq(properties.provider, viewer.provider));
        assert!(std::ptr::eq(
            properties.descriptor(),
            &provider().definition().properties[0]
        ));
        assert!(std::ptr::eq(
            viewer.descriptor(),
            &provider().definition().viewers[0]
        ));
        assert!(properties.matches_extension("gpx"));
        assert!(properties.matches_extension("FIT"));
        assert!(viewer.matches_extension("gpx"));
        assert!(!viewer.matches_extension("txt"));
        assert_eq!(provider().definition().columns.len(), 0);
        assert_eq!(
            properties.descriptor().header.demand.max_bytes,
            geo::MAX_BYTES
        );
        assert_eq!(
            viewer.descriptor().header.demand.max_items,
            geo::MAX_POINTS as u64
        );
        let mut reordered = provider().definition().clone();
        reordered.fields.reverse();
        let reordered_provider = super::GeoProvider {
            definition: reordered,
        };
        let prepared = reordered_provider.prepare_data(GeoData {
            duration_seconds: Some(Measurement {
                value: 2.0,
                origin: MeasurementOrigin::Recorded,
            }),
            distance_meters: Some(Measurement {
                value: 3.0,
                origin: MeasurementOrigin::Recorded,
            }),
            ..GeoData::default()
        });
        assert!(
            matches!(prepared.duration, ResultState::Ready { value, .. } if value.quantity.unit() == Unit::Seconds)
        );
        assert!(
            matches!(prepared.distance, ResultState::Ready { value, .. } if value.quantity.unit() == Unit::Metres)
        );
        assert_eq!(reordered_provider.definition.fields.len(), 2);
    }

    #[test]
    fn measurement_preparation_keeps_provenance_completeness_and_failures_independent() {
        let data = GeoData {
            truncated: true,
            points: 3,
            segments: vec![vec![Point {
                latitude: 0.0,
                longitude: 0.0,
            }]],
            duration_seconds: Some(Measurement {
                value: 60.0,
                origin: MeasurementOrigin::Recorded,
            }),
            distance_meters: Some(Measurement {
                value: -1.0,
                origin: MeasurementOrigin::Inferred,
            }),
            ..GeoData::default()
        };
        let provider = provider();
        let prepared = provider.prepare_data(data);
        assert!(
            matches!(prepared.duration, ResultState::Ready { value, completeness: Completeness::Complete } if value.provenance == Provenance::Recorded && value.quantity.unit() == Unit::Seconds)
        );
        assert!(matches!(prepared.distance, ResultState::Failed(_)));
        assert_eq!(prepared.data.points, 3);
        assert_eq!(prepared.data.segments.len(), 1);

        let inferred = super::prepare_measurement(
            &prepared.data,
            Some(Measurement {
                value: 0.0,
                origin: MeasurementOrigin::Inferred,
            }),
            provider
                .definition
                .fields
                .iter()
                .find(|field| field.id.as_str() == "builtin.geo.distance")
                .expect("distance field is registered"),
        );
        assert!(
            matches!(inferred, ResultState::Ready { value, completeness: Completeness::Truncated(_) } if value.quantity.value() == 0.0)
        );
    }

    #[test]
    fn handles_read_values_and_keep_absence_and_failures_separate() {
        let directory = TempDir::new().expect("temporary directory");
        let valid = directory.path().join("route.gpx");
        fs::write(
            &valid,
            r#"<gpx version="1.1" creator="test"><trk><trkseg><trkpt lat="0" lon="0"><time>2020-01-01T00:00:00Z</time></trkpt><trkpt lat="0" lon="0.001"><time>2020-01-01T00:01:00Z</time></trkpt></trkseg></trk></gpx>"#,
        )
        .expect("write GPX fixture");
        let prepared = super::properties()
            .read(&valid, || false)
            .expect("read GPX");
        assert!(
            matches!(prepared.duration, ResultState::Ready { value, completeness: Completeness::Complete } if value.provenance == Provenance::Inferred && value.quantity.value() == 60.0)
        );
        assert!(
            matches!(prepared.distance, ResultState::Ready { value, .. } if value.provenance == Provenance::Inferred && (value.quantity.value() - 111.2).abs() < 1.0)
        );
        let cancelled = super::viewer()
            .read(&valid, || true)
            .expect_err("cancelled read");
        assert_eq!(cancelled.kind(), std::io::ErrorKind::Interrupted);

        let malformed = directory.path().join("bad.gpx");
        fs::write(&malformed, b"not GPX").expect("write malformed fixture");
        assert!(super::viewer().read(&malformed, || false).is_err());

        let no_gps = directory.path().join("metadata.gpx");
        fs::write(
            &no_gps,
            r#"<gpx version="1.1" creator="test"><metadata><name>Only metadata</name></metadata><wpt lat="bad" lon="bad"/></gpx>"#,
        )
        .expect("write no-GPS fixture");
        let prepared = super::viewer()
            .read(&no_gps, || false)
            .expect("read metadata-only GPX");
        assert_eq!(prepared.data.name.as_deref(), Some("Only metadata"));
        assert!(matches!(prepared.duration, ResultState::Absent));
        assert!(matches!(prepared.distance, ResultState::Absent));
    }

    #[test]
    fn inferred_gpx_metrics_report_point_limit_completeness() {
        let directory = TempDir::new().expect("temporary directory");
        let path = directory.path().join("long.gpx");
        let mut xml = String::from("<gpx version=\"1.1\" creator=\"test\"><trk><trkseg>");
        for _ in 0..=geo::MAX_POINTS {
            xml.push_str("<trkpt lat=\"0\" lon=\"0\"/>");
        }
        xml.push_str("</trkseg></trk></gpx>");
        fs::write(&path, xml).expect("write bounded GPX fixture");
        let prepared = super::viewer()
            .read(&path, || false)
            .expect("read bounded GPX");
        assert_eq!(prepared.data.points, geo::MAX_POINTS);
        assert!(prepared.data.truncated);
        assert!(matches!(
            prepared.distance,
            ResultState::Ready {
                completeness: Completeness::Truncated(_),
                ..
            }
        ));
    }

    #[test]
    fn format_mapping_is_shared_with_both_descriptors() {
        assert_eq!(format_id("GPX"), Some("geo.activity"));
        assert_eq!(format_id("txt"), None);
        assert_eq!(format_id("fit"), Some("geo.activity"));
    }
}

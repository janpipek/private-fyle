//! GPX and FIT route/activity facts mapped into a Properties group.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::data_providers::geo::{GeoProperties, PreparedGeo};
use crate::formats::geo as geo_format;
use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyValue,
};

/// GPX and FIT metadata provider for the Properties registry.
pub(super) struct Provider {
    provider: GeoProperties,
}

impl Provider {
    /// Builds the Properties adapter over the shared geo provider.
    pub fn new() -> Self {
        Self {
            provider: crate::data_providers::geo::properties(),
        }
    }
}

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "route_activity"
    }

    fn matches(&self, probe: &Probe) -> bool {
        probe.is_file && self.provider.matches_extension(&probe.ext)
    }

    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        let prepared = self
            .provider
            .read(&probe.path, || cancel.load(Ordering::Relaxed))?;
        Ok(vec![group(&prepared)])
    }
}

fn group(prepared: &PreparedGeo) -> PropertyGroup {
    let data = &prepared.data;
    let mut group = PropertyGroup {
        id: GroupId("route_activity".into()),
        status: if data.truncated {
            GroupStatus::Truncated {
                reason: format!("GPS points limited to {}", geo_format::MAX_POINTS),
            }
        } else {
            GroupStatus::Available
        },
        fields: Vec::new(),
        groups: Vec::new(),
        tables: Vec::new(),
    };
    text(&mut group, "format", data.format);
    for (id, value) in [
        ("type", &data.kind),
        ("name", &data.name),
        ("sport", &data.sport),
        ("start", &data.start),
    ] {
        if let Some(value) = value {
            text(&mut group, id, value);
        }
    }
    metric(&mut group, "duration_seconds", &prepared.duration);
    metric(&mut group, "distance_meters", &prepared.distance);
    for (id, value) in [
        ("ascent_meters", data.ascent_meters),
        ("descent_meters", data.descent_meters),
    ] {
        if let Some(value) = value {
            group.fields.push(PropertyField {
                id: id.into(),
                value: PropertyValue::Text(format!("{value:.1}")),
            });
        }
    }
    for (id, value) in [
        ("tracks", data.tracks),
        ("routes", data.routes),
        ("waypoints", data.waypoints),
        ("laps", data.laps),
        ("gps_points", data.points),
    ] {
        if value > 0 {
            group.fields.push(PropertyField {
                id: id.into(),
                value: PropertyValue::Unsigned(value as u64),
            });
        }
    }
    group
}

fn metric(
    group: &mut PropertyGroup,
    id: &str,
    state: &crate::data_providers::ResultState<crate::data_providers::geo::PreparedQuantity>,
) {
    use crate::data_providers::ResultState;
    let value = match state {
        ResultState::Ready { value, .. } => format!("{:.1}", value.quantity.value()),
        ResultState::Failed(reason) => reason.clone(),
        ResultState::Pending | ResultState::Absent | ResultState::Unavailable(_) => return,
    };
    group.fields.push(PropertyField {
        id: id.into(),
        value: PropertyValue::Text(value),
    });
}

fn text(group: &mut PropertyGroup, id: &str, value: &str) {
    group.fields.push(PropertyField {
        id: id.into(),
        value: PropertyValue::Text(value.into()),
    });
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use tempfile::TempDir;

    use super::{PreparedGeo, Provider};
    use crate::data_providers::geo::PreparedQuantity;
    use crate::data_providers::{Completeness, Provenance, Quantity, ResultState, Unit};
    use crate::formats::geo::GeoData;
    use crate::properties::{Probe, PropertyProvider, PropertyValue};

    #[test]
    fn provider_adds_route_facts() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("route.gpx");
        fs::write(&path, r#"<gpx version="1.1" creator="test"><trk><name>Loop</name><trkseg><trkpt lat="0" lon="0"><time>2020-01-01T00:00:00Z</time></trkpt><trkpt lat="0" lon="0.001"><time>2020-01-01T00:01:01Z</time></trkpt></trkseg></trk></gpx>"#).expect("write GPX");
        let probe = Probe {
            path,
            name: "route.gpx".into(),
            ext: "gpx".into(),
            is_dir: false,
            is_file: true,
            size: 0,
        };
        let provider = Provider::new();
        assert!(provider.matches(&probe));
        let groups = provider
            .read(&probe, &Arc::new(AtomicBool::new(false)))
            .expect("read properties");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id.0, "route_activity");
        let value = |id: &str| {
            groups[0]
                .fields
                .iter()
                .find(|field| field.id == id)
                .map(|field| &field.value)
        };
        assert_eq!(
            value("duration_seconds"),
            Some(&PropertyValue::Text("61.0".into()))
        );
        assert_eq!(
            value("distance_meters"),
            Some(&PropertyValue::Text("111.2".into()))
        );
        assert_eq!(value("type"), Some(&PropertyValue::Text("Track".into())));
    }

    #[test]
    fn failed_metric_keeps_other_properties_and_numeric_zero() {
        let data = GeoData {
            format: "GPX",
            kind: Some("Track".into()),
            tracks: 1,
            points: 2,
            ..GeoData::default()
        };
        let group = super::group(&PreparedGeo {
            data,
            duration: ResultState::Ready {
                value: PreparedQuantity {
                    quantity: Quantity::new(0.0, Unit::Seconds).expect("zero duration"),
                    provenance: Provenance::Inferred,
                },
                completeness: Completeness::Complete,
            },
            distance: ResultState::Failed("invalid distance".into()),
        });
        let value = |id: &str| {
            group
                .fields
                .iter()
                .find(|field| field.id == id)
                .map(|field| &field.value)
        };
        assert_eq!(
            value("duration_seconds"),
            Some(&PropertyValue::Text("0.0".into()))
        );
        assert_eq!(
            value("distance_meters"),
            Some(&PropertyValue::Text("invalid distance".into()))
        );
        assert_eq!(value("gps_points"), Some(&PropertyValue::Unsigned(2)));
    }
}

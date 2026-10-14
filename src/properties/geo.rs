//! GPX and FIT route/activity facts mapped into a Properties group.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::formats::geo::{self, GeoData};
use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyValue,
};

/// GPX and FIT metadata provider for the Properties registry.
pub(super) struct Provider;

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "route_activity"
    }

    fn matches(&self, probe: &Probe) -> bool {
        probe.is_file && matches!(probe.ext.as_str(), "gpx" | "fit")
    }

    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        let data = geo::read(&probe.path, || cancel.load(Ordering::Relaxed))?;
        Ok(vec![group(&data)])
    }
}

fn group(data: &GeoData) -> PropertyGroup {
    let mut group = PropertyGroup {
        id: GroupId("route_activity".into()),
        status: if data.truncated {
            GroupStatus::Truncated {
                reason: format!("GPS points limited to {}", geo::MAX_POINTS),
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
    for (id, value) in [
        ("duration_seconds", data.duration_seconds),
        ("distance_meters", data.distance_meters),
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

    use super::Provider;
    use crate::properties::{Probe, PropertyProvider};

    #[test]
    fn provider_adds_route_facts() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("route.gpx");
        fs::write(&path, r#"<gpx version="1.1" creator="test"><rte><name>Loop</name><rtept lat="0" lon="0"/><rtept lat="0" lon="0.001"/></rte></gpx>"#).expect("write GPX");
        let probe = Probe {
            path,
            name: "route.gpx".into(),
            ext: "gpx".into(),
            is_dir: false,
            is_file: true,
            size: 0,
        };
        assert!(Provider.matches(&probe));
        let groups = Provider
            .read(&probe, &Arc::new(AtomicBool::new(false)))
            .expect("read properties");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id.0, "route_activity");
        assert!(
            groups[0]
                .fields
                .iter()
                .any(|field| field.id == "distance_meters")
        );
    }
}

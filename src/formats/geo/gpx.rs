//! Bounded GPX 1.0/1.1 waypoint, route, and track reader.

use std::fs;
use std::io;
use std::path::Path;

use roxmltree::{Document, Node};

use super::{
    GeoData, MAX_BYTES, MAX_POINTS, Measurement, MeasurementOrigin, Point, distance, point,
};

pub fn read(path: &Path, cancelled: impl Fn() -> bool) -> io::Result<GeoData> {
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("GPX file exceeds 16 MiB limit"));
    }
    let xml = std::str::from_utf8(&bytes).map_err(|error| invalid(error.to_string()))?;
    let document = Document::parse(xml).map_err(|error| invalid(error.to_string()))?;
    let root = document.root_element();
    if !root.has_tag_name("gpx") {
        return Err(invalid("not a GPX document"));
    }
    let mut data = GeoData {
        format: "GPX",
        ..GeoData::default()
    };
    if let Some(metadata) = child(root, "metadata") {
        data.name = child_text(metadata, "name");
    }
    let mut first_time = None;
    let mut last_time = None;
    let mut waypoint_time = None;
    let mut ascent = 0.0;
    let mut descent = 0.0;
    let mut distance_meters = 0.0;
    let mut measured = false;
    for item in root.children().filter(Node::is_element) {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        match item.tag_name().name() {
            "wpt" => {
                data.waypoints += 1;
                if data.points == MAX_POINTS {
                    data.truncated = true;
                    break;
                }
                if let Some(location) = coordinates(item) {
                    if data.name.is_none() {
                        data.name = child_text(item, "name");
                    }
                    if waypoint_time.is_none() {
                        waypoint_time = child_text(item, "time");
                    }
                    data.segments.push(vec![location]);
                    data.points += 1;
                }
            }
            "rte" | "trk" => {
                let is_track = item.has_tag_name("trk");
                if is_track {
                    data.tracks += 1;
                } else {
                    data.routes += 1;
                }
                if data.name.is_none() {
                    data.name = child_text(item, "name");
                }
                let segments: Vec<_> = if is_track {
                    item.children()
                        .filter(|n| n.has_tag_name("trkseg"))
                        .collect()
                } else {
                    vec![item]
                };
                for segment in segments {
                    let mut outline = Vec::new();
                    let mut previous: Option<(Point, Option<f64>)> = None;
                    for node in segment
                        .children()
                        .filter(|n| n.has_tag_name(if is_track { "trkpt" } else { "rtept" }))
                    {
                        if cancelled() {
                            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                        }
                        if data.points == MAX_POINTS {
                            data.truncated = true;
                            break;
                        }
                        let Some(location) = coordinates(node) else {
                            continue;
                        };
                        let elevation = child_text(node, "ele")
                            .and_then(|s| s.parse::<f64>().ok())
                            .filter(|v| v.is_finite());
                        let time = child_text(node, "time");
                        if let Some(time) = time {
                            if first_time.is_none() {
                                first_time = Some(time.clone());
                            }
                            last_time = Some(time);
                        }
                        if let Some((before, before_elevation)) = previous {
                            distance_meters += distance(before, location);
                            measured = true;
                            if let (Some(before), Some(after)) = (before_elevation, elevation) {
                                let change = after - before;
                                if change > 0.0 {
                                    ascent += change;
                                } else {
                                    descent -= change;
                                }
                            }
                        }
                        previous = Some((location, elevation));
                        outline.push(location);
                        data.points += 1;
                    }
                    if !outline.is_empty() {
                        data.segments.push(outline);
                    }
                    if data.truncated {
                        break;
                    }
                }
            }
            _ => {}
        }
        if data.truncated {
            break;
        }
    }
    data.kind = Some(
        if data.tracks > 0 {
            "Track"
        } else if data.routes > 0 {
            "Route"
        } else {
            "Waypoints"
        }
        .into(),
    );
    data.start = first_time.clone().or(waypoint_time);
    if let (Some(first), Some(last)) = (first_time, last_time)
        && let (Ok(first), Ok(last)) = (
            first.parse::<jiff::Timestamp>(),
            last.parse::<jiff::Timestamp>(),
        )
        && last.as_second() >= first.as_second()
    {
        data.duration_seconds = Some(Measurement {
            value: (last.as_second() - first.as_second()) as f64,
            origin: MeasurementOrigin::Inferred,
        });
    }
    if measured {
        data.distance_meters = Some(Measurement {
            value: distance_meters,
            origin: MeasurementOrigin::Inferred,
        });
    }
    if ascent > 0.0 {
        data.ascent_meters = Some(ascent);
    }
    if descent > 0.0 {
        data.descent_meters = Some(descent);
    }
    Ok(data)
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn coordinates(node: Node<'_, '_>) -> Option<Point> {
    node.attribute("lat")
        .and_then(|value| value.parse().ok())
        .zip(node.attribute("lon").and_then(|value| value.parse().ok()))
        .and_then(|(lat, lon)| point(lat, lon))
}

fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    child(node, name)?
        .text()
        .map(|text| text.chars().take(1024).collect())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::read;
    use crate::formats::geo::MeasurementOrigin;

    #[test]
    fn reads_tracks_without_joining_segments() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("route.gpx");
        fs::write(&path, r#"<gpx version="1.1" creator="test"><metadata><name>Morning</name></metadata><trk><trkseg><trkpt lat="0" lon="0"><ele>10</ele><time>2020-01-01T00:00:00Z</time></trkpt><trkpt lat="0" lon="0.001"><ele>15</ele><time>2020-01-01T00:01:00Z</time></trkpt></trkseg><trkseg><trkpt lat="1" lon="1"/></trkseg></trk></gpx>"#).expect("write GPX");
        let data = read(&path, || false).expect("read GPX");
        assert_eq!(data.name.as_deref(), Some("Morning"));
        assert_eq!(data.segments.len(), 2);
        assert_eq!(data.points, 3);
        assert!((data.distance_meters.expect("distance").value - 111.2).abs() < 1.0);
        assert_eq!(data.duration_seconds.expect("duration").value, 60.0);
        assert_eq!(
            data.duration_seconds.expect("duration").origin,
            MeasurementOrigin::Inferred
        );
        assert_eq!(
            data.distance_meters.expect("distance").origin,
            MeasurementOrigin::Inferred
        );
        assert_eq!(data.ascent_meters, Some(5.0));
    }

    #[test]
    fn retains_waypoints_without_inventing_distance() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("stops.gpx");
        fs::write(&path, r#"<gpx version="1.1" creator="test"><wpt lat="0" lon="0"><name>Start</name></wpt><wpt lat="1" lon="1"/></gpx>"#).expect("write GPX");
        let data = read(&path, || false).expect("read GPX");
        assert_eq!(data.waypoints, 2);
        assert_eq!(data.points, 2);
        assert_eq!(data.segments.len(), 2);
        assert_eq!(data.distance_meters, None);
    }
}

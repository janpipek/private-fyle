//! Bounded FIT metadata and GPS record reader.

use std::fs;
use std::io;
use std::path::Path;

use fitparser::profile::MesgNum;
use fitparser::{FitDataRecord, Value};

use super::{GeoData, MAX_BYTES, MAX_POINTS, distance, point};

pub fn read(path: &Path, cancelled: impl Fn() -> bool) -> io::Result<GeoData> {
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("FIT file exceeds 16 MiB limit"));
    }
    // fitparser validates the header and CRC and resolves FIT field scales.
    // Decoding is one bounded call; cancellation is checked around and after it.
    if cancelled() {
        return Err(interrupted());
    }
    let records = fitparser::from_bytes(&bytes).map_err(|error| invalid(error.to_string()))?;
    if cancelled() {
        return Err(interrupted());
    }
    let mut data = GeoData {
        format: "FIT",
        ..GeoData::default()
    };
    let mut session_distance = 0.0;
    let mut session_duration = 0.0;
    let mut session_ascent = 0.0;
    let mut session_descent = 0.0;
    let mut has_distance = false;
    let mut has_duration = false;
    let mut has_ascent = false;
    let mut has_descent = false;
    let mut previous = None;
    let mut computed_distance = 0.0;
    let mut last_time = None;
    let mut outline = Vec::new();
    for record in &records {
        if cancelled() {
            return Err(interrupted());
        }
        match record.kind() {
            MesgNum::FileId => {
                data.kind = string(record, "type");
            }
            MesgNum::Course => {
                data.name = string(record, "name");
                if data.sport.is_none() {
                    data.sport = string(record, "sport");
                }
            }
            MesgNum::Workout => {
                if data.name.is_none() {
                    data.name = string(record, "wkt_name");
                }
                if data.sport.is_none() {
                    data.sport = string(record, "sport");
                }
            }
            MesgNum::Session => {
                data.tracks += 1;
                if data.start.is_none() {
                    data.start = timestamp(record, "start_time").map(|(_, text)| text);
                }
                if data.sport.is_none() {
                    data.sport = string(record, "sport");
                }
                if let Some(value) = number(record, "total_distance") {
                    session_distance += value;
                    has_distance = true;
                }
                if let Some(value) = number(record, "total_elapsed_time") {
                    session_duration += value;
                    has_duration = true;
                }
                if let Some(value) = number(record, "total_ascent") {
                    session_ascent += value;
                    has_ascent = true;
                }
                if let Some(value) = number(record, "total_descent") {
                    session_descent += value;
                    has_descent = true;
                }
            }
            MesgNum::Lap => data.laps += 1,
            MesgNum::Record => {
                let time = timestamp(record, "timestamp");
                if data.start.is_none() {
                    data.start = time.as_ref().map(|(_, text)| text.clone());
                }
                let latitude = number(record, "position_lat");
                let longitude = number(record, "position_long");
                let Some(location) = latitude.zip(longitude).and_then(|(lat, lon)| {
                    point(lat * 180.0 / 2_147_483_648.0, lon * 180.0 / 2_147_483_648.0)
                }) else {
                    if !outline.is_empty() {
                        data.segments.push(std::mem::take(&mut outline));
                    }
                    previous = None;
                    last_time = None;
                    continue;
                };
                if data.points == MAX_POINTS {
                    data.truncated = true;
                    continue;
                }
                let next_time = time.as_ref().map(|(second, _)| *second);
                if let (Some(before), Some(after)) = (last_time, next_time)
                    && (after < before || after - before > 300)
                {
                    if !outline.is_empty() {
                        data.segments.push(std::mem::take(&mut outline));
                    }
                    previous = None;
                }
                if let Some(before) = previous {
                    computed_distance += distance(before, location);
                }
                previous = Some(location);
                last_time = next_time;
                outline.push(location);
                data.points += 1;
            }
            _ => {}
        }
    }
    if !outline.is_empty() {
        data.segments.push(outline);
    }
    if has_distance {
        data.distance_meters = Some(session_distance);
    } else if data.points > 1 {
        data.distance_meters = Some(computed_distance);
    }
    if has_duration {
        data.duration_seconds = Some(session_duration);
    }
    if has_ascent {
        data.ascent_meters = Some(session_ascent);
    }
    if has_descent {
        data.descent_meters = Some(session_descent);
    }
    if data
        .kind
        .as_deref()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("course"))
    {
        data.routes = 1;
    }
    Ok(data)
}

fn field<'a>(record: &'a FitDataRecord, name: &str) -> Option<&'a Value> {
    record
        .fields()
        .iter()
        .find(|field| field.name() == name)
        .map(|field| field.value())
}

fn number(record: &FitDataRecord, name: &str) -> Option<f64> {
    let number = match field(record, name)? {
        Value::Float64(value) => *value,
        Value::Float32(value) => f64::from(*value),
        Value::SInt64(value) => *value as f64,
        Value::SInt32(value) => f64::from(*value),
        Value::UInt64(value) => *value as f64,
        Value::UInt32(value) => f64::from(*value),
        Value::UInt16(value) => f64::from(*value),
        _ => return None,
    };
    number.is_finite().then_some(number)
}

fn string(record: &FitDataRecord, name: &str) -> Option<String> {
    match field(record, name)? {
        Value::String(value) => Some(value.chars().take(1024).collect()),
        _ => None,
    }
}

fn timestamp(record: &FitDataRecord, name: &str) -> Option<(i64, String)> {
    match field(record, name)? {
        Value::Timestamp(value) => Some((value.timestamp(), value.to_rfc3339())),
        _ => None,
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::read;

    #[test]
    fn rejects_invalid_fit() {
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("bad.fit");
        fs::write(&path, b"not a FIT file").expect("write FIT");
        assert!(read(&path, || false).is_err());
    }

    #[test]
    fn reads_activity_summary_and_track() {
        // Small activity fixture from fitparser's MIT-licensed test suite.
        let dir = TempDir::new().expect("temporary directory");
        let path = dir.path().join("activity.fit");
        fs::write(&path, include_bytes!("fixtures/activity.fit")).expect("write FIT");
        let data = read(&path, || false).expect("read FIT");
        assert!(data.tracks > 0);
        assert!(data.laps > 0);
        assert!(data.points > 0);
        assert!(data.distance_meters.is_some());
    }
}

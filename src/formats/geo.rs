//! Shared bounded route and activity model for GPX and FIT files.

use std::io;
use std::path::Path;

mod fit;
mod gpx;

/// Largest source file accepted by either reader.
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum coordinates retained for an outline.
pub const MAX_POINTS: usize = 100_000;

/// One WGS84 position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Latitude in degrees north.
    pub latitude: f64,
    /// Longitude in degrees east.
    pub longitude: f64,
}

/// Data common to routes and recorded activities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoData {
    /// Source format label.
    pub format: &'static str,
    /// FIT file type or GPX content type.
    pub kind: Option<String>,
    /// Human-readable activity or route name.
    pub name: Option<String>,
    /// Sport reported by a FIT session.
    pub sport: Option<String>,
    /// First recorded or session start time.
    pub start: Option<String>,
    /// Elapsed duration when recorded timestamps or a session total exist.
    pub duration_seconds: Option<f64>,
    /// Total distance from session data or consecutive GPS points.
    pub distance_meters: Option<f64>,
    /// Positive elevation gain when available.
    pub ascent_meters: Option<f64>,
    /// Positive elevation loss when available.
    pub descent_meters: Option<f64>,
    /// GPX track or FIT session count.
    pub tracks: usize,
    /// GPX route or FIT course count.
    pub routes: usize,
    /// GPX waypoint count.
    pub waypoints: usize,
    /// FIT lap count.
    pub laps: usize,
    /// Valid GPS coordinates retained for the outline.
    pub points: usize,
    /// Each segment stays separate so a gap never draws as a traveled line.
    pub segments: Vec<Vec<Point>>,
    /// More coordinates existed beyond the retained-point limit.
    pub truncated: bool,
}

/// Reads a local GPX or FIT file with a size bound and optional cancellation.
pub fn read(path: &Path, cancelled: impl Fn() -> bool) -> io::Result<GeoData> {
    if cancelled() {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    }
    let size = std::fs::metadata(path)?.len();
    if size > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPX/FIT file exceeds 16 MiB limit",
        ));
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("gpx") => gpx::read(path, cancelled),
        Some("fit") => fit::read(path, cancelled),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a GPX or FIT file",
        )),
    }
}

/// Distance in meters along a great-circle arc.
pub fn distance(a: Point, b: Point) -> f64 {
    let lat = (b.latitude - a.latitude).to_radians();
    let lon = (b.longitude - a.longitude).to_radians();
    let h = (lat / 2.0).sin().powi(2)
        + a.latitude.to_radians().cos() * b.latitude.to_radians().cos() * (lon / 2.0).sin().powi(2);
    12_742_000.0 * h.clamp(0.0, 1.0).sqrt().asin()
}

/// Validates finite WGS84 coordinates before retaining or measuring them.
pub fn point(latitude: f64, longitude: f64) -> Option<Point> {
    (latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude))
    .then_some(Point {
        latitude,
        longitude,
    })
}

#[cfg(test)]
mod tests {
    use super::{distance, point};

    #[test]
    fn validates_coordinates_and_distance() {
        assert!(point(91.0, 0.0).is_none());
        assert!(point(f64::NAN, 0.0).is_none());
        let a = point(0.0, 0.0).expect("valid origin");
        let b = point(0.0, 1.0).expect("valid longitude");
        assert!((distance(a, b) - 111_195.0).abs() < 100.0);
    }
}
